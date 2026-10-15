-- Durable fences: no lease expiry releases a possibly accepted remote write.
CREATE TABLE collection_operations (
    organization_id INTEGER PRIMARY KEY REFERENCES organizations(id),
    token TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'active' CHECK(state IN ('active','recovery_required')),
    error_code TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT (DATETIME('now'))
);
CREATE TABLE file_operations (
    file_hash TEXT PRIMARY KEY REFERENCES files(hash),
    organization_id INTEGER NOT NULL REFERENCES organizations(id),
    owner_id INTEGER NOT NULL REFERENCES users(id),
    token TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK(state IN ('presigning','reserved','uploading','uploaded','ingesting','deleting','recovery_required')),
    expires_at INTEGER NOT NULL,
    query_digest TEXT,
    event_received BOOLEAN NOT NULL DEFAULT 0,
    error_code TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT (DATETIME('now'))
);
CREATE INDEX file_operations_organization_id ON file_operations(organization_id);

-- Legacy non-final metadata cannot establish whether a remote write finished.
-- Preserve the rows and require scoped reconciliation before destructive work.
INSERT INTO collection_operations (organization_id, token, kind, payload, state, error_code)
SELECT DISTINCT organization_id, 'migration-0004-' || organization_id,
       'legacy_recovery', '{}', 'recovery_required', 'legacy_file_outcome_unknown'
FROM files WHERE status != 'processed';
