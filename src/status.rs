//! The lifecycle of an event, stored in the `event_status` column.

use serde::{Deserialize, Serialize};
use sqlx::{
    encode::IsNull,
    error::BoxDynError,
    postgres::{PgArgumentBuffer, PgTypeInfo, PgValueRef},
    Decode, Encode, Postgres, Type,
};

/// Serialized as the database labels `OPEN`, `CONFIRMED` and `CANCELLED`, in JSON and SQL alike.
///
/// Queries read the column as `status::text` and write it through `$n::event_status`, so the
/// value travels as TEXT and needs no knowledge of the enum's type OID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub(crate) enum Status {
    Open,
    Confirmed,
    Cancelled,
}

impl Status {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Status::Open => "OPEN",
            Status::Confirmed => "CONFIRMED",
            Status::Cancelled => "CANCELLED",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "OPEN" => Some(Status::Open),
            "CONFIRMED" => Some(Status::Confirmed),
            "CANCELLED" => Some(Status::Cancelled),
            _ => None,
        }
    }

    /// How a change to this status is reported in request logs.
    pub(crate) const fn outcome(self) -> &'static str {
        match self {
            Status::Open => "reopened",
            Status::Confirmed => "confirmed",
            Status::Cancelled => "cancelled",
        }
    }
}

impl Type<Postgres> for Status {
    fn type_info() -> PgTypeInfo {
        <str as Type<Postgres>>::type_info()
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        <str as Type<Postgres>>::compatible(ty)
    }
}

impl Encode<'_, Postgres> for Status {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        <&str as Encode<Postgres>>::encode(self.as_str(), buf)
    }
}

impl<'r> Decode<'r, Postgres> for Status {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        let text = <&str as Decode<Postgres>>::decode(value)?;
        Status::parse(text).ok_or_else(|| format!("unknown event status {text:?}").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Status; 3] = [Status::Open, Status::Confirmed, Status::Cancelled];

    #[test]
    fn status_labels_round_trip_through_text_and_json() {
        for (status, label) in ALL.into_iter().zip(["OPEN", "CONFIRMED", "CANCELLED"]) {
            assert_eq!(status.as_str(), label);
            assert_eq!(Status::parse(label), Some(status));
            let json = serde_json::to_value(status).unwrap();
            assert_eq!(json, serde_json::json!(label));
            assert_eq!(serde_json::from_value::<Status>(json).unwrap(), status);
        }
        for unknown in ["open", "Open", "", "CLOSED"] {
            assert_eq!(Status::parse(unknown), None, "{unknown:?}");
        }
    }

    #[test]
    fn status_outcomes_name_the_change() {
        let outcomes = ALL.map(Status::outcome);
        assert_eq!(outcomes, ["reopened", "confirmed", "cancelled"]);
    }

    #[test]
    fn status_binds_as_text() {
        let text = <String as Type<Postgres>>::type_info();
        assert_eq!(<Status as Type<Postgres>>::type_info(), text);
        assert!(<Status as Type<Postgres>>::compatible(&text));
        let mut buf = PgArgumentBuffer::default();
        let null = Encode::<Postgres>::encode_by_ref(&Status::Confirmed, &mut buf).unwrap();
        assert!(matches!(null, IsNull::No));
        assert_eq!(&**buf, b"CONFIRMED");
    }
}
