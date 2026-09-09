//! The cross-product walk, and the three things every point of it carries.
//!
//! For each invocation: it succeeded, its process tree connected to no endpoint
//! but the configured one, the request the server received is the one that
//! command makes carrying the caller's own values, and the effect is confirmed
//! by reading it back through this same surface with those same values in the
//! record.
//!
//! # What "the effect" is, and what it is not
//!
//! A request written into the history is not an effect: `ActionRequested` is
//! appended **before** the machine is asked, so a command whose action reached
//! a machine that did nothing records one just the same. What is asserted here
//! is the resulting state or the changed record:
//!
//! * the machine reports the state that action produces — waited for, because a
//!   machine takes its own time, and never asked for, because asking would be
//!   the journey producing the effect it is checking;
//! * an adjustment's bounded intervention, carrying the caller's own value;
//! * a start's manifest, read back **whole** rather than by one field;
//! * and, for every action alike, `ActionExecuted` against that same action —
//!   the record written only once the printer port took it.
//!
//! `journeys/tainting.rs` drives all of it over a machine that answers success
//! and changes nothing, which is the no-op this exists to refuse.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;

use printobserver_server::Located;
use printobserver_types::serde_json::{Map, Value};

use crate::machine::Reports;
use crate::proxy::Received;
use crate::traced::Ran;
use crate::walk::{self, Driven, Invocation, as_sent};
use crate::world::World;

use super::answers::{answered, at};
use super::running;

/// Drive every point of the cross-product, and hold each to all of it.
pub fn the_cross_product_walk(world: &World) {
    let driven = walk::walk(world);
    let uncovered = walk::covers(&driven, world);
    assert!(
        uncovered.is_empty(),
        "this walk does not cover the surface it claims to: {uncovered:#?}"
    );

    for one in &driven {
        for invocation in walk::invocations(one, world) {
            // Before every invocation rather than once per command: a real
            // machine is moved by the command that just ran, so the next
            // invocation of that same command needs it put back.
            world.wants(one.reports);
            let ran = drive(world, &invocation);
            let received = world.proxy.the_one_request();
            the_request_carries_the_callers_values(one, &received);
            the_effect_is_confirmed_by_reading_it_back(world, one, &invocation, &ran);
        }
    }
}

/// One invocation, run and held to the two things every run is held to.
pub fn drive(world: &World, invocation: &Invocation) -> Ran {
    world.proxy.forget();
    let ran = running::run(world, invocation);
    assert_eq!(
        ran.code,
        Some(0),
        "{:?} did not succeed: {}",
        invocation.arguments,
        ran.said()
    );
    only_the_configured_endpoint(world.proxy.address, &ran);
    ran
}

/// One invocation's process tree reached nothing but the configured address.
pub fn only_the_configured_endpoint(configured: SocketAddr, ran: &Ran) {
    let expected: BTreeSet<SocketAddr> = [configured].into_iter().collect();
    assert_eq!(
        ran.connected, expected,
        "{:?} connected to something other than the supervisor it was configured with",
        ran.arguments
    );
}

/// The request that arrived is the one this command makes, with these values.
pub fn the_request_carries_the_callers_values(one: &Driven, received: &Received) {
    let operation = one.operation();
    assert_eq!(
        received.method,
        operation.method.as_str(),
        "`{}` made a {} request",
        one.command.name,
        received.method
    );
    assert_eq!(
        received.target,
        expected_target(one),
        "`{}` asked for something other than what its caller named",
        one.command.name
    );
    assert_eq!(
        body_of(received),
        expected_body(one),
        "`{}` sent a body other than the values its caller gave",
        one.command.name
    );
}

/// The target one command's own values name.
fn expected_target(one: &Driven) -> String {
    let mut target = one.operation().full_path();
    let mut asked = Vec::new();
    for field in &one.command.fields {
        let text = one.text(&field.parameter.name);
        match field.parameter.located {
            Located::Path => {
                target = target.replace(&format!("{{{}}}", field.parameter.name), &text);
            }
            Located::Query => asked.push(format!("{}={text}", field.parameter.name)),
            Located::Body => {}
        }
    }
    if asked.is_empty() {
        target
    } else {
        format!("{target}?{}", asked.join("&"))
    }
}

/// The body one command's own values make.
fn expected_body(one: &Driven) -> Value {
    if one.operation().accepts.is_none() {
        return Value::Null;
    }
    let carried: Map<String, Value> = one
        .command
        .fields
        .iter()
        .filter(|field| field.parameter.located == Located::Body)
        .map(|field| {
            (
                field.parameter.name.clone(),
                as_sent(field.parameter.kind, &one.text(&field.parameter.name)),
            )
        })
        .collect();
    Value::Object(carried)
}

/// The body that arrived, as a document.
fn body_of(received: &Received) -> Value {
    if received.body.is_empty() {
        return Value::Null;
    }
    printobserver_types::serde_json::from_str(&received.body).unwrap_or_else(|error| {
        panic!(
            "the body that arrived is not a document ({error}): {}",
            received.body
        )
    })
}

/// What one command did, read back through this same surface.
pub fn the_effect_is_confirmed_by_reading_it_back(
    world: &World,
    one: &Driven,
    invocation: &Invocation,
    ran: &Ran,
) {
    let said = answered(ran, invocation.machine_readable);
    match one.command.name.as_str() {
        "status" => {
            assert_eq!(at(&said, "print.id"), world.print_id);
            assert!(
                said.contains_key("printer.connection") && said.contains_key("job.state"),
                "a status read answered no printer and no job: {said:#?}"
            );
        }
        "context" => assert_eq!(at(&said, "context.print.id"), world.print_id),
        "image" => assert_eq!(at(&said, "record.id"), world.image_id),
        "history" => the_history_it_answers_is_this_prints(world, &said),
        // A write and the read beside it are confirmed the same way and
        // deliberately so: what a write did is what the read answers, and the
        // manifest both are about is the one the write drove.
        "manifest-get" | "manifest-set" => the_manifest_it_answers_is_the_one_that_was_written(
            world,
            &walk::manifest_of(
                "manifest-set",
                MANIFEST_SET_FEEDRATE,
                &world.printable_file(),
            ),
        ),
        _ => the_action_was_carried_out(world, one, &said),
    }
}

/// The widest feedrate the manifest each command drives allows.
///
/// Named here because a complete comparison needs the whole manifest rather
/// than a field of it, and the walk builds it from these two numbers.
const MANIFEST_SET_FEEDRATE: f64 = 1.4;

/// The same, for the manifest a start attaches.
const START_PRINT_FEEDRATE: f64 = 1.3;

/// The state each action leaves the machine reporting, for the actions that
/// move one.
///
/// The other three ask the machine for nothing that changes what it is doing —
/// an adjustment changes a value rather than a state, and an acknowledgement
/// with no stop asks for nothing at all — so what they are confirmed by is
/// below rather than here.
fn state_after(command: &str) -> Option<Reports> {
    match command {
        "pause" => Some(Reports::Paused),
        "resume" | "start-print" => Some(Reports::Printing),
        "cancel" => Some(Reports::Operational),
        _ => None,
    }
}

/// The events a history read answers belong to the print it named, and there
/// are no more of them than it asked for.
fn the_history_it_answers_is_this_prints(world: &World, said: &BTreeMap<String, String>) {
    let limit: usize = 7;
    let mut seen = 0;
    for index in 0..=limit {
        let Some(print) = said.get(&format!("events.{index}.print_id")) else {
            continue;
        };
        assert_eq!(
            print, &world.print_id,
            "a history read answered an event belonging to another print"
        );
        seen += 1;
    }
    assert!(
        seen > 0 && seen <= limit,
        "a history read asked for at most {limit} events and answered {seen}"
    );
}

/// The manifest a read answers is the whole of the one that was written.
///
/// Compared whole rather than by its profile: a write that carried the file
/// name, the material, the nozzle and the ranges and stored one of them would
/// satisfy a comparison of one field and lose the rest.
fn the_manifest_it_answers_is_the_one_that_was_written(world: &World, written: &Value) {
    let read = running::read(world, &["manifest-get", "--print-id", &world.print_id]);
    assert_eq!(
        read.get("manifest"),
        Some(written),
        "the manifest read back is not the one that was written"
    );
}

/// The action a command asked for was carried out, and carried its own values.
///
/// Three things rather than one. The request in the history carries the
/// caller's reason and every value they gave — which is what a substituted
/// value fails. `ActionExecuted` against that same action is the record the
/// printer port taking it writes, and nothing else writes. And where the action
/// moves the machine, the machine is waited for and asked nothing: a state it
/// never reaches is a command that did not produce it.
fn the_action_was_carried_out(world: &World, one: &Driven, said: &BTreeMap<String, String>) {
    let action_id = at(said, "record.id");
    let read = running::read(
        world,
        &["history", "--print-id", &world.print_id, "--limit", "40"],
    );
    the_request_in_the_history_carries_the_callers_values(one, &read, &action_id);
    assert!(
        the_history_carries(&read, "action_executed", &action_id),
        "`{}` left a request in the history and no record of it reaching the machine: {read}",
        one.command.name
    );
    if let Some(state) = state_after(&one.command.name) {
        world.settles_at(state, &one.command.name);
    }
    if one.command.name == "start-print" {
        the_manifest_it_answers_is_the_one_that_was_written(
            world,
            &walk::manifest_of("start-print", START_PRINT_FEEDRATE, &world.printable_file()),
        );
    }
    if let Some(applied) = adjusted_value(one) {
        the_intervention_it_opened_carries_that_value(world, one, &applied);
    }
}

/// The request the history carries for one action, with the caller's own
/// reason and every value they gave.
fn the_request_in_the_history_carries_the_callers_values(
    one: &Driven,
    read: &Value,
    action_id: &str,
) {
    let asked = requested_action(read, action_id).unwrap_or_else(|| {
        panic!(
            "`{}` left no request in the print's own history: {read}",
            one.command.name
        )
    });
    assert_eq!(
        asked.get("reason").and_then(Value::as_str),
        Some(walk::reason_of(&one.command.name).as_str()),
        "`{}` asked for something carrying a reason its caller did not give: {asked}",
        one.command.name
    );
    for field in &one.command.fields {
        if field.parameter.located != Located::Body || field.parameter.name == "actor" {
            continue;
        }
        let name = &field.parameter.name;
        assert_eq!(
            asked.get(name),
            Some(&as_sent(field.parameter.kind, &one.text(name))),
            "`{}` asked for `{name}` other than the value its caller gave: {asked}",
            one.command.name
        );
    }
}

/// The bounded intervention an adjustment opened is in force, carrying the
/// value its caller asked for.
fn the_intervention_it_opened_carries_that_value(world: &World, one: &Driven, applied: &Value) {
    let read = running::read(world, &["status", "--print-id", &world.print_id]);
    let held: Vec<&Value> = read
        .get("interventions")
        .and_then(Value::as_array)
        .map(|held| held.iter().filter(|_| true).collect())
        .unwrap_or_default();
    assert!(
        held.iter()
            .any(|found| found.get("applied_value") == Some(applied)),
        "`{}` opened no intervention carrying {applied}: {read}",
        one.command.name
    );
}

/// The value one adjustment applies, for a command that applies one.
fn adjusted_value(one: &Driven) -> Option<Value> {
    one.command
        .fields
        .iter()
        .find(|field| {
            matches!(
                field.parameter.name.as_str(),
                "factor" | "target_c" | "percent"
            )
        })
        .map(|field| as_sent(field.parameter.kind, &one.text(&field.parameter.name)))
}

/// The action one request in the history asked for, by its own identifier.
fn requested_action(read: &Value, action_id: &str) -> Option<Value> {
    one_event(read, "action_requested", action_id)?
        .pointer("/payload/action")
        .cloned()
}

/// Whether the history carries one kind of event about one action.
fn the_history_carries(read: &Value, kind: &str, action_id: &str) -> bool {
    one_event(read, kind, action_id).is_some()
}

/// One event of one kind about one action.
fn one_event(read: &Value, kind: &str, action_id: &str) -> Option<Value> {
    read.get("events")?
        .as_array()?
        .iter()
        .find(|event| {
            event.get("kind").and_then(Value::as_str) == Some(kind)
                && event.pointer("/payload/action_id").and_then(Value::as_str) == Some(action_id)
        })
        .cloned()
}
