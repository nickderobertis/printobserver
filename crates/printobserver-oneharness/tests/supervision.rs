//! The supervision port, driven end to end against the real `OneHarness`.
//!
//! Every journey here goes through `OneHarness`'s own run path with the paid
//! provider process replaced by the deterministic responder it publishes, which
//! this repository ships a binary for. Nothing doubles the port.
//!
//! They share one test binary so that the helpers they share are used by
//! something in every build of it, rather than needing a rule silenced to say
//! that one journey does not use what another does.

#[path = "supervision/assessment_schema.rs"]
mod assessment_schema;
#[path = "supervision/closing.rs"]
mod closing;
#[path = "supervision/configuration.rs"]
mod configuration;
#[path = "supervision/failures.rs"]
mod failures;
#[path = "supervision/prompting.rs"]
mod prompting;
#[path = "supervision/reports.rs"]
mod reports;
#[path = "supervision/retries.rs"]
mod retries;
#[path = "supervision/sessions.rs"]
mod sessions;
#[path = "supervision/spawning.rs"]
mod spawning;
#[path = "supervision/support.rs"]
mod support;
