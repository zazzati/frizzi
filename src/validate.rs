//! Input checks shared by the handlers: text, emails, tokens, locales, dates and deadlines.

use axum::http::StatusCode;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use sqlx::{Postgres, Transaction};
use validator::ValidateEmail;

use crate::error::{bad, db, error, ApiError};
use crate::slots::SlotInput;

pub(crate) fn locale(s: &str) -> &str {
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
pub(crate) fn default_locale() -> String {
    "it".into()
}
/// Control characters and bidirectional embedding/override/isolate characters can disguise
/// text in emails and pages. Multi-line fields keep line breaks and tabs.
pub(crate) fn clean(s: &str, multiline: bool) -> bool {
    !s.chars().any(|c| {
        (c.is_control() && !(multiline && matches!(c, '\n' | '\r' | '\t')))
            || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
    })
}
pub(crate) fn valid(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.chars().count() <= max && clean(s, false)
}
pub(crate) fn valid_multiline(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.chars().count() <= max && clean(s, true)
}
pub(crate) fn email(s: &str) -> Result<String, ApiError> {
    let normalized = s.trim().to_lowercase();
    if normalized.len() > 255 || !normalized.validate_email() {
        return Err(bad());
    }
    Ok(normalized)
}
pub(crate) fn valid_token(s: &str) -> bool {
    s.len() == 21
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

pub(crate) fn validate_details(
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
            .is_none_or(|s| s.chars().count() <= 10000 && clean(s, true))
        && timezone.parse::<chrono_tz::Tz>().is_ok()
        && !slots.is_empty()
        && slots.len() <= 100
        && unique_dates.len() == slots.len()
        && slots.iter().all(|s| s.end.is_none_or(|end| end > s.start))
}
/// How far ahead a proposed date may be: catches mistyped years without limiting real plans.
const MAX_SLOT_MONTHS: u32 = 60;
pub(crate) fn clean_description(description: Option<String>) -> Option<String> {
    description
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}
fn today_in(timezone: &str) -> NaiveDate {
    let timezone = timezone.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC);
    Utc::now().with_timezone(&timezone).date_naive()
}
/// Dates that are not already proposed (`existing`) must lie between today and the horizon, so
/// an edit can keep a past option but not add one.
fn slot_dates_in_range(slots: &[SlotInput], existing: &[NaiveDate], today: NaiveDate) -> bool {
    let horizon = today.checked_add_months(chrono::Months::new(MAX_SLOT_MONTHS));
    slots
        .iter()
        .filter(|s| !existing.contains(&s.date))
        .all(|s| s.date >= today && horizon.is_some_and(|last| s.date <= last))
}
pub(crate) fn validate_slot_dates(
    slots: &[SlotInput],
    existing: &[NaiveDate],
    timezone: &str,
) -> Result<(), ApiError> {
    if !slot_dates_in_range(slots, existing, today_in(timezone)) {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_dates"));
    }
    Ok(())
}
/// Voting may close on the last proposed day at the latest.
pub(crate) fn deadline_within_slots(deadline: Option<NaiveDate>, slots: &[SlotInput]) -> bool {
    deadline.is_none_or(|deadline| slots.iter().any(|s| s.date >= deadline))
}
pub(crate) fn deadline_passed(deadline: Option<NaiveDate>, timezone: &str) -> bool {
    deadline_passed_at(deadline, timezone, Utc::now())
}
fn deadline_passed_at(deadline: Option<NaiveDate>, timezone: &str, now: DateTime<Utc>) -> bool {
    deadline.is_some_and(|date| {
        let timezone = timezone.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC);
        now.with_timezone(&timezone).date_naive() > date
    })
}
pub(crate) fn validate_deadline(
    deadline: Option<NaiveDate>,
    timezone: &str,
) -> Result<(), ApiError> {
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
/// chrono-tz and PostgreSQL ship separate tz databases; SQL (e.g. retention) needs the zone too.
pub(crate) async fn require_database_timezone(
    tx: &mut Transaction<'_, Postgres>,
    timezone: &str,
) -> Result<(), ApiError> {
    let known: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_timezone_names WHERE name=$1)")
            .bind(timezone)
            .fetch_one(&mut **tx)
            .await
            .map_err(db)?;
    if !known {
        return Err(bad());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

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
    fn descriptions_are_trimmed_and_blank_ones_dropped() {
        assert_eq!(
            clean_description(Some("  Cena \n".into())),
            Some("Cena".into())
        );
        assert_eq!(clean_description(Some(" \n\t ".into())), None);
        assert_eq!(clean_description(None), None);
    }
    #[test]
    fn new_dates_must_be_between_today_and_the_horizon() {
        let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        let slot = |date| SlotInput {
            id: None,
            date,
            start: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            end: None,
        };
        let today = day(2026, 11, 20);
        assert!(slot_dates_in_range(&[slot(today)], &[], today));
        assert!(slot_dates_in_range(&[slot(day(2031, 11, 20))], &[], today));
        assert!(!slot_dates_in_range(&[slot(day(2031, 11, 21))], &[], today));
        assert!(!slot_dates_in_range(&[slot(day(2026, 11, 19))], &[], today));
        // An edit keeps an existing past option but cannot add a new one.
        let past = day(2026, 11, 1);
        assert!(slot_dates_in_range(
            &[slot(past), slot(today)],
            &[past],
            today
        ));
        assert!(!slot_dates_in_range(
            &[slot(past), slot(day(2026, 11, 2))],
            &[past],
            today
        ));
    }
    #[test]
    fn deadline_may_not_follow_the_last_proposed_date() {
        let day = |d| NaiveDate::from_ymd_opt(2030, 2, d).unwrap();
        let slots: Vec<_> = [3, 4]
            .map(|d| SlotInput {
                id: None,
                date: day(d),
                start: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
                end: None,
            })
            .to_vec();
        assert!(deadline_within_slots(None, &slots));
        assert!(deadline_within_slots(Some(day(1)), &slots));
        assert!(deadline_within_slots(Some(day(4)), &slots));
        assert!(!deadline_within_slots(Some(day(5)), &slots));
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
    fn text_rejects_control_and_bidi_characters() {
        assert!(valid("Cena da Ada", 20));
        assert!(valid("عشاء", 20));
        assert!(!valid(" ", 20));
        assert!(!valid("abc", 2));
        for bad in [
            "a\nb",
            "a\tb",
            "a\u{0}b",
            "a\u{1b}[31mb",
            "a\u{7f}b",
            "a\u{85}b",
            "a\u{202A}b",
            "a\u{202E}b",
            "a\u{2066}b",
            "a\u{2069}b",
        ] {
            assert!(!valid(bad, 20), "{bad:?}");
        }
        assert!(valid_multiline("Ciao,\r\n\tci vediamo", 50));
        assert!(!valid_multiline("Ciao\u{7}", 50));
        assert!(!valid_multiline("Ciao\u{202E}", 50));
        let slots = [SlotInput {
            id: None,
            date: NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(),
            start: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            end: None,
        }];
        let details = |description: &str| {
            validate_details(
                "Cena",
                &Some(description.into()),
                "Roma",
                "Ada",
                "Europe/Rome",
                &slots,
            )
        };
        assert!(details("Prima riga\nSeconda\triga"));
        assert!(details(""));
        assert!(!details("Testo\u{202E}nascosto"));
        assert!(!details("Testo\u{8}"));
    }
}
