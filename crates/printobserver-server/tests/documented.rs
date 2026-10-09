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
//!
//! The service configurations an operator edits — the two installers' and the
//! real-print cases' — state the periodic observation's default interval
//! beside its key, and are held to the server's
//! `DEFAULT_OBSERVATION_INTERVAL_S` the same way.

use std::path::PathBuf;

use printobserver_core::{DEFAULT_DETECTOR_RESUME_GRACE, MAX_LOOK_WAIT_S};
use printobserver_obico::HANDLED_OVERWRITE;
use printobserver_server::DEFAULT_OBSERVATION_INTERVAL_S;

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

/// Every service configuration an operator edits states the default
/// observation interval, and what it costs over an eight-hour print, as the
/// interval the server runs under when the key is left out.
#[test]
fn every_stated_observation_interval_is_the_servers_default() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let turns = 8 * 60 * 60 / DEFAULT_OBSERVATION_INTERVAL_S;
    for relative in [
        "scripts/install-service.sh",
        "scripts/install-service.ps1",
        "tests/real-prints/service-config.toml",
    ] {
        let path = root.join(relative);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} reads: {error}", path.display()));
        let folded = text
            .lines()
            .map(|line| line.trim_start_matches('#').trim())
            .collect::<Vec<_>>()
            .join(" ");
        for passage in [
            format!("observation_interval_s = {DEFAULT_OBSERVATION_INTERVAL_S}"),
            format!("Left out, every {DEFAULT_OBSERVATION_INTERVAL_S} seconds"),
            format!("an eight-hour print is {turns} turns"),
        ] {
            assert!(
                folded.contains(&passage),
                "{relative} does not say `{passage}`"
            );
        }
    }
}
