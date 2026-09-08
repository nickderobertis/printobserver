//! Every action passes one decision, and that is structural.
//!
//! The property is proven in four parts, no one of which is sufficient alone.
//!
//! **By reference.** Every syntactic reference this crate makes to an action
//! method of the printer port — a call, a path taken as a value, a function
//! pointer, an alias — lies inside one named function, and exactly one function
//! claims that role. Enumerating references rather than call sites is what
//! reaches the alias and the function pointer, each of which is an action call
//! a call-site walk could not see.
//!
//! **By reachability.** The printer handle is private to that one module, so an
//! item elsewhere in this crate has nothing to call an action method *on*.
//! These two close different holes and neither closes the other's: the first
//! reaches an item that names the method, the second an item that holds the
//! object, and an indirect dispatch needs both.
//!
//! **By execution.** This crate's tests drive core against exactly one
//! printer-port double, which fails the test on any action call a recorded
//! decision did not precede — so every path any test drives is covered whatever
//! a source check can resolve. That assertion is made load-bearing here by
//! driving an action call through the fake with no decision recorded.
//!
//! **By vocabulary.** Every variant of the action vocabulary is driven through
//! the real core; that part lives beside the walk it belongs to.
//!
//! What these establish is a bound rather than a universal. A route assembled
//! entirely outside this crate — core handing the handle to another crate which
//! calls an action method itself — presents no reference here to refuse and no
//! driven path to guard. That residue is bounded rather than checked: handing
//! the handle out from anywhere is itself a finding of the second part, and
//! this crate's manifest admits only the type crate and the four ports, none of
//! which calls a port it declares.

use crate::fakes::{FakePrinter, PrinterMethod};
use crate::journal::Journal;
use crate::source::{
    Module, chokepoint_findings, crate_dir, crate_modules, crate_test_sources, handle_findings,
    impls_of_trait, parse, read, trait_method_names,
};
use std::sync::Arc;

/// The crate whose sources these checks read.
const CRATE: &str = "printobserver-core";

/// The one function every action this crate takes at the printer is issued from.
const CHOKEPOINT: &str = "issue_decided_action";

/// The module that function, and the printer handle, live in.
const CHOKEPOINT_MODULE: &str = "supervisor";

/// The trait naming the printer handle.
const HANDLE: &str = "PrinterPort";

/// Every method the printer port declares, read off the contracts themselves.
fn declared_methods() -> Vec<String> {
    let path = crate_dir("printobserver-printer-api")
        .join("src")
        .join("lib.rs");
    trait_method_names(&parse(&read(&path)), HANDLE)
}

/// Every **action** method: the whole trait but its two reads.
///
/// The reads are outside the rule because the loop reads the printer without
/// taking a decision, and a rule demanding one before them would demand a loop
/// this crate must not have.
fn action_methods() -> Vec<String> {
    declared_methods()
        .into_iter()
        .filter(|method| method != "snapshot" && method != "job")
        .collect()
}

/// The committed tree's own modules.
fn committed() -> Vec<Module> {
    crate_modules(CRATE)
}

/// The committed modules with one module's source replaced or added.
fn with(modules: &[Module], name: &str, source: &str) -> Vec<Module> {
    let mut copy: Vec<Module> = modules.to_vec();
    if let Some(existing) = copy.iter_mut().find(|(held, _)| held == name) {
        existing.1 = source.to_owned();
    } else {
        copy.push((name.to_owned(), source.to_owned()));
    }
    copy
}

/// One module's committed source.
fn source_of(modules: &[Module], name: &str) -> String {
    modules
        .iter()
        .find(|(held, _)| held == name)
        .map(|(_, source)| source.clone())
        .unwrap_or_else(|| panic!("the tree carries a `{name}` module"))
}

/// The committed modules with one module's source extended.
fn extended(modules: &[Module], name: &str, addition: &str) -> Vec<Module> {
    let source = format!("{}\n{addition}\n", source_of(modules, name));
    with(modules, name, &source)
}

/// Assert a fixture is refused, and refused for the reason it was built for.
///
/// A fixture refused for some other reason proves nothing about the property,
/// so every one of these names the finding it expects.
#[track_caller]
fn refused(findings: &[String], expected: &str) {
    assert!(
        findings.iter().any(|finding| finding.contains(expected)),
        "expected a finding naming {expected:?}, found {findings:?}"
    );
}

/// The printer port's own trait names the methods this crate is checked against.
#[test]
fn the_action_methods_are_read_off_the_port_rather_than_a_list_here() {
    let declared = declared_methods();
    assert!(
        declared.contains(&"snapshot".to_owned()) && declared.contains(&"job".to_owned()),
        "the port declares its two reads: {declared:?}"
    );
    assert_eq!(declared.len(), action_methods().len() + 2);
    let walked: Vec<String> = PrinterMethod::ALL
        .iter()
        .map(|method| method.name().to_owned())
        .collect();
    assert_eq!(
        walked, declared,
        "the walk over the port's methods has fallen behind the port"
    );
}

/// The committed tree issues every action from the one named function.
#[test]
fn the_committed_tree_issues_every_action_from_one_function() {
    assert_eq!(
        chokepoint_findings(&committed(), CHOKEPOINT, &action_methods()),
        Vec::<String>::new()
    );
}

/// A free function in the chokepoint's own module is refused.
#[test]
fn a_free_function_in_the_chokepoints_own_module_is_refused() {
    let tree = extended(
        &committed(),
        CHOKEPOINT_MODULE,
        "async fn nudge(printer: &Arc<dyn PrinterPort>) -> Result<(), PrinterError> { \
         printer.pause().await }",
    );
    refused(&chokepoint_findings(&tree, CHOKEPOINT, &action_methods()), "`supervisor` references");
}

/// A method on a type of this crate is refused.
#[test]
fn a_method_on_a_type_of_this_crate_is_refused() {
    let tree = extended(
        &committed(),
        CHOKEPOINT_MODULE,
        "struct Nudger { printer: Arc<dyn PrinterPort> } \
         impl Nudger { async fn go(&self) { let _ = self.printer.resume().await; } }",
    );
    refused(&chokepoint_findings(&tree, CHOKEPOINT, &action_methods()), "`supervisor` references");
}

/// A call site in another module of this crate is refused.
#[test]
fn a_call_site_in_another_module_is_refused() {
    let tree = extended(
        &committed(),
        "actions",
        "async fn stop(printer: &Arc<dyn PrinterPort>) { let _ = printer.cancel().await; }",
    );
    refused(&chokepoint_findings(&tree, CHOKEPOINT, &action_methods()), "`actions` references");
}

/// A module taking an action method as a function pointer is refused.
#[test]
fn an_action_method_taken_as_a_function_pointer_is_refused() {
    let tree = with(
        &committed(),
        "dispatch",
        "use printobserver_printer_api::PrinterPort; \
         pub fn dispatch(port: &dyn PrinterPort) { \
         let held = PrinterPort::set_fan_percent; held(port, 100.0); }",
    );
    refused(&chokepoint_findings(&tree, CHOKEPOINT, &action_methods()), "`dispatch` references");
}

/// A module aliasing an action method under another name is refused.
#[test]
fn an_action_method_aliased_under_another_name_is_refused() {
    let tree = with(
        &committed(),
        "aliased",
        "use printobserver_printer_api::PrinterPort; \
         const HALT: fn(&dyn PrinterPort) -> BoxFuture<'_, Result<(), PrinterError>> = \
         PrinterPort::cancel; \
         pub fn halt(port: &dyn PrinterPort) { let _ = HALT(port); }",
    );
    refused(&chokepoint_findings(&tree, CHOKEPOINT, &action_methods()), "`aliased` references");
}

/// A tree in which no function claims the chokepoint role is refused.
#[test]
fn a_tree_with_no_chokepoint_is_refused() {
    let modules = committed();
    let renamed = source_of(&modules, CHOKEPOINT_MODULE).replace(CHOKEPOINT, "issue_anywhere");
    let tree = with(&modules, CHOKEPOINT_MODULE, &renamed);
    refused(&chokepoint_findings(&tree, CHOKEPOINT, &action_methods()), "0 functions claim the chokepoint role");
}

/// A tree in which two functions claim the chokepoint role is refused.
#[test]
fn a_tree_with_two_chokepoints_is_refused() {
    let tree = extended(
        &committed(),
        "actions",
        &format!("async fn {CHOKEPOINT}() {{ }}"),
    );
    refused(&chokepoint_findings(&tree, CHOKEPOINT, &action_methods()), "2 functions claim the chokepoint role");
}

/// The committed tree keeps the printer handle inside the chokepoint's module.
#[test]
fn the_committed_tree_keeps_the_handle_inside_the_chokepoints_module() {
    assert_eq!(
        handle_findings(&committed(), CHOKEPOINT_MODULE, HANDLE),
        Vec::<String>::new()
    );
}

/// Another module holding the handle in a field of its own is refused.
#[test]
fn another_module_holding_the_handle_is_refused() {
    let tree = extended(
        &committed(),
        "events",
        "struct Held { printer: Arc<dyn PrinterPort> }",
    );
    refused(&handle_findings(&tree, CHOKEPOINT_MODULE, HANDLE), "`events` names `PrinterPort`");
}

/// A public accessor handing the handle out of that module is refused.
#[test]
fn a_public_accessor_handing_the_handle_out_is_refused() {
    let tree = extended(
        &committed(),
        CHOKEPOINT_MODULE,
        "impl Supervisor { pub fn printer(&self) -> &Arc<dyn PrinterPort> { &self.printer } }",
    );
    refused(&handle_findings(&tree, CHOKEPOINT_MODULE, HANDLE), "hands `PrinterPort` back out");
}

/// Passing the handle out of that module as an argument is refused.
#[test]
fn passing_the_handle_out_as_an_argument_is_refused() {
    let tree = extended(
        &committed(),
        "expiry",
        "fn take(printer: Arc<dyn PrinterPort>) { let _ = printer; }",
    );
    refused(&handle_findings(&tree, CHOKEPOINT_MODULE, HANDLE), "`expiry` names `PrinterPort`");
}

/// This crate's tests declare exactly one implementation of the printer port.
#[test]
fn this_crates_tests_declare_exactly_one_printer_double() {
    let declared: usize = crate_test_sources(CRATE)
        .iter()
        .map(|(_, source)| impls_of_trait(&parse(source), HANDLE))
        .sum();
    assert_eq!(declared, 1, "this crate's tests declare {declared} printer doubles");
}

/// A tree whose tests declare a second printer double is refused.
#[test]
fn a_second_printer_double_in_the_tests_is_refused() {
    let fixture = "struct One; impl PrinterPort for One {} struct Two; impl PrinterPort for Two {}";
    assert_eq!(impls_of_trait(&parse(fixture), HANDLE), 2);
}

/// The fake's own assertion is load-bearing: an undecided call fails the test.
///
/// The fixture is a core built to issue an action call with no decision
/// recorded — which is exactly an action method reached with no licence — and
/// what it establishes is that the assertion every other journey relies on
/// would have caught such a call.
#[test]
fn an_action_call_with_no_recorded_decision_fails_the_test() {
    let journal = Arc::new(Journal::default());
    let printer = FakePrinter::new(Arc::clone(&journal));
    let port: &dyn printobserver_printer_api::PrinterPort = &printer;

    assert_eq!(printobserver_core::block_on(port.pause()), Ok(()));

    assert_eq!(journal.violations().len(), 1);
    assert!(journal.violations()[0].contains("pause"));
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        journal.assert_no_violations();
    }));
    assert!(
        refused.is_err(),
        "the fake accepted an action call with no decision recorded before it"
    );
}
