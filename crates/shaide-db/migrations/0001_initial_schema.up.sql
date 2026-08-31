CREATE TABLE embedding_models (
    id                           INTEGER PRIMARY KEY AUTOINCREMENT,
    url                          TEXT NOT NULL,
    name                         TEXT NOT NULL,
    vector_size                  INTEGER NOT NULL,
    created_at                   TIMESTAMP DEFAULT (DATETIME('now')),
    updated_at                   TIMESTAMP DEFAULT (DATETIME('now')),
    platform                     TEXT,
    api_schema                   TEXT,
    max_embedding_model_text_len INTEGER
);

CREATE TABLE users (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at      TIMESTAMP DEFAULT (DATETIME('now')),
    updated_at      TIMESTAMP DEFAULT (DATETIME('now')),
    username        TEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL DEFAULT '',
    role            TEXT CHECK(role IN ('user', 'admin')) NOT NULL DEFAULT 'user',
    expiry          TIMESTAMP NOT NULL DEFAULT (DATETIME('now'))
);

CREATE TABLE models (
    id                         INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at                 TIMESTAMP DEFAULT (DATETIME('now')),
    updated_at                 TIMESTAMP DEFAULT (DATETIME('now')),
    name                       TEXT NOT NULL UNIQUE,
    variant                    TEXT NOT NULL,
    chat_completions_endpoint  TEXT NOT NULL,
    completions_endpoint       TEXT,
    responses_endpoint         TEXT,
    api_schema                 TEXT CHECK(api_schema IN ('open_ai', 'vertex', 'anthropic')) NOT NULL,
    daily_input_token_limit    INTEGER,
    daily_output_token_limit   INTEGER,
    max_generated_tokens       INTEGER NOT NULL DEFAULT 32000,
    platform                   TEXT,
    context_size               INTEGER NOT NULL,
    native_fim_mode            TEXT CHECK(native_fim_mode IN ('completions_suffix', 'fim_tokens')),
    fim_prompt_template        TEXT,
    supports_images            BOOLEAN NOT NULL DEFAULT 0,
    max_images_per_request     INTEGER,
    max_image_bytes            INTEGER,
    max_image_width_px         INTEGER,
    max_image_height_px        INTEGER,
    reasoning_effort_values    TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE daily_usage_token (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    date                     TEXT NOT NULL,
    user                     INTEGER REFERENCES users(id) ON DELETE CASCADE NOT NULL,
    model                    INTEGER REFERENCES models(id) ON DELETE CASCADE NOT NULL,
    total_input_token_count  INTEGER NOT NULL DEFAULT 0,
    total_output_token_count INTEGER NOT NULL DEFAULT 0,
    created_at               TIMESTAMP DEFAULT (DATETIME('now')),
    updated_at               TIMESTAMP DEFAULT (DATETIME('now'))
);

CREATE TABLE api_usage (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    route              TEXT NOT NULL,
    request_made       TIMESTAMP DEFAULT (DATETIME('now')),
    user               INTEGER REFERENCES users(id) ON DELETE CASCADE NOT NULL,
    model              INTEGER REFERENCES models(id) ON DELETE CASCADE,
    input_token_count  INTEGER,
    output_token_count INTEGER,
    created_at         TIMESTAMP DEFAULT (DATETIME('now')),
    updated_at         TIMESTAMP DEFAULT (DATETIME('now'))
);
