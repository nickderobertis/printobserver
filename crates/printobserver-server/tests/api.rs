//! The server's own tier: its API, its ingress, its record and its service.
//!
//! Every journey here drives the real surface over HTTP. What a journey stands
//! in for is the two external systems on the far side of a port — the machine
//! and the supervising agent — and the same declared operation list is walked
//! against the real ones by `tests/integration.rs`.

#[path = "support/agent.rs"]
mod agent;
#[path = "support/hosts.rs"]
mod hosts;
#[path = "support/printer.rs"]
mod printer;
#[path = "support/world.rs"]
mod world;

#[path = "journeys/configuration.rs"]
mod configuration;
#[path = "journeys/images.rs"]
mod images;
#[path = "journeys/ingress.rs"]
mod ingress;
#[path = "journeys/operating.rs"]
mod operating;
#[path = "journeys/reconciling.rs"]
mod reconciling;
#[path = "journeys/restarting.rs"]
mod restarting;
#[path = "journeys/surface.rs"]
mod surface;
