//! The authenticated cron endpoint that runs maintenance on serverless deployments.

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    Json,
};
use serde_json::json;
use sqlx::PgPool;

use crate::error::{db, error, ApiResult};
use crate::{maintain, maintain_observability};

pub(super) async fn retry_mail(State(pool): State<PgPool>, headers: HeaderMap) -> ApiResult {
    let secret = std::env::var("CRON_SECRET")
        .ok()
        .filter(|s| s.len() >= 32)
        .ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "cron_unconfigured"))?;
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    if !provided.is_some_and(|token| constant_time_eq(token.as_bytes(), secret.as_bytes())) {
        return Err(error(StatusCode::UNAUTHORIZED, "unauthorized"));
    }
    maintain(&pool).await.map_err(db)?;
    maintain_observability(&pool).await;
    Ok(Json(json!({"ok": true})))
}
/// Compares secrets without stopping at the first differing byte. Only the length, which is not
/// secret, can affect the timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(diff) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_comparison() {
        assert!(constant_time_eq(b"", b""));
        assert!(constant_time_eq(b"secret-value", b"secret-value"));
        assert!(!constant_time_eq(b"secret-value", b"secret-valuf"));
        assert!(!constant_time_eq(b"secret-value", b"secret-valu"));
        assert!(!constant_time_eq(b"secret", b""));
    }
}
