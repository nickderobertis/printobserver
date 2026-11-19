//! A turn reads a print's context, then reads it again since the newest event
//! it has already read.
//!
//! Every read here is the real `printobserver context`, against the real
//! supervisor over the stood-in machine. The print gains events through the
//! real program's own adjustments between the reads, so the print has events
//! both before and after the one a read names, and the history read through
//! the same program without one is what each answer is held to.

use printobserver_types::serde_json::{Value, json};

use crate::world::World;

use super::running;

/// What every adjustment here gives as its reason.
const REASON: &str = "a turn adjusting between two context reads";

/// The context answer's own fields: the print once, as it is now, and the
/// events. Nothing beside them is a field the agent decides anything by.
const CONTEXT_FIELDS: [&str; 10] = [
    "bounds",
    "detector",
    "interventions",
    "latest_image",
    "newest_event",
    "print",
    "printer",
    "recent_events",
    "slicer",
    "job_error",
];

/// Every field the agent's context once carried and no decision used, which a
/// read must never carry again: the job's file, progress and position, the
/// printer's own state, offsets, factors and observation time, the manifest,
/// and the detector's health beyond its status: since when, and why.
const REMOVED_FIELDS: [&str; 21] = [
    "/manifest",
    "/detector/since",
    "/job",
    "/print/file_name",
    "/print/provider_print_id",
    "/print/opened_at",
    "/print/job_started_at",
    "/print/job_print_time_s",
    "/print/ended_at",
    "/print/narrowings",
    "/printer/connection",
    "/printer/observed_at",
    "/printer/chamber",
    "/printer/feedrate_factor",
    "/printer/flowrate_factor",
    "/printer/fan_percent",
    "/printer/bed/offset_c",
    "/printer/tools/0/offset_c",
    "/slicer/layers",
    "/printer/bed/actual_c/value",
    "/printer/tools/0/actual_c/out_of_range",
];

/// The snapshot fields an event once repeated whole, which no event may carry.
const SNAPSHOT_FIELDS: [&str; 2] = ["printer", "job"];

/// Ask for one bounded adjustment of the print, through the program.
fn adjust(world: &World, print_id: &str, percent: &str) {
    running::read(
        world,
        &[
            "set-fan-percent",
            "--print-id",
            print_id,
            "--percent",
            percent,
            "--duration-s",
            "3600",
            "--reason",
            REASON,
            "--actor",
            "operator",
        ],
    );
}

/// The print's context, read through the program, since an event or not.
fn context(world: &World, print_id: &str, since: Option<&str>) -> Value {
    let mut asked = vec!["context", "--print-id", print_id];
    if let Some(since) = since {
        asked.extend(["--since", since]);
    }
    running::read(world, &asked)["context"].clone()
}

/// The identifiers of a context's events, newest first.
fn ids(context: &Value) -> Vec<String> {
    context["recent_events"]
        .as_array()
        .unwrap_or_else(|| panic!("a context answers its events: {context}"))
        .iter()
        .map(|event| {
            event["id"]
                .as_str()
                .unwrap_or_else(|| panic!("an event names itself: {event}"))
                .to_owned()
        })
        .collect()
}

/// Hold a context to the current state stated once, in full, and to events
/// that state only what happened.
fn states_the_print_once(context: &Value, print_id: &str) {
    let fields: Vec<&str> = context
        .as_object()
        .unwrap_or_else(|| panic!("a context is a document: {context}"))
        .keys()
        .map(String::as_str)
        .collect();
    for field in &fields {
        assert!(
            CONTEXT_FIELDS.contains(field),
            "a context carries {field}, which no decision reads: {context}"
        );
    }
    for removed in REMOVED_FIELDS {
        assert_eq!(
            context.pointer(removed),
            None,
            "a removed field, {removed}, is back in the context: {context}"
        );
    }
    assert_eq!(context["print"]["id"], json!(print_id), "{context}");
    assert!(context["print"]["state"].is_string(), "{context}");
    assert!(context["detector"].is_string(), "{context}");
    assert!(
        context["printer"]["tools"][0]["actual_c"].is_number(),
        "the current state is not stated in full: {context}"
    );
    assert!(context["bounds"]["allowed"].is_object(), "{context}");
    for event in context["recent_events"].as_array().into_iter().flatten() {
        for snapshot in SNAPSHOT_FIELDS {
            assert_eq!(
                event["payload"].get(snapshot),
                None,
                "an event repeats the {snapshot} snapshot the context states once: {event}"
            );
        }
    }
}

/// Make enough later events that `newest` is no longer among the latest the
/// context answers, and hold a read since it to every one of them, read from
/// the store's own history rather than from the window.
fn answers_past_the_window(world: &World, print_id: &str, newest: &str) {
    let print_id = print_id.to_owned();
    let newest = newest.to_owned();
    let mut window = ids(&context(world, &print_id, None));
    for percent in (40..=99).cycle().take(40) {
        if !window.contains(&newest) {
            break;
        }
        adjust(world, &print_id, &percent.to_string());
        window = ids(&context(world, &print_id, None));
    }
    assert!(
        !window.contains(&newest),
        "{newest} never left the context's window of {} events",
        window.len()
    );
    // Past the window's edge, so the later events outnumber what it holds.
    adjust(world, &print_id, "60");
    adjust(world, &print_id, "65");
    let history: Vec<String> = running::read(
        world,
        &["history", "--print-id", &print_id, "--limit", "1000"],
    )["events"]
        .as_array()
        .unwrap_or_else(|| panic!("a history answers its events"))
        .iter()
        .map(|event| {
            event["id"]
                .as_str()
                .expect("an event names itself")
                .to_owned()
        })
        .collect();
    let at = history
        .iter()
        .position(|event| *event == newest)
        .unwrap_or_else(|| panic!("{newest} is not in the print's history: {history:?}"));
    let wide = context(world, &print_id, Some(&newest));
    states_the_print_once(&wide, &print_id);
    assert_eq!(
        ids(&wide),
        history[..at].to_vec(),
        "a read since an event older than the window did not answer exactly every later event"
    );
    assert!(
        ids(&wide).len() > window.len(),
        "the read since an event older than the window answered no more than the window"
    );
}

/// The whole journey.
///
/// # Panics
///
/// Panics at the first read that answered an event at or before the one named,
/// left out a later one, stated the current state less than whole, or named a
/// newest event the next read refused.
pub fn a_read_since_an_event_answers_only_what_came_after_it(world: &World) {
    let print_id = world.print_id().clone();
    adjust(world, &print_id, "70");

    let first = context(world, &print_id, None);
    states_the_print_once(&first, &print_id);
    let read = ids(&first);
    assert!(
        read.len() >= 2,
        "the print has too few events to name one with others before it: {first}"
    );
    assert_eq!(
        first["newest_event"],
        json!(read[0]),
        "the newest event named is not the newest answered: {first}"
    );
    let named = read[0].clone();

    adjust(world, &print_id, "80");
    adjust(world, &print_id, "90");
    let whole = ids(&context(world, &print_id, None));
    let at = whole
        .iter()
        .position(|event| *event == named)
        .unwrap_or_else(|| panic!("{named} is not among the print's events: {whole:?}"));
    let later = whole[..at].to_vec();
    assert!(
        !later.is_empty() && at + 1 < whole.len(),
        "the event named has no events both before and after it: {whole:?}"
    );

    let since = context(world, &print_id, Some(&named));
    states_the_print_once(&since, &print_id);
    assert_eq!(
        ids(&since),
        later,
        "a read since {named} did not answer exactly every later event: {since}"
    );
    for earlier in &whole[at..] {
        assert!(
            !ids(&since).contains(earlier),
            "a read since {named} answered {earlier}, which is not after it: {since}"
        );
    }
    let newest = since["newest_event"]
        .as_str()
        .unwrap_or_else(|| panic!("a read since an event names no newest: {since}"))
        .to_owned();
    assert_eq!(newest, later[0], "{since}");

    let nothing_newer = context(world, &print_id, Some(&newest));
    states_the_print_once(&nothing_newer, &print_id);
    assert_eq!(
        ids(&nothing_newer),
        Vec::<String>::new(),
        "a read since the newest event answered events: {nothing_newer}"
    );
    assert_eq!(
        nothing_newer["newest_event"],
        json!(newest),
        "a read with nothing newer does not name the event it was asked since: {nothing_newer}"
    );

    answers_past_the_window(world, &print_id, &newest);

    // A print's own identifier is well formed, and names none of its events.
    let unknown = running::command(
        world,
        &[
            "context",
            "--print-id",
            &print_id,
            "--since",
            &print_id,
            "--json",
        ],
    );
    assert_ne!(
        unknown.code,
        Some(0),
        "a read since an event the print does not have answered: {}",
        unknown.said()
    );
}
