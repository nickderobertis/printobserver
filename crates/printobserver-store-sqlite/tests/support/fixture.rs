//! The two stores under one handle, and the records the journeys drive them with.
//!
//! The conformance suite runs every journey against both implementations, so a
//! journey names the store it is driving only in what it reports when it fails.

use std::path::Path;
use std::sync::Arc;

use printobserver_store_api::{EventDraft, StorePort};
use printobserver_store_sqlite::{HoldPoints, MemoryStore, SqliteStore};
use printobserver_types::contract::Sample;
use printobserver_types::{
    AcknowledgementDisposition, ActionRequest, Actor, EventId, EventPayload, EventSource,
    JobManifest, MalformedExternalEventPayload, ObicoFailureAlertPayload,
    OperatorAcknowledgementPayload, PrintAction, PrintId, SupervisionSession,
    SupervisionSessionOpenedPayload, Timestamp,
};
use tempfile::TempDir;

/// Which implementation a fixture is holding.
enum Backing {
    /// The durable one.
    Sqlite(Arc<SqliteStore>),
    /// The in-memory one.
    Memory(Arc<MemoryStore>),
}

/// One store under test, in a state directory of its own.
pub struct Fixture {
    /// The state directory, removed when the fixture is dropped.
    dir: TempDir,
    /// The store.
    backing: Backing,
}

impl Fixture {
    /// The durable store, in a fresh state directory.
    ///
    /// # Panics
    ///
    /// Panics when the state directory or the database cannot be created.
    pub fn sqlite() -> Self {
        let dir = TempDir::new().expect("a temporary state directory");
        let store = SqliteStore::open(dir.path()).expect("the store opens");
        Self {
            dir,
            backing: Backing::Sqlite(Arc::new(store)),
        }
    }

    /// The in-memory store, in a fresh state directory.
    ///
    /// # Panics
    ///
    /// Panics when the state directory cannot be created.
    pub fn memory() -> Self {
        let dir = TempDir::new().expect("a temporary state directory");
        let store = MemoryStore::new(dir.path()).expect("the store opens");
        Self {
            dir,
            backing: Backing::Memory(Arc::new(store)),
        }
    }

    /// Both implementations, which every conformance journey runs against.
    pub fn both() -> Vec<Self> {
        vec![Self::sqlite(), Self::memory()]
    }

    /// Which implementation this is, for what a failing journey reports.
    pub fn name(&self) -> &'static str {
        match self.backing {
            Backing::Sqlite(_) => "SqliteStore",
            Backing::Memory(_) => "MemoryStore",
        }
    }

    /// The store, behind the trait object the supervision core holds it behind.
    pub fn port(&self) -> Arc<dyn StorePort> {
        match &self.backing {
            Backing::Sqlite(store) => Arc::clone(store) as Arc<dyn StorePort>,
            Backing::Memory(store) => Arc::clone(store) as Arc<dyn StorePort>,
        }
    }

    /// The state directory this store was opened on.
    pub fn state_dir(&self) -> &Path {
        self.dir.path()
    }

    /// The places this store holds a call at.
    pub fn hold_points(&self) -> &HoldPoints {
        match &self.backing {
            Backing::Sqlite(store) => store.hold_points(),
            Backing::Memory(store) => store.hold_points(),
        }
    }
}

/// One instant, spelled the way this repository spells instants.
///
/// # Panics
///
/// Panics when the text is not an RFC 3339 instant, which is a broken fixture.
pub fn instant(text: &str) -> Timestamp {
    text.parse().expect("a fixed RFC 3339 instant")
}

/// One payload of each kind these journeys drive the history with.
pub fn payload(kind: &str) -> EventPayload {
    match kind {
        "obico_failure_alert" => EventPayload::ObicoFailureAlert(ObicoFailureAlertPayload {
            is_warning: false,
            print_paused: true,
            obico_print_id: Some(7),
            file_name: Some("bracket.gcode".to_owned()),
            started_at: None,
            ended_at: None,
        }),
        "malformed_external_event" => {
            EventPayload::MalformedExternalEvent(MalformedExternalEventPayload {
                detail: "the body was not JSON".to_owned(),
            })
        }
        "supervision_session_opened" => {
            EventPayload::SupervisionSessionOpened(SupervisionSessionOpenedPayload {
                session_name: "watch-7".to_owned(),
                harness_identity: "oneharness".to_owned(),
            })
        }
        "operator_acknowledgement" => {
            EventPayload::OperatorAcknowledgement(OperatorAcknowledgementPayload {
                acknowledged_event_id: EventId::sample_full(),
                disposition: AcknowledgementDisposition::Watch,
            })
        }
        other => panic!("no fixture payload for {other}"),
    }
}

/// One event on its way into a store.
pub fn draft(print_id: Option<PrintId>, kind: &str, received_at: Timestamp) -> EventDraft {
    EventDraft {
        print_id,
        source: EventSource::Obico,
        received_at,
        payload: payload(kind),
        raw: None,
    }
}

/// One request an operator made, at an instant.
pub fn request(requested_at: Timestamp) -> ActionRequest {
    ActionRequest {
        action: PrintAction::Pause {
            reason: "the first layer lifted".to_owned(),
            actor: Actor::Operator,
        },
        actor: Actor::Operator,
        requested_at,
    }
}

/// One sliced job's manifest.
pub fn manifest() -> JobManifest {
    JobManifest::sample_full()
}

/// One print's supervision session.
pub fn session(print_id: PrintId, created_at: Timestamp) -> SupervisionSession {
    SupervisionSession {
        print_id,
        session_name: "watch-7".to_owned(),
        harness_identity: "oneharness".to_owned(),
        created_at,
        last_turn_at: created_at,
        closed_at: None,
        close_reason: None,
    }
}
