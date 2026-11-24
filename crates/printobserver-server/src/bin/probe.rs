//! One probe of the shipped supervisor configuration, for the real-print suite.
//!
//! `printobserver-server-probe --config <path> --session <name> --prompt <text>
//! [--usage <path>] [--image <path> | --no-system]`, or
//! `printobserver-server-probe --config <path> --compact-print <print id>`
//! loads a server configuration exactly as `printobserver server` does, composes
//! its supervisor port exactly as the server does, and runs one prompt through
//! it in a fresh session of its own with no print: the run request a turn is
//! built as, less everything a check adds — or, with `--image`, less everything
//! but the one check image a turn attaches, which is how what that image adds
//! to a call is measured; with `--no-system`, less the system text the port
//! sends as well, which is how what the harness itself sends is measured apart
//! from it. `--usage` names a file of
//! the program's usage lines, one per line, as `printobserver --help` prints
//! them, which the composed port is handed as the server running under
//! `printobserver server` is. It prints the engine's
//! result for that run as JSON on standard output, its per-call usage among
//! it, which is what the suite's base-context figure is read from.
//!
//! With `--compact-print`, it compacts that print's conversation once, in the
//! state the configuration names, as the server's idle compaction does, and
//! prints the compaction as the ledger records it: what the suite's per-print
//! projection takes its compaction's input and what it left from, when no case's
//! conversation reached the threshold.
//!
//! Exit 0 with the result printed; 2 for arguments it cannot read; 1 for a
//! configuration it cannot load or a run the port could not take, the reason
//! on standard error.

use std::path::PathBuf;
use std::process::ExitCode;

use printobserver_oneharness::{CompactionEnd, OneharnessSupervisor};
use printobserver_server::{ServerConfig, agent_config_with};
use printobserver_types::PrintId;

/// What the probe was asked to run.
struct Asked {
    /// The server configuration.
    config: PathBuf,
    /// The file of the program's usage lines, when one was named.
    usage: Option<PathBuf>,
    kind: Kind,
}

/// What a probe runs: one prompt in a session of its own, or one compaction of
/// a print's conversation.
enum Kind {
    /// One prompt, in the session named.
    Run {
        /// Prefixed apart from every print's session, so a probe never continues one.
        session: String,
        prompt: String,
        probe: Probe,
    },
    /// One compaction of this print's conversation, as its idle compaction runs.
    Compaction(PrintId),
}

/// The run probes, each a run of the shipped configuration differing in one thing.
enum Probe {
    /// The configuration as shipped, with no image.
    Text,
    /// The configuration as shipped, this image attached as a check's is.
    Image(PathBuf),
    /// None of the port's system text, so only what the harness sends itself.
    HarnessAlone,
}

/// `--config`, each option once and none empty, and either a run — `--session`
/// and `--prompt`, `--usage` and `--image` optional, and the `--no-system`
/// switch, which excludes `--image` — or `--compact-print`, which takes none of
/// a run's.
fn asked(mut arguments: impl Iterator<Item = String>) -> Result<Asked, String> {
    let (mut config, mut session, mut prompt, mut usage, mut image, mut compact) =
        (None, None, None, None, None, None);
    let mut no_system = false;
    while let Some(flag) = arguments.next() {
        if flag == "--no-system" {
            if no_system {
                return Err(format!("{flag} is given twice"));
            }
            no_system = true;
            continue;
        }
        let slot = match flag.as_str() {
            "--config" => &mut config,
            "--session" => &mut session,
            "--prompt" => &mut prompt,
            "--usage" => &mut usage,
            "--image" => &mut image,
            "--compact-print" => &mut compact,
            other => return Err(format!("`{other}` is no option of this probe")),
        };
        let value = arguments
            .next()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("{flag} takes a value"))?;
        if slot.replace(value).is_some() {
            return Err(format!("{flag} is given twice"));
        }
    }
    let config = PathBuf::from(config.ok_or("--config is required")?);
    let usage = usage.map(PathBuf::from);
    if let Some(print_id) = compact {
        if session.is_some() || prompt.is_some() || image.is_some() || no_system {
            return Err(
                "--compact-print compacts a print's own conversation, so it takes no \
                 --session, --prompt, --image or --no-system"
                    .to_owned(),
            );
        }
        let print_id = print_id
            .parse::<PrintId>()
            .map_err(|error| format!("--compact-print {print_id} is no print: {error}"))?;
        return Ok(Asked {
            config,
            usage,
            kind: Kind::Compaction(print_id),
        });
    }
    let probe = match (no_system, image) {
        (true, Some(_)) => {
            return Err(
                "--no-system measures the harness alone, so it takes no --image".to_owned(),
            );
        }
        (true, None) => Probe::HarnessAlone,
        (false, Some(image)) => Probe::Image(PathBuf::from(image)),
        (false, None) => Probe::Text,
    };
    Ok(Asked {
        config,
        usage,
        kind: Kind::Run {
            session: session.ok_or("--session is required")?,
            prompt: prompt.ok_or("--prompt is required")?,
            probe,
        },
    })
}

fn main() -> ExitCode {
    let asked = match asked(std::env::args().skip(1)) {
        Ok(asked) => asked,
        Err(error) => {
            eprintln!("printobserver-server-probe: {error}");
            return ExitCode::from(2);
        }
    };
    // An image named is held to a JPEG that decodes whole here — every check
    // image is one — so a probe never runs a paid turn on an image the engine
    // would refuse or the model could not read.
    if let Kind::Run {
        probe: Probe::Image(image),
        ..
    } = &asked.kind
        && let Err(error) = std::fs::read(image)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                printobserver_panels::whole_frame(&bytes)
                    .map(|_| ())
                    .map_err(|error| format!("it is no JPEG image: {error}"))
            })
    {
        eprintln!(
            "printobserver-server-probe: {} is unreadable: {error}",
            image.display()
        );
        return ExitCode::FAILURE;
    }
    let usage: Vec<String> = match &asked.usage {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
            Err(error) => {
                eprintln!(
                    "printobserver-server-probe: {} is unreadable: {error}",
                    path.display()
                );
                return ExitCode::FAILURE;
            }
        },
        None => Vec::new(),
    };
    let probed = ServerConfig::load(&asked.config)
        .map_err(|error| error.to_string())
        .and_then(|config| agent_config_with(&config, &usage).map_err(|error| error.to_string()))
        .and_then(|composed| {
            OneharnessSupervisor::open(composed).map_err(|error| error.to_string())
        })
        .and_then(|port| match &asked.kind {
            Kind::Run {
                session,
                prompt,
                probe,
            } => match probe {
                Probe::Text => port.probe(session, prompt),
                Probe::Image(image) => port.probe_with_image(session, prompt, Some(image.clone())),
                Probe::HarnessAlone => port.probe_without_system(session, prompt),
            }
            .map_err(|error| error.to_string())
            .and_then(|result| serde_json::to_string(&result).map_err(|error| error.to_string())),
            Kind::Compaction(print_id) => port
                .compact_once(print_id)
                .map_err(|error| error.to_string())
                .and_then(|compaction| {
                    // A compaction that did not complete measured nothing.
                    if compaction.ended == CompactionEnd::Completed {
                        serde_json::to_string(&compaction).map_err(|error| error.to_string())
                    } else {
                        Err(format!(
                            "the compaction of print {print_id} ended {:?}: {}",
                            compaction.ended,
                            compaction.error.as_deref().unwrap_or("no detail")
                        ))
                    }
                }),
        });
    match probed {
        Ok(answered) => {
            println!("{answered}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("printobserver-server-probe: {error}");
            ExitCode::FAILURE
        }
    }
}
