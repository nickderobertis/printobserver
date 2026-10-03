//! The numbers the agent-facing documents state are the ones this program runs
//! with.
//!
//! The skill and its references tell the agent how long a look may wait, how
//! long after its adjustment a print the detector paused is resumed, and what
//! `Obico` is told when it is. Each is a constant of the code — the core's
//! `MAX_LOOK_WAIT_S` and `DEFAULT_DETECTOR_RESUME_GRACE`, the `Obico` adapter's
//! `HANDLED_OVERWRITE` — and every passage stating one is read here and held to
//! it, so a constant that moves without its documents fails this rather than
//! leaving the agent working to a number the program no longer keeps.

use std::path::PathBuf;

use printobserver_core::{DEFAULT_DETECTOR_RESUME_GRACE, MAX_LOOK_WAIT_S};
use printobserver_obico::HANDLED_OVERWRITE;

/// One document of the skill, its whitespace folded so that a passage reads
/// the same wherever it wraps.
fn document(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/printobserver")
        .join(relative);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} reads: {error}", path.display()));
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How the skill, which is prose the agent reads, says a number of seconds.
///
/// Only the numbers the code keeps today are spelled here. A constant moved to
/// another is refused by name, which is the moment to rewrite the skill's
/// sentence and this spelling together.
fn spoken(seconds: u64) -> &'static str {
    match seconds {
        20 => "twenty seconds",
        90 => "a minute and a half",
        other => panic!(
            "the skill says {other} seconds in words: spell it here and rewrite the skill's \
             sentence to it"
        ),
    }
}

/// Every passage stating how long a look may wait states the core's limit.
#[test]
fn every_stated_look_wait_is_the_cores_limit() {
    let limit = u64::from(MAX_LOOK_WAIT_S);
    let figure = format!("at most {MAX_LOOK_WAIT_S}");
    for (relative, passage) in [
        (
            "SKILL.md",
            format!("it can wait up to {} first", spoken(limit)),
        ),
        ("reference/api-and-clients.md", figure.clone()),
        ("reference/command-surface.md", figure.clone()),
        (
            "reference/command-surface.md",
            format!("`refused` for a wait over {MAX_LOOK_WAIT_S}"),
        ),
        ("reference/common-operations.md", figure.clone()),
    ] {
        assert!(
            document(relative).contains(&passage),
            "{relative} no longer says `{passage}`, which is how long the core lets a look wait"
        );
    }
}

/// Every passage stating when a print the detector paused is resumed states
/// the core's grace.
#[test]
fn every_stated_resume_grace_is_the_cores() {
    let grace = spoken(DEFAULT_DETECTOR_RESUME_GRACE.as_secs());
    for (relative, passage) in [
        (
            "SKILL.md",
            format!("resumes the print itself {grace} after your last such change"),
        ),
        (
            "reference/intervention-policy.md",
            format!("{grace} after the agent's last such adjustment"),
        ),
    ] {
        assert!(
            document(relative).contains(&passage),
            "{relative} no longer says `{passage}`, which is when the core resumes the print"
        );
    }
}

/// The overwrite the intervention policy says `Obico` is told is the one the
/// adapter sends.
#[test]
fn the_stated_overwrite_is_the_one_obico_is_sent() {
    let passage = format!("with the overwrite `{HANDLED_OVERWRITE}`");
    assert!(
        document("reference/intervention-policy.md").contains(&passage),
        "the intervention policy no longer says `{passage}`, which is what the adapter sends"
    );
}
