//! The server's own tier: its API, its ingress, its record and its service.
//!
//! Every journey here drives the real surface over HTTP. What a journey stands
//! in for is the two external systems on the far side of a port — the machine
//! and the supervising agent — and the same declared operation list is walked
//! against the real ones by `tests/integration.rs`.

#[path = "support/agent.rs"]
mod agent;
#[path = "support/failing_store.rs"]
mod failing_store;
#[path = "support/held_events.rs"]
mod held_events;
#[path = "support/http_host.rs"]
mod http_host;
#[path = "support/printer.rs"]
mod printer;
#[path = "support/probes.rs"]
mod probes;
#[path = "support/refusing_prints.rs"]
mod refusing_prints;
#[path = "support/world.rs"]
mod world;

#[path = "journeys/authenticating.rs"]
mod authenticating;
#[path = "journeys/binding.rs"]
mod binding;
#[path = "journeys/configuration.rs"]
mod configuration;
#[path = "journeys/failing.rs"]
mod failing;
#[path = "journeys/finding.rs"]
mod finding;
#[path = "journeys/images.rs"]
mod images;
#[path = "journeys/ingress.rs"]
mod ingress;
#[path = "journeys/looking.rs"]
mod looking;
#[path = "journeys/observing.rs"]
mod observing;
#[path = "journeys/operating.rs"]
mod operating;
#[path = "journeys/reconciling.rs"]
mod reconciling;
#[path = "journeys/refusing.rs"]
mod refusing;
// The responder binary this journey runs is built only under the feature this
// crate's own test target is run with.
#[cfg(feature = "test-responder")]
#[path = "journeys/responding.rs"]
mod responding;
#[path = "journeys/restarting.rs"]
mod restarting;
#[cfg(feature = "test-responder")]
#[path = "journeys/skilled.rs"]
mod skilled;
#[path = "journeys/surface.rs"]
mod surface;
