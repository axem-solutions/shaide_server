use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use sqlx::{Sqlite, SqliteExecutor, Transaction, query, query_as, query_scalar};

use crate::{
    DbConn,
    error::{Resource, ShaideDBError},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationDao {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub can_users_upload: bool,
    pub embedding_model_id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct InsertOrganizationDao {
    pub name: String,
    pub description: String,
    pub can_users_upload: bool,
    pub embedding_model_id: i64,
    /// Initial members. Repeated IDs are stored once.
    pub user_ids: Vec<i64>,
}

/// Fields to change; `None` keeps the stored value.
#[derive(Debug, Clone, Default)]
pub struct UpdateOrganizationDao {
    pub name: Option<String>,
    pub description: Option<String>,
    pub can_users_upload: Option<bool>,
    pub embedding_model_id: Option<i64>,
}

/// The organization before and after an update, read in the same transaction,
/// so callers can tell whether e.g. the embedding model changed.
#[derive(Debug, Clone)]
pub struct UpdatedOrganizationDao {
    pub previous: OrganizationDao,
    pub current: OrganizationDao,
}

/// A deleted organization and the hashes of its files, whose metadata was
/// deleted with it. Stored objects and vectors are not touched here.
#[derive(Debug, Clone)]
pub struct DeletedOrganizationDao {
    pub organization: OrganizationDao,
    pub file_hashes: Vec<String>,
}

/// Starts a write transaction that takes the database write lock up front, so
/// a read followed by a write in the same transaction cannot fail with
/// `SQLITE_BUSY` when another writer committed in between.
pub(crate) async fn begin_write(
    db: &DbConn,
) -> Result<Transaction<'static, Sqlite>, ShaideDBError> {
    Ok(db.pool.begin_with("BEGIN IMMEDIATE").await?)
}

async fn select_organization<'e>(
    executor: impl SqliteExecutor<'e>,
    organization_id: i64,
) -> Result<OrganizationDao, ShaideDBError> {
    query_as!(
        OrganizationDao,
        r#"SELECT
            id as "id!",
            organization_name as name,
            description,
            can_users_upload,
            embedding_model_id,
            created_at as "created_at: DateTime<Utc>",
            updated_at as "updated_at: DateTime<Utc>"
        FROM organizations WHERE id = ?"#,
        organization_id
    )
    .fetch_optional(executor)
    .await?
    .ok_or(ShaideDBError::NotFound(Resource::Organization))
}

pub(crate) async fn ensure_embedding_model_exists(
    transaction: &mut Transaction<'static, Sqlite>,
    embedding_model_id: i64,
) -> Result<(), ShaideDBError> {
    let exists = query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM embedding_models WHERE id = ?) as "exists!: bool""#,
        embedding_model_id
    )
    .fetch_one(&mut **transaction)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(ShaideDBError::MissingReference(Resource::EmbeddingModel))
    }
}

/// Adds the users to the organization, skipping existing memberships. Fails
/// without writing anything else if a user does not exist. Returns the number
/// of new memberships.
pub(crate) async fn insert_memberships(
    transaction: &mut Transaction<'static, Sqlite>,
    organization_id: i64,
    user_ids: &[i64],
) -> Result<u64, ShaideDBError> {
    let mut added = 0;
    for user_id in user_ids.iter().copied().collect::<BTreeSet<_>>() {
        let user_exists = query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM users WHERE id = ?) as "exists!: bool""#,
            user_id
        )
        .fetch_one(&mut **transaction)
        .await?;
        if !user_exists {
            return Err(ShaideDBError::MissingReference(Resource::User));
        }
        added += query!(
            "INSERT INTO organization_memberships (user_id, organization_id) VALUES (?, ?)
             ON CONFLICT (user_id, organization_id) DO NOTHING",
            user_id,
            organization_id
        )
        .execute(&mut **transaction)
        .await?
        .rows_affected();
    }
    Ok(added)
}

/// db read/write operations for `organizations` and `organization_memberships`
impl DbConn {
    /// Creates the organization and its memberships atomically.
    pub async fn create_organization(
        &self,
        organization: InsertOrganizationDao,
    ) -> Result<i64, ShaideDBError> {
        let mut transaction = begin_write(self).await?;
        ensure_embedding_model_exists(&mut transaction, organization.embedding_model_id).await?;
        let id = query!(
            "INSERT INTO organizations (organization_name, description, can_users_upload, embedding_model_id)
             VALUES (?, ?, ?, ?)",
            organization.name,
            organization.description,
            organization.can_users_upload,
            organization.embedding_model_id
        )
        .execute(&mut *transaction)
        .await?
        .last_insert_rowid();
        insert_memberships(&mut transaction, id, &organization.user_ids).await?;
        transaction.commit().await?;
        Ok(id)
    }

    pub async fn get_organization(
        &self,
        organization_id: i64,
    ) -> Result<OrganizationDao, ShaideDBError> {
        select_organization(&self.pool, organization_id).await
    }

    pub async fn update_organization(
        &self,
        organization_id: i64,
        update: UpdateOrganizationDao,
    ) -> Result<UpdatedOrganizationDao, ShaideDBError> {
        let mut transaction = begin_write(self).await?;
        let previous = select_organization(&mut *transaction, organization_id).await?;
        if let Some(embedding_model_id) = update.embedding_model_id {
            ensure_embedding_model_exists(&mut transaction, embedding_model_id).await?;
        }
        query!(
            "UPDATE organizations SET
                organization_name = COALESCE(?, organization_name),
                description = COALESCE(?, description),
                can_users_upload = COALESCE(?, can_users_upload),
                embedding_model_id = COALESCE(?, embedding_model_id),
                updated_at = DATETIME('now')
            WHERE id = ?",
            update.name,
            update.description,
            update.can_users_upload,
            update.embedding_model_id,
            organization_id
        )
        .execute(&mut *transaction)
        .await?;
        let current = select_organization(&mut *transaction, organization_id).await?;
        transaction.commit().await?;
        Ok(UpdatedOrganizationDao { previous, current })
    }

    /// Deletes the organization with its memberships and file metadata.
    pub async fn delete_organization(
        &self,
        organization_id: i64,
    ) -> Result<DeletedOrganizationDao, ShaideDBError> {
        let mut transaction = begin_write(self).await?;
        let organization = select_organization(&mut *transaction, organization_id).await?;
        let file_hashes = query_scalar!(
            "SELECT hash FROM files WHERE organization_id = ? ORDER BY hash",
            organization_id
        )
        .fetch_all(&mut *transaction)
        .await?;
        query!("DELETE FROM organizations WHERE id = ?", organization_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(DeletedOrganizationDao {
            organization,
            file_hashes,
        })
    }

    /// Every organization, by ascending ID. For admin views.
    pub async fn list_organizations(&self) -> Result<Vec<OrganizationDao>, ShaideDBError> {
        Ok(query_as!(
            OrganizationDao,
            r#"SELECT
                id as "id!",
                organization_name as name,
                description,
                can_users_upload,
                embedding_model_id,
                created_at as "created_at: DateTime<Utc>",
                updated_at as "updated_at: DateTime<Utc>"
            FROM organizations ORDER BY id"#
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// The organizations the user is a member of, by ascending ID.
    pub async fn list_organizations_for_user(
        &self,
        user_id: i64,
    ) -> Result<Vec<OrganizationDao>, ShaideDBError> {
        Ok(query_as!(
            OrganizationDao,
            r#"SELECT
                organizations.id as "id!",
                organizations.organization_name as name,
                organizations.description,
                organizations.can_users_upload,
                organizations.embedding_model_id,
                organizations.created_at as "created_at: DateTime<Utc>",
                organizations.updated_at as "updated_at: DateTime<Utc>"
            FROM organizations
            JOIN organization_memberships
                ON organization_memberships.organization_id = organizations.id
            WHERE organization_memberships.user_id = ?
            ORDER BY organizations.id"#,
            user_id
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn is_organization_member(
        &self,
        organization_id: i64,
        user_id: i64,
    ) -> Result<bool, ShaideDBError> {
        Ok(query_scalar!(
            r#"SELECT EXISTS(
                SELECT 1 FROM organization_memberships WHERE organization_id = ? AND user_id = ?
            ) as "member!: bool""#,
            organization_id,
            user_id
        )
        .fetch_one(&self.pool)
        .await?)
    }

    /// Adds the users to the organization. Existing members are skipped. If a
    /// user does not exist, nothing is added. Returns the number of new members.
    pub async fn add_organization_members(
        &self,
        organization_id: i64,
        user_ids: &[i64],
    ) -> Result<u64, ShaideDBError> {
        let mut transaction = begin_write(self).await?;
        select_organization(&mut *transaction, organization_id).await?;
        let added = insert_memberships(&mut transaction, organization_id, user_ids).await?;
        transaction.commit().await?;
        Ok(added)
    }

    /// Removes the users from the organization. Users that are not members are
    /// skipped. Returns the number of removed members.
    pub async fn remove_organization_members(
        &self,
        organization_id: i64,
        user_ids: &[i64],
    ) -> Result<u64, ShaideDBError> {
        let mut transaction = begin_write(self).await?;
        select_organization(&mut *transaction, organization_id).await?;
        let mut removed = 0;
        for user_id in user_ids.iter().copied().collect::<BTreeSet<_>>() {
            removed += query!(
                "DELETE FROM organization_memberships WHERE organization_id = ? AND user_id = ?",
                organization_id,
                user_id
            )
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        }
        transaction.commit().await?;
        Ok(removed)
    }

    /// Member user IDs in ascending order.
    pub async fn list_organization_member_ids(
        &self,
        organization_id: i64,
    ) -> Result<Vec<i64>, ShaideDBError> {
        Ok(query_scalar!(
            "SELECT user_id FROM organization_memberships WHERE organization_id = ? ORDER BY user_id",
            organization_id
        )
        .fetch_all(&self.pool)
        .await?)
    }
}
