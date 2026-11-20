//! Proposed dates: the request/response shape and the batched insert.

use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::{db, ApiError};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct SlotInput {
    #[serde(default)]
    pub(crate) id: Option<Uuid>,
    pub(crate) date: NaiveDate,
    pub(crate) start: NaiveTime,
    pub(crate) end: Option<NaiveTime>,
}

/// Slots split into one array per column, for a single `UNNEST` insert.
#[derive(Debug, Default, PartialEq)]
struct SlotColumns {
    dates: Vec<NaiveDate>,
    starts: Vec<NaiveTime>,
    ends: Vec<Option<NaiveTime>>,
}
fn slot_columns<'a>(slots: impl IntoIterator<Item = &'a SlotInput>) -> SlotColumns {
    let mut columns = SlotColumns::default();
    for s in slots {
        columns.dates.push(s.date);
        columns.starts.push(s.start);
        columns.ends.push(s.end);
    }
    columns
}
pub(crate) async fn insert_slots<'a>(
    tx: &mut Transaction<'_, Postgres>,
    event_id: Uuid,
    slots: impl IntoIterator<Item = &'a SlotInput>,
) -> Result<(), ApiError> {
    let SlotColumns {
        dates,
        starts,
        ends,
    } = slot_columns(slots);
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let SlotColumns {
            dates,
            starts,
            ends,
        } = slot_columns(&slots);
        assert_eq!(dates, [slots[0].date, slots[1].date]);
        assert_eq!(starts, [time(20), time(19)]);
        assert_eq!(ends, [Some(time(22)), None]);
        assert_eq!(slot_columns(&[]), Default::default());
    }
}
