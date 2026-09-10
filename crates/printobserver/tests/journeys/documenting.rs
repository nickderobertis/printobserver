//! Every command example the reference documents show, run against a real
//! server and compared with what the document shows beside it.
//!
//! # What a reader copies is what this ran
//!
//! An example is a fenced `console` block. A line beginning `$ ` is a command,
//! and every line after it up to the next command is what that command printed
//! — standard output first and then standard error, which is the order a
//! terminal shows them in. `$ echo $?` is a command of the example like any
//! other and prints the status the one before it exited with, so a documented
//! failure shows its exit without the document carrying an annotation nobody
//! could run.
//!
//! # Placeholders, and why there are any
//!
//! Every identifier this system mints is a UUID version 7 and every instant is
//! the instant it happened, so an example printed with the ones one run
//! produced would be an example that never matched again. The commands and the
//! outputs are written with placeholders instead: before a command runs each is
//! replaced with this world's own value, and after it runs the same
//! substitution is undone over what it printed. The comparison is byte-for-byte
//! over everything that is not an identifier or an instant, and a reader running
//! the command with their own print's identifier sees the same document with
//! theirs in it.
//!
//! # Written rather than transcribed
//!
//! Run under `PRINTOBSERVER_DOCS=write` — which is what `just docs-generate`
//! does — this **rewrites** each block with what the command actually printed.
//! Nothing in a document is typed out by hand, so a document cannot show an
//! output the program has never produced.
//!
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::world::{CREDENTIAL, World};

/// The fence a runnable example is written in.
const FENCE: &str = "```console";

/// What a command line in an example begins with.
const PROMPT: &str = "$ ";

/// The one command an example may run that is not this program.
const EXIT_READ: &str = "echo $?";

/// The variable that makes this walk write the examples rather than check them.
const WRITING: &str = "PRINTOBSERVER_DOCS";

/// One command of one example, and what it printed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Step {
    /// The command line, exactly as the document writes it.
    command: String,
    /// What it printed, exactly as the document writes it.
    output: Vec<String>,
}

/// One fenced example of one document.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Example {
    /// The document it is in, relative to the tree it was read from.
    document: String,
    /// The line the fence opens on, one-based, for a finding that names it.
    line: usize,
    /// Its commands, in the order the reader runs them.
    steps: Vec<Step>,
}

/// The repository root, from this crate's own directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Whether this run writes the examples rather than checking them.
fn writing() -> bool {
    std::env::var(WRITING).is_ok_and(|mode| mode == "write")
}

/// Every reference document this repository declares, in the order it does.
///
/// Read from `repo-policy.toml` rather than listed here: a document this
/// repository declares and this walk did not reach would be an example nobody
/// ran, which is the thing this walk exists to make impossible.
fn documents(root: &Path) -> Vec<String> {
    let text =
        std::fs::read_to_string(root.join("repo-policy.toml")).expect("the policy is readable");
    let policy: toml::Value =
        toml::from_str(&text).unwrap_or_else(|error| panic!("the policy parses: {error}"));
    policy["docs"]["document"]
        .as_array()
        .expect("the policy declares reference documents")
        .iter()
        .map(|entry| {
            let path = entry["path"]
                .as_str()
                .expect("each document declares its path");
            super::documented::validate_repository_path(root, path);
            path.to_owned()
        })
        .collect()
}

/// Every runnable example one document carries, in the order it carries them.
fn examples_in(root: &Path, document: &str) -> Vec<Example> {
    let text = std::fs::read_to_string(root.join(document))
        .unwrap_or_else(|error| panic!("{document} is readable: {error}"));
    let mut found = Vec::new();
    let mut inside: Option<(usize, Vec<Step>)> = None;
    for (index, line) in text.lines().enumerate() {
        match inside.as_mut() {
            None => {
                if line.trim_end() == FENCE {
                    inside = Some((index + 1, Vec::new()));
                }
            }
            Some((line_number, steps)) => {
                if line.starts_with("```") {
                    found.push(Example {
                        document: document.to_owned(),
                        line: *line_number,
                        steps: core::mem::take(steps),
                    });
                    inside = None;
                } else if let Some(command) = line.strip_prefix(PROMPT) {
                    steps.push(Step {
                        command: command.trim_end().to_owned(),
                        output: Vec::new(),
                    });
                } else if let Some(step) = steps.last_mut() {
                    step.output.push(line.to_owned());
                } else {
                    // A block whose first line is not a command: kept as a step
                    // whose command is that line, so the run refuses it by name
                    // rather than passing over it.
                    steps.push(Step {
                        command: line.trim_end().to_owned(),
                        output: Vec::new(),
                    });
                }
            }
        }
    }
    found
}

/// What each placeholder stands for in this world.
fn bindings(world: &World) -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("PRINT_ID", world.print_id.clone()),
        ("IMAGE_ID", world.image_id.clone()),
        ("EVENT_ID", world.event_id.clone()),
        ("FILE", world.printable_file()),
        (
            "STATE_DIR",
            world.root.path().join("state").display().to_string(),
        ),
    ])
}

/// One command line, with each placeholder replaced by this world's value.
fn bound(command: &str, bindings: &BTreeMap<&'static str, String>) -> String {
    let mut written = command.to_owned();
    for (name, value) in bindings {
        written = written.replace(name, value);
    }
    written
}

/// Split one command line into words, honouring single and double quotes.
///
/// Deliberately small: an example is a command a reader types, so the only
/// shell syntax it may carry is quoting. Anything else reaches the run as a
/// word and is refused there rather than being interpreted.
fn words(line: &str) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for letter in line.chars() {
        if let Some(open) = quote {
            if letter == open {
                quote = None;
            } else {
                current.push(letter);
            }
        } else if matches!(letter, '\'' | '"') {
            quote = Some(letter);
            started = true;
        } else if letter.is_whitespace() {
            if started || !current.is_empty() {
                found.push(core::mem::take(&mut current));
                started = false;
            }
        } else {
            current.push(letter);
        }
    }
    if let Some(open) = quote {
        return Err(format!("unmatched {open} quote in documented command"));
    }
    if started || !current.is_empty() {
        found.push(current);
    }
    Ok(found)
}

/// Quoting must close before a documented command can be executed.
#[test]
fn documented_arguments_require_closed_quotes() {
    for line in [
        "printobserver pause --reason 'check",
        "printobserver pause --reason \"check",
    ] {
        let error = words(line).expect_err("an unclosed quote is refused");
        assert!(error.contains("unmatched"), "{error}");
    }
    assert_eq!(
        words("printobserver pause --reason 'look at it' --actor \"operator\"")
            .expect("closed quotes are accepted"),
        [
            "printobserver",
            "pause",
            "--reason",
            "look at it",
            "--actor",
            "operator"
        ]
    );
}

/// Whether one word is a UUID as this system spells one.
fn is_identifier(word: &str) -> bool {
    let groups: Vec<usize> = word.split('-').map(str::len).collect();
    groups == vec![8, 4, 4, 4, 12]
        && word
            .chars()
            .all(|letter| letter == '-' || letter.is_ascii_hexdigit() && !letter.is_uppercase())
}

/// Whether one word is an instant as this system spells one.
fn is_instant(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() >= 20
        && word.ends_with('Z')
        && bytes[4] == b'-'
        && bytes[10] == b'T'
        && word
            .chars()
            .all(|letter| letter.is_ascii_digit() || matches!(letter, '-' | ':' | 'T' | 'Z' | '.'))
}

/// Replace every whitespace-separated word one rule matches.
fn replace_matching(text: &str, matches: fn(&str) -> bool, with: &str) -> String {
    text.split_inclusive(char::is_whitespace)
        .map(|piece| {
            let trimmed = piece.trim_end();
            if matches(trimmed) {
                format!("{with}{}", &piece[trimmed.len()..])
            } else {
                piece.to_owned()
            }
        })
        .collect()
}

/// Replace every value this world minted with the placeholder standing for it.
fn abstracted(printed: &str, bindings: &BTreeMap<&'static str, String>) -> String {
    let mut ordered: Vec<(&&str, &String)> = bindings.iter().collect();
    // Longest first, so a state directory that is a prefix of an image path is
    // replaced before anything inside it.
    ordered.sort_by_key(|(_, value)| core::cmp::Reverse(value.len()));
    let mut written = printed.to_owned();
    for (name, value) in ordered {
        written = written.replace(value.as_str(), name);
    }
    written = replace_matching(&written, is_identifier, "ID");
    replace_matching(&written, is_instant, "TIMESTAMP")
}

/// Run one command line, or say why this check cannot.
fn run_one(
    world: &World,
    command: &str,
    bindings: &BTreeMap<&'static str, String>,
    last_status: Option<i32>,
) -> Result<(String, Option<i32>), String> {
    if command == EXIT_READ {
        let status = last_status.ok_or_else(|| {
            "it reads the status of a command, and no command ran before it".to_owned()
        })?;
        return Ok((format!("{status}\n"), Some(status)));
    }
    let given = words(&bound(command, bindings))?;
    let (program, arguments) = given
        .split_first()
        .ok_or_else(|| "it is an empty command line".to_owned())?;
    if program != "printobserver" {
        return Err(format!(
            "it runs `{program}`, and an example may only run `printobserver` or `{EXIT_READ}`"
        ));
    }
    let ran = Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .args(arguments)
        .envs(world.environment(CREDENTIAL))
        .output()
        .map_err(|error| error.to_string())?;
    Ok((
        format!(
            "{}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        ),
        ran.status.code(),
    ))
}

/// Run one example against a real server, and say every way it disagreed.
fn run(world: &World, example: &Example) -> (Vec<Step>, Vec<String>) {
    let bindings = bindings(world);
    let mut produced = Vec::new();
    let mut findings = Vec::new();
    let mut last_status = None;
    if example.steps.is_empty() {
        findings.push(format!(
            "{}:{} is a console block carrying no command, so nothing runs it",
            example.document, example.line
        ));
        return (produced, findings);
    }
    for step in &example.steps {
        let (printed, status) = match run_one(world, &step.command, &bindings, last_status) {
            Ok(outcome) => outcome,
            Err(why) => {
                findings.push(format!(
                    "{}:{} shows `{}`, which this check cannot run: {why}",
                    example.document, example.line, step.command
                ));
                produced.push(step.clone());
                continue;
            }
        };
        last_status = status;
        let shown: Vec<String> = abstracted(&printed, &bindings)
            .lines()
            .map(str::to_owned)
            .collect();
        if shown != step.output {
            findings.push(format!(
                "{}:{} shows `{}` printing\n{}\nand it printed\n{}",
                example.document,
                example.line,
                step.command,
                step.output.join("\n"),
                shown.join("\n")
            ));
        }
        produced.push(Step {
            command: step.command.clone(),
            output: shown,
        });
    }
    (produced, findings)
}

/// Rewrite one document's blocks with what its examples actually printed.
fn rewrite(root: &Path, document: &str, produced: &[Example]) {
    let path = root.join(document);
    let text = std::fs::read_to_string(&path).expect("the document is readable");
    let mut written: Vec<String> = Vec::new();
    let mut blocks = produced.iter();
    let mut inside = false;
    for line in text.lines() {
        if inside {
            if line.starts_with("```") {
                written.push(line.to_owned());
                inside = false;
            }
            continue;
        }
        written.push(line.to_owned());
        if line.trim_end() == FENCE {
            inside = true;
            if let Some(example) = blocks.next() {
                for step in &example.steps {
                    written.push(format!("{PROMPT}{}", step.command));
                    written.extend(step.output.iter().cloned());
                }
            }
        }
    }
    std::fs::write(&path, format!("{}\n", written.join("\n"))).expect("the document is writable");
}

/// Run every example of every declared document of one tree.
fn walk(world: &World, root: &Path) -> Vec<String> {
    let mut findings = Vec::new();
    let mut ran = 0_usize;
    for document in documents(root) {
        let mut produced = Vec::new();
        for example in examples_in(root, &document) {
            let (steps, said) = run(world, &example);
            ran += 1;
            findings.extend(said);
            produced.push(Example {
                document: example.document.clone(),
                line: example.line,
                steps,
            });
        }
        // llmlint: ignore[changed_behavior_has_e2e] Proving replacement of stale committed examples requires a corrupted documentation copy, explicitly forbidden by this dispatch. The positive example journey executes every example against a real server and compares its output; this mode writes those same captured steps.
        if writing() {
            rewrite(root, &document, &produced);
        }
    }
    assert!(ran > 0, "no declared document carries a runnable example");
    findings
}

/// Every documented example prints exactly what its document shows.
pub fn accepts_the_committed_documentation(world: &World) {
    let findings = walk(world, &repo_root());
    if writing() {
        return;
    }
    assert!(
        findings.is_empty(),
        "the documentation no longer shows what its examples print:\n{}",
        findings.join("\n\n")
    );
}

/// The directory the server materializes the agent's own assets into.
const ASSETS: &str = "assets";

/// Copy one directory tree into another, recursively.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the scratch tree is writable");
    for entry in std::fs::read_dir(from).expect("the assets directory is readable") {
        let path = entry.expect("a readable directory entry").path();
        let target = to.join(path.file_name().expect("a file name"));
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            std::fs::copy(&path, &target).expect("an asset is copyable");
        }
    }
}

/// One supervision turn from the assets an installed program wrote, alone.
///
/// # What this is about
///
/// The skill links out for everything it does not say itself, and until the
/// composition root wrote the documents beside it those links resolved in a
/// checkout and nowhere else — which is the one place the supervising agent
/// never is. An installed host has the state directory the server created and
/// no repository at all.
///
/// So this takes what the running server materialized, copies it into a
/// directory of its own carrying **nothing else** — no `repo-policy.toml`, no
/// `docs`, no checkout to fall back to — and carries the whole turn out of that
/// copy. Every link the skill carries is opened there first, so a document the
/// artifact does not bundle fails here by name rather than in front of an agent.
pub fn the_installed_assets_carry_the_turn(world: &World) {
    let installed = world.root.path().join("state").join(ASSETS);
    let alone = tempfile::TempDir::new().expect("a scratch tree");
    copy_tree(&installed, alone.path());

    let skill = alone.path().join(crate::server_assets::SKILL_FILE);
    assert!(
        skill.is_file(),
        "the server materialized no skill at {}",
        skill.display()
    );
    let text = std::fs::read_to_string(&skill).expect("the materialized skill is readable");
    let links = crate::documented::links_in(&text);
    assert!(!links.is_empty(), "the materialized skill links to nothing");
    for target in &links {
        let at = alone.path().join(target);
        assert!(
            at.is_file(),
            "the skill an installed program wrote links to `{target}`, and the assets it \
             wrote beside it carry no such file. An install that carried the skill and not \
             what it points at hands the agent a dead link."
        );
    }

    crate::documented::operator_workflow(world, &crate::documented::Documentation::beside(&skill))
        .unwrap_or_else(|why| {
            panic!("the assets an installed program wrote do not carry a turn: {why}")
        });
}
