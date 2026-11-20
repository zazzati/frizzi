//! Answering a poll and editing an answer through the participant link.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use super::sorted_json;
use crate::error::{bad, db, error, missing, unique_or_db, ApiError, ApiResult};
use crate::limits::{per_ip, rate_limits, ClientIp, VOTES_PER_IP, VOTES_PER_PARTICIPANT};
use crate::mail::{Email, MailKind};
use crate::status::Status;
use crate::validate::{deadline_passed, default_locale, email, locale, valid, valid_token};
use crate::{mail, schedule_mail, Runtime};

#[derive(Deserialize)]
pub(super) struct VoteInput {
    name: String,
    email: String,
    #[serde(default = "default_locale")]
    locale: String,
    slots: Vec<Uuid>,
}
pub(super) async fn vote(
    State(pool): State<PgPool>,
    runtime: Runtime,
    ip: ClientIp,
    Path(public): Path<String>,
    Json(v): Json<VoteInput>,
) -> ApiResult {
    let response = save_vote(&pool, ip, &public, None, v).await?;
    schedule_mail(pool, runtime);
    Ok(response)
}
pub(super) async fn edit_vote(
    State(pool): State<PgPool>,
    runtime: Runtime,
    ip: ClientIp,
    Path((public, secret)): Path<(String, String)>,
    Json(v): Json<VoteInput>,
) -> ApiResult {
    let response = save_vote(&pool, ip, &public, Some(&secret), v).await?;
    schedule_mail(pool, runtime);
    Ok(response)
}
async fn save_vote(
    pool: &PgPool,
    ip: ClientIp,
    public: &str,
    secret: Option<&str>,
    v: VoteInput,
) -> ApiResult {
    let email = email(&v.email)?;
    if !valid_token(public) || secret.is_some_and(|s| !valid_token(s)) {
        return Err(missing());
    }
    if !valid(&v.name, 100) || v.slots.len() > 100 {
        return Err(bad());
    }
    rate_limits(
        pool,
        &[
            (ip.key("vote"), per_ip(VOTES_PER_IP)),
            (format!("vote:{public}:{email}"), VOTES_PER_PARTICIPANT),
        ],
    )
    .await?;
    let mut tx = pool.begin().await.map_err(db)?;
    let event = sqlx::query("SELECT id,title,allow_multiple_choices,status::text,timezone,voting_deadline,tiebreak_slot_id FROM events WHERE public_token=$1 FOR UPDATE")
        .bind(public).fetch_optional(&mut *tx).await.map_err(db)?.ok_or_else(missing)?;
    let id: Uuid = event.get("id");
    if event.get::<Status, _>("status") != Status::Open {
        return Err(error(StatusCode::CONFLICT, "event_closed"));
    }
    if deadline_passed(
        event.get("voting_deadline"),
        &event.get::<String, _>("timezone"),
    ) {
        return Err(error(StatusCode::CONFLICT, "deadline_passed"));
    }
    if !event.get::<bool, _>("allow_multiple_choices") && v.slots.len() > 1 {
        return Err(bad());
    }
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM event_slots WHERE event_id=$1 AND id=ANY($2)")
            .bind(id)
            .bind(&v.slots)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
    if n as usize != v.slots.len() {
        return Err(bad());
    }
    let (pid, edit_token): (Uuid, String) = if let Some(secret) = secret {
        let row = sqlx::query("UPDATE participants SET name=$1,email=$2,locale=$3,updated_at=now() WHERE event_id=$4 AND edit_token=$5 RETURNING id,edit_token")
            .bind(v.name.trim()).bind(&email).bind(locale(&v.locale)).bind(id).bind(secret)
            .fetch_optional(&mut *tx).await.map_err(unique_or_db)?.ok_or_else(missing)?;
        (row.get("id"), row.get("edit_token"))
    } else {
        let token = nanoid::nanoid!();
        let pid = sqlx::query_scalar("INSERT INTO participants (event_id,edit_token,name,email,locale) VALUES ($1,$2,$3,$4,$5) RETURNING id")
            .bind(id).bind(&token).bind(v.name.trim()).bind(&email).bind(locale(&v.locale))
            .fetch_one(&mut *tx).await.map_err(unique_or_db)?;
        (pid, token)
    };
    sqlx::query("DELETE FROM votes WHERE participant_id=$1")
        .bind(pid)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if !v.slots.is_empty() {
        sqlx::query("INSERT INTO votes (participant_id,slot_id) SELECT $1,UNNEST($2::uuid[])")
            .bind(pid)
            .bind(&v.slots)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    if let Some(slot) = event.get::<Option<Uuid>, _>("tiebreak_slot_id") {
        if !can_break_tie(&mut tx, id, slot).await? {
            sqlx::query("UPDATE events SET tiebreak_slot_id=NULL WHERE id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
    }
    let summary = mail::summary(&mut tx, id, locale(&v.locale)).await?;
    let title: String = event.get("title");
    let url = format!("{}/{public}/{edit_token}", mail::base_url());
    let receipt = Email {
        details: Some(&summary),
        ..Email::new(locale(&v.locale), MailKind::Vote, &title, &url)
    };
    mail::enqueue(&mut tx, id, &email, receipt).await?;
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"ok": true})))
}

pub(super) async fn participant(
    State(pool): State<PgPool>,
    Path((public, secret)): Path<(String, String)>,
) -> ApiResult {
    if !valid_token(&public) || !valid_token(&secret) {
        return Err(missing());
    }
    let mut answer: Answer = sqlx::query_as("SELECT p.id,p.name,p.email,p.locale FROM participants p JOIN events e ON e.id=p.event_id WHERE e.public_token=$1 AND p.edit_token=$2")
        .bind(public).bind(secret).fetch_optional(&pool).await.map_err(db)?.ok_or_else(missing)?;
    answer.slots = sqlx::query_scalar("SELECT slot_id FROM votes WHERE participant_id=$1")
        .bind(answer.id)
        .fetch_all(&pool)
        .await
        .map_err(db)?;
    Ok(sorted_json(&answer))
}
/// A participant's own answer, as their private link shows it for editing.
#[derive(FromRow, Serialize)]
struct Answer {
    #[serde(skip)]
    id: Uuid,
    name: String,
    email: String,
    locale: String,
    #[sqlx(skip)]
    slots: Vec<Uuid>,
}

/// Whether `slot` is one of several dates sharing the most votes, so it may break the tie.
pub(super) async fn can_break_tie(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    slot: Uuid,
) -> Result<bool, ApiError> {
    sqlx::query_scalar(
        "WITH counts AS (
            SELECT s.id, count(v.slot_id) AS votes
            FROM event_slots s LEFT JOIN votes v ON v.slot_id=s.id
            WHERE s.event_id=$1 GROUP BY s.id
         )
         SELECT count(*)>1 AND coalesce(bool_or(id=$2),false)
         FROM counts WHERE votes>0 AND votes=(SELECT max(votes) FROM counts)",
    )
    .bind(event)
    .bind(slot)
    .fetch_one(&mut **tx)
    .await
    .map_err(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_serializes_exactly_like_the_legacy_json() {
        for slots in [vec![], vec![Uuid::from_u128(7), Uuid::from_u128(8)]] {
            let answer = Answer {
                id: Uuid::from_u128(1),
                name: "Lucía \"L\" <b>".into(),
                email: "lucia@example.test".into(),
                locale: "es".into(),
                slots,
            };
            // The `json!` literal `participant` built before it used `Answer`.
            let legacy = json!({"name":answer.name,"email":answer.email,"locale":answer.locale,"slots":answer.slots});
            let Json(actual) = sorted_json(&answer);
            assert_eq!(
                serde_json::to_string(&actual).unwrap(),
                serde_json::to_string(&legacy).unwrap()
            );
        }
    }
}
