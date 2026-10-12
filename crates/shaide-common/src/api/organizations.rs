//! Organization (knowledge center collection) request and response bodies.
//! Field names match what the control panel sends and reads.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Name of the object storage bucket and the vector collection that hold the
/// organization's files.
pub fn collection_name(organization_id: i64) -> String {
    format!("collection-{organization_id}")
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq, Eq)]
pub struct CreateOrganizationRequest {
    pub organization_name: String,
    pub organization_description: String,
    /// Initial member user IDs.
    pub users: Vec<i64>,
    pub can_users_upload: bool,
    pub embedding_model_id: i64,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq, Eq)]
pub struct CreateOrganizationResponse {
    pub id: i64,
}

/// Omitted fields keep their current value.
#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq, Eq)]
pub struct UpdateOrganizationRequest {
    pub organization_id: i64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub can_users_upload: Option<bool>,
    #[serde(default)]
    pub embedding_model_id: Option<i64>,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq, Eq)]
pub struct DeleteOrganizationRequest {
    pub id: i64,
}

/// Adds or removes members, depending on the method.
#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq, Eq)]
pub struct OrganizationMembershipRequest {
    pub organization_id: i64,
    pub user_ids: Vec<i64>,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq, Eq)]
pub struct DeleteOrganizationFileRequest {
    pub organization_id: i64,
    pub file_hash: String,
}

/// Lifecycle of an uploaded file, reported with the stored names. The control
/// panel labels `processed` as "Ready"; it has no `ready` or `failed` value
/// from the server.
#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationFileStatus {
    Uploading,
    UploadingFailed,
    Processing,
    ProcessingFailed,
    Processed,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq, Eq)]
pub struct OrganizationFileResponse {
    pub hash: String,
    pub name: String,
    pub size: Option<i64>,
    pub mime_type: String,
    pub status: OrganizationFileStatus,
    #[schema(value_type = String, format = DateTime)]
    pub uploaded_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq, Eq)]
pub struct OrganizationResponse {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub can_users_upload: bool,
    /// Member user IDs for admins; `null` for users.
    pub users: Option<Vec<i64>>,
    pub files: Vec<OrganizationFileResponse>,
    pub embedding_model_id: i64,
    pub collection_name: String,
    pub bucket_name: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime<Utc>,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq, Eq)]
pub struct OrganizationsResponse {
    pub organization: Vec<OrganizationResponse>,
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::*;

    // Request bodies exactly as the control panel sends them
    // (`src/services/{create,modify,delete}-collection*.ts`).
    #[test]
    fn control_panel_request_bodies_are_accepted() {
        let create: CreateOrganizationRequest = serde_json::from_value(json!({
            "organization_name": "Research",
            "organization_description": "Shared papers",
            "users": [2, 3],
            "can_users_upload": true,
            "embedding_model_id": 1
        }))
        .unwrap();
        assert_eq!(
            create,
            CreateOrganizationRequest {
                organization_name: "Research".to_owned(),
                organization_description: "Shared papers".to_owned(),
                users: vec![2, 3],
                can_users_upload: true,
                embedding_model_id: 1,
            }
        );

        let update: UpdateOrganizationRequest = serde_json::from_value(json!({
            "organization_id": 4,
            "name": "Renamed",
            "description": "",
            "can_users_upload": false
        }))
        .unwrap();
        assert_eq!(
            update,
            UpdateOrganizationRequest {
                organization_id: 4,
                name: Some("Renamed".to_owned()),
                description: Some(String::new()),
                can_users_upload: Some(false),
                embedding_model_id: None,
            }
        );

        let membership: OrganizationMembershipRequest =
            serde_json::from_value(json!({"organization_id": 4, "user_ids": [2]})).unwrap();
        assert_eq!(membership.organization_id, 4);
        assert_eq!(membership.user_ids, vec![2]);

        let delete: DeleteOrganizationRequest = serde_json::from_value(json!({"id": 4})).unwrap();
        assert_eq!(delete.id, 4);

        let delete_file: DeleteOrganizationFileRequest =
            serde_json::from_value(json!({"organization_id": 4, "file_hash": "abc"})).unwrap();
        assert_eq!(delete_file.organization_id, 4);
        assert_eq!(delete_file.file_hash, "abc");
    }

    #[test]
    fn organization_list_has_the_control_panel_shape() {
        let timestamp = Utc.with_ymd_and_hms(2026, 10, 10, 12, 0, 0).unwrap();
        let response = OrganizationsResponse {
            organization: vec![OrganizationResponse {
                id: 4,
                name: "Research".to_owned(),
                description: "Shared papers".to_owned(),
                can_users_upload: true,
                users: None,
                files: vec![OrganizationFileResponse {
                    hash: "abc".to_owned(),
                    name: "paper.pdf".to_owned(),
                    size: None,
                    mime_type: "application/pdf".to_owned(),
                    status: OrganizationFileStatus::ProcessingFailed,
                    uploaded_at: timestamp,
                }],
                embedding_model_id: 1,
                collection_name: collection_name(4),
                bucket_name: collection_name(4),
                created_at: timestamp,
                updated_at: timestamp,
            }],
        };

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "organization": [{
                    "id": 4,
                    "name": "Research",
                    "description": "Shared papers",
                    "can_users_upload": true,
                    "users": null,
                    "files": [{
                        "hash": "abc",
                        "name": "paper.pdf",
                        "size": null,
                        "mime_type": "application/pdf",
                        "status": "processing_failed",
                        "uploaded_at": "2026-10-10T12:00:00Z"
                    }],
                    "embedding_model_id": 1,
                    "collection_name": "collection-4",
                    "bucket_name": "collection-4",
                    "created_at": "2026-10-10T12:00:00Z",
                    "updated_at": "2026-10-10T12:00:00Z"
                }]
            })
        );
        assert_eq!(
            serde_json::to_value(CreateOrganizationResponse { id: 4 }).unwrap(),
            json!({"id": 4})
        );
    }

    // The server reports the stored lifecycle names. UI-only labels such as
    // "ready" or "failed" are never produced or accepted.
    #[test]
    fn file_statuses_use_the_stored_lifecycle_names() {
        let statuses = [
            (OrganizationFileStatus::Uploading, "uploading"),
            (OrganizationFileStatus::UploadingFailed, "uploading_failed"),
            (OrganizationFileStatus::Processing, "processing"),
            (
                OrganizationFileStatus::ProcessingFailed,
                "processing_failed",
            ),
            (OrganizationFileStatus::Processed, "processed"),
        ];
        for (status, name) in statuses {
            assert_eq!(serde_json::to_value(status).unwrap(), json!(name));
            assert_eq!(
                serde_json::from_value::<OrganizationFileStatus>(json!(name)).unwrap(),
                status
            );
        }
        for ui_label in ["ready", "failed", "reprocessing"] {
            assert!(serde_json::from_value::<OrganizationFileStatus>(json!(ui_label)).is_err());
        }
    }
}
