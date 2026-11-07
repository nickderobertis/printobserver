//! The `oneharness` command line, at the release this workspace's lock holds.
//!
//! The real-print suite's operator conversation runs as `oneharness run
//! --session <name>`, the way an operator's own agent is driven. Built from the
//! locked `oneharness` crate rather than taken from whatever a host installed,
//! so the conversation runs on the same engine release the supervisor port
//! links, and nothing outside this lock decides how the harness is started.
//! It is the crate's own entry point, word for word: nothing here adds
//! behaviour.

use clap::Parser as _;

fn main() {
    // The crate's own entry point answers as its published responder when
    // asked to, and so does this.
    if std::env::var_os("ONEHARNESS_INTERNAL_MOCK_HARNESS").is_some() {
        oneharness::mock_harness::run();
    }
    std::process::exit(oneharness::dispatch(oneharness::Cli::parse()));
}
