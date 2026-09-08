//! One assembled supervisor and its four fakes, and the fixtures they drive.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use printobserver_core::{ActionOutcome, CoreConfig, CoreError, Supervisor, block_on};
use printobserver_types::{
    ActionKind, Actor, ActorClass, Adjustable, EventPayload, EventRecord, JobManifest,
    ObicoFailureAlertPayload, PrintAction, PrintId, PrintRecord, Range, RawBytes, SafetyEnvelope,
    Timestamp,
};
use printobserver_vision_api::NormalizedAlert;

use crate::fakes::{FakeClock, FakePrinter, FakeStore, FakeSupervisor, FakeVision};
use crate::journal::Journal;

/// The instant the fake clock starts at.
pub const EPOCH_SECONDS: i64 = 1_700_000_000;

/// Every action the vocabulary declares, as a kind.
///
/// The walk over the vocabulary asserts this is exactly what the contracts
/// declare, so a variant added there cannot go undriven here.
pub const ACTION_KINDS: [ActionKind; 10] = [
    ActionKind::Pause,
    ActionKind::Resume,
    ActionKind::Cancel,
    ActionKind::StartPrint,
    ActionKind::SetFeedrateFactor,
    ActionKind::SetFlowrateFactor,
    ActionKind::SetToolTargetC,
    ActionKind::SetBedTargetC,
    ActionKind::SetFanPercent,
    ActionKind::AcknowledgeFailure,
];

/// The safety envelope every journey runs under unless it says otherwise.
///
/// It grants every actor class every action, so that a journey reaching a
/// rejection reaches the one it meant to rather than tripping over a grant it
/// did not think about.
#[must_use]
pub fn permissive_envelope() -> SafetyEnvelope {
    let mut allowed = BTreeMap::new();
    allowed.insert(Adjustable::Feedrate, Range::new(0.5, 2.0));
    allowed.insert(Adjustable::Flowrate, Range::new(0.8, 1.2));
    allowed.insert(Adjustable::ToolTarget { tool: 0 }, Range::new(180.0, 250.0));
    allowed.insert(Adjustable::BedTarget, Range::new(0.0, 80.0));
    allowed.insert(Adjustable::Fan, Range::new(0.0, 100.0));
    let mut actions = BTreeMap::new();
    for class in [ActorClass::Agent, ActorClass::Operator, ActorClass::System] {
        actions.insert(class, ACTION_KINDS.to_vec());
    }
    SafetyEnvelope {
        allowed,
        actions,
        agent_min_interval_s: 0,
    }
}

/// One assembled supervisor, its four fakes, its clock and its journal.
pub struct World {
    /// The supervisor under test.
    pub core: Arc<Supervisor>,
    /// The ordered record of every port call.
    pub journal: Arc<Journal>,
    /// The store.
    pub store: Arc<FakeStore>,
    /// The printer.
    pub printer: Arc<FakePrinter>,
    /// The vision port.
    pub vision: Arc<FakeVision>,
    /// The supervising agent's harness.
    pub agent: Arc<FakeSupervisor>,
    /// The one time source.
    pub clock: Arc<FakeClock>,
}

impl World {
    /// A world under the permissive envelope.
    #[must_use]
    pub fn new() -> Self {
        Self::with_envelope(permissive_envelope())
    }

    /// A world under one envelope.
    #[must_use]
    pub fn with_envelope(envelope: SafetyEnvelope) -> Self {
        let journal = Arc::new(Journal::default());
        let clock = Arc::new(FakeClock::new(EPOCH_SECONDS));
        let store = Arc::new(FakeStore::new(Arc::clone(&journal), Arc::clone(&clock)));
        let printer = Arc::new(FakePrinter::new(Arc::clone(&journal)));
        let vision = Arc::new(FakeVision::new(Arc::clone(&journal)));
        let agent = Arc::new(FakeSupervisor::new(
            Arc::clone(&journal),
            Arc::clone(&clock),
        ));
        let mut config = CoreConfig::new(
            envelope,
            format!("printobserver context {}", printobserver_core::PRINT_ID_PLACEHOLDER),
        );
        config.expiry_poll = Duration::from_millis(5);
        let core = Supervisor::new(
            config,
            Arc::clone(&printer) as Arc<dyn printobserver_printer_api::PrinterPort>,
            Arc::clone(&store) as Arc<dyn printobserver_store_api::StorePort>,
            Arc::clone(&vision) as Arc<dyn printobserver_vision_api::VisionPort>,
            Arc::clone(&agent) as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
            Arc::clone(&clock) as Arc<dyn printobserver_core::Clock>,
        );
        agent.attach(&core);
        Self {
            core,
            journal,
            store,
            printer,
            vision,
            agent,
            clock,
        }
    }

    /// Open one print directly at the store, the way an earlier event would.
    ///
    /// # Panics
    ///
    /// Panics when the store refuses to open it.
    pub fn open_print(&self, obico_print_id: i64) -> PrintRecord {
        block_on(
            printobserver_store_api::StorePort::open_print(
                self.store.as_ref(),
                Some(obico_print_id),
                Some("benchy.gcode".to_owned()),
            ),
        )
        .expect("the store opens a print")
    }

    /// Drive one event through the real loop.
    pub fn handle(&self, alert: NormalizedAlert) -> Result<EventRecord, CoreError> {
        block_on(self.core.handle_event(alert))
    }

    /// Request one action through the real action path.
    pub fn request(
        &self,
        print_id: PrintId,
        action: PrintAction,
    ) -> Result<ActionOutcome, CoreError> {
        block_on(self.core.request_action(print_id, action))
    }

    /// Read one print's context back the way the context command does.
    pub fn context(&self, print_id: PrintId) -> Result<printobserver_types::PrintContext, CoreError> {
        block_on(self.core.context(print_id))
    }

    /// Wait until a condition the fakes can observe holds, or give up.
    ///
    /// # Panics
    ///
    /// Panics when the condition has not held within the deadline, which is a
    /// property the run never reached rather than a slow machine.
    pub fn wait_until(&self, what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if condition() {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("{what} did not happen within the deadline");
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

/// The instant `seconds` after the fake clock's own epoch.
///
/// # Panics
///
/// Panics when that instant is not representable, which it is by construction.
#[must_use]
pub fn at(seconds: i64) -> Timestamp {
    Timestamp::from_unix_seconds(EPOCH_SECONDS + seconds).expect("a representable instant")
}

/// The agent, as an actor naming its session.
#[must_use]
pub fn agent_actor(print_id: PrintId) -> Actor {
    Actor::Agent {
        session_name: format!("print-{print_id}"),
    }
}

/// One Obico failure alert about a print, carrying no image.
#[must_use]
pub fn failure_alert(obico_print_id: i64) -> NormalizedAlert {
    NormalizedAlert {
        source: printobserver_types::EventSource::Obico,
        received_at: at(0),
        payload: EventPayload::ObicoFailureAlert(ObicoFailureAlertPayload {
            is_warning: false,
            print_paused: false,
            obico_print_id: Some(obico_print_id),
            file_name: Some("benchy.gcode".to_owned()),
        }),
        raw: RawBytes::new(br#"{"event":"print_failure"}"#.to_vec()),
        image_url: None,
    }
}

/// The same alert, naming an image for the loop to fetch and store.
#[must_use]
pub fn failure_alert_with_image(obico_print_id: i64) -> NormalizedAlert {
    NormalizedAlert {
        image_url: Some("https://obico.example/snapshot.png".to_owned()),
        ..failure_alert(obico_print_id)
    }
}

/// A manifest naming the ranges given for it, and nothing else.
#[must_use]
pub fn manifest(allowed: &[(Adjustable, Range)]) -> JobManifest {
    JobManifest {
        file_name: "benchy.gcode".to_owned(),
        material: "PLA".to_owned(),
        nozzle_diameter_mm: 0.4,
        slicer_profile: "0.20mm SPEED".to_owned(),
        allowed: allowed.iter().copied().collect(),
        metadata: BTreeMap::new(),
    }
}
