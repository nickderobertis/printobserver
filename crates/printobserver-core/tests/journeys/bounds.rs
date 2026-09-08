//! Effective bounds are the envelope intersected with the print's manifest.
//!
//! Four manifests, each chosen for the one property it establishes: a range
//! narrower than the envelope's, a range wider than it, an adjustable the
//! manifest is silent about, and an adjustable the envelope does not name at
//! all. Every one of them is driven through the real core and read back off the
//! context a supervision turn is given and off the print itself.

use printobserver_types::{
    Actor, Adjustable, FileName, JobManifest, PolicyDecision, PrintAction, PrintContext,
    PrintRecord, PrinterState, Range, RejectionReason,
};

use crate::world::{World, manifest};

/// The envelope's own range for the feedrate, which every fixture measures
/// against.
const ENVELOPE_FEEDRATE: Range = Range::new(0.5, 2.0);

/// The envelope's own range for the fan, which no fixture's manifest names.
const ENVELOPE_FAN: Range = Range::new(0.0, 100.0);

/// Start a print under one manifest, and read back its context and its record.
fn start_under(manifest: JobManifest) -> (World, PrintRecord, PrintContext) {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Operational);
    let outcome = world
        .request(
            print.id,
            PrintAction::StartPrint {
                file_name: FileName::new("benchy.gcode").expect("a name"),
                manifest,
                reason: "the operator started it".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the request is recorded");
    assert_eq!(outcome.decision(), &PolicyDecision::Accepted);
    let context = world.context(print.id).expect("the context reads back");
    let record = world
        .store
        .print_now(print.id)
        .expect("the print reads back");
    world.journal.assert_no_violations();
    (world, record, context)
}

/// A manifest range narrower than the envelope's is the bound that stands.
#[test]
fn a_narrower_manifest_range_is_the_effective_bound() {
    let narrower = Range::new(0.9, 1.1);
    let (_world, record, context) = start_under(manifest(&[(Adjustable::Feedrate, narrower)]));

    assert_eq!(
        context.bounds.allowed.get(&Adjustable::Feedrate),
        Some(&narrower)
    );
    assert_eq!(record.narrowings, Vec::new());
}

/// A manifest range wider than the envelope's is narrowed to the envelope's,
/// and the narrowing reads back off the print carrying both ranges.
#[test]
fn a_wider_manifest_range_is_narrowed_to_the_envelope_and_recorded() {
    let wider = Range::new(0.1, 5.0);
    let (_world, record, context) = start_under(manifest(&[(Adjustable::Feedrate, wider)]));

    assert_eq!(
        context.bounds.allowed.get(&Adjustable::Feedrate),
        Some(&ENVELOPE_FEEDRATE)
    );
    assert_eq!(record.narrowings.len(), 1);
    let narrowing = &record.narrowings[0];
    assert_eq!(narrowing.adjustable, Adjustable::Feedrate);
    assert_eq!(narrowing.requested, wider);
    assert_eq!(narrowing.applied, ENVELOPE_FEEDRATE);
}

/// An adjustable the manifest is silent about takes the envelope's own range.
///
/// The fixture is the omission rather than an extra entry, because inheriting
/// the envelope is a property of an adjustable the manifest does not name.
#[test]
fn an_adjustable_the_manifest_omits_takes_the_envelopes_range() {
    let (_world, _record, context) =
        start_under(manifest(&[(Adjustable::Feedrate, Range::new(0.9, 1.1))]));

    assert_eq!(
        context.bounds.allowed.get(&Adjustable::Fan),
        Some(&ENVELOPE_FAN)
    );
}

/// An adjustable the envelope does not name is not one this printer has.
///
/// It acquires no bound from the manifest naming it: a request for it is
/// refused as unsupported rather than judged against a range from nowhere.
#[test]
fn an_adjustable_the_envelope_does_not_name_is_unsupported() {
    let absent = Adjustable::ToolTarget { tool: 1 };
    let (world, _record, context) = start_under(manifest(&[(absent, Range::new(180.0, 260.0))]));

    assert_eq!(context.bounds.allowed.get(&absent), None);

    world.printer.reports_state(PrinterState::Printing);
    world.journal.clear();
    let outcome = world
        .request(
            context.print.id,
            PrintAction::SetToolTargetC {
                tool: 1,
                target_c: 200.0,
                duration_s: Some(60),
                reason: "the manifest named this tool".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the request is recorded");

    assert_eq!(
        outcome.decision(),
        &PolicyDecision::Rejected(RejectionReason::UnsupportedAdjustable { adjustable: absent })
    );
    assert_eq!(world.journal.printer_actions(), Vec::new());
    world.journal.assert_no_violations();
}
