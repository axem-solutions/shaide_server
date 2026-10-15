//! Durable fences for public orchestration. Transactions end before KC calls.
//! Abandoned or uncertain operations are never reclaimed by elapsed time.
use crate::{
    DbConn,
    error::{Resource, ShaideDBError},
    files::InsertFileDao,
    organizations::{
        InsertOrganizationDao, begin_write, ensure_embedding_model_exists, insert_memberships,
    },
};
use sqlx::{Row, Sqlite, Transaction};

fn conflict() -> ShaideDBError {
    ShaideDBError::InsertFailedOnConflict("operation requires reconciliation".into())
}

async fn collection_available(
    tx: &mut Transaction<'static, Sqlite>,
    id: i64,
) -> Result<(), ShaideDBError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM organizations WHERE id=?)")
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
    if !exists {
        return Err(ShaideDBError::NotFound(Resource::Organization));
    }
    let locked: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM collection_operations WHERE organization_id=?)",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?;
    if locked {
        return Err(conflict());
    }
    Ok(())
}

async fn upload_owner_allowed(
    tx: &mut Transaction<'static, Sqlite>,
    id: i64,
    hash: &str,
) -> Result<(), ShaideDBError> {
    let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM file_operations f JOIN users u ON u.id=f.owner_id JOIN organizations o ON o.id=f.organization_id WHERE f.organization_id=? AND f.file_hash=? AND (u.role='admin' OR (unixepoch(u.expiry)>unixepoch() AND o.can_users_upload AND EXISTS(SELECT 1 FROM organization_memberships m WHERE m.organization_id=o.id AND m.user_id=u.id))))")
        .bind(id).bind(hash).fetch_one(&mut **tx).await?;
    if !allowed {
        return Err(conflict());
    }
    Ok(())
}

impl DbConn {
    /// Restrict access under the same write lock as upload/ingestion admission.
    /// Existing collection, file and recovery fences are deliberately untouched.
    pub async fn disable_organization_uploads(&self, id: i64) -> Result<(), ShaideDBError> {
        let mut tx = begin_write(self).await?;
        let n = sqlx::query("UPDATE organizations SET can_users_upload=false, updated_at=DATETIME('now') WHERE id=?")
            .bind(id).execute(&mut *tx).await?.rows_affected();
        if n != 1 {
            return Err(ShaideDBError::NotFound(Resource::Organization));
        }
        tx.commit().await?;
        Ok(())
    }

    /// Bind the exact KC-issued query without persisting any signature/credential text.
    pub async fn bind_reservation_query(
        &self,
        id: i64,
        hash: &str,
        digest: &str,
    ) -> Result<(), ShaideDBError> {
        let n=sqlx::query("UPDATE file_operations SET state='reserved',query_digest=? WHERE organization_id=? AND file_hash=? AND state='presigning'")
            .bind(digest).bind(id).bind(hash).execute(&self.pool).await?.rows_affected();
        if n != 1 {
            return Err(conflict());
        }
        Ok(())
    }
    pub async fn reservation_query_matches(
        &self,
        id: i64,
        hash: &str,
        digest: &str,
    ) -> Result<bool, ShaideDBError> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM file_operations WHERE organization_id=? AND file_hash=? AND query_digest=?)")
            .bind(id).bind(hash).bind(digest).fetch_one(&self.pool).await?)
    }

    pub async fn create_reserved_organization(
        &self,
        org: InsertOrganizationDao,
        token: &str,
    ) -> Result<i64, ShaideDBError> {
        let mut tx = begin_write(self).await?;
        ensure_embedding_model_exists(&mut tx, org.embedding_model_id).await?;
        let id=sqlx::query("INSERT INTO organizations(organization_name,description,can_users_upload,embedding_model_id) VALUES(?,?,?,?)")
            .bind(org.name).bind(org.description).bind(org.can_users_upload).bind(org.embedding_model_id).execute(&mut *tx).await?.last_insert_rowid();
        insert_memberships(&mut tx, id, &org.user_ids).await?;
        sqlx::query("INSERT INTO collection_operations(organization_id,token,kind,payload) VALUES(?,?,'create','{}')").bind(id).bind(token).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(id)
    }
    pub async fn claim_collection_operation(
        &self,
        id: i64,
        token: &str,
        kind: &str,
        payload: &str,
    ) -> Result<(), ShaideDBError> {
        let mut tx = begin_write(self).await?;
        collection_available(&mut tx, id).await?;
        let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM file_operations WHERE organization_id=?) OR EXISTS(SELECT 1 FROM files WHERE organization_id=? AND status IN ('uploading','processing'))")
            .bind(id).bind(id).fetch_one(&mut *tx).await?;
        if busy {
            return Err(conflict());
        }
        sqlx::query(
            "INSERT INTO collection_operations(organization_id,token,kind,payload) VALUES(?,?,?,?)",
        )
        .bind(id)
        .bind(token)
        .bind(kind)
        .bind(payload)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn finish_collection_operation(
        &self,
        id: i64,
        token: &str,
        delete: bool,
    ) -> Result<(), ShaideDBError> {
        let mut tx = begin_write(self).await?;
        let n=sqlx::query("DELETE FROM collection_operations WHERE organization_id=? AND token=? AND state='active'").bind(id).bind(token).execute(&mut *tx).await?.rows_affected();
        if n != 1 {
            return Err(conflict());
        }
        if delete {
            sqlx::query("DELETE FROM organizations WHERE id=?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn quarantine_collection_operation(
        &self,
        id: i64,
        token: &str,
        code: &str,
    ) -> Result<(), ShaideDBError> {
        sqlx::query("UPDATE collection_operations SET state='recovery_required',error_code=? WHERE organization_id=? AND token=?").bind(code).bind(id).bind(token).execute(&self.pool).await?;
        Ok(())
    }
    /// Authorization and metadata reservation are one write transaction.
    pub async fn reserve_upload(
        &self,
        file: InsertFileDao,
        owner: i64,
        token: &str,
        expiry: i64,
    ) -> Result<(), ShaideDBError> {
        let mut tx = begin_write(self).await?;
        collection_available(&mut tx, file.organization_id).await?;
        let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users u JOIN organizations o ON o.id=? WHERE u.id=? AND (u.role='admin' OR (unixepoch(u.expiry)>unixepoch() AND o.can_users_upload AND EXISTS(SELECT 1 FROM organization_memberships m WHERE m.organization_id=o.id AND m.user_id=u.id))))")
            .bind(file.organization_id).bind(owner).fetch_one(&mut *tx).await?;
        if !allowed {
            return Err(conflict());
        }
        sqlx::query("INSERT INTO files(hash,organization_id,file_name,mime_type) VALUES(?,?,?,?)")
            .bind(&file.hash)
            .bind(file.organization_id)
            .bind(file.file_name)
            .bind(file.mime_type)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO file_operations(file_hash,organization_id,owner_id,token,state,expires_at) VALUES(?,?,?,?,'presigning',unixepoch()+?)").bind(file.hash).bind(file.organization_id).bind(owner).bind(token).bind(expiry).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn confirm_reservation(&self, id: i64, hash: &str) -> Result<(), ShaideDBError> {
        self.transition_file_operation(id, hash, "presigning", "reserved")
            .await
    }
    pub async fn transition_file_operation(
        &self,
        id: i64,
        hash: &str,
        from: &str,
        to: &str,
    ) -> Result<(), ShaideDBError> {
        let n=sqlx::query("UPDATE file_operations SET state=? WHERE organization_id=? AND file_hash=? AND state=?").bind(to).bind(id).bind(hash).bind(from).execute(&self.pool).await?.rows_affected();
        if n != 1 {
            return Err(conflict());
        }
        Ok(())
    }
    pub async fn start_upload(&self, id: i64, hash: &str) -> Result<(), ShaideDBError> {
        let mut tx = begin_write(self).await?;
        collection_available(&mut tx, id).await?;
        let n=sqlx::query("UPDATE file_operations SET state='uploading' WHERE organization_id=? AND file_hash=? AND state='reserved' AND expires_at>unixepoch() AND EXISTS(SELECT 1 FROM users u JOIN organizations o ON o.id=file_operations.organization_id WHERE u.id=file_operations.owner_id AND (u.role='admin' OR (unixepoch(u.expiry)>unixepoch() AND o.can_users_upload AND EXISTS(SELECT 1 FROM organization_memberships m WHERE m.organization_id=o.id AND m.user_id=u.id))))")
            .bind(id).bind(hash).execute(&mut *tx).await?.rows_affected();
        if n != 1 {
            return Err(conflict());
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn record_upload_event(&self, id: i64, hash: &str) -> Result<String, ShaideDBError> {
        let mut tx = begin_write(self).await?;
        collection_available(&mut tx, id).await?;
        let processed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM files WHERE organization_id=? AND hash=? AND status='processed')").bind(id).bind(hash).fetch_one(&mut *tx).await?;
        if processed {
            tx.commit().await?;
            return Ok("processed".into());
        }
        upload_owner_allowed(&mut tx, id, hash).await?;
        let state:Option<String>=sqlx::query_scalar("UPDATE file_operations SET event_received=1 WHERE organization_id=? AND file_hash=? AND state IN ('uploading','uploaded','ingesting') RETURNING state").bind(id).bind(hash).fetch_optional(&mut *tx).await?;
        let state = state.ok_or_else(conflict)?;
        tx.commit().await?;
        Ok(state)
    }
    pub async fn upload_event_received(&self, id: i64, hash: &str) -> Result<bool, ShaideDBError> {
        Ok(sqlx::query_scalar(
            "SELECT event_received FROM file_operations WHERE organization_id=? AND file_hash=?",
        )
        .bind(id)
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(false))
    }
    pub async fn claim_ingestion(&self, id: i64, hash: &str) -> Result<bool, ShaideDBError> {
        let mut tx = begin_write(self).await?;
        collection_available(&mut tx, id).await?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM file_operations WHERE organization_id=? AND file_hash=?)",
        )
        .bind(id)
        .bind(hash)
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            tx.commit().await?;
            return Ok(false);
        }
        upload_owner_allowed(&mut tx, id, hash).await?;
        let n=sqlx::query("UPDATE file_operations SET state='ingesting' WHERE organization_id=? AND file_hash=? AND state='uploaded' AND event_received=1").bind(id).bind(hash).execute(&mut *tx).await?.rows_affected();
        if n == 1 {
            let updated=sqlx::query("UPDATE files SET status='processing',updated_at=DATETIME('now') WHERE organization_id=? AND hash=? AND status='uploading'").bind(id).bind(hash).execute(&mut *tx).await?.rows_affected();
            if updated != 1 {
                return Err(conflict());
            }
        }
        tx.commit().await?;
        Ok(n == 1)
    }
    pub async fn complete_ingestion(
        &self,
        id: i64,
        hash: &str,
        size: i64,
        pages: i64,
    ) -> Result<(), ShaideDBError> {
        let mut tx = begin_write(self).await?;
        let n=sqlx::query("DELETE FROM file_operations WHERE organization_id=? AND file_hash=? AND state='ingesting'").bind(id).bind(hash).execute(&mut *tx).await?.rows_affected();
        if n != 1 {
            return Err(conflict());
        }
        let updated=sqlx::query("UPDATE files SET status='processed',size=?,pages=?,error_code=NULL,updated_at=DATETIME('now') WHERE organization_id=? AND hash=? AND status='processing'").bind(size).bind(pages).bind(id).bind(hash).execute(&mut *tx).await?.rows_affected();
        if updated != 1 {
            return Err(conflict());
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn quarantine_file_operation(
        &self,
        id: i64,
        hash: &str,
        code: &str,
    ) -> Result<(), ShaideDBError> {
        let mut tx = begin_write(self).await?;
        let updated=sqlx::query("UPDATE file_operations SET state='recovery_required',error_code=? WHERE organization_id=? AND file_hash=?").bind(code).bind(id).bind(hash).execute(&mut *tx).await?.rows_affected();
        if updated != 1 {
            return Err(conflict());
        }
        sqlx::query("UPDATE files SET status=CASE WHEN status IN ('uploading','uploading_failed') THEN 'uploading_failed' ELSE 'processing_failed' END,error_code=?,updated_at=DATETIME('now') WHERE organization_id=? AND hash=?").bind(code).bind(id).bind(hash).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn claim_file_delete(
        &self,
        id: i64,
        hash: &str,
        token: &str,
        owner: i64,
    ) -> Result<(), ShaideDBError> {
        let mut tx = begin_write(self).await?;
        collection_available(&mut tx, id).await?;
        let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users u JOIN organizations o ON o.id=? WHERE u.id=? AND (u.role='admin' OR (unixepoch(u.expiry)>unixepoch() AND o.can_users_upload AND EXISTS(SELECT 1 FROM organization_memberships m WHERE m.organization_id=o.id AND m.user_id=u.id))))")
            .bind(id).bind(owner).fetch_one(&mut *tx).await?;
        if !allowed {
            return Err(conflict());
        }
        let row = sqlx::query("SELECT status FROM files WHERE organization_id=? AND hash=?")
            .bind(id)
            .bind(hash)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(ShaideDBError::NotFound(Resource::File))?;
        if matches!(
            row.get::<String, _>("status").as_str(),
            "uploading" | "processing"
        ) {
            return Err(conflict());
        }
        // A failed file may still have a remote writer: never override its fence.
        let n=sqlx::query("INSERT INTO file_operations(file_hash,organization_id,owner_id,token,state,expires_at) VALUES(?,?,?,?,'deleting',0) ON CONFLICT(file_hash) DO NOTHING").bind(hash).bind(id).bind(owner).bind(token).execute(&mut *tx).await?.rows_affected();
        if n != 1 {
            return Err(conflict());
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn complete_file_delete(
        &self,
        id: i64,
        hash: &str,
        token: &str,
    ) -> Result<(), ShaideDBError> {
        let mut tx = begin_write(self).await?;
        let n=sqlx::query("DELETE FROM file_operations WHERE organization_id=? AND file_hash=? AND token=? AND state='deleting'").bind(id).bind(hash).bind(token).execute(&mut *tx).await?.rows_affected();
        if n != 1 {
            return Err(conflict());
        }
        sqlx::query("DELETE FROM files WHERE organization_id=? AND hash=?")
            .bind(id)
            .bind(hash)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
