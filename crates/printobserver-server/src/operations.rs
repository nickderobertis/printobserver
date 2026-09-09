//! The public operations this server serves, declared once.
//!
//! # One operation per public operation, and no more
//!
//! The list here is the whole API. It is exactly one operation per variant of
//! [`PrintAction`](printobserver_types::PrintAction) the contracts declare,
//! plus six reads — status, context, image materialization, history, and the
//! manifest's read and write — and nothing else. The router is built by folding
//! over this array rather than by writing routes out, so the set served and the
//! set declared are one thing; and this crate's own tier derives the required
//! set from the contracts' own `PrintAction` rather than from a list of its
//! own, so growing this array and the routes together cannot satisfy it.
//!
//! Every operation sits beneath [`VERSION_PREFIX`], takes JSON where it takes a
//! body at all, and answers JSON.
//!
//! # The ingress is not one of them
//!
//! [`INGRESS_PATH`] is the endpoint `Obico`'s webhook notification plugin posts
//! to. It is the producer's own ingress rather than an operation a client
//! calls — it is authenticated by a shared secret rather than by an actor, it
//! carries `Obico`'s body rather than this system's, and it answers before its
//! handling completes. So it is declared separately and served outside the
//! versioned prefix, where no client can mistake one surface for the other.

use printobserver_types::ActionKind;

/// The one versioned prefix every public operation is served beneath.
pub const VERSION_PREFIX: &str = "/v1";

/// The media type every operation takes a body in and answers in.
pub const MEDIA_TYPE: &str = "application/json";

/// Where `Obico`'s webhook notification plugin posts, which is not a public
/// operation. See this module's own comment.
pub const INGRESS_PATH: &str = "/obico/webhook";

/// The method one operation is reached by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Method {
    /// A read.
    Get,
    /// A request that changes something.
    Post,
    /// A write that replaces what is there.
    Put,
}

impl Method {
    /// The method as it is spelled on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
        }
    }
}

impl core::fmt::Display for Method {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What one operation does to the machine or the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    /// It reads, and changes nothing.
    Read,
    /// It changes something, so it carries a reason and passes the policy.
    Mutating(ActionKind),
    /// It writes a record without asking anything of the machine.
    Write,
}

/// One public operation: what it is called, how it is reached, and what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operation {
    /// The operation's own name. For a mutating operation this is the spelling
    /// the contracts' own `PrintAction` tags that variant with.
    pub name: &'static str,
    /// The method it is reached by.
    pub method: Method,
    /// Its path beneath [`VERSION_PREFIX`].
    pub path: &'static str,
    /// What it does.
    pub effect: Effect,
    /// The media type it takes a body in, when it takes one.
    pub accepts: Option<&'static str>,
    /// The media type it answers in.
    pub answers: &'static str,
}

impl Operation {
    /// Its whole path, including the versioned prefix.
    #[must_use]
    pub fn full_path(&self) -> String {
        format!("{VERSION_PREFIX}{}", self.path)
    }

    /// Which action this operation asks for, when it asks for one.
    #[must_use]
    pub const fn action_kind(&self) -> Option<ActionKind> {
        match self.effect {
            Effect::Mutating(kind) => Some(kind),
            _ => None,
        }
    }
}

/// One mutating operation, at the path its action is asked for under.
const fn action(name: &'static str, path: &'static str, kind: ActionKind) -> Operation {
    Operation {
        name,
        method: Method::Post,
        path,
        effect: Effect::Mutating(kind),
        accepts: Some(MEDIA_TYPE),
        answers: MEDIA_TYPE,
    }
}

/// One read, which takes no body.
const fn read(name: &'static str, path: &'static str) -> Operation {
    Operation {
        name,
        method: Method::Get,
        path,
        effect: Effect::Read,
        accepts: None,
        answers: MEDIA_TYPE,
    }
}

/// Every public operation this server serves, and there is no other.
pub const OPERATIONS: [Operation; 16] = [
    read("status", "/prints/{print_id}/status"),
    read("context", "/prints/{print_id}/context"),
    read("image", "/images/{image_id}"),
    read("history", "/prints/{print_id}/history"),
    read("manifest_get", "/prints/{print_id}/manifest"),
    Operation {
        name: "manifest_set",
        method: Method::Put,
        path: "/prints/{print_id}/manifest",
        effect: Effect::Write,
        accepts: Some(MEDIA_TYPE),
        answers: MEDIA_TYPE,
    },
    action(
        "pause",
        "/prints/{print_id}/actions/pause",
        ActionKind::Pause,
    ),
    action(
        "resume",
        "/prints/{print_id}/actions/resume",
        ActionKind::Resume,
    ),
    action(
        "cancel",
        "/prints/{print_id}/actions/cancel",
        ActionKind::Cancel,
    ),
    action(
        "start_print",
        "/prints/{print_id}/actions/start_print",
        ActionKind::StartPrint,
    ),
    action(
        "set_feedrate_factor",
        "/prints/{print_id}/actions/set_feedrate_factor",
        ActionKind::SetFeedrateFactor,
    ),
    action(
        "set_flowrate_factor",
        "/prints/{print_id}/actions/set_flowrate_factor",
        ActionKind::SetFlowrateFactor,
    ),
    action(
        "set_tool_target_c",
        "/prints/{print_id}/actions/set_tool_target_c",
        ActionKind::SetToolTargetC,
    ),
    action(
        "set_bed_target_c",
        "/prints/{print_id}/actions/set_bed_target_c",
        ActionKind::SetBedTargetC,
    ),
    action(
        "set_fan_percent",
        "/prints/{print_id}/actions/set_fan_percent",
        ActionKind::SetFanPercent,
    ),
    action(
        "acknowledge_failure",
        "/prints/{print_id}/actions/acknowledge_failure",
        ActionKind::AcknowledgeFailure,
    ),
];

/// The six reads this server serves beside the action vocabulary.
pub const READS: [&str; 6] = [
    "status",
    "context",
    "image",
    "history",
    "manifest_get",
    "manifest_set",
];

/// The operation of one name, when this server serves one.
#[must_use]
pub fn operation(name: &str) -> Option<&'static Operation> {
    OPERATIONS.iter().find(|declared| declared.name == name)
}

#[cfg(test)]
mod tests {
    use printobserver_types::ActionKind;

    use super::{Effect, MEDIA_TYPE, Method, OPERATIONS, VERSION_PREFIX, operation};

    /// Every method is spelled the way the wire spells it.
    #[test]
    fn every_method_is_spelled_as_the_wire_spells_it() {
        for (method, spelling) in [
            (Method::Get, "GET"),
            (Method::Post, "POST"),
            (Method::Put, "PUT"),
        ] {
            assert_eq!(method.as_str(), spelling);
            assert_eq!(method.to_string(), spelling);
        }
    }

    /// An operation's whole path carries the one versioned prefix.
    #[test]
    fn every_operation_sits_beneath_the_one_versioned_prefix() {
        for declared in OPERATIONS {
            let whole = declared.full_path();
            assert!(
                whole.starts_with(VERSION_PREFIX),
                "`{}` is served at {whole}",
                declared.name
            );
            assert_eq!(declared.answers, MEDIA_TYPE);
        }
    }

    /// A mutating operation names the action it asks for, and a read names none.
    #[test]
    fn a_mutating_operation_names_its_action_and_a_read_names_none() {
        for declared in OPERATIONS {
            match declared.effect {
                Effect::Mutating(kind) => assert_eq!(declared.action_kind(), Some(kind)),
                Effect::Read | Effect::Write => assert_eq!(declared.action_kind(), None),
            }
        }
    }

    /// A mutating operation takes JSON and answers JSON; a read takes no body.
    ///
    /// The two constructors are the only way an entry of the declared list is
    /// made, so this is the shape every entry has by construction.
    #[test]
    fn a_mutating_operation_takes_a_body_and_a_read_takes_none() {
        let mutating = super::action(
            "pause",
            "/prints/{print_id}/actions/pause",
            ActionKind::Pause,
        );
        assert_eq!(mutating.method, Method::Post);
        assert_eq!(mutating.accepts, Some(MEDIA_TYPE));
        assert_eq!(mutating.answers, MEDIA_TYPE);
        assert_eq!(mutating.action_kind(), Some(ActionKind::Pause));

        let reading = super::read("status", "/prints/{print_id}/status");
        assert_eq!(reading.method, Method::Get);
        assert_eq!(reading.accepts, None);
        assert_eq!(reading.answers, MEDIA_TYPE);
        assert_eq!(reading.effect, Effect::Read);
    }

    /// An operation this server does not serve is answered as none.
    #[test]
    fn an_operation_this_server_does_not_serve_is_answered_as_none() {
        assert_eq!(operation("pause").map(|found| found.name), Some("pause"));
        assert!(operation("reboot").is_none());
    }
}
