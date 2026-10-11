//! The arithmetic of filament, and the one rule for which command clears which
//! mark: a correction clears every mark, a swap clears only the outgoing
//! spool's unconfirmed filament change, and a confirmation in place clears both
//! filament-change marks.
//!
//! These are the functions the store and the supervision domain call rather
//! than restate, so each is pinned here on a worked figure: the seven
//! densities, a gram figure worked by hand, the buffer on both sides of the
//! point where its minimum stops mattering, and the clearing rule over a spool
//! holding marks of more than one kind.

use printobserver_core::{
    DEFAULT_DIAMETER_MM, MATERIAL_DENSITIES, MarkClearing, SpoolPolicy, SpoolTrust, UnverifiedMark,
    UnverifiedReason, UnverifiedReasonKind, buffer_g, density_for, grams_for_length,
    marks_cleared_by,
};
use printobserver_types::contract::Sample;
use printobserver_types::{PrintId, Timestamp};

/// One fixed instant, `seconds` after the samples' own.
fn at(seconds: i64) -> Timestamp {
    Timestamp::sample_full()
        .plus_seconds(seconds)
        .expect("an instant")
}

/// A figure rounded to two decimals, the way a person reads grams.
fn to_hundredths(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Assert two figures agree to well within a thousandth of a gram.
#[track_caller]
fn assert_grams(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{what}: {actual} is not {expected}"
    );
}

/// The table holds exactly the seven densities stated, and each is matched
/// whatever its case and surrounding space.
#[test]
fn the_material_table_holds_the_seven_stated_densities() {
    assert_eq!(
        MATERIAL_DENSITIES,
        [
            ("PLA", 1.24),
            ("PETG", 1.27),
            ("ABS", 1.04),
            ("ASA", 1.07),
            ("TPU", 1.21),
            ("PC", 1.20),
            ("PA", 1.14),
        ]
    );
    for (material, density) in MATERIAL_DENSITIES {
        assert_eq!(density_for(material), Some(density), "{material}");
        assert_eq!(
            density_for(&material.to_lowercase()),
            Some(density),
            "{material} in lower case"
        );
    }
    assert_eq!(density_for("Petg"), Some(1.27));
    assert_eq!(density_for(" pla "), Some(1.24));
    assert_eq!(density_for("PEEK"), None);
    assert_eq!(density_for("PLA+"), None);
    assert_eq!(density_for(""), None);
}

/// A metre of 1.75 mm PLA weighs 2.98 g: π·(1.75/2)²·1000 mm³ is 2.405 cm³,
/// at 1.24 g/cm³.
#[test]
fn a_metre_of_pla_weighs_two_point_nine_eight_grams() {
    assert_grams(DEFAULT_DIAMETER_MM, 1.75, "the default diameter");
    let grams = grams_for_length(1_000.0, DEFAULT_DIAMETER_MM, 1.24);
    assert_grams(
        to_hundredths(grams),
        2.98,
        "a metre of PLA, to two decimals",
    );
    assert_grams(
        grams,
        std::f64::consts::PI * 0.875 * 0.875 * 1.24,
        "a metre of PLA",
    );
    assert_grams(
        to_hundredths(grams_for_length(2_000.0, DEFAULT_DIAMETER_MM, 1.24)),
        5.97,
        "twice the length is twice the weight",
    );
    assert_grams(
        grams_for_length(0.0, DEFAULT_DIAMETER_MM, 1.24),
        0.0,
        "no length",
    );
    assert_grams(
        to_hundredths(grams_for_length(1_000.0, 2.85, 1.24)),
        7.91,
        "the diameter is squared",
    );
}

/// The buffer is the policy's percentage of the requirement or its minimum,
/// whichever is more, on both sides of the fifty grams where the two meet.
#[test]
fn the_buffer_is_the_larger_of_the_percentage_and_the_minimum() {
    let policy = SpoolPolicy::default();
    assert_grams(buffer_g(10.0, &policy), 5.0, "a tenth of 10 g is below 5 g");
    assert_grams(buffer_g(49.0, &policy), 5.0, "just below the crossover");
    assert_grams(buffer_g(50.0, &policy), 5.0, "at the crossover");
    assert_grams(buffer_g(51.0, &policy), 5.1, "just above it");
    assert_grams(buffer_g(200.0, &policy), 20.0, "well above it");

    let generous = SpoolPolicy {
        buffer_percent: 25.0,
        buffer_min_g: 2.0,
        ..SpoolPolicy::default()
    };
    assert_grams(buffer_g(4.0, &generous), 2.0, "a quarter of 4 g");
    assert_grams(buffer_g(40.0, &generous), 10.0, "a quarter of 40 g");
}

/// A spool holding an uncharged print and an unconfirmed filament change.
fn twice_marked(print_id: PrintId) -> SpoolTrust {
    SpoolTrust::Verified
        .marked(UnverifiedMark {
            reason: UnverifiedReason::UnchargedPrint { print_id },
            since: at(0),
        })
        .marked(UnverifiedMark {
            reason: UnverifiedReason::FilamentChangeUnconfirmed { print_id },
            since: at(60),
        })
}

/// A spool keeps every mark it is given, oldest first, and marking it again
/// for a reason it already holds adds nothing; the same kind with another
/// payload is another reason.
#[test]
fn a_spool_keeps_every_distinct_mark_and_no_duplicate() {
    let print_id = PrintId::sample_full();
    let trust = twice_marked(print_id);
    let SpoolTrust::Unverified { marks } = &trust else {
        panic!("a marked spool is unverified, not {trust:?}");
    };
    let kinds: Vec<UnverifiedReasonKind> = marks.iter().map(|mark| mark.reason.kind()).collect();
    assert_eq!(
        kinds,
        [
            UnverifiedReasonKind::UnchargedPrint,
            UnverifiedReasonKind::FilamentChangeUnconfirmed
        ]
    );

    let again = trust
        .clone()
        .marked(UnverifiedMark {
            reason: UnverifiedReason::UnchargedPrint { print_id },
            since: at(120),
        })
        .marked(UnverifiedMark {
            reason: UnverifiedReason::FilamentChangeUnconfirmed { print_id },
            since: at(180),
        });
    assert_eq!(again, trust, "marking again for a held reason added a mark");

    let other_print = PrintId::new();
    let widened = trust.marked(UnverifiedMark {
        reason: UnverifiedReason::UnchargedPrint {
            print_id: other_print,
        },
        since: at(240),
    });
    let SpoolTrust::Unverified { marks } = widened else {
        panic!("a marked spool is unverified");
    };
    assert_eq!(
        marks.len(),
        3,
        "an uncharged print of another print is its own reason"
    );
}

/// A swap clears only the unconfirmed filament change and leaves the spool
/// unverified on the other mark; a confirmation in place clears both
/// filament-change marks and nothing else; a correction clears every one; and
/// each says what it removed.
#[test]
fn each_clearing_removes_exactly_the_marks_its_rule_names() {
    let print_id = PrintId::sample_full();

    let (swapped, removed) =
        twice_marked(print_id).cleared(marks_cleared_by(MarkClearing::SwappedOut));
    assert_eq!(
        removed,
        vec![UnverifiedReason::FilamentChangeUnconfirmed { print_id }]
    );
    assert_eq!(
        swapped,
        SpoolTrust::Unverified {
            marks: vec![UnverifiedMark {
                reason: UnverifiedReason::UnchargedPrint { print_id },
                since: at(0),
            }]
        }
    );

    let positionless = twice_marked(print_id).marked(UnverifiedMark {
        reason: UnverifiedReason::FilamentChangeUnconfirmedPosition { print_id },
        since: at(120),
    });
    let (swapped, removed) = positionless
        .clone()
        .cleared(marks_cleared_by(MarkClearing::SwappedOut));
    assert_eq!(
        removed,
        vec![UnverifiedReason::FilamentChangeUnconfirmed { print_id }],
        "a swap cleared the doubt over a position nobody read"
    );
    assert_eq!(
        swapped.oldest_mark().map(|mark| mark.reason.kind()),
        Some(UnverifiedReasonKind::UnchargedPrint)
    );

    let (confirmed, removed) =
        positionless.cleared(marks_cleared_by(MarkClearing::ConfirmedInPlace));
    assert_eq!(
        removed,
        vec![
            UnverifiedReason::FilamentChangeUnconfirmed { print_id },
            UnverifiedReason::FilamentChangeUnconfirmedPosition { print_id },
        ]
    );
    assert_eq!(
        confirmed,
        SpoolTrust::Unverified {
            marks: vec![UnverifiedMark {
                reason: UnverifiedReason::UnchargedPrint { print_id },
                since: at(0),
            }]
        },
        "a confirmation in place cleared the uncharged print"
    );

    let (adjusted, removed) =
        twice_marked(print_id).cleared(marks_cleared_by(MarkClearing::Adjusted));
    assert_eq!(adjusted, SpoolTrust::Verified);
    assert_eq!(
        removed,
        vec![
            UnverifiedReason::UnchargedPrint { print_id },
            UnverifiedReason::FilamentChangeUnconfirmed { print_id },
        ]
    );

    let (untouched, removed) =
        SpoolTrust::Verified.cleared(marks_cleared_by(MarkClearing::Adjusted));
    assert_eq!(untouched, SpoolTrust::Verified);
    assert!(removed.is_empty());
}

/// The clearing rule names exactly those kinds for each command: every kind
/// for a correction, the unconfirmed change for a swap, and both
/// filament-change kinds for a confirmation in place.
#[test]
fn the_clearing_rule_is_the_stated_one() {
    assert_eq!(
        marks_cleared_by(MarkClearing::Adjusted),
        [
            UnverifiedReasonKind::UnchargedPrint,
            UnverifiedReasonKind::UnaccountedPrint,
            UnverifiedReasonKind::FilamentChangeUnconfirmed,
            UnverifiedReasonKind::FilamentChangeUnconfirmedPosition,
            UnverifiedReasonKind::TallyReset,
        ]
    );
    assert_eq!(
        marks_cleared_by(MarkClearing::SwappedOut),
        [UnverifiedReasonKind::FilamentChangeUnconfirmed]
    );
    assert_eq!(
        marks_cleared_by(MarkClearing::ConfirmedInPlace),
        [
            UnverifiedReasonKind::FilamentChangeUnconfirmed,
            UnverifiedReasonKind::FilamentChangeUnconfirmedPosition,
        ]
    );
}

/// The oldest mark is the earliest given, and a verified spool has none.
#[test]
fn the_oldest_mark_is_the_earliest_given() {
    let print_id = PrintId::sample_full();
    let oldest = twice_marked(print_id);
    assert_eq!(
        oldest.oldest_mark(),
        Some(&UnverifiedMark {
            reason: UnverifiedReason::UnchargedPrint { print_id },
            since: at(0),
        })
    );
    assert_eq!(SpoolTrust::Verified.oldest_mark(), None);
}

/// Marks given out of time order are still held oldest first: the wire lists
/// them by `since`, the oldest mark is the earliest instant rather than the
/// first given, and a clearing answers what it removed in that same order. A
/// reason already held is still not added again, whatever its instant.
#[test]
fn marks_given_out_of_order_are_held_oldest_first() {
    let print_id = PrintId::sample_full();
    let tally_reset = UnverifiedReason::TallyReset {
        file_name: "benchy.gcode".to_owned(),
        origin: "local".to_owned(),
    };
    let trust = SpoolTrust::Verified
        .marked(UnverifiedMark {
            reason: UnverifiedReason::FilamentChangeUnconfirmed { print_id },
            since: at(120),
        })
        .marked(UnverifiedMark {
            reason: tally_reset.clone(),
            since: at(60),
        })
        .marked(UnverifiedMark {
            reason: UnverifiedReason::UnchargedPrint { print_id },
            since: at(0),
        })
        .marked(UnverifiedMark {
            reason: tally_reset.clone(),
            since: at(-60),
        });

    let written = printobserver_types::serde_json::to_value(&trust).expect("a trust writes");
    let order: Vec<&str> = written["marks"]
        .as_array()
        .expect("an unverified spool lists its marks")
        .iter()
        .map(|mark| mark["reason"]["reason"].as_str().expect("a tagged reason"))
        .collect();
    assert_eq!(
        order,
        [
            "uncharged_print",
            "tally_reset",
            "filament_change_unconfirmed"
        ],
        "{written}"
    );
    assert_eq!(
        written["marks"][1]["since"],
        printobserver_types::serde_json::to_value(at(60)).expect("an instant writes"),
        "a held reason given again moved its mark"
    );

    assert_eq!(
        trust.oldest_mark(),
        Some(&UnverifiedMark {
            reason: UnverifiedReason::UnchargedPrint { print_id },
            since: at(0),
        })
    );

    let (_, removed) = trust.cleared(marks_cleared_by(MarkClearing::Adjusted));
    assert_eq!(
        removed,
        vec![
            UnverifiedReason::UnchargedPrint { print_id },
            tally_reset,
            UnverifiedReason::FilamentChangeUnconfirmed { print_id },
        ]
    );
}
