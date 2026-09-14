//! Signing the supervising agent's harness in, as the user the service runs as.
//!
//! # Where the sign-in is kept
//!
//! The service runs as a system user with no home, under a unit that hides
//! every home on the machine, and on every event it runs the harness its
//! configuration names. So a harness this program can sign in keeps its sign-in
//! under the state directory — the one place that unit lets the service write —
//! in the directory [`HarnessSignIn::directory`] names, and this command and
//! every supervision turn are pointed at that same directory by the variable
//! the adapter's table declares for it.
//!
//! # What this command does, and what it does not
//!
//! It reads two values from the server's own configuration file, creates that
//! directory, and runs the harness's own interactive sign-in with the terminal
//! it was run from, exiting with whatever the harness exits with. It starts no
//! server, listens on nothing, writes nothing a running server would, and
//! reaches no printer and no failure detector — so an operator signs the agent
//! in before or after filling any of those in.

use std::path::Path;
use std::process::{Command, ExitStatus};

use printobserver_server::{HarnessSignIn, SignInConfig};

use crate::failure::{Exit, Failure};

/// The status this program exits with when the harness was ended by a signal,
/// less the signal's own number, as a shell reports one.
const SIGNALLED: u8 = 128;

/// Sign the harness one configuration file names in, and answer the status the
/// harness exited with.
///
/// # Errors
///
/// Returns a failure of [`Exit::Unconfigured`] — before anything is run — when
/// the file cannot be read for its state directory and its harness, when that
/// harness is not one this program can sign in, when the directory its sign-in
/// is kept in cannot be created, and when the harness's program cannot be run
/// by the user this runs as.
pub fn sign_in(config: &Path) -> Result<u8, Failure> {
    let configured = SignInConfig::load(config).map_err(|error| {
        Failure::of(
            Exit::Unconfigured,
            format!(
                "{error}. Signing in reads `state_dir` and `supervisor.harness` from the \
                 server's own configuration; name it with `--config`."
            ),
        )
    })?;
    let Some(harness) = HarnessSignIn::of(&configured.harness) else {
        return Err(Failure::of(
            Exit::Unconfigured,
            format!(
                "`supervisor.harness` in {} is `{}`, and this program signs in only {}. \
                 Configure one of those, or sign `{}` in for the service's user by its own \
                 means.",
                config.display(),
                configured.harness,
                HarnessSignIn::supported()
                    .iter()
                    .map(|identity| format!("`{identity}`"))
                    .collect::<Vec<_>>()
                    .join(" and "),
                configured.harness,
            ),
        ));
    };
    let directory = harness.prepare(&configured.state_dir).map_err(|error| {
        Failure::of(
            Exit::Unconfigured,
            format!(
                "{} could not be created: {error}. Run this as the user the service runs \
                 as, which owns `state_dir`.",
                harness.directory(&configured.state_dir).display()
            ),
        )
    })?;
    eprintln!(
        "printobserver: signing {} in with `{} {}`, keeping its sign-in in {}",
        harness.identity(),
        harness.program(),
        harness.arguments().join(" "),
        directory.display()
    );
    // Run inside its own directory rather than wherever this was run from: the
    // documented invocation is `sudo -u` from an operator's own shell, whose
    // working directory is somewhere the service's user cannot read.
    let status = Command::new(harness.program())
        .args(harness.arguments())
        .env(harness.config_env(), &directory)
        .current_dir(&directory)
        .status()
        .map_err(|error| {
            Failure::of(
                Exit::Unconfigured,
                format!(
                    "`{}` could not be run: {error}. Install the {} harness where this \
                     user's PATH finds it, then sign in again.",
                    harness.program(),
                    harness.identity()
                ),
            )
        })?;
    // llmlint: ignore[cli_output_contract] suppressions.toml has the reason.
    Ok(exited_with(status))
}

/// The status to exit with, for a harness that exited with this one.
fn exited_with(status: ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt as _;

    match (status.code(), status.signal()) {
        (Some(code), _) => u8::try_from(code).unwrap_or(u8::MAX),
        (None, Some(signal)) => {
            SIGNALLED.saturating_add(u8::try_from(signal).unwrap_or(u8::MAX - SIGNALLED))
        }
        (None, None) => u8::MAX,
    }
}
