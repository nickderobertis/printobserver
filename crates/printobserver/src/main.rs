//! The `printobserver` command.
//!
//! Everything this file does is dispatch: reading one invocation, and either
//! running the supervisor or making the one request that invocation names. The
//! surface itself, the configuration, the transport and the two renderings are
//! [`printobserver`]'s own modules, so this crate's tier can drive them and
//! build a variant of this program with one defect in it.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use printobserver::client::{perform, refusal};
use printobserver::failure::{Exit, Failure};
use printobserver::parse::{Invocation, parse};
use printobserver::surface::{usage, version};
use printobserver_server::Server;

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

/// Run the supervisor, on a runtime of this program's own.
fn run_server(config: PathBuf) -> Exit {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("printobserver could not start a runtime: {error}");
            return Exit::Refused;
        }
    };
    match runtime.block_on(serve(config)) {
        Ok(()) => Exit::Success,
        Err(detail) => {
            eprintln!("printobserver will not start: {detail}");
            Exit::Unconfigured
        }
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match parse(&arguments) {
        Invocation::Usage => {
            print!("{}", usage());
            return ExitCode::SUCCESS;
        }
        Invocation::Version => {
            print!("{}", version());
            return ExitCode::SUCCESS;
        }
        Invocation::Serve { config } => return ExitCode::from(run_server(config).status()),
        Invocation::Refused { detail } => refusal(&Failure::of(Exit::Usage, detail)),
        Invocation::Call(call) => perform(&call),
    };
    let mut out = std::io::stdout();
    let _ = out.write_all(outcome.out.as_bytes());
    let _ = out.flush();
    let mut err = std::io::stderr();
    let _ = err.write_all(outcome.err.as_bytes());
    let _ = err.flush();
    ExitCode::from(outcome.exit.status())
}
