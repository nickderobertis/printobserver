//! The types this port declares into the schema set — the three snapshots,
//! the state a printer reports and the closed set of adjustables — each with
//! its canonical values.
//!
//! Shared by the `schemas` test, which reconciles the checked-in schema files
//! against them, and the `ranges` test, which drives every ranged field of them
//! at its boundary — so the two read one list rather than each keeping its own.

use printobserver_printer_api::{
    Adjustable, HeaterSnapshot, JobSnapshot, PrinterSnapshot, PrinterState,
};
use printobserver_types::contract::TypeContract;

/// Every type this port declares into the schema set.
pub fn declared() -> Vec<TypeContract> {
    vec![
        TypeContract::of::<Adjustable>("Adjustable"),
        TypeContract::of::<HeaterSnapshot>("HeaterSnapshot"),
        TypeContract::of::<JobSnapshot>("JobSnapshot"),
        TypeContract::of::<PrinterSnapshot>("PrinterSnapshot"),
        TypeContract::of::<PrinterState>("PrinterState"),
    ]
}

/// The contract for one declared type, by name.
///
/// # Panics
///
/// Panics when this port declares no type of that name.
pub fn contract(type_name: &str) -> TypeContract {
    declared()
        .into_iter()
        .find(|entry| entry.name == type_name)
        .unwrap_or_else(|| panic!("{type_name} is declared"))
}
