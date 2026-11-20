//! Creating an event and reading its public page.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use super::sorted_json;
use crate::error::{bad, db, error, missing, ApiResult};
use crate::limits::{per_ip, rate_limits, ClientIp, CREATE_PER_IP, CREATE_PER_ORGANIZER};
use crate::mail::{Email, MailKind};
use crate::places::LocationGeo;
use crate::slots::{insert_slots, SlotInput};
use crate::status::Status;
use crate::validate::{
    clean_description, deadline_passed, deadline_within_slots, default_locale, email, locale,
    require_database_timezone, valid_token, validate_deadline, validate_details,
    validate_slot_dates,
};
use crate::{mail, schedule_mail, Runtime};

#[derive(Deserialize)]
pub(super) struct EventInput {
    title: String,
    description: Option<String>,
    location: String,
    location_geo: Option<LocationGeo>,
    organizer_name: String,
    organizer_email: String,
    #[serde(default = "default_locale")]
    locale: String,
    allow_multiple_choices: bool,
    timezone: String,
    voting_deadline: Option<NaiveDate>,
    slots: Vec<SlotInput>,
}
pub(super) async fn create(
    State(pool): State<PgPool>,
    runtime: Runtime,
    ip: ClientIp,
    Json(mut e): Json<EventInput>,
) -> ApiResult {
    let organizer_email = email(&e.organizer_email)?;
    e.description = clean_description(e.description.take());
    if !validate_details(
        &e.title,
        &e.description,
        &e.location,
        &e.organizer_name,
        &e.timezone,
        &e.slots,
    ) || e.slots.iter().any(|s| s.id.is_some())
    {
        return Err(bad());
    }
    validate_slot_dates(&e.slots, &[], &e.timezone)?;
    validate_deadline(e.voting_deadline, &e.timezone)?;
    if !deadline_within_slots(e.voting_deadline, &e.slots) {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_deadline"));
    }
    if let Some(geo) = &e.location_geo {
        geo.validate()?;
    }
    let public = nanoid::nanoid!();
    let secret = nanoid::nanoid!();
    rate_limits(
        &pool,
        &[
            (ip.key("create"), per_ip(CREATE_PER_IP)),
            (format!("create:{organizer_email}"), CREATE_PER_ORGANIZER),
        ],
    )
    .await?;
    let mut tx = pool.begin().await.map_err(db)?;
    require_database_timezone(&mut tx, &e.timezone).await?;
    let id: Uuid = sqlx::query_scalar("INSERT INTO events (public_token,admin_token,title,description,location,organizer_name,organizer_email,organizer_locale,allow_multiple_choices,timezone,voting_deadline,location_geo) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING id")
        .bind(&public).bind(&secret).bind(e.title.trim()).bind(&e.description).bind(e.location.trim())
        .bind(e.organizer_name.trim()).bind(&organizer_email).bind(locale(&e.locale))
        .bind(e.allow_multiple_choices).bind(&e.timezone).bind(e.voting_deadline).bind(e.location_geo.map(sqlx::types::Json)).fetch_one(&mut *tx).await.map_err(db)?;
    insert_slots(&mut tx, id, &e.slots).await?;
    let url = format!("{}/{public}/admin/{secret}", mail::base_url());
    let email = Email::new(locale(&e.locale), MailKind::Admin, &e.title, &url);
    mail::enqueue(&mut tx, id, &organizer_email, email).await?;
    tx.commit().await.map_err(db)?;
    schedule_mail(pool, runtime);
    Ok(Json(json!({"public_token": public})))
}
pub(super) async fn public_event(
    State(pool): State<PgPool>,
    Path(public): Path<String>,
) -> ApiResult {
    get_event(&pool, &public).await
}
/// The event page shared by the public and organizer links.
#[derive(FromRow, Serialize)]
pub(super) struct EventView {
    #[serde(skip)]
    id: Uuid,
    title: String,
    description: Option<String>,
    location: String,
    location_geo: Option<sqlx::types::Json<LocationGeo>>,
    organizer_name: String,
    allow_multiple_choices: bool,
    status: Status,
    confirmed_slot_id: Option<Uuid>,
    tiebreak_slot_id: Option<Uuid>,
    timezone: String,
    voting_deadline: Option<NaiveDate>,
    #[sqlx(skip)]
    voting_ended: bool,
    #[sqlx(skip)]
    slots: Vec<SlotView>,
    #[sqlx(skip)]
    participants: Vec<ParticipantView>,
}
#[derive(FromRow, Serialize)]
struct SlotView {
    id: Uuid,
    #[sqlx(rename = "slot_date")]
    date: NaiveDate,
    #[sqlx(rename = "start_time")]
    start: NaiveTime,
    #[sqlx(rename = "end_time")]
    end: Option<NaiveTime>,
}
/// A participant's name and chosen slots; addresses and links stay private.
#[derive(FromRow, Serialize)]
struct ParticipantView {
    name: String,
    slots: Vec<Uuid>,
}
pub(super) async fn get_event(pool: &PgPool, public: &str) -> ApiResult {
    if !valid_token(public) {
        return Err(missing());
    }
    let mut event: EventView = sqlx::query_as("SELECT id,title,description,location,organizer_name,allow_multiple_choices,status::text,confirmed_slot_id,tiebreak_slot_id,timezone,voting_deadline,location_geo FROM events WHERE public_token=$1")
        .bind(public).fetch_optional(pool).await.map_err(db)?.ok_or_else(missing)?;
    event.slots = sqlx::query_as("SELECT id,slot_date,start_time,end_time FROM event_slots WHERE event_id=$1 ORDER BY slot_date")
        .bind(event.id).fetch_all(pool).await.map_err(db)?;
    event.participants = sqlx::query_as("SELECT p.id,p.name,COALESCE(array_agg(v.slot_id) FILTER (WHERE v.slot_id IS NOT NULL), '{}') AS slots FROM participants p LEFT JOIN votes v ON v.participant_id=p.id WHERE p.event_id=$1 GROUP BY p.id ORDER BY p.created_at")
        .bind(event.id).fetch_all(pool).await.map_err(db)?;
    event.voting_ended = deadline_passed(event.voting_deadline, &event.timezone);
    Ok(sorted_json(&event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn sample(geo: Option<LocationGeo>, deadline: Option<NaiveDate>) -> EventView {
        let uuid = |n: u128| Uuid::from_u128(n);
        let time = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        EventView {
            id: uuid(1),
            title: "Cena <b>&</b> \"amici\" ☕".into(),
            description: Some("Prima riga\nSeconda".into()),
            location: "Via Roma 1, Venezia".into(),
            location_geo: geo.map(sqlx::types::Json),
            organizer_name: "Ada".into(),
            allow_multiple_choices: true,
            status: Status::Confirmed,
            confirmed_slot_id: Some(uuid(2)),
            tiebreak_slot_id: None,
            timezone: "Europe/Rome".into(),
            voting_deadline: deadline,
            voting_ended: true,
            slots: vec![
                SlotView {
                    id: uuid(2),
                    date: NaiveDate::from_ymd_opt(2027, 1, 10).unwrap(),
                    start: time(20, 0),
                    end: Some(time(22, 30)),
                },
                SlotView {
                    id: uuid(3),
                    date: NaiveDate::from_ymd_opt(2027, 1, 11).unwrap(),
                    start: time(19, 45),
                    end: None,
                },
            ],
            participants: vec![
                ParticipantView {
                    name: "Lucia".into(),
                    slots: vec![uuid(2), uuid(3)],
                },
                ParticipantView {
                    name: "Bob".into(),
                    slots: vec![],
                },
            ],
        }
    }

    /// The `json!` literal `get_event` built before it used `EventView`.
    fn legacy(e: &EventView) -> Value {
        let slots = e
            .slots
            .iter()
            .map(|r| json!({"id":r.id,"date":r.date,"start":r.start,"end":r.end}))
            .collect::<Vec<_>>();
        let people = e
            .participants
            .iter()
            .map(|r| json!({"name":r.name,"slots":r.slots}))
            .collect::<Vec<_>>();
        json!({"title":e.title,"description":e.description,"location":e.location,"location_geo":e.location_geo,"organizer_name":e.organizer_name,"allow_multiple_choices":e.allow_multiple_choices,"status":e.status.as_str(),"confirmed_slot_id":e.confirmed_slot_id,"tiebreak_slot_id":e.tiebreak_slot_id,"timezone":e.timezone,"voting_deadline":e.voting_deadline,"voting_ended":e.voting_ended,"slots":slots,"participants":people})
    }

    #[test]
    fn event_view_serializes_exactly_like_the_legacy_json() {
        let geo: LocationGeo = serde_json::from_value(json!({
            "latitude": 45.43, "longitude": 12.33, "kind": "venue",
            "attribution": "© OSM", "place_id": "51ab"
        }))
        .unwrap();
        let deadline = NaiveDate::from_ymd_opt(2027, 1, 9);
        for view in [sample(Some(geo), deadline), sample(None, None)] {
            let expected = serde_json::to_vec(&legacy(&view)).unwrap();
            let Json(actual) = sorted_json(&view);
            assert_eq!(
                String::from_utf8(serde_json::to_vec(&actual).unwrap()).unwrap(),
                String::from_utf8(expected).unwrap()
            );
            assert!(actual.get("id").is_none());
        }
    }
}
