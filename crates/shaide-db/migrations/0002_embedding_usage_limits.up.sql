-- Daily input token limit per embedding model, enforced per user like the chat
-- model limits. NULL means unlimited.
ALTER TABLE embedding_models ADD COLUMN daily_input_token_limit INTEGER;

-- Embedding tokens used per user, embedding model and day. The unique key lets
-- usage be recorded with a single atomic upsert.
CREATE TABLE daily_usage_embedding_token (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    date                    TEXT NOT NULL,
    user                    INTEGER REFERENCES users(id) ON DELETE CASCADE NOT NULL,
    embedding_model         INTEGER REFERENCES embedding_models(id) ON DELETE CASCADE NOT NULL,
    total_input_token_count INTEGER NOT NULL DEFAULT 0,
    created_at              TIMESTAMP DEFAULT (DATETIME('now')),
    updated_at              TIMESTAMP DEFAULT (DATETIME('now')),
    UNIQUE (date, user, embedding_model)
);
