//! The conformance suite exercises every method the port declares.
//!
//! A suite that covers most of a port is a suite whose gaps nobody can name, so
//! this reads the port's own declarations and the suite's own calls rather than
//! a list maintained beside either — and it is driven against a fixture port
//! carrying a method the suite does not reach, so that what refuses the fixture
//! is what reads the committed pair.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::surface::{crate_dir, method_calls, parse, read, trait_methods};

/// The files the conformance suite is written across.
const SUITE_FILES: [&str; 2] = ["tests/journeys/conformance.rs", "tests/support/fixture.rs"];

/// The suite, parsed.
fn suite() -> Vec<syn::File> {
    SUITE_FILES
        .iter()
        .map(|name| {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name);
            parse(&read(&path))
        })
        .collect()
}

/// The store port, as this repository's port crate declares it.
fn port() -> syn::File {
    parse(&read(
        &crate_dir("printobserver-store-api")
            .join("src")
            .join("lib.rs"),
    ))
}

/// Every method the port declares that the suite never calls.
fn unexercised(port: &syn::File, suite: &[syn::File]) -> Vec<String> {
    let called: BTreeSet<String> = suite.iter().flat_map(method_calls).collect();
    trait_methods(port, "StorePort")
        .into_iter()
        .map(|method| method.name)
        .filter(|name| !called.contains(name))
        .collect()
}

/// The suite exercises every method the store port declares.
#[test]
fn the_conformance_suite_exercises_every_method_the_port_declares() {
    let port = port();
    let declared = trait_methods(&port, "StorePort");
    assert!(
        declared.len() >= 20,
        "the reader found only {} port methods, which is not the port",
        declared.len()
    );
    assert_eq!(
        unexercised(&port, &suite()),
        Vec::<String>::new(),
        "the conformance suite does not exercise every method the port declares"
    );
}

/// A port carrying a method the suite does not reach.
const FIXTURE_UNEXERCISED_PORT: &str = r"
pub trait StorePort: Send + Sync {
    fn print(&self, print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>>;
    fn whole_history(&self, print_id: PrintId) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>>;
}
";

/// The check refuses a port method the suite does not reach.
#[test]
fn the_check_refuses_a_port_method_the_suite_does_not_reach() {
    let fixture = parse(FIXTURE_UNEXERCISED_PORT);
    assert_eq!(
        unexercised(&fixture, &suite()),
        vec!["whole_history".to_owned()],
        "the check did not name the method the suite never calls"
    );
}
