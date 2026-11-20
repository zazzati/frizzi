use crate::error::{db, ApiError};
use crate::observability::db_class;
use serde_json::{json, Value};
use sqlx::{types::Json, PgPool, Postgres, Row, Transaction};
use std::{
    fmt::Write,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::LazyLock,
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command, sync::OnceCell};
use uuid::Uuid;

use super::template::Details;

pub(super) const FILENAME: &str = "invito-frizzi.pdf";

const PDF_ASSETS: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/pdf_assets.rs"));
static PDF_BUNDLE: OnceCell<Option<PathBuf>> = OnceCell::const_new();

#[cfg(unix)]
fn setup_pdf_bundle() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("frizzi-pdf-{}", std::process::id()));
    for (name, content) in PDF_ASSETS {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, content)?;
        if *name == "typst" {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(root)
}

async fn pdf_bundle() -> Option<&'static Path> {
    if PDF_ASSETS.is_empty() {
        return None;
    }
    #[cfg(unix)]
    return PDF_BUNDLE
        .get_or_init(|| async {
            // Writing the bundled binary and fonts is blocking file I/O; keep it off the runtime.
            tokio::task::spawn_blocking(setup_pdf_bundle)
                .await
                .ok()
                .and_then(Result::ok)
        })
        .await
        .as_deref();
    #[cfg(not(unix))]
    None
}

pub(super) async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    lang: &str,
    title: &str,
    organizer: &str,
    intro: Option<&str>,
    details: &Details,
) -> Result<Option<Uuid>, ApiError> {
    if PDF_ASSETS.is_empty()
        && std::env::var_os("FRIZZI_TYPST").is_none_or(|value| value.is_empty())
    {
        return Ok(None);
    }
    let default_intro = rust_i18n::t!("mail.confirmedBody", locale = lang);
    let payload = json!({
        "lang": lang,
        "title": title,
        "organizer": organizer,
        "headline": rust_i18n::t!("mail.confirmedSubject", locale = lang),
        "intro": intro.unwrap_or(&default_intro),
        "description": details.description,
        "location": details.location,
        "dates": details.dates,
        "timezone": details.timezone,
        "directions": details.directions.as_str(),
        "website": details.website.as_ref().map(reqwest::Url::as_str),
        "labels": {
            "invitation": rust_i18n::t!("pdf.invitation", locale = lang),
            "when": rust_i18n::t!("pdf.when", locale = lang),
            "where": rust_i18n::t!("pdf.where", locale = lang),
            "timezone": rust_i18n::t!("ui.timezone", locale = lang),
            "directions": rust_i18n::t!("mail.directions", locale = lang),
            "website": rust_i18n::t!("mail.venueWebsite", locale = lang),
            "organizer": rust_i18n::t!("ui.organized", locale = lang),
            "description": rust_i18n::t!("ui.description", locale = lang),
        },
    });
    sqlx::query_scalar(
        "INSERT INTO confirmation_invitations (event_id,payload) VALUES ($1,$2) RETURNING id",
    )
    .bind(event)
    .bind(Json(payload))
    .fetch_one(&mut **tx)
    .await
    .map(Some)
    .map_err(db)
}

#[derive(Debug)]
pub(super) enum AttachmentError {
    Database(sqlx::Error),
    /// Another dispatcher holds the render lease.
    RenderInProgress,
    /// Rendering failed transiently; the payload is kept for a later attempt.
    RenderFailed,
}

impl From<sqlx::Error> for AttachmentError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl AttachmentError {
    pub(super) fn reason(&self) -> &'static str {
        match self {
            Self::Database(error) => db_class(error),
            Self::RenderInProgress => "render_in_progress",
            Self::RenderFailed => "invitation_pdf_retry",
        }
    }

    /// The mail should simply wait for a later run (as opposed to a database failure).
    pub(super) fn is_render_pending(&self) -> bool {
        matches!(self, Self::RenderInProgress | Self::RenderFailed)
    }
}

/// Renders that may fail for a transient reason before the mails go out without a PDF.
const MAX_RENDER_ATTEMPTS: i32 = 3;

/// Timeouts and resource exhaustion can pass; a missing renderer or a document that does not
/// compile fails the same way every time.
fn transient(reason: &str) -> bool {
    matches!(
        reason,
        "timeout"
            | "typst_start_failed"
            | "typst_input_unavailable"
            | "typst_input_failed"
            | "typst_output_failed"
    )
}

fn retry_render(reason: &str, failed_renders: i32, may_wait: bool) -> bool {
    may_wait && transient(reason) && failed_renders < MAX_RENDER_ATTEMPTS
}

/// How long a dispatcher waits for a render another one claimed (Typst times out after 6s).
const RENDER_WAIT: Duration = Duration::from_secs(8);
const RENDER_POLL: Duration = Duration::from_millis(250);

/// `may_wait` is false on a mail's last outbox attempt: it then goes out without the PDF rather
/// than failing because of it.
pub(super) async fn attachment(
    pool: &PgPool,
    id: Uuid,
    may_wait: bool,
) -> Result<Option<Vec<u8>>, AttachmentError> {
    let deadline = tokio::time::Instant::now() + RENDER_WAIT;
    loop {
        // Claim the render with a lease instead of a row lock, so no transaction or pooled
        // connection is held while Typst runs; the result is retained for the other recipients.
        let claimed = sqlx::query(
            "UPDATE confirmation_invitations SET render_locked_until=now()+interval '30 seconds'
             WHERE id=$1 AND payload IS NOT NULL
               AND (render_locked_until IS NULL OR render_locked_until<now())
             RETURNING payload,render_attempts",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        if let Some(row) = claimed {
            let Json(payload): Json<Value> = row.get("payload");
            let pdf = match render(&payload).await {
                Ok(attachment) => Some(attachment),
                Err(reason) => {
                    let failed_renders = row.get::<i32, _>("render_attempts") + 1;
                    if retry_render(reason, failed_renders, may_wait) {
                        tracing::warn!(event = "invitation_pdf_deferred", reason, failed_renders);
                        // Keep the payload and release the lease so a later run renders again.
                        sqlx::query(
                            "UPDATE confirmation_invitations
                             SET render_attempts=$2,render_locked_until=NULL
                             WHERE id=$1 AND payload IS NOT NULL",
                        )
                        .bind(id)
                        .bind(failed_renders)
                        .execute(pool)
                        .await?;
                        return Err(AttachmentError::RenderFailed);
                    }
                    tracing::warn!(event = "invitation_pdf_fallback", reason, failed_renders);
                    None
                }
            };
            // A renderer whose lease expired must not overwrite a result stored meanwhile.
            sqlx::query(
                "UPDATE confirmation_invitations SET payload=NULL,pdf=$2,render_locked_until=NULL
                 WHERE id=$1 AND payload IS NOT NULL",
            )
            .bind(id)
            .bind(&pdf)
            .execute(pool)
            .await?;
            return Ok(pdf);
        }
        let row = sqlx::query(
            "SELECT payload IS NOT NULL AS pending,pdf FROM confirmation_invitations WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        match row {
            None => return Ok(None),
            Some(row) if !row.get::<bool, _>("pending") => return Ok(row.get("pdf")),
            Some(_) if tokio::time::Instant::now() >= deadline => {
                return if may_wait {
                    Err(AttachmentError::RenderInProgress)
                } else {
                    Ok(None)
                };
            }
            Some(_) => tokio::time::sleep(RENDER_POLL).await,
        }
    }
}

async fn render(payload: &Value) -> Result<Vec<u8>, &'static str> {
    let (executable, fonts) = if let Some(root) = pdf_bundle().await {
        (
            root.join("typst").into_os_string(),
            Some(root.join("fonts").into_os_string()),
        )
    } else {
        let executable = std::env::var_os("FRIZZI_TYPST")
            .filter(|path| !path.is_empty())
            .ok_or("typst_unconfigured")?;
        (executable, std::env::var_os("FRIZZI_PDF_FONTS"))
    };
    let mut command = Command::new(executable);
    command
        .args(["compile", "--root", ".", "--ignore-system-fonts"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(fonts) = fonts.filter(|path| !path.is_empty()) {
        command.arg("--font-path").arg(fonts);
    }
    command.args(["-", "-"]);
    let source = source(payload);
    let output = tokio::time::timeout(Duration::from_secs(6), async {
        let mut child = command.spawn().map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => {
                "typst_unavailable"
            }
            _ => "typst_start_failed",
        })?;
        let mut stdin = child.stdin.take().ok_or("typst_input_unavailable")?;
        stdin
            .write_all(source.as_bytes())
            .await
            .map_err(|_| "typst_input_failed")?;
        drop(stdin);
        child
            .wait_with_output()
            .await
            .map_err(|_| "typst_output_failed")
    })
    .await
    .map_err(|_| "timeout")??;
    if !output.status.success() {
        return Err("typst_compile_failed");
    }
    if !output.stdout.starts_with(b"%PDF-") || output.stdout.len() > 512 * 1024 {
        return Err("invalid_pdf");
    }
    Ok(output.stdout)
}

fn byte_list(source: &mut String, bytes: &[u8]) {
    for byte in bytes {
        let _ = write!(source, "{byte},");
    }
}

/// Everything after the payload is constant; encoding the 44KB image as decimal text is done once.
static SOURCE_TAIL: LazyLock<String> = LazyLock::new(|| {
    let mut tail = String::from(")))\n#let guests-image = bytes((");
    byte_list(&mut tail, include_bytes!("invitation-guests.webp"));
    tail.push_str("))\n");
    tail.push_str(include_str!("invitation.typ"));
    tail
});

fn source(payload: &Value) -> String {
    // Treat the snapshot as bytes so event text cannot become Typst source code.
    let payload = payload.to_string();
    let mut source = String::with_capacity(32 + payload.len() * 4 + SOURCE_TAIL.len());
    source.push_str("#let invitation = json(bytes((");
    byte_list(&mut source, payload.as_bytes());
    source.push_str(&SOURCE_TAIL);
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_encodes_payload_and_image_as_byte_lists() {
        let payload = json!({"title": "Cena \"#import\" ✨"});
        let mut expected = String::from("#let invitation = json(bytes((");
        for byte in payload.to_string().bytes() {
            expected.push_str(&format!("{byte},"));
        }
        expected.push_str(")))\n#let guests-image = bytes((");
        for byte in include_bytes!("invitation-guests.webp") {
            expected.push_str(&format!("{byte},"));
        }
        expected.push_str("))\n");
        expected.push_str(include_str!("invitation.typ"));
        assert_eq!(source(&payload), expected);
        assert!(source(&json!({})).starts_with("#let invitation = json(bytes((123,125,)))\n"));
    }

    #[test]
    fn only_transient_render_failures_are_retried_and_only_a_few_times() {
        assert!(retry_render("timeout", 1, true));
        assert!(retry_render(
            "typst_start_failed",
            MAX_RENDER_ATTEMPTS - 1,
            true
        ));
        assert!(!retry_render("timeout", MAX_RENDER_ATTEMPTS, true));
        assert!(
            !retry_render("timeout", 1, false),
            "a mail's last attempt never waits"
        );
        for reason in [
            "typst_unconfigured",
            "typst_unavailable",
            "typst_compile_failed",
            "invalid_pdf",
        ] {
            assert!(!retry_render(reason, 1, true), "{reason}");
        }
        assert!(AttachmentError::RenderFailed.is_render_pending());
        assert!(AttachmentError::RenderInProgress.is_render_pending());
    }
}
