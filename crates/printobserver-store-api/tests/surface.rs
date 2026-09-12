//! The store port declares exactly the surface the contract states.
//!
//! These tests read this crate's own declarations rather than a table
//! maintained beside them, and every fixture they are driven against is a
//! source snippet read by the same reader. The reader is the type crate's
//! `tests/support/surface.rs`, included here by `#[path]`: a port crate
//! declares the type crate as its only workspace dependency, and a reader of
//! Rust sources is a claim about no crate in particular.

#[path = "../../printobserver-types/tests/support/surface.rs"]
pub mod surface;

use surface::{
    Field, Method, Param, Variant, crate_dir, crate_source, enum_variants, parse, public_constants,
    public_functions, trait_method_docs, trait_methods, workspace_dependency_names,
};

/// This crate's own name.
const CRATE: &str = "printobserver-store-api";

/// The method the contract states, as a name, its parameters and its answer.
fn method(name: &str, params: &[(&str, &str)], returns: &str) -> Method {
    Method {
        name: name.to_owned(),
        params: params
            .iter()
            .map(|(name, ty)| Param {
                name: (*name).to_owned(),
                ty: (*ty).to_owned(),
            })
            .collect(),
        returns: returns.to_owned(),
    }
}

/// The variant the contract states, as a name and its payload's fields.
fn variant(name: &str, fields: &[(&str, &str)]) -> Variant {
    Variant {
        name: name.to_owned(),
        fields: fields
            .iter()
            .map(|(name, ty)| Field {
                name: (*name).to_owned(),
                ty: (*ty).to_owned(),
            })
            .collect(),
    }
}

/// The store port's stated methods over prints, events, images and actions.
fn stated_store_methods_through_actions() -> Vec<Method> {
    vec![
        method(
            "open_print",
            &[
                ("obico_print_id", "Option<i64>"),
                ("file_name", "Option<String>"),
            ],
            "BoxFuture<'_,Result<PrintRecord,StoreError>>",
        ),
        method(
            "print",
            &[("print_id", "PrintId")],
            "BoxFuture<'_,Result<Option<PrintRecord>,StoreError>>",
        ),
        method(
            "open_prints",
            &[],
            "BoxFuture<'_,Result<Vec<PrintRecord>,StoreError>>",
        ),
        method(
            "print_by_obico_id",
            &[("obico_print_id", "i64")],
            "BoxFuture<'_,Result<Option<PrintRecord>,StoreError>>",
        ),
        method(
            "end_print",
            &[
                ("print_id", "PrintId"),
                ("state", "PrinterState"),
                ("ended_at", "Timestamp"),
                ("reason", "String"),
            ],
            "BoxFuture<'_,Result<PrintRecord,StoreError>>",
        ),
        method(
            "record_narrowing",
            &[("print_id", "PrintId"), ("narrowing", "ManifestNarrowing")],
            "BoxFuture<'_,Result<PrintRecord,StoreError>>",
        ),
        method(
            "append_event",
            &[("draft", "EventDraft")],
            "BoxFuture<'_,Result<EventRecord,StoreError>>",
        ),
        method(
            "put_image",
            &[
                ("print_id", "PrintId"),
                ("event_id", "EventId"),
                ("source_url", "Option<String>"),
                ("content_type", "String"),
                ("bytes", "RawBytes"),
            ],
            "BoxFuture<'_,Result<ImageRecord,StoreError>>",
        ),
        method(
            "image",
            &[("image_id", "ImageId")],
            "BoxFuture<'_,Result<ImageLookup,StoreError>>",
        ),
        method(
            "record_action",
            &[("request", "ActionRequest"), ("decision", "PolicyDecision")],
            "BoxFuture<'_,Result<ActionRecord,StoreError>>",
        ),
        method(
            "record_execution",
            &[("action_id", "ActionId"), ("outcome", "ExecutionOutcome")],
            "BoxFuture<'_,Result<ActionRecord,StoreError>>",
        ),
    ]
}

/// The store port's stated methods over interventions, manifests and history.
fn stated_store_methods_from_interventions() -> Vec<Method> {
    vec![
        method(
            "open_intervention",
            &[
                ("action_id", "ActionId"),
                ("adjustable", "Adjustable"),
                ("prior_value", "Option<f64>"),
                ("applied_value", "f64"),
                ("applied_at", "Timestamp"),
                ("expires_at", "Timestamp"),
            ],
            "BoxFuture<'_,Result<Intervention,StoreError>>",
        ),
        method(
            "settle_intervention",
            &[
                ("intervention_id", "InterventionId"),
                ("outcome", "InterventionOutcome"),
            ],
            "BoxFuture<'_,Result<SettleOutcome,StoreError>>",
        ),
        method(
            "due_interventions",
            &[("at", "Timestamp")],
            "BoxFuture<'_,Result<Vec<Intervention>,StoreError>>",
        ),
        method(
            "active_interventions",
            &[("print_id", "PrintId")],
            "BoxFuture<'_,Result<Vec<Intervention>,StoreError>>",
        ),
        method(
            "put_manifest",
            &[("print_id", "PrintId"), ("manifest", "JobManifest")],
            "BoxFuture<'_,Result<(),StoreError>>",
        ),
        method(
            "manifest",
            &[("print_id", "PrintId")],
            "BoxFuture<'_,Result<Option<JobManifest>,StoreError>>",
        ),
        method(
            "history",
            &[("query", "HistoryQuery")],
            "BoxFuture<'_,Result<Vec<EventRecord>,StoreError>>",
        ),
        method(
            "audit_page",
            &[
                ("print_id", "PrintId"),
                ("after", "Option<EventId>"),
                ("page_size", "u32"),
            ],
            "BoxFuture<'_,Result<AuditPage,StoreError>>",
        ),
        method(
            "put_session",
            &[("session", "SupervisionSession")],
            "BoxFuture<'_,Result<(),StoreError>>",
        ),
        method(
            "session",
            &[("print_id", "PrintId")],
            "BoxFuture<'_,Result<Option<SupervisionSession>,StoreError>>",
        ),
    ]
}

/// Every method the store port is stated to declare, and no other.
#[test]
fn store_port_declares_exactly_the_stated_methods() {
    let source = parse(&crate_source(CRATE));
    let mut expected = stated_store_methods_through_actions();
    expected.extend(stated_store_methods_from_interventions());
    assert_eq!(trait_methods(&source, "StorePort"), expected);
}

/// Every variant the store port's error vocabulary is stated to carry.
#[test]
fn store_error_carries_exactly_the_stated_variants() {
    let source = parse(&crate_source(CRATE));
    let expected = vec![
        variant("ConstraintRefused", &[("constraint", "String")]),
        variant("NotFound", &[("what", "String")]),
        variant("LimitRefused", &[("limit", "u32"), ("asked_for", "u32")]),
        variant("Database", &[("detail", "String")]),
        variant("Io", &[("detail", "String")]),
    ];
    assert_eq!(enum_variants(&source, "StoreError"), expected);
}

/// Every port method is asynchronous, in the one shape a trait object carries.
#[test]
fn every_port_method_answers_a_boxed_future() {
    let source = parse(&crate_source(CRATE));
    let methods = trait_methods(&source, "StorePort");
    assert!(!methods.is_empty(), "StorePort declares no method");
    for declared in methods {
        assert!(
            declared.returns.starts_with("BoxFuture<'_,Result<"),
            "StorePort::{} answers {}, which is neither asynchronous nor a Result",
            declared.name,
            declared.returns
        );
    }
}

/// The port's error vocabulary carries no not-yet-implemented variant.
#[test]
fn the_port_error_carries_no_not_yet_implemented_variant() {
    let source = parse(&crate_source(CRATE));
    for declared in enum_variants(&source, "StoreError") {
        let name = declared.name.to_lowercase();
        assert!(
            !(name.contains("notimplemented")
                || name.contains("unimplemented")
                || name.contains("todo")),
            "StoreError carries {}, a variant no real implementation can produce",
            declared.name
        );
    }
}

/// This crate names the type crate as its only workspace dependency, across
/// every dependency table: a port that named an implementation would stop
/// being a port.
#[test]
fn the_manifest_names_the_type_crate_and_no_other_workspace_crate() {
    let manifest = std::fs::read_to_string(crate_dir(CRATE).join("Cargo.toml"))
        .expect("the manifest is readable");
    assert_eq!(
        workspace_dependency_names(&manifest),
        vec!["printobserver-types".to_owned()]
    );
}

/// Which store methods take either half of the action pair.
fn action_pair_partition(source: &syn::File, trait_name: &str) -> (Vec<Method>, Vec<Method>) {
    trait_methods(source, trait_name)
        .into_iter()
        .partition(|declared| {
            declared.params.iter().any(|param| {
                param.ty.contains("ActionRequest") || param.ty.contains("PolicyDecision")
            })
        })
}

/// `record_action` is the only method taking either half, and it takes both.
#[test]
fn record_action_is_the_only_method_taking_the_action_pair() {
    let source = parse(&crate_source(CRATE));
    let (taking, rest) = action_pair_partition(&source, "StorePort");
    assert_eq!(
        taking.len(),
        1,
        "these methods take a half of the pair: {taking:?}"
    );
    let recorder = &taking[0];
    assert_eq!(recorder.name, "record_action");
    assert_eq!(
        recorder
            .params
            .iter()
            .map(|param| param.ty.as_str())
            .collect::<Vec<_>>(),
        vec!["ActionRequest", "PolicyDecision"],
        "record_action does not take the request and the decision together"
    );
    assert!(
        recorder.returns.contains("ActionRecord"),
        "record_action answers {}, which carries no minted identifier",
        recorder.returns
    );
    assert!(
        !rest.is_empty()
            && rest
                .iter()
                .any(|declared| declared.returns.contains("EventRecord")),
        "the rest of the surface should still carry identifier-bearing answers"
    );
}

/// A fixture declaring a second method that takes an `ActionRequest`.
const FIXTURE_SECOND_REQUEST: &str = r"
pub trait Fixture {
    fn record_action(&self, request: ActionRequest, decision: PolicyDecision)
        -> BoxFuture<'_, Result<ActionRecord, StoreError>>;
    fn replay(&self, request: ActionRequest) -> BoxFuture<'_, Result<ActionRecord, StoreError>>;
}
";

/// A fixture declaring a method that takes a `PolicyDecision` alone.
const FIXTURE_DECISION_ALONE: &str = r"
pub trait Fixture {
    fn record_decision(&self, decision: PolicyDecision)
        -> BoxFuture<'_, Result<ActionRecord, StoreError>>;
}
";

/// A fixture whose `record_action` takes the request without the decision.
const FIXTURE_REQUEST_WITHOUT_DECISION: &str = r"
pub trait Fixture {
    fn record_action(&self, request: ActionRequest)
        -> BoxFuture<'_, Result<ActionRecord, StoreError>>;
}
";

/// A fixture whose `record_action` answers nothing carrying an `ActionId`.
const FIXTURE_NO_ACTION_ID: &str = r"
pub trait Fixture {
    fn record_action(&self, request: ActionRequest, decision: PolicyDecision)
        -> BoxFuture<'_, Result<(), StoreError>>;
}
";

/// The partition refuses each fixture the contract names.
#[test]
fn the_action_pair_partition_refuses_every_fixture_it_must() {
    let fixture = parse(FIXTURE_SECOND_REQUEST);
    let (taking, _) = action_pair_partition(&fixture, "Fixture");
    assert_eq!(
        taking.len(),
        2,
        "a second method taking an ActionRequest was not seen"
    );

    let fixture = parse(FIXTURE_DECISION_ALONE);
    let (taking, _) = action_pair_partition(&fixture, "Fixture");
    assert_eq!(taking.len(), 1);
    assert_ne!(
        taking[0].name, "record_action",
        "a decision-only method was read as the recorder"
    );

    let fixture = parse(FIXTURE_REQUEST_WITHOUT_DECISION);
    let (taking, _) = action_pair_partition(&fixture, "Fixture");
    assert_eq!(
        taking[0]
            .params
            .iter()
            .map(|param| param.ty.as_str())
            .collect::<Vec<_>>(),
        vec!["ActionRequest"],
        "a record_action without the decision was read as taking the pair"
    );

    let fixture = parse(FIXTURE_NO_ACTION_ID);
    let (taking, _) = action_pair_partition(&fixture, "Fixture");
    assert!(
        !taking[0].returns.contains("ActionRecord"),
        "a record_action answering nothing was read as answering a record"
    );
}

/// The constants a consumer could import as the default history window.
fn default_window_constants(source: &syn::File) -> Vec<Field> {
    public_constants(source)
        .into_iter()
        .filter(|entry| entry.name.contains("DEFAULT") && entry.name.contains("WINDOW"))
        .collect()
}

/// The constants a consumer could import as the maximum history limit.
fn maximum_limit_constants(source: &syn::File) -> Vec<Field> {
    public_constants(source)
        .into_iter()
        .filter(|entry| entry.name.contains("MAX") && entry.name.contains("LIMIT"))
        .collect()
}

/// The functions a consumer could import as the resolution of the limit rule.
fn limit_resolutions(source: &syn::File) -> Vec<Method> {
    public_functions(source)
        .into_iter()
        .filter(|declared| {
            declared
                .params
                .iter()
                .any(|param| param.ty == "Option<u32>")
                && declared.returns == "Result<u32,StoreError>"
        })
        .collect()
}

/// A consumer importing the store port finds one answer of each, not two.
#[test]
fn the_store_port_exports_one_of_each_history_answer() {
    let source = parse(&crate_source(CRATE));
    assert_eq!(
        default_window_constants(&source)
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>(),
        vec!["DEFAULT_HISTORY_WINDOW".to_owned()]
    );
    assert_eq!(
        maximum_limit_constants(&source)
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>(),
        vec!["MAX_HISTORY_LIMIT".to_owned()]
    );
    assert_eq!(
        limit_resolutions(&source)
            .into_iter()
            .map(|declared| declared.name)
            .collect::<Vec<_>>(),
        vec!["resolve_history_limit".to_owned()]
    );
}

/// A fixture crate exporting a second constant of either kind.
const FIXTURE_SECOND_CONSTANTS: &str = r"
pub const DEFAULT_HISTORY_WINDOW: u32 = 100;
pub const DEFAULT_AUDIT_WINDOW: u32 = 50;
pub const MAX_HISTORY_LIMIT: u32 = 1_000;
pub const MAX_AUDIT_LIMIT: u32 = 500;
";

/// A fixture crate exporting a second resolution of the limit rule.
const FIXTURE_SECOND_RESOLUTION: &str = r"
pub fn resolve_history_limit(limit: Option<u32>) -> Result<u32, StoreError> {
    Ok(0)
}
pub fn clamp_history_limit(limit: Option<u32>) -> Result<u32, StoreError> {
    Ok(0)
}
";

/// The exported-surface reading refuses a crate offering two answers.
#[test]
fn the_exported_surface_reading_refuses_a_second_answer() {
    let fixture = parse(FIXTURE_SECOND_CONSTANTS);
    assert_eq!(
        default_window_constants(&fixture).len(),
        2,
        "a second window was not seen"
    );
    assert_eq!(
        maximum_limit_constants(&fixture).len(),
        2,
        "a second maximum was not seen"
    );

    let fixture = parse(FIXTURE_SECOND_RESOLUTION);
    assert_eq!(
        limit_resolutions(&fixture).len(),
        2,
        "a second resolution was not seen"
    );
}

/// The default window is below the maximum limit.
#[test]
fn the_default_window_is_below_the_maximum_limit() {
    let source = crate_source(CRATE);
    assert!(
        constant_value(&source, "DEFAULT_HISTORY_WINDOW")
            < constant_value(&source, "MAX_HISTORY_LIMIT"),
        "the default window is not below the maximum limit"
    );
}

/// The value one declared constant carries.
fn constant_value(source: &str, name: &str) -> u32 {
    let needle = format!("pub const {name}: u32 = ");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("{name} is declared"))
        + needle.len();
    let rest = &source[start..];
    let end = rest.find(';').expect("the declaration ends");
    rest[..end]
        .replace('_', "")
        .parse()
        .expect("the declaration carries a number")
}

/// The `history` documentation states the ordering and the empty-kinds meaning.
#[test]
fn the_history_documentation_states_its_two_unvalued_facts() {
    let source = parse(&crate_source(CRATE));
    let documentation = trait_method_docs(&source, "StorePort", "history").to_lowercase();
    assert!(
        documentation.contains("newest first"),
        "the history documentation does not state the ordering: {documentation}"
    );
    assert!(
        documentation.contains("empty `kinds` means **every kind**")
            || documentation.contains("empty `kinds` means every kind"),
        "the history documentation does not state what an empty kinds means: {documentation}"
    );
}

/// A fixture whose `history` states neither fact.
const FIXTURE_UNDOCUMENTED_HISTORY: &str = r"
pub trait Fixture {
    /// Read a print's events.
    fn history(&self, query: HistoryQuery) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>>;
}
";

/// The documentation reading refuses a `history` that states neither fact.
#[test]
fn the_history_documentation_reading_refuses_an_undocumented_history() {
    let fixture = parse(FIXTURE_UNDOCUMENTED_HISTORY);
    let documentation = trait_method_docs(&fixture, "Fixture", "history").to_lowercase();
    assert!(
        !documentation.is_empty(),
        "the reader found no documentation at all"
    );
    assert!(!documentation.contains("newest first"));
    assert!(!documentation.contains("every kind"));
}
