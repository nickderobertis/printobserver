//! A stand-in for a harness program whose supervision turn holds its print
//! until a journey lets it go.
//!
//! The supervisor holds a print's turn from before it reads the machine for
//! that turn until the harness it ran returns, and no read that finds the
//! machine's job over ends a print whose turn is held. So a journey that needs
//! a print to stay open while it tells the machine something every other read
//! would end it for puts this program on the supervisor's path as the harness,
//! raises a turn on that print, and waits for the turn to say it is running.
//!
//! Run as a turn, it reads its prompt for the print the prompt names, writes
//! [`HELD`] followed by that print's identifier into its own directory once it
//! has found it, and returns — refusing the turn — only once [`HOLD`] is gone
//! from that directory. Asked for its version, it answers one, as a harness on
//! a path is asked before it is run. Nothing else is read: no variable names a
//! directory, because the supervisor runs a harness with its own environment.
//!
//! Built only under the `test-fixtures` feature, so nothing an ordinary
//! consumer installs carries it.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The file whose presence beside this program holds every turn it runs.
const HOLD: &str = "hold";

/// What the file a running turn writes is named, before its print.
const HELD: &str = "held-";

/// What the prompt names its print after.
const PRINT_OPTION: &str = "--print-id ";

/// How long a print identifier is.
const PRINT_ID_LEN: usize = 36;

/// How often a held turn looks for its hold again.
const LOOK_AGAIN: std::time::Duration = std::time::Duration::from_millis(20);

fn main() -> ExitCode {
    if std::env::args().any(|argument| argument == "--version") {
        println!("0.0.0 (a stand-in harness that holds its turn)");
        return ExitCode::SUCCESS;
    }
    let Some(directory) = std::env::current_exe()
        .ok()
        .and_then(|program| program.parent().map(Path::to_path_buf))
    else {
        eprintln!("this stand-in cannot find its own directory");
        return ExitCode::FAILURE;
    };
    let Some(print) = print_in_prompt() else {
        eprintln!("this stand-in found no print in its prompt");
        return ExitCode::FAILURE;
    };
    if let Err(error) = say_held(&directory, &print) {
        eprintln!("this stand-in could not say its turn is held: {error}");
        return ExitCode::FAILURE;
    }
    while directory.join(HOLD).exists() {
        std::thread::sleep(LOOK_AGAIN);
    }
    eprintln!("this stand-in held its turn on {print} and answers nothing");
    ExitCode::FAILURE
}

/// Read standard input until the print the prompt names, and keep reading the
/// rest of it on a thread of its own, so the caller writing the prompt never
/// waits on a pipe nobody empties.
fn print_in_prompt() -> Option<String> {
    let mut input = std::io::stdin().lock();
    let mut read = Vec::new();
    let mut buffer = [0_u8; 4096];
    let found = loop {
        let taken = input.read(&mut buffer).ok()?;
        if taken == 0 {
            break None;
        }
        read.extend_from_slice(&buffer[..taken]);
        let text = String::from_utf8_lossy(&read);
        if let Some(at) = text.find(PRINT_OPTION) {
            let named: String = text[at + PRINT_OPTION.len()..]
                .chars()
                .take_while(|character| character.is_ascii_hexdigit() || *character == '-')
                .collect();
            if named.len() == PRINT_ID_LEN {
                break Some(named);
            }
        }
    };
    drop(input);
    std::thread::spawn(|| std::io::copy(&mut std::io::stdin(), &mut std::io::sink()));
    found
}

/// Write the file saying this print's turn is running, whole: beside its name
/// first, then renamed into place, so a look never finds half of it.
fn say_held(directory: &Path, print: &str) -> std::io::Result<()> {
    let held: PathBuf = directory.join(format!("{HELD}{print}"));
    let part = directory.join(format!("{HELD}{print}.part"));
    std::fs::write(&part, print)?;
    std::fs::rename(part, held)
}
