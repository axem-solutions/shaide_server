use std::fmt::{self, Display, Formatter};

use chrono::{DateTime, Utc};
use shaide_common::api::organizations::OrganizationFileStatus;
use sqlx::{Sqlite, Transaction, query_as, query_scalar};

use crate::{
    DbConn,
    error::{Resource, ShaideDBError},
    organizations::begin_write,
};

/// Lifecycle of an uploaded file. Stored and serialized with the snake_case
/// names of the earlier shaide schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum FileStatus {
    /// Metadata exists; the object upload has not been confirmed yet.
    Uploading,
    UploadingFailed,
    /// Claimed by exactly one ingestion run.
    Processing,
    ProcessingFailed,
    /// Ingested and searchable.
    Processed,
}

impl FileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uploading => "uploading",
            Self::UploadingFailed => "uploading_failed",
            Self::Processing => "processing",
            Self::ProcessingFailed => "processing_failed",
            Self::Processed => "processed",
        }
    }
}

impl From<FileStatus> for OrganizationFileStatus {
    fn from(status: FileStatus) -> Self {
        match status {
            FileStatus::Uploading => Self::Uploading,
            FileStatus::UploadingFailed => Self::UploadingFailed,
            FileStatus::Processing => Self::Processing,
            FileStatus::ProcessingFailed => Self::ProcessingFailed,
            FileStatus::Processed => Self::Processed,
        }
    }
}

impl Display for FileStatus {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDao {
    pub hash: String,
    pub organization_id: i64,
    pub file_name: String,
    pub mime_type: String,
    pub size: Option<i64>,
    pub pages: Option<i64>,
    pub status: FileStatus,
    /// Machine-readable failure reason; only set on failed statuses.
    pub error_code: Option<String>,
    pub uploaded_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct InsertFileDao {
    pub hash: String,
    pub organization_id: i64,
    pub file_name: String,
    pub mime_type: String,
}

/// Result of trying to start ingestion of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileClaim {
    /// The file moved from `uploading` to `processing`; the caller owns the run.
    Claimed(FileDao),
    /// The file was not `uploading`, e.g. a duplicate event for a file that is
    /// already being or has been processed. Nothing was changed.
    NotClaimable(FileStatus),
}

async fn current_status(
    transaction: &mut Transaction<'static, Sqlite>,
    organization_id: i64,
    hash: &str,
) -> Result<FileStatus, ShaideDBError> {
    query_scalar!(
        r#"SELECT status as "status: FileStatus" FROM files WHERE organization_id = ? AND hash = ?"#,
        organization_id,
        hash
    )
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(ShaideDBError::NotFound(Resource::File))
}

// Nullable columns are marked `"column?"` in every query: sqlx infers columns
// assigned by `UPDATE ... RETURNING` as NOT NULL and would decode NULL as 0 or "".

/// db read/write operations for `files`. Every file is addressed by its
/// organization and hash, so a caller scoped to one organization cannot read
/// or change another organization's files.
impl DbConn {
    /// Records a new file as `uploading`. A hash is stored once across all
    /// organizations.
    pub async fn insert_file(&self, file: InsertFileDao) -> Result<FileDao, ShaideDBError> {
        let mut transaction = begin_write(self).await?;
        let organization_exists = query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM organizations WHERE id = ?) as "exists!: bool""#,
            file.organization_id
        )
        .fetch_one(&mut *transaction)
        .await?;
        if !organization_exists {
            return Err(ShaideDBError::NotFound(Resource::Organization));
        }
        let inserted = query_as!(
            FileDao,
            r#"INSERT INTO files (hash, organization_id, file_name, mime_type)
            VALUES (?, ?, ?, ?)
            ON CONFLICT (hash) DO NOTHING
            RETURNING
                hash,
                organization_id,
                file_name,
                mime_type,
                size as "size?",
                pages as "pages?",
                status as "status: FileStatus",
                error_code as "error_code?",
                uploaded_at as "uploaded_at: DateTime<Utc>",
                updated_at as "updated_at: DateTime<Utc>""#,
            file.hash,
            file.organization_id,
            file.file_name,
            file.mime_type
        )
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| ShaideDBError::InsertFailedOnConflict("file".to_owned()))?;
        transaction.commit().await?;
        Ok(inserted)
    }

    pub async fn get_file(
        &self,
        organization_id: i64,
        hash: &str,
    ) -> Result<FileDao, ShaideDBError> {
        query_as!(
            FileDao,
            r#"SELECT
                hash,
                organization_id,
                file_name,
                mime_type,
                size as "size?",
                pages as "pages?",
                status as "status: FileStatus",
                error_code as "error_code?",
                uploaded_at as "uploaded_at: DateTime<Utc>",
                updated_at as "updated_at: DateTime<Utc>"
            FROM files WHERE organization_id = ? AND hash = ?"#,
            organization_id,
            hash
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(ShaideDBError::NotFound(Resource::File))
    }

    /// The organization's files, oldest upload first.
    pub async fn list_files(&self, organization_id: i64) -> Result<Vec<FileDao>, ShaideDBError> {
        let mut transaction = self.pool.begin().await?;
        let organization_exists = query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM organizations WHERE id = ?) as "exists!: bool""#,
            organization_id
        )
        .fetch_one(&mut *transaction)
        .await?;
        if !organization_exists {
            return Err(ShaideDBError::NotFound(Resource::Organization));
        }
        let files = query_as!(
            FileDao,
            r#"SELECT
                hash,
                organization_id,
                file_name,
                mime_type,
                size as "size?",
                pages as "pages?",
                status as "status: FileStatus",
                error_code as "error_code?",
                uploaded_at as "uploaded_at: DateTime<Utc>",
                updated_at as "updated_at: DateTime<Utc>"
            FROM files WHERE organization_id = ?
            ORDER BY uploaded_at, hash"#,
            organization_id
        )
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(files)
    }

    /// Deletes the file metadata and returns it. The stored object and its
    /// vectors are not touched here.
    pub async fn delete_file(
        &self,
        organization_id: i64,
        hash: &str,
    ) -> Result<FileDao, ShaideDBError> {
        query_as!(
            FileDao,
            r#"DELETE FROM files WHERE organization_id = ? AND hash = ?
            RETURNING
                hash,
                organization_id,
                file_name,
                mime_type,
                size as "size?",
                pages as "pages?",
                status as "status: FileStatus",
                error_code as "error_code?",
                uploaded_at as "uploaded_at: DateTime<Utc>",
                updated_at as "updated_at: DateTime<Utc>""#,
            organization_id,
            hash
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(ShaideDBError::NotFound(Resource::File))
    }

    /// Atomically moves an `uploading` file to `processing`. Of concurrent
    /// callers for the same file, exactly one gets `Claimed`.
    pub async fn claim_file_for_processing(
        &self,
        organization_id: i64,
        hash: &str,
    ) -> Result<FileClaim, ShaideDBError> {
        let mut transaction = begin_write(self).await?;
        let claimed = query_as!(
            FileDao,
            r#"UPDATE files SET status = 'processing', updated_at = DATETIME('now')
            WHERE organization_id = ? AND hash = ? AND status = 'uploading'
            RETURNING
                hash,
                organization_id,
                file_name,
                mime_type,
                size as "size?",
                pages as "pages?",
                status as "status: FileStatus",
                error_code as "error_code?",
                uploaded_at as "uploaded_at: DateTime<Utc>",
                updated_at as "updated_at: DateTime<Utc>""#,
            organization_id,
            hash
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let claim = match claimed {
            Some(file) => FileClaim::Claimed(file),
            None => FileClaim::NotClaimable(
                current_status(&mut transaction, organization_id, hash).await?,
            ),
        };
        transaction.commit().await?;
        Ok(claim)
    }

    /// Completes a `processing` file with the details found during ingestion.
    pub async fn mark_file_processed(
        &self,
        organization_id: i64,
        hash: &str,
        size: Option<i64>,
        pages: Option<i64>,
    ) -> Result<FileDao, ShaideDBError> {
        let mut transaction = begin_write(self).await?;
        let processed = query_as!(
            FileDao,
            r#"UPDATE files
            SET status = 'processed', size = ?, pages = ?, error_code = NULL,
                updated_at = DATETIME('now')
            WHERE organization_id = ? AND hash = ? AND status = 'processing'
            RETURNING
                hash,
                organization_id,
                file_name,
                mime_type,
                size as "size?",
                pages as "pages?",
                status as "status: FileStatus",
                error_code as "error_code?",
                uploaded_at as "uploaded_at: DateTime<Utc>",
                updated_at as "updated_at: DateTime<Utc>""#,
            size,
            pages,
            organization_id,
            hash
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(processed) = processed else {
            let current = current_status(&mut transaction, organization_id, hash).await?;
            return Err(ShaideDBError::FileStatusConflict { current });
        };
        transaction.commit().await?;
        Ok(processed)
    }

    /// Fails an `uploading` file as `uploading_failed` or a `processing` file
    /// as `processing_failed`, recording `error_code`.
    pub async fn mark_file_failed(
        &self,
        organization_id: i64,
        hash: &str,
        error_code: &str,
    ) -> Result<FileDao, ShaideDBError> {
        let mut transaction = begin_write(self).await?;
        let failed = query_as!(
            FileDao,
            r#"UPDATE files
            SET status = CASE status
                    WHEN 'uploading' THEN 'uploading_failed'
                    ELSE 'processing_failed'
                END,
                error_code = ?,
                updated_at = DATETIME('now')
            WHERE organization_id = ? AND hash = ? AND status IN ('uploading', 'processing')
            RETURNING
                hash,
                organization_id,
                file_name,
                mime_type,
                size as "size?",
                pages as "pages?",
                status as "status: FileStatus",
                error_code as "error_code?",
                uploaded_at as "uploaded_at: DateTime<Utc>",
                updated_at as "updated_at: DateTime<Utc>""#,
            error_code,
            organization_id,
            hash
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(failed) = failed else {
            let current = current_status(&mut transaction, organization_id, hash).await?;
            return Err(ShaideDBError::FileStatusConflict { current });
        };
        transaction.commit().await?;
        Ok(failed)
    }
}
