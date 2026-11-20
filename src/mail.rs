use crate::observability::{db_class, upstream_class};
use crate::{db, locale, places::LocationGeo, ApiError, SlotInput};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{Locale, NaiveDate};
use lettre::message::{header::ContentType, Attachment, MultiPart, SinglePart};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::{collections::HashMap, sync::LazyLock, time::Duration};
use uuid::Uuid;

mod invitation;
mod template;
use template::Details;

static GEMINI: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);
static RESEND: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

pub struct ConfirmationExtras {
    geo: Option<Json<LocationGeo>>,
    website: Option<reqwest::Url>,
    intros: HashMap<String, String>,
}

pub async fn prepare_confirmation(
    pool: &PgPool,
    public: &str,
    secret: &str,
    slot: Uuid,
    organizer_locale: &str,
) -> Option<ConfirmationExtras> {
    let event = sqlx::query(
        "SELECT location_geo, ARRAY(SELECT DISTINCT locale FROM participants WHERE event_id=events.id) AS languages
         FROM events WHERE public_token=$1 AND admin_token=$2
         AND (status<>'CONFIRMED' OR confirmed_slot_id IS DISTINCT FROM $3)
         AND EXISTS (SELECT 1 FROM event_slots WHERE id=$3 AND event_id=events.id)",
    )
    .bind(public)
    .bind(secret)
    .bind(slot)
    .fetch_optional(pool)
    .await
    .inspect_err(|error| tracing::warn!(event = "confirmation_enrichment_fallback",
        reason = db_class(error)))
    .ok()??;
    let geo: Option<Json<LocationGeo>> = event.get("location_geo");
    let languages: Vec<String> = event.get("languages");
    let mut languages: Vec<_> = languages.iter().map(|lang| locale(lang)).collect();
    languages.push(locale(organizer_locale));
    languages.sort_unstable();
    languages.dedup();
    let (intros, website) = tokio::join!(generate_intros(&languages), async {
        geo.as_ref()?.venue_website().await
    });
    Some(ConfirmationExtras {
        geo,
        website,
        intros: intros.unwrap_or_default(),
    })
}

async fn generate_intros(languages: &[&str]) -> Option<HashMap<String, String>> {
    let key = std::env::var("GEMINI_API_KEY").ok()?;
    if key.trim().is_empty() {
        return None;
    }
    let properties: serde_json::Map<_, _> = languages
        .iter()
        .map(|&lang| (lang.to_owned(), serde_json::json!({"type":"string"})))
        .collect();
    let request = GEMINI
        .post("https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-flash-lite:generateContent")
        .header(reqwest::header::USER_AGENT, "Frizzi/1.0")
        .header("x-goog-api-key", key.trim())
        .timeout(Duration::from_secs(7))
        .json(&serde_json::json!({
            "contents": [{"parts":[{"text": format!(
                "Write one short, upbeat sentence for a Frizzi invitation in each of these languages: {}. It follows a headline that already celebrates the confirmed date, so don't repeat that. Sound like a friend excited to get everyone together. Example tone in Italian: 'Tieni un po’ di spazio in agenda per due chiacchiere e un bel po’ di risate!' Write naturally in each language. Avoid formal announcements, clichés, invented plans, names, links, emoji, and formatting. Return a JSON object keyed by the given language codes.",
                languages.join(", ")
            )}]}],
            "generationConfig": {
                "thinkingConfig": {"thinkingLevel": "minimal"},
                "maxOutputTokens": 100 * languages.len(),
                "responseMimeType": "application/json",
                "responseSchema": {"type":"object", "properties":properties, "required":languages}
            }
        }));
    let response = match request
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(
                event = "gemini_fallback",
                reason = upstream_class(&error),
                upstream_status = error.status().map(|status| status.as_u16())
            );
            return None;
        }
    };
    let intros = response
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|json| parse_intros(&json, languages));
    let generated = intros.as_ref().map_or(0, HashMap::len);
    if generated < languages.len() {
        tracing::warn!(
            event = "gemini_fallback",
            reason = "missing_or_invalid_text",
            fallback_languages = languages.len() - generated
        );
    }
    intros
}

fn parse_intros(
    response: &serde_json::Value,
    languages: &[&str],
) -> Option<HashMap<String, String>> {
    let candidate = &response["candidates"][0];
    if candidate["finishReason"] != "STOP" {
        return None;
    }
    let part = candidate["content"]["parts"]
        .as_array()?
        .iter()
        .find(|part| part["thought"] != true && part["text"].is_string())?;
    let mut intros: HashMap<String, String> = serde_json::from_str(part["text"].as_str()?).ok()?;
    intros.retain(|lang, text| {
        *text = text.trim().to_owned();
        languages.contains(&lang.as_str())
            && (10..=300).contains(&text.chars().count())
            && !text.contains(['/', '@', '<', '>', '\r', '\n'])
            && !text.to_ascii_lowercase().contains("www.")
    });
    Some(intros)
}

pub fn base_url() -> String {
    std::env::var("APP_URL")
        .expect("APP_URL required")
        .trim_end_matches('/')
        .to_string()
}
fn date_locale(lang: &str) -> Locale {
    match lang {
        "en" => Locale::en_GB,
        "es" => Locale::es_ES,
        "zh" => Locale::zh_CN,
        "hi" => Locale::hi_IN,
        "ar" => Locale::ar_SA,
        "fr" => Locale::fr_FR,
        "bn" => Locale::bn_BD,
        "pt" => Locale::pt_PT,
        "ru" => Locale::ru_RU,
        "de" => Locale::de_DE,
        _ => Locale::it_IT,
    }
}
fn numeric_date(date: NaiveDate, lang: &str) -> String {
    let pattern = match lang {
        "de" | "ru" => "%d.%m.%Y",
        "zh" => "%Y/%m/%d",
        _ => "%d/%m/%Y",
    };
    let text = date.format(pattern).to_string();
    let zero = match lang {
        "ar" => '٠',
        "bn" => '০',
        _ => return text,
    };
    text.chars()
        .map(|c| {
            c.to_digit(10)
                .and_then(|n| char::from_u32(zero as u32 + n))
                .unwrap_or(c)
        })
        .collect()
}
fn format_slots(slots: &[SlotInput], lang: &str) -> String {
    let loc = date_locale(lang);
    slots
        .iter()
        .map(|s| {
            let mut text = format!(
                "{} · {} · {}",
                s.date.format_localized("%A", loc),
                numeric_date(s.date, lang),
                s.date
                    .and_time(s.start)
                    .and_utc()
                    .format_localized("%X", loc)
            );
            if let Some(end) = s.end {
                text.push_str(&format!(
                    " – {}",
                    s.date.and_time(end).and_utc().format_localized("%X", loc)
                ));
            }
            text
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub async fn summary(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    lang: &str,
) -> Result<Details, ApiError> {
    let e = sqlx::query("SELECT description,location,location_geo,timezone,confirmed_slot_id FROM events WHERE id=$1")
        .bind(event).fetch_one(&mut **tx).await.map_err(db)?;
    let rows = sqlx::query("SELECT id,slot_date,start_time,end_time FROM event_slots WHERE event_id=$1 ORDER BY slot_date")
        .bind(event).fetch_all(&mut **tx).await.map_err(db)?;
    let slots: Vec<_> = rows
        .iter()
        .map(|s| SlotInput {
            id: Some(s.get("id")),
            date: s.get("slot_date"),
            start: s.get("start_time"),
            end: s.get("end_time"),
        })
        .collect();
    let confirmed: Option<Uuid> = e.get("confirmed_slot_id");
    let dates = if let Some(confirmed) = confirmed {
        format_slots(
            &slots
                .into_iter()
                .filter(|s| s.id == Some(confirmed))
                .collect::<Vec<_>>(),
            lang,
        )
    } else {
        format_slots(&slots, lang)
    };
    let location: String = e.get("location");
    let geo: Option<Json<LocationGeo>> = e.get("location_geo");
    let directions = directions_url(&location, geo.as_deref());
    Ok(Details {
        description: e.get("description"),
        location,
        directions,
        timezone: e.get("timezone"),
        dates,
        confirmed: confirmed.is_some(),
        website: None,
        calendar: None,
        notes: String::new(),
    })
}

fn directions_url(location: &str, geo: Option<&LocationGeo>) -> reqwest::Url {
    let destination = geo.and_then(LocationGeo::directions_destination);
    let mut url = reqwest::Url::parse("https://www.google.com/maps/dir/").unwrap();
    url.query_pairs_mut()
        .append_pair("api", "1")
        .append_pair("destination", destination.as_deref().unwrap_or(location));
    url
}

struct Composed {
    subject: String,
    body: String,
    html: String,
}

fn compose(
    lang: &str,
    kind: &str,
    title: &str,
    url: &str,
    details: Option<&Details>,
    intro: Option<&str>,
) -> Composed {
    let subject_key = format!("mail.{kind}Subject");
    let body_key = format!("mail.{kind}Body");
    let headline = rust_i18n::t!(&subject_key, locale = lang);
    let icon = match kind {
        "confirmed" => "🎉",
        "cancelled" => "📌",
        "open" => "🗳️",
        "changed" => "📣",
        "vote" => "✅",
        _ => "🔑",
    };
    let default_intro = rust_i18n::t!(&body_key, locale = lang);
    let intro = intro.unwrap_or(&default_intro);
    Composed {
        subject: format!("{icon} {headline} · {}", title.replace(['\r', '\n'], " ")),
        body: template::plain(lang, kind, &headline, title, intro, url, details, None),
        html: template::html(lang, kind, &headline, title, intro, url, details, None),
    }
}

pub async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    to: &str,
    lang: &str,
    kind: &str,
    title: &str,
    url: &str,
    details: Option<&Details>,
    intro: Option<&str>,
    invitation: Option<Uuid>,
) -> Result<(), ApiError> {
    let Composed {
        subject,
        body,
        html,
    } = compose(lang, kind, title, url, details, intro);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO email_outbox (recipient,subject,body,html,event_id,invitation_id) VALUES ($1,$2,$3,$4,$5,$6) RETURNING id",
    )
    .bind(to)
    .bind(subject)
    .bind(body)
    .bind(html)
    .bind(event)
    .bind(invitation)
    .fetch_one(&mut **tx)
    .await
    .map_err(db)?;
    tracing::info!(event = "email_prepared", mail_id = %id, kind);
    Ok(())
}

pub async fn enqueue_participant_message(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    public: &str,
    title: &str,
    input: &str,
) -> Result<u64, ApiError> {
    let message = template::MessageText::parse(input).ok_or_else(crate::bad)?;
    let languages: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT locale FROM participants WHERE event_id=$1")
            .bind(event)
            .fetch_all(&mut **tx)
            .await
            .map_err(db)?;
    let url = format!("{}/{public}", base_url());
    let clean_title = title.replace(['\r', '\n'], " ");
    let payloads: Vec<_> = languages.iter().map(|original| {
        let lang = locale(original);
        let headline = rust_i18n::t!("mail.messageSubject", locale = lang);
        let intro = rust_i18n::t!("mail.messageBody", locale = lang);
        serde_json::json!({
            "locale": original,
            "subject": format!("💌 {headline} · {clean_title}"),
            "body": template::plain(lang, "message", &headline, title, &intro, &url, None, Some(&message)),
            "html": template::html(lang, "message", &headline, title, &intro, &url, None, Some(&message)),
        })
    }).collect();
    let result = sqlx::query(
        "INSERT INTO email_outbox (event_id,recipient,subject,body,html)
         SELECT p.event_id,p.email,m.subject,m.body,m.html FROM participants p
         JOIN jsonb_to_recordset($2) AS m(locale text,subject text,body text,html text)
           ON m.locale=p.locale WHERE p.event_id=$1",
    )
    .bind(event)
    .bind(Json(payloads))
    .execute(&mut **tx)
    .await
    .map_err(db)?;
    Ok(result.rows_affected())
}
type Changes<'a> = Option<(&'a [(String, String, String)], &'a [SlotInput], bool)>;
pub async fn notify(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    kind: &str,
    changes: Changes<'_>,
    confirmation: Option<&ConfirmationExtras>,
) -> Result<(), ApiError> {
    let e = sqlx::query("SELECT public_token,admin_token,title,organizer_name,organizer_email,organizer_locale,location_geo FROM events WHERE id=$1")
        .bind(event).fetch_one(&mut **tx).await.map_err(db)?;
    let current_geo: Option<Json<LocationGeo>> = e.get("location_geo");
    let website = confirmation
        .filter(|extra| extra.geo.as_deref() == current_geo.as_deref())
        .and_then(|extra| extra.website.as_ref());
    let public: String = e.get("public_token");
    let participants =
        sqlx::query("SELECT email,locale,edit_token FROM participants WHERE event_id=$1")
            .bind(event)
            .fetch_all(&mut **tx)
            .await
            .map_err(db)?;
    let mut recipients: Vec<(String, String, String)> = participants
        .iter()
        .map(|p| {
            (
                p.get("email"),
                p.get("locale"),
                format!(
                    "{}/{}/{}",
                    base_url(),
                    public,
                    p.get::<String, _>("edit_token")
                ),
            )
        })
        .collect();
    recipients.push((
        e.get("organizer_email"),
        e.get("organizer_locale"),
        format!(
            "{}/{}/admin/{}",
            base_url(),
            public,
            e.get::<String, _>("admin_token")
        ),
    ));
    let kind = match kind {
        "CONFIRMED" => "confirmed",
        "CANCELLED" => "cancelled",
        "OPEN" => "open",
        _ => "changed",
    };
    let title: String = e.get("title");
    let organizer: String = e.get("organizer_name");
    // Everything except the recipient's link depends only on the language, so the summary
    // queries, change notes and invitation snapshot are built once per language.
    let mut languages: HashMap<&str, (Details, Option<Uuid>)> = HashMap::new();
    let mut messages = Vec::with_capacity(recipients.len());
    for (email, lang, url) in &recipients {
        let lang = locale(lang);
        let intro = confirmation
            .and_then(|extra| extra.intros.get(lang))
            .map(String::as_str);
        if !languages.contains_key(lang) {
            let mut details = summary(tx, event, lang).await?;
            let invitation_id = if kind == "confirmed" {
                details.website = website.cloned();
                details.calendar = Some(format!("{}/api/events/{public}/calendar.ics", base_url()));
                invitation::enqueue(tx, event, lang, &title, &organizer, intro, &details).await?
            } else {
                None
            };
            if let Some((changes, before_slots, dates_changed)) = changes {
                details.notes = change_notes(changes, before_slots, dates_changed, lang);
            }
            languages.insert(lang, (details, invitation_id));
        }
        let (details, invitation_id) = &languages[lang];
        let message = compose(lang, kind, &title, url, Some(details), intro);
        messages.push(serde_json::json!({
            "recipient": email,
            "subject": message.subject,
            "body": message.body,
            "html": message.html,
            "invitation_id": invitation_id,
        }));
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "INSERT INTO email_outbox (recipient,subject,body,html,event_id,invitation_id)
         SELECT m.recipient,m.subject,m.body,m.html,$1,m.invitation_id
         FROM jsonb_to_recordset($2)
           AS m(recipient text,subject text,body text,html text,invitation_id uuid)
         RETURNING id",
    )
    .bind(event)
    .bind(Json(messages))
    .fetch_all(&mut **tx)
    .await
    .map_err(db)?;
    for id in ids {
        tracing::info!(event = "email_prepared", mail_id = %id, kind);
    }
    Ok(())
}

fn change_notes(
    changes: &[(String, String, String)],
    before_slots: &[SlotInput],
    dates_changed: bool,
    lang: &str,
) -> String {
    let mut notes = format!("{}\n", rust_i18n::t!("mail.changes", locale = lang));
    for (key, before, after) in changes {
        let label_key = format!("ui.{key}");
        let label = rust_i18n::t!(&label_key, locale = lang);
        let value = |v: &str| -> String {
            if key == "multiple" {
                rust_i18n::t!(if v == "true" { "ui.yes" } else { "ui.no" }, locale = lang)
                    .to_string()
            } else if key == "votingDeadline" {
                chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d")
                    .map_or_else(|_| "—".to_string(), |date| numeric_date(date, lang))
            } else {
                v.to_string()
            }
        };
        notes.push_str(&format!(
            "{}\n{}: {}\n{}: {}\n",
            label,
            rust_i18n::t!("mail.before", locale = lang),
            value(before),
            rust_i18n::t!("mail.after", locale = lang),
            value(after)
        ));
    }
    if dates_changed {
        notes.push_str(&format!(
            "\n{}:\n{}\n",
            rust_i18n::t!("mail.before", locale = lang),
            format_slots(before_slots, lang)
        ));
    }
    if dates_changed || changes.iter().any(|(key, _, _)| key == "timezone") {
        notes.push_str(&format!(
            "\n{}\n",
            rust_i18n::t!("ui.datesChangedHint", locale = lang)
        ));
    }
    notes
}
#[derive(Debug)]
struct DeliveryError {
    reason: &'static str,
    status: Option<u16>,
}

impl From<&'static str> for DeliveryError {
    fn from(reason: &'static str) -> Self {
        Self {
            reason,
            status: None,
        }
    }
}

async fn deliver(
    id: Uuid,
    to: &str,
    subject: &str,
    body: &str,
    html: Option<&str>,
    pdf: Option<Vec<u8>>,
) -> Result<(), DeliveryError> {
    let from = std::env::var("MAIL_FROM").map_err(|_| "missing_mail_from")?;
    let smtp_host = std::env::var("SMTP_HOST")
        .ok()
        .filter(|host| !host.is_empty());
    if let Some(host) = smtp_host {
        let port = std::env::var("SMTP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1025);
        let message = Message::builder()
            .from(from.parse().map_err(|_| "invalid_mail_from")?)
            .to(to.parse().map_err(|_| "invalid_recipient")?)
            .subject(subject);
        let message = if let Some(pdf) = pdf {
            let mixed = MultiPart::mixed();
            let mixed = if let Some(html) = html {
                mixed.multipart(MultiPart::alternative_plain_html(
                    body.to_owned(),
                    html.to_owned(),
                ))
            } else {
                mixed.singlepart(SinglePart::plain(body.to_owned()))
            };
            let pdf = Attachment::new(invitation::FILENAME.to_owned()).body(
                pdf,
                ContentType::parse("application/pdf").map_err(|_| "invalid_pdf_content_type")?,
            );
            message.multipart(mixed.singlepart(pdf))
        } else if let Some(html) = html {
            message.multipart(MultiPart::alternative_plain_html(
                body.to_owned(),
                html.to_owned(),
            ))
        } else {
            message
                .header(ContentType::TEXT_PLAIN)
                .body(body.to_owned())
        }
        .map_err(|_| "invalid_smtp_message")?;
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
            .port(port)
            .timeout(Some(Duration::from_secs(3)))
            .build()
            .send(message)
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    "smtp_timeout"
                } else if error.is_transient() {
                    "smtp_transient"
                } else if error.is_permanent() {
                    "smtp_rejected"
                } else {
                    "smtp_transport"
                }
            })?;
    } else {
        let key = std::env::var("RESEND_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty())
            .ok_or("missing_resend_key")?;
        let mut payload = serde_json::json!({"from":from,"to":[to],"subject":subject,"text":body});
        if let Some(html) = html {
            payload["html"] = serde_json::json!(html);
        }
        if let Some(pdf) = pdf {
            payload["attachments"] = serde_json::json!([{
                "filename": invitation::FILENAME,
                "content": STANDARD.encode(pdf)
            }]);
        }
        RESEND
            .post("https://api.resend.com/emails")
            .timeout(Duration::from_secs(3))
            .bearer_auth(key)
            .header("Idempotency-Key", format!("frizzi/{id}"))
            .json(&payload)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| DeliveryError {
                reason: upstream_class(&error),
                status: error.status().map(|status| status.as_u16()),
            })?;
    }
    Ok(())
}
pub async fn dispatch(pool: &PgPool, limit: usize) {
    for _ in 0..limit {
        let message = sqlx::query("UPDATE email_outbox SET locked_until=now()+interval '2 minutes',attempts=attempts+1 WHERE id=(SELECT id FROM email_outbox WHERE sent_at IS NULL AND available_at<=now() AND (locked_until IS NULL OR locked_until<now()) ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING id,recipient,subject,body,html,invitation_id,attempts")
            .fetch_optional(pool).await;
        let row = match message {
            Ok(Some(row)) => row,
            Ok(None) => break,
            Err(error) => {
                tracing::error!(event = "mail_queue_claim_failed", reason = db_class(&error));
                break;
            }
        };
        let id: Uuid = row.get("id");
        let attempts: i32 = row.get("attempts");
        let start = std::time::Instant::now();
        let html: Option<String> = row.get("html");
        let invitation: Option<Uuid> = row.get("invitation_id");
        let pdf = match invitation {
            Some(invitation) => invitation::attachment(pool, invitation).await,
            None => Ok(None),
        };
        let result = match pdf {
            Ok(pdf) => {
                deliver(
                    id,
                    &row.get::<String, _>("recipient"),
                    &row.get::<String, _>("subject"),
                    &row.get::<String, _>("body"),
                    html.as_deref(),
                    pdf,
                )
                .await
            }
            Err(error) => {
                tracing::error!(event = "invitation_load_failed", mail_id = %id, reason = error.reason());
                Err("invitation_load_failed".into())
            }
        };
        let duration_ms = start.elapsed().as_millis() as u64;
        if let Err(error) = result {
            tracing::warn!(event = "mail_deferred", mail_id = %id, attempts, duration_ms,
                reason = error.reason, upstream_status = error.status);
            if let Err(error) = sqlx::query("UPDATE email_outbox SET locked_until=NULL,available_at=now()+make_interval(secs=>LEAST(3600,30*attempts)) WHERE id=$1").bind(id).execute(pool).await {
                tracing::error!(event = "mail_retry_schedule_failed", mail_id = %id, reason = db_class(&error));
            }
        } else {
            // Remove bearer links and addresses after successful delivery.
            match sqlx::query("UPDATE email_outbox SET sent_at=now(),locked_until=NULL,body='',html=NULL,recipient='',subject='',invitation_id=NULL WHERE id=$1")
                .bind(id).execute(pool).await {
                Err(error) => tracing::error!(event = "mail_receipt_write_failed", mail_id = %id, reason = db_class(&error)),
                Ok(_) => tracing::info!(event = "mail_accepted", mail_id = %id, attempts, duration_ms),
            }
            if let Some(invitation) = invitation {
                // Drop the rendered PDF as soon as its last recipient has it; cleanup() catches races.
                if let Err(error) = sqlx::query("DELETE FROM confirmation_invitations WHERE id=$1 AND NOT EXISTS (SELECT 1 FROM email_outbox WHERE invitation_id=$1)")
                    .bind(invitation).execute(pool).await {
                    tracing::error!(event = "maintenance_cleanup_failed", operation = "invitation", reason = db_class(&error));
                }
            }
        }
    }
}

/// Periodic housekeeping, run by the maintenance worker and the cron rather than after every write.
pub async fn cleanup(pool: &PgPool) {
    for (operation, query) in [
        ("invitation", "DELETE FROM confirmation_invitations WHERE NOT EXISTS (SELECT 1 FROM email_outbox WHERE invitation_id=confirmation_invitations.id)"),
        ("sent_mail", "DELETE FROM email_outbox WHERE sent_at<now()-interval '7 days'"),
        ("rate_limit", "DELETE FROM request_limits WHERE hour<now()-interval '2 hours'"),
    ] {
        if let Err(error) = sqlx::query(query).execute(pool).await {
            tracing::error!(event = "maintenance_cleanup_failed", operation, reason = db_class(&error));
        }
    }
}

pub async fn inspect_queue(pool: &PgPool) {
    let counts = sqlx::query_as::<_, (i64, i64)>(
        "SELECT count(*), COALESCE(EXTRACT(EPOCH FROM now()-min(created_at))::bigint, 0)
         FROM email_outbox WHERE sent_at IS NULL",
    )
    .fetch_one(pool)
    .await;
    match counts {
        Ok((pending, oldest_seconds)) if oldest_seconds >= 600 => {
            tracing::error!(event = "mail_queue_delayed", pending, oldest_seconds);
        }
        Ok((pending, oldest_seconds)) if pending > 0 => {
            tracing::info!(event = "mail_queue_pending", pending, oldest_seconds);
        }
        Ok(_) => {}
        Err(error) => tracing::error!(
            event = "mail_queue_inspection_failed",
            reason = db_class(&error)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn invalid_or_blocked_gemini_text_leaves_the_locale_on_fallback() {
        let response = json!({"candidates":[{
            "finishReason":"STOP",
            "content":{"parts":[{"text": json!({
                "it":"Ci siamo! Ci vediamo presto.",
                "en":"See you soon at https://example.test!",
                "de":"Trag den Termin ein!"
            }).to_string()}]}
        }]});
        let intros = parse_intros(&response, &["it", "en"]).unwrap();
        assert_eq!(
            intros,
            HashMap::from([("it".into(), "Ci siamo! Ci vediamo presto.".into())])
        );
        assert!(
            parse_intros(&json!({"candidates":[{"finishReason":"SAFETY"}]}), &["it"]).is_none()
        );
    }

    #[test]
    fn change_notes_localize_values_and_flag_schedule_changes() {
        let changes = [
            ("multiple".into(), "true".into(), "false".into()),
            ("votingDeadline".into(), "2027-01-31".into(), String::new()),
        ];
        assert_eq!(
            change_notes(&changes, &[], false, "en"),
            "What changed\nAllow multiple dates per person\nBefore: Yes\nAfter: No\n\
             Voting deadline\nBefore: 31/01/2027\nAfter: —\n"
        );
        let hint = rust_i18n::t!("ui.datesChangedHint", locale = "en");
        let slots = [SlotInput {
            id: None,
            date: NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(),
            start: chrono::NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            end: None,
        }];
        assert_eq!(
            change_notes(&[], &slots, true, "en"),
            format!(
                "What changed\n\nBefore:\n{}\n\n{hint}\n",
                format_slots(&slots, "en")
            )
        );
        let timezone = [("timezone".into(), "Europe/Rome".into(), "UTC".into())];
        assert!(change_notes(&timezone, &[], false, "en").ends_with(&format!("\n{hint}\n")));
        assert!(!change_notes(&changes, &[], false, "en").contains(hint.as_ref()));
    }

    #[test]
    fn composed_subject_has_icon_and_single_line_title() {
        let message = compose(
            "en",
            "changed",
            "Dinner\r\nparty",
            "https://x.test/a",
            None,
            None,
        );
        let headline = rust_i18n::t!("mail.changedSubject", locale = "en");
        assert_eq!(message.subject, format!("📣 {headline} · Dinner  party"));
        assert!(message.body.contains("https://x.test/a"));
        assert!(message.html.contains("https://x.test/a"));
    }

    #[test]
    fn route_uses_precise_coordinates_or_the_original_address() {
        let mut geo = json!({"latitude":45.43,"longitude":12.33,"kind":"venue"});
        for (kind, expected) in [("venue", "45.43,12.33"), ("approximate", "Via Roma 1 & 2")] {
            geo["kind"] = json!(kind);
            let geo = serde_json::from_value(geo.clone()).unwrap();
            let route = directions_url("Via Roma 1 & 2", Some(&geo));
            let query: HashMap<_, _> = route.query_pairs().collect();
            assert_eq!(query["destination"], expected);
            assert_eq!(query["api"], "1");
            assert_eq!(query.len(), 2);
        }
    }
}
