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
#[path = "support/http_host.rs"]
mod http_host;
#[path = "support/printer.rs"]
mod printer;
#[path = "support/probes.rs"]
mod probes;
#[path = "support/world.rs"]
mod world;

#[path = "journeys/configuration.rs"]
mod configuration;
#[path = "journeys/failing.rs"]
mod failing;
#[path = "journeys/images.rs"]
mod images;
#[path = "journeys/ingress.rs"]
mod ingress;
#[path = "journeys/operating.rs"]
mod operating;
#[path = "journeys/reconciling.rs"]
mod reconciling;
#[path = "journeys/refusing.rs"]
mod refusing;
#[path = "journeys/restarting.rs"]
mod restarting;
#[path = "journeys/surface.rs"]
mod surface;
