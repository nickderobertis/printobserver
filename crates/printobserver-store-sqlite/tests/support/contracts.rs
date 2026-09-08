//! What the contracts declare, derived from the contracts themselves.
//!
//! Two of this crate's checks rest on a set nobody maintains by hand: which
//! record kinds the contracts declare, and which references those records make
//! to one another. Both are read off the declarations — the store port's own
//! answers, and the type crate's own structs — so a record kind or a reference
//! added to the contracts is one this crate is held to without anybody
//! remembering to add it here.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use crate::surface::{
    crate_dir, crate_sources, named_types, parse, read, struct_fields, struct_names, trait_methods,
};

/// The table each record kind is held in.
///
/// This is naming rather than relationship: the references below are derived,
/// and a derived record kind with no table here fails the check that walks
/// them rather than being passed over.
pub const RECORD_TABLES: [(&str, &str); 7] = [
    ("PrintRecord", "prints"),
    ("EventRecord", "events"),
    ("ImageRecord", "images"),
    ("ActionRecord", "actions"),
    ("Intervention", "interventions"),
    ("JobManifest", "manifests"),
    ("SupervisionSession", "sessions"),
];

/// The table each identifier newtype identifies a row of.
pub const IDENTIFIER_TABLES: [(&str, &str); 5] = [
    ("PrintId", "prints"),
    ("EventId", "events"),
    ("ImageId", "images"),
    ("ActionId", "actions"),
    ("InterventionId", "interventions"),
];

/// One reference a record makes to another record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reference {
    /// The record kind that makes it.
    pub record: String,
    /// The table that record is held in.
    pub table: String,
    /// The column the reference is held in.
    pub column: String,
    /// The table it references.
    pub references: String,
}

/// The type crate's sources, parsed.
pub fn type_sources() -> Vec<syn::File> {
    crate_sources("printobserver-types")
}

/// The store port, parsed.
pub fn port_source() -> syn::File {
    parse(&read(
        &crate_dir("printobserver-store-api")
            .join("src")
            .join("lib.rs"),
    ))
}

/// Every record kind the contracts declare that this store persists.
///
/// A record kind is a struct the type crate declares that a method of the store
/// port answers: the port answering it is what makes it a record this store
/// holds, and the type crate declaring it is what makes it a contract rather
/// than a shape of the port's own.
pub fn record_kinds() -> BTreeSet<String> {
    let declared = struct_names(&type_sources());
    let mut kinds = BTreeSet::new();
    for method in trait_methods(&port_source(), "StorePort") {
        for name in named_types(&method.returns) {
            if declared.contains(&name) {
                kinds.insert(name);
            }
        }
    }
    kinds
}

/// The table one record kind is held in.
///
/// # Panics
///
/// Panics when the contracts declare a record kind no table here names, which
/// is the check failing rather than passing over what it does not know about.
pub fn table_of(record: &str) -> &'static str {
    RECORD_TABLES
        .iter()
        .find(|(kind, _)| *kind == record)
        .map_or_else(
            || {
                panic!(
                    "the contracts declare the record kind {record}, which this crate \
                     holds in no table"
                )
            },
            |(_, table)| *table,
        )
}

/// Every reference the contracts' own record types declare.
///
/// A reference is a field of a record whose type is one of the identifier
/// newtypes and which is not that record's own identifier — optional or not,
/// since a reference that may be absent is still a reference when it is there.
pub fn references() -> Vec<Reference> {
    let sources = type_sources();
    let identifiers: BTreeMap<&str, &str> = IDENTIFIER_TABLES.iter().copied().collect();
    let mut found = Vec::new();
    for record in record_kinds() {
        let table = table_of(&record);
        for field in struct_fields(&sources, &record) {
            if field.name == "id" {
                continue;
            }
            let named = named_types(&field.ty);
            let mut referenced = named
                .iter()
                .filter_map(|name| identifiers.get(name.as_str()))
                .peekable();
            if let Some(references) = referenced.next() {
                found.push(Reference {
                    record: record.clone(),
                    table: table.to_owned(),
                    column: field.name.clone(),
                    references: (*references).to_owned(),
                });
            }
        }
    }
    found.sort();
    found
}
