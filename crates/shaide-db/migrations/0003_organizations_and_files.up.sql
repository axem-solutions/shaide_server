-- Organizations group users and the files they share for retrieval. Column
-- names follow the earlier shaide schema so existing clients keep their field
-- mapping. An organization's embedding model cannot be deleted while in use,
-- because its stored vectors depend on that model. The default NO ACTION is
-- used instead of RESTRICT so SQLite reports a foreign key violation (787).
CREATE TABLE organizations (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    organization_name   TEXT NOT NULL,
    description         TEXT NOT NULL DEFAULT '',
    can_users_upload    BOOLEAN NOT NULL DEFAULT 0,
    embedding_model_id  INTEGER NOT NULL REFERENCES embedding_models(id),
    created_at          TIMESTAMP NOT NULL DEFAULT (DATETIME('now')),
    updated_at          TIMESTAMP NOT NULL DEFAULT (DATETIME('now'))
);

CREATE INDEX organizations_embedding_model_id ON organizations (embedding_model_id);

CREATE TABLE organization_memberships (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id             INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    organization_id     INTEGER NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    created_at          TIMESTAMP NOT NULL DEFAULT (DATETIME('now')),
    updated_at          TIMESTAMP NOT NULL DEFAULT (DATETIME('now')),
    UNIQUE (user_id, organization_id)
);

CREATE INDEX organization_memberships_organization_id
    ON organization_memberships (organization_id);

-- Uploaded files of an organization. `hash` is the object key. `status` keeps
-- the earlier lifecycle values; `error_code` is set only on failed states.
CREATE TABLE files (
    hash                TEXT NOT NULL PRIMARY KEY,
    organization_id     INTEGER NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    file_name           TEXT NOT NULL,
    mime_type           TEXT NOT NULL,
    size                INTEGER CHECK (size IS NULL OR size >= 0),
    pages               INTEGER CHECK (pages IS NULL OR pages >= 0),
    status              TEXT NOT NULL DEFAULT 'uploading' CHECK (status IN (
                            'uploading',
                            'uploading_failed',
                            'processing',
                            'processing_failed',
                            'processed'
                        )),
    error_code          TEXT,
    uploaded_at         TIMESTAMP NOT NULL DEFAULT (DATETIME('now')),
    updated_at          TIMESTAMP NOT NULL DEFAULT (DATETIME('now')),
    CHECK (error_code IS NULL OR status IN ('uploading_failed', 'processing_failed'))
);

CREATE INDEX files_organization_id ON files (organization_id);
