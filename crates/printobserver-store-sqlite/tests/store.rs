//! Every journey this crate is held to, in one test binary.
//!
//! One binary rather than one per journey, because the support modules below
//! are shared and a helper that one binary did not happen to use would be dead
//! code in that binary — answerable only by a whole-file suppression, which
//! this repository refuses. Held in one crate, every support item is used by
//! some journey and none of them needs silencing.
//!
//! The journeys stay one file each: `journeys/` is what a reader opens, and a
//! test's own name carries the file it lives in.

#[path = "support/block_on.rs"]
mod block_on;
#[path = "support/child.rs"]
mod child;
#[path = "support/contracts.rs"]
mod contracts;
#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/surface.rs"]
mod surface;

#[path = "journeys/concurrency.rs"]
mod concurrency;
#[path = "journeys/conformance.rs"]
mod conformance;
#[path = "journeys/durability.rs"]
mod durability;
#[path = "journeys/images.rs"]
mod images;
#[path = "journeys/migrations.rs"]
mod migrations;
#[path = "journeys/port_coverage.rs"]
mod port_coverage;
#[path = "journeys/schema.rs"]
mod schema;
#[path = "journeys/unbounded.rs"]
mod unbounded;
#[path = "journeys/unreadable.rs"]
mod unreadable;
