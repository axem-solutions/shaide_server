use std::fmt::{self, Display, Formatter};

use thiserror::Error;

use crate::files::FileStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Model,
    User,
    EmbeddingModel,
    Organization,
    File,
}

impl Display for Resource {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Model => "model",
            Self::User => "user",
            Self::EmbeddingModel => "embedding model",
            Self::Organization => "organization",
            Self::File => "file",
        })
    }
}

#[derive(Debug, Error)]
pub enum ShaideDBError {
    #[error("Database error: {0}")]
    DBError(#[from] sqlx::Error),

    #[error("Instance not found: {0}")]
    NotFound(Resource),

    #[error("Insert failed on conflict: {0}")]
    InsertFailedOnConflict(String),

    /// The record being written refers to a record that does not exist, such
    /// as a membership for a missing user. Unlike `NotFound`, the addressed
    /// record itself may exist.
    #[error("Referenced {0} does not exist")]
    MissingReference(Resource),

    /// A file lifecycle transition is not allowed from the file's `current`
    /// status. Nothing was changed.
    #[error("File status transition is not allowed from {current}")]
    FileStatusConflict { current: FileStatus },
}
