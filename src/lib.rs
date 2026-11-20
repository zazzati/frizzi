mod calendar;
mod error;
mod limits;
mod mail;
mod observability;
mod places;
mod retention;
mod routes;
mod slots;
mod status;
mod validate;
pub use observability::{init_tracing, monitor_public_urls};
pub use routes::router;

use axum::{Extension, Router};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::{str::FromStr, time::Duration};

type Runtime = Option<Extension<vercel_runtime::AppState>>;
static MAIL_READY: tokio::sync::Notify = tokio::sync::Notify::const_new();

rust_i18n::i18n!("locales", fallback = "it");

pub async fn pool() -> PgPool {
    let options =
        PgConnectOptions::from_str(&std::env::var("DATABASE_URL").expect("DATABASE_URL required"))
            .expect("Invalid DATABASE_URL")
            .statement_cache_capacity(0);
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(30))
        .connect_lazy_with(options)
}
pub async fn app() -> Router {
    router(pool().await)
}
pub async fn maintain(pool: &PgPool) -> Result<(), sqlx::Error> {
    let (events, legacy_mail) = retention::purge(pool).await?;
    if events + legacy_mail > 0 {
        tracing::info!(event = "event_retention_purge", events, legacy_mail);
    }
    mail::dispatch(pool, 25).await;
    mail::cleanup(pool).await;
    Ok(())
}
pub async fn maintain_observability(pool: &PgPool) {
    observability::prune_logs();
    if let Err(error) = observability::purge_metrics(pool).await {
        tracing::error!(
            event = "metrics_retention_failed",
            reason = observability::db_class(&error)
        );
    }
    mail::inspect_queue(pool).await;
}
pub fn log_maintenance_error(error: &sqlx::Error) {
    tracing::error!(
        event = "event_retention_failed",
        reason = observability::db_class(error)
    );
}
pub async fn wait_for_mail() {
    MAIL_READY.notified().await;
}
/// Sends queued mail after the response: inside the Vercel invocation, or by waking the local
/// mail worker.
fn schedule_mail(pool: PgPool, runtime: Runtime) {
    if let Some(Extension(runtime)) = runtime {
        runtime.wait_until(async move { mail::dispatch(&pool, 25).await });
    } else {
        MAIL_READY.notify_one();
    }
}
