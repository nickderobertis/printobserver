//! The `oneharness` command line, at the release this workspace's lock holds.
//!
//! The real-print suite's operator conversation runs as `oneharness run
//! --session <name>`, the way an operator's own agent is driven. Built from the
//! locked `oneharness` crate rather than taken from whatever a host installed,
//! so the conversation runs on the same engine release the supervisor port
//! links, and nothing outside this lock decides how the harness is started.
//! It parses its arguments and dispatches them through the crate's own
//! public entry points, and adds nothing: the replay's stand-in for the paid
//! model is reached through `--bin`, never through this program.

use clap::Parser as _;

fn main() {
    std::process::exit(oneharness::dispatch(oneharness::Cli::parse()));
}
