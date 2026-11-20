//! One probe of the shipped supervisor configuration, for the real-print suite.
//!
//! `printobserver-server-probe --config <path> --session <name> --prompt <text>
//! [--usage <path>] [--image <path>]`
//! loads a server configuration exactly as `printobserver server` does, composes
//! its supervisor port exactly as the server does, and runs one prompt through
//! it in a fresh session of its own with no print: the run request a turn is
//! built as, less everything a check adds — or, with `--image`, less everything
//! but the one check image a turn attaches, which is how what that image adds
//! to a call is measured. `--usage` names a file of
//! the program's usage lines, one per line, as `printobserver --help` prints
//! them, which the composed port is handed as the server running under
//! `printobserver server` is. It prints the engine's
//! result for that run as JSON on standard output, its per-call usage among
//! it, which is what the suite's base-context figure is read from.
//!
//! Exit 0 with the result printed; 2 for arguments it cannot read; 1 for a
//! configuration it cannot load or a run the port could not take, the reason
//! on standard error.

use std::path::PathBuf;
use std::process::ExitCode;

use printobserver_oneharness::OneharnessSupervisor;
use printobserver_server::{ServerConfig, agent_config_with};

/// What the probe was asked to run.
struct Asked {
    /// The server configuration.
    config: PathBuf,
    /// The probe's session name.
    session: String,
    /// The prompt.
    prompt: String,
    /// The file of the program's usage lines, when one was named.
    usage: Option<PathBuf>,
    /// The image to attach, when one was named.
    image: Option<PathBuf>,
}

/// The five options, each once and none empty, and `--usage` and `--image`
/// alone optional.
fn asked(mut arguments: impl Iterator<Item = String>) -> Result<Asked, String> {
    let (mut config, mut session, mut prompt, mut usage, mut image) =
        (None, None, None, None, None);
    while let Some(flag) = arguments.next() {
        let slot = match flag.as_str() {
            "--config" => &mut config,
            "--session" => &mut session,
            "--prompt" => &mut prompt,
            "--usage" => &mut usage,
            "--image" => &mut image,
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
        usage: usage.map(PathBuf::from),
        image: image.map(PathBuf::from),
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
    // An image named is held to a readable file here, so a probe never runs a
    // paid turn on an image the engine would refuse.
    if let Some(image) = &asked.image
        && let Err(error) = std::fs::metadata(image).and_then(|held| {
            if held.is_file() {
                Ok(())
            } else {
                Err(std::io::Error::other("it is no file"))
            }
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
        .and_then(|port| {
            port.probe_with_image(&asked.session, &asked.prompt, asked.image.clone())
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
