//! The one supervisor all three clients' printer-integration journeys drive.
//!
//! It is stood up by this repository's own tool rather than by each client in
//! its own language: a world built three times would be three worlds, and what
//! the three journeys are for is that the same nine steps against the same
//! supervisor come out the same in all three.
//!
//! The supervisor is real, the machine behind it is the `OctoPrint` `just
//! octoprint-up` started, and the print every step is about was opened through
//! the supervisor's own ingress. Closing this program's input brings it all
//! down.

use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Where the supervisor is, and what a journey acts on.
#[derive(Debug, Clone)]
pub struct Supervisor {
    /// Where it answers, as its own client configuration writes it.
    pub server: String,
    /// The print every step is about.
    pub print_id: String,
    /// The image the materialization step is about.
    pub image_id: String,
    /// The file the start step asks the machine to print.
    pub file_name: String,
}

/// A supervisor held up for as long as this is alive.
pub struct Standing {
    /// Where it is.
    pub at: Supervisor,
    /// The program holding it up.
    holding: Child,
}

impl Drop for Standing {
    fn drop(&mut self) {
        // Closing its input is how it is asked to stop, which is what it waits
        // for; killing it is what happens if it will not.
        drop(self.holding.stdin.take());
        if self.holding.wait().is_err() {
            let _ = self.holding.kill();
        }
    }
}

/// The repository this crate is in.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// The packages this repository's own tools live in, read from the justfile.
///
/// # Panics
///
/// Panics when the justfile exports none, which is a tree these tools are not
/// reachable in.
fn python_path(root: &Path) -> String {
    let justfile = std::fs::read_to_string(root.join("justfile")).expect("the justfile reads");
    justfile
        .lines()
        .find_map(|line| line.strip_prefix("export PYTHONPATH :="))
        .map(|value| value.trim().trim_matches('"').to_owned())
        .expect("the justfile exports the path this repository's tools live on")
}

/// Bring up a supervisor over the scripted `OctoPrint`, and hold it up.
///
/// # Panics
///
/// Panics when the environment is not up, naming the recipe that brings one
/// up: a tier that quietly passed against no printer would prove nothing.
pub fn standing(into: &Path) -> Standing {
    let root = repo_root();
    let mut holding = Command::new("uv")
        .args([
            "run",
            "-q",
            "python",
            "-m",
            "release_artifacts",
            "world",
            "--octoprint",
        ])
        .arg("--into")
        .arg(into)
        .current_dir(&root)
        .env("PYTHONPATH", python_path(&root))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the world this journey drives starts");

    let mut said = String::new();
    let mut reading = BufReader::new(holding.stdout.take().expect("the world says where it is"));
    reading
        .read_line(&mut said)
        .expect("the world says where it is");
    if said.trim().is_empty() {
        let mut why = String::new();
        if let Some(stderr) = holding.stderr.as_mut() {
            let _ = std::io::Read::read_to_string(stderr, &mut why);
        }
        let _ = holding.kill();
        panic!(
            "the world did not come up. Run `just octoprint-up` first; this tier drives \
             a real OctoPrint and has no fixture to fall back to.\n{why}"
        );
    }
    let described: serde_json::Value =
        serde_json::from_str(said.trim()).expect("the world says where it is, as a document");
    let named = |key: &str| {
        described[key]
            .as_str()
            .unwrap_or_else(|| panic!("the world says its `{key}`"))
            .to_owned()
    };
    Standing {
        at: Supervisor {
            server: named("server"),
            print_id: named("print_id"),
            image_id: named("image_id"),
            file_name: named("file_name"),
        },
        holding,
    }
}

/// Ask the world to stop, without waiting for it to be dropped.
impl Standing {
    /// Close the input the world waits on, which is how it is asked to stop.
    pub fn stop(&mut self) {
        drop(self.holding.stdin.take());
        let _ = self.holding.wait();
    }
}
