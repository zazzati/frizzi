use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Days, NaiveDateTime, Offset, TimeZone, Utc};
use sqlx::{PgPool, Row};

use crate::error::{bad, db, missing, ApiError};
use crate::mail;
use crate::validate::valid_token;

pub async fn download(
    State(pool): State<PgPool>,
    Path(public): Path<String>,
) -> Result<Response, ApiError> {
    if !valid_token(&public) {
        return Err(missing());
    }
    let event = sqlx::query(
        "SELECT e.title, e.location, e.description, e.timezone, e.created_at, e.updated_at,
                s.slot_date, s.start_time, s.end_time
         FROM events e JOIN event_slots s ON s.id=e.confirmed_slot_id
         WHERE e.public_token=$1 AND e.status='CONFIRMED'",
    )
    .bind(&public)
    .fetch_optional(&pool)
    .await
    .map_err(db)?
    .ok_or_else(missing)?;

    let timezone = event
        .get::<String, _>("timezone")
        .parse()
        .map_err(|_| bad())?;
    let date: chrono::NaiveDate = event.get("slot_date");
    let start = to_utc(timezone, date.and_time(event.get("start_time"))).ok_or_else(bad)?;
    let end = event
        .get::<Option<chrono::NaiveTime>, _>("end_time")
        .and_then(|time| to_utc(timezone, date.and_time(time)))
        .filter(|end| *end > start);
    let stamp: DateTime<Utc> = event.get("updated_at");
    let created: DateTime<Utc> = event.get("created_at");

    let mut lines = vec![
        "BEGIN:VCALENDAR".into(),
        "VERSION:2.0".into(),
        "PRODID:-//Frizzi//Event Calendar//EN".into(),
        "METHOD:PUBLISH".into(),
        "BEGIN:VEVENT".into(),
        format!("UID:{public}@frizzi"),
        format!("SEQUENCE:{}", sequence(created, stamp)),
        format!("DTSTAMP:{}", timestamp(stamp)),
        format!("DTSTART:{}", timestamp(start)),
    ];
    if let Some(end) = end {
        lines.push(format!("DTEND:{}", timestamp(end)));
    }
    for (property, value) in [
        ("SUMMARY", event.get::<String, _>("title")),
        ("LOCATION", event.get::<String, _>("location")),
        (
            "DESCRIPTION",
            event
                .get::<Option<String>, _>("description")
                .unwrap_or_default(),
        ),
        ("URL", format!("{}/{public}", mail::base_url())),
    ] {
        lines.push(format!("{property}:{}", escape_text(&value)));
    }
    lines.extend(["END:VEVENT".into(), "END:VCALENDAR".into()]);
    let mut body = String::new();
    for line in lines {
        body.push_str(&fold_line(&line));
        body.push_str("\r\n");
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/calendar; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"frizzi.ics\"",
            ),
        ],
        body,
    )
        .into_response())
}

/// Folds a content line at 75 octets (RFC 5545 §3.1) without splitting a UTF-8 character.
fn fold_line(line: &str) -> String {
    let mut folded = String::with_capacity(line.len());
    let mut column = 0;
    for character in line.chars() {
        if column + character.len_utf8() > 75 {
            folded.push_str("\r\n ");
            column = 1;
        }
        folded.push(character);
        column += character.len_utf8();
    }
    folded
}

/// Every change to the event bumps updated_at, so the seconds since creation grow with each
/// revision and let calendar apps replace an earlier import of the same UID.
fn sequence(created: DateTime<Utc>, updated: DateTime<Utc>) -> i64 {
    (updated - created).num_seconds().max(0)
}

fn timestamp(date: DateTime<Utc>) -> String {
    date.format("%Y%m%dT%H%M%SZ").to_string()
}

fn to_utc(timezone: chrono_tz::Tz, time: NaiveDateTime) -> Option<DateTime<Utc>> {
    if let Some(date) = timezone.from_local_datetime(&time).earliest() {
        return Some(date.to_utc());
    }
    // iCalendar interprets times skipped by a DST jump using the previous offset.
    let before = timezone
        .from_local_datetime(&time.checked_sub_days(Days::new(1))?)
        .earliest()?;
    Some(
        time.and_local_timezone(before.offset().fix())
            .single()?
            .to_utc(),
    )
}

fn escape_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\\n")
        .replace(',', "\\,")
        .replace(';', "\\;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use chrono_tz::Europe::Rome;

    fn rome_to_utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> String {
        let local = NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap();
        timestamp(to_utc(Rome, local).unwrap())
    }

    #[test]
    fn local_times_convert_to_utc_across_dst_transitions() {
        assert_eq!(rome_to_utc(2027, 1, 15, 20, 0), "20270115T190000Z");
        assert_eq!(rome_to_utc(2027, 6, 15, 20, 0), "20270615T180000Z");
        // 02:30 does not exist on 2027-03-28; the pre-jump offset (+01:00) is used.
        assert_eq!(rome_to_utc(2027, 3, 28, 2, 30), "20270328T013000Z");
        // 02:30 happens twice on 2027-10-31; the earlier instant (+02:00) wins.
        assert_eq!(rome_to_utc(2027, 10, 31, 2, 30), "20271031T003000Z");
    }

    #[test]
    fn text_values_escape_ical_specials() {
        assert_eq!(
            escape_text("a,b;c\\d\r\ne\rf\ng"),
            "a\\,b\\;c\\\\d\\ne\\nf\\ng"
        );
        assert_eq!(escape_text("plain"), "plain");
    }

    fn assert_folded(line: &str, folded: &str) {
        assert_eq!(
            folded.replace("\r\n ", ""),
            line,
            "unfolding restores input"
        );
        for (i, part) in folded.split("\r\n").enumerate() {
            assert!(
                part.len() <= 75,
                "physical line {i} is {} octets",
                part.len()
            );
            assert!(i == 0 || part.starts_with(' '));
        }
    }

    #[test]
    fn lines_fold_at_75_octets() {
        let short = "A".repeat(75);
        assert_eq!(fold_line(&short), short);
        let long = "A".repeat(76);
        assert_eq!(fold_line(&long), format!("{}\r\n A", "A".repeat(75)));
        let longer = "B".repeat(200);
        let folded = fold_line(&longer);
        assert_eq!(folded.matches("\r\n ").count(), 2);
        assert_folded(&longer, &folded);
    }

    #[test]
    fn folding_never_splits_multibyte_characters() {
        // 37 × "é" = 74 octets; the 38th would cross the limit.
        let accents = "é".repeat(40);
        let folded = fold_line(&accents);
        assert_eq!(folded, format!("{}\r\n {}", "é".repeat(37), "é".repeat(3)));
        assert_folded(&accents, &folded);
        let mixed = format!("SUMMARY:{}", "Cena 🍝 da Ada · ".repeat(10));
        assert_folded(&mixed, &fold_line(&mixed));
    }

    #[test]
    fn sequence_grows_with_each_revision() {
        let created = Utc.with_ymd_and_hms(2030, 1, 1, 12, 0, 0).unwrap();
        assert_eq!(sequence(created, created), 0);
        let edited = created + chrono::Duration::seconds(90);
        assert_eq!(sequence(created, edited), 90);
        assert!(sequence(created, edited + chrono::Duration::days(3)) > sequence(created, edited));
        assert_eq!(sequence(edited, created), 0);
    }
}
