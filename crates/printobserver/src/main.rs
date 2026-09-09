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
//! This program takes two subcommands with three options between them. A parser
//! dependency would be more code in the artifact than the code it replaced, and
//! the installed unit's own start command is one line this file has to admit
//! exactly; reading it here is what keeps the two legible together.
//!
//! # Why the one request this program makes is written out too
//!
//! `context` asks a server on this program's **own host** one question and
//! prints what it answered. The typed client of that surface is
//! `printobserver-sdk`, which does not exist yet; until it does, one loopback
//! request written out here is less in the installed artifact than a TLS stack
//! would be, and it is the request the supervising agent's own turn makes.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;

use printobserver_server::Server;
use printobserver_types::PrintId;

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
    /// Read one print's whole context from a running supervisor.
    Context {
        /// Where that supervisor is answering.
        server: SocketAddr,
        /// The print to read.
        print: PrintId,
    },
    /// The arguments do not name anything this program does.
    Refused {
        /// What to tell the caller.
        detail: String,
    },
}

/// The option `server` takes.
const CONFIG_OPTION: &str = "--config";

/// The option `context` takes to find the running supervisor.
const SERVER_OPTION: &str = "--server";

/// The option `context` takes to name the print.
const PRINT_OPTION: &str = "--print";

/// The one scheme this program reaches a supervisor over: it is on this host.
const SCHEME: &str = "http://";

/// The command surface, which is also what an unknown invocation is answered
/// with.
const USAGE: &str = "\
printobserver — a supervision layer between a 3D printer and an agent.

Usage:
  printobserver server --config <path>              Run the supervisor.
  printobserver context --server <url> --print <id> Read one print's context.
  printobserver --help                              Show this.
";

/// Read one invocation out of the arguments, without a parser dependency.
fn read(arguments: &[String]) -> Invocation {
    match arguments.first().map(String::as_str) {
        None | Some("--help" | "-h" | "help") => Invocation::Usage,
        Some("server") => read_serve(&arguments[1..]),
        Some("context") => read_context(&arguments[1..]),
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

/// Read the `context` subcommand's own arguments.
fn read_context(arguments: &[String]) -> Invocation {
    let mut server: Option<String> = None;
    let mut print: Option<String> = None;
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let held = match argument.as_str() {
            SERVER_OPTION => &mut server,
            PRINT_OPTION => &mut print,
            other => {
                return Invocation::Refused {
                    detail: format!("`{other}` is not an option of `context`.\n\n{USAGE}"),
                };
            }
        };
        let Some(value) = rest.next() else {
            return Invocation::Refused {
                detail: format!("`{argument}` takes a value."),
            };
        };
        if held.is_some() {
            return Invocation::Refused {
                detail: format!("`{argument}` was given twice."),
            };
        }
        *held = Some(value.clone());
    }
    let (Some(server), Some(print)) = (server, print) else {
        return Invocation::Refused {
            detail: format!(
                "`context` needs the server it reads from and the print it reads.\n\n{USAGE}"
            ),
        };
    };
    // Both are parsed here rather than carried as text. The print reaches an
    // HTTP request target, and a value carrying a space or a line ending would
    // be a second request rather than a print; the identifier this system mints
    // admits neither.
    let Some(authority) = server.trim().strip_prefix(SCHEME) else {
        return Invocation::Refused {
            detail: format!(
                "`{server}` is not an address this program speaks to: it is \
                             reached over {SCHEME}"
            ),
        };
    };
    let Ok(server) = authority.trim_end_matches('/').parse::<SocketAddr>() else {
        return Invocation::Refused {
            detail: format!("`{authority}` is not an address and a port"),
        };
    };
    match print.parse::<PrintId>() {
        Ok(print) => Invocation::Context { server, print },
        Err(error) => Invocation::Refused {
            detail: format!("`{print}` is not a print this system minted: {error}"),
        },
    }
}

/// Read one print's whole context, and print what the supervisor answered.
fn context(server: SocketAddr, print: PrintId) -> Result<String, String> {
    let path = printobserver_server::operation("context")
        .ok_or_else(|| "this program serves no context read".to_owned())?
        .full_path()
        .replace("{print_id}", &print.to_string());
    let mut stream = TcpStream::connect(server)
        .map_err(|error| format!("nothing is answering at {server}: {error}"))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {server}\r\nAccept: {}\r\n\
         Connection: close\r\n\r\n",
        printobserver_server::MEDIA_TYPE
    )
    .map_err(|error| format!("the request could not be sent: {error}"))?;
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .map_err(|error| format!("the answer could not be read: {error}"))?;
    let (status, body) = read_answer(&answer)?;
    if status != 200 {
        return Err(format!(
            "the supervisor answered {status} for print {print}: {body}"
        ));
    }
    Ok(body)
}

/// The status and the whole body of one HTTP answer.
///
/// The body is taken as the length the answer declares rather than as whatever
/// arrived: an answer that declared more than it sent is a truncated context,
/// and printing it would hand the agent a document that parses as less than the
/// supervisor said.
fn read_answer(answer: &str) -> Result<(u16, String), String> {
    let (head, body) = answer
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("the supervisor answered something unreadable: {answer}"))?;
    let status = head
        .split_whitespace()
        .nth(1)
        .filter(|_| head.starts_with("HTTP/1.1 "))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| format!("the supervisor answered no status: {head}"))?;
    let declared = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .ok_or_else(|| format!("the supervisor declared no length for what it answered: {head}"))?;
    if body.len() != declared {
        return Err(format!(
            "the supervisor declared {declared} bytes and sent {}",
            body.len()
        ));
    }
    Ok((status, body.to_owned()))
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
        Invocation::Context { server, print } => match context(server, print) {
            Ok(answered) => {
                println!("{answered}");
                ExitCode::SUCCESS
            }
            Err(detail) => {
                eprintln!("printobserver could not read that context: {detail}");
                ExitCode::FAILURE
            }
        },
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
