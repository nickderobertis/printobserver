//! The conformance suite exercises every method the store traits declare.
//!
//! A suite that covers most of the traits is a suite whose gaps nobody can
//! name, so this reads the traits' own declarations and the suite's own calls
//! rather than a list maintained beside either — and it is driven against a
//! fixture trait carrying a method the suite does not reach, so that what
//! refuses the fixture is what reads the committed pair.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::contracts::{port_source, store_trait_methods};
use crate::surface::{method_calls, parse, read, trait_methods};

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

/// Every method the traits declare that the suite never calls.
fn unexercised(traits: &[String], suite: &[syn::File]) -> Vec<String> {
    let called: BTreeSet<String> = suite.iter().flat_map(method_calls).collect();
    traits
        .iter()
        .filter(|name| !called.contains(*name))
        .cloned()
        .collect()
}

/// The suite exercises every method every store trait declares.
#[test]
fn the_conformance_suite_exercises_every_method_the_traits_declare() {
    let declared: Vec<String> = store_trait_methods(&port_source())
        .into_iter()
        .map(|method| method.name)
        .collect();
    assert!(
        declared.len() >= 20,
        "the reader found only {} store methods, which is not the store",
        declared.len()
    );
    assert_eq!(
        unexercised(&declared, &suite()),
        Vec::<String>::new(),
        "the conformance suite does not exercise every method the traits declare"
    );
}

/// A trait carrying a method the suite does not reach.
const FIXTURE_UNEXERCISED_TRAIT: &str = r"
pub trait EventStore: Send + Sync {
    fn print(&self, print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>>;
    fn whole_history(&self, print_id: PrintId) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>>;
}
";

/// The check refuses a trait method the suite does not reach.
#[test]
fn the_check_refuses_a_trait_method_the_suite_does_not_reach() {
    let fixture = parse(FIXTURE_UNEXERCISED_TRAIT);
    let declared: Vec<String> = trait_methods(&fixture, "EventStore")
        .into_iter()
        .map(|method| method.name)
        .collect();
    assert_eq!(
        unexercised(&declared, &suite()),
        vec!["whole_history".to_owned()],
        "the check did not name the method the suite never calls"
    );
}
