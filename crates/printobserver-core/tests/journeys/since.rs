//! A turn reads a print's context since an event older than the recent window.
//!
//! The window holds the print's twenty newest events, and a turn that last read
//! the context many events ago is still handed every event after the one it
//! names, rather than the window's twenty, through the real core over the fakes.

use printobserver_core::store::StoreError;
use printobserver_core::{Actor, CoreError, DEFAULT_RECENT_EVENTS, PrintAction};
use printobserver_printer_api::PrinterState;

use crate::world::World;

/// How many adjustments the journey asks for: each records its request and its
/// execution, so together more events than the window holds.
const ADJUSTMENTS: u32 = DEFAULT_RECENT_EVENTS;

#[test]
fn a_read_since_an_event_older_than_the_window_carries_every_later_event() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    let adjust = |factor: f64| {
        world
            .request(
                print.id,
                PrintAction::SetFeedrateFactor {
                    factor,
                    duration_s: Some(3600),
                    reason: "a turn adjusting between two reads".to_owned(),
                    actor: Actor::Operator,
                },
            )
            .expect("the adjustment is recorded");
    };
    adjust(1.1);
    let first = world
        .agent_context(print.id, None)
        .expect("the context reads");
    let named = first.newest_event.expect("the print has events");
    for step in 0..ADJUSTMENTS {
        adjust(if step % 2 == 0 { 1.2 } else { 1.1 });
    }

    let window = world
        .agent_context(print.id, None)
        .expect("the context reads");
    assert!(
        window.recent_events.iter().all(|event| event.id != named),
        "the event named is still inside the window, so the read would not widen"
    );
    let since = world
        .agent_context(print.id, Some(named))
        .expect("a read since an older event is answered");
    assert!(
        since.recent_events.len() > window.recent_events.len(),
        "a read since an event older than the window carried no more than the window: {}",
        since.recent_events.len()
    );
    assert!(since.recent_events.iter().all(|event| event.id != named));
    assert_eq!(
        since.recent_events.first().map(|event| event.id),
        window.newest_event
    );
    assert_eq!(since.newest_event, window.newest_event);
    let ids: Vec<_> = since.recent_events.iter().map(|event| event.id).collect();
    let mut ordered = ids.clone();
    ordered.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(ids, ordered, "the events are not newest first");

    // A print's own identifier is an event identifier no event of it carries.
    let unknown = print
        .id
        .to_string()
        .parse::<printobserver_types::EventId>()
        .expect("one spelling of identifier");
    match world.agent_context(print.id, Some(unknown)) {
        Err(CoreError::Store(StoreError::NotFound { what })) => {
            assert!(what.contains(&unknown.to_string()), "{what}");
        }
        other => panic!("a read since no event of the print's answered {other:?}"),
    }
}
