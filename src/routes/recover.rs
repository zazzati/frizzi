//! Re-sending private links to an address that belongs to an event.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::error::{db, ApiResult};
use crate::limits::{
    per_ip, rate_limit, rate_limits, ClientIp, RECOVERY_PER_EMAIL, RECOVERY_PER_EVENT,
    RECOVERY_PER_IP,
};
use crate::mail::{Email, MailKind};
use crate::validate::{email, locale, valid_token};
use crate::{mail, schedule_mail, Runtime};

#[derive(Deserialize)]
pub(super) struct RecoveryInput {
    email: String,
}
pub(super) async fn recover_links(
    State(pool): State<PgPool>,
    runtime: Runtime,
    ip: ClientIp,
    Path(public): Path<String>,
    Json(input): Json<RecoveryInput>,
) -> ApiResult {
    let accepted = Json(json!({"ok": true}));
    let address = email(&input.email)?;
    if !valid_token(&public) {
        return Ok(accepted);
    }
    rate_limit(&pool, &ip.key("recover"), per_ip(RECOVERY_PER_IP)).await?;
    // Address and event limits answer like a normal request so they reveal nothing.
    if let Err(err) = rate_limits(
        &pool,
        &[
            (format!("recover-email:{address}"), RECOVERY_PER_EMAIL),
            (format!("recover-event:{public}"), RECOVERY_PER_EVENT),
        ],
    )
    .await
    {
        if err.0 == StatusCode::TOO_MANY_REQUESTS {
            return Ok(accepted);
        }
        return Err(err);
    }
    let mut tx = pool.begin().await.map_err(db)?;
    let Some(event) = sqlx::query("SELECT id FROM events WHERE public_token=$1 FOR KEY SHARE")
        .bind(&public)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
    else {
        tx.commit().await.map_err(db)?;
        return Ok(accepted);
    };
    let event_id: Uuid = event.get("id");
    let matches = sqlx::query(
        "SELECT e.title, e.admin_token AS token, e.organizer_locale AS locale, true AS organizer
         FROM events e WHERE e.id=$1 AND e.organizer_email=$2
         UNION ALL
         SELECT e.title,p.edit_token,p.locale,false FROM participants p
         JOIN events e ON e.id=p.event_id WHERE e.id=$1 AND p.email=$2",
    )
    .bind(event_id)
    .bind(&address)
    .fetch_all(&mut *tx)
    .await
    .map_err(db)?;
    for row in &matches {
        let token: String = row.get("token");
        let role = if row.get("organizer") { "admin/" } else { "" };
        let url = format!("{}/{public}/{role}{token}", mail::base_url());
        let (lang, title): (String, String) = (row.get("locale"), row.get("title"));
        let email = Email::new(locale(&lang), MailKind::Recover, &title, &url);
        mail::enqueue(&mut tx, event_id, &address, email).await?;
    }
    tx.commit().await.map_err(db)?;
    if !matches.is_empty() {
        schedule_mail(pool, runtime);
    }
    Ok(accepted)
}
