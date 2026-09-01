use hyper::Uri;

use crate::error::ShaideError;

pub mod chat;
pub mod completions;
pub mod embedding;
pub mod health;
pub mod mcp;
pub mod metrics;
pub mod models;
pub mod responses;
pub mod statistics;
pub mod trial;
pub mod users;
pub mod vector_db;

pub async fn fallback_404(uri: Uri) -> ShaideError {
    ShaideError::not_found_route(uri.to_string())
}
