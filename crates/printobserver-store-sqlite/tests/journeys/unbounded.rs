//! No method of this store answers a whole history in one response.
//!
//! This is a property of the surface rather than of any answer: a store
//! exposing both a correct paged read and an unbounded one satisfies every
//! assertion the paged read is held to. So this enumerates every method the
//! crate's store types expose and every method the store traits declare, and refuses
//! one answering a collection of events whose signature declares neither bound
//! — a limit the declared maximum bounds, or a page size answered with a
//! cursor — unless it is one of the writes the store contract freezes, listed
//! below by owner, name, parameter types and answer type. Those answer only the
//! events they appended, which the ledger's conformance journeys assert of
//! every answer they give; a method is not one of them by taking a draft, and a
//! listed write whose signature changes is refused like any other.
use std::collections::BTreeSet;

use crate::contracts::{store_source, store_trait_methods};
use crate::surface::{
    Method, crate_dir, exposed_methods, named_types, parse, read, struct_fields, struct_names,
};

/// The store types this crate exposes.
const STORE_TYPES: [&str; 2] = ["SqliteStore", "MemoryStore"];

/// Every `.rs` source under one directory, at any depth, in path order.
///
/// At any depth because a store's implementation of a trait may live in a
/// module of its own, and a method this enumeration cannot see is one it
/// cannot refuse.
fn sources_beneath(dir: &std::path::Path) -> Vec<syn::File> {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .map(|entry| entry.expect("a readable directory entry").path())
        .collect();
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            files.extend(sources_beneath(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(parse(&read(&path)));
        }
    }
    files
}

/// The sources the enumeration reads: this crate's, and the store traits'.
fn sources() -> Vec<syn::File> {
    let mut files = sources_beneath(&crate_dir("printobserver-store-sqlite").join("src"));
    files.push(store_source());
    files
}

/// One write the store contract freezes that answers events with neither bound.
struct PinnedWrite {
    /// The method's name.
    name: &'static str,
    /// Its parameter types, in order, with whitespace removed.
    params: &'static [&'static str],
    /// Its answer type, with whitespace removed.
    returns: &'static str,
    /// Why the events it answers are bounded with no limit to say so.
    reason: &'static str,
}

/// The trait that declares the pinned writes, and the store types that
/// implement it; a pinned write is pinned under each of these owners alone.
const PINNED_OWNERS: [&str; 3] = ["FilamentStore", "SqliteStore", "MemoryStore"];

/// The writes the store contract freezes, and no other.
const PINNED_WRITES: [PinnedWrite; 2] = [
    PinnedWrite {
        name: "apply_spool_changes",
        params: &["Vec<SpoolMutation>", "Vec<Charge>", "Vec<Pend>"],
        returns: "BoxFuture<'_,Result<SpoolWrite,StoreError>>",
        reason: "it answers only the events it appended, one per mutation, charge or pend \
                 it was handed that wrote one",
    },
    PinnedWrite {
        name: "account_tallies",
        params: &[
            "Vec<FileTally>",
            "Vec<Charge>",
            "Vec<SpoolMutation>",
            "Vec<EventDraft>",
        ],
        returns: "BoxFuture<'_,Result<Vec<EventRecord>,StoreError>>",
        reason: "it answers only the events it appended, one per charge, mark or event it \
                 was handed that wrote one",
    },
];

/// The pinned write a method is, by owner, name and exact signature.
fn pinned(method: &Method) -> Option<&'static PinnedWrite> {
    if !PINNED_OWNERS.contains(&method.owner.as_str()) {
        return None;
    }
    let params: Vec<&str> = method
        .params
        .iter()
        .map(|param| param.ty.as_str())
        .collect();
    PINNED_WRITES.iter().find(|write| {
        write.name == method.name
            && write.params == params.as_slice()
            && write.returns == method.returns
    })
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

/// Whether a method's signature declares a limit, or a page size answered
/// with a cursor.
fn declares_a_bound(
    method: &Method,
    cursors: &BTreeSet<String>,
    limited: &BTreeSet<String>,
) -> bool {
    let takes_limit = method.params.iter().any(|param| {
        param.name == "limit"
            || named_types(&param.ty)
                .iter()
                .any(|name| limited.contains(name))
    });
    let paged = method.params.iter().any(|param| param.name == "page_size")
        && named_types(&method.returns)
            .iter()
            .any(|name| cursors.contains(name));
    takes_limit || paged
}

/// Every enumerated method answering events under no bound that is not a
/// pinned write.
fn unbounded(files: &[syn::File]) -> Vec<String> {
    let carriers = declaring(files, Some("events"), "Vec<EventRecord>");
    let cursors = declaring(files, Some("next"), "Option<EventId>");
    let limited = declaring(files, Some("limit"), "Option<u32>");
    enumerated(files)
        .into_iter()
        .filter(|method| answers_events(&method.returns, &carriers))
        .filter(|method| !declares_a_bound(method, &cursors, &limited))
        .filter(|method| pinned(method).is_none())
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

/// A history read that takes an event draft it never uses.
const FIXTURE_UNBOUNDED_READ_WITH_A_DRAFT: &str = r"
impl SqliteStore {
    pub fn history_beside(
        &self,
        print_id: PrintId,
        draft: EventDraft,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        unimplemented!()
    }
}
";

/// Taking a draft is no bound: an unbounded read carrying one is refused.
#[test]
fn an_unbounded_read_taking_an_event_draft_is_refused() {
    let mut files = sources();
    files.push(parse(FIXTURE_UNBOUNDED_READ_WITH_A_DRAFT));
    assert_eq!(
        unbounded(&files),
        vec!["SqliteStore::history_beside".to_owned()],
        "the enumeration took an event draft in the signature as a bound"
    );
}

/// A pinned write's name, widened by a parameter, with the same answer.
const FIXTURE_PINNED_NAME_WIDENED: &str = r"
impl MemoryStore {
    pub fn account_tallies(
        &self,
        tallies: Vec<FileTally>,
        charges: Vec<Charge>,
        marks: Vec<SpoolMutation>,
        events: Vec<EventDraft>,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        unimplemented!()
    }
}
";

/// A pinned write whose signature changed is no longer pinned, and is refused.
#[test]
fn a_pinned_write_whose_signature_changed_is_refused() {
    let mut files = sources();
    files.push(parse(FIXTURE_PINNED_NAME_WIDENED));
    assert_eq!(
        unbounded(&files),
        vec!["MemoryStore::account_tallies".to_owned()],
        "the enumeration took a pinned name with another signature as the pinned write"
    );
}

/// Every pinned write is declared by the trait and implemented by both stores
/// exactly as pinned, so the list holds nothing the surface does not.
#[test]
fn every_pinned_write_stands_on_every_owner_as_pinned() {
    let files = sources();
    let found: BTreeSet<(String, &str)> = enumerated(&files)
        .iter()
        .filter_map(|method| pinned(method).map(|write| (method.owner.clone(), write.name)))
        .collect();
    for write in &PINNED_WRITES {
        assert!(
            !write.reason.is_empty(),
            "{} is pinned with no reason",
            write.name
        );
        for owner in PINNED_OWNERS {
            assert!(
                found.contains(&(owner.to_owned(), write.name)),
                "{owner}::{} is pinned but does not stand with that signature",
                write.name
            );
        }
    }
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
