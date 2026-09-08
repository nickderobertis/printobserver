//! Every journey `printobserver-core` is held to, in one binary.
//!
//! The support modules — the fakes, the ordered journal of port calls, the
//! reader that holds this crate's own sources to their structural properties,
//! and the assembled world the journeys drive — are shared by every journey
//! here rather than copied per test binary, so that there is exactly one
//! printer-port double in this crate's tests and one place the rules about it
//! live.

#[path = "support/journal.rs"]
mod journal;

#[path = "support/fakes.rs"]
mod fakes;

#[path = "support/source.rs"]
mod source;

#[path = "support/world.rs"]
mod world;

#[path = "journeys/bounds.rs"]
mod bounds;

#[path = "journeys/chokepoint.rs"]
mod chokepoint;

#[path = "journeys/action_vocabulary.rs"]
mod action_vocabulary;

#[path = "journeys/rejections.rs"]
mod rejections;

#[path = "journeys/interventions.rs"]
mod interventions;

#[path = "journeys/event_loop.rs"]
mod event_loop;

#[path = "journeys/port_failures.rs"]
mod port_failures;

#[path = "journeys/serialization.rs"]
mod serialization;

#[path = "journeys/terminal_cleanup.rs"]
mod terminal_cleanup;

#[path = "journeys/messages.rs"]
mod messages;

#[path = "journeys/surface.rs"]
mod surface;
