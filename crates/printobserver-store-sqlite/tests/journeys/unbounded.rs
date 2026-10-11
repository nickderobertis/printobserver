//! No method of this store answers a whole history in one response.
//!
//! This is a property of the surface rather than of any answer: a store
//! exposing both a correct paged read and an unbounded one satisfies every
//! assertion the paged read is held to. So this enumerates every method the
//! crate's store types expose and every method the store traits declare, and refuses
//! one whose signature declares none of three bounds on an answer carrying a
//! collection of events — a limit the declared maximum bounds, a page size
//! answered with a cursor, or drafts of the events it appends. The third is a
//! reading of the signature alone: that such a method answers only the events
//! its own drafts became is what the ledger's conformance journeys assert of
//! every answer it gives.

use std::collections::BTreeSet;

use crate::contracts::{store_source, store_trait_methods};
use crate::surface::{
    Method, crate_dir, crate_sources, exposed_methods, named_types, parse, read, struct_fields,
    struct_names,
};

/// The store types this crate exposes.
const STORE_TYPES: [&str; 2] = ["SqliteStore", "MemoryStore"];

/// The sources the enumeration reads: this crate's, and the store traits'.
fn sources() -> Vec<syn::File> {
    let mut files = crate_sources("printobserver-store-sqlite");
    files.push(store_source());
    files
}

/// Every method the enumeration covers, across the stores and the traits.
fn enumerated(files: &[syn::File]) -> Vec<Method> {
    let mut methods: Vec<Method> = STORE_TYPES
        .iter()
        .flat_map(|name| exposed_methods(files, name))
        .collect();
    for file in files {
        methods.extend(store_trait_methods(file));
    }
    methods
}

/// Every struct declaring a field of the named type, across the sources.
fn declaring(files: &[syn::File], field_name: Option<&str>, field_type: &str) -> BTreeSet<String> {
    struct_names(files)
        .into_iter()
        .filter(|name| {
            struct_fields(files, name).iter().any(|field| {
                field_name.is_none_or(|wanted| field.name == wanted) && field.ty == field_type
            })
        })
        .collect()
}

/// Whether an answer carries a collection of events.
fn answers_events(returns: &str, carriers: &BTreeSet<String>) -> bool {
    returns.contains("Vec<EventRecord>")
        || named_types(returns)
            .iter()
            .any(|name| carriers.contains(name))
}

/// The types whose presence in a signature declares a bound on its answer.
struct Bounds {
    /// Answers carrying the cursor a next page follows.
    cursors: BTreeSet<String>,
    /// Queries carrying a limit.
    limited: BTreeSet<String>,
    /// Parts of a write carrying the draft of an event it appends.
    drafted: BTreeSet<String>,
}

/// Whether a method's signature declares one of the three bounds.
fn declares_a_bound(method: &Method, bounds: &Bounds) -> bool {
    let takes_limit = method.params.iter().any(|param| {
        param.name == "limit"
            || named_types(&param.ty)
                .iter()
                .any(|name| bounds.limited.contains(name))
    });
    let paged = method.params.iter().any(|param| param.name == "page_size")
        && named_types(&method.returns)
            .iter()
            .any(|name| bounds.cursors.contains(name));
    let takes_drafts = method.params.iter().any(|param| {
        named_types(&param.ty)
            .iter()
            .any(|name| name == "EventDraft" || bounds.drafted.contains(name))
    });
    takes_limit || paged || takes_drafts
}

/// Every enumerated method answering events whose signature declares no bound.
fn unbounded(files: &[syn::File]) -> Vec<String> {
    let carriers = declaring(files, Some("events"), "Vec<EventRecord>");
    let bounds = Bounds {
        cursors: declaring(files, Some("next"), "Option<EventId>"),
        limited: declaring(files, Some("limit"), "Option<u32>"),
        drafted: declaring(files, None, "EventDraft"),
    };
    enumerated(files)
        .into_iter()
        .filter(|method| answers_events(&method.returns, &carriers))
        .filter(|method| !declares_a_bound(method, &bounds))
        .map(|method| format!("{}::{}", method.owner, method.name))
        .collect()
}

/// No method the store types or the traits expose answers events unbounded.
#[test]
fn no_exposed_method_answers_events_under_no_bound() {
    let files = sources();
    let answering: Vec<String> = enumerated(&files)
        .into_iter()
        .filter(|method| {
            answers_events(
                &method.returns,
                &declaring(&files, Some("events"), "Vec<EventRecord>"),
            )
        })
        .map(|method| format!("{}::{}", method.owner, method.name))
        .collect();
    assert!(
        answering.len() >= 6,
        "the reader found only {} methods answering events, which is not this surface: {answering:?}",
        answering.len()
    );
    assert_eq!(
        unbounded(&files),
        Vec::<String>::new(),
        "a method answers a collection of events under neither a bounded limit \
         nor a page size with a cursor"
    );
}

/// A method answering every event of a print in one response.
const FIXTURE_UNBOUNDED_METHOD: &str = r"
impl SqliteStore {
    pub fn whole_history(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        unimplemented!()
    }
}
";

/// A read answering events that takes a part of a write, but no draft of one.
const FIXTURE_UNBOUNDED_READ_BESIDE_A_WRITE: &str = r"
impl MemoryStore {
    pub fn spool_history(
        &self,
        spool_id: SpoolId,
        tally: FileTally,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        unimplemented!()
    }
}
";

/// The third bound is drafts in the signature, and nothing else a write takes.
#[test]
fn a_read_taking_no_draft_is_refused_however_near_a_write_it_is() {
    let mut files = sources();
    files.push(parse(FIXTURE_UNBOUNDED_READ_BESIDE_A_WRITE));
    assert_eq!(
        unbounded(&files),
        vec!["MemoryStore::spool_history".to_owned()],
        "the enumeration took a part of a write carrying no draft as a bound"
    );
}

/// The enumeration refuses such a method added to this crate.
#[test]
fn the_enumeration_refuses_an_unbounded_method_added_to_this_crate() {
    let mut files = sources();
    files.push(parse(FIXTURE_UNBOUNDED_METHOD));
    assert_eq!(
        unbounded(&files),
        vec!["SqliteStore::whole_history".to_owned()],
        "the enumeration did not refuse an unbounded answer added to the crate"
    );
}

/// The bounded reads take the port's own resolution of the limit rule.
#[test]
fn the_bounded_reads_take_the_ports_own_resolution() {
    let source = crate_dir("printobserver-store-sqlite").join("src");
    let text: String = ["sqlite.rs", "memory.rs"]
        .iter()
        .map(|name| read(&source.join(name)))
        .collect();
    assert!(
        text.contains("resolve_history_limit") && text.contains("resolved_limit"),
        "a bounded read does not reach the port's own resolution of the limit rule"
    );
    assert!(
        !text.contains("const MAX_HISTORY_LIMIT"),
        "this crate declares a maximum of its own, which is a second answer to \
         a rule the port declares once"
    );
}
