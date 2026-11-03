//! A scheduled check of one print, through the real server, against a camera
//! on the loopback address.
//!
//! The server is the real composition root with its interval shortened to a
//! second and every other default as it ships — the vision adapter's per-fetch
//! timeout and the supervisor's retries among them — over a machine part-way
//! through a print. What a journey reads is the print's history as the store
//! holds it, in the order it was stored, and the camera's own record of what
//! it was asked and when.

use core::time::Duration;
use std::time::Instant;

use printobserver_core::store::StoredQuery;
use printobserver_core::{
    CameraRetryPayload, PeriodicObservationPayload, PortFailurePayload, PortFailureSite,
};
use printobserver_types::{EventPayload as _, EventRecord, PrintId};

use crate::agent::StandInAgent;
use crate::printer::RecordingPrinter;
use crate::world::{World, set};

/// The interval every check here is scheduled at, in seconds.
pub const INTERVAL_S: i64 = 1;

/// How long a journey waits for the first check before it gives up: enough
/// for four attempts held near the per-fetch timeout, and the waits between.
pub const PATIENCE: Duration = Duration::from_secs(60);

/// A world checking its prints every second, whose camera answers at `url`.
pub async fn checking(url: String) -> World {
    checking_over(url, |stores| stores).await
}

/// The same, with the durable stores as one journey stands something between
/// them and the server.
pub async fn checking_over(
    url: String,
    between: impl FnOnce(printobserver_core::store::Stores) -> printobserver_core::store::Stores,
) -> World {
    World::configured_over(
        RecordingPrinter::printing(),
        StandInAgent::new(),
        |document| {
            set(
                document,
                "supervisor.observation_interval_s",
                toml::Value::Integer(INTERVAL_S),
            );
            let mut table = toml::Table::new();
            table.insert("snapshot_url".to_owned(), toml::Value::String(url));
            set(document, "camera", toml::Value::Table(table));
        },
        between,
    )
    .await
}

/// One print's events, in the order they were stored.
///
/// # Panics
///
/// Panics when the log cannot be read.
pub async fn stored(world: &World, print_id: PrintId) -> Vec<EventRecord> {
    world
        .stores
        .events
        .stored_after(StoredQuery {
            after: None,
            print_id: Some(print_id),
            limit: Some(500),
        })
        .await
        .expect("the log reads")
}

/// What the first scheduled check of a print wrote, as the store holds it.
#[derive(Debug)]
pub struct FirstCheck {
    /// The check itself.
    pub observation: EventRecord,
    /// Every retry the check wrote down before it, in order.
    pub retries: Vec<CameraRetryPayload>,
    /// When the check was first seen in the store.
    pub seen: Instant,
}

impl FirstCheck {
    /// Wait for the first scheduled check of a print to be stored.
    ///
    /// # Panics
    ///
    /// Panics when none is stored within [`PATIENCE`].
    pub async fn of(world: &World, print_id: PrintId) -> Self {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let events = stored(world, print_id).await;
            if let Some(at) = events
                .iter()
                .position(|event| event.kind().as_str() == PeriodicObservationPayload::KIND)
            {
                let retries = events[..at]
                    .iter()
                    .filter_map(EventRecord::payload_as::<CameraRetryPayload>)
                    .map(|retry| retry.expect("a retry reads as one"))
                    .collect();
                return Self {
                    observation: events[at].clone(),
                    retries,
                    seen: Instant::now(),
                };
            }
            assert!(
                Instant::now() < deadline,
                "no scheduled check was stored within {PATIENCE:?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Every port failure recorded against the check, once a moment has
    /// passed for the one written just after it to be stored.
    ///
    /// # Panics
    ///
    /// Panics when the check belongs to no print, or the log cannot be read.
    pub async fn failures(&self, world: &World) -> Vec<PortFailurePayload> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let print_id = self.observation.print_id.expect("a check is a print's");
        stored(world, print_id)
            .await
            .iter()
            .filter_map(EventRecord::payload_as::<PortFailurePayload>)
            .map(|failure| failure.expect("a failure reads as one"))
            .filter(|failure| failure.event_id == self.observation.id)
            .collect()
    }

    /// The one failure at the camera recorded against the check.
    ///
    /// # Panics
    ///
    /// Panics when there is not exactly one.
    pub async fn camera_failure(&self, world: &World) -> PortFailurePayload {
        let mut failures = self.failures(world).await;
        assert_eq!(failures.len(), 1, "{failures:?}");
        let failure = failures.remove(0);
        assert_eq!(failure.site, PortFailureSite::CameraLook, "{failure:?}");
        failure
    }
}
