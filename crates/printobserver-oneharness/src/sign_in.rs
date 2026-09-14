//! The harness identities this program can sign in, and where each keeps it.
//!
//! A harness keeps its configuration and its authentication in one directory,
//! and one environment variable selects which. The service runs as a system
//! user with no home and cannot see anybody else's, so every identity here
//! keeps that directory under the state directory instead — the one place the
//! service's unit lets it write — and the sign-in command and every supervision
//! turn are pointed at the same one.
//!
//! [`SIGN_INS`] is the whole of what that takes, as data. Each variable is
//! `OneHarness`'s own constant rather than a spelling of it, and each program
//! is held by this crate's own tests to the one `OneHarness`'s registry runs
//! for that identity. Nothing here runs either: the command-line program runs
//! the sign-in, and `OneHarness` runs the turns.

use std::path::{Path, PathBuf};

use oneharness_core::io::usage::{CLAUDE_IDENTITY_ENV, CODEX_IDENTITY_ENV};

use crate::config::{ConfigError, EnvAssignment, HarnessIdentity};

/// The directory under the state directory every identity's own directory is
/// kept in.
pub const HARNESS_DIRECTORY: &str = "harness";

/// One harness identity this program can sign in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessSignIn {
    /// The identity, as `OneHarness` names it and a configuration selects it.
    // llmlint: ignore[invalid_states_unrepresentable] `HarnessIdentity` owns a `String` and cannot be built in a const table. The fields are private, so `SIGN_INS` is the only place an entry is constructed, and this crate's tests hold every identity to one OneHarness's registry runs.
    identity: &'static str,
    /// The variable selecting the directory that harness keeps its sign-in in.
    config_env: &'static str,
    /// The program `OneHarness` runs for that identity.
    program: &'static str,
    /// The arguments of that harness's own interactive sign-in.
    arguments: &'static [&'static str],
}

/// Every harness identity this program can sign in, and there is no other.
///
/// Each sign-in is the one that harness's own help names: `claude auth login`
/// ("Sign in to your Anthropic account"), and `codex login` in its device-code
/// form, which is the one that completes on a machine with no browser — the
/// board beside a printer is reached over a terminal.
pub const SIGN_INS: [HarnessSignIn; 2] = [
    HarnessSignIn {
        identity: "claude-code",
        config_env: CLAUDE_IDENTITY_ENV,
        program: "claude",
        // llmlint: ignore[contracts_have_one_source_or_a_drift_gate] A third-party CLI's login command has no machine-readable source: OneHarness declares each harness's program, which the tests hold `program` to, but not its sign-in. This is read from `claude auth login --help`, and a gate drift check would have to install and run the paid provider's CLI.
        arguments: &["auth", "login"],
    },
    HarnessSignIn {
        identity: "codex",
        config_env: CODEX_IDENTITY_ENV,
        program: "codex",
        // llmlint: ignore[contracts_have_one_source_or_a_drift_gate] A third-party CLI's login command has no machine-readable source: OneHarness declares each harness's program, which the tests hold `program` to, but not its sign-in. This is read from `codex login --help`, and a gate drift check would have to install and run the paid provider's CLI.
        arguments: &["login", "--device-auth"],
    },
];

impl HarnessSignIn {
    /// The entry for one configured harness, when this program can sign it in.
    #[must_use]
    pub fn of(harness: &HarnessIdentity) -> Option<&'static Self> {
        SIGN_INS
            .iter()
            .find(|entry| entry.identity == harness.as_str())
    }

    /// Every identity this program can sign in, in the order declared.
    #[must_use]
    pub fn supported() -> Vec<&'static str> {
        SIGN_INS.iter().map(|entry| entry.identity).collect()
    }

    /// The identity, as a configuration selects it.
    #[must_use]
    pub const fn identity(&self) -> &'static str {
        self.identity
    }

    /// The variable selecting the directory this harness keeps its sign-in in.
    #[must_use]
    pub const fn config_env(&self) -> &'static str {
        self.config_env
    }

    /// The program `OneHarness` runs for this identity, which is the program
    /// the sign-in is made with.
    #[must_use]
    pub const fn program(&self) -> &'static str {
        self.program
    }

    /// The arguments of this harness's own interactive sign-in.
    #[must_use]
    pub const fn arguments(&self) -> &'static [&'static str] {
        self.arguments
    }

    /// The directory this identity keeps its sign-in in, under one state
    /// directory.
    #[must_use]
    pub fn directory(&self, state_dir: &Path) -> PathBuf {
        state_dir.join(HARNESS_DIRECTORY).join(self.identity)
    }

    /// That directory, created where it is not there yet, and readable by its
    /// owner alone whether it was just created or was already there.
    ///
    /// The state directory is the one this program was configured with, which
    /// is resolved where it is configured. Below it, each of the two levels is
    /// created on its own rather than recursively, so neither is ever reached
    /// through a symlink.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::ErrorKind::InvalidInput`] when something other than a
    /// directory is at either level — a symlink included, since a sign-in kept
    /// through one is kept wherever it points — and the operating system's own
    /// error when the state directory is not there, or a level cannot be created
    /// or made private.
    pub fn prepare(&self, state_dir: &Path) -> std::io::Result<PathBuf> {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = self.directory(state_dir);
        for level in [state_dir.join(HARNESS_DIRECTORY), directory.clone()] {
            real_directory(&level)?;
            // Stated on every preparation rather than trusted: a creation mode
            // is narrowed by the process's own mask, and a directory that was
            // already there may have been widened since. Only its owner can
            // change its mode, so a directory another user owns is refused.
            std::fs::set_permissions(&level, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(directory)
    }

    /// The assignment that points this harness at one directory.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::EnvAssignmentMalformed`] when the directory's
    /// path is not text an environment can carry.
    pub fn assignment(&self, directory: &Path) -> Result<EnvAssignment, ConfigError> {
        EnvAssignment::new(&format!("{}={}", self.config_env, directory.display()))
    }
}

/// One directory, created with no access for anybody but its owner where it
/// is not there, and refused where something other than a directory is.
fn real_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;

    match std::fs::symlink_metadata(path) {
        Ok(found) if found.is_dir() => Ok(()),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is something other than a directory", path.display()),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::DirBuilder::new().mode(0o700).create(path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use oneharness_core::io::usage::{CLAUDE_IDENTITY_ENV, CODEX_IDENTITY_ENV};

    use super::{HARNESS_DIRECTORY, HarnessSignIn, SIGN_INS};
    use crate::config::HarnessIdentity;

    /// The table carries the two identities a supervisor is configured with,
    /// each selected by `OneHarness`'s own variable for it.
    #[test]
    fn the_table_carries_claude_code_and_codex_under_oneharnesss_own_variables() {
        let found: Vec<(&str, &str)> = SIGN_INS
            .iter()
            .map(|entry| (entry.identity(), entry.config_env()))
            .collect();

        assert_eq!(
            found,
            vec![
                ("claude-code", CLAUDE_IDENTITY_ENV),
                ("codex", CODEX_IDENTITY_ENV)
            ]
        );
    }

    /// Every identity is one `OneHarness` runs, and the program is the one its
    /// registry names for it — so a registry that renames a program fails here.
    #[test]
    fn every_identity_is_one_oneharness_runs() {
        for entry in &SIGN_INS {
            let spec = oneharness_core::domain::harness::by_id(entry.identity())
                .unwrap_or_else(|| panic!("OneHarness runs no `{}`", entry.identity()));
            assert_eq!(entry.program(), spec.default_bin);
            assert!(
                !entry.arguments().is_empty(),
                "`{}` declares no sign-in",
                entry.identity()
            );
        }
    }

    /// A configured harness is found by its identity, and one outside the
    /// table is not.
    #[test]
    fn a_harness_outside_the_table_has_no_entry() {
        let codex = HarnessIdentity::new("codex").expect("an identity");
        let copilot = HarnessIdentity::new("copilot").expect("an identity");

        assert_eq!(
            HarnessSignIn::of(&codex).map(HarnessSignIn::identity),
            Some("codex")
        );
        assert_eq!(HarnessSignIn::of(&copilot), None);
        assert_eq!(HarnessSignIn::supported(), vec!["claude-code", "codex"]);
    }

    /// The directory is created private under the state directory, and one
    /// already there keeps what it holds and is made private again.
    #[test]
    fn the_directory_is_created_private_under_the_state_directory() {
        let state = std::env::temp_dir().join(format!(
            "printobserver-sign-in-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock is past the epoch")
                .as_nanos()
        ));
        let entry = &SIGN_INS[0];
        std::fs::create_dir_all(&state).expect("a state directory");

        let prepared = entry.prepare(&state).expect("the directory is created");

        assert_eq!(prepared, state.join(HARNESS_DIRECTORY).join("claude-code"));
        let mode = std::fs::metadata(&prepared)
            .expect("the directory is there")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "the directory is mode {mode:o}");

        std::fs::write(prepared.join("signed-in"), "kept").expect("the directory is writable");
        std::fs::set_permissions(&prepared, std::fs::Permissions::from_mode(0o755))
            .expect("the directory is widened");
        let again = entry.prepare(&state).expect("the directory is there");
        let narrowed = std::fs::metadata(&again)
            .map(|found| found.permissions().mode() & 0o777)
            .expect("the directory is there");
        let kept = std::fs::read_to_string(again.join("signed-in"));
        let assigned = entry.assignment(&again).map(|found| found.to_string());

        // A symlink where a directory belongs is refused rather than followed.
        let elsewhere = state.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("a directory to point at");
        std::os::unix::fs::symlink(&elsewhere, SIGN_INS[1].directory(&state))
            .expect("a symlink in the directory's place");
        let through_a_symlink = SIGN_INS[1].prepare(&state).map_err(|error| error.kind());

        // And one at the level every harness's directory is kept in.
        let linked_state = state.join("linked-state");
        std::fs::create_dir_all(&linked_state).expect("a second state directory");
        std::os::unix::fs::symlink(&elsewhere, linked_state.join(HARNESS_DIRECTORY))
            .expect("a symlink in the harness level's place");
        let through_a_linked_level = entry.prepare(&linked_state).map_err(|error| error.kind());
        std::fs::remove_dir_all(&state).expect("the state directory is removable");

        assert_eq!(
            narrowed, 0o700,
            "a widened directory was left mode {narrowed:o}"
        );
        assert_eq!(kept.expect("what it held is kept"), "kept");
        assert_eq!(
            through_a_symlink,
            Err(std::io::ErrorKind::InvalidInput),
            "a symlink in the directory's place was prepared"
        );
        assert_eq!(
            through_a_linked_level,
            Err(std::io::ErrorKind::InvalidInput),
            "a symlink in the harness level's place was followed"
        );
        assert_eq!(
            assigned.expect("an assignment"),
            format!("{CLAUDE_IDENTITY_ENV}={}", again.display())
        );
    }
}
