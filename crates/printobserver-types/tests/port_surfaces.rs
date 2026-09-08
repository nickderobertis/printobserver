//! The four port crates declare exactly the surface the contract states.
//!
//! These tests read the port crates' own declarations rather than a table
//! maintained beside them, and every fixture they are driven against is a
//! source snippet read by the same reader. They live here, in the crate the
//! four ports depend on, because a port crate declares the type crate as its
//! only dependency and so can carry no reader of its own.

#[path = "support/closure.rs"]
mod closure;
#[path = "support/surface.rs"]
mod surface;

use closure::Universe;
use surface::{
    Field, Method, Param, Variant, crate_source, crate_sources, declared_type_names, enum_variants,
    parse, public_constants, public_functions, trait_methods,
};

/// The method the contract states, as a name, its parameters and its answer.
fn method(name: &str, params: &[(&str, &str)], returns: &str) -> Method {
    Method {
        name: name.to_owned(),
        params: params
            .iter()
            .map(|(name, ty)| Param { name: (*name).to_owned(), ty: (*ty).to_owned() })
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
            .map(|(name, ty)| Field { name: (*name).to_owned(), ty: (*ty).to_owned() })
            .collect(),
    }
}

/// Every method the printer port is stated to declare, and no other.
#[test]
fn printer_port_declares_exactly_the_stated_methods() {
    let source = parse(&crate_source("printobserver-printer-api"));
    let expected = vec![
        method("snapshot", &[], "BoxFuture<'_,Result<PrinterSnapshot,PrinterError>>"),
        method("job", &[], "BoxFuture<'_,Result<JobSnapshot,PrinterError>>"),
        method("start", &[("file_name", "FileName")], "BoxFuture<'_,Result<(),PrinterError>>"),
        method("pause", &[], "BoxFuture<'_,Result<(),PrinterError>>"),
        method("resume", &[], "BoxFuture<'_,Result<(),PrinterError>>"),
        method("cancel", &[], "BoxFuture<'_,Result<(),PrinterError>>"),
        method(
            "set_feedrate_factor",
            &[("factor", "f64")],
            "BoxFuture<'_,Result<(),PrinterError>>",
        ),
        method(
            "set_flowrate_factor",
            &[("factor", "f64")],
            "BoxFuture<'_,Result<(),PrinterError>>",
        ),
        method(
            "set_tool_target_c",
            &[("tool", "i64"), ("target_c", "f64")],
            "BoxFuture<'_,Result<(),PrinterError>>",
        ),
        method("set_bed_target_c", &[("target_c", "f64")], "BoxFuture<'_,Result<(),PrinterError>>"),
        method("set_fan_percent", &[("percent", "f64")], "BoxFuture<'_,Result<(),PrinterError>>"),
    ];
    assert_eq!(trait_methods(&source, "PrinterPort"), expected);
}

/// Every variant the printer port's error vocabulary is stated to carry.
#[test]
fn printer_error_carries_exactly_the_stated_variants() {
    let source = parse(&crate_source("printobserver-printer-api"));
    let expected = vec![
        variant("Unreachable", &[("detail", "String")]),
        variant("Unauthorized", &[("detail", "String")]),
        variant("Refused", &[("status", "u16"), ("detail", "String")]),
        variant("StateConflict", &[("detail", "String")]),
        variant("Unsupported", &[("adjustable", "Adjustable")]),
        variant("Malformed", &[("detail", "String")]),
    ];
    assert_eq!(enum_variants(&source, "PrinterError"), expected);
}

/// Every method the vision port is stated to declare, and no other.
#[test]
fn vision_port_declares_exactly_the_stated_methods() {
    let source = parse(&crate_source("printobserver-vision-api"));
    let expected = vec![
        method(
            "normalize",
            &[("body", "RawBytes"), ("content_type", "Option<String>")],
            "BoxFuture<'_,Result<NormalizedAlert,VisionError>>",
        ),
        method(
            "fetch_image",
            &[("source_url", "String")],
            "BoxFuture<'_,Result<FetchedImage,VisionError>>",
        ),
    ];
    assert_eq!(trait_methods(&source, "VisionPort"), expected);
}

/// Every variant the vision port's error vocabulary is stated to carry.
#[test]
fn vision_error_carries_exactly_the_stated_variants() {
    let source = parse(&crate_source("printobserver-vision-api"));
    let expected = vec![
        variant("Malformed", &[("detail", "String"), ("raw", "RawBytes")]),
        variant("TimedOut", &[]),
        variant("TooLarge", &[("limit", "i64")]),
        variant("UnacceptableContentType", &[("content_type", "String")]),
        variant("Unreachable", &[("detail", "String")]),
    ];
    assert_eq!(enum_variants(&source, "VisionError"), expected);
}

/// Every method the supervisor port is stated to declare, and no other.
#[test]
fn supervisor_port_declares_exactly_the_stated_methods() {
    let source = parse(&crate_source("printobserver-supervisor-api"));
    let expected = vec![
        method(
            "run_turn",
            &[("request", "TurnRequest")],
            "BoxFuture<'_,Result<TurnOutcome,SupervisorError>>",
        ),
        method(
            "close_session",
            &[("print_id", "PrintId"), ("close_reason", "String")],
            "BoxFuture<'_,Result<(),SupervisorError>>",
        ),
    ];
    assert_eq!(trait_methods(&source, "SupervisorPort"), expected);
}

/// Every variant the supervisor port's error vocabulary is stated to carry.
#[test]
fn supervisor_error_carries_exactly_the_stated_variants() {
    let source = parse(&crate_source("printobserver-supervisor-api"));
    let expected = vec![
        variant("InvalidAnswer", &[("detail", "String")]),
        variant("IdentityRefused", &[("detail", "String")]),
        variant("Unavailable", &[("detail", "String")]),
        variant("TimedOut", &[]),
    ];
    assert_eq!(enum_variants(&source, "SupervisorError"), expected);
}

/// The store port's stated methods over prints, events, images and actions.
fn stated_store_methods_through_actions() -> Vec<Method> {
    vec![
        method(
            "open_print",
            &[("obico_print_id", "Option<i64>"), ("file_name", "Option<String>")],
            "BoxFuture<'_,Result<PrintRecord,StoreError>>",
        ),
        method(
            "print",
            &[("print_id", "PrintId")],
            "BoxFuture<'_,Result<Option<PrintRecord>,StoreError>>",
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
        method("image", &[("image_id", "ImageId")], "BoxFuture<'_,Result<ImageLookup,StoreError>>"),
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
            &[("intervention_id", "InterventionId"), ("outcome", "InterventionOutcome")],
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
            &[("print_id", "PrintId"), ("after", "Option<EventId>"), ("page_size", "u32")],
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
    let source = parse(&crate_source("printobserver-store-api"));
    let mut expected = stated_store_methods_through_actions();
    expected.extend(stated_store_methods_from_interventions());
    assert_eq!(trait_methods(&source, "StorePort"), expected);
}

/// Every variant the store port's error vocabulary is stated to carry.
#[test]
fn store_error_carries_exactly_the_stated_variants() {
    let source = parse(&crate_source("printobserver-store-api"));
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
    for (crate_name, trait_name) in [
        ("printobserver-printer-api", "PrinterPort"),
        ("printobserver-vision-api", "VisionPort"),
        ("printobserver-supervisor-api", "SupervisorPort"),
        ("printobserver-store-api", "StorePort"),
    ] {
        let source = parse(&crate_source(crate_name));
        let methods = trait_methods(&source, trait_name);
        assert!(!methods.is_empty(), "{trait_name} declares no method");
        for declared in methods {
            assert!(
                declared.returns.starts_with("BoxFuture<'_,Result<"),
                "{trait_name}::{} answers {}, which is neither asynchronous nor a Result",
                declared.name,
                declared.returns
            );
        }
    }
}

/// No port's error vocabulary carries a not-yet-implemented variant.
#[test]
fn no_port_error_carries_a_not_yet_implemented_variant() {
    for (crate_name, error_name) in [
        ("printobserver-printer-api", "PrinterError"),
        ("printobserver-vision-api", "VisionError"),
        ("printobserver-supervisor-api", "SupervisorError"),
        ("printobserver-store-api", "StoreError"),
    ] {
        let source = parse(&crate_source(crate_name));
        for declared in enum_variants(&source, error_name) {
            let name = declared.name.to_lowercase();
            assert!(
                !(name.contains("notimplemented")
                    || name.contains("unimplemented")
                    || name.contains("todo")),
                "{error_name} carries {}, a variant no real implementation can produce",
                declared.name
            );
        }
    }
}

/// The closure a trait's parameters reach, resolved against the type crate.
fn parameter_closure(
    fixture: Option<&str>,
    crate_name: &str,
    trait_name: &str,
) -> closure::Findings {
    let mut sources = crate_sources("printobserver-types");
    let declaration = match fixture {
        Some(text) => parse(text),
        None => parse(&crate_source(crate_name)),
    };
    sources.push(declaration.clone());
    let roots: Vec<(String, String)> = trait_methods(&declaration, trait_name)
        .into_iter()
        .flat_map(|declared| {
            declared.params.into_iter().map(move |param| {
                (format!("{}::{}({})", trait_name, declared.name, param.name), param.ty)
            })
        })
        .collect();
    Universe::new(sources).closure(&roots)
}

/// The one fixture shape a `FileName` exemption keyed on a parameter would let
/// through, and the four a no-string rule read only at the signature would.
const FIXTURE_BARE_STRING: &str = r"
pub trait Fixture {
    fn start(&self, file_name: String) -> BoxFuture<'_, Result<(), PrinterError>>;
}
";

/// A fixture whose parameter is a struct carrying a string.
const FIXTURE_STRUCT_WITH_STRING: &str = r"
pub struct Instruction {
    pub gcode: String,
}
pub trait Fixture {
    fn start(&self, instruction: Instruction) -> BoxFuture<'_, Result<(), PrinterError>>;
}
";

/// A fixture whose parameter is an enum carrying a byte sequence.
const FIXTURE_ENUM_WITH_BYTES: &str = r"
pub enum Blob {
    Raw(Vec<u8>),
}
pub trait Fixture {
    fn start(&self, blob: Blob) -> BoxFuture<'_, Result<(), PrinterError>>;
}
";

/// A fixture in which a method other than `start` takes a `FileName`.
const FIXTURE_SECOND_FILE_NAME: &str = r"
pub trait Fixture {
    fn start(&self, file_name: FileName) -> BoxFuture<'_, Result<(), PrinterError>>;
    fn cancel(&self, file_name: FileName) -> BoxFuture<'_, Result<(), PrinterError>>;
}
";

/// The printer port admits no byte sequence, and one string in one place.
#[test]
fn printer_port_closure_carries_no_bytes_and_one_string() {
    let findings = parameter_closure(None, "printobserver-printer-api", "PrinterPort");
    assert!(findings.bytes.is_empty(), "the closure reaches bytes at {:?}", findings.bytes);
    assert_eq!(findings.strings.len(), 1, "the closure reaches {:?}", findings.strings);
    assert!(
        findings.strings[0].ends_with("/FileName.0"),
        "the one string is {}, not the one FileName wraps",
        findings.strings[0]
    );
    assert!(findings.reached.contains("FileName"), "the closure does not reach FileName at all");
}

/// Exactly one printer-port method takes a `FileName`, and it is `start`.
#[test]
fn only_start_takes_a_file_name() {
    let source = parse(&crate_source("printobserver-printer-api"));
    let taking: Vec<(String, usize)> = trait_methods(&source, "PrinterPort")
        .into_iter()
        .map(|declared| {
            let count =
                declared.params.iter().filter(|param| param.ty.contains("FileName")).count();
            (declared.name, count)
        })
        .filter(|(_, count)| *count > 0)
        .collect();
    assert_eq!(taking, vec![("start".to_owned(), 1)]);
}

/// A fixture carrying a string or a byte sequence anywhere in the closure is
/// refused, which is what makes the committed trait's acceptance mean something.
#[test]
fn the_closure_walk_refuses_every_fixture_it_must() {
    for (label, fixture) in [
        ("a bare string parameter", FIXTURE_BARE_STRING),
        ("a struct carrying a string", FIXTURE_STRUCT_WITH_STRING),
    ] {
        let findings = parameter_closure(Some(fixture), "", "Fixture");
        assert!(
            findings.strings.iter().any(|path| !path.ends_with("/FileName.0")),
            "{label} was not refused"
        );
    }
    let findings = parameter_closure(Some(FIXTURE_ENUM_WITH_BYTES), "", "Fixture");
    assert!(!findings.bytes.is_empty(), "an enum carrying a byte sequence was not refused");

    let fixture = parse(FIXTURE_SECOND_FILE_NAME);
    let taking: Vec<String> = trait_methods(&fixture, "Fixture")
        .into_iter()
        .filter(|declared| declared.params.iter().any(|param| param.ty.contains("FileName")))
        .map(|declared| declared.name)
        .collect();
    assert_eq!(
        taking,
        vec!["start".to_owned(), "cancel".to_owned()],
        "a second method taking a FileName was not seen"
    );

    let fixture = parse(FIXTURE_BARE_STRING);
    let start = trait_methods(&fixture, "Fixture")
        .into_iter()
        .find(|declared| declared.name == "start")
        .expect("the fixture declares start");
    assert!(
        !start.params.iter().any(|param| param.ty.contains("FileName")),
        "a start taking a bare string was read as taking the newtype"
    );
}

/// Which store methods take either half of the action pair.
fn action_pair_partition(source: &syn::File, trait_name: &str) -> (Vec<Method>, Vec<Method>) {
    trait_methods(source, trait_name).into_iter().partition(|declared| {
        declared
            .params
            .iter()
            .any(|param| param.ty.contains("ActionRequest") || param.ty.contains("PolicyDecision"))
    })
}

/// `record_action` is the only method taking either half, and it takes both.
#[test]
fn record_action_is_the_only_method_taking_the_action_pair() {
    let source = parse(&crate_source("printobserver-store-api"));
    let (taking, rest) = action_pair_partition(&source, "StorePort");
    assert_eq!(taking.len(), 1, "these methods take a half of the pair: {taking:?}");
    let recorder = &taking[0];
    assert_eq!(recorder.name, "record_action");
    assert_eq!(
        recorder.params.iter().map(|param| param.ty.as_str()).collect::<Vec<_>>(),
        vec!["ActionRequest", "PolicyDecision"],
        "record_action does not take the request and the decision together"
    );
    assert!(
        recorder.returns.contains("ActionRecord"),
        "record_action answers {}, which carries no minted identifier",
        recorder.returns
    );
    assert!(
        !rest.is_empty() && rest.iter().any(|declared| declared.returns.contains("EventRecord")),
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
    assert_eq!(taking.len(), 2, "a second method taking an ActionRequest was not seen");

    let fixture = parse(FIXTURE_DECISION_ALONE);
    let (taking, _) = action_pair_partition(&fixture, "Fixture");
    assert_eq!(taking.len(), 1);
    assert_ne!(taking[0].name, "record_action", "a decision-only method was read as the recorder");

    let fixture = parse(FIXTURE_REQUEST_WITHOUT_DECISION);
    let (taking, _) = action_pair_partition(&fixture, "Fixture");
    assert_eq!(
        taking[0].params.iter().map(|param| param.ty.as_str()).collect::<Vec<_>>(),
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
            declared.params.iter().any(|param| param.ty == "Option<u32>")
                && declared.returns == "Result<u32,StoreError>"
        })
        .collect()
}

/// A consumer importing the store port finds one answer of each, not two.
#[test]
fn the_store_port_exports_one_of_each_history_answer() {
    let source = parse(&crate_source("printobserver-store-api"));
    assert_eq!(
        default_window_constants(&source)
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>(),
        vec!["DEFAULT_HISTORY_WINDOW".to_owned()]
    );
    assert_eq!(
        maximum_limit_constants(&source).into_iter().map(|entry| entry.name).collect::<Vec<_>>(),
        vec!["MAX_HISTORY_LIMIT".to_owned()]
    );
    assert_eq!(
        limit_resolutions(&source).into_iter().map(|declared| declared.name).collect::<Vec<_>>(),
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
    assert_eq!(default_window_constants(&fixture).len(), 2, "a second window was not seen");
    assert_eq!(maximum_limit_constants(&fixture).len(), 2, "a second maximum was not seen");

    let fixture = parse(FIXTURE_SECOND_RESOLUTION);
    assert_eq!(limit_resolutions(&fixture).len(), 2, "a second resolution was not seen");
}

/// The default window is below the maximum limit.
#[test]
fn the_default_window_is_below_the_maximum_limit() {
    assert!(
        printobserver_store_api_default_window() < printobserver_store_api_maximum_limit(),
        "the default window is not below the maximum limit"
    );
}

/// The declared default window, read from the store port's own source.
fn printobserver_store_api_default_window() -> u32 {
    constant_value(&crate_source("printobserver-store-api"), "DEFAULT_HISTORY_WINDOW")
}

/// The declared maximum limit, read from the store port's own source.
fn printobserver_store_api_maximum_limit() -> u32 {
    constant_value(&crate_source("printobserver-store-api"), "MAX_HISTORY_LIMIT")
}

/// The value one declared constant carries.
fn constant_value(source: &str, name: &str) -> u32 {
    let needle = format!("pub const {name}: u32 = ");
    let start = source.find(&needle).unwrap_or_else(|| panic!("{name} is declared")) + needle.len();
    let rest = &source[start..];
    let end = rest.find(';').expect("the declaration ends");
    rest[..end].replace('_', "").parse().expect("the declaration carries a number")
}

/// The `history` documentation states the ordering and the empty-kinds meaning.
#[test]
fn the_history_documentation_states_its_two_unvalued_facts() {
    let source = parse(&crate_source("printobserver-store-api"));
    let documentation =
        surface::trait_method_docs(&source, "StorePort", "history").to_lowercase();
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
    let documentation =
        surface::trait_method_docs(&fixture, "Fixture", "history").to_lowercase();
    assert!(!documentation.is_empty(), "the reader found no documentation at all");
    assert!(!documentation.contains("newest first"));
    assert!(!documentation.contains("every kind"));
}

/// The type crate declares none of the ten types the port crates own.
#[test]
fn the_type_crate_declares_none_of_the_port_owned_types() {
    let declared: Vec<String> =
        crate_sources("printobserver-types").iter().flat_map(declared_type_names).collect();
    for owned in printobserver_types::contract::PORT_OWNED_TYPES {
        assert!(
            !declared.iter().any(|name| name == owned),
            "the type crate declares {owned}, which the port that carries it owns"
        );
    }
    assert!(declared.iter().any(|name| name == "PrintRecord"), "the reader found no declarations");
}

/// A fixture crate declaring one of the port-owned types.
const FIXTURE_PORT_OWNED_TYPE: &str = r"
pub struct TurnRequest {
    pub print_id: PrintId,
}
";

/// The ownership reading refuses a crate declaring a port-owned type.
#[test]
fn the_ownership_reading_refuses_a_crate_declaring_a_port_owned_type() {
    let declared = declared_type_names(&parse(FIXTURE_PORT_OWNED_TYPE));
    let offending: Vec<&&str> = printobserver_types::contract::PORT_OWNED_TYPES
        .iter()
        .filter(|owned| declared.iter().any(|name| name == *owned))
        .collect();
    assert_eq!(offending, vec![&"TurnRequest"], "a port-owned declaration was not seen");
}

/// Each port crate declares the type crate as its only dependency.
#[test]
fn each_port_crate_declares_the_type_crate_and_nothing_else() {
    for crate_name in [
        "printobserver-printer-api",
        "printobserver-vision-api",
        "printobserver-supervisor-api",
        "printobserver-store-api",
    ] {
        let manifest = std::fs::read_to_string(surface::crate_dir(crate_name).join("Cargo.toml"))
            .expect("the manifest is readable");
        let names = dependency_names(&manifest);
        assert_eq!(
            names,
            vec!["printobserver-types".to_owned()],
            "{crate_name} declares {names:?}"
        );
    }
}

/// The dependencies one manifest declares, across every dependency table.
fn dependency_names(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut inside = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed.contains("dependencies]");
            continue;
        }
        if inside
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && let Some((name, _)) = trimmed.split_once('=')
        {
            names.push(name.trim().to_owned());
        }
    }
    names
}
