//! Read projection for a single gateway HTTP request. This helper neither
//! authenticates tokens nor records usage. Callers must recheck access against
//! the returned current account before signing; display never authorizes.
use crate::{
    DbConn, UserDAO, UserRole,
    error::{Resource, ShaideDBError},
};
use chrono::{DateTime, Utc};
use sqlx::FromRow;

pub struct CollectionContextSnapshot {
    pub user: UserDAO,
    pub collections: Vec<CollectionContextRow>,
}

#[derive(FromRow)]
pub struct CollectionContextRow {
    pub collection_id: i64,
    pub model_name: Option<String>,
    pub vector_size: Option<i64>,
    pub name: String,
    pub description: String,
    pub uploaded_documents: i64,
    pub searchable_documents: i64,
    pub document_names: Vec<String>,
}

#[derive(FromRow)]
struct AccountRow {
    id: i64,
    username: String,
    password_hash: String,
    role: String,
    expiry: DateTime<Utc>,
}

impl DbConn {
    /// One deferred read transaction pins identity, role, complete collection /
    /// model references and optional file catalog to the same SQLite snapshot.
    /// Reads at most 65 references: the extra row detects the 64-item contract
    /// overflow. A missing model is retained as NULL for explicit rejection.
    /// No lifecycle writes, document contents, provider URLs or caches.
    pub async fn collection_context_snapshot(
        &self,
        user_id: i64,
        catalog: bool,
    ) -> Result<CollectionContextSnapshot, ShaideDBError> {
        let mut tx = self.pool.begin().await?;
        let account = sqlx::query_as::<_, AccountRow>(
            "SELECT id, username, password_hash, role, expiry FROM users WHERE id = ?",
        )
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ShaideDBError::NotFound(Resource::User))?;
        let role = match account.role.as_str() {
            "user" => UserRole::User,
            "admin" => UserRole::Admin,
            _ => return Err(ShaideDBError::NotFound(Resource::User)),
        };
        let rows = sqlx::query("SELECT o.id, m.name, m.vector_size, o.organization_name, o.description
            FROM organizations o LEFT JOIN embedding_models m ON m.id = o.embedding_model_id
            WHERE ? OR EXISTS (SELECT 1 FROM organization_memberships om WHERE om.organization_id = o.id AND om.user_id = ?)
            ORDER BY o.id LIMIT 65")
            .bind(role == UserRole::Admin).bind(user_id).fetch_all(&mut *tx).await?;
        use sqlx::Row;
        let mut collections = Vec::with_capacity(rows.len());
        for row in rows {
            let id: i64 = row.try_get(0)?;
            let (uploaded, searchable, names) = if catalog && collections.len() < 64 {
                let counts: (i64, i64) = sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(status = 'processed'), 0) FROM files WHERE organization_id = ?")
                    .bind(id).fetch_one(&mut *tx).await?;
                let names = sqlx::query_scalar("SELECT file_name FROM files WHERE organization_id = ? AND status = 'processed' ORDER BY uploaded_at DESC, hash ASC LIMIT 3")
                    .bind(id).fetch_all(&mut *tx).await?;
                (counts.0, counts.1, names)
            } else {
                (0, 0, Vec::new())
            };
            collections.push(CollectionContextRow {
                collection_id: id,
                model_name: row.try_get(1)?,
                vector_size: row.try_get(2)?,
                name: row.try_get(3)?,
                description: row.try_get(4)?,
                uploaded_documents: uploaded,
                searchable_documents: searchable,
                document_names: names,
            });
        }
        tx.commit().await?;
        Ok(CollectionContextSnapshot {
            user: UserDAO {
                id: account.id,
                username: account.username,
                password_hash: account.password_hash,
                role,
                expiry: account.expiry,
            },
            collections,
        })
    }
}
