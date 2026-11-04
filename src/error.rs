use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    Unauthorized(String),
    BadRequest(String),
    NotFound(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<pebble_core::PebbleError> for ApiError {
    fn from(error: pebble_core::PebbleError) -> Self {
        use pebble_core::PebbleError;
        match error {
            PebbleError::Internal(_) | PebbleError::Storage(_) => {
                ApiError::Internal(error.to_string())
            }
            other => ApiError::BadRequest(other.to_string()),
        }
    }
}
