//! HTTP routes, grouped by resource.

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post, put},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::time::Duration;

use crate::{calendar, observability, places};

mod admin;
mod cron;
mod events;
mod recover;
mod votes;

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/api/ready", get(ready))
        .route(
            "/api/telemetry",
            post(observability::interaction).layer(DefaultBodyLimit::max(256)),
        )
        .route("/api/events", post(events::create))
        .route("/api/places/suggest", post(places::suggest))
        .route("/api/events/{public}", get(events::public_event))
        .route("/api/events/{public}/votes", post(votes::vote))
        .route("/api/events/{public}/recover", post(recover::recover_links))
        .route("/api/events/{public}/calendar.ics", get(calendar::download))
        .route(
            "/api/events/{public}/participant/{secret}",
            get(votes::participant).put(votes::edit_vote),
        )
        .route(
            "/api/events/{public}/admin/{secret}",
            get(admin::admin).put(admin::update_status),
        )
        .route(
            "/api/events/{public}/admin/{secret}/details",
            put(admin::update_details),
        )
        .route(
            "/api/events/{public}/admin/{secret}/tiebreak",
            put(admin::update_tiebreak),
        )
        .route(
            "/api/events/{public}/admin/{secret}/messages",
            post(admin::message_participants),
        )
        .route("/api/cron/email", get(cron::retry_mail))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn_with_state(
            pool.clone(),
            observability::request,
        ))
        .with_state(pool)
}
/// Renders a response body through [`Value`], whose objects keep their keys sorted, so a
/// response struct produces the same bytes as the `json!` literal it replaced, nested types
/// included.
fn sorted_json<T: Serialize>(body: &T) -> Json<Value> {
    Json(serde_json::to_value(body).expect("response bodies serialize to JSON"))
}
async fn ready(State(pool): State<PgPool>) -> (StatusCode, Json<Value>) {
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query("SELECT 1 FROM usage_metrics LIMIT 1").execute(&pool),
    )
    .await;
    let ok = matches!(result, Ok(Ok(_)));
    (
        if ok {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(json!({"ok": ok})),
    )
}
async fn security_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}
