//! The service managers the installer writes a definition for, and what the
//! tree states about each: the pair of commands `AGENTS.md`'s install path gives
//! it, and its table of `repo-policy.toml`'s `[service]`.
//!
//! Read from those two files rather than restated, so a test holding the
//! installer or the running service to them is held to what the tree says.

use std::path::{Path, PathBuf};

/// The repository root.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// The service managers the installer writes a definition for, spelled as
/// `AGENTS.md`'s supported-platform list and `repo-policy.toml` spell them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manager {
    /// Linux's: a unit under `/etc/systemd/system`.
    Systemd,
    /// macOS's: a property list under `/Library/LaunchDaemons`.
    Launchd,
}

impl Manager {
    /// The one this host runs a service under.
    pub fn host() -> Self {
        if cfg!(target_os = "macos") {
            Self::Launchd
        } else {
            Self::Systemd
        }
    }

    /// How `AGENTS.md` and `repo-policy.toml` spell it.
    pub fn spelled(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::Launchd => "launchd",
        }
    }

    /// This manager's table of `repo-policy.toml`'s `[service]`.
    pub fn policy(self) -> toml::Value {
        policy()["service"][self.spelled()].clone()
    }

    /// The pair of commands `AGENTS.md`'s install path states for this manager.
    pub fn pair(self) -> Vec<String> {
        let agents =
            std::fs::read_to_string(repo_root().join("AGENTS.md")).expect("AGENTS.md reads");
        let heading = format!("\n#### {}\n", self.spelled());
        let body = &agents[agents
            .find(&heading)
            .unwrap_or_else(|| panic!("AGENTS.md states no `{}` pair", self.spelled()))
            + heading.len()..];
        let body = &body[..body.find("\n### ").unwrap_or(body.len())];
        let body = &body[..body.find("\n#### ").unwrap_or(body.len())];
        let mut commands = Vec::new();
        let mut fenced = false;
        for line in body.lines() {
            match (fenced, line.trim()) {
                (false, "```console") => fenced = true,
                (true, "```") => fenced = false,
                (true, command) if !command.is_empty() => commands.push(command.to_owned()),
                _ => {}
            }
        }
        commands
    }

    /// Where, beneath a root, this manager's definition is written: the
    /// directory the policy states it loads definitions from at boot, and the
    /// file the install path's own start command names.
    pub fn definition(self, root: &Path) -> PathBuf {
        let start = self.pair().pop().expect("the pair states a start command");
        let named = start
            .split_whitespace()
            .last()
            .expect("the start command names the service");
        let file = Path::new(named)
            .file_name()
            .expect("the start command names a file or a unit");
        let directory = self.policy()["unit_directory"]
            .as_str()
            .expect("the policy states where the definition is written")
            .trim_start_matches('/')
            .to_owned();
        root.join(directory).join(file)
    }
}

/// `repo-policy.toml`, which states what the installer must not invoke and, per
/// service manager, where the definition goes and what starts it unattended.
pub fn policy() -> toml::Value {
    toml::from_str(
        &std::fs::read_to_string(repo_root().join("repo-policy.toml")).expect("the policy reads"),
    )
    .expect("the policy is a document")
}
