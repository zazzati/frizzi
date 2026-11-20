use crate::error::{db, ApiError};
use crate::observability::{db_class, upstream_class};
use crate::validate::locale;
use crate::{places::LocationGeo, slots::SlotInput};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{Locale, NaiveDate, NaiveTime};
use lettre::message::{header::ContentType, Attachment, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::{collections::HashMap, sync::LazyLock, time::Duration};
use uuid::Uuid;

mod invitation;
mod kind;
mod template;
pub use kind::MailKind;
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
    native_digits(date.format(pattern).to_string(), lang)
}
/// 24-hour `HH:MM`: `%X` would add seconds (and a 12-hour clock with a "UTC" suffix for hi/bn).
fn clock_time(time: NaiveTime, lang: &str) -> String {
    native_digits(time.format("%H:%M").to_string(), lang)
}
fn native_digits(text: String, lang: &str) -> String {
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
                clock_time(s.start, lang)
            );
            if let Some(end) = s.end {
                text.push_str(&format!(" – {}", clock_time(end, lang)));
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
        response: None,
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

/// What an email says, before it is addressed to anyone.
pub struct Email<'a> {
    pub lang: &'a str,
    pub kind: MailKind,
    /// The event title.
    pub title: &'a str,
    /// The recipient's private link, behind the main button.
    pub url: &'a str,
    pub details: Option<&'a Details>,
    /// Replaces the kind's standard opening sentence.
    pub intro: Option<&'a str>,
}

impl<'a> Email<'a> {
    pub fn new(lang: &'a str, kind: MailKind, title: &'a str, url: &'a str) -> Self {
        Self {
            lang,
            kind,
            title,
            url,
            details: None,
            intro: None,
        }
    }
}

fn compose(email: &Email) -> Composed {
    let Email {
        lang,
        kind,
        title,
        url,
        details,
        intro,
    } = *email;
    let subject_key = format!("mail.{}Subject", kind.as_str());
    let body_key = format!("mail.{}Body", kind.as_str());
    let headline = rust_i18n::t!(&subject_key, locale = lang);
    let icon = match kind {
        MailKind::Confirmed => "🎉",
        MailKind::Cancelled => "📌",
        MailKind::Open => "🗳️",
        MailKind::Changed => "📣",
        MailKind::Vote => "✅",
        MailKind::Admin | MailKind::Recover | MailKind::Message => "🔑",
    };
    let default_intro = rust_i18n::t!(&body_key, locale = lang);
    let content = template::Content {
        lang,
        kind,
        headline: &headline,
        title,
        intro: intro.unwrap_or(&default_intro),
        url,
        details,
        message: None,
    };
    Composed {
        subject: format!("{icon} {headline} · {}", title.replace(['\r', '\n'], " ")),
        body: template::plain(&content),
        html: template::html(&content),
    }
}

/// Queues one email to `to` about `event`, sent once the transaction commits.
pub async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    to: &str,
    email: Email<'_>,
) -> Result<(), ApiError> {
    let Composed {
        subject,
        body,
        html,
    } = compose(&email);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO email_outbox (recipient,subject,body,html,event_id) VALUES ($1,$2,$3,$4,$5) RETURNING id",
    )
    .bind(to)
    .bind(subject)
    .bind(body)
    .bind(html)
    .bind(event)
    .fetch_one(&mut **tx)
    .await
    .map_err(db)?;
    tracing::info!(event = "email_prepared", mail_id = %id, kind = email.kind.as_str());
    Ok(())
}

pub async fn enqueue_participant_message(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    public: &str,
    title: &str,
    input: &str,
) -> Result<u64, ApiError> {
    let message = template::MessageText::parse(input).ok_or_else(crate::error::bad)?;
    let languages: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT locale FROM participants WHERE event_id=$1")
            .bind(event)
            .fetch_all(&mut **tx)
            .await
            .map_err(db)?;
    let url = format!("{}/{public}", base_url());
    let clean_title = title.replace(['\r', '\n'], " ");
    let payloads: Vec<_> = languages
        .iter()
        .map(|original| {
            let lang = locale(original);
            let headline = rust_i18n::t!("mail.messageSubject", locale = lang);
            let intro = rust_i18n::t!("mail.messageBody", locale = lang);
            let content = template::Content {
                lang,
                kind: MailKind::Message,
                headline: &headline,
                title,
                intro: &intro,
                url: &url,
                details: None,
                message: Some(&message),
            };
            serde_json::json!({
                "locale": original,
                "subject": format!("💌 {headline} · {clean_title}"),
                "body": template::plain(&content),
                "html": template::html(&content),
            })
        })
        .collect();
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
/// One edited field: `key` names its `ui.<key>` label, `before` and `after` are its values.
pub struct Change {
    pub key: &'static str,
    pub before: String,
    pub after: String,
}

/// What an edit of the event details changed, listed in the notification.
pub struct Changes<'a> {
    pub fields: &'a [Change],
    /// The dates as they were, shown when `dates_changed`.
    pub before_slots: &'a [SlotInput],
    pub dates_changed: bool,
}

struct Recipient {
    email: String,
    locale: String,
    url: String,
    /// Secondary participant link, set when the organizer also voted with this address.
    response: Option<String>,
}

/// One notification per address: an organizer who also voted gets the organizer email (admin
/// link, organizer language) carrying their participant link too, since only that page edits
/// their own availability.
fn recipients(mut participants: Vec<Recipient>, mut organizer: Recipient) -> Vec<Recipient> {
    let same = |p: &Recipient| p.email.trim().eq_ignore_ascii_case(organizer.email.trim());
    if let Some(index) = participants.iter().position(same) {
        organizer.response = Some(participants.remove(index).url);
    }
    participants.push(organizer);
    participants
}
pub async fn notify(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    kind: MailKind,
    changes: Option<&Changes<'_>>,
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
    let participants = participants
        .iter()
        .map(|p| Recipient {
            email: p.get("email"),
            locale: p.get("locale"),
            url: format!(
                "{}/{}/{}",
                base_url(),
                public,
                p.get::<String, _>("edit_token")
            ),
            response: None,
        })
        .collect();
    let organizer_recipient = Recipient {
        email: e.get("organizer_email"),
        locale: e.get("organizer_locale"),
        url: format!(
            "{}/{}/admin/{}",
            base_url(),
            public,
            e.get::<String, _>("admin_token")
        ),
        response: None,
    };
    let title: String = e.get("title");
    let organizer: String = e.get("organizer_name");
    let recipients = recipients(participants, organizer_recipient);
    // Everything except the recipient's links depends only on the language, so the summary
    // queries, change notes and invitation snapshot are built once per language.
    let mut languages: HashMap<&str, (Details, Option<Uuid>)> = HashMap::new();
    let mut messages = Vec::with_capacity(recipients.len());
    for recipient in &recipients {
        let lang = locale(&recipient.locale);
        let intro = confirmation
            .and_then(|extra| extra.intros.get(lang))
            .map(String::as_str);
        if !languages.contains_key(lang) {
            let mut details = summary(tx, event, lang).await?;
            let invitation_id = if kind == MailKind::Confirmed {
                details.website = website.cloned();
                details.calendar = Some(format!("{}/api/events/{public}/calendar.ics", base_url()));
                invitation::enqueue(tx, event, lang, &title, &organizer, intro, &details).await?
            } else {
                None
            };
            if let Some(changes) = changes {
                details.notes = change_notes(changes, lang);
            }
            languages.insert(lang, (details, invitation_id));
        }
        let (details, invitation_id) = &languages[lang];
        let with_response;
        let details = match &recipient.response {
            // Only an organizer who also voted needs a second link; copy the shared details.
            Some(response) => {
                with_response = Details {
                    response: Some(response.clone()),
                    ..details.clone()
                };
                &with_response
            }
            None => details,
        };
        let message = compose(&Email {
            details: Some(details),
            intro,
            ..Email::new(lang, kind, &title, &recipient.url)
        });
        messages.push(serde_json::json!({
            "recipient": recipient.email,
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
        tracing::info!(event = "email_prepared", mail_id = %id, kind = kind.as_str());
    }
    Ok(())
}

fn change_notes(changes: &Changes, lang: &str) -> String {
    let Changes {
        fields,
        before_slots,
        dates_changed,
    } = *changes;
    let mut notes = format!("{}\n", rust_i18n::t!("mail.changes", locale = lang));
    for Change { key, before, after } in fields {
        let key = *key;
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
    if dates_changed || fields.iter().any(|change| change.key == "timezone") {
        notes.push_str(&format!(
            "\n{}\n",
            rust_i18n::t!("ui.datesChangedHint", locale = lang)
        ));
    }
    notes
}

/// After this many claims a message is marked failed instead of being retried again.
const MAX_ATTEMPTS: i32 = 12;

#[derive(Debug)]
struct DeliveryError {
    reason: &'static str,
    status: Option<u16>,
    /// The provider rejected this exact message: retrying cannot succeed.
    permanent: bool,
}

impl From<&'static str> for DeliveryError {
    fn from(reason: &'static str) -> Self {
        Self {
            reason,
            status: None,
            permanent: false,
        }
    }
}

impl DeliveryError {
    fn rejected(reason: &'static str) -> Self {
        Self {
            permanent: true,
            ..reason.into()
        }
    }
}

/// SMTP 5xx replies are final for the message, except authentication failures (x3x), which
/// point at a fixable configuration problem rather than at this message.
fn smtp_rejects_message(code: u16) -> bool {
    (500..600).contains(&code) && (code / 10) % 10 != 3
}

/// Resend 4xx responses reject the request itself, except credential/domain problems (401/403),
/// request timeouts (408), idempotency races (409) and rate limiting (429), which can recover.
fn resend_rejects_message(status: u16) -> bool {
    (400..500).contains(&status) && !matches!(status, 401 | 403 | 408 | 409 | 429)
}

fn gives_up(error: &DeliveryError, attempts: i32) -> bool {
    error.permanent || attempts >= MAX_ATTEMPTS
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
        let port = std::env::var("SMTP_PORT").ok().and_then(|s| s.parse().ok());
        let message = Message::builder()
            .from(from.parse().map_err(|_| "invalid_mail_from")?)
            .to(to
                .parse()
                .map_err(|_| DeliveryError::rejected("invalid_recipient"))?)
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
        .map_err(|_| DeliveryError::rejected("invalid_smtp_message"))?;
        let tls = smtp_tls(&host, port, std::env::var("SMTP_TLS").ok().as_deref())?;
        smtp_transport(&host, port, tls)?
            .send(message)
            .await
            .map_err(|error| {
                let status = error.status().map(u16::from);
                let reason = if error.is_timeout() {
                    "smtp_timeout"
                } else if error.is_transient() {
                    "smtp_transient"
                } else if error.is_permanent() {
                    "smtp_rejected"
                } else {
                    "smtp_transport"
                };
                DeliveryError {
                    reason,
                    status,
                    permanent: error.is_permanent() && status.is_some_and(smtp_rejects_message),
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
            .map_err(|error| {
                let status = error.status().map(|status| status.as_u16());
                DeliveryError {
                    reason: upstream_class(&error),
                    status,
                    permanent: status.is_some_and(resend_rejects_message),
                }
            })?;
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum SmtpTls {
    None,
    StartTls,
    /// Implicit TLS from the first byte, usually on port 465.
    Tls,
}
fn local_smtp(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}
/// `SMTP_TLS` is `none`, `starttls` or `tls`. When unset, loopback relays such as Mailpit run
/// without TLS, port 465 uses implicit TLS and any other relay requires STARTTLS. An unknown
/// value fails delivery rather than falling back to plain text.
fn smtp_tls(host: &str, port: Option<u16>, setting: Option<&str>) -> Result<SmtpTls, &'static str> {
    match setting
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("") if local_smtp(host) => Ok(SmtpTls::None),
        None | Some("") if port == Some(465) => Ok(SmtpTls::Tls),
        None | Some("") | Some("starttls") => Ok(SmtpTls::StartTls),
        Some("none") => Ok(SmtpTls::None),
        Some("tls") => Ok(SmtpTls::Tls),
        Some(_) => Err("invalid_smtp_tls"),
    }
}
/// Builds the SMTP client, signing in with `SMTP_USERNAME`/`SMTP_PASSWORD` when both are set.
fn smtp_transport(
    host: &str,
    port: Option<u16>,
    tls: SmtpTls,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, &'static str> {
    let (builder, default_port) = match tls {
        SmtpTls::None => (
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
            1025,
        ),
        SmtpTls::StartTls => (
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
                .map_err(|_| "smtp_tls_config")?,
            587,
        ),
        SmtpTls::Tls => (
            AsyncSmtpTransport::<Tokio1Executor>::relay(host).map_err(|_| "smtp_tls_config")?,
            465,
        ),
    };
    let mut builder = builder
        .port(port.unwrap_or(default_port))
        .timeout(Some(Duration::from_secs(3)));
    let secret = |name| std::env::var(name).ok().filter(|value| !value.is_empty());
    if let (Some(username), Some(password)) = (secret("SMTP_USERNAME"), secret("SMTP_PASSWORD")) {
        builder = builder.credentials(Credentials::new(username, password));
    }
    Ok(builder.build())
}
pub async fn dispatch(pool: &PgPool, limit: usize) {
    // Invitations whose PDF is still pending after a transient failure or a lease held by
    // another worker: their other mails wait for the next run instead of rendering or waiting
    // again now.
    let mut pending_pdfs = std::collections::HashSet::new();
    for _ in 0..limit {
        let message = sqlx::query("UPDATE email_outbox SET locked_until=now()+interval '2 minutes',attempts=attempts+1 WHERE id=(SELECT id FROM email_outbox WHERE sent_at IS NULL AND failed_at IS NULL AND available_at<=now() AND (locked_until IS NULL OR locked_until<now()) ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING id,recipient,subject,body,html,invitation_id,attempts")
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
        let may_wait = attempts < MAX_ATTEMPTS;
        let pdf = match invitation {
            Some(invitation) if may_wait && pending_pdfs.contains(&invitation) => {
                Err(invitation::AttachmentError::RenderInProgress)
            }
            Some(invitation) => invitation::attachment(pool, invitation, may_wait).await,
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
            Err(error) if error.is_render_pending() => {
                pending_pdfs.extend(invitation);
                Err(error.reason().into())
            }
            Err(error) => {
                tracing::error!(event = "invitation_load_failed", mail_id = %id, reason = error.reason());
                Err("invitation_load_failed".into())
            }
        };
        let duration_ms = start.elapsed().as_millis() as u64;
        if let Some(error) = result
            .as_ref()
            .err()
            .filter(|error| gives_up(error, attempts))
        {
            tracing::error!(event = "mail_failed", mail_id = %id, attempts, duration_ms,
                reason = error.reason, upstream_status = error.status, permanent = error.permanent);
            // Undeliverable: drop bearer links and addresses just like after delivery.
            if let Err(error) = sqlx::query("UPDATE email_outbox SET failed_at=now(),locked_until=NULL,body='',html=NULL,recipient='',subject='',invitation_id=NULL WHERE id=$1").bind(id).execute(pool).await {
                tracing::error!(event = "mail_failure_write_failed", mail_id = %id, reason = db_class(&error));
            }
            release_invitation(pool, invitation).await;
        } else if let Err(error) = result {
            tracing::warn!(event = "mail_deferred", mail_id = %id, attempts, duration_ms,
                reason = error.reason, upstream_status = error.status);
            // Exponential backoff (30 s doubling, capped at 1 h) spreads MAX_ATTEMPTS over ~5 h.
            if let Err(error) = sqlx::query("UPDATE email_outbox SET locked_until=NULL,available_at=now()+make_interval(secs=>LEAST(3600,30*power(2,LEAST(attempts,8)-1))) WHERE id=$1").bind(id).execute(pool).await {
                tracing::error!(event = "mail_retry_schedule_failed", mail_id = %id, reason = db_class(&error));
            }
        } else {
            // Remove bearer links and addresses after successful delivery.
            match sqlx::query("UPDATE email_outbox SET sent_at=now(),locked_until=NULL,body='',html=NULL,recipient='',subject='',invitation_id=NULL WHERE id=$1")
                .bind(id).execute(pool).await {
                Err(error) => tracing::error!(event = "mail_receipt_write_failed", mail_id = %id, reason = db_class(&error)),
                Ok(_) => tracing::info!(event = "mail_accepted", mail_id = %id, attempts, duration_ms),
            }
            release_invitation(pool, invitation).await;
        }
    }
}

/// Drop the rendered PDF as soon as no pending mail needs it; cleanup() catches races.
async fn release_invitation(pool: &PgPool, invitation: Option<Uuid>) {
    let Some(invitation) = invitation else { return };
    if let Err(error) = sqlx::query("DELETE FROM confirmation_invitations WHERE id=$1 AND NOT EXISTS (SELECT 1 FROM email_outbox WHERE invitation_id=$1)")
        .bind(invitation).execute(pool).await {
        tracing::error!(event = "maintenance_cleanup_failed", operation = "invitation", reason = db_class(&error));
    }
}

/// Periodic housekeeping, run by the maintenance worker and the cron rather than after every write.
pub async fn cleanup(pool: &PgPool) {
    for (operation, query) in [
        ("invitation", "DELETE FROM confirmation_invitations WHERE NOT EXISTS (SELECT 1 FROM email_outbox WHERE invitation_id=confirmation_invitations.id)"),
        ("sent_mail", "DELETE FROM email_outbox WHERE sent_at<now()-interval '7 days' OR failed_at<now()-interval '7 days'"),
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
         FROM email_outbox WHERE sent_at IS NULL AND failed_at IS NULL",
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
    fn smtp_tls_mode_defaults_and_overrides() {
        use SmtpTls::{StartTls, Tls};
        let none = Ok(SmtpTls::None);
        assert_eq!(smtp_tls("localhost", Some(1025), None), none);
        assert_eq!(smtp_tls("127.0.0.1", None, Some("")), none);
        assert_eq!(smtp_tls("::1", None, None), none);
        assert_eq!(smtp_tls("smtp.example.com", None, None), Ok(StartTls));
        assert_eq!(smtp_tls("smtp.example.com", Some(587), None), Ok(StartTls));
        assert_eq!(smtp_tls("smtp.example.com", Some(465), None), Ok(Tls));
        assert_eq!(smtp_tls("mailpit", Some(1025), Some("none")), none);
        assert_eq!(
            smtp_tls("localhost", Some(1025), Some(" STARTTLS ")),
            Ok(StartTls)
        );
        assert_eq!(
            smtp_tls("smtp.example.com", Some(2465), Some("tls")),
            Ok(Tls)
        );
        assert_eq!(
            smtp_tls("smtp.example.com", Some(25), Some("off")),
            Err("invalid_smtp_tls")
        );
        assert!(local_smtp("localhost") && !local_smtp("smtp.example.com"));
        for mode in [SmtpTls::None, StartTls, Tls] {
            assert!(smtp_transport("smtp.example.com", None, mode).is_ok());
        }
    }

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
        let change = |key, before: &str, after: &str| Change {
            key,
            before: before.into(),
            after: after.into(),
        };
        let notes = |fields: &[Change], before_slots: &[SlotInput], dates_changed| {
            let changes = Changes {
                fields,
                before_slots,
                dates_changed,
            };
            change_notes(&changes, "en")
        };
        let changes = [
            change("multiple", "true", "false"),
            change("votingDeadline", "2027-01-31", ""),
        ];
        assert_eq!(
            notes(&changes, &[], false),
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
            notes(&[], &slots, true),
            format!(
                "What changed\n\nBefore:\n{}\n\n{hint}\n",
                format_slots(&slots, "en")
            )
        );
        let timezone = [change("timezone", "Europe/Rome", "UTC")];
        assert!(notes(&timezone, &[], false).ends_with(&format!("\n{hint}\n")));
        assert!(!notes(&changes, &[], false).contains(hint.as_ref()));
    }

    #[test]
    fn composed_subject_has_icon_and_single_line_title() {
        let message = compose(&Email::new(
            "en",
            MailKind::Changed,
            "Dinner\r\nparty",
            "https://x.test/a",
        ));
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

    #[test]
    fn numeric_dates_follow_locale_order_and_native_digits() {
        let date = NaiveDate::from_ymd_opt(2027, 6, 5).unwrap();
        for (lang, expected) in [
            ("it", "05/06/2027"),
            ("en", "05/06/2027"),
            ("de", "05.06.2027"),
            ("ru", "05.06.2027"),
            ("zh", "2027/06/05"),
            ("ar", "٠٥/٠٦/٢٠٢٧"),
            ("bn", "০৫/০৬/২০২৭"),
        ] {
            assert_eq!(numeric_date(date, lang), expected, "{lang}");
        }
    }

    #[test]
    fn only_message_specific_rejections_stop_retries() {
        assert!(smtp_rejects_message(550) && smtp_rejects_message(552));
        assert!(!smtp_rejects_message(535) && !smtp_rejects_message(530));
        assert!(!smtp_rejects_message(421) && !smtp_rejects_message(451));
        assert!(resend_rejects_message(400) && resend_rejects_message(422));
        for status in [401, 403, 408, 409, 429, 500, 503] {
            assert!(!resend_rejects_message(status), "{status}");
        }
        let transient = DeliveryError::from("smtp_timeout");
        assert!(!gives_up(&transient, 1) && !gives_up(&transient, MAX_ATTEMPTS - 1));
        assert!(gives_up(&transient, MAX_ATTEMPTS));
        assert!(gives_up(&DeliveryError::rejected("invalid_recipient"), 1));
    }

    #[test]
    fn an_organizer_who_voted_gets_one_email_with_both_links() {
        let person = |email: &str, url: &str| Recipient {
            email: email.into(),
            locale: "es".into(),
            url: url.into(),
            response: None,
        };
        let organizer = Recipient {
            locale: "it".into(),
            ..person("ada@example.test", "/admin")
        };
        let list = recipients(
            vec![
                person("lucia@example.test", "/lucia"),
                person("ADA@example.test", "/ada"),
            ],
            organizer,
        );
        let summary: Vec<_> = list
            .iter()
            .map(|r| (r.url.as_str(), r.locale.as_str(), r.response.as_deref()))
            .collect();
        assert_eq!(
            summary,
            [("/lucia", "es", None), ("/admin", "it", Some("/ada"))]
        );
        let alone = recipients(
            vec![person("lucia@example.test", "/lucia")],
            person("ada@example.test", "/admin"),
        );
        assert_eq!(alone.len(), 2);
        assert!(alone.iter().all(|r| r.response.is_none()));
    }

    #[test]
    fn slot_times_are_hours_and_minutes_in_every_locale() {
        let slot = SlotInput {
            id: None,
            date: NaiveDate::from_ymd_opt(2030, 2, 3).unwrap(),
            start: NaiveTime::from_hms_opt(20, 5, 0).unwrap(),
            end: Some(NaiveTime::from_hms_opt(22, 30, 0).unwrap()),
        };
        for (lang, expected) in [
            ("it", "20:05 – 22:30"),
            ("en", "20:05 – 22:30"),
            ("de", "20:05 – 22:30"),
            ("zh", "20:05 – 22:30"),
            ("hi", "20:05 – 22:30"),
            ("ar", "٢٠:٠٥ – ٢٢:٣٠"),
            ("bn", "২০:০৫ – ২২:৩০"),
        ] {
            let text = format_slots(std::slice::from_ref(&slot), lang);
            assert!(text.ends_with(&format!(" · {expected}")), "{lang}: {text}");
        }
    }
}
