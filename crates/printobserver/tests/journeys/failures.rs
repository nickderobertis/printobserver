//! The three-way failure contract, over the whole client surface.
//!
//! Which command owes which failure is read off the contract rather than off
//! what the finished program happens to make reachable: **every** client
//! command owes the unreachable server and the unconfigured program, because
//! every one of them is a request to a server it has to find and be configured
//! for; and every command carrying an action of the vocabulary owes the policy
//! rejection, because every action reaches the same policy and an action the
//! policy cannot reject is an action that escaped it.

use printobserver::failure::Exit;

use crate::traced::Ran;
use crate::walk::{self, Driven};
use crate::world::World;

use super::running;

/// The value each adjustment is refused for asking for, outside its bound.
fn outside_the_bounds(field: &str) -> Option<&'static str> {
    match field {
        "factor" => Some("9.5"),
        // The two temperatures and the fan share a value outside every one of
        // their bounds, which is what makes this a rule rather than a table.
        "target_c" | "percent" => Some("900"),
        _ => None,
    }
}

/// The arguments one command succeeds with.
pub fn succeeding(one: &Driven) -> Vec<String> {
    let mut given = vec![one.command.name.clone()];
    for field in &one.command.fields {
        given.push(field.forms[0].option.clone());
        given.push(one.text(&field.parameter.name));
    }
    given
}

/// The arguments one command is refused by the policy for.
///
/// An adjustment asks for a value outside the range the envelope allows, so the
/// rejection carries the value and the range. Every other action asks as the
/// agent, whom this world's envelope grants nothing — which is the rejection an
/// action with no value to rule on gets.
pub fn rejected(world: &World, one: &Driven) -> Vec<String> {
    let mut given = vec![one.command.name.clone()];
    let mut out_of_bounds = false;
    for field in &one.command.fields {
        let name = &field.parameter.name;
        let value = match outside_the_bounds(name) {
            Some(outside) => {
                out_of_bounds = true;
                outside.to_owned()
            }
            None => one.text(name),
        };
        given.push(field.forms[0].option.clone());
        given.push(value);
    }
    if !out_of_bounds {
        let actor = given
            .iter()
            .position(|word| word == "--actor")
            .expect("every action names who is asking");
        given[actor + 1] = agent_actor(world);
    }
    given
}

/// An actor of the class this world's envelope grants nothing.
fn agent_actor(world: &World) -> String {
    format!(
        "{{\"agent\":{{\"session_name\":\"watch-{}\"}}}}",
        world.print_id
    )
}

/// Every client command, driven to each failure the contract says it owes.
pub fn every_command_owes_its_failures(world: &World) {
    for one in walk::walk(world) {
        world.machine.reports(one.reports);
        let arguments = succeeding(&one);

        let nothing_there = running::against_nothing(world, &arguments, crate::world::CREDENTIAL);
        said_what_to_do(
            &nothing_there,
            Exit::Unreachable,
            "printobserver server --config",
        );

        let nothing_configured = running::unconfigured(world, &arguments, crate::world::CREDENTIAL);
        said_what_to_do(
            &nothing_configured,
            Exit::Unconfigured,
            "PRINTOBSERVER_SERVER",
        );

        if one.operation().action_kind().is_some() {
            the_policy_refuses_it(world, &one);
        }
    }
}

/// One failure exits with its own status and says what to do next.
fn said_what_to_do(ran: &Ran, exit: Exit, next: &str) {
    assert_eq!(
        ran.code,
        Some(i32::from(exit.status())),
        "{:?} did not exit as `{exit}`: {}",
        ran.arguments,
        ran.said()
    );
    assert!(
        ran.err.contains(next),
        "{:?} was refused without naming a next action: {}",
        ran.arguments,
        ran.said()
    );
    assert!(
        !ran.said().contains("panicked") && !ran.said().contains("os error"),
        "{:?} exposed a raw transport error or panicked: {}",
        ran.arguments,
        ran.said()
    );
}

/// The policy refuses this action, and the refusal says what may be asked for.
fn the_policy_refuses_it(world: &World, one: &Driven) {
    let arguments = rejected(world, one);
    let asked: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let ran = running::command(world, &asked);

    assert_eq!(
        ran.code,
        Some(i32::from(Exit::Rejected.status())),
        "`{}` was not refused by the policy: {}",
        one.command.name,
        ran.said()
    );
    assert!(
        ran.err.contains("policy refused this action"),
        "`{}` was refused without saying the policy refused it: {}",
        one.command.name,
        ran.said()
    );
    let said = ran.said();
    assert!(
        said.contains("out_of_bounds") || said.contains("actor_may_not_request"),
        "`{}` was refused without the policy's own reason: {said}",
        one.command.name
    );
    if said.contains("out_of_bounds") {
        assert!(
            said.contains("It was asked for") && said.contains("what is allowed is"),
            "`{}` was refused without the value asked for and the range allowed: {said}",
            one.command.name
        );
    }
}

/// No mutating command runs without a reason, and nothing reaches the server.
pub fn no_mutating_command_runs_without_a_reason(world: &World) {
    for one in walk::walk(world) {
        if !one.operation().is_mutating() {
            continue;
        }
        let mut given = succeeding(&one);
        let at = given
            .iter()
            .position(|word| word == "--reason")
            .expect("every mutating command takes a reason");
        given.drain(at..=at + 1);
        let asked: Vec<&str> = given.iter().map(String::as_str).collect();

        world.proxy.forget();
        let ran = running::command(world, &asked);

        assert_eq!(
            ran.code,
            Some(i32::from(Exit::Usage.status())),
            "`{}` ran with no reason given: {}",
            one.command.name,
            ran.said()
        );
        assert!(
            ran.err.contains("--reason"),
            "`{}` was refused without naming the reason it needs: {}",
            one.command.name,
            ran.said()
        );
        assert_eq!(
            world.proxy.received(),
            Vec::new(),
            "`{}` with no reason reached the server",
            one.command.name
        );
    }
}
