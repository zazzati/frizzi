use crate::status::Status;

/// Which email is being written. The name selects the `mail.<name>Subject`/`mail.<name>Body`
/// translations and is logged with each prepared message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailKind {
    /// The organizer's private link, sent when an event is created.
    Admin,
    /// A participant's receipt with their edit link.
    Vote,
    /// A private link re-sent on request.
    Recover,
    /// A free-form message from the organizer to every participant.
    Message,
    Open,
    Confirmed,
    Cancelled,
    /// The organizer edited the event details.
    Changed,
}

impl MailKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            MailKind::Admin => "admin",
            MailKind::Vote => "vote",
            MailKind::Recover => "recover",
            MailKind::Message => "message",
            MailKind::Open => "open",
            MailKind::Confirmed => "confirmed",
            MailKind::Cancelled => "cancelled",
            MailKind::Changed => "changed",
        }
    }
}

/// The notification every participant receives when the event moves to a status.
impl From<Status> for MailKind {
    fn from(status: Status) -> Self {
        match status {
            Status::Open => MailKind::Open,
            Status::Confirmed => MailKind::Confirmed,
            Status::Cancelled => MailKind::Cancelled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_keep_their_translation_names() {
        let kinds = [
            (MailKind::Admin, "admin"),
            (MailKind::Vote, "vote"),
            (MailKind::Recover, "recover"),
            (MailKind::Message, "message"),
            (MailKind::Open, "open"),
            (MailKind::Confirmed, "confirmed"),
            (MailKind::Cancelled, "cancelled"),
            (MailKind::Changed, "changed"),
        ];
        for (kind, name) in kinds {
            assert_eq!(kind.as_str(), name);
            let subject = format!("mail.{name}Subject");
            assert_ne!(rust_i18n::t!(&subject, locale = "en"), subject, "{name}");
        }
    }

    #[test]
    fn statuses_map_to_their_notifications() {
        assert_eq!(MailKind::from(Status::Open), MailKind::Open);
        assert_eq!(MailKind::from(Status::Confirmed), MailKind::Confirmed);
        assert_eq!(MailKind::from(Status::Cancelled), MailKind::Cancelled);
    }
}
