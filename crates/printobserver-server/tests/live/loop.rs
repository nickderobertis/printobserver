//! Every declared operation, against the real machine and the real harness.
//!
//! One walk rather than several tests, because every step acts on one shared
//! machine and the order they run in is part of what is proven: an adjustment
//! needs a running print, a resume needs a paused one, and a cancel ends the
//! print the steps before it needed. A runner free to interleave them would be
//! proving something else.
//!
//! # Where each operation's effect is confirmed
//!
//! Seven of the ten actions have a read-back at the machine — start, pause,
//! resume, cancel, the two heater targets and the acknowledgement that stops a
//! print — and are confirmed by a status read afterwards. Three have none:
//! `OctoPrint` reports neither an applied feedrate factor nor an applied
//! flowrate factor nor a fan setting. For those the observable is **what the
//! instance received**, read out of a recording proxy that forwards every
//! request verbatim to the real machine — so what was recorded is what the
//! machine ruled on rather than what this server believed it sent. The value
//! each is asserted to carry is computed with the adapter's own exported
//! conversion, because the number on the wire is that adapter's business and no
//! crate but that adapter may spell one.

use printobserver_octoprint::{
    FAN_PWM_PARAMETER, FAN_SET_COMMAND, fan_pwm_of_percent, percent_of_multiplier,
};
use printobserver_server::{Effect, OPERATIONS, Operation};
use printobserver_types::serde_json::{Value, json};
use printobserver_types::{EventKind, PrintId};

use crate::composition::{
    AGENT_DURATION_S, AGENT_FACTOR, AGENT_REFUSED_FACTOR, Composed, NARROWED, SECRET,
    hold_the_print_running,
};
use crate::http_host::image_host;
use crate::proxy::Proxy;
use crate::scripted::Scripted;
use crate::waiting::until;

/// The file the scripted environment uploads and this walk starts.
const HOLD_FILE: &str = "hold.gcode";

/// Obico's own identifier for the print this walk's alerts are about.
const OBICO_PRINT: i64 = 4211;

/// How long a bounded adjustment stands for, in the step that bounds one.
const DURATION_S: i64 = 600;

/// The bytes this walk's own image host serves.
fn snapshot_bytes() -> Vec<u8> {
    let mut bytes = vec![0xff, 0xd8, 0xff];
    bytes.extend(b"printobserver-integration-snapshot".repeat(8));
    bytes
}

/// The committed `Obico` failure alert, naming an image host this walk serves.
fn alert_body(image_url: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../printobserver-types/samples/obico/failure-alert.json");
    let mut sample: Value = printobserver_types::serde_json::from_str(
        &std::fs::read_to_string(&path).expect("the committed sample reads"),
    )
    .expect("the committed sample is JSON");
    sample["print"]["id"] = json!(OBICO_PRINT);
    sample["img_url"] = json!(image_url);
    sample.to_string()
}

/// One body carrying a reason and an operator, plus whatever else is given.
fn body(extra: &[(&str, Value)]) -> Value {
    let mut body = json!({ "reason": "the integration tier is asking", "actor": "operator" });
    let object = body.as_object_mut().expect("the body is an object");
    for (name, value) in extra {
        object.insert((*name).to_owned(), value.clone());
    }
    body
}

/// The same body, as the actor class the envelope grants one adjustment to and
/// nothing else.
fn as_the_agent(extra: &[(&str, Value)]) -> Value {
    let mut body = body(extra);
    body.as_object_mut().expect("the body is an object").insert(
        "actor".to_owned(),
        json!({ "agent": { "session_name": "watch-1" } }),
    );
    body
}

/// The manifest a start is bounded by, naming the file it is about.
fn manifest() -> Value {
    json!({
        "file_name": HOLD_FILE,
        "material": "PLA",
        "nozzle_diameter_mm": 0.4,
        "slicer_profile": "0.20mm QUALITY",
        "allowed": { "feedrate": { "min": NARROWED.0, "max": NARROWED.1 } },
        "metadata": {},
    })
}

/// The body a manifest write takes: the manifest, and why it is written.
fn manifest_write(reason: Option<&str>) -> Value {
    let mut body = json!({ "manifest": manifest() });
    if let Some(reason) = reason {
        body.as_object_mut()
            .expect("the body is an object")
            .insert("reason".to_owned(), json!(reason));
    }
    body
}

/// What one mutating operation is asked, and what asking should do.
struct Live {
    /// A body the policy accepts.
    accepted: Value,
    /// A body the policy rejects, of this operation's own kind.
    rejected: Value,
    /// The state the machine should report once it has taken the accepted body,
    /// when the machine reports the effect at all.
    reaches: Option<&'static str>,
    /// Where a snapshot reports the value this operation set, and what it
    /// should read as.
    reported: Option<(&'static str, Value)>,
    /// What the instance should have received, for a value it reports nothing
    /// about: every fragment one recorded body has to carry.
    received: Vec<String>,
    /// The adjustable this operation changes, when it changes one.
    adjustable: Option<&'static str>,
}

/// What every mutating operation is asked, by the name it is declared under.
///
/// The rejected body is of that operation's own kind: a value outside the
/// bounds the manifest narrowed for an adjustment, and an actor class the
/// envelope does not grant the action to for the rest.
fn live(name: &str) -> Live {
    match name {
        "pause" => Live {
            accepted: body(&[]),
            rejected: as_the_agent(&[]),
            reaches: Some("paused"),
            reported: None,
            received: Vec::new(),
            adjustable: None,
        },
        "resume" => Live {
            accepted: body(&[]),
            rejected: as_the_agent(&[]),
            reaches: Some("printing"),
            reported: None,
            received: Vec::new(),
            adjustable: None,
        },
        "cancel" => Live {
            accepted: body(&[]),
            rejected: as_the_agent(&[]),
            reaches: Some("operational"),
            reported: None,
            received: Vec::new(),
            adjustable: None,
        },
        "start_print" => Live {
            accepted: body(&[("file_name", json!(HOLD_FILE)), ("manifest", manifest())]),
            rejected: as_the_agent(&[("file_name", json!(HOLD_FILE)), ("manifest", manifest())]),
            reaches: Some("printing"),
            reported: None,
            received: Vec::new(),
            adjustable: None,
        },
        "acknowledge_failure" => Live {
            accepted: body(&[
                ("event_id", json!(printobserver_types::EventId::new())),
                ("disposition", json!("stop")),
            ]),
            rejected: as_the_agent(&[
                ("event_id", json!(printobserver_types::EventId::new())),
                ("disposition", json!("continue")),
            ]),
            // Stopping a print is cancelling it, which is what the machine
            // reports afterwards.
            reaches: Some("operational"),
            reported: None,
            received: Vec::new(),
            adjustable: None,
        },
        "set_tool_target_c" => Live {
            accepted: body(&[("tool", json!(0)), ("target_c", json!(205.0))]),
            rejected: body(&[("tool", json!(0)), ("target_c", json!(900.0))]),
            reaches: None,
            reported: Some(("printer.tools.0.target_c.value", json!(205.0))),
            received: Vec::new(),
            adjustable: Some("tool_target:0"),
        },
        "set_bed_target_c" => Live {
            accepted: body(&[("target_c", json!(61.0))]),
            rejected: body(&[("target_c", json!(400.0))]),
            reaches: None,
            reported: Some(("printer.bed.target_c.value", json!(61.0))),
            received: Vec::new(),
            adjustable: Some("bed_target"),
        },
        "set_feedrate_factor" => Live {
            accepted: body(&[("factor", json!(1.13))]),
            rejected: body(&[("factor", json!(1.45))]),
            reaches: None,
            reported: None,
            received: vec![percent_of_multiplier(1.13).to_string()],
            adjustable: Some("feedrate"),
        },
        "set_flowrate_factor" => Live {
            accepted: body(&[("factor", json!(1.07))]),
            rejected: body(&[("factor", json!(4.0))]),
            reaches: None,
            reported: None,
            received: vec![percent_of_multiplier(1.07).to_string()],
            adjustable: Some("flowrate"),
        },
        "set_fan_percent" => Live {
            accepted: body(&[("percent", json!(55.0))]),
            rejected: body(&[("percent", json!(500.0))]),
            reaches: None,
            reported: None,
            // The one command this adapter ever sends, the parameter it binds
            // its number to, and the duty its own conversion gives for that
            // percentage — all three read off the adapter's own exports.
            received: vec![
                FAN_SET_COMMAND.to_owned(),
                FAN_PWM_PARAMETER.to_owned(),
                fan_pwm_of_percent(55.0).to_string(),
            ],
            adjustable: Some("fan"),
        },
        other => panic!("`{other}` is a mutating operation this walk does not know"),
    }
}

/// Deliver one alert to the real ingress, and wait its handling out.
///
/// The proxy's record and the responder's own log are forgotten first, so that
/// everything read afterwards is *this* turn's: the machine's requests since
/// this alert, and the actions the agent issued inside the turn it prompted.
/// Without that boundary an assertion about "the accepted action" would be
/// satisfied by one an earlier turn issued, and the walk would go on passing
/// with the turn after a restart doing nothing at all.
async fn deliver(world: &Composed, proxy: &Proxy, image_url: &str) {
    proxy.forget();
    world.forget_responder_log();
    let mut completions = world.server.completions();
    let status = world
        .client
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(alert_body(image_url))
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(
        status,
        reqwest::StatusCode::ACCEPTED,
        "the real ingress did not take the committed sample"
    );
    completions.changed().await.expect("the handling finishes");
}

/// The print this walk's alerts opened.
async fn print_of(world: &Composed) -> PrintId {
    world
        .store
        .print_by_obico_id(OBICO_PRINT)
        .await
        .expect("the print reads")
        .expect("the alert opened a print")
        .id
}

/// One status read of the print.
async fn status(world: &Composed, print_id: PrintId) -> Value {
    let (code, answer) = world.get(&world.operation_url("status", print_id)).await;
    assert_eq!(code, reqwest::StatusCode::OK, "{answer}");
    answer
}

/// One value of an answer, by a dotted path through it.
fn at<'a>(answer: &'a Value, path: &str) -> &'a Value {
    let mut here = answer;
    for segment in path.split('.') {
        here = match segment.parse::<usize>() {
            Ok(index) => &here[index],
            Err(_) => &here[segment],
        };
    }
    here
}

/// Wait until the machine reports one connection state.
async fn until_state(world: &Composed, print_id: PrintId, wanted: &str) {
    until(&format!("the machine to report {wanted}"), || async {
        let seen = status(world, print_id).await["printer"]["connection"].clone();
        if seen == json!(wanted) {
            None
        } else {
            Some(seen.to_string())
        }
    })
    .await;
}

/// Ask for one operation, and answer what the server said.
async fn ask(world: &Composed, print_id: PrintId, name: &str, body: &Value) -> (u16, Value) {
    let url = world.operation_url(name, print_id);
    let (code, answer) = match name {
        "manifest_set" => world.put(&url, body).await,
        _ => world.post(&url, body).await,
    };
    (code.as_u16(), answer)
}

/// The whole loop.
pub async fn walk(instance: &Scripted) {
    let host = image_host(snapshot_bytes()).await;
    // An alert delivered to an idle machine ends the print it opens, so the
    // machine is printing before the first one arrives.
    hold_the_print_running(instance).await;
    let proxy = Proxy::in_front_of(&instance.url);
    let world = Composed::open(instance, &proxy.base_url()).await;

    let print_id = an_alert_opens_a_print_a_session_and_an_image(&world, &proxy, &host.url()).await;
    the_agent_acted_through_the_api(&world, &proxy, print_id).await;
    the_reads_answer_the_records_they_name(&world, print_id).await;
    every_change_refuses_a_request_with_no_reason(&world, &proxy, print_id).await;
    every_mutating_operation_is_rejected_in_its_own_kind(&world, &proxy, print_id).await;
    every_adjustment_applies_the_duration_it_is_given(&world, print_id).await;
    every_mutating_operation_has_its_own_effect(&world, &proxy, print_id).await;
    the_history_accounts_for_every_step(&world, print_id).await;

    // Restart, and a second alert continues the first alert's session — with the
    // agent acting through the API again inside it.
    hold_the_print_running(instance).await;
    let world = world.restart().await;
    assert!(
        world.server.reconciliation().adopted.contains(&print_id),
        "the restart did not adopt the print it was watching: {:?}",
        world.server.reconciliation()
    );
    let before = status(&world, print_id).await["session"]["session_name"].clone();
    deliver(&world, &proxy, &host.url()).await;
    let after = status(&world, print_id).await;
    assert_eq!(
        after["session"]["session_name"], before,
        "the second alert did not continue the first alert's session"
    );
    // The whole of the assertion again, against the boundary this alert set:
    // the turn after a restart has to act exactly as the first one did, and a
    // log read across both would be satisfied by the first turn alone.
    the_agent_acted_through_the_api(&world, &proxy, print_id).await;

    let reconciled = history(&world, print_id).await;
    assert!(
        reconciled.contains(&EventKind::StartupReconciliation),
        "the restart recorded none of what it adopted: {reconciled:?}"
    );
    assert_eq!(
        reconciled
            .iter()
            .filter(|kind| **kind == EventKind::SupervisionSessionOpened)
            .count(),
        1,
        "the second alert opened a session of its own: {reconciled:?}"
    );

    // Leave the environment as the bring-up recipe left it.
    hold_the_print_running(instance).await;
    world.server.stop().await;
}

/// The alert opens the print, stores its image, and opens a session.
async fn an_alert_opens_a_print_a_session_and_an_image(
    world: &Composed,
    proxy: &Proxy,
    image_url: &str,
) -> PrintId {
    deliver(world, proxy, image_url).await;
    let print_id = print_of(world).await;
    let recorded = history(world, print_id).await;
    assert!(
        recorded.contains(&EventKind::ObicoFailureAlert),
        "the alert was not recorded: {recorded:?}"
    );
    assert!(
        recorded.contains(&EventKind::SupervisionSessionOpened),
        "no session opened through the harness: {recorded:?}"
    );
    let opened = status(world, print_id).await;
    assert!(
        opened["session"]["session_name"]
            .as_str()
            .is_some_and(|name| !name.is_empty()),
        "the alert opened no session through the harness: {opened}"
    );
    stored_image_is_a_path(world, print_id).await;
    print_id
}

/// The agent asked this server for something, through the running API, in the
/// turn this alert prompted.
///
/// The responder issues both of its actions on every turn: one the policy admits
/// and one it does not. What is asserted here is about **that turn alone** — the
/// log and the machine's record were forgotten before the alert was delivered —
/// and it is asserted at the machine and at this server's own persisted records
/// rather than at the status the answer came under:
///
/// * the accepted action reached the real instance, which received the request
///   carrying the factor that adapter converts the asked-for one to;
/// * it opened an intervention whose expiry is the duration asked for, and that
///   intervention is the one the API answers as still in force;
/// * the action, the intervention and the assessment the turn wrote are all
///   about the same print and the same session;
/// * and the refused one is refused for the value it asked, with nothing
///   carrying that value reaching the machine.
async fn the_agent_acted_through_the_api(world: &Composed, proxy: &Proxy, print_id: PrintId) {
    let acted = world.responder_log();
    assert!(
        !acted.is_empty(),
        "the agent's own turn issued no action through the API; nothing was written \
         down since this alert"
    );

    let answered = |status: u16| -> Vec<&Value> {
        acted
            .iter()
            .filter(|line| line["status"] == json!(status))
            .collect()
    };
    let taken = answered(200);
    let refused = answered(409);
    assert_eq!(
        taken.len(),
        1,
        "this turn issued {} accepted actions and the agent issues one: {acted:?}",
        taken.len()
    );
    assert_eq!(
        refused.len(),
        1,
        "this turn issued {} refused actions and the agent issues one: {acted:?}",
        refused.len()
    );
    assert_eq!(
        acted.len(),
        2,
        "this turn issued something beside the two actions the agent is scripted \
         with: {acted:?}"
    );

    let accepted = &taken[0]["body"];
    let rejected = &refused[0]["body"];
    the_accepted_action_reached_the_machine(proxy);
    the_accepted_action_says_what_the_agent_asked(accepted, print_id);
    the_records_of_this_turn_are_about_one_print_and_one_session(world, print_id, accepted).await;
    the_refused_action_says_what_it_was_refused_for(rejected, print_id);
}

/// The instance received the request the accepted action asked for, and none
/// carrying the value the policy refused.
///
/// `OctoPrint` reports no applied feedrate factor at all, so the answer's own
/// `succeeded` says only that the instance did not refuse the request — what
/// says the request was made, and with what, is the machine's own record of it.
fn the_accepted_action_reached_the_machine(proxy: &Proxy) {
    let asked = percent_of_multiplier(AGENT_FACTOR).to_string();
    let bodies = proxy.bodies();
    assert!(
        bodies.iter().any(|body| body.contains(asked.as_str())),
        "the agent's accepted action answered `succeeded` and the instance received \
         no request carrying {asked:?}: {bodies:?}"
    );
    let never_sent = percent_of_multiplier(AGENT_REFUSED_FACTOR).to_string();
    assert!(
        !bodies.iter().any(|body| body.contains(never_sent.as_str())),
        "the value the policy refused reached the machine as {never_sent:?}: {bodies:?}"
    );
}

/// The answer says what the agent asked, and opened the intervention it asked
/// the change to stand for.
fn the_accepted_action_says_what_the_agent_asked(accepted: &Value, print_id: PrintId) {
    assert_eq!(
        accepted["record"]["decision"],
        json!("accepted"),
        "{accepted}"
    );
    assert_eq!(
        accepted["record"]["outcome"],
        json!("succeeded"),
        "{accepted}"
    );
    assert_eq!(
        accepted["record"]["print_id"],
        json!(print_id.to_string()),
        "the agent's action was recorded against another print: {accepted}"
    );
    let asked_for = &accepted["record"]["request"]["action"];
    assert_eq!(
        asked_for["action"],
        json!("set_feedrate_factor"),
        "{accepted}"
    );
    assert_eq!(asked_for["factor"], json!(AGENT_FACTOR), "{accepted}");
    assert_eq!(
        asked_for["duration_s"],
        json!(AGENT_DURATION_S),
        "{accepted}"
    );

    let intervention = &accepted["intervention"];
    assert!(
        !intervention.is_null(),
        "the bounded action the agent issued opened no intervention: {accepted}"
    );
    assert_eq!(intervention["adjustable"], json!("feedrate"), "{accepted}");
    assert_eq!(
        intervention["applied_value"],
        json!(AGENT_FACTOR),
        "the intervention is not about the value the agent asked for: {accepted}"
    );
    assert_eq!(
        instant(&intervention["expires_at"]) - instant(&intervention["applied_at"]),
        AGENT_DURATION_S,
        "the intervention's expiry is not the duration the agent asked for: {accepted}"
    );
    assert_eq!(
        intervention["action_id"], accepted["record"]["id"],
        "the intervention was opened by another action: {accepted}"
    );
    assert_eq!(
        intervention["print_id"],
        json!(print_id.to_string()),
        "the intervention is against another print: {accepted}"
    );
}

/// The action, the intervention and the assessment this turn wrote are about
/// one print and one session, read back out of this server's own records.
async fn the_records_of_this_turn_are_about_one_print_and_one_session(
    world: &Composed,
    print_id: PrintId,
    accepted: &Value,
) {
    let answered = status(world, print_id).await;
    let session = answered["session"]["session_name"].clone();
    assert_eq!(
        accepted["record"]["request"]["actor"]["agent"]["session_name"], session,
        "the action the agent issued names another session than the turn it ran in: \
         {accepted} against {session}"
    );

    let intervention = &accepted["intervention"];
    let held = answered["interventions"]
        .as_array()
        .expect("the interventions read")
        .iter()
        .find(|found| found["id"] == intervention["id"])
        .unwrap_or_else(|| {
            panic!(
                "the intervention the agent opened is not one this server holds in \
                 force: {answered}"
            )
        });
    assert_eq!(
        held["expires_at"], intervention["expires_at"],
        "the intervention this server holds expires at another instant: {held}"
    );
    assert_eq!(
        held["action_id"], accepted["record"]["id"],
        "the intervention this server holds was opened by another action: {held}"
    );
    assert_eq!(
        held["print_id"],
        json!(print_id.to_string()),
        "the intervention this server holds is against another print: {held}"
    );

    let assessed = history_records(world, print_id)
        .await
        .into_iter()
        // Newest first, so the first is the one this turn wrote.
        .find(|event| event["kind"] == json!("agent_assessment"))
        .unwrap_or_else(|| panic!("the turn wrote down no assessment for print {print_id}"));
    assert_eq!(
        assessed["print_id"],
        json!(print_id.to_string()),
        "the assessment is about another print: {assessed}"
    );
    assert_eq!(
        assessed["payload"]["session_name"], session,
        "the assessment names another session than the action the agent issued: \
         {assessed}"
    );
}

/// The refused action says the value it asked and the range it is held to, and
/// reached nothing.
fn the_refused_action_says_what_it_was_refused_for(rejected: &Value, print_id: PrintId) {
    assert_eq!(
        rejected["record"]["print_id"],
        json!(print_id.to_string()),
        "the refused action was recorded against another print: {rejected}"
    );
    let bounds = &rejected["record"]["decision"]["rejected"]["out_of_bounds"];
    assert_eq!(
        bounds["adjustable"],
        json!("feedrate"),
        "the policy refused the agent without naming what was out of bounds: {rejected}"
    );
    assert_eq!(
        bounds["requested"],
        json!(AGENT_REFUSED_FACTOR),
        "the policy refused a value other than the one the agent asked: {rejected}"
    );
    assert!(
        !bounds["allowed"].is_null(),
        "the policy refused the agent without the range it holds it to: {rejected}"
    );
    assert!(
        rejected["record"]["outcome"].is_null(),
        "a refused action reached the machine anyway: {rejected}"
    );
}

/// Every operation that changes something refuses a request carrying no reason.
async fn every_change_refuses_a_request_with_no_reason(
    world: &Composed,
    proxy: &Proxy,
    print_id: PrintId,
) {
    let mut walked = Vec::new();
    for operation in OPERATIONS {
        if !operation.is_mutating() {
            continue;
        }
        for (described, reason) in [
            ("no reason at all", None),
            ("a reason that is only whitespace", Some("   ")),
        ] {
            let before = stored(world, print_id).await;
            proxy.forget();
            let body = reasonless(&operation, reason);

            let (code, answer) = ask(world, print_id, operation.name, &body).await;

            assert_eq!(
                code, 400,
                "`{}` took a request carrying {described}: {answer}",
                operation.name
            );
            assert!(
                proxy.bodies().is_empty(),
                "`{}` carried a request with {described} to the machine: {:?}",
                operation.name,
                proxy.bodies()
            );
            assert_eq!(
                stored(world, print_id).await,
                before,
                "`{}` changed a stored record for a request with {described}",
                operation.name
            );
        }
        walked.push(operation.name);
    }
    let declared: Vec<&str> = OPERATIONS
        .iter()
        .filter(|operation| operation.is_mutating())
        .map(|operation| operation.name)
        .collect();
    assert_eq!(
        walked, declared,
        "the declared list holds an operation that changes something and this walk \
         did not reach"
    );
}

/// The body one mutating operation takes with the reason given — or with none.
///
/// The manifest it offers narrows the feedrate, so a server that wrote it before
/// ruling on the reason would move both the stored manifest and the print's
/// narrowings, and the record comparison would see it.
fn reasonless(operation: &Operation, reason: Option<&str>) -> Value {
    if operation.effect == Effect::Write {
        return manifest_write(reason);
    }
    let mut body = live(operation.name).accepted;
    let object = body.as_object_mut().expect("the body is an object");
    match reason {
        Some(blank) => {
            object.insert("reason".to_owned(), json!(blank));
        }
        None => {
            object.remove("reason");
        }
    }
    body
}

/// Every mutating operation answers the policy's own rejection, and moves
/// nothing.
async fn every_mutating_operation_is_rejected_in_its_own_kind(
    world: &Composed,
    proxy: &Proxy,
    print_id: PrintId,
) {
    let mut walked = Vec::new();
    for operation in OPERATIONS {
        let Effect::Mutating(_) = operation.effect else {
            continue;
        };
        let plan = live(operation.name);
        let before = stored(world, print_id).await;
        let state_before = status(world, print_id).await["printer"]["connection"].clone();
        proxy.forget();

        let (code, answer) = ask(world, print_id, operation.name, &plan.rejected).await;

        assert_eq!(code, 409, "`{}` was not rejected: {answer}", operation.name);
        let rejection = &answer["record"]["decision"]["rejected"];
        assert!(
            !rejection.is_null(),
            "`{}` answered no rejection the caller can act on: {answer}",
            operation.name
        );
        if let Some(adjustable) = plan.adjustable {
            let bounds = &rejection["out_of_bounds"];
            assert_eq!(
                bounds["adjustable"],
                json!(adjustable),
                "`{}` was rejected without naming what was out of bounds: {answer}",
                operation.name
            );
            assert!(
                !bounds["requested"].is_null() && !bounds["allowed"].is_null(),
                "`{}` was rejected without the value asked for and the range \
                 allowed: {answer}",
                operation.name
            );
        } else {
            assert!(
                !rejection["actor_may_not_request"].is_null(),
                "`{}` was rejected for something other than the actor asking: {answer}",
                operation.name
            );
        }
        assert!(
            proxy.bodies().is_empty(),
            "`{}` was rejected and the machine was asked {:?}",
            operation.name,
            proxy.bodies()
        );
        assert_eq!(
            status(world, print_id).await["printer"]["connection"],
            state_before,
            "`{}` was rejected and the machine moved anyway",
            operation.name
        );
        assert_eq!(
            stored(world, print_id).await,
            before,
            "`{}` was rejected and a stored record moved anyway",
            operation.name
        );
        walked.push(operation.name);
    }
    assert_eq!(
        walked,
        vocabulary(),
        "the declared list holds a mutating operation this walk did not reject"
    );
}

/// Every adjustment applies the duration it is given, and opens nothing without.
async fn every_adjustment_applies_the_duration_it_is_given(world: &Composed, print_id: PrintId) {
    let mut walked = Vec::new();
    for operation in OPERATIONS {
        let Effect::Mutating(_) = operation.effect else {
            continue;
        };
        let plan = live(operation.name);
        let Some(adjustable) = plan.adjustable else {
            continue;
        };

        let mut bounded = plan.accepted.clone();
        bounded
            .as_object_mut()
            .expect("the body is an object")
            .insert("duration_s".to_owned(), json!(DURATION_S));
        let (code, answer) = ask(world, print_id, operation.name, &bounded).await;
        assert_eq!(code, 200, "`{}`: {answer}", operation.name);

        let intervention = &answer["intervention"];
        assert!(
            !intervention.is_null(),
            "`{}` was given a duration and opened no intervention: {answer}",
            operation.name
        );
        assert_eq!(
            intervention["adjustable"],
            json!(adjustable),
            "`{}` opened an intervention about something else: {answer}",
            operation.name
        );
        assert_eq!(
            instant(&intervention["expires_at"]) - instant(&intervention["applied_at"]),
            DURATION_S,
            "`{}` opened an intervention whose expiry is not the duration asked \
             for: {answer}",
            operation.name
        );

        let (code, answer) = ask(world, print_id, operation.name, &plan.accepted).await;
        assert_eq!(code, 200, "`{}`: {answer}", operation.name);
        assert!(
            answer["intervention"].is_null(),
            "`{}` was given no duration and opened an intervention anyway: {answer}",
            operation.name
        );
        walked.push(operation.name);
    }
    assert_eq!(
        walked.len(),
        5,
        "this walk bounded {} adjustments, and the vocabulary declares five: {walked:?}",
        walked.len()
    );
}

/// The order a shared machine admits: the adjustments while the print runs, then
/// pause and resume, then the cancel that ends it, then the start that puts it
/// back, then the acknowledgement that stops it again.
const IN_ORDER: [&str; 10] = [
    "set_bed_target_c",
    "set_tool_target_c",
    "set_feedrate_factor",
    "set_flowrate_factor",
    "set_fan_percent",
    "pause",
    "resume",
    "cancel",
    "start_print",
    "acknowledge_failure",
];

/// Every mutating operation has the effect it names, at the machine itself.
async fn every_mutating_operation_has_its_own_effect(
    world: &Composed,
    proxy: &Proxy,
    print_id: PrintId,
) {
    let mut walked = Vec::new();
    for name in IN_ORDER {
        let plan = live(name);
        proxy.forget();
        let (code, answer) = ask(world, print_id, name, &plan.accepted).await;
        assert_eq!(code, 200, "`{name}` was not accepted: {answer}");
        assert_eq!(
            answer["record"]["decision"],
            json!("accepted"),
            "`{name}` answered a decision that is not acceptance: {answer}"
        );
        assert_eq!(
            answer["record"]["outcome"],
            json!("succeeded"),
            "`{name}` reached the real instance and it did not take it: {answer}"
        );

        if let Some(state) = plan.reaches {
            until_state(world, print_id, state).await;
        }
        if let Some((path, value)) = plan.reported {
            until(
                &format!("the machine to report {path} as {value}"),
                || async {
                    let seen = at(&status(world, print_id).await, path).clone();
                    if seen == value {
                        None
                    } else {
                        Some(seen.to_string())
                    }
                },
            )
            .await;
        }
        if !plan.received.is_empty() {
            let bodies = proxy.bodies();
            assert!(
                bodies.iter().any(|body| plan
                    .received
                    .iter()
                    .all(|fragment| body.contains(fragment.as_str()))),
                "`{name}` reports nothing a snapshot can read, and the instance \
                 received no request carrying all of {:?}: {bodies:?}",
                plan.received
            );
        }
        walked.push(name);
    }

    let mut reached = walked.clone();
    reached.sort_unstable();
    let mut declared = vocabulary();
    declared.sort_unstable();
    assert_eq!(
        reached, declared,
        "the declared list holds a mutating operation whose effect this walk did \
         not reach"
    );
}

/// Every read answers the record it names, read back out of the store.
async fn the_reads_answer_the_records_they_name(world: &Composed, print_id: PrintId) {
    let (code, written) = ask(
        world,
        print_id,
        "manifest_set",
        &manifest_write(Some("the integration tier is narrowing the bounds")),
    )
    .await;
    assert_eq!(code, 200, "{written}");

    let (code, read_back) = world
        .get(&world.operation_url("manifest_get", print_id))
        .await;
    assert_eq!(code, reqwest::StatusCode::OK);
    assert_eq!(
        read_back["manifest"],
        manifest(),
        "the manifest read back is not the one written"
    );

    let answered = status(world, print_id).await;
    assert_eq!(answered["print"]["id"], json!(print_id.to_string()));
    assert!(
        !answered["printer"]["connection"].is_null(),
        "the status carries nothing the machine reported: {answered}"
    );

    let (code, context) = world.get(&world.operation_url("context", print_id)).await;
    assert_eq!(code, reqwest::StatusCode::OK);
    assert_eq!(
        context["context"]["manifest"],
        manifest(),
        "the context does not carry the manifest that was written"
    );
    assert_eq!(
        context["context"]["print"]["id"],
        json!(print_id.to_string()),
        "the context is about another print: {context}"
    );

    let (code, events) = world.get(&world.operation_url("history", print_id)).await;
    assert_eq!(code, reqwest::StatusCode::OK);
    assert!(
        events["events"]
            .as_array()
            .is_some_and(|found| !found.is_empty()),
        "the history answered no events at all: {events}"
    );
}

/// The image the alert brought is a path on this host whose contents are what
/// the record declares.
async fn stored_image_is_a_path(world: &Composed, print_id: PrintId) {
    let image = world
        .store
        .history(printobserver_store_api::HistoryQuery {
            print_id,
            kinds: Vec::new(),
            since: None,
            until: None,
            limit: None,
        })
        .await
        .expect("the history reads")
        .into_iter()
        .find_map(|event| event.image)
        .expect("the alert's image was stored");
    let url = format!(
        "http://{}{}",
        world.server.address(),
        printobserver_server::operation("image")
            .expect("image is served")
            .full_path()
            .replace("{image_id}", &image.id.to_string())
    );
    let (code, answer) = world.get(&url).await;
    assert_eq!(code, reqwest::StatusCode::OK, "{answer}");
    let path = std::path::PathBuf::from(
        answer["path"]
            .as_str()
            .unwrap_or_else(|| panic!("the image answer carries no path: {answer}")),
    );
    assert!(path.is_absolute() && path.exists(), "{}", path.display());
    assert_eq!(
        digest_at(&path),
        answer["record"]["sha256"].as_str().expect("a digest"),
        "what is at the path this server answered is not what the record declares"
    );
}

/// The history afterwards accounts for every step, and ties them together.
///
/// Every kind this walk's own steps raise has to be there — the alert that
/// started it, the session the turn opened, the assessment it wrote, and the
/// port failure the image host's own teardown raises is deliberately *not*
/// among them, because a walk that required one would be requiring a failure.
/// And the events are not read as a set of kinds alone: the alert, the session
/// and the assessment are asserted to be about the same print and the same
/// session the agent's own action named, which is what makes the history an
/// account of one loop rather than a list of things that happened.
async fn the_history_accounts_for_every_step(world: &Composed, print_id: PrintId) {
    let records = history_records(world, print_id).await;
    let kinds: Vec<Value> = records.iter().map(|event| event["kind"].clone()).collect();
    for wanted in [
        EventKind::ObicoFailureAlert,
        EventKind::SupervisionSessionOpened,
        EventKind::AgentAssessment,
    ] {
        let spelled = printobserver_types::serde_json::to_value(wanted).expect("a kind renders");
        assert!(
            kinds.contains(&spelled),
            "the history does not account for {wanted:?}: {kinds:?}"
        );
    }
    assert!(
        records
            .iter()
            .all(|event| event["print_id"] == json!(print_id.to_string())),
        "the history of one print carries an event about another: {records:?}"
    );

    let session = status(world, print_id).await["session"]["session_name"].clone();
    let opened = records
        .iter()
        .find(|event| event["kind"] == json!("supervision_session_opened"))
        .expect("the history accounts for the session that opened");
    assert_eq!(
        opened["payload"]["session_name"], session,
        "the session the history says opened is not the one this print is watched \
         through: {opened}"
    );
    for assessment in records
        .iter()
        .filter(|event| event["kind"] == json!("agent_assessment"))
    {
        assert_eq!(
            assessment["payload"]["session_name"], session,
            "an assessment was written into a session this print is not watched \
             through: {assessment}"
        );
    }

    // The alert that started it is the one the producer sent, carrying its own
    // identifier for the print and the bytes it arrived as.
    let alert = records
        .iter()
        .find(|event| event["kind"] == json!("obico_failure_alert"))
        .expect("the history accounts for the alert");
    assert_eq!(
        alert["payload"]["obico_print_id"],
        json!(OBICO_PRINT),
        "the alert in the history is about another of the producer's prints: {alert}"
    );
    assert!(
        !alert["raw"].is_null(),
        "the alert in the history carries none of the bytes that arrived: {alert}"
    );
    assert!(
        !alert["image"].is_null(),
        "the alert in the history carries no image: {alert}"
    );
}

/// Every record this system stores about one print, as a caller reads them.
///
/// The printer's own snapshot is deliberately not among them: it carries the
/// instant it was observed at, so two reads of an unchanged machine differ.
async fn stored(world: &Composed, print_id: PrintId) -> Value {
    let answered = status(world, print_id).await;
    let (_, events) = world.get(&world.operation_url("history", print_id)).await;
    let (_, manifest) = world
        .get(&world.operation_url("manifest_get", print_id))
        .await;
    json!({
        "print": answered["print"],
        "session": answered["session"],
        "interventions": answered["interventions"],
        "events": events["events"],
        "manifest": manifest,
    })
}

/// Every operation of the action vocabulary, in the order it is declared.
fn vocabulary() -> Vec<&'static str> {
    OPERATIONS
        .iter()
        .filter(|operation| matches!(operation.effect, Effect::Mutating(_)))
        .map(|operation| operation.name)
        .collect()
}

/// Every event the print's history carries, newest first.
async fn history_records(world: &Composed, print_id: PrintId) -> Vec<Value> {
    let (code, answer) = world.get(&world.operation_url("history", print_id)).await;
    assert_eq!(code, reqwest::StatusCode::OK, "{answer}");
    answer["events"]
        .as_array()
        .expect("a history is a list")
        .clone()
}

/// Every kind the print's history carries.
async fn history(world: &Composed, print_id: PrintId) -> Vec<EventKind> {
    let (code, answer) = world.get(&world.operation_url("history", print_id)).await;
    assert_eq!(code, reqwest::StatusCode::OK, "{answer}");
    answer["events"]
        .as_array()
        .expect("a history is a list")
        .iter()
        .filter_map(|event| printobserver_types::serde_json::from_value(event["kind"].clone()).ok())
        .collect()
}

/// One instant of an answer, as whole seconds after the epoch.
fn instant(value: &Value) -> i64 {
    let text = value.as_str().expect("an instant is a string");
    let parsed: printobserver_types::Timestamp = text.parse().expect("an instant is RFC 3339");
    parsed.as_utc().timestamp()
}

/// The digest of what is at one path, lowercase hexadecimal.
fn digest_at(path: &std::path::Path) -> String {
    use sha2::{Digest as _, Sha256};
    let bytes = std::fs::read(path).expect("the image reads");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut rendered, byte| {
            use core::fmt::Write as _;
            let _ = write!(rendered, "{byte:02x}");
            rendered
        })
}
