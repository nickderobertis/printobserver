//! The port composes nothing of its own into a turn.
//!
//! The prompt is the committed template with its slots filled and nothing
//! else, and the system prompt is the prose of the skill file at the configured
//! path rather than text this crate carries.

use std::fs;
use std::sync::Arc;

use printobserver_oneharness::{
    ACTOR_SLOT, CONTEXT_COMMAND_SLOT, EVENT_SLOT, IMAGE_SLOT, NO_IMAGE, OneharnessSupervisor,
    SLOTS, UnclosedFrontmatter, skill_prose,
};
use printobserver_supervisor_api::{SupervisorError, SupervisorPort};
use printobserver_types::{PrintId, serde_json};

use crate::support::{
    Fixture, HARNESS, OTHER_HARNESS, Watch, always, assessment, block_on, config, event,
    failure_alert, generated_assessment_schema, notification, port, schema_read_lock, skill_path,
    template_path, turn, unreadable,
};

/// The literal text of the committed template between its slots, in the order
/// the template writes them, and the slots in that same order.
fn segments(template: &str) -> (Vec<String>, Vec<&'static str>) {
    let mut positions: Vec<(usize, &'static str)> = SLOTS
        .iter()
        .map(|slot| {
            (
                template.find(slot).expect("the template declares the slot"),
                *slot,
            )
        })
        .collect();
    positions.sort_unstable();
    let mut literals = Vec::new();
    let mut order = Vec::new();
    let mut cursor = 0;
    for (at, slot) in positions {
        literals.push(template[cursor..at].to_owned());
        order.push(slot);
        cursor = at + slot.len();
    }
    literals.push(template[cursor..].to_owned());
    (literals, order)
}

/// What one prompt put in each slot, or `None` when it is not this template
/// with only its slots filled.
///
/// The first literal must be the prompt's own prefix and the last its own
/// suffix, so a port that put a sentence of its own before or after the
/// template fails here rather than being read as a longer filling.
fn fillings(literals: &[String], prompt: &str) -> Option<Vec<String>> {
    let first = literals.first()?;
    let last = literals.last()?;
    if !prompt.starts_with(first.as_str()) || !prompt.ends_with(last.as_str()) {
        return None;
    }
    let mut cursor = first.len();
    let end = prompt.len() - last.len();
    if end < cursor {
        return None;
    }
    let mut found = Vec::new();
    for literal in &literals[1..literals.len() - 1] {
        let at = cursor + prompt.get(cursor..end)?.find(literal.as_str())?;
        found.push(prompt.get(cursor..at)?.to_owned());
        cursor = at + literal.len();
    }
    found.push(prompt.get(cursor..end)?.to_owned());
    Some(found)
}

/// Every prompt is the committed template with only its slots differing, and
/// the actor it hands the agent is the agent in the session the turn ran in.
#[test]
fn every_prompt_is_the_committed_template_with_only_its_slots_filled() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let schemas = schema_read_lock();
    let fixture = Fixture::new("prompting");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            always("SID-PROMPT", &assessment("the print is fine", "high")),
        ),
        &watch,
    );

    let template = fs::read_to_string(template_path()).expect("the committed template is readable");
    let (literals, order) = segments(&template);

    let picture = fixture.path("frame.jpg");
    fs::write(&picture, b"not really a picture").expect("a scratch picture");
    let triggers = [
        (
            failure_alert(7, Some("bracket.gcode")),
            Some(picture.clone()),
        ),
        (notification("filament_change", 7, None), None),
        (unreadable("the body was not JSON"), Some(picture)),
    ];

    let mut seen = Vec::new();
    for (payload, image) in triggers {
        let print_id = PrintId::new();
        let request = turn(print_id, event(print_id, payload), image.clone());
        let expected_event =
            serde_json::to_string_pretty(&request.event).expect("the event is writable");
        let expected_image = image
            .as_ref()
            .map_or_else(|| NO_IMAGE.to_owned(), |path| path.display().to_string());
        let expected_command = request.context_command.clone();
        // A print's first turn runs in that print's first session.
        let expected_actor = actor_in(&format!("print-{print_id}"));
        block_on(supervisor.run_turn(request)).expect("the turn runs");

        let prompt = watch
            .requests()
            .last()
            .expect("the port built a run request")
            .prompt
            .first()
            .expect("the run request carries a prompt")
            .clone();
        let found = fillings(&literals, &prompt).unwrap_or_else(|| {
            panic!("the prompt is not the committed template with only its slots filled:\n{prompt}")
        });
        for (slot, filling) in order.iter().zip(&found) {
            match *slot {
                EVENT_SLOT => assert_eq!(filling, &expected_event),
                IMAGE_SLOT => assert_eq!(filling, &expected_image),
                CONTEXT_COMMAND_SLOT => assert_eq!(filling, &expected_command),
                ACTOR_SLOT => assert_eq!(filling, &expected_actor),
                other => panic!("the template declares an unknown slot {other}"),
            }
        }
        seen.push(found);
    }

    // The literals never moved: only the slots differ between the turns.
    assert_eq!(seen.len(), 3);
    assert_ne!(
        seen[0], seen[1],
        "two different events filled the same slots"
    );
}

/// The actor document the agent names itself by in one session, spelled the
/// way the supervisor's own action requests read one. This crate cannot see
/// the core's type; the server's suite parses a real turn's actor as it.
fn actor_in(session_name: &str) -> String {
    serde_json::json!({ "agent": { "session_name": session_name } }).to_string()
}

/// The prompt one run request carries.
fn prompt_of(request: &oneharness_core::io::run::RunRequest) -> String {
    request
        .prompt
        .first()
        .expect("the run request carries a prompt")
        .clone()
}

/// A turn moved to a new session is prompted with the actor of the session it
/// moved to, not the one the harness refused: an agent naming a session the
/// turn no longer runs in is acting as somebody else.
#[test]
fn a_turn_moved_to_a_new_session_hands_the_agent_that_sessions_actor() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("prompting-moved");
    let schema = generated_assessment_schema();
    let print_id = PrintId::new();
    let answering = || always("SID-MOVED", &assessment("the print is fine", "high"));

    let first = port(
        config(&schemas, &fixture, HARNESS, &schema, answering()),
        &Arc::new(Watch::default()),
    );
    block_on(first.run_turn(turn(print_id, event(print_id, unreadable("first")), None)))
        .expect("the first turn runs");
    drop(first);

    // The same state directory, and so the same session, on another identity,
    // which refuses to continue it: the turn moves to the print's second session.
    let watch = Arc::new(Watch::default());
    let moved = port(
        config(&schemas, &fixture, OTHER_HARNESS, &schema, answering()),
        &watch,
    );
    let after =
        block_on(moved.run_turn(turn(print_id, event(print_id, unreadable("second")), None)))
            .expect("the moved turn runs");
    assert_eq!(after.session.session_name, format!("print-{print_id}-2"));

    let prompts: Vec<String> = watch.requests().iter().map(prompt_of).collect();
    let [refused, ran] = prompts.as_slice() else {
        panic!("the moved turn was not asked twice: {prompts:#?}");
    };
    let quoted = |session: &str| format!("--actor '{}'", actor_in(session));
    assert!(
        refused.contains(&quoted(&format!("print-{print_id}"))),
        "the refused attempt did not name the session it asked to continue:\n{refused}"
    );
    assert!(
        ran.contains(&quoted(&format!("print-{print_id}-2"))),
        "the turn that ran did not name the session it ran in:\n{ran}"
    );
    assert!(
        !ran.contains(&quoted(&format!("print-{print_id}"))),
        "the turn that ran still named the session the harness refused:\n{ran}"
    );
}

/// The committed template hands acting to the skill and the policy, and says
/// which commands take the agent's actor, rather than calling the turn a
/// reading. Read off a real filled prompt, because that is what the agent reads.
#[test]
fn the_filled_prompt_hands_acting_to_the_skill_and_the_policy() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("prompting-wording");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            always("SID-WORDING", &assessment("the print is fine", "high")),
        ),
        &watch,
    );
    let print_id = PrintId::new();
    block_on(supervisor.run_turn(turn(
        print_id,
        event(print_id, failure_alert(7, Some("bracket.gcode"))),
        None,
    )))
    .expect("the turn runs");
    let prompt = prompt_of(
        watch
            .requests()
            .first()
            .expect("the port built a run request"),
    );
    // The template wraps its prose at a fixed width, so it is read as words.
    let words = prompt.split_whitespace().collect::<Vec<_>>().join(" ");

    for said in [
        "decide, and act as your skill describes.",
        "Everything you ask for goes through the same policy an operator's requests do, \
         and that policy decides what your role may change.",
        "Give every other command you run the same `--config` file this one takes.",
        "A command whose usage lists `--actor` names who is asking.",
        "A command whose usage does not list it, the one above included, refuses it:",
        "what you did (each command you ran and what it answered)",
    ] {
        assert!(
            words.contains(said),
            "the prompt does not say {said:?}:\n{prompt}"
        );
    }
    for unsaid in [
        "This turn is a reading, not a repair.",
        "Nothing here asks you to change what the machine is doing",
        "the only command named below is the one that reads",
    ] {
        assert!(
            !words.contains(unsaid),
            "the prompt still says {unsaid:?}:\n{prompt}"
        );
    }
    assert!(
        prompt.contains(&format!(
            "--actor '{}'",
            actor_in(&format!("print-{print_id}"))
        )),
        "the prompt does not hand the agent its own actor:\n{prompt}"
    );
}

/// The committed skill's prose, split off its frontmatter by hand rather than
/// by the function under test: everything after the second line that is
/// exactly `---`.
fn committed_prose(committed: &str) -> &str {
    let rest = committed
        .strip_prefix("---\n")
        .expect("the committed skill opens with a frontmatter block");
    let closing = rest
        .find("\n---\n")
        .expect("the committed skill's frontmatter closes");
    &rest[closing + "\n---\n".len()..]
}

/// The system prompt one port sends, built over the skill at `skill`.
fn system_prompt_over(
    schemas: &crate::support::SchemaHold<crate::support::Shared>,
    fixture: &Fixture,
    skill: &std::path::Path,
) -> String {
    let schema = generated_assessment_schema();
    let environment = always("SID-SKILL", &assessment("the print is fine", "high"));
    let mut configured = config(schemas, fixture, HARNESS, &schema, environment);
    configured.skill_path = skill.to_path_buf();
    let watch = Arc::new(Watch::default());
    let supervisor = port(configured, &watch);
    let print_id = PrintId::new();
    block_on(supervisor.run_turn(turn(
        print_id,
        event(print_id, unreadable("the body was not JSON")),
        None,
    )))
    .expect("the turn runs");
    watch
        .requests()
        .last()
        .expect("the port built a run request")
        .system
        .clone()
        .expect("the request carries a system prompt")
}

/// The system prompt is the prose of the skill file at the configured path,
/// read from the tree when the port is built.
#[test]
fn the_system_prompt_is_the_skill_file_at_the_configured_path() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let schemas = schema_read_lock();
    let fixture = Fixture::new("skill");

    // The committed skill, with its frontmatter removed: the skill's own text,
    // opening on its title and carrying none of the frontmatter's keys.
    let committed = fs::read_to_string(skill_path()).expect("the committed skill is readable");
    let prose = committed_prose(&committed);
    assert!(
        prose.starts_with("# Supervising a 3D print\n"),
        "the committed skill's prose does not open on its title"
    );
    let sent = system_prompt_over(&schemas, &fixture, &skill_path());
    assert_eq!(
        sent, prose,
        "the system prompt is not the committed skill's prose"
    );
    assert!(
        !sent.contains("name: printobserver"),
        "the system prompt carries the skill's frontmatter"
    );

    // And a skill file whose content changes on disk changes what the request
    // carries, which a crate that embedded the skill's text could not do. A
    // file with no frontmatter is sent as it is.
    let scratch = fixture.path("skill.md");
    for (text, expected) in [
        (committed.as_str(), prose),
        (
            "Watch the print. Say what you see.",
            "Watch the print. Say what you see.",
        ),
    ] {
        fs::write(&scratch, text).expect("the scratch skill is writable");
        assert_eq!(
            system_prompt_over(&schemas, &fixture, &scratch),
            expected,
            "the system prompt did not follow the file at the configured path"
        );
    }
}

/// A skill as `gh skill install` rewrites it is sent as the same prose.
///
/// An install adds its own `metadata` to the frontmatter and reorders the keys
/// — a local install a `local-path`, a remote one where on the forge it came
/// from — and leaves the prose byte for byte. What a turn sends is the prose,
/// so neither shape changes it.
#[test]
fn a_skill_as_an_install_rewrote_it_is_sent_as_the_same_prose() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let schemas = schema_read_lock();
    let fixture = Fixture::new("skill-installed");
    let committed = fs::read_to_string(skill_path()).expect("the committed skill is readable");
    let prose = committed_prose(&committed);

    let installs = [
        (
            "local",
            "---\ndescription: Supervise one 3D print.\nlicense: MIT\nmetadata:\n    \
             local-path: /tmp/checkout/skills/printobserver\nname: printobserver\n---\n",
        ),
        (
            "remote",
            "---\ndescription: Supervise one 3D print.\nlicense: MIT\nmetadata:\n    \
             github-path: skills/printobserver\n    github-ref: main\n    \
             github-repo: nickderobertis/printobserver\n    \
             github-tree-sha: 0123456789abcdef0123456789abcdef01234567\n\
             name: printobserver\n---\n",
        ),
    ];
    for (install, frontmatter) in installs {
        let installed = fixture.path(&format!("{install}-SKILL.md"));
        fs::write(&installed, format!("{frontmatter}{prose}")).expect("an installed skill");
        assert_eq!(
            system_prompt_over(&schemas, &fixture, &installed),
            prose,
            "the {install} install's rewritten frontmatter changed the prose sent"
        );
    }
}

/// A skill whose frontmatter never closes is refused, naming the file, rather
/// than sent whole as the agent's instructions.
#[test]
fn a_skill_whose_frontmatter_never_closes_is_refused() {
    // The configuration every port here is built from reads the checked-in
    // assessment schema, so the schema tree is held still while it is read.
    let schemas = schema_read_lock();
    let fixture = Fixture::new("skill-unclosed");
    let unclosed = fixture.path("unclosed-SKILL.md");
    fs::write(
        &unclosed,
        "---\nname: printobserver\ndescription: Supervise one print.\n# Supervising\n",
    )
    .expect("an unclosed skill");
    let schema = generated_assessment_schema();
    let environment = always("SID-SKILL", &assessment("the print is fine", "high"));
    let mut configured = config(&schemas, &fixture, HARNESS, &schema, environment);
    configured.skill_path = unclosed;

    let refused = OneharnessSupervisor::open(configured)
        .expect_err("a port was built over a skill whose frontmatter never closes");
    assert!(
        matches!(refused, SupervisorError::Unavailable { .. }),
        "the refusal is not unavailability: {refused}"
    );
    let said = refused.to_string();
    assert!(
        said.contains("unclosed-SKILL.md") && said.contains("frontmatter"),
        "the refusal does not name the file and why: {said}"
    );
}

/// The split itself, over the shared cases `check-repo`'s own split is held to.
///
/// The adapter and `check-repo` each split a `SKILL.md` into frontmatter and
/// prose — one to send the prose, the other to measure it — and
/// `skill-prose-cases.json` beside this suite is the one set of inputs and
/// answers both are run over, so the two readings cannot drift apart unseen.
/// A `null` prose is a file refused as a frontmatter block that never closes.
#[test]
fn the_prose_is_everything_after_the_line_closing_the_frontmatter() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("skill-prose-cases.json"))
        .expect("the shared cases are JSON");
    let cases = cases.as_array().expect("the shared cases are a list");
    assert!(!cases.is_empty(), "the shared cases carry no case");
    for case in cases {
        let named = case["case"].as_str().expect("every case is named");
        let text = case["text"].as_str().expect("every case carries a text");
        let expected = case["prose"].as_str().ok_or(UnclosedFrontmatter);
        assert_eq!(skill_prose(text), expected, "the case `{named}`");
    }
}
