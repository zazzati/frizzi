mod calendar;
mod mail;
mod observability;
mod places;
mod retention;
pub use observability::{init_tracing, monitor_public_urls};

use places::LocationGeo;

use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool, Postgres, Row, Transaction,
};
use std::{str::FromStr, time::Duration};
use uuid::Uuid;
use validator::ValidateEmail;

type Runtime = Option<Extension<vercel_runtime::AppState>>;
static MAIL_READY: tokio::sync::Notify = tokio::sync::Notify::const_new();

rust_i18n::i18n!("locales", fallback = "it");

#[derive(Debug)]
struct ApiError(StatusCode, &'static str);
type ApiResult = Result<Json<Value>, ApiError>;
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (self.0, Json(json!({"error": self.1}))).into_response();
        response.extensions_mut().insert(self.1);
        response
    }
}
fn error(code: StatusCode, message: &'static str) -> ApiError {
    ApiError(code, message)
}
fn bad() -> ApiError {
    error(StatusCode::BAD_REQUEST, "invalid_input")
}
#[track_caller]
fn db(e: sqlx::Error) -> ApiError {
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
fn unique_or_db(e: sqlx::Error) -> ApiError {
    if e.as_database_error()
        .is_some_and(|e| e.is_unique_violation())
    {
        error(StatusCode::CONFLICT, "email_exists")
    } else {
        db(e)
    }
}
fn missing() -> ApiError {
    error(StatusCode::NOT_FOUND, "not_found")
}
fn locale(s: &str) -> &str {
    if [
        "it", "en", "es", "zh", "hi", "ar", "fr", "bn", "pt", "ru", "de",
    ]
    .contains(&s)
    {
        s
    } else {
        "it"
    }
}
fn valid(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.chars().count() <= max
}
fn email(s: &str) -> Result<String, ApiError> {
    let normalized = s.trim().to_lowercase();
    if normalized.len() > 255 || !normalized.validate_email() {
        return Err(bad());
    }
    Ok(normalized)
}
fn valid_token(s: &str) -> bool {
    s.len() == 21
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

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
pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/api/ready", get(ready))
        .route(
            "/api/telemetry",
            post(observability::interaction).layer(DefaultBodyLimit::max(256)),
        )
        .route("/api/events", post(create))
        .route("/api/places/suggest", post(places::suggest))
        .route("/api/events/{public}", get(public_event))
        .route("/api/events/{public}/votes", post(vote))
        .route("/api/events/{public}/recover", post(recover_links))
        .route("/api/events/{public}/calendar.ics", get(calendar::download))
        .route(
            "/api/events/{public}/participant/{secret}",
            get(participant).put(edit_vote),
        )
        .route(
            "/api/events/{public}/admin/{secret}",
            get(admin).put(update_status),
        )
        .route(
            "/api/events/{public}/admin/{secret}/details",
            axum::routing::put(update_details),
        )
        .route(
            "/api/events/{public}/admin/{secret}/tiebreak",
            axum::routing::put(update_tiebreak),
        )
        .route(
            "/api/events/{public}/admin/{secret}/messages",
            post(message_participants),
        )
        .route("/api/cron/email", get(retry_mail))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn_with_state(
            pool.clone(),
            observability::request,
        ))
        .with_state(pool)
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
fn schedule_mail(pool: PgPool, runtime: Runtime) {
    if let Some(Extension(runtime)) = runtime {
        runtime.wait_until(async move { mail::dispatch(&pool, 25).await });
    } else {
        MAIL_READY.notify_one();
    }
}
async fn retry_mail(State(pool): State<PgPool>, headers: HeaderMap) -> ApiResult {
    let secret = std::env::var("CRON_SECRET")
        .ok()
        .filter(|s| s.len() >= 32)
        .ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "cron_unconfigured"))?;
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        != Some(format!("Bearer {secret}").as_str())
    {
        return Err(error(StatusCode::UNAUTHORIZED, "unauthorized"));
    }
    maintain(&pool).await.map_err(db)?;
    maintain_observability(&pool).await;
    Ok(Json(json!({"ok": true})))
}
async fn rate_limit(
    tx: &mut Transaction<'_, Postgres>,
    key: &str,
    limit: i32,
) -> Result<(), ApiError> {
    let count: i32 = sqlx::query_scalar("INSERT INTO request_limits (key) VALUES ($1) ON CONFLICT (key, hour) DO UPDATE SET count=request_limits.count+1 RETURNING count")
        .bind(key).fetch_one(&mut **tx).await.map_err(db)?;
    if count > limit {
        return Err(error(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct SlotInput {
    #[serde(default)]
    id: Option<Uuid>,
    date: NaiveDate,
    start: NaiveTime,
    end: Option<NaiveTime>,
}
#[derive(Deserialize)]
struct EventInput {
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
fn default_locale() -> String {
    "it".into()
}
fn validate_details(
    title: &str,
    description: &Option<String>,
    location: &str,
    organizer: &str,
    timezone: &str,
    slots: &[SlotInput],
) -> bool {
    let unique_dates: std::collections::HashSet<_> = slots.iter().map(|s| s.date).collect();
    valid(title, 255)
        && valid(location, 255)
        && valid(organizer, 100)
        && description
            .as_ref()
            .is_none_or(|s| s.chars().count() <= 10000)
        && timezone.parse::<chrono_tz::Tz>().is_ok()
        && !slots.is_empty()
        && slots.len() <= 100
        && unique_dates.len() == slots.len()
        && slots.iter().all(|s| s.end.is_none_or(|end| end > s.start))
}
fn deadline_passed(deadline: Option<NaiveDate>, timezone: &str) -> bool {
    deadline_passed_at(deadline, timezone, Utc::now())
}
fn deadline_passed_at(deadline: Option<NaiveDate>, timezone: &str, now: DateTime<Utc>) -> bool {
    deadline.is_some_and(|date| {
        let timezone = timezone.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC);
        now.with_timezone(&timezone).date_naive() > date
    })
}
fn validate_deadline(deadline: Option<NaiveDate>, timezone: &str) -> Result<(), ApiError> {
    validate_deadline_at(deadline, timezone, Utc::now())
}
fn validate_deadline_at(
    deadline: Option<NaiveDate>,
    timezone: &str,
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    if deadline.is_some_and(|date| !(1..=9999).contains(&date.year()))
        || deadline_passed_at(deadline, timezone, now)
    {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_deadline"));
    }
    Ok(())
}
async fn create(
    State(pool): State<PgPool>,
    runtime: Runtime,
    Json(e): Json<EventInput>,
) -> ApiResult {
    let organizer_email = email(&e.organizer_email)?;
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
    validate_deadline(e.voting_deadline, &e.timezone)?;
    if let Some(geo) = &e.location_geo {
        geo.validate()?;
    }
    let public = nanoid::nanoid!();
    let secret = nanoid::nanoid!();
    let mut tx = pool.begin().await.map_err(db)?;
    rate_limit(&mut tx, &format!("create:{organizer_email}"), 5).await?;
    let id: Uuid = sqlx::query_scalar("INSERT INTO events (public_token,admin_token,title,description,location,organizer_name,organizer_email,organizer_locale,allow_multiple_choices,timezone,voting_deadline,location_geo) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING id")
        .bind(&public).bind(&secret).bind(e.title.trim()).bind(&e.description).bind(e.location.trim())
        .bind(e.organizer_name.trim()).bind(&organizer_email).bind(locale(&e.locale))
        .bind(e.allow_multiple_choices).bind(&e.timezone).bind(e.voting_deadline).bind(e.location_geo.map(sqlx::types::Json)).fetch_one(&mut *tx).await.map_err(db)?;
    insert_slots(&mut tx, id, &e.slots).await?;
    mail::enqueue(
        &mut tx,
        id,
        &organizer_email,
        locale(&e.locale),
        "admin",
        &e.title,
        &format!("{}/{public}/admin/{secret}", mail::base_url()),
        None,
        None,
        None,
    )
    .await?;
    tx.commit().await.map_err(db)?;
    schedule_mail(pool, runtime);
    Ok(Json(json!({"public_token": public})))
}
type SlotColumns = (Vec<NaiveDate>, Vec<NaiveTime>, Vec<Option<NaiveTime>>);
fn slot_columns<'a>(slots: impl IntoIterator<Item = &'a SlotInput>) -> SlotColumns {
    let mut columns: SlotColumns = Default::default();
    for s in slots {
        columns.0.push(s.date);
        columns.1.push(s.start);
        columns.2.push(s.end);
    }
    columns
}
async fn insert_slots<'a>(
    tx: &mut Transaction<'_, Postgres>,
    event_id: Uuid,
    slots: impl IntoIterator<Item = &'a SlotInput>,
) -> Result<(), ApiError> {
    let (dates, starts, ends) = slot_columns(slots);
    if dates.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO event_slots (event_id,slot_date,start_time,end_time)
         SELECT $1,* FROM UNNEST($2::date[],$3::time[],$4::time[])",
    )
    .bind(event_id)
    .bind(dates)
    .bind(starts)
    .bind(ends)
    .execute(&mut **tx)
    .await
    .map_err(db)?;
    Ok(())
}
async fn public_event(State(pool): State<PgPool>, Path(public): Path<String>) -> ApiResult {
    get_event(&pool, &public).await
}
async fn get_event(pool: &PgPool, public: &str) -> ApiResult {
    if !valid_token(public) {
        return Err(missing());
    }
    let e = sqlx::query("SELECT id,title,description,location,organizer_name,allow_multiple_choices,status::text,confirmed_slot_id,tiebreak_slot_id,timezone,voting_deadline,location_geo FROM events WHERE public_token=$1")
        .bind(public).fetch_optional(pool).await.map_err(db)?.ok_or_else(missing)?;
    let id: Uuid = e.get("id");
    let slots = sqlx::query("SELECT id,slot_date,start_time,end_time FROM event_slots WHERE event_id=$1 ORDER BY slot_date")
        .bind(id).fetch_all(pool).await.map_err(db)?.iter().map(|r| json!({"id":r.get::<Uuid,_>("id"),"date":r.get::<NaiveDate,_>("slot_date"),"start":r.get::<NaiveTime,_>("start_time"),"end":r.get::<Option<NaiveTime>,_>("end_time")})).collect::<Vec<_>>();
    let people = sqlx::query("SELECT p.id,p.name,COALESCE(array_agg(v.slot_id) FILTER (WHERE v.slot_id IS NOT NULL), '{}') AS slots FROM participants p LEFT JOIN votes v ON v.participant_id=p.id WHERE p.event_id=$1 GROUP BY p.id ORDER BY p.created_at")
        .bind(id).fetch_all(pool).await.map_err(db)?.iter().map(|r| json!({"name":r.get::<String,_>("name"),"slots":r.get::<Vec<Uuid>,_>("slots")})).collect::<Vec<_>>();
    Ok(Json(
        json!({"title":e.get::<String,_>("title"),"description":e.get::<Option<String>,_>("description"),"location":e.get::<String,_>("location"),"location_geo":e.get::<Option<sqlx::types::Json<LocationGeo>>,_>("location_geo"),"organizer_name":e.get::<String,_>("organizer_name"),"allow_multiple_choices":e.get::<bool,_>("allow_multiple_choices"),"status":e.get::<String,_>("status"),"confirmed_slot_id":e.get::<Option<Uuid>,_>("confirmed_slot_id"),"tiebreak_slot_id":e.get::<Option<Uuid>,_>("tiebreak_slot_id"),"timezone":e.get::<String,_>("timezone"),"voting_deadline":e.get::<Option<NaiveDate>,_>("voting_deadline"),"voting_ended":deadline_passed(e.get("voting_deadline"),&e.get::<String,_>("timezone")),"slots":slots,"participants":people}),
    ))
}

#[derive(Deserialize)]
struct VoteInput {
    name: String,
    email: String,
    #[serde(default = "default_locale")]
    locale: String,
    slots: Vec<Uuid>,
}
async fn vote(
    State(pool): State<PgPool>,
    runtime: Runtime,
    Path(public): Path<String>,
    Json(v): Json<VoteInput>,
) -> ApiResult {
    let response = save_vote(&pool, &public, None, v).await?;
    schedule_mail(pool, runtime);
    Ok(response)
}
async fn edit_vote(
    State(pool): State<PgPool>,
    runtime: Runtime,
    Path((public, secret)): Path<(String, String)>,
    Json(v): Json<VoteInput>,
) -> ApiResult {
    let response = save_vote(&pool, &public, Some(&secret), v).await?;
    schedule_mail(pool, runtime);
    Ok(response)
}
async fn save_vote(pool: &PgPool, public: &str, secret: Option<&str>, v: VoteInput) -> ApiResult {
    let email = email(&v.email)?;
    if !valid_token(public) || secret.is_some_and(|s| !valid_token(s)) {
        return Err(missing());
    }
    if !valid(&v.name, 100) || v.slots.len() > 100 {
        return Err(bad());
    }
    let mut tx = pool.begin().await.map_err(db)?;
    let event = sqlx::query("SELECT id,title,allow_multiple_choices,status::text,timezone,voting_deadline,tiebreak_slot_id FROM events WHERE public_token=$1 FOR UPDATE")
        .bind(public).fetch_optional(&mut *tx).await.map_err(db)?.ok_or_else(missing)?;
    let id: Uuid = event.get("id");
    if event.get::<String, _>("status") != "OPEN" {
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
    rate_limit(&mut tx, &format!("vote:{id}:{email}"), 30).await?;
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
    mail::enqueue(
        &mut tx,
        id,
        &email,
        locale(&v.locale),
        "vote",
        &event.get::<String, _>("title"),
        &format!("{}/{public}/{edit_token}", mail::base_url()),
        Some(&summary),
        None,
        None,
    )
    .await?;
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"ok": true})))
}
#[derive(Deserialize)]
struct RecoveryInput {
    email: String,
}
async fn recover_links(
    State(pool): State<PgPool>,
    runtime: Runtime,
    Path(public): Path<String>,
    Json(input): Json<RecoveryInput>,
) -> ApiResult {
    let accepted = Json(json!({"ok": true}));
    let address = email(&input.email)?;
    if !valid_token(&public) {
        return Ok(accepted);
    }

    let mut tx = pool.begin().await.map_err(db)?;
    for (key, limit) in [
        (format!("recover-email:{address}"), 3),
        (format!("recover-event:{public}"), 30),
    ] {
        if let Err(err) = rate_limit(&mut tx, &key, limit).await {
            if err.0 == StatusCode::TOO_MANY_REQUESTS {
                return Ok(accepted);
            }
            return Err(err);
        }
    }
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
        mail::enqueue(
            &mut tx,
            event_id,
            &address,
            locale(&row.get::<String, _>("locale")),
            "recover",
            &row.get::<String, _>("title"),
            &url,
            None,
            None,
            None,
        )
        .await?;
    }
    tx.commit().await.map_err(db)?;
    if !matches.is_empty() {
        schedule_mail(pool, runtime);
    }
    Ok(accepted)
}

async fn participant(
    State(pool): State<PgPool>,
    Path((public, secret)): Path<(String, String)>,
) -> ApiResult {
    if !valid_token(&public) || !valid_token(&secret) {
        return Err(missing());
    }
    let p = sqlx::query("SELECT p.id,p.name,p.email,p.locale FROM participants p JOIN events e ON e.id=p.event_id WHERE e.public_token=$1 AND p.edit_token=$2")
        .bind(public).bind(secret).fetch_optional(&pool).await.map_err(db)?.ok_or_else(missing)?;
    let slots: Vec<Uuid> = sqlx::query_scalar("SELECT slot_id FROM votes WHERE participant_id=$1")
        .bind(p.get::<Uuid, _>("id"))
        .fetch_all(&pool)
        .await
        .map_err(db)?;
    Ok(Json(
        json!({"name":p.get::<String,_>("name"),"email":p.get::<String,_>("email"),"locale":p.get::<String,_>("locale"),"slots":slots}),
    ))
}
async fn admin(
    State(pool): State<PgPool>,
    Path((public, secret)): Path<(String, String)>,
) -> ApiResult {
    authorize_admin(&pool, &public, &secret).await?;
    get_event(&pool, &public).await
}
async fn authorize_admin(pool: &PgPool, public: &str, secret: &str) -> Result<(), ApiError> {
    if !valid_token(public) || !valid_token(secret) {
        return Err(missing());
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM events WHERE public_token=$1 AND admin_token=$2)",
    )
    .bind(public)
    .bind(secret)
    .fetch_one(pool)
    .await
    .map_err(db)?;
    if !exists {
        return Err(missing());
    }
    Ok(())
}
#[derive(Deserialize)]
struct StatusInput {
    status: String,
    confirmed_slot_id: Option<Uuid>,
    #[serde(default = "default_locale")]
    locale: String,
}
async fn update_status(
    State(pool): State<PgPool>,
    runtime: Runtime,
    Path((public, secret)): Path<(String, String)>,
    Json(v): Json<StatusInput>,
) -> Result<Response, ApiError> {
    let outcome = match v.status.as_str() {
        "OPEN" => "reopened",
        "CONFIRMED" => "confirmed",
        "CANCELLED" => "cancelled",
        _ => return Err(bad()),
    };
    if (v.status == "CONFIRMED") != v.confirmed_slot_id.is_some() {
        return Err(bad());
    }
    authorize_admin(&pool, &public, &secret).await?;
    let confirmation = match v.confirmed_slot_id {
        Some(slot) => mail::prepare_confirmation(&pool, &public, &secret, slot, &v.locale).await,
        None => None,
    };
    let mut tx = pool.begin().await.map_err(db)?;
    let e = sqlx::query("SELECT id,status::text,confirmed_slot_id FROM events WHERE public_token=$1 AND admin_token=$2 FOR UPDATE")
        .bind(&public).bind(&secret).fetch_optional(&mut *tx).await.map_err(db)?.ok_or_else(missing)?;
    let id: Uuid = e.get("id");
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
    sqlx::query("UPDATE events SET status=$1::event_status,confirmed_slot_id=$2,organizer_locale=$3,updated_at=now(),
       tiebreak_slot_id=CASE WHEN ($1='CONFIRMED' AND tiebreak_slot_id=$2) OR ($1='OPEN' AND status='OPEN')
                           THEN tiebreak_slot_id ELSE NULL END WHERE id=$4")
        .bind(&v.status).bind(v.confirmed_slot_id).bind(locale(&v.locale)).bind(id).execute(&mut *tx).await.map_err(db)?;
    let changed = e.get::<String, _>("status") != v.status
        || e.get::<Option<Uuid>, _>("confirmed_slot_id") != v.confirmed_slot_id;
    if changed {
        mail::notify(&mut tx, id, &v.status, None, confirmation.as_ref()).await?;
    }
    tx.commit().await.map_err(db)?;
    schedule_mail(pool, runtime);
    let mut response = Json(json!({"ok": true})).into_response();
    response
        .extensions_mut()
        .insert(if changed { outcome } else { "unchanged" });
    Ok(response)
}

async fn can_break_tie(
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

#[derive(Deserialize)]
struct TiebreakInput {
    slot_id: Option<Uuid>,
}
async fn update_tiebreak(
    State(pool): State<PgPool>,
    Path((public, secret)): Path<(String, String)>,
    Json(input): Json<TiebreakInput>,
) -> ApiResult {
    if !valid_token(&public) || !valid_token(&secret) {
        return Err(missing());
    }
    let mut tx = pool.begin().await.map_err(db)?;
    let event = sqlx::query(
        "SELECT id,status::text FROM events WHERE public_token=$1 AND admin_token=$2 FOR UPDATE",
    )
    .bind(public)
    .bind(secret)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?
    .ok_or_else(missing)?;
    let id = event.get("id");
    if event.get::<String, _>("status") != "OPEN" {
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
struct ParticipantMessage {
    request_id: Uuid,
    message: String,
}
async fn message_participants(
    State(pool): State<PgPool>,
    runtime: Runtime,
    Path((public, secret)): Path<(String, String)>,
    Json(input): Json<ParticipantMessage>,
) -> ApiResult {
    if !valid_token(&public) || !valid_token(&secret) {
        return Err(missing());
    }
    if !valid(&input.message, 2000) {
        return Err(bad());
    }
    let mut tx = pool.begin().await.map_err(db)?;
    let event = sqlx::query(
        "SELECT id,title FROM events WHERE public_token=$1 AND admin_token=$2 FOR UPDATE",
    )
    .bind(&public)
    .bind(&secret)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?
    .ok_or_else(missing)?;
    let id: Uuid = event.get("id");
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
    rate_limit(&mut tx, &format!("participant-message:{id}"), 5).await?;
    let queued = mail::enqueue_participant_message(
        &mut tx,
        id,
        &public,
        &event.get::<String, _>("title"),
        input.message.trim(),
    )
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
struct DetailsInput {
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
async fn update_details(
    State(pool): State<PgPool>,
    runtime: Runtime,
    Path((public, secret)): Path<(String, String)>,
    Json(v): Json<DetailsInput>,
) -> ApiResult {
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
    authorize_admin(&pool, &public, &secret).await?;
    let mut tx = pool.begin().await.map_err(db)?;
    let old = sqlx::query("SELECT id,title,description,location,organizer_name,allow_multiple_choices,timezone,voting_deadline,location_geo FROM events WHERE public_token=$1 AND admin_token=$2 FOR UPDATE")
        .bind(public).bind(secret).fetch_optional(&mut *tx).await.map_err(db)?.ok_or_else(missing)?;
    let id: Uuid = old.get("id");
    let old_location: String = old.get("location");
    let old_geo = old
        .get::<Option<sqlx::types::Json<LocationGeo>>, _>("location_geo")
        .map(|geo| geo.0);
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
    let old_deadline: Option<NaiveDate> = old.get("voting_deadline");
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
    let mut changes: Vec<(String, String, String)> = Vec::new();
    for (key, before, after) in [
        (
            "title",
            old.get::<String, _>("title"),
            v.title.trim().to_string(),
        ),
        (
            "description",
            old.get::<Option<String>, _>("description")
                .unwrap_or_default(),
            v.description.clone().unwrap_or_default(),
        ),
        (
            "location",
            old_location.clone(),
            v.location.trim().to_string(),
        ),
        (
            "organizer",
            old.get::<String, _>("organizer_name"),
            v.organizer_name.trim().to_string(),
        ),
        (
            "timezone",
            old.get::<String, _>("timezone"),
            v.timezone.clone(),
        ),
        (
            "votingDeadline",
            old_deadline
                .map(|date| date.to_string())
                .unwrap_or_default(),
            deadline.map(|date| date.to_string()).unwrap_or_default(),
        ),
        (
            "multiple",
            old.get::<bool, _>("allow_multiple_choices").to_string(),
            v.allow_multiple_choices.to_string(),
        ),
    ] {
        if before != after {
            changes.push((key.into(), before, after));
        }
    }
    if old_location == v.location.trim() && geo != old_geo {
        changes.push((
            "location".into(),
            old_geo.as_ref().map_or_else(
                || old_location.clone(),
                |geo| geo.with_coordinates(&old_location),
            ),
            geo.as_ref().map_or_else(
                || old_location.clone(),
                |geo| geo.with_coordinates(&old_location),
            ),
        ));
    }
    if dates_changed || old.get::<String, _>("timezone") != v.timezone {
        // A changed date/time is a new availability question; preserve only unaffected votes.
        sqlx::query("UPDATE events SET status='OPEN',confirmed_slot_id=NULL,tiebreak_slot_id=NULL WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        if old.get::<String, _>("timezone") != v.timezone {
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
    if dates_changed || !changes.is_empty() {
        mail::notify(
            &mut tx,
            id,
            "changed",
            Some((&changes, &old_slots, dates_changed)),
            None,
        )
        .await?;
    }
    tx.commit().await.map_err(db)?;
    schedule_mail(pool, runtime);
    Ok(Json(json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_rejects_duplicate_days_invalid_timezones_and_time_ranges() {
        let mut slots = vec![SlotInput {
            id: None,
            date: NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(),
            start: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            end: None,
        }];
        assert!(validate_details(
            "Cena",
            &None,
            "Roma",
            "Ada",
            "Europe/Rome",
            &slots
        ));
        assert!(!validate_details(
            "Cena",
            &None,
            "Roma",
            "Ada",
            "Mars/City",
            &slots
        ));
        slots.push(slots[0].clone());
        assert!(!validate_details(
            "Cena",
            &None,
            "Roma",
            "Ada",
            "Europe/Rome",
            &slots
        ));
        slots.pop();
        slots[0].end = Some(NaiveTime::from_hms_opt(19, 0, 0).unwrap());
        assert!(!validate_details(
            "Cena",
            &None,
            "Roma",
            "Ada",
            "Europe/Rome",
            &slots
        ));
    }
    #[test]
    fn slot_columns_keep_rows_aligned() {
        let time = |h| NaiveTime::from_hms_opt(h, 0, 0).unwrap();
        let slots = [
            SlotInput {
                id: None,
                date: NaiveDate::from_ymd_opt(2027, 1, 2).unwrap(),
                start: time(20),
                end: Some(time(22)),
            },
            SlotInput {
                id: None,
                date: NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(),
                start: time(19),
                end: None,
            },
        ];
        let (dates, starts, ends) = slot_columns(&slots);
        assert_eq!(dates, [slots[0].date, slots[1].date]);
        assert_eq!(starts, [time(20), time(19)]);
        assert_eq!(ends, [Some(time(22)), None]);
        assert_eq!(slot_columns(&[]), Default::default());
    }
    #[test]
    fn tokens_and_locales() {
        let token = nanoid::nanoid!();
        assert!(valid_token(&token));
        assert!(!valid_token("not-a-token"));
        assert_eq!(locale("ar"), "ar");
        assert_eq!(locale("xx"), "it");
        assert_eq!(email(" ADA@example.com ").unwrap(), "ada@example.com");
    }

    fn day(y: i32, m: u32, d: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(y, m, d)
    }

    fn utc(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    #[test]
    fn deadline_closes_after_its_day_ends_in_the_event_timezone() {
        let deadline = day(2027, 1, 10);
        let now = utc("2027-01-10T12:00:00Z");
        // Auckland (UTC+13) is already on Jan 11; Los Angeles (UTC-8) is still on Jan 10.
        assert!(deadline_passed_at(deadline, "Pacific/Auckland", now));
        assert!(!deadline_passed_at(deadline, "America/Los_Angeles", now));
        assert!(!deadline_passed_at(deadline, "UTC", now));
        assert!(!deadline_passed_at(
            deadline,
            "UTC",
            utc("2027-01-10T23:59:59Z")
        ));
        assert!(deadline_passed_at(
            deadline,
            "UTC",
            utc("2027-01-11T00:00:00Z")
        ));
        // Rome is UTC+1 in January: Jan 10 23:00Z is already Jan 11 there.
        assert!(deadline_passed_at(
            deadline,
            "Europe/Rome",
            utc("2027-01-10T23:00:00Z")
        ));
        assert!(!deadline_passed_at(
            deadline,
            "Europe/Rome",
            utc("2027-01-10T22:59:59Z")
        ));
        // Unknown zones fall back to UTC; no deadline never passes.
        assert!(!deadline_passed_at(deadline, "Mars/City", now));
        assert!(!deadline_passed_at(
            None,
            "UTC",
            utc("9999-12-31T00:00:00Z")
        ));
    }

    #[test]
    fn deadline_validation_rejects_past_and_out_of_range_dates() {
        let now = utc("2027-01-10T12:00:00Z");
        assert!(validate_deadline_at(None, "UTC", now).is_ok());
        assert!(validate_deadline_at(day(2027, 1, 10), "UTC", now).is_ok());
        assert!(validate_deadline_at(day(9999, 12, 31), "UTC", now).is_ok());
        assert!(validate_deadline_at(day(2027, 1, 9), "UTC", now).is_err());
        assert!(validate_deadline_at(day(2027, 1, 10), "Pacific/Auckland", now).is_err());
        assert!(validate_deadline_at(day(10000, 1, 1), "UTC", now).is_err());
        assert!(validate_deadline_at(day(0, 1, 1), "UTC", now).is_err());
    }

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
        assert_eq!(set.voting_deadline, Some(day(2027, 1, 10)));
        assert!(matches!(set.location_geo, Some(Some(_))));
    }
}
