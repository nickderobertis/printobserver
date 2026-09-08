//! The history read's limit rule has one resolution, and it refuses.
//!
//! This crate declares the default window, the maximum limit and the one
//! resolution of the rule, so an implementation imports an answer rather than
//! choosing one. What an implementation then does with it is proven where the
//! implementations are.

use printobserver_store_api::{
    DEFAULT_HISTORY_WINDOW, HistoryQuery, MAX_HISTORY_LIMIT, StoreError, resolve_history_limit,
};
use printobserver_types::contract::Sample;
use printobserver_types::{EventKind, PrintId};

/// The default window is below the maximum limit.
#[test]
fn the_default_window_is_below_the_maximum_limit() {
    assert!(
        DEFAULT_HISTORY_WINDOW < MAX_HISTORY_LIMIT,
        "the default window {DEFAULT_HISTORY_WINDOW} is not below the maximum {MAX_HISTORY_LIMIT}"
    );
}

/// An absent limit takes the declared default window.
#[test]
fn an_absent_limit_takes_the_default_window() {
    assert_eq!(resolve_history_limit(None), Ok(DEFAULT_HISTORY_WINDOW));
}

/// A limit at the maximum is taken as asked for.
#[test]
fn a_limit_at_the_maximum_is_taken_as_asked_for() {
    assert_eq!(
        resolve_history_limit(Some(MAX_HISTORY_LIMIT)),
        Ok(MAX_HISTORY_LIMIT)
    );
    assert_eq!(resolve_history_limit(Some(1)), Ok(1));
}

/// A limit above the maximum is refused naming the maximum, not clamped.
#[test]
fn a_limit_above_the_maximum_is_refused_rather_than_clamped() {
    let asked_for = MAX_HISTORY_LIMIT + 1;
    assert_eq!(
        resolve_history_limit(Some(asked_for)),
        Err(StoreError::LimitRefused {
            limit: MAX_HISTORY_LIMIT,
            asked_for
        }),
        "a resolution that clamped would have answered the maximum here"
    );
}

/// A query resolves its own limit by the same one rule.
#[test]
fn a_query_resolves_its_limit_by_the_same_rule() {
    let query = HistoryQuery {
        print_id: PrintId::sample_full(),
        kinds: vec![EventKind::ObicoFailureAlert],
        since: None,
        until: None,
        limit: None,
    };
    assert_eq!(query.resolved_limit(), Ok(DEFAULT_HISTORY_WINDOW));

    let refused = HistoryQuery {
        limit: Some(MAX_HISTORY_LIMIT + 1),
        ..query.clone()
    };
    assert_eq!(
        refused.resolved_limit(),
        Err(StoreError::LimitRefused {
            limit: MAX_HISTORY_LIMIT,
            asked_for: MAX_HISTORY_LIMIT + 1
        })
    );

    let empty_kinds = HistoryQuery {
        kinds: vec![],
        ..query
    };
    assert!(
        empty_kinds.kinds.is_empty(),
        "an empty kinds is representable, and this crate's documentation says it means every kind"
    );
}
