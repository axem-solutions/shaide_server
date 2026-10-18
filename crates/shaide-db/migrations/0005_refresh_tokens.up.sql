-- Refresh tokens let a client renew its access token without the user's password. Only a
-- SHA-256 digest of each token is stored. Every token rotated out of one login shares a
-- `family_id`, so presenting an already rotated token can revoke the whole session.
CREATE TABLE refresh_tokens (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    family_id    TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    expires_at   TIMESTAMP NOT NULL,
    -- Set when the token is exchanged for its successor.
    used_at      TIMESTAMP,
    -- Set when the token's family is revoked, by logout or detected reuse.
    revoked_at   TIMESTAMP,
    created_at   TIMESTAMP NOT NULL DEFAULT (DATETIME('now'))
);

CREATE INDEX refresh_tokens_family_id ON refresh_tokens (family_id);
CREATE INDEX refresh_tokens_user_id ON refresh_tokens (user_id);
