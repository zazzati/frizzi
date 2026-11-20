//! The organizer link: reading the event, changing its status, details and tiebreak, and
//! messaging participants.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use super::events::get_event;
use super::votes::can_break_tie;
use crate::error::{bad, db, error, missing, ApiError, ApiResult};
use crate::limits::{
    limit_notifications, per_ip, rate_limit, rate_limits, ClientIp, DETAILS_WRITES_PER_EVENT,
    MESSAGES_PER_EVENT, MESSAGES_PER_IP, STATUS_WRITES_PER_EVENT,
};
use crate::mail::{Change, Changes, MailKind};
use crate::places::LocationGeo;
use crate::slots::{insert_slots, SlotInput};
use crate::status::Status;
use crate::validate::{
    clean_description, deadline_within_slots, default_locale, locale, require_database_timezone,
    valid_multiline, valid_token, validate_deadline, validate_details, validate_slot_dates,
};
use crate::{mail, schedule_mail, Runtime};

pub(super) async fn admin(
    State(pool): State<PgPool>,
    Path((public, secret)): Path<(String, String)>,
) -> ApiResult {
    authorize_admin(&pool, &public, &secret).await?;
    get_event(&pool, &public).await
}
/// Returns the event id for a valid organizer link.
async fn authorize_admin(pool: &PgPool, public: &str, secret: &str) -> Result<Uuid, ApiError> {
    if !valid_token(public) || !valid_token(secret) {
        return Err(missing());
    }
    sqlx::query_scalar("SELECT id FROM events WHERE public_token=$1 AND admin_token=$2")
        .bind(public)
        .bind(secret)
        .fetch_optional(pool)
        .await
        .map_err(db)?
        .ok_or_else(missing)
}
/// The organizer's event, locked for the rest of the transaction.
#[derive(sqlx::FromRow)]
struct LockedEvent {
    id: Uuid,
    title: String,
    description: Option<String>,
    location: String,
    organizer_name: String,
    allow_multiple_choices: bool,
    status: Status,
    confirmed_slot_id: Option<Uuid>,
    timezone: String,
    voting_deadline: Option<NaiveDate>,
    location_geo: Option<sqlx::types::Json<LocationGeo>>,
}
/// Locks the event behind a valid organizer link. This re-checks the link inside the
/// transaction, so the event cannot change or disappear between authorization and the write.
///
/// The writes that also call [`authorize_admin`] do so before the transaction because their rate
/// limits are keyed on the event id and must commit even when the write later fails.
async fn lock_admin_event(
    tx: &mut Transaction<'_, Postgres>,
    public: &str,
    secret: &str,
) -> Result<LockedEvent, ApiError> {
    sqlx::query_as("SELECT id,title,description,location,organizer_name,allow_multiple_choices,status::text,confirmed_slot_id,timezone,voting_deadline,location_geo FROM events WHERE public_token=$1 AND admin_token=$2 FOR UPDATE")
        .bind(public)
        .bind(secret)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db)?
        .ok_or_else(missing)
}
#[derive(Deserialize)]
pub(super) struct StatusInput {
    status: String,
    confirmed_slot_id: Option<Uuid>,
    #[serde(default = "default_locale")]
    locale: String,
}
pub(super) async fn update_status(
    State(pool): State<PgPool>,
    runtime: Runtime,
    Path((public, secret)): Path<(String, String)>,
    Json(v): Json<StatusInput>,
) -> Result<Response, ApiError> {
    // Parsed here rather than by serde so an unknown status stays a 400 invalid_input.
    let status = Status::parse(&v.status).ok_or_else(bad)?;
    if (status == Status::Confirmed) != v.confirmed_slot_id.is_some() {
        return Err(bad());
    }
    let event = authorize_admin(&pool, &public, &secret).await?;
    rate_limit(&pool, &format!("status:{event}"), STATUS_WRITES_PER_EVENT).await?;
    let confirmation = match v.confirmed_slot_id {
        Some(slot) => mail::prepare_confirmation(&pool, &public, &secret, slot, &v.locale).await,
        None => None,
    };
    let mut tx = pool.begin().await.map_err(db)?;
    let e = lock_admin_event(&mut tx, &public, &secret).await?;
    let id = e.id;
    if let Some(slot) = v.confirmed_slot_id {
        let belongs: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM event_slots WHERE id=$1 AND event_id=$2)",
        )
        .bind(slot)
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if !belongs {
            return Err(bad());
        }
    }
    let changed = e.status != status || e.confirmed_slot_id != v.confirmed_slot_id;
    if changed {
        limit_notifications(&mut tx, id).await?;
    }
    sqlx::query("UPDATE events SET status=$1::event_status,confirmed_slot_id=$2,organizer_locale=$3,updated_at=now(),
       tiebreak_slot_id=CASE WHEN ($1='CONFIRMED' AND tiebreak_slot_id=$2) OR ($1='OPEN' AND status='OPEN')
                           THEN tiebreak_slot_id ELSE NULL END WHERE id=$4")
        .bind(status).bind(v.confirmed_slot_id).bind(locale(&v.locale)).bind(id).execute(&mut *tx).await.map_err(db)?;
    if changed {
        mail::notify(&mut tx, id, status.into(), None, confirmation.as_ref()).await?;
    }
    tx.commit().await.map_err(db)?;
    schedule_mail(pool, runtime);
    let mut response = Json(json!({"ok": true})).into_response();
    response.extensions_mut().insert(if changed {
        status.outcome()
    } else {
        "unchanged"
    });
    Ok(response)
}

#[derive(Deserialize)]
pub(super) struct TiebreakInput {
    slot_id: Option<Uuid>,
}
pub(super) async fn update_tiebreak(
    State(pool): State<PgPool>,
    Path((public, secret)): Path<(String, String)>,
    Json(input): Json<TiebreakInput>,
) -> ApiResult {
    if !valid_token(&public) || !valid_token(&secret) {
        return Err(missing());
    }
    let mut tx = pool.begin().await.map_err(db)?;
    let event = lock_admin_event(&mut tx, &public, &secret).await?;
    let id = event.id;
    if event.status != Status::Open {
        return Err(error(StatusCode::CONFLICT, "event_closed"));
    }
    if let Some(slot) = input.slot_id {
        if !can_break_tie(&mut tx, id, slot).await? {
            return Err(error(StatusCode::CONFLICT, "tiebreak_unavailable"));
        }
    }
    sqlx::query("UPDATE events SET tiebreak_slot_id=$1,updated_at=now() WHERE id=$2")
        .bind(input.slot_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
pub(super) struct ParticipantMessage {
    request_id: Uuid,
    message: String,
}
pub(super) async fn message_participants(
    State(pool): State<PgPool>,
    runtime: Runtime,
    ip: ClientIp,
    Path((public, secret)): Path<(String, String)>,
    Json(input): Json<ParticipantMessage>,
) -> ApiResult {
    if !valid_token(&public) || !valid_token(&secret) {
        return Err(missing());
    }
    if !valid_multiline(&input.message, 2000) {
        return Err(bad());
    }
    let event = authorize_admin(&pool, &public, &secret).await?;
    // A retried request returns its receipt without counting against the limits.
    let existing: Option<i32> = sqlx::query_scalar(
        "SELECT recipients FROM participant_messages WHERE event_id=$1 AND request_id=$2",
    )
    .bind(event)
    .bind(input.request_id)
    .fetch_optional(&pool)
    .await
    .map_err(db)?;
    if let Some(queued) = existing {
        schedule_mail(pool, runtime);
        return Ok(Json(json!({"queued": queued})));
    }
    rate_limits(
        &pool,
        &[
            (ip.key("participant-message"), per_ip(MESSAGES_PER_IP)),
            (format!("participant-message:{event}"), MESSAGES_PER_EVENT),
        ],
    )
    .await?;
    let mut tx = pool.begin().await.map_err(db)?;
    let event = lock_admin_event(&mut tx, &public, &secret).await?;
    let id = event.id;
    let existing: Option<i32> = sqlx::query_scalar(
        "SELECT recipients FROM participant_messages WHERE event_id=$1 AND request_id=$2",
    )
    .bind(id)
    .bind(input.request_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;
    if let Some(queued) = existing {
        tx.commit().await.map_err(db)?;
        schedule_mail(pool, runtime);
        return Ok(Json(json!({"queued": queued})));
    }
    let queued =
        mail::enqueue_participant_message(&mut tx, id, &public, &event.title, input.message.trim())
            .await?;
    if queued == 0 {
        return Err(error(StatusCode::CONFLICT, "no_participants"));
    }
    sqlx::query(
        "INSERT INTO participant_messages (event_id,request_id,recipients) VALUES ($1,$2,$3)",
    )
    .bind(id)
    .bind(input.request_id)
    .bind(queued as i32)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    tx.commit().await.map_err(db)?;
    schedule_mail(pool, runtime);
    Ok(Json(json!({"queued": queued})))
}

#[derive(Deserialize)]
pub(super) struct DetailsInput {
    title: String,
    description: Option<String>,
    location: String,
    #[serde(default, deserialize_with = "optional_value")]
    location_geo: Option<Option<LocationGeo>>,
    organizer_name: String,
    allow_multiple_choices: bool,
    timezone: String,
    #[serde(default, deserialize_with = "optional_value")]
    voting_deadline: Option<Option<NaiveDate>>,
    slots: Vec<SlotInput>,
    #[serde(default = "default_locale")]
    locale: String,
}
fn optional_value<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(deserializer).map(Some)
}
pub(super) async fn update_details(
    State(pool): State<PgPool>,
    runtime: Runtime,
    Path((public, secret)): Path<(String, String)>,
    Json(mut v): Json<DetailsInput>,
) -> ApiResult {
    v.description = clean_description(v.description.take());
    if !validate_details(
        &v.title,
        &v.description,
        &v.location,
        &v.organizer_name,
        &v.timezone,
        &v.slots,
    ) {
        return Err(bad());
    }
    let event = authorize_admin(&pool, &public, &secret).await?;
    rate_limit(&pool, &format!("details:{event}"), DETAILS_WRITES_PER_EVENT).await?;
    let mut tx = pool.begin().await.map_err(db)?;
    let old = lock_admin_event(&mut tx, &public, &secret).await?;
    let id = old.id;
    if old.timezone != v.timezone {
        require_database_timezone(&mut tx, &v.timezone).await?;
    }
    let old_location = old.location;
    let old_geo = old.location_geo.map(|geo| geo.0);
    let geo = v.location_geo.unwrap_or_else(|| {
        if old_location == v.location.trim() {
            old_geo.clone()
        } else {
            None
        }
    });
    if let Some(geo) = &geo {
        geo.validate()?;
    }
    let old_deadline = old.voting_deadline;
    let deadline = v.voting_deadline.unwrap_or(old_deadline);
    if deadline != old_deadline {
        validate_deadline(deadline, &v.timezone)?;
    }
    if !v.allow_multiple_choices {
        let multiple: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM votes v JOIN participants p ON p.id=v.participant_id WHERE p.event_id=$1 GROUP BY p.id HAVING count(*)>1)")
            .bind(id).fetch_one(&mut *tx).await.map_err(db)?;
        if multiple {
            return Err(error(StatusCode::CONFLICT, "multiple_votes_exist"));
        }
    }
    let old_slots: Vec<SlotInput> = sqlx::query("SELECT id,slot_date,start_time,end_time FROM event_slots WHERE event_id=$1 ORDER BY slot_date")
        .bind(id).fetch_all(&mut *tx).await.map_err(db)?.iter().map(|s| SlotInput { id: Some(s.get("id")), date:s.get("slot_date"), start:s.get("start_time"), end:s.get("end_time") }).collect();
    let ids: Vec<_> = v.slots.iter().filter_map(|s| s.id).collect();
    let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
    if unique_ids.len() != ids.len()
        || ids
            .iter()
            .any(|id| !old_slots.iter().any(|s| s.id == Some(*id)))
    {
        return Err(bad());
    }
    let dates_changed =
        old_slots.len() != v.slots.len() || v.slots.iter().any(|s| !old_slots.contains(s));
    let old_dates: Vec<_> = old_slots.iter().map(|s| s.date).collect();
    validate_slot_dates(&v.slots, &old_dates, &v.timezone)?;
    if (deadline != old_deadline || dates_changed) && !deadline_within_slots(deadline, &v.slots) {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_deadline"));
    }
    let mut changes: Vec<Change> = Vec::new();
    for (key, before, after) in [
        ("title", old.title, v.title.trim().to_string()),
        (
            "description",
            old.description.unwrap_or_default(),
            v.description.clone().unwrap_or_default(),
        ),
        (
            "location",
            old_location.clone(),
            v.location.trim().to_string(),
        ),
        (
            "organizer",
            old.organizer_name,
            v.organizer_name.trim().to_string(),
        ),
        ("timezone", old.timezone.clone(), v.timezone.clone()),
        (
            "votingDeadline",
            old_deadline
                .map(|date| date.to_string())
                .unwrap_or_default(),
            deadline.map(|date| date.to_string()).unwrap_or_default(),
        ),
        (
            "multiple",
            old.allow_multiple_choices.to_string(),
            v.allow_multiple_choices.to_string(),
        ),
    ] {
        if before != after {
            changes.push(Change { key, before, after });
        }
    }
    if old_location == v.location.trim() && geo != old_geo {
        changes.push(Change {
            key: "location",
            before: old_geo.as_ref().map_or_else(
                || old_location.clone(),
                |geo| geo.with_coordinates(&old_location),
            ),
            after: geo.as_ref().map_or_else(
                || old_location.clone(),
                |geo| geo.with_coordinates(&old_location),
            ),
        });
    }
    let notify = dates_changed || !changes.is_empty();
    if notify {
        limit_notifications(&mut tx, id).await?;
    }
    if dates_changed || old.timezone != v.timezone {
        // A changed date/time is a new availability question; preserve only unaffected votes.
        sqlx::query("UPDATE events SET status='OPEN',confirmed_slot_id=NULL,tiebreak_slot_id=NULL WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        if old.timezone != v.timezone {
            sqlx::query(
                "DELETE FROM votes WHERE slot_id IN (SELECT id FROM event_slots WHERE event_id=$1)",
            )
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }
    }
    if dates_changed {
        // Delete changed slots before inserts so changing/swapping calendar days respects UNIQUE(event_id, slot_date).
        let removed: Vec<Uuid> = old_slots
            .iter()
            .filter(|s| !v.slots.contains(s))
            .filter_map(|s| s.id)
            .collect();
        if !removed.is_empty() {
            sqlx::query("DELETE FROM event_slots WHERE id=ANY($1)")
                .bind(&removed)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        insert_slots(
            &mut tx,
            id,
            v.slots.iter().filter(|s| !old_slots.contains(s)),
        )
        .await?;
    }
    sqlx::query("UPDATE events SET title=$1,description=$2,location=$3,organizer_name=$4,allow_multiple_choices=$5,timezone=$6,organizer_locale=$7,voting_deadline=$8,location_geo=$9,updated_at=now() WHERE id=$10")
        .bind(v.title.trim()).bind(&v.description).bind(v.location.trim()).bind(v.organizer_name.trim()).bind(v.allow_multiple_choices)
        .bind(&v.timezone).bind(locale(&v.locale)).bind(deadline).bind(geo.map(sqlx::types::Json)).bind(id).execute(&mut *tx).await.map_err(db)?;
    if notify {
        let changes = Changes {
            fields: &changes,
            before_slots: &old_slots,
            dates_changed,
        };
        mail::notify(&mut tx, id, MailKind::Changed, Some(&changes), None).await?;
    }
    tx.commit().await.map_err(db)?;
    schedule_mail(pool, runtime);
    Ok(Json(json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn details_patch_distinguishes_missing_from_null() {
        let parse = |extra: Value| {
            let mut body = json!({
                "title": "Cena", "location": "Roma", "organizer_name": "Ada",
                "allow_multiple_choices": false, "timezone": "Europe/Rome", "slots": []
            });
            body.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            serde_json::from_value::<DetailsInput>(body).unwrap()
        };
        let missing = parse(json!({}));
        assert_eq!(missing.voting_deadline, None);
        assert!(missing.location_geo.is_none());
        let cleared = parse(json!({"voting_deadline": null, "location_geo": null}));
        assert_eq!(cleared.voting_deadline, Some(None));
        assert_eq!(cleared.location_geo, Some(None));
        let set = parse(json!({
            "voting_deadline": "2027-01-10",
            "location_geo": {"latitude": 1.0, "longitude": 2.0, "kind": "venue", "attribution": null}
        }));
        assert_eq!(
            set.voting_deadline,
            Some(NaiveDate::from_ymd_opt(2027, 1, 10))
        );
        assert!(matches!(set.location_geo, Some(Some(_))));
    }
}
