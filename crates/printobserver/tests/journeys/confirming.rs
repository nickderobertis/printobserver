//! The cross-product walk, and the three things every point of it carries.
//!
//! For each invocation: it succeeded, its process tree connected to no endpoint
//! but the configured one, the request the server received is the one that
//! command makes carrying the caller's own values, and the effect is confirmed
//! by reading it back through this same surface with those same values in the
//! record.

use std::collections::BTreeSet;
use std::net::SocketAddr;

use printobserver_server::Located;
use printobserver_types::serde_json::{Map, Value};

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
        world.machine.reports(one.reports);
        for invocation in walk::invocations(one, world) {
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
fn the_effect_is_confirmed_by_reading_it_back(
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
        "manifest-get" => assert_eq!(
            at(&said, "manifest.slicer_profile"),
            profile_of("manifest-set")
        ),
        "manifest-set" => the_manifest_it_wrote_is_the_one_read_back(world),
        _ => the_action_it_asked_for_is_in_the_history(world, one),
    }
}

/// The events a history read answers belong to the print it named, and there
/// are no more of them than it asked for.
fn the_history_it_answers_is_this_prints(
    world: &World,
    said: &std::collections::BTreeMap<String, String>,
) {
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

/// The manifest a write put in place is the one a read answers.
fn the_manifest_it_wrote_is_the_one_read_back(world: &World) {
    let read = running::read(world, &["manifest-get", "--print-id", &world.print_id]);
    assert_eq!(
        read.pointer("/manifest/slicer_profile")
            .and_then(Value::as_str),
        Some(profile_of("manifest-set").as_str()),
        "the manifest read back is not the one that was written: {read}"
    );
}

/// The profile a manifest one command drove carries.
fn profile_of(command: &str) -> String {
    walk::manifest_of(command, 1.0)
        .pointer("/slicer_profile")
        .and_then(Value::as_str)
        .expect("a manifest names its profile")
        .to_owned()
}

/// The action a command asked for is in the print's own history, carrying the
/// caller's reason and the caller's own values.
fn the_action_it_asked_for_is_in_the_history(world: &World, one: &Driven) {
    let read = running::read(
        world,
        &["history", "--print-id", &world.print_id, "--limit", "40"],
    );
    let asked = newest_request(&read).unwrap_or_else(|| {
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

/// The newest action request the history carries.
fn newest_request(read: &Value) -> Option<Value> {
    read.get("events")?
        .as_array()?
        .iter()
        .find(|event| event.get("kind").and_then(Value::as_str) == Some("action_requested"))
        .and_then(|event| event.pointer("/payload/action").cloned())
}
