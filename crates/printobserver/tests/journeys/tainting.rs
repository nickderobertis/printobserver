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

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use printobserver::render::fields;
use printobserver_types::serde_json::{self, Value};

use crate::machine::Reports;
use crate::traced::{Ran, traced};
use crate::walk::{self, Driven};
use crate::world::World;

use super::confirming::{only_the_configured_endpoint, the_request_carries_the_callers_values};
use super::running;

/// The variant of this program that carries one defect.
fn variant() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_printobserver-tainted"))
}

/// The command every defect here is driven over.
const OVER: &str = "set-fan-percent";

/// Drive every assertion over the defect it is about, and refuse each.
pub fn every_assertion_here_refuses_the_defect_it_is_about(world: &World) {
    world.machine.reports(Reports::Printing);
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
}

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
        world.machine.address.to_string(),
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
