use std::fmt::{self, Display, Formatter};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Model,
    User,
}

impl Display for Resource {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Model => "model",
            Self::User => "user",
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
}
