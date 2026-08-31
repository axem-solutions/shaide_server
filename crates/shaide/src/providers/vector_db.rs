use qdrant_client::{
    Qdrant, QdrantError,
    qdrant::{
        Condition, CreateCollectionBuilder, DeleteCollectionBuilder, DeletePointsBuilder, Distance,
        Filter, PointStruct, QueryPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
    },
};
use shaide_common::api::rag::SearchResult;
use tokio::sync::OnceCell;
use tonic::Code;
use tracing::{debug, error};

use crate::{config::get_environment_config, services::embedding::EmbeddedSnippets};

// TODO: this needs a bit of refinement
#[derive(Debug)]
pub enum RagCollectionError {
    ServiceUnavailble,
    CollectionNotFound,
    CollectionAlreadyExists,
    // catch all for unhandled errors
    Other(String),
}

impl From<QdrantError> for RagCollectionError {
    fn from(value: QdrantError) -> Self {
        match value {
            QdrantError::ResponseError { status } => match status.code() {
                Code::NotFound => RagCollectionError::CollectionNotFound,
                Code::Unavailable => RagCollectionError::ServiceUnavailble,
                Code::AlreadyExists => RagCollectionError::CollectionAlreadyExists,
                other => RagCollectionError::Other(format!(
                    "qdrant response error ({:?}): {}",
                    other,
                    status.message()
                )),
            },
            QdrantError::ResourceExhaustedError {
                status,
                retry_after_seconds,
            } => {
                error!(
                    retry_after_seconds = retry_after_seconds,
                    error = status.message(),
                    "Qdrant resource exhausted"
                );
                RagCollectionError::ServiceUnavailble
            }
            QdrantError::ConversionError(msg) => RagCollectionError::Other(msg.clone()),
            QdrantError::InvalidUri(err) => RagCollectionError::Other(err.to_string()),
            QdrantError::NoSnapshotFound(name) => {
                RagCollectionError::Other(format!("snapshot not found for collection {name}"))
            }
            QdrantError::Io(err) => RagCollectionError::Other(err.to_string()),
            QdrantError::Reqwest(err) => RagCollectionError::Other(err.to_string()),
            QdrantError::JsonToPayload(value) => RagCollectionError::Other(value.to_string()),
            QdrantError::PayloadDeserialization(err) => RagCollectionError::Other(err.to_string()),
        }
    }
}

#[derive(Clone)]
pub struct VectorDB {
    client: Qdrant,
}

impl VectorDB {
    pub fn new(client_url: &str) -> Self {
        let client = Qdrant::from_url(client_url)
            .skip_compatibility_check()
            .build()
            .unwrap();
        Self { client }
    }
}

pub struct RagCollectionParameters {
    collection_name: String,
    vector_size: u64,
}

impl RagCollectionParameters {
    pub fn new(collection_name: String, vector_size: u64) -> Self {
        Self {
            collection_name,
            vector_size,
        }
    }
}

pub fn translate_collection_name(collection_name: String) -> String {
    collection_name.replace("/", "_")
}

impl VectorDB {
    pub async fn query_code(
        &self,
        collection_name: String,
        embedding: Vec<f32>,
        limit: u64,
        min_score: f32,
        identifier: Option<String>,
        r#type: Option<String>,
    ) -> Result<Vec<SearchResult>, RagCollectionError> {
        let mut conditions = vec![];
        if let Some(identifier) = identifier {
            conditions.push(Condition::matches("identifier", identifier));
        }
        if let Some(r#type) = r#type {
            conditions.push(Condition::matches("type", r#type));
        }
        let collection_name = translate_collection_name(collection_name);
        let query_point = QueryPointsBuilder::new(collection_name)
            .query(embedding)
            .limit(limit)
            .filter(Filter::must(conditions))
            .score_threshold(min_score)
            .with_payload(true);
        let response = self.client.query(query_point).await?.result;
        let results = response
            .into_iter()
            .map(|point| SearchResult {
                score: point.score,
                file_path: point.get("file_path").as_str().unwrap().clone(),
                code_chunk: point.get("text").as_str().unwrap().clone(),
                start_line: point.get("start_line").as_integer().unwrap() as u64,
                end_line: point.get("end_line").as_integer().unwrap() as u64,
            })
            .collect();
        Ok(results)
    }

    pub async fn create_collection(
        &self,
        collection_parameters: RagCollectionParameters,
    ) -> Result<(), RagCollectionError> {
        let RagCollectionParameters {
            collection_name,
            vector_size,
        } = collection_parameters;
        let collection_name = translate_collection_name(collection_name);
        let request = CreateCollectionBuilder::new(collection_name)
            .vectors_config(VectorParamsBuilder::new(vector_size, Distance::Cosine));
        self.client.create_collection(request).await?;
        Ok(())
    }

    pub async fn delete_collection(&self, collection_name: &str) -> Result<(), RagCollectionError> {
        let collection_name = translate_collection_name(collection_name.to_owned());
        self.client
            .delete_collection(DeleteCollectionBuilder::new(collection_name))
            .await?;
        Ok(())
    }

    pub async fn upsert_embedded_snippets(
        &self,
        collection_name: String,
        embedded_snippets: EmbeddedSnippets,
    ) -> Result<(), RagCollectionError> {
        let collection_name = translate_collection_name(collection_name);
        if !self.client.collection_exists(&collection_name).await? {
            self.create_collection(RagCollectionParameters {
                collection_name: collection_name.clone(),
                vector_size: embedded_snippets.vector_size,
            })
            .await?;
        }
        let points: Vec<PointStruct> = embedded_snippets
            .snippets
            .into_iter()
            .map(|e| e.into_point_struct())
            .collect();
        let upsert_points_builder = UpsertPointsBuilder::new(collection_name, points);
        self.client.upsert_points(upsert_points_builder).await?;
        Ok(())
    }

    pub async fn delete_embedded_code_snippets(
        &self,
        collection_name: String,
        file_path: &[String],
    ) -> Result<(), RagCollectionError> {
        let collection_name = translate_collection_name(collection_name);
        let conditions: Vec<_> = file_path
            .iter()
            .map(|fp| Condition::matches("file_path", fp.to_owned()))
            .collect();
        let filter = Filter::any(conditions);
        let delete_point_request = DeletePointsBuilder::new(collection_name)
            .points(filter)
            .wait(true);
        self.client.delete_points(delete_point_request).await?;
        Ok(())
    }
}

static VECTOR_DB: OnceCell<VectorDB> = OnceCell::const_new();

pub async fn get_vector_db() -> &'static VectorDB {
    (VECTOR_DB
        .get_or_init(async || {
            let rag_client_url = get_environment_config().vector_db_url.as_ref();
            debug!(
                vector_db_url = rag_client_url,
                "Initializing vector DB client"
            );
            VectorDB::new(rag_client_url)
        })
        .await) as _
}
