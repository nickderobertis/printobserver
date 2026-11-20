//! One probe of the shipped supervisor configuration, for the real-print suite.
//!
//! `printobserver-server-probe --config <path> --session <name> --prompt <text>`
//! loads a server configuration exactly as `printobserver server` does, composes
//! its supervisor port exactly as the server does, and runs one prompt through
//! it in a fresh session of its own with no image and no print: the run request
//! a turn is built as, less everything a check adds. It prints the engine's
//! result for that run as JSON on standard output, its per-call usage among
//! it, which is what the suite's base-context figure is read from.
//!
//! Exit 0 with the result printed; 2 for arguments it cannot read; 1 for a
//! configuration it cannot load or a run the port could not take, the reason
//! on standard error.

use std::path::PathBuf;
use std::process::ExitCode;

use printobserver_oneharness::OneharnessSupervisor;
use printobserver_server::{ServerConfig, agent_config};

/// What the probe was asked to run.
struct Asked {
    /// The server configuration.
    config: PathBuf,
    /// The probe's session name.
    session: String,
    /// The prompt.
    prompt: String,
}

/// The three options, each once and none empty.
fn asked(mut arguments: impl Iterator<Item = String>) -> Result<Asked, String> {
    let (mut config, mut session, mut prompt) = (None, None, None);
    while let Some(flag) = arguments.next() {
        let slot = match flag.as_str() {
            "--config" => &mut config,
            "--session" => &mut session,
            "--prompt" => &mut prompt,
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
    Ok(Asked {
        config: PathBuf::from(config.ok_or("--config is required")?),
        session: session.ok_or("--session is required")?,
        prompt: prompt.ok_or("--prompt is required")?,
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
    let probed = ServerConfig::load(&asked.config)
        .map_err(|error| error.to_string())
        .and_then(|config| agent_config(&config).map_err(|error| error.to_string()))
        .and_then(|composed| {
            OneharnessSupervisor::open(composed).map_err(|error| error.to_string())
        })
        .and_then(|port| {
            port.probe(&asked.session, &asked.prompt)
                .map_err(|error| error.to_string())
        })
        .and_then(|result| serde_json::to_string(&result).map_err(|error| error.to_string()));
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
