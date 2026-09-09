//! Every assertion this tier makes, driven over the defect it is about.
//!
//! A tier whose assertions cannot catch a violation is a tier nobody has proven
//! catches one, so each of them is run here over a command variant that is this
//! program with exactly one thing done wrong — and each has to refuse it. The
//! variant is built from the same library the program itself is, so what
//! differs between them is the defect and nothing else.
//!
//! The second connecting defect is the one a per-option walk cannot catch: it
//! connects directly **only** when machine-readable output and an explicit
//! configuration file are both given. It is here so that the cross-product's
//! combination coverage is proved rather than asserted.
//!
//! # Two of the defects are the machine's rather than the program's
//!
//! An effect assertion is about what happened at the machine, so what makes it
//! bite is a machine that does not do what it is asked. The stood-in one can be
//! told to answer success and change nothing, which is the **successful no-op**
//! — every action reaches it, every action is taken, the record says so, and
//! nothing moved — and to be told that only once an adjustment has landed,
//! which is an **incorrect restoration**: the record says the prior value went
//! back and the machine is still holding the adjusted one.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use printobserver::render::fields;
use printobserver_types::serde_json::{self, Value};

use crate::machine::Reports;
use crate::traced::{Ran, traced};
use crate::walk::{self, Driven};
use crate::world::World;

use super::confirming::{
    only_the_configured_endpoint, the_effect_is_confirmed_by_reading_it_back,
    the_request_carries_the_callers_values,
};
use super::{durations, running};

/// The variant of this program that carries one defect.
fn variant() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_printobserver-tainted"))
}

/// The command every defect here is driven over.
const OVER: &str = "set-fan-percent";

/// Drive every assertion over the defect it is about, and refuse each.
pub fn every_assertion_here_refuses_the_defect_it_is_about(world: &World) {
    world.wants(Reports::Printing);
    let one = the_command(world);

    the_endpoint_assertion_refuses(world, &one, "connects-directly", &[]);
    the_endpoint_assertion_refuses(
        world,
        &one,
        "connects-when-combined",
        &[
            "--json".to_owned(),
            "--config".to_owned(),
            world.client_config().display().to_string(),
        ],
    );
    the_request_assertion_refuses(world, &one, "fixed-value");
    the_request_assertion_refuses(world, &one, "fixed-reason");
    the_output_assertion_refuses(world, &one, "image-bytes");
    the_output_assertion_refuses(world, &one, "image-base64");
    the_effect_assertion_refuses_a_successful_no_op(world);
    the_restoration_assertion_refuses_an_incorrect_restoration(world);
}

/// The effect assertion refuses a machine that took the action and did nothing.
///
/// The request reaches the machine, the machine answers success, and the record
/// carries both the request and its execution — everything a walk that stopped
/// at `ActionRequested` would accept. What the assertion is about is the state
/// the action was supposed to produce, and this machine never reaches it.
fn the_effect_assertion_refuses_a_successful_no_op(world: &World) {
    if !world.machine_is_deaf(true) {
        return;
    }
    let paused = walk::walk(world)
        .into_iter()
        .find(|found| found.command.name == "pause")
        .expect("this walk drives a pause");
    let invocation = walk::invocations(&paused, world)
        .into_iter()
        .find(|invocation| invocation.machine_readable)
        .expect("one invocation asks for machine-readable output");

    let ran = running::run(world, &invocation);
    the_record_a_weaker_check_would_have_accepted_is_there(world, &ran, &invocation);
    refused("a successful no-op", || {
        the_effect_is_confirmed_by_reading_it_back(world, &paused, &invocation, &ran);
    });
    world.machine_is_deaf(false);
    world.wants(Reports::Printing);
}

/// The no-op left behind everything a check of the record alone looks at.
///
/// Both events are there, about this very action, carrying what the caller
/// asked for — so the refusal above is the state assertion doing the work
/// rather than the record having gone missing. That is what makes this a
/// **successful** no-op rather than a failure wearing one's clothes.
fn the_record_a_weaker_check_would_have_accepted_is_there(
    world: &World,
    ran: &Ran,
    invocation: &crate::walk::Invocation,
) {
    assert_eq!(
        ran.code,
        Some(0),
        "the no-op did not succeed, so it is not the violation this is about: {}",
        ran.said()
    );
    let action_id = super::answers::at(
        &super::answers::answered(ran, invocation.machine_readable),
        "record.id",
    );
    let read = running::read(
        world,
        &["history", "--print-id", &world.print_id, "--limit", "40"],
    );
    for kind in ["action_requested", "action_executed"] {
        assert!(
            read.get("events")
                .and_then(Value::as_array)
                .is_some_and(|events| events.iter().any(|event| {
                    event.get("kind").and_then(Value::as_str) == Some(kind)
                        && event.pointer("/payload/action_id").and_then(Value::as_str)
                            == Some(action_id.as_str())
                })),
            "the no-op left no `{kind}` for this action, so a check of the record alone \
             would have refused it for the wrong reason: {read}"
        );
    }
}

/// The restoration assertion refuses a machine that never put the value back.
///
/// The adjustment lands on a machine that honours it, and the machine goes deaf
/// before the expiry sweep reaches it — so the restoring call is answered with
/// success, the record says `restored`, and the machine is still holding the
/// adjusted value. That is exactly what a check of the record alone would
/// accept and what a check of the value refuses.
fn the_restoration_assertion_refuses_an_incorrect_restoration(world: &World) {
    if !world.machine_is_deaf(false) {
        return;
    }
    let heater = walk::walk(world)
        .into_iter()
        .find(|found| found.command.name == "set-bed-target-c")
        .expect("this walk drives a bed target");

    let opened = durations::each_asks_for(world, std::slice::from_ref(&heater), SHORT);
    durations::the_adjusted_value_is_in_place_shortly_before_it_expires(world, &opened);
    world.machine_is_deaf(true);
    refused("an incorrect restoration", || {
        durations::the_prior_value_is_back_shortly_after_it_expires(world, &opened);
    });
    world.machine_is_deaf(false);
}

/// How long the adjustment the restoration defect is driven over stands for.
const SHORT: i64 = 2;

/// The command of the walk every defect is driven over.
fn the_command(world: &World) -> Driven {
    walk::walk(world)
        .into_iter()
        .find(|one| one.command.name == OVER)
        .unwrap_or_else(|| panic!("this walk drives no `{OVER}`"))
}

/// The environment one defect runs under.
fn under(world: &World, defect: &str) -> Vec<(String, String)> {
    let mut given = world.environment(crate::world::CREDENTIAL);
    given.push(("PRINTOBSERVER_TAINT".to_owned(), defect.to_owned()));
    given.push((
        "PRINTOBSERVER_TAINT_ELSEWHERE".to_owned(),
        world.elsewhere(),
    ));
    given.push((
        "PRINTOBSERVER_TAINT_IMAGE".to_owned(),
        world.image_path().display().to_string(),
    ));
    given.push(("PRINTOBSERVER_TAINT_FIELD".to_owned(), "percent".to_owned()));
    given.push(("PRINTOBSERVER_TAINT_AS".to_owned(), "99".to_owned()));
    given
}

/// One run of the variant carrying one defect.
fn run(world: &World, one: &Driven, defect: &str, also: &[String]) -> Ran {
    let mut arguments = super::failures::succeeding(one);
    arguments.extend_from_slice(also);
    world.proxy.forget();
    traced(
        &variant(),
        &arguments,
        &under(world, defect),
        &running::traces(world),
    )
}

/// The endpoint assertion refuses a variant that connects somewhere else.
fn the_endpoint_assertion_refuses(world: &World, one: &Driven, defect: &str, also: &[String]) {
    let ran = run(world, one, defect, also);
    refused(defect, || {
        only_the_configured_endpoint(world.proxy.address, &ran);
    });
    // The same variant under the other combination is what makes the second
    // defect the one only a cross-product catches: it connects to nothing there.
    if defect == "connects-when-combined" {
        let elsewhere = run(world, one, defect, &[]);
        only_the_configured_endpoint(world.proxy.address, &elsewhere);
    }
}

/// The request assertion refuses a variant that substitutes a value.
fn the_request_assertion_refuses(world: &World, one: &Driven, defect: &str) {
    let _ = run(world, one, defect, &[]);
    let received = world.proxy.the_one_request();
    refused(defect, || {
        the_request_carries_the_callers_values(one, &received);
    });
}

/// The output assertion refuses a variant that puts an image's bytes in it.
fn the_output_assertion_refuses(world: &World, one: &Driven, defect: &str) {
    let ran = run(world, one, defect, &["--json".to_owned()]);
    let answered = world.proxy.the_one_request().answered;
    let response: Value = serde_json::from_str(&answered).expect("the supervisor's own answer");
    let printed: Value = serde_json::from_str(&ran.out)
        .unwrap_or_else(|error| panic!("`{defect}` printed {error}: {}", ran.out));
    refused(defect, || {
        assert_eq!(
            fields(&printed),
            fields(&response),
            "a variant putting an image's bytes in its output was not refused"
        );
    });
}

/// One assertion, driven over one defect, which it has to refuse.
fn refused(defect: &str, asserting: impl FnOnce()) {
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let held = catch_unwind(AssertUnwindSafe(asserting));
    std::panic::set_hook(quiet);
    assert!(
        held.is_err(),
        "the assertion this tier makes accepted a variant carrying `{defect}`, so it says \
         nothing about the program it is meant to be about"
    );
}
