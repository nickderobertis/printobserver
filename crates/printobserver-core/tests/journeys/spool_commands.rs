//! Every spool command goes through the one granted, audited path.
//!
//! A spool command is granted per actor class from the safety envelope exactly
//! as an action is, refused naming why when the ledger cannot carry it out,
//! and written down either way — so every figure a spool carries can be traced
//! to the command that set it, who asked and why.

use std::sync::Arc;

use printobserver_core::kinds::{
    FilamentConsumedPayload, SpoolAdjustedPayload, SpoolCommandRejectedPayload, SpoolLoadedPayload,
    SpoolRegisteredPayload, SpoolRetiredPayload, SpoolUnloadedPayload,
};
use printobserver_core::{
    ActionKind, Actor, ActorClass, DEFAULT_DIAMETER_MM, PolicyDecision, RejectionReason,
    SpoolCommand, SpoolField, SpoolId, SpoolRecord, SpoolRefusalCause, SpoolTrust, block_on,
    density_for,
};
use printobserver_printer_api::PrinterState;

use crate::shelf::{
    FILE, SIZE, TOTAL_MM, adjust_command, assert_close, assert_ledger_explains, charges_to,
    command, ends, envelope_without_spools_for, every, grams, load, load_command, loaded_spool,
    on_tool, register, running_at, spool_now, spool_world, start, timeline,
};
use crate::world::{World, agent_actor};

/// The agent, as an actor of no print in particular.
fn agent() -> Actor {
    agent_actor(printobserver_types::PrintId::new())
}

/// One command of each kind, about one spool, asked for by one actor.
fn each_command(spool_id: SpoolId, actor: &Actor) -> Vec<SpoolCommand> {
    let reason = "keeping the shelf straight".to_owned();
    vec![
        SpoolCommand::RegisterSpool {
            name: "Galaxy Black".to_owned(),
            material: "PLA".to_owned(),
            color: None,
            diameter_mm: None,
            density_g_cm3: None,
            initial_g: 1_000.0,
            remaining_g: None,
            tare_g: None,
            reason: reason.clone(),
            actor: actor.clone(),
        },
        SpoolCommand::LoadSpool {
            spool_id,
            tool: 0,
            reason: reason.clone(),
            actor: actor.clone(),
        },
        SpoolCommand::UnloadSpool {
            spool_id,
            reason: reason.clone(),
            actor: actor.clone(),
        },
        SpoolCommand::AdjustSpool {
            spool_id,
            remaining_g: Some(500.0),
            gross_g: None,
            reason: reason.clone(),
            actor: actor.clone(),
        },
        SpoolCommand::RetireSpool {
            spool_id,
            reason,
            actor: actor.clone(),
        },
    ]
}

/// An agent the envelope grants no spool command is refused every one of
/// them, the refusal names the class and the action, and each refusal is in
/// the timeline with the command and its reason — while the operator, granted
/// them, has each carried out.
#[test]
fn each_spool_command_is_granted_or_refused_by_the_envelope_per_actor_class() {
    let world = World::with_config(envelope_without_spools_for(ActorClass::Agent), |_| {});
    let spool = register(&world, "Shelf Spool", 1_000.0);

    for asked in each_command(spool.id, &agent()) {
        let kind = asked.kind();
        let outcome = command(&world, asked.clone());
        assert_eq!(
            outcome.decision,
            PolicyDecision::Rejected(RejectionReason::ActorMayNotRequest {
                actor_class: ActorClass::Agent,
                action: kind,
            }),
            "{kind:?} was not refused to an agent granted none"
        );
        let refused = every::<SpoolCommandRejectedPayload>(&world);
        let last = refused.last().expect("the refusal is in the timeline");
        assert_eq!(last.command, asked);
        assert_eq!(last.decision, outcome.decision);
        assert_eq!(last.command.reason(), "keeping the shelf straight");
    }
    assert_eq!(every::<SpoolCommandRejectedPayload>(&world).len(), 5);
    let untouched = spool_now(&world, spool.id);
    assert_eq!(untouched.loaded_tool(), None);
    assert_close(untouched.remaining_g, 1_000.0);

    for asked in each_command(spool.id, &Actor::Operator) {
        let kind = asked.kind();
        let outcome = command(&world, asked);
        assert_eq!(
            outcome.decision,
            PolicyDecision::Accepted,
            "the operator was refused {kind:?}"
        );
    }
    assert!(spool_now(&world, spool.id).is_retired());
    assert_eq!(
        ActionKind::RetireSpool,
        SpoolCommand::RetireSpool {
            spool_id: spool.id,
            reason: String::new(),
            actor: Actor::Operator,
        }
        .kind()
    );
}

/// The refusal one command earns, which must be the ledger's own: its cause,
/// and a message naming what the cause says.
#[track_caller]
fn refused_with(world: &World, asked: &SpoolCommand, cause: &SpoolRefusalCause, naming: &str) {
    let outcome = command(world, asked.clone());
    let PolicyDecision::Rejected(RejectionReason::SpoolRefused {
        cause: given,
        message,
    }) = &outcome.decision
    else {
        panic!("expected a spool refusal, found {:?}", outcome.decision);
    };
    assert_eq!(given, cause, "the refusal {message:?} has another cause");
    assert!(
        message.contains(naming),
        "the refusal {message:?} does not name {naming:?}"
    );
    let written = every::<SpoolCommandRejectedPayload>(world);
    let last = written.last().expect("the refusal is written down");
    assert_eq!(&last.command, asked);
    assert_eq!(last.decision, outcome.decision);
}

/// Each case the ledger cannot carry out about the spool or the tool named is
/// refused `spool_refused` with its cause, and written down: an unknown spool,
/// a retired one, a load of the spool already on a tool with nothing to
/// confirm, and a tool the printer cannot have.
#[test]
fn every_case_the_ledger_cannot_carry_out_is_refused_saying_why() {
    let world = spool_world();
    let unknown = SpoolId::new();
    let not_known = SpoolRefusalCause::UnknownSpool { spool_id: unknown };
    refused_with(
        &world,
        &load_command(unknown, 0),
        &not_known,
        "there is no spool",
    );
    refused_with(
        &world,
        &adjust_command(unknown, 10.0),
        &not_known,
        "there is no spool",
    );

    let retired = register(&world, "Old Spool", 1_000.0);
    let outcome = command(
        &world,
        SpoolCommand::RetireSpool {
            spool_id: retired.id,
            reason: "empty".to_owned(),
            actor: Actor::Operator,
        },
    );
    assert_eq!(outcome.decision, PolicyDecision::Accepted);
    let is_retired = SpoolRefusalCause::RetiredSpool {
        spool_id: retired.id,
    };
    refused_with(
        &world,
        &load_command(retired.id, 0),
        &is_retired,
        "is retired",
    );
    refused_with(
        &world,
        &adjust_command(retired.id, 10.0),
        &is_retired,
        "is retired",
    );
    refused_with(
        &world,
        &SpoolCommand::RetireSpool {
            spool_id: retired.id,
            reason: "again".to_owned(),
            actor: Actor::Operator,
        },
        &is_retired,
        "is retired",
    );

    let loaded = loaded_spool(&world, "On The Printer", 1_000.0);
    refused_with(
        &world,
        &load_command(loaded.id, 0),
        &SpoolRefusalCause::NothingToConfirm { tool: 0 },
        "already on tool 0",
    );
    refused_with(
        &world,
        &load_command(loaded.id, -1),
        &SpoolRefusalCause::UnknownTool { tool: -1 },
        "there is no tool -1",
    );
    assert_eq!(spool_now(&world, loaded.id).loaded_tool(), Some(0));
}

/// A correction naming both figures or neither, a scale reading of a spool
/// with no tare, and a registration of a material with no known density are
/// each refused with the cause that names what to give instead.
#[test]
fn a_correction_or_registration_missing_what_it_needs_is_refused_saying_why() {
    let world = spool_world();
    let loaded = loaded_spool(&world, "On The Printer", 1_000.0);
    let both = SpoolCommand::AdjustSpool {
        spool_id: loaded.id,
        remaining_g: Some(10.0),
        gross_g: Some(300.0),
        reason: "weighed".to_owned(),
        actor: Actor::Operator,
    };
    refused_with(
        &world,
        &both,
        &SpoolRefusalCause::ConflictingFields {
            fields: vec![SpoolField::RemainingG, SpoolField::GrossG],
        },
        "exactly one of remaining_g and gross_g",
    );
    let neither = SpoolCommand::AdjustSpool {
        spool_id: loaded.id,
        remaining_g: None,
        gross_g: None,
        reason: "weighed".to_owned(),
        actor: Actor::Operator,
    };
    refused_with(
        &world,
        &neither,
        &SpoolRefusalCause::MissingField {
            field: SpoolField::RemainingG,
        },
        "exactly one of remaining_g and gross_g",
    );

    let no_tare = command(
        &world,
        SpoolCommand::RegisterSpool {
            name: "No Tare".to_owned(),
            material: "PETG".to_owned(),
            color: None,
            diameter_mm: None,
            density_g_cm3: None,
            initial_g: 1_000.0,
            remaining_g: None,
            tare_g: None,
            reason: "bought".to_owned(),
            actor: Actor::Operator,
        },
    )
    .spool
    .expect("registered");
    refused_with(
        &world,
        &SpoolCommand::AdjustSpool {
            spool_id: no_tare.id,
            remaining_g: None,
            gross_g: Some(900.0),
            reason: "weighed".to_owned(),
            actor: Actor::Operator,
        },
        &SpoolRefusalCause::TareRequired {
            spool_id: no_tare.id,
        },
        "has no tare",
    );

    refused_with(
        &world,
        &SpoolCommand::RegisterSpool {
            name: "Exotic".to_owned(),
            material: "PEEK".to_owned(),
            color: None,
            diameter_mm: None,
            density_g_cm3: None,
            initial_g: 500.0,
            remaining_g: None,
            tare_g: None,
            reason: "bought".to_owned(),
            actor: Actor::Operator,
        },
        &SpoolRefusalCause::DensityRequired {
            spool_id: None,
            material: "PEEK".to_owned(),
        },
        "PEEK has no known density",
    );
    assert_eq!(
        block_on(world.core.spools(false))
            .expect("listed")
            .iter()
            .filter(|spool| spool.name == "Exotic")
            .count(),
        0,
        "a refused registration wrote a spool"
    );
}

/// A registration with an empty name or material, or a figure no spool can
/// have, is refused with the cause that names the field, and nothing is
/// written but the refusal.
#[test]
fn every_figure_and_field_a_command_cannot_carry_is_refused_naming_it() {
    let world = spool_world();
    let registration = |name: &str, material: &str| SpoolCommand::RegisterSpool {
        name: name.to_owned(),
        material: material.to_owned(),
        color: None,
        diameter_mm: None,
        density_g_cm3: None,
        initial_g: 1_000.0,
        remaining_g: None,
        tare_g: None,
        reason: "bought".to_owned(),
        actor: Actor::Operator,
    };
    let with = |edit: Edit| {
        let mut asked = registration("Bad Figure", "PLA");
        edit(&mut asked);
        asked
    };
    let figure = |field, value| SpoolRefusalCause::InvalidAdjustmentFigure { field, value };
    refused_with(
        &world,
        &registration("  ", "PLA"),
        &SpoolRefusalCause::MissingField {
            field: SpoolField::Name,
        },
        "needs a name",
    );
    refused_with(
        &world,
        &registration("No Material", " "),
        &SpoolRefusalCause::MissingField {
            field: SpoolField::Material,
        },
        "needs its material",
    );
    let cases: [(Edit, SpoolRefusalCause, &str); 4] = [
        (
            |asked| {
                if let SpoolCommand::RegisterSpool { diameter_mm, .. } = asked {
                    *diameter_mm = Some(0.0);
                }
            },
            figure(SpoolField::DiameterMm, 0.0),
            "a diameter of 0mm",
        ),
        (
            |asked| {
                if let SpoolCommand::RegisterSpool { density_g_cm3, .. } = asked {
                    *density_g_cm3 = Some(-1.0);
                }
            },
            figure(SpoolField::DensityGCm3, -1.0),
            "a density of -1g/cm3",
        ),
        (
            |asked| {
                if let SpoolCommand::RegisterSpool { initial_g, .. } = asked {
                    *initial_g = 0.0;
                }
            },
            figure(SpoolField::InitialG, 0.0),
            "a full spool of 0g",
        ),
        (
            |asked| {
                if let SpoolCommand::RegisterSpool { tare_g, .. } = asked {
                    *tare_g = Some(-5.0);
                }
            },
            figure(SpoolField::TareG, -5.0),
            "at least zero grams",
        ),
    ];
    for (edit, cause, naming) in cases {
        refused_with(&world, &with(edit), &cause, naming);
    }
    assert!(
        block_on(world.core.spools(true))
            .expect("listed")
            .is_empty(),
        "a refused registration wrote a spool"
    );
}

/// An unload of a spool on no tool and a correction to a weight below zero are
/// refused with the cause naming the spool or the field, and change nothing.
#[test]
fn an_unload_of_a_shelved_spool_or_a_negative_reading_is_refused_naming_it() {
    let world = spool_world();
    let figure = |field, value| SpoolRefusalCause::InvalidAdjustmentFigure { field, value };
    let shelved = register(&world, "On The Shelf", 1_000.0);
    refused_with(
        &world,
        &SpoolCommand::UnloadSpool {
            spool_id: shelved.id,
            reason: "tidying".to_owned(),
            actor: Actor::Operator,
        },
        &SpoolRefusalCause::NotLoaded {
            spool_id: shelved.id,
        },
        "is not loaded on any tool",
    );
    let weighed = register(&world, "Weighed", 1_000.0);
    refused_with(
        &world,
        &SpoolCommand::AdjustSpool {
            spool_id: weighed.id,
            remaining_g: None,
            gross_g: Some(-10.0),
            reason: "weighed".to_owned(),
            actor: Actor::Operator,
        },
        &figure(SpoolField::GrossG, -10.0),
        "not a weight in grams",
    );
    assert_close(spool_now(&world, weighed.id).remaining_g, 1_000.0);
}

/// A registration naming neither diameter nor density takes 1.75 mm and the
/// material's own density, matched without regard to case; one naming them
/// keeps them; and the remaining figure defaults to the full one. The event
/// carries the spool as written, with the actor and reason.
#[test]
fn registration_resolves_the_diameter_and_density_defaults() {
    let world = spool_world();
    let defaulted = command(
        &world,
        SpoolCommand::RegisterSpool {
            name: "Defaults".to_owned(),
            material: " petg ".to_owned(),
            color: Some("orange".to_owned()),
            diameter_mm: None,
            density_g_cm3: None,
            initial_g: 750.0,
            remaining_g: None,
            tare_g: None,
            reason: "bought a spool".to_owned(),
            actor: Actor::Operator,
        },
    );
    let spool = defaulted.spool.expect("registered");
    assert_close(spool.diameter_mm, DEFAULT_DIAMETER_MM);
    assert_close(spool.density_g_cm3, density_for("PETG").expect("known"));
    assert_eq!(spool.material, "petg");
    assert_close(spool.remaining_g, 750.0);
    assert_eq!(spool.trust, SpoolTrust::Verified);

    let given = command(
        &world,
        SpoolCommand::RegisterSpool {
            name: "Given".to_owned(),
            material: "PEEK".to_owned(),
            color: None,
            diameter_mm: Some(2.85),
            density_g_cm3: Some(1.30),
            initial_g: 500.0,
            remaining_g: Some(120.0),
            tare_g: Some(200.0),
            reason: "a part-used spool".to_owned(),
            actor: Actor::Operator,
        },
    )
    .spool
    .expect("registered");
    assert_close(given.diameter_mm, 2.85);
    assert_close(given.density_g_cm3, 1.30);
    assert_close(given.remaining_g, 120.0);

    let registered = every::<SpoolRegisteredPayload>(&world);
    assert_eq!(registered.len(), 2);
    assert_eq!(registered[0].spool, spool);
    assert_eq!(registered[0].actor, Actor::Operator);
    assert_eq!(registered[0].reason, "bought a spool");
    assert_eq!(registered[1].spool, given);
}

/// A load onto an occupied tool is a swap: exactly one spool is on the tool
/// afterwards, the outgoing one is unloaded in the same write, and the
/// `spool_loaded` event names it as replaced.
#[test]
fn a_swap_leaves_exactly_one_spool_on_the_tool_and_names_the_one_replaced() {
    let world = spool_world();
    let first = loaded_spool(&world, "First", 1_000.0);
    let second = register(&world, "Second", 1_000.0);

    let outcome = load(&world, second.id, 0);
    assert_eq!(outcome.decision, PolicyDecision::Accepted);
    assert_eq!(
        outcome.spool.as_ref().map(SpoolRecord::loaded_tool),
        Some(Some(0))
    );

    let on_tool: Vec<SpoolId> = block_on(world.core.spools(false))
        .expect("listed")
        .into_iter()
        .filter(|spool| spool.loaded_tool() == Some(0))
        .map(|spool| spool.id)
        .collect();
    assert_eq!(on_tool, vec![second.id]);
    assert_eq!(spool_now(&world, first.id).loaded_tool(), None);

    let loaded = every::<SpoolLoadedPayload>(&world);
    let swap = loaded.last().expect("the swap is written down");
    assert_eq!(swap.spool_id, second.id);
    assert_eq!(swap.replaced, Some(first.id));
    let unloaded = every::<SpoolUnloadedPayload>(&world);
    assert_eq!(unloaded.len(), 1);
    assert_eq!(unloaded[0].spool_id, first.id);
    assert_eq!(unloaded[0].tool, 0);
}

/// A correction from a scale reading sets the figure to the reading less the
/// spool's tare, and the event carries both figures and the reading.
#[test]
fn a_correction_from_a_scale_reading_subtracts_the_tare() {
    let world = spool_world();
    let spool = register(&world, "Weighed", 1_000.0);
    let outcome = command(
        &world,
        SpoolCommand::AdjustSpool {
            spool_id: spool.id,
            remaining_g: None,
            gross_g: Some(870.0),
            reason: "put it on the scale".to_owned(),
            actor: Actor::Operator,
        },
    );
    assert_eq!(outcome.decision, PolicyDecision::Accepted);
    assert_close(spool_now(&world, spool.id).remaining_g, 870.0 - 250.0);
    let adjusted = every::<SpoolAdjustedPayload>(&world);
    assert_eq!(adjusted.len(), 1);
    assert_close(adjusted[0].remaining_before_g, 1_000.0);
    assert_close(adjusted[0].remaining_after_g, 620.0);
    assert_eq!(adjusted[0].gross_g, Some(870.0));
}

/// Every accepted command's event carries the actor that asked and the reason
/// it gave, whoever asked — the agent included, when it is granted the command.
#[test]
fn every_accepted_command_s_event_carries_its_actor_and_reason() {
    let world = spool_world();
    let agent = agent();
    let spool = command(&world, each_command(SpoolId::new(), &agent).remove(0))
        .spool
        .expect("registered");
    for asked in each_command(spool.id, &agent).into_iter().skip(1) {
        assert_eq!(command(&world, asked).decision, PolicyDecision::Accepted);
    }
    let registered = every::<SpoolRegisteredPayload>(&world);
    assert_eq!(registered[0].actor, agent);
    let loaded = every::<SpoolLoadedPayload>(&world);
    let unloaded = every::<SpoolUnloadedPayload>(&world);
    let adjusted = every::<SpoolAdjustedPayload>(&world);
    let retired = every::<SpoolRetiredPayload>(&world);
    for (actor, reason) in [
        (&registered[0].actor, &registered[0].reason),
        (&loaded[0].actor, &loaded[0].reason),
        (&unloaded[0].actor, &unloaded[0].reason),
        (&adjusted[0].actor, &adjusted[0].reason),
        (&retired[0].actor, &retired[0].reason),
    ] {
        assert_eq!(actor, &agent);
        assert_eq!(reason, "keeping the shelf straight");
    }
    for event in timeline(&world) {
        assert_eq!(event.source.as_str(), "agent", "{:?}", event.kind());
    }
}

/// An operator's correction made while a close-out is charging the same spool
/// loses neither: both events are in the timeline, the later one's figure
/// before is the earlier one's figure after, and the spool ends at the last
/// event's figure after.
#[test]
fn a_correction_racing_a_close_out_loses_neither_the_correction_nor_the_charge() {
    for _ in 0..8 {
        let world = spool_world();
        let spool = loaded_spool(&world, "Raced", 1_000.0);
        let print_id = start(&world);
        running_at(&world, SIZE / 2);
        world
            .printer
            .extrudes(FILE, SIZE / 2, vec![on_tool(0, TOTAL_MM / 2.0)]);

        let core = Arc::clone(&world.core);
        let id = spool.id;
        let correction = std::thread::spawn(move || {
            block_on(core.request_spool_command(adjust_command(id, 640.0)))
                .expect("the correction is written down")
        });
        ends(&world, PrinterState::Cancelling);
        let corrected = correction.join().expect("the correction thread finishes");
        assert_eq!(corrected.decision, PolicyDecision::Accepted);

        let charge = charges_to(&world, spool.id)
            .into_iter()
            .find(|charge| {
                matches!(&charge.charge_key, printobserver_core::ChargeKey::Print { print_id: p, .. } if *p == print_id)
            })
            .expect("the close-out charged the spool");
        assert_close(charge.length_mm, TOTAL_MM / 2.0);
        let charged = charge.charged.clone().expect("weighed");
        assert_close(charged.grams, grams(&spool, TOTAL_MM / 2.0));
        let adjusted = every::<SpoolAdjustedPayload>(&world)
            .pop()
            .expect("the correction is in the timeline");

        let events = timeline(&world);
        let place = |kind: &str| {
            events
                .iter()
                .rposition(|event| event.kind().as_str() == kind)
                .expect("written")
        };
        let charged_first = place("filament_consumed") < place("spool_adjusted");
        let last_after = if charged_first {
            assert_close(adjusted.remaining_before_g, charged.remaining_after_g);
            adjusted.remaining_after_g
        } else {
            assert_close(charged.remaining_before_g, adjusted.remaining_after_g);
            charged.remaining_after_g
        };
        assert_close(spool_now(&world, spool.id).remaining_g, last_after);
        assert_ledger_explains(&world, spool.id);
        assert_eq!(every::<FilamentConsumedPayload>(&world).len(), 1);
    }
}

/// One change to a registration, for the walk over the ones refused.
type Edit = fn(&mut SpoolCommand);

/// A spool unloaded during a print is charged for what it fed up to where the
/// print is, and what the tool feeds after it, with no spool on it, is
/// recorded uncharged — together the tool's total.
#[test]
fn a_spool_unloaded_mid_print_is_charged_to_the_unload() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Unloaded Mid-Print", 1_000.0);
    let print_id = start(&world);
    running_at(&world, 400_000);
    world
        .printer
        .extrudes(FILE, 400_000, vec![on_tool(0, 2_000.0)]);
    let unloaded = command(
        &world,
        SpoolCommand::UnloadSpool {
            spool_id: spool.id,
            reason: "ran out early".to_owned(),
            actor: Actor::Operator,
        },
    );
    assert_eq!(unloaded.decision, PolicyDecision::Accepted);
    running_at(&world, SIZE);
    ends(&world, PrinterState::Operational);

    let charged = charges_to(&world, spool.id);
    assert_eq!(charged.len(), 1);
    assert_close(charged[0].length_mm, 2_000.0);
    let rest: Vec<FilamentConsumedPayload> = every::<FilamentConsumedPayload>(&world)
        .into_iter()
        .filter(|charge| charge.charged.is_none())
        .collect();
    assert_eq!(rest.len(), 1);
    assert_close(rest[0].length_mm, TOTAL_MM - 2_000.0);
    let unload = every::<SpoolUnloadedPayload>(&world);
    assert_eq!(unload.len(), 1);
    let _ = print_id;
    assert_ledger_explains(&world, spool.id);
}

/// The spool commands these journeys drive, and the kinds the spool envelope
/// grants, are exactly the vocabulary `SpoolCommand` declares, read off its own
/// declaration: a command added there cannot go ungranted or undriven here.
#[test]
fn the_commands_driven_here_are_the_whole_declared_vocabulary() {
    let declared = crate::source::enum_variant_names(
        &crate::source::parse(&crate::source::read(
            &crate::source::crate_dir("printobserver-core")
                .join("src")
                .join("records")
                .join("spool_command.rs"),
        )),
        "SpoolCommand",
    );
    let driven: Vec<String> = each_command(SpoolId::new(), &Actor::Operator)
        .iter()
        .map(|asked| format!("{:?}", asked.kind()))
        .collect();
    let granted: Vec<String> = crate::shelf::SPOOL_KINDS
        .iter()
        .map(|kind| format!("{kind:?}"))
        .collect();
    assert_eq!(driven, declared);
    assert_eq!(granted, declared);
}

/// A spool retired while it is loaded is charged for what it fed up to its
/// retirement and taken off its tool in the same write, so it is retired and
/// on no tool; a spool loaded after it mid-print is charged only the
/// remainder.
#[test]
fn a_spool_retired_while_loaded_is_charged_for_what_it_fed_before_a_swap() {
    let world = spool_world();
    let retired = loaded_spool(&world, "Retired On The Printer", 1_000.0);
    let incoming = register(&world, "Loaded Over It", 1_000.0);
    start(&world);
    running_at(&world, 400_000);
    world
        .printer
        .extrudes(FILE, 400_000, vec![on_tool(0, 2_000.0)]);
    let retire = command(
        &world,
        SpoolCommand::RetireSpool {
            spool_id: retired.id,
            reason: "nearly empty, finishing this print".to_owned(),
            actor: Actor::Operator,
        },
    );
    assert_eq!(retire.decision, PolicyDecision::Accepted);
    let kinds: Vec<&str> = retire
        .events
        .iter()
        .map(|event| event.kind().as_str())
        .collect();
    assert_eq!(
        kinds,
        ["filament_consumed", "spool_unloaded", "spool_retired"],
        "retiring a loaded spool mid-print did not charge, unload and retire it"
    );
    let after = spool_now(&world, retired.id);
    assert!(after.is_retired(), "{after:?}");
    assert_eq!(after.loaded_tool(), None, "a retired spool kept its tool");
    assert_eq!(
        load(&world, incoming.id, 0).decision,
        PolicyDecision::Accepted
    );
    running_at(&world, SIZE);
    ends(&world, PrinterState::Operational);

    let to_retired = charges_to(&world, retired.id);
    assert_eq!(to_retired.len(), 1);
    assert_close(to_retired[0].length_mm, 2_000.0);
    let to_incoming = charges_to(&world, incoming.id);
    assert_eq!(to_incoming.len(), 1);
    assert_close(to_incoming[0].length_mm, TOTAL_MM - 2_000.0);
    assert!(
        every::<FilamentConsumedPayload>(&world)
            .iter()
            .all(|charge| charge.charged.is_some()),
        "a charge went to no spool"
    );
    assert_ledger_explains(&world, retired.id);
}

/// A change the store refuses on one of its own constraints is answered as a
/// spool refusal whose cause names no storage constraint, written down like
/// any other, and changes nothing.
#[test]
fn a_change_the_store_refuses_is_answered_as_a_refusal() {
    let world = spool_world();
    let spool = register(&world, "Refused By The Store", 1_000.0);
    world.store.fails(
        crate::fakes::StoreMethod::ApplySpoolChanges,
        printobserver_core::store::StoreError::ConstraintRefused {
            constraint: "spools.loaded_on_tool".to_owned(),
        },
    );
    refused_with(
        &world,
        &load_command(spool.id, 0),
        &SpoolRefusalCause::LedgerRefused,
        "the spools changed while this command was carried out",
    );
    world.store.heals();
    assert_eq!(spool_now(&world, spool.id).loaded_tool(), None);
}
