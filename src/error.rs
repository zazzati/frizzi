//! API errors: a status code plus a stable machine-readable reason.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::observability;

#[derive(Debug)]
pub(crate) struct ApiError(pub(crate) StatusCode, pub(crate) &'static str);
pub(crate) type ApiResult = Result<Json<Value>, ApiError>;
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (self.0, Json(json!({"error": self.1}))).into_response();
        response.extensions_mut().insert(self.1);
        response
    }
}
pub(crate) fn error(code: StatusCode, message: &'static str) -> ApiError {
    ApiError(code, message)
}
pub(crate) fn bad() -> ApiError {
    error(StatusCode::BAD_REQUEST, "invalid_input")
}
#[track_caller]
pub(crate) fn db(e: sqlx::Error) -> ApiError {
    let caller = std::panic::Location::caller();
    let code = e.as_database_error().and_then(|error| error.code());
    tracing::error!(
        event = "database_error",
        reason = observability::db_class(&e),
        sqlstate = code.as_deref(),
        file = caller.file(),
        line = caller.line()
    );
    error(StatusCode::INTERNAL_SERVER_ERROR, "database_error")
}
pub(crate) fn unique_or_db(e: sqlx::Error) -> ApiError {
    if e.as_database_error()
        .is_some_and(|e| e.is_unique_violation())
    {
        error(StatusCode::CONFLICT, "email_exists")
    } else {
        db(e)
    }
}
pub(crate) fn missing() -> ApiError {
    error(StatusCode::NOT_FOUND, "not_found")
}
