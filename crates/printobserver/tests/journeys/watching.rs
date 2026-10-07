//! One agent per print, the turn's situation, and the detector's pause, with a
//! supervision turn running.
//!
//! The turn is run by `support/harness.rs`'s stand-in for the configured
//! harness, a shell script, which is why this is Unix alone. It replaces the
//! paid provider process and only that: the real supervisor runs it as it runs
//! the real program, and what the stand-in does during its turn it does the way
//! an agent does — reading the prompt it was handed and running this program's
//! own commands with the `--actor` that prompt names and no `--config`, the
//! server and the credential its turn was minted reaching those commands
//! through the turn's own environment. The agent acts inside a turn or not at
//! all, so every journey about what an agent's action does is here. What each
//! journey here asserts is read back through those same commands.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use printobserver::failure::Exit;
use printobserver_server::{HarnessSignIn, SIGN_INS};
use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

use crate::harness::{PROMPT, SIGN_IN_STATE, SIGNED_IN, assessment_answer, stand_in_acting};
use crate::machine::{RUNNING_FILE, Reports};
use crate::world::{STOOD_IN, World, committed_skill};

use super::looking::{
    OBICO_PRINTER, PATIENCE, acknowledged, actions_asked, alert, detector_configuration, events_of,
    obico, post, reports, wait_for,
};

/// The harness the world's configuration names.
const IDENTITY: &str = "claude-code";

/// The file a turn waits for before it carries on, when it waits.
const GO: &str = "go";

/// A turn that says it started, and waits for the journey to let it carry on.
const WAITING: &str = r#"touch "$dir/started-$print-$n"
until [ -e "$dir/go" ]; do sleep 0.1; done"#;

/// The configured harness, as the adapter's table declares it.
fn entry() -> &'static HarnessSignIn {
    SIGN_INS
        .iter()
        .find(|entry| entry.identity() == IDENTITY)
        .unwrap_or_else(|| panic!("`{IDENTITY}` is not in the adapter's table"))
}

/// A supervisor whose turns run `turn`, and the directory those turns run in.
struct Watching {
    /// The supervisor, the machine and the camera.
    world: World,
    /// Where the stand-in keeps its sign-in, and its turns write.
    directory: PathBuf,
    /// Every invocation of the stand-in, one line each.
    invocations: PathBuf,
    /// The root the stand-in lives under, removed when this is dropped.
    _root: TempDir,
}

impl Watching {
    /// Start a supervisor under `edit`, whose harness runs `turn` every turn,
    /// and sign the stand-in in.
    fn start(turn: &str, edit: impl FnOnce(&mut Value)) -> Self {
        let root = TempDir::new().expect("a journey's own root");
        let bin = root.path().join("harness-bin");
        let invocations = root.path().join("invocations");
        stand_in_acting(&bin, entry(), &invocations, 0, &assessment_answer(), turn);
        let search = PathBuf::from(format!("{}:/usr/bin:/bin", bin.display()));
        let world = World::configured(STOOD_IN, &committed_skill(), Some(&search), edit);
        let directory = entry().directory(&world.root.path().join("state"));
        std::fs::write(directory.join(SIGNED_IN), format!("{SIGN_IN_STATE}\n"))
            .expect("the stand-in's sign-in is writable");
        Self {
            world,
            directory,
            invocations,
            _root: root,
        }
    }

    /// How many supervision turns the stand-in has been asked to run.
    fn turns(&self) -> usize {
        std::fs::read_to_string(&self.invocations)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.starts_with("claude -p"))
            .count()
    }

    /// The whole argument list of every invocation of the stand-in that ran
    /// a turn, in order: a prompt runs over many lines, and every invocation
    /// begins one with the program's own name.
    fn turn_invocations(&self) -> Vec<String> {
        let mut invoked: Vec<String> = Vec::new();
        for line in std::fs::read_to_string(&self.invocations)
            .unwrap_or_default()
            .lines()
        {
            if line.starts_with("claude ") {
                invoked.push(String::new());
            }
            if let Some(current) = invoked.last_mut() {
                current.push_str(line);
                current.push('\n');
            }
        }
        invoked.retain(|one| one.starts_with("claude -p"));
        invoked
    }

    /// A file a turn wrote, once it is there.
    fn written(&self, name: &str) -> PathBuf {
        let path = self.directory.join(name);
        wait_for(&format!("a turn to write {name}"), PATIENCE, || {
            path.exists()
        });
        path
    }

    /// Let every waiting turn carry on.
    fn go(&self) {
        std::fs::write(self.directory.join(GO), b"").expect("the signal is writable");
    }

    /// The prompt the `n`th turn about one print was handed.
    fn prompt(&self, print: &str, n: usize) -> String {
        std::fs::read_to_string(self.written(&format!("{PROMPT}-{print}-{n}")))
            .expect("the prompt reads")
    }

    /// Wait until the print's history holds `count` assessments, then a little
    /// longer, so that a turn that should not run would have.
    fn settled_after(&self, print: &str, count: usize) {
        wait_for("the turns to be written down", PATIENCE, || {
            events_of(&self.world, print, "agent_assessment").len() >= count
        });
        std::thread::sleep(Duration::from_secs(3));
    }
}

/// The JSON document one `##` section of a prompt carries in its fence.
fn section_document(prompt: &str, heading: &str) -> Value {
    let after = prompt
        .split_once(&format!("## {heading}"))
        .unwrap_or_else(|| panic!("the prompt has no `{heading}` section:\n{prompt}"))
        .1;
    let fenced = after
        .split_once("```json\n")
        .expect("the section carries a JSON fence")
        .1;
    let document = fenced.split_once("\n```").expect("the fence closes").0;
    printobserver_types::serde_json::from_str(document)
        .unwrap_or_else(|error| panic!("the `{heading}` section is not JSON ({error}): {document}"))
}

/// The situation one prompt hands the agent.
fn situation(prompt: &str) -> Value {
    section_document(prompt, "The situation when this turn began")
}

/// The event one prompt hands the agent.
fn event(prompt: &str) -> Value {
    section_document(prompt, "The event")
}

/// The identifiers of the print's alerts the journey posted, oldest first:
/// the world opens its print carrying one alert of its own, which is not one.
fn alerts(world: &World, print: &str) -> Vec<Value> {
    events_of(world, print, "obico_failure_alert")
        .into_iter()
        .skip(1)
        .map(|alert| alert["id"].clone())
        .collect()
}

/// An event for a print whose turn is running is handed to that turn: its
/// look returns early carrying it, and no second turn is started.
#[test]
fn an_event_during_a_turn_reaches_its_next_look_rather_than_a_second_turn() {
    let watching = Watching::start(
        r#"touch "$dir/started-$print"
eval "printobserver look --json --wait-s 60 $about" > "$dir/look-$print.part" 2>&1
mv "$dir/look-$print.part" "$dir/look-$print""#,
        |_| {},
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    post(world, &alert(world, 4211, RUNNING_FILE, true, false));
    watching.written(&format!("started-{print}"));
    std::thread::sleep(Duration::from_secs(1));
    let posted = Instant::now();
    post(world, &alert(world, 4211, RUNNING_FILE, true, false));

    let look: Value = printobserver_types::serde_json::from_str(
        &std::fs::read_to_string(watching.written(&format!("look-{print}"))).expect("the look"),
    )
    .expect("the look is a document");
    assert!(
        posted.elapsed() < Duration::from_secs(30),
        "the look waited out its wait rather than returning with what arrived"
    );
    let handed = alerts(world, &print);
    assert_eq!(handed.len(), 2);
    let arrived = look["arrived"]
        .as_array()
        .expect("the look carries what arrived");
    assert_eq!(arrived.len(), 1, "{look}");
    assert_eq!(arrived[0]["id"], handed[1], "{look}");
    assert_eq!(look["event"]["payload"]["delivered"][0], handed[1]);
    assert!(look["frame"].is_object(), "{look}");

    watching.settled_after(&print, 1);
    assert_eq!(watching.turns(), 1, "the arrival started a second turn");
}

/// What a turn never took gets exactly one more turn, in the same session:
/// the newest as its event, the rest as what arrived while it was busy, and
/// each prompt carries the situation's actual values.
#[test]
fn what_a_turn_never_took_gets_exactly_one_more_turn_in_the_same_session() {
    let watching = Watching::start(WAITING, |_| {});
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Paused);
    post(world, &alert(world, 4211, RUNNING_FILE, false, true));
    watching.written(&format!("started-{print}-1"));
    post(world, &alert(world, 4211, RUNNING_FILE, true, false));
    post(world, &alert(world, 4211, RUNNING_FILE, true, false));
    wait_for("both later alerts to be written down", PATIENCE, || {
        alerts(world, &print).len() == 3
    });
    watching.go();
    watching.settled_after(&print, 2);

    let handed = alerts(world, &print);
    assert_eq!(
        watching.turns(),
        2,
        "what arrived was not given exactly one turn"
    );
    let first = watching.prompt(&print, 1);
    let begun = situation(&first);
    assert_eq!(begun["printer_state"], "paused", "{begun}");
    assert_eq!(begun["detector_warned"], false, "{begun}");
    assert_eq!(begun["detector_paused_the_print"], true, "{begun}");
    assert_eq!(
        begun["arrived_while_busy"],
        Value::Array(Vec::new()),
        "{begun}"
    );
    assert_eq!(event(&first)["id"], handed[0]);

    let second = watching.prompt(&print, 2);
    let followed = situation(&second);
    assert_eq!(
        event(&second)["id"],
        handed[2],
        "the newest is the turn's own event"
    );
    let earlier: Vec<Value> = followed["arrived_while_busy"]
        .as_array()
        .expect("what arrived while busy")
        .iter()
        .map(|arrival| arrival["id"].clone())
        .collect();
    assert_eq!(earlier, vec![handed[1].clone()], "{followed}");
    assert_eq!(followed["detector_warned"], true, "{followed}");
    assert_eq!(followed["detector_paused_the_print"], false, "{followed}");

    let invoked = watching.turn_invocations();
    assert!(
        !invoked[0].contains("--resume"),
        "the first turn continued a session nothing had opened: {}",
        invoked[0]
    );
    assert!(
        invoked[1].contains("--resume"),
        "the second turn did not continue the first's session: {}",
        invoked[1]
    );
    assert_eq!(
        events_of(world, &print, "supervision_session_opened").len(),
        1,
        "a second session was opened"
    );
    let sessions: Vec<Value> = events_of(world, &print, "agent_assessment")
        .into_iter()
        .map(|assessed| assessed["payload"]["session_name"].clone())
        .collect();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0], sessions[1]);
}

/// An event for another print is written down and supervised while a look
/// inside the first print's turn is still waiting.
#[test]
fn another_prints_event_is_handled_while_a_look_waits() {
    let watching = Watching::start(
        r#"touch "$dir/started-$print"
eval "printobserver look --json --wait-s 15 $about" > "$dir/look-$print.part" 2>&1
mv "$dir/look-$print.part" "$dir/look-$print""#,
        |_| {},
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    post(world, &alert(world, 4211, RUNNING_FILE, true, false));
    watching.written(&format!("started-{print}"));
    post(
        world,
        &alert(world, 5000, "a-second-part.gcode", true, false),
    );

    let looking = watching.directory.join(format!("look-{print}"));
    let other_started = || {
        std::fs::read_dir(&watching.directory)
            .expect("the turns' directory reads")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .any(|name| name.starts_with("started-") && name != format!("started-{print}"))
    };
    wait_for("the second print's turn to start", PATIENCE, other_started);
    assert!(
        !looking.exists(),
        "the first print's look had returned before the second print's turn started"
    );
    watching.written(&format!("look-{print}"));
    assert_eq!(watching.turns(), 2);
}

/// A print whose turn saw it end releases its inbox: what was waiting for it
/// gets no turn, and neither does an event that arrives for it afterwards.
#[test]
fn an_ended_print_releases_its_inbox() {
    let watching = Watching::start(WAITING, |_| {});
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Operational);
    post(world, &alert(world, 4211, RUNNING_FILE, true, false));
    watching.written(&format!("started-{print}-1"));
    post(world, &alert(world, 4211, RUNNING_FILE, true, false));
    wait_for("the second alert to be written down", PATIENCE, || {
        alerts(world, &print).len() == 2
    });
    watching.go();
    wait_for("the print's session to be closed", PATIENCE, || {
        !events_of(world, &print, "supervision_session_closed").is_empty()
    });
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        watching.turns(),
        1,
        "what waited for an ended print got a turn"
    );

    post(world, &alert(world, 4211, RUNNING_FILE, true, false));
    wait_for("the later alert to be written down", PATIENCE, || {
        alerts(world, &print).len() == 3
    });
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        watching.turns(),
        1,
        "an alert for an ended print started a turn"
    );
}

/// An adjustment the agent asks for in its turn, while the detector's pause
/// holds the print, is applied at once; the print is resumed as the system
/// when the turn ends, and `Obico` is told.
#[test]
fn an_adjustment_in_the_turn_resumes_the_print_when_the_turn_ends() {
    let api = obico("200 OK");
    let watching = Watching::start(
        r#"eval "printobserver look --json $about" > "$dir/look-$print" 2>&1
eval "printobserver set-fan-percent --percent 80 --reason 'more cooling for the overhang' $actor $about" > "$dir/fan-$print" 2>&1"#,
        detector_configuration(&api, true),
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Paused);
    let posted = Instant::now();
    post(world, &alert(world, 4211, RUNNING_FILE, false, true));

    wait_for("the print to be resumed", PATIENCE, || {
        reports(world) == Some(Reports::Printing)
    });
    assert!(
        posted.elapsed() < Duration::from_secs(15),
        "the print was resumed by the grace rather than by the turn ending"
    );
    let look: Value = printobserver_types::serde_json::from_str(
        &std::fs::read_to_string(watching.written(&format!("look-{print}"))).expect("the look"),
    )
    .expect("the look is a document");
    assert_eq!(look["detector_paused"], true, "{look}");
    let asked = actions_asked(world, &print);
    let kinds: Vec<&Value> = asked.iter().map(|action| &action["action"]).collect();
    assert_eq!(kinds, ["set_fan_percent", "resume"], "{asked:?}");
    assert!(asked[0]["actor"]["agent"].is_object(), "{asked:?}");
    assert_eq!(asked[1]["actor"], "system");
    wait_for("Obico to be told", PATIENCE, || acknowledged(&api));
}

/// An agent that acknowledges the detection with `stop` leaves the pause for a
/// person, even after it adjusted.
#[test]
fn acknowledging_stop_leaves_the_pause_for_a_person() {
    let api = obico("200 OK");
    let watching = Watching::start(
        r#"eval "printobserver set-fan-percent --percent 80 --reason 'more cooling for the overhang' $actor $about" > "$dir/fan-$print" 2>&1
eval "printobserver acknowledge-failure --event-id $event --disposition stop --reason 'nothing I may change reaches this' $actor $about" > "$dir/stop-$print" 2>&1"#,
        detector_configuration(&api, true),
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Paused);
    post(world, &alert(world, 4211, RUNNING_FILE, false, true));
    watching.settled_after(&print, 1);

    let asked = actions_asked(world, &print);
    let kinds: Vec<&Value> = asked.iter().map(|action| &action["action"]).collect();
    assert_eq!(
        kinds,
        ["set_fan_percent", "acknowledge_failure"],
        "{asked:?}"
    );
    assert_eq!(asked[1]["disposition"], "stop");
    assert_eq!(reports(world), Some(Reports::Paused));
    assert!(
        api.received().is_empty(),
        "Obico was told about a pause left for a person"
    );
}

/// A resume the policy refuses — the system is not granted it — leaves the
/// print paused, the refusal is in the record, and `Obico` is told nothing.
#[test]
fn a_resume_the_policy_refuses_leaves_the_print_paused() {
    let api = obico("200 OK");
    let watching = Watching::start(
        r#"eval "printobserver set-fan-percent --percent 80 --reason 'more cooling for the overhang' $actor $about" > "$dir/fan-$print" 2>&1"#,
        detector_configuration(&api, false),
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Paused);
    post(world, &alert(world, 4211, RUNNING_FILE, false, true));
    watching.settled_after(&print, 1);

    let asked = actions_asked(world, &print);
    let kinds: Vec<&Value> = asked.iter().map(|action| &action["action"]).collect();
    assert_eq!(kinds, ["set_fan_percent", "resume"], "{asked:?}");
    assert_eq!(asked[1]["actor"], "system");
    let rejected = events_of(world, &print, "action_rejected");
    assert_eq!(rejected.len(), 1, "{rejected:?}");
    let refusal = &rejected[0]["payload"]["decision"]["rejected"]["actor_may_not_request"];
    assert_eq!(refusal["actor_class"], "system", "{rejected:?}");
    assert_eq!(refusal["action"], "resume", "{rejected:?}");
    assert_eq!(reports(world), Some(Reports::Paused));
    assert!(
        api.received().is_empty(),
        "Obico was told about a print still paused"
    );
}

/// A machine that refuses the resume the turn's end asks for leaves the print
/// paused: the resume is in the record and never carried out, and `Obico` is
/// told nothing.
#[test]
fn a_resume_the_machine_refuses_leaves_the_print_paused() {
    let api = obico("200 OK");
    let watching = Watching::start(
        r#"eval "printobserver set-fan-percent --percent 80 --reason 'more cooling for the overhang' $actor $about" > "$dir/fan-$print" 2>&1
until [ -e "$dir/go" ]; do sleep 0.1; done"#,
        detector_configuration(&api, true),
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Paused);
    post(world, &alert(world, 4211, RUNNING_FILE, false, true));
    watching.written(&format!("fan-{print}"));
    wait_for("the adjustment to be recorded", PATIENCE, || {
        !actions_asked(world, &print).is_empty()
    });
    assert!(
        world.machine_refuses(true),
        "a stood-in machine can be told to refuse"
    );
    watching.go();
    wait_for("the system to ask for the resume", PATIENCE, || {
        actions_asked(world, &print)
            .iter()
            .any(|action| action["action"] == "resume")
    });
    std::thread::sleep(Duration::from_secs(2));
    assert!(world.machine_refuses(false));

    let resume = events_of(world, &print, "action_requested")
        .into_iter()
        .find(|requested| requested["payload"]["action"]["action"] == "resume")
        .expect("the resume is in the record");
    assert_eq!(resume["payload"]["action"]["actor"], "system");
    assert!(
        !events_of(world, &print, "action_executed")
            .iter()
            .any(|executed| executed["payload"]["action_id"] == resume["payload"]["action_id"]),
        "a resume the machine refused is recorded as carried out"
    );
    assert_eq!(reports(world), Some(Reports::Paused));
    assert!(
        api.received().is_empty(),
        "Obico was told about a print still paused"
    );
}

/// A print cancelled under the detector's pause, after the agent adjusted it,
/// is no longer the detector's to resume: neither the turn's end nor the grace
/// resumes it, and `Obico` is told nothing.
#[test]
fn a_print_cancelled_under_the_detectors_pause_is_not_resumed() {
    let api = obico("200 OK");
    let watching = Watching::start(
        r#"eval "printobserver set-fan-percent --percent 80 --reason 'more cooling for the overhang' $actor $about" > "$dir/fan-$print" 2>&1
until [ -e "$dir/go" ]; do sleep 0.1; done"#,
        detector_configuration(&api, true),
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Paused);
    post(world, &alert(world, 4211, RUNNING_FILE, false, true));
    watching.written(&format!("fan-{print}"));
    let adjusted = Instant::now();
    let ran = super::running::command(
        world,
        &[
            "cancel",
            "--print-id",
            &print,
            "--actor",
            "operator",
            "--reason",
            "the part has come off the bed",
        ],
    );
    assert_eq!(ran.code, Some(0), "{}", ran.said());
    watching.go();
    watching.settled_after(&print, 1);
    // Past the grace the adjustment would have earned, so a resume it could
    // still have scheduled would have been asked for by now.
    if let Some(left) = Duration::from_secs(23).checked_sub(adjusted.elapsed()) {
        std::thread::sleep(left);
    }

    let asked = actions_asked(world, &print);
    let kinds: Vec<&Value> = asked.iter().map(|action| &action["action"]).collect();
    assert_eq!(kinds, ["set_fan_percent", "cancel"], "{asked:?}");
    assert_eq!(reports(world), Some(Reports::Operational));
    assert!(
        api.received().is_empty(),
        "Obico was told about a cancelled print"
    );
}

/// A print somebody else resumed is no longer the detector's: when it is
/// paused again by a person and the agent then adjusts it, nothing resumes it
/// over that person's pause.
#[test]
fn a_print_somebody_else_resumed_is_not_resumed_over_their_next_pause() {
    let api = obico("200 OK");
    let watching = Watching::start(
        r#"touch "$dir/started-$print-$n"
until [ -e "$dir/go" ]; do sleep 0.1; done
eval "printobserver set-fan-percent --percent 80 --reason 'more cooling for the overhang' $actor $about" > "$dir/fan-$print" 2>&1"#,
        detector_configuration(&api, true),
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Paused);
    post(world, &alert(world, 4211, RUNNING_FILE, false, true));
    watching.written(&format!("started-{print}-1"));
    for (command, reason) in [
        ("resume", "I looked, and the part is fine"),
        ("pause", "I want to look at the first layer myself"),
    ] {
        let ran = super::running::command(
            world,
            &[
                command,
                "--print-id",
                &print,
                "--actor",
                "operator",
                "--reason",
                reason,
            ],
        );
        assert_eq!(ran.code, Some(0), "`{command}`: {}", ran.said());
    }
    watching.go();
    watching.settled_after(&print, 1);

    let asked = actions_asked(world, &print);
    let kinds: Vec<&Value> = asked.iter().map(|action| &action["action"]).collect();
    assert_eq!(kinds, ["resume", "pause", "set_fan_percent"], "{asked:?}");
    assert_eq!(reports(world), Some(Reports::Paused));
    assert!(
        api.received().is_empty(),
        "Obico was told about a person's pause"
    );
}

/// What the machine reports it is doing, read off the stood-in machine's own
/// socket: while a turn is running the supervisor answers the context it
/// collected for that turn, so a read through this program would answer the
/// state the turn began in. A request target carrying a query is answered the
/// machine's connection document, whose state names what it is doing.
fn machine_reports(world: &World) -> Reports {
    use std::io::{Read as _, Write as _};

    let crate::world::Printer::StoodIn(machine) = &world.printer else {
        panic!("the journeys holding a turn open run over a stood-in machine")
    };
    let mut stream =
        std::net::TcpStream::connect(machine.address).expect("the stood-in machine answers");
    write!(
        stream,
        "GET /state?now HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        machine.address
    )
    .expect("the read is written");
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .expect("the machine's answer is read");
    let body = answer.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    let document: Value =
        printobserver_types::serde_json::from_str(body).expect("the machine answers a document");
    match document["state"]["text"].as_str() {
        Some("Printing") => Reports::Printing,
        Some("Paused") => Reports::Paused,
        Some("Operational") => Reports::Operational,
        other => panic!("the machine reports {other:?}"),
    }
}

/// The exit status one command a turn ran exited with, once the turn has
/// written it whole: the file is there a moment before the status is in it.
fn exited(watching: &Watching, name: &str) -> i32 {
    let path = watching.written(&format!("{name}.code"));
    let mut status = None;
    wait_for(
        &format!("the status of {name} to be written"),
        PATIENCE,
        || {
            status = std::fs::read_to_string(&path)
                .ok()
                .and_then(|held| held.trim().parse().ok());
            status.is_some()
        },
    );
    status.expect("a status")
}

/// What one command a turn ran printed, as the turn wrote it.
fn printed(watching: &Watching, name: &str) -> String {
    std::fs::read_to_string(watching.written(name)).expect("the output reads")
}

/// An agent's adjustment in its turn, while the detector's pause holds the
/// print, is applied at once; twenty seconds after its last such adjustment —
/// a second one moves the resume on — the system resumes the print through the
/// policy while the turn is still running, and acknowledges the alert to
/// `Obico`, and nothing the supervisor wrote or printed carries `Obico`'s
/// token.
#[test]
fn an_adjustment_under_the_detectors_pause_earns_a_resume_after_the_grace() {
    let api = obico("200 OK");
    let watching = Watching::start(
        r#"eval "printobserver set-fan-percent --percent 80 --reason 'more cooling for the overhang' $actor $about" > "$dir/fan-1-$print" 2>&1; echo $? > "$dir/fan-1-$print.code"
eval "printobserver look --json $about" > "$dir/look-$print" 2>&1
sleep 2
eval "printobserver set-fan-percent --percent 90 --reason 'still more cooling for the overhang' $actor $about" > "$dir/fan-2-$print" 2>&1; echo $? > "$dir/fan-2-$print.code"
until [ -e "$dir/go" ]; do sleep 0.1; done"#,
        detector_configuration(&api, true),
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Paused);
    post(world, &alert(world, 4211, RUNNING_FILE, false, true));

    assert_eq!(exited(&watching, &format!("fan-1-{print}")), 0);
    assert_eq!(
        exited(&watching, &format!("fan-2-{print}")),
        0,
        "{}",
        printed(&watching, &format!("fan-2-{print}"))
    );
    // The look ran before the second adjustment, so it is whole once that
    // adjustment's status is written.
    let look: Value =
        printobserver_types::serde_json::from_str(&printed(&watching, &format!("look-{print}")))
            .expect("the look is a document");
    assert_eq!(look["detector_paused"], true, "{look}");
    let again = Instant::now();
    assert_eq!(
        machine_reports(world),
        Reports::Paused,
        "resumed inside the grace"
    );

    wait_for("the print to be resumed", PATIENCE, || {
        machine_reports(world) == Reports::Printing
    });
    // Within a second of the second adjustment's own status being written, so
    // a resume at least nineteen seconds later is one the second adjustment
    // moved on rather than the first adjustment's grace.
    assert!(
        again.elapsed() >= Duration::from_secs(19),
        "the print was resumed {:?} after the second adjustment, before its grace",
        again.elapsed()
    );
    let resumes: Vec<Value> = actions_asked(world, &print)
        .into_iter()
        .filter(|action| action["action"] == "resume")
        .collect();
    assert_eq!(resumes.len(), 1, "{resumes:?}");
    assert_eq!(resumes[0]["actor"], "system");
    wait_for("Obico to be told", PATIENCE, || acknowledged(&api));
    watching.go();

    let history = Value::Array(super::looking::history(world, &print)).to_string();
    assert!(
        !history.contains(super::looking::OBICO_TOKEN),
        "the history carries the token"
    );
    assert!(
        !world.said_so_far().contains(super::looking::OBICO_TOKEN),
        "the supervisor printed the token"
    );
}

/// An acknowledgement `Obico` refuses, once the turn that adjusted the print
/// ends and resumes it, is recorded as a port failure against the detection's
/// own event, saying what `Obico` answered and never the token.
#[test]
fn an_acknowledgement_obico_refuses_is_recorded_against_the_detection() {
    let api = obico("403 Forbidden");
    let watching = Watching::start(
        r#"eval "printobserver set-fan-percent --percent 80 --reason 'more cooling for the overhang' $actor $about" > "$dir/fan-$print" 2>&1; echo $? > "$dir/fan-$print.code""#,
        detector_configuration(&api, true),
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Paused);
    post(world, &alert(world, 4211, RUNNING_FILE, false, true));
    assert_eq!(
        exited(&watching, &format!("fan-{print}")),
        0,
        "{}",
        printed(&watching, &format!("fan-{print}"))
    );

    let refused = || {
        events_of(world, &print, "port_failure")
            .into_iter()
            .find(|failure| failure["payload"]["site"] == "detector_acknowledgement")
    };
    wait_for(
        "the refused acknowledgement to be recorded",
        PATIENCE,
        || refused().is_some(),
    );
    let detection = events_of(world, &print, "obico_failure_alert")
        .pop()
        .expect("the alert")["id"]
        .clone();
    let failure = refused().expect("recorded");
    assert_eq!(failure["payload"]["event_id"], detection);
    let detail = failure["payload"]["detail"].as_str().unwrap_or_default();
    assert!(detail.contains("403"), "{detail}");
    assert!(!detail.contains(super::looking::OBICO_TOKEN), "{detail}");
    assert_eq!(reports(world), Some(Reports::Printing));
    assert!(
        !world.said_so_far().contains(super::looking::OBICO_TOKEN),
        "the supervisor printed the token"
    );
}

/// While the print is paused the agent's minimum interval does not hold its
/// adjustments back; cancelling still waits on it, and once the print is
/// moving an adjustment does too — all of it asked for by the turn itself,
/// with the credential it was minted.
#[test]
fn adjustments_to_a_paused_print_skip_the_interval_and_nothing_else_does() {
    let watching = Watching::start(
        r#"ask() { eval "printobserver $2 --reason 'a journey about the interval' $actor $about" > "$dir/$1-$print" 2>&1; echo $? > "$dir/$1-$print.code"; }
ask fan-paused "set-fan-percent --percent 70"
ask feedrate-paused "set-feedrate-factor --factor 0.9"
ask cancel-paused "cancel"
touch "$dir/paused-done-$print"
until [ -e "$dir/go" ]; do sleep 0.1; done
ask fan-moving "set-fan-percent --percent 60"
ask pause-moving "pause""#,
        |document| {
            document["safety"]["agent_min_interval_s"] = json!(300);
            document["safety"]["actions"]["agent"] =
                json!(["set_fan_percent", "set_feedrate_factor", "pause", "cancel"]);
        },
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Paused);
    post(world, &alert(world, 4211, RUNNING_FILE, true, false));

    watching.written(&format!("paused-done-{print}"));
    for name in ["fan-paused", "feedrate-paused"] {
        assert_eq!(
            exited(&watching, &format!("{name}-{print}")),
            0,
            "`{name}` on a paused print: {}",
            printed(&watching, &format!("{name}-{print}"))
        );
    }
    let rejected = i32::from(Exit::Rejected.status());
    assert_eq!(
        exited(&watching, &format!("cancel-paused-{print}")),
        rejected
    );
    assert!(
        printed(&watching, &format!("cancel-paused-{print}")).contains("min_interval_not_elapsed")
    );

    world.wants(Reports::Printing);
    watching.go();
    for name in ["fan-moving", "pause-moving"] {
        assert_eq!(
            exited(&watching, &format!("{name}-{print}")),
            rejected,
            "`{name}` on a moving print did not wait on the interval: {}",
            printed(&watching, &format!("{name}-{print}"))
        );
        assert!(
            printed(&watching, &format!("{name}-{print}")).contains("min_interval_not_elapsed"),
            "{}",
            printed(&watching, &format!("{name}-{print}"))
        );
    }
}

/// A detector that reports the pause again while the turn that adjusted the
/// print is still running does not undo the adjustment: the resume it earned
/// still comes when the turn ends, and the newer detection is the one
/// acknowledged — to its own printer, and with a refusal recorded against its
/// own event.
#[test]
fn a_repeated_detection_keeps_the_resume_and_is_the_one_acknowledged() {
    let api = obico("403 Forbidden");
    let watching = Watching::start(
        &format!(
            r#"eval "printobserver set-fan-percent --percent 80 --reason 'more cooling for the overhang' $actor $about" > "$dir/fan-$print-$n" 2>&1
{WAITING}"#
        ),
        detector_configuration(&api, true),
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Paused);
    post(world, &alert(world, 4211, RUNNING_FILE, false, true));
    watching.written(&format!("started-{print}-1"));
    let mut again = alert(world, 4211, RUNNING_FILE, false, true);
    again["printer"]["id"] = json!(OBICO_PRINTER + 1);
    post(world, &again);
    // The newer detection is written down before the turn is let go.
    wait_for("the newer detection to be written down", PATIENCE, || {
        events_of(world, &print, "obico_failure_alert").len() >= 2
    });
    watching.go();

    let refused = || {
        events_of(world, &print, "port_failure")
            .into_iter()
            .find(|failure| failure["payload"]["site"] == "detector_acknowledgement")
    };
    wait_for(
        "the refused acknowledgement to be recorded",
        PATIENCE,
        || refused().is_some(),
    );
    assert_eq!(reports(world), Some(Reports::Printing));
    let resumes = actions_asked(world, &print)
        .into_iter()
        .filter(|action| action["action"] == "resume")
        .count();
    assert_eq!(resumes, 1);
    let newer = events_of(world, &print, "obico_failure_alert")
        .pop()
        .expect("the newer detection")["id"]
        .clone();
    assert_eq!(refused().expect("recorded")["payload"]["event_id"], newer);
    let heads = api.received();
    assert_eq!(heads.len(), 1, "{heads:?}");
    assert!(
        heads[0].starts_with(&format!(
            "POST /api/v1/printers/{}/acknowledge_alert/",
            OBICO_PRINTER + 1
        )),
        "{}",
        heads[0]
    );
}

/// A turn reaches the supervisor with the credential minted for it alone,
/// handed to it in its environment and in no file: the commands it runs name
/// no `--config`, act on its own print, and are refused claiming the operator,
/// reaching another print or replacing a manifest. Once the turn has returned
/// the credential is refused, two turns are minted two credentials, and none
/// of them is in any file the supervisor wrote or anything it printed.
#[test]
fn a_turn_acts_with_its_own_credential_and_no_other() {
    let captured = TempDir::new().expect("a directory outside the state directory");
    let capture = captured.path().display().to_string();
    let watching = Watching::start(
        &format!(
            r#"printf '%s' "${{PRINTOBSERVER_CREDENTIAL:-}}" > "{capture}/credential-$n"
printf '%s' "${{PRINTOBSERVER_SERVER:-}}" > "{capture}/server-$n"
ls "$dir/../.." > "{capture}/state-$n" 2>&1
ask() {{ eval "printobserver $2 $about" > "{capture}/$1-$n" 2>&1; echo $? > "{capture}/$1-$n.code"; }}
ask context "context"
ask fan "set-fan-percent --percent 70 --reason 'more cooling for the overhang' $actor"
ask as-operator "cancel --reason 'stopping this as the operator' --actor operator"
ask manifest "manifest-set --reason 'widening my own bounds' --manifest '{{}}'"
other=$(cat "{capture}/other")
eval "printobserver status --print-id $other" > "{capture}/other-print-$n" 2>&1; echo $? > "{capture}/other-print-$n.code"
touch "{capture}/done-$n""#
        ),
        |document| {
            document["safety"]["actions"]["agent"] = json!(["set_fan_percent", "pause"]);
        },
    );
    let world = &watching.world;
    let print = world.print_id().clone();
    world.wants(Reports::Printing);
    let wait_for_turn = |n: usize| {
        let done = captured.path().join(format!("done-{n}"));
        wait_for(&format!("turn {n} to finish"), PATIENCE, || done.exists());
        watching.settled_after(&print, n);
    };
    let read = |name: &str| std::fs::read_to_string(captured.path().join(name)).unwrap_or_default();

    std::fs::write(captured.path().join("other"), world.ended_print_id())
        .expect("the other print is written down for the turn");
    post(world, &alert(world, 4211, RUNNING_FILE, true, false));
    wait_for_turn(1);

    let first = read("credential-1");
    assert!(!first.is_empty(), "the turn was handed no credential");
    assert_ne!(
        first,
        crate::world::CREDENTIAL,
        "the turn was handed the operator's credential"
    );
    assert!(
        read("server-1").starts_with("http://127.0.0.1:"),
        "the turn was not told where the supervisor is: {}",
        read("server-1")
    );
    assert!(
        !read("state-1").contains("client.toml"),
        "a client configuration is in the state directory: {}",
        read("state-1")
    );
    the_first_turn_acted_as_itself(world, &print, captured.path());

    // The turn has returned: its credential is refused, as a caller with that
    // environment and no file would find.
    let after = super::running::with(
        world,
        &["status".to_owned(), "--print-id".to_owned(), print.clone()],
        &[
            ("PRINTOBSERVER_SERVER".to_owned(), read("server-1")),
            ("PRINTOBSERVER_CREDENTIAL".to_owned(), first.clone()),
        ],
    );
    assert_eq!(
        after.code,
        Some(i32::from(Exit::Unconfigured.status())),
        "a returned turn's credential was admitted: {}",
        after.said()
    );

    post(world, &alert(world, 4211, RUNNING_FILE, true, false));
    wait_for_turn(2);
    let second = read("credential-2");
    assert!(!second.is_empty());
    assert_ne!(first, second, "two turns were handed one credential");

    let state = world.root.path().join("state");
    for credential in [&first, &second] {
        for written in walk_state(&state) {
            assert!(
                !std::fs::read(&written)
                    .unwrap_or_default()
                    .windows(credential.len())
                    .any(|window| window == credential.as_bytes()),
                "{} carries a turn's credential",
                written.display()
            );
        }
        assert!(
            !world.said_so_far().contains(credential.as_str()),
            "the supervisor printed a turn's credential"
        );
    }
}

/// What the first turn's commands answered, as it wrote them down under
/// `captured`: its context read and its adjustment served; claiming the
/// operator, replacing a manifest and reaching another print refused, naming
/// who asked; and nothing but the adjustment recorded.
fn the_first_turn_acted_as_itself(world: &World, print: &str, captured: &std::path::Path) {
    let read = |name: &str| std::fs::read_to_string(captured.join(name)).unwrap_or_default();
    let status = |name: &str| -> i32 { read(&format!("{name}.code")).trim().parse().unwrap_or(-1) };
    assert_eq!(status("context-1"), 0, "{}", read("context-1"));
    assert!(read("context-1").contains(print), "{}", read("context-1"));
    assert_eq!(status("fan-1"), 0, "{}", read("fan-1"));
    let refused = i32::from(Exit::Refused.status());
    for (name, naming) in [
        ("as-operator-1", "the operator"),
        ("manifest-1", "manifest-set"),
        ("other-print-1", "its own print alone"),
    ] {
        assert_eq!(
            status(name),
            refused,
            "`{name}` was not refused: {}",
            read(name)
        );
        assert!(
            read(name).contains("403") && read(name).contains(naming),
            "`{name}` was refused without naming who asked: {}",
            read(name)
        );
    }
    let asked = actions_asked(world, print);
    assert_eq!(
        asked
            .iter()
            .map(|action| action["action"].clone())
            .collect::<Vec<_>>(),
        [json!("set_fan_percent")],
        "a refused request was recorded: {asked:?}"
    );
    assert!(asked[0]["actor"]["agent"].is_object(), "{asked:?}");
}

/// Every file under a directory.
fn walk_state(root: &std::path::Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk_state(&path));
        } else {
            found.push(path);
        }
    }
    found
}
