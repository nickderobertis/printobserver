//! `printobserver`.
//!
//! Owns: the `printobserver` command — the single installable artifact of this
//! repository. Its `server` subcommand runs the supervisor; the rest of its
//! subcommands are the operator's surface onto a running one.
//!
//! May depend on: `printobserver-server`, `printobserver-core`,
//! `printobserver-types`, `printobserver-sdk` and the implementation crates —
//! it is the outermost composition root.
//!
//! # Why the argument reading is written out rather than derived
//!
//! This program takes one subcommand with one option today. A parser
//! dependency would be more code in the artifact than the code it replaced, and
//! the installed unit's own start command is one line this file has to admit
//! exactly; reading it here is what keeps the two legible together.

use std::path::PathBuf;
use std::process::ExitCode;

use printobserver_server::Server;

/// What the program was asked to do.
#[derive(Debug, PartialEq, Eq)]
enum Invocation {
    /// Print the command surface and exit successfully.
    Usage,
    /// Run the supervisor over one configuration file.
    Serve {
        /// The configuration file to run under.
        config: PathBuf,
    },
    /// The arguments do not name anything this program does.
    Refused {
        /// What to tell the caller.
        detail: String,
    },
}

/// The option `server` takes.
const CONFIG_OPTION: &str = "--config";

/// The command surface, which is also what an unknown invocation is answered
/// with.
const USAGE: &str = "\
printobserver — a supervision layer between a 3D printer and an agent.

Usage:
  printobserver server --config <path>   Run the supervisor.
  printobserver --help                   Show this.
";

/// Read one invocation out of the arguments, without a parser dependency.
fn read(arguments: &[String]) -> Invocation {
    match arguments.first().map(String::as_str) {
        None | Some("--help" | "-h" | "help") => Invocation::Usage,
        Some("server") => read_serve(&arguments[1..]),
        Some(other) => Invocation::Refused {
            detail: format!("`{other}` is not a subcommand of this program.\n\n{USAGE}"),
        },
    }
}

/// Read the `server` subcommand's own arguments.
fn read_serve(arguments: &[String]) -> Invocation {
    let mut config: Option<PathBuf> = None;
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        if argument != CONFIG_OPTION {
            return Invocation::Refused {
                detail: format!("`{argument}` is not an option of `server`.\n\n{USAGE}"),
            };
        }
        let Some(path) = rest.next() else {
            return Invocation::Refused {
                detail: format!("`{CONFIG_OPTION}` takes the path of a configuration file."),
            };
        };
        // A second one is refused rather than taken. This program reads ONE
        // configuration file, and silently running under the last of two is a
        // service running under a file nobody meant it to.
        if config.is_some() {
            return Invocation::Refused {
                detail: format!(
                    "`{CONFIG_OPTION}` was given twice, and this program runs under one \
                     configuration file."
                ),
            };
        }
        config = Some(PathBuf::from(path));
    }
    match config {
        Some(config) => Invocation::Serve { config },
        None => Invocation::Refused {
            detail: format!("`server` needs the one configuration file it runs under.\n\n{USAGE}"),
        },
    }
}

/// Run the supervisor until the service manager stops it.
async fn serve(config: PathBuf) -> Result<(), String> {
    let running = Server::start(&config)
        .await
        .map_err(|error| error.to_string())?;
    eprintln!("printobserver is serving on {}", running.address());
    running
        .serve_until_signalled()
        .await
        .map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match read(&arguments) {
        Invocation::Usage => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Invocation::Refused { detail } => {
            eprintln!("{detail}");
            ExitCode::FAILURE
        }
        Invocation::Serve { config } => {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("printobserver could not start a runtime: {error}");
                    return ExitCode::FAILURE;
                }
            };
            match runtime.block_on(serve(config)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(detail) => {
                    eprintln!("printobserver will not start: {detail}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
