//! One supervision turn, carried out from the committed documentation alone.
//!
//! # What this establishes
//!
//! A reader who starts at the skill `OneHarness` sends as the system prompt, and
//! follows only what it and the documents it links to provide, can carry one
//! supervision turn out end to end against a real supervisor. Every command run
//! here is taken out of the skill or out of a document the skill links to, and
//! every value composed into one is either read out of that document or read out
//! of the answer a previous command gave. Nothing here knows the name of a
//! command, of an option, or of a field except by having read it there.
//!
//! # Why that self-limitation is load-bearing rather than a claim
//!
//! Nothing here reads the tree except through [`Documentation`], which opens the
//! skill and the documents the skill links to and nothing else. Every command
//! name, option and field is resolved out of that text at run time, so a
//! document that stopped saying how to do one of these steps stops this journey
//! at that step rather than being papered over by what the test itself knew.
//!
//! # Where it runs
//!
//! Against the stood-in machine in the fast tier and against the `OctoPrint`
//! `just octoprint-up` provisioned in the printer tier — the same walk over both,
//! because a turn a reader carries out is the same turn whichever machine is on
//! the far side of the printer port.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::machine::Reports;
use crate::world::{CREDENTIAL, World};

/// The fence a runnable example is written in.
const FENCE: &str = "```console";

/// What a command line in an example begins with.
const PROMPT: &str = "$ ";

/// The program every documented example runs.
const PROGRAM: &str = "printobserver";

/// What separates a field's path from its value in this program's own output.
const LABEL: &str = ": ";

/// The placeholder a documented example writes the state directory as.
const STATE_DIR: &str = "STATE_DIR";

/// The placeholder a documented example writes an image identifier as.
const IMAGE_ID: &str = "IMAGE_ID";

/// The placeholder a documented example writes an event identifier as.
const EVENT_ID: &str = "EVENT_ID";

/// The placeholder a documented example writes a print identifier as.
const PRINT_ID: &str = "PRINT_ID";

/// How many events the observation and the escalation are read back over.
const READ_BACK: i64 = 50;

/// The reason the bounded decision is made with.
const DECISION_REASON: &str = "the picture shows the first layer lifting at one corner";

/// The reason the second, acceptable request is made with.
const INSIDE_REASON: &str = "asking again inside what the rejection said was allowed";

/// The reason the observation is recorded with.
const OBSERVATION_REASON: &str = "loose strands at the left of the frame and the part still down";

/// The reason the escalation is made with.
const ESCALATION_REASON: &str =
    "the picture and the history disagree; a person should look at this";

/// One command of a documented example, and what it prints.
#[derive(Debug, Clone)]
struct Step {
    /// The command line, exactly as the document writes it.
    command: String,
    /// What the document shows it printing.
    output: Vec<String>,
}

/// The documentation, as a reader who starts at the skill sees it.
///
/// The skill and nothing but the documents it links to: a document this
/// repository ships and the skill does not link to is one the reader never
/// reaches, so it is not read here either.
pub struct Documentation {
    /// The skill's own text.
    skill: String,
    /// Every document the skill links to, by the path it links to it at.
    linked: BTreeMap<String, String>,
}

/// The repository root, from this crate's own directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// A declared path cannot bypass its root lexically.
fn validate_relative_path(value: &str) {
    let path = Path::new(value);
    assert!(
        !path.is_absolute()
            && !path
                .components()
                .any(|part| part == std::path::Component::ParentDir),
        "repository path `{value}` must be relative without parent traversal"
    );
}

/// Policy paths name existing repository assets, including symlinks within the tree.
pub fn validate_repository_path(root: &Path, value: &str) {
    validate_relative_path(value);
    let resolved = root.join(value).canonicalize().expect("the asset exists");
    assert!(
        resolved.starts_with(root.canonicalize().expect("the repository exists")),
        "repository path `{value}` resolves outside the repository"
    );
}

/// Inline asset declarations cannot escape their root, including through symlinks.
#[test]
fn repository_asset_paths_stay_inside_their_root() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    std::fs::write(root.join("guide.md"), "A guide.\n").expect("write an inline asset");
    validate_repository_path(root, "guide.md");
    std::os::unix::fs::symlink(root.parent().expect("a parent"), root.join("escape"))
        .expect("create an escaping symlink");
    for value in [root.to_str().expect("a UTF-8 path"), "../outside", "escape"] {
        assert!(
            std::panic::catch_unwind(|| validate_repository_path(root, value)).is_err(),
            "{value} must be refused"
        );
    }
}

impl Documentation {
    /// Read one skill and every document it links to, from beside it.
    ///
    /// Links are resolved against the skill's **own directory** rather than
    /// against a tree root, because that is the anchor an install reproduces:
    /// the server writes the reference documents beside the skill it
    /// materialized and stands the agent in that directory. So this reads a
    /// checkout and a state directory the same way, which is what lets the same
    /// walk run over both.
    pub fn beside(skill_path: &Path) -> Self {
        let beside = skill_path.parent().expect("the skill sits in a directory");
        let skill = std::fs::read_to_string(skill_path)
            .unwrap_or_else(|error| panic!("{} is readable: {error}", skill_path.display()));
        let mut linked = BTreeMap::new();
        for target in links_in(&skill) {
            validate_relative_path(&target);
            // llmlint: ignore[boundary_inputs_validated] Checkout references intentionally follow the assets/reference symlink outside the skill directory into docs/reference. The docs skill check validates this declared bundle; the installed-assets journey proves its materialized links without a checkout. Confining links to the skill directory would reject the supported checkout layout.
            let body = std::fs::read_to_string(beside.join(&target)).unwrap_or_else(|error| {
                panic!("linked document `{target}` must be readable: {error}")
            });
            linked.insert(target, body);
        }
        Self { skill, linked }
    }

    /// The skill of one checkout, found the way the policy declares it.
    pub fn in_tree(root: &Path) -> Self {
        let text =
            std::fs::read_to_string(root.join("repo-policy.toml")).expect("the policy is readable");
        let policy: toml::Value =
            toml::from_str(&text).unwrap_or_else(|error| panic!("the policy parses: {error}"));
        let skill_path = policy["docs"]["skill"]
            .as_str()
            .expect("the policy names the skill");
        validate_repository_path(root, skill_path);
        Self::beside(&root.join(skill_path))
    }

    /// The paragraph of the skill one numbered workflow step is written in.
    fn step(&self, ordinal: usize) -> Option<String> {
        let opens = format!("{ordinal}. **");
        let mut said: Vec<&str> = Vec::new();
        for line in self.skill.lines() {
            if said.is_empty() {
                if line.trim_start().starts_with(&opens) {
                    said.push(line.trim());
                }
                continue;
            }
            // Any next step ends this one, not just the one after it. A reader
            // handed a skill whose next step is missing is handed one step,
            // rather than that step and everything the author wrote after it.
            if opens_a_step(line) || line.starts_with("## ") {
                break;
            }
            said.push(line.trim());
        }
        (!said.is_empty()).then(|| said.join(" "))
    }

    /// The body of one `##` section of the skill.
    fn section(&self, heading: &str) -> Option<String> {
        let mut said: Vec<&str> = Vec::new();
        let mut inside = false;
        for line in self.skill.lines() {
            if let Some(title) = line.strip_prefix("## ") {
                if inside {
                    break;
                }
                inside = title.trim().eq_ignore_ascii_case(heading);
                continue;
            }
            if inside {
                said.push(line.trim());
            }
        }
        let said = said.join(" ").trim().to_owned();
        (!said.is_empty()).then_some(said)
    }

    /// Every command the linked documents show how to run.
    fn commands(&self) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for text in self.linked.values() {
            for line in text.lines() {
                let Some(rest) = line.strip_prefix(&format!("{PROMPT}{PROGRAM} ")) else {
                    continue;
                };
                if let Some(name) = rest.split_whitespace().next()
                    && !found.iter().any(|held| held == name)
                {
                    found.push(name.to_owned());
                }
            }
        }
        found
    }

    /// The command one passage of the skill points at, if the documents show one.
    ///
    /// A command matches when every word of its name is a word of the passage,
    /// and the most specific match wins — so a passage naming a failure to
    /// acknowledge reaches the acknowledgement rather than something shorter.
    fn command_in(&self, passage: &str) -> Option<String> {
        let lowered = passage.to_lowercase();
        let words: Vec<String> = lowered
            .split(|letter: char| !letter.is_ascii_alphanumeric())
            .map(str::to_owned)
            .collect();
        let mut best: Option<String> = None;
        for command in self.commands() {
            let parts: Vec<&str> = command.split('-').collect();
            if !parts
                .iter()
                .all(|part| words.iter().any(|word| word == part))
            {
                continue;
            }
            if best
                .as_ref()
                .is_none_or(|held| held.split('-').count() < parts.len())
            {
                best = Some(command);
            }
        }
        best
    }

    /// The worked example the documents show for one command.
    fn example(&self, command: &str) -> Option<Vec<Step>> {
        let opens = format!("{PROGRAM} {command} ");
        self.linked
            .values()
            .flat_map(|text| blocks(text))
            .find(|block| {
                block
                    .first()
                    .is_some_and(|step| step.command.starts_with(&opens))
            })
    }

    /// The worked example the documents show for a request that was refused.
    ///
    /// Found by what the document shows rather than by a name: it is the one
    /// example whose own output shows the command exiting non-zero.
    fn refused_example(&self) -> Option<Vec<Step>> {
        for text in self.linked.values() {
            for block in blocks(text) {
                let refused = block.iter().any(|step| {
                    step.command == "echo $?"
                        && step
                            .output
                            .first()
                            .is_some_and(|said| said.trim() != "0" && !said.trim().is_empty())
                });
                if refused {
                    return Some(block);
                }
            }
        }
        None
    }
}

/// Whether one line of the skill opens a numbered workflow step.
fn opens_a_step(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed
        .split_once(". **")
        .is_some_and(|(head, _)| head.parse::<u8>().is_ok())
}

/// Every markdown link target one document carries.
pub fn links_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("](") {
        let after = &rest[open + 2..];
        if let Some(close) = after.find(')') {
            found.push(after[..close].to_owned());
            rest = &after[close..];
        } else {
            break;
        }
    }
    found
}

/// Every fenced example one document carries, in the order it carries them.
fn blocks(text: &str) -> Vec<Vec<Step>> {
    let mut found = Vec::new();
    let mut inside: Option<Vec<Step>> = None;
    for line in text.lines() {
        match inside.as_mut() {
            None => {
                if line.trim_end() == FENCE {
                    inside = Some(Vec::new());
                }
            }
            Some(steps) => {
                if line.starts_with("```") {
                    found.push(core::mem::take(steps));
                    inside = None;
                } else if let Some(command) = line.strip_prefix(PROMPT) {
                    steps.push(Step {
                        command: command.trim_end().to_owned(),
                        output: Vec::new(),
                    });
                } else if let Some(step) = steps.last_mut() {
                    step.output.push(line.to_owned());
                }
            }
        }
    }
    found
}

/// One answer of this program, read as the labelled lines it prints.
#[derive(Debug, Clone, Default)]
struct Answer {
    /// Every field the answer carried, by the path it sat at.
    fields: BTreeMap<String, String>,
    /// Everything it printed, for a failure that has to show it.
    printed: String,
    /// What the caller's shell saw.
    status: i32,
}

impl Answer {
    /// Read one answer out of what a command printed.
    fn of(printed: &str, status: i32) -> Self {
        let mut fields = BTreeMap::new();
        for line in printed.lines() {
            if let Some((path, value)) = line.split_once(LABEL)
                && !path.contains(' ')
            {
                fields.insert(path.to_owned(), value.to_owned());
            }
        }
        Self {
            fields,
            printed: printed.to_owned(),
            status,
        }
    }

    /// The value at one path, when the answer carried one.
    fn at(&self, path: &str) -> Option<&str> {
        self.fields.get(path).map(String::as_str)
    }

    /// Every path this answer carried beginning with one prefix.
    fn under(&self, prefix: &str) -> Vec<&String> {
        self.fields
            .keys()
            .filter(|path| path.starts_with(prefix))
            .collect()
    }
}

/// The values the documented placeholders stand for in this run.
type Bindings = BTreeMap<String, String>;

/// One documented command line, with every placeholder bound to a real value.
fn bound(command: &str, bindings: &Bindings) -> String {
    let mut written = command.to_owned();
    for (name, value) in bindings {
        written = written.replace(name, value);
    }
    written
}

/// Split one command line into words, honouring single and double quotes.
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

/// An inline skill cannot silently lose a reference its reader cannot open.
#[test]
#[should_panic(expected = "linked document `missing.md` must be readable")]
fn unreadable_documentation_link_names_its_target() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let skill = directory.path().join("skill.md");
    std::fs::write(&skill, "Read the [reference](missing.md).\n").expect("write an inline skill");
    Documentation::beside(&skill);
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

/// Run one documented command line against this world's supervisor.
fn run(world: &World, command: &str) -> Answer {
    let given = words(command).expect("documented arguments have closed quotes");
    let (program, arguments) = given.split_first().expect("a command line");
    assert_eq!(program, PROGRAM, "a documented example runs this program");
    let ran = Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .args(arguments)
        .envs(world.environment(CREDENTIAL))
        .output()
        .expect("the documented command runs");
    Answer::of(
        &format!(
            "{}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        ),
        ran.status.code().unwrap_or(-1),
    )
}

/// Replace the value one option carries on a documented command line.
fn with_option(command: &str, option: &str, value: &str) -> String {
    let mut given = words(command).expect("documented arguments have closed quotes");
    let at = given
        .iter()
        .position(|word| word == option)
        .expect("the documented command carries the option");
    value.clone_into(&mut given[at + 1]);
    given
        .iter()
        .map(|word| {
            if word.contains(' ') {
                format!("'{word}'")
            } else {
                word.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every way one documented command names a value, as option and value.
fn options(command: &str) -> Vec<(String, String)> {
    let given = words(command).expect("documented arguments have closed quotes");
    given
        .windows(2)
        .filter(|pair| pair[0].starts_with("--"))
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect()
}

/// The one key of a documented output whose value is a given placeholder.
fn key_showing(steps: &[Step], placeholder: &str) -> Option<String> {
    steps.iter().flat_map(|step| &step.output).find_map(|line| {
        let (path, value) = line.split_once(LABEL)?;
        (value.trim() == placeholder).then(|| path.to_owned())
    })
}

/// The one key of a documented output whose value opens with a placeholder.
///
/// A path is written as the placeholder for the directory it is under followed
/// by the rest of it, so the field naming a file is the one whose shown value
/// begins with that placeholder rather than one that is only it.
fn key_showing_under(steps: &[Step], placeholder: &str) -> Option<String> {
    steps.iter().flat_map(|step| &step.output).find_map(|line| {
        let (path, value) = line.split_once(LABEL)?;
        value
            .trim()
            .starts_with(placeholder)
            .then(|| path.to_owned())
    })
}

/// Every key of a documented output whose path ends in one segment.
fn keys_ending(steps: &[Step], segment: &str) -> Vec<String> {
    steps
        .iter()
        .flat_map(|step| &step.output)
        .filter_map(|line| {
            let (path, _) = line.split_once(LABEL)?;
            path.ends_with(segment).then(|| path.to_owned())
        })
        .collect()
}

/// The value one documented output shows at one key.
fn shown(steps: &[Step], key: &str) -> Option<String> {
    steps.iter().flat_map(|step| &step.output).find_map(|line| {
        let (path, value) = line.split_once(LABEL)?;
        (path == key).then(|| value.trim().to_owned())
    })
}

/// One step of the turn could not be reached from what the reader was given.
fn unreachable(element: &str, why: &str) -> String {
    format!("{element}: {why}")
}

/// Carry one supervision turn out from the documentation of one tree.
///
/// # Errors
///
/// Returns the element of the turn that could not be reached from the skill and
/// the documents it links to, and why.
// Why: `suppressions.toml`, which is where this repository keeps the reason
// for every suppression standing in the tree.
#[expect(clippy::too_many_lines)]
pub fn operator_workflow(world: &World, documentation: &Documentation) -> Result<(), String> {
    let mut bindings: Bindings = BTreeMap::from([(PRINT_ID.to_owned(), world.print_id.clone())]);

    let step = documentation
        .step(1)
        .ok_or_else(|| unreachable("the context command", "the skill states no first step"))?;
    let name = documentation.command_in(&step).ok_or_else(|| {
        unreachable(
            "the context command",
            "the skill's first step names no command the documents show how to run",
        )
    })?;
    let example = documentation.example(&name).ok_or_else(|| {
        unreachable(
            "the context command",
            "no document the skill links to shows a worked example of it",
        )
    })?;
    let reading = bound(&example[0].command, &bindings);
    let context = run(world, &reading);
    if context.status != 0 {
        return Err(unreachable(
            "the context command",
            &format!("`{reading}` exited {}: {}", context.status, context.printed),
        ));
    }
    // Everything the skill says the read will carry, taken as the field paths
    // the worked example shows it carrying.
    let mut carried: Vec<String> = example
        .iter()
        .flat_map(|step| &step.output)
        .filter_map(|line| line.split_once(LABEL))
        .map(|(path, _)| path.split('.').take(2).collect::<Vec<_>>().join("."))
        .collect();
    carried.sort_unstable();
    carried.dedup();
    if carried.is_empty() {
        return Err(unreachable(
            "the context command",
            "the worked example shows it answering nothing",
        ));
    }
    for path in &carried {
        if context.under(path).is_empty() {
            return Err(unreachable(
                "the context command",
                &format!("the answer carries no `{path}`, which the document shows it carrying"),
            ));
        }
    }

    documentation
        .step(2)
        .ok_or_else(|| unreachable("the image inspection", "the skill states no second step"))?;
    let path_key = key_showing_under(&example, STATE_DIR).ok_or_else(|| {
        unreachable(
            "the image inspection",
            "no document says which field of the context answer names the image's file",
        )
    })?;
    let image_key = key_showing(&example, IMAGE_ID).ok_or_else(|| {
        unreachable(
            "the image inspection",
            "no document says which field of the context answer names the image",
        )
    })?;
    let path = context.at(&path_key).ok_or_else(|| {
        unreachable(
            "the image inspection",
            &format!("the answer carries no `{path_key}`"),
        )
    })?;
    bindings.insert(
        IMAGE_ID.to_owned(),
        context
            .at(&image_key)
            .ok_or_else(|| {
                unreachable(
                    "the image inspection",
                    &format!("the answer carries no `{image_key}`"),
                )
            })?
            .to_owned(),
    );
    let image_example = documentation
        .example(&documentation.command_in("image").unwrap_or_default())
        .ok_or_else(|| {
            unreachable(
                "the image inspection",
                "no document shows how to read an image record",
            )
        })?;
    let digest_key = keys_ending(&image_example, "sha256")
        .into_iter()
        .next()
        .ok_or_else(|| {
            unreachable(
                "the image inspection",
                "no document says which field of an image record declares its digest",
            )
        })?;
    let record = run(world, &bound(&image_example[0].command, &bindings));
    let declared = record.at(&digest_key).ok_or_else(|| {
        unreachable(
            "the image inspection",
            &format!("the image record carries no `{digest_key}`"),
        )
    })?;
    let bytes = std::fs::read(path).map_err(|error| {
        unreachable(
            "the image inspection",
            &format!("the file at `{path}` could not be opened: {error}"),
        )
    })?;
    let taken = {
        use sha2::{Digest as _, Sha256};
        format!("{:x}", Sha256::digest(&bytes))
    };
    assert_eq!(
        taken, declared,
        "the file the documentation said to open is not the image the record declares"
    );

    let refused = documentation.refused_example().ok_or_else(|| {
        unreachable(
            "the rejection's own fields",
            "no document shows a request the policy refused",
        )
    })?;
    let adjustable_key = keys_ending(&refused, "adjustable")
        .into_iter()
        .next()
        .ok_or_else(|| {
            unreachable(
                "the rejection's own fields",
                "no document says which field of a rejection names the adjustable",
            )
        })?;
    let adjustable = shown(&refused, &adjustable_key).ok_or_else(|| {
        unreachable(
            "the rejection's own fields",
            "the refused example shows no adjustable",
        )
    })?;
    let wanted = format!("allowed.{adjustable}.min");
    let bounds_prefix = context
        .fields
        .keys()
        .find(|path| path.ends_with(&wanted))
        .cloned()
        .ok_or_else(|| {
            unreachable(
                "the effective bounds",
                &format!("the context answer reports no bound for `{adjustable}`"),
            )
        })?;
    if !carried.iter().any(|path| bounds_prefix.starts_with(path)) {
        return Err(unreachable(
            "the effective bounds",
            "no document shows the context answer carrying the bounds in force",
        ));
    }
    let low: f64 = context
        .at(&bounds_prefix)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            unreachable(
                "the effective bounds",
                "the reported minimum is not a number",
            )
        })?;
    let high_key = bounds_prefix.replace(".min", ".max");
    let high: f64 = context
        .at(&high_key)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            unreachable(
                "the effective bounds",
                "the reported maximum is not a number",
            )
        })?;
    let asked_key = keys_ending(&refused, "requested")
        .into_iter()
        .next()
        .ok_or_else(|| {
            unreachable(
                "the rejection's own fields",
                "no document says which field of a rejection carries the value asked for",
            )
        })?;
    let refused_value = shown(&refused, &asked_key).ok_or_else(|| {
        unreachable(
            "the rejection's own fields",
            "the refused example shows no value asked for",
        )
    })?;
    let (value_option, _) = options(&refused[0].command)
        .into_iter()
        .find(|(_, given)| given == &refused_value)
        .ok_or_else(|| {
            unreachable(
                "the effective bounds",
                "no document says which argument of the adjustment carries the bounded value",
            )
        })?;
    let inside = midpoint(low, high);
    let adjusting = with_option(
        &with_option(
            &bound(&refused[0].command, &bindings),
            &value_option,
            &format!("{inside}"),
        ),
        "--reason",
        DECISION_REASON,
    );
    let accepted = run(world, &adjusting);
    assert_eq!(
        accepted.status, 0,
        "the supervisor refused a value inside the bounds it reported: {}",
        accepted.printed
    );

    let outside = high + (high - low).max(1.0);
    let overreaching = with_option(
        &with_option(
            &bound(&refused[0].command, &bindings),
            &value_option,
            &format!("{outside}"),
        ),
        "--reason",
        DECISION_REASON,
    );
    let rejection = run(world, &overreaching);
    assert_ne!(
        rejection.status, 0,
        "a value outside the reported bounds was accepted: {}",
        rejection.printed
    );
    let allowed_low_key = keys_ending(&refused, "allowed.min")
        .into_iter()
        .next()
        .ok_or_else(|| {
            unreachable(
                "the rejection's own fields",
                "no document says which field of a rejection carries the range allowed",
            )
        })?;
    let allowed_high_key = allowed_low_key.replace(".min", ".max");
    let told_low: f64 = rejection
        .at(&allowed_low_key)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            unreachable(
                "the rejection's own fields",
                &format!("the rejection carries no `{allowed_low_key}`"),
            )
        })?;
    let told_high: f64 = rejection
        .at(&allowed_high_key)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            unreachable(
                "the rejection's own fields",
                &format!("the rejection carries no `{allowed_high_key}`"),
            )
        })?;
    let told_asked: f64 = rejection
        .at(&asked_key)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            unreachable(
                "the rejection's own fields",
                &format!("the rejection carries no `{asked_key}`"),
            )
        })?;
    assert!(
        (told_asked - outside).abs() < f64::EPSILON,
        "the rejection reports a value nobody asked for"
    );
    let second = with_option(
        &with_option(
            &bound(&refused[0].command, &bindings),
            &value_option,
            &format!("{}", midpoint(told_low, told_high)),
        ),
        "--reason",
        INSIDE_REASON,
    );
    let after = run(world, &second);
    assert_eq!(
        after.status, 0,
        "a request composed out of the rejection's own fields was refused: {}",
        after.printed
    );

    let step = documentation
        .step(5)
        .ok_or_else(|| unreachable("the observation record", "the skill states no fifth step"))?;
    let recording = documentation.command_in(&step).ok_or_else(|| {
        unreachable(
            "the observation record",
            "the skill's fifth step names no command the documents show how to run",
        )
    })?;
    let record_example = documentation.example(&recording).ok_or_else(|| {
        unreachable(
            "the observation record",
            "no document the skill links to shows a worked example of it",
        )
    })?;
    let event_key = key_showing(&example, EVENT_ID).ok_or_else(|| {
        unreachable(
            "the observation record",
            "no document says which field of the context answer names the event to record against",
        )
    })?;
    let kind_key = event_key.replace(".id", ".kind");
    let wanted_kind = shown(&example, &kind_key).ok_or_else(|| {
        unreachable(
            "the observation record",
            "no document says which kind of event is the one to record against",
        )
    })?;
    let event = context
        .under(&collection_of(&event_key))
        .into_iter()
        .filter(|path| {
            path.rsplit_once('.')
                .is_some_and(|(_, last)| last == "kind")
        })
        .find(|path| context.at(path) == Some(wanted_kind.as_str()))
        .map(|path| path.replace(".kind", ".id"))
        .and_then(|path| context.at(&path))
        .ok_or_else(|| {
            unreachable(
                "the observation record",
                &format!("the context answer carries no `{wanted_kind}` to record against"),
            )
        })?
        .to_owned();
    bindings.insert(EVENT_ID.to_owned(), event);
    let observing = with_option(
        &bound(&record_example[0].command, &bindings),
        "--reason",
        OBSERVATION_REASON,
    );
    let observed = run(world, &observing);
    assert_eq!(
        observed.status, 0,
        "the observation was refused: {}",
        observed.printed
    );
    reads_back(
        world,
        documentation,
        OBSERVATION_REASON,
        "the observation record",
    )?;

    let escalation = documentation
        .section("When to escalate instead")
        .ok_or_else(|| unreachable("the escalation path", "the skill states no escalation path"))?;
    let escalating = documentation.command_in(&escalation).ok_or_else(|| {
        unreachable(
            "the escalation path",
            "the skill's escalation path names no command the documents show how to run",
        )
    })?;
    let escalation_example = documentation.example(&escalating).ok_or_else(|| {
        unreachable(
            "the escalation path",
            "no document the skill links to shows a worked example of it",
        )
    })?;
    world.wants(Reports::Printing);
    let raised = run(
        world,
        &with_option(
            &bound(&escalation_example[0].command, &bindings),
            "--reason",
            ESCALATION_REASON,
        ),
    );
    assert_eq!(
        raised.status, 0,
        "the escalation was refused: {}",
        raised.printed
    );
    reads_back(
        world,
        documentation,
        ESCALATION_REASON,
        "the escalation path",
    )?;
    // The tier this runs in leaves the machine as it found it.
    put_the_print_back(world);
    Ok(())
}

/// How many times the print is put back before this walk gives up on it.
const PUT_BACK_ROUNDS: usize = 4;

/// How long the machine is watched for after each attempt to put it back.
///
/// Longer than one command of the hold print the printer tier's environment
/// runs, which is a ten-second dwell: a pause that has been asked for lands at
/// the next command boundary, so anything shorter reads the state part-way
/// through pausing.
const PUT_BACK_PAUSE: core::time::Duration = core::time::Duration::from_secs(12);

/// Put the print back where this tier found it, and make sure it stays there.
///
/// A real machine finishes pausing at the next command boundary. Until it does
/// it reports itself both on its way to paused and still printing, so a walk
/// that asked once and read once would resume a machine that had not finished
/// pausing, be told it was printing, and leave it paused a dwell later — which
/// is what this tier's next run would find instead of a running print.
///
/// # Panics
///
/// Panics when the machine will not stay printing, saying what it is reporting.
fn put_the_print_back(world: &World) {
    for _ in 0..PUT_BACK_ROUNDS {
        world.wants(Reports::Printing);
        std::thread::sleep(PUT_BACK_PAUSE);
        if world.reported_state() == Some(Reports::Printing) {
            return;
        }
    }
    panic!(
        "the escalation left the print not running: the machine is reporting {:?}",
        world.reported_state()
    );
}

/// The value half-way between two bounds, at the precision an example writes.
fn midpoint(low: f64, high: f64) -> f64 {
    (f64::midpoint(low, high) * 100.0).round() / 100.0
}

/// The collection one indexed field of an answer sits in.
///
/// `context.recent_events.1.id` is one field of one entry, and what a reader
/// needs is every entry: the document shows which entry it happened to be about
/// on the run that produced it, and the live answer numbers its own.
fn collection_of(path: &str) -> String {
    let mut head = path.rsplit_once('.').map_or(path, |(head, _)| head);
    if let Some((rest, last)) = head.rsplit_once('.')
        && last.parse::<usize>().is_ok()
    {
        head = rest;
    }
    head.to_owned()
}

/// One reason reads back through the history command the documents show.
fn reads_back(
    world: &World,
    documentation: &Documentation,
    reason: &str,
    element: &str,
) -> Result<(), String> {
    let name = documentation
        .commands()
        .into_iter()
        .find(|command| {
            documentation.example(command).is_some_and(|steps| {
                steps
                    .iter()
                    .flat_map(|step| &step.output)
                    .any(|line| line.starts_with("events."))
            })
        })
        .ok_or_else(|| unreachable(element, "no document shows how to read a print's events"))?;
    let steps = documentation
        .example(&name)
        .ok_or_else(|| unreachable(element, "no document shows a worked example of it"))?;
    let mut command = bound(
        &steps[0].command,
        &BTreeMap::from([(PRINT_ID.to_owned(), world.print_id.clone())]),
    );
    // The documented example answers the newest event alone; this reads far
    // enough back to find what was just written, through the same argument the
    // document says carries that number.
    if let Some((option, _)) = options(&command)
        .into_iter()
        .find(|(_, given)| given.parse::<i64>().is_ok())
    {
        command = with_option(&command, &option, &READ_BACK.to_string());
    }
    let history = run(world, &command);
    assert!(
        history.printed.contains(reason),
        "`{reason}` does not read back through `{command}`:\n{}",
        history.printed
    );
    Ok(())
}

/// The turn, carried out from the committed documentation.
pub fn walk(world: &World) {
    // A supervision turn is about a print that is running and something that
    // has just been seen, so the world is put there first — through this
    // crate's own settling, which is world setup rather than part of the turn.
    // Everything after this is the reader's.
    world.wants(Reports::Printing);
    world.freshen_the_image();
    operator_workflow(world, &Documentation::in_tree(&repo_root()))
        .unwrap_or_else(|why| panic!("the committed documentation does not carry a turn: {why}"));
}
