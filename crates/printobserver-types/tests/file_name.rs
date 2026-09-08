//! `FileName` refuses a string if and only if it carries a forbidden property.
//!
//! Every one of those categories but the empty string is **infinite** —
//! "contains a separator" is a property of unboundedly many strings, the text
//! either side of it being arbitrary — so a fixed example per category would
//! prove only that the implementation refuses that example, and one
//! special-casing it while accepting another string carrying the same property
//! would pass. That is the path-traversal gap this test exists to close.
//!
//! So the proof ranges over the two dimensions a stand-in fixes and a
//! special-case hides behind: the **surrounding components**, generated rather
//! than written into the test, and the **insertion index**, walked over every
//! valid position rather than sampled at the start, the middle and the end.
//!
//! Beside the generation stands a fixed accepted corpus, pinning each policy
//! the type's own declaration states and the generation alphabet does not
//! reach. An implementation that widened the refusal to reach any of those
//! fails on it, which the generation alone cannot ask.

use printobserver_types::contract::Sample;
use printobserver_types::{FileName, FileNameRefusal, PrintAction, SEPARATORS};
use proptest::prelude::*;
use proptest::test_runner::{Config, FileFailurePersistence, TestCaseError, TestRng, TestRunner};
use serde_json::Value;

/// The characters a generated segment is drawn from.
///
/// The three non-ASCII characters are in it so that the type's Unicode policy
/// is exercised rather than assumed.
const ALPHABET: [char; 72] = [
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
    'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l',
    'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4',
    '5', '6', '7', '8', '9', '.', '-', '_', '+', '(', ')', ' ', 'é', '中', '😀',
];

/// The seed the generation runs under, so that a refusal reproduces.
const DEFAULT_SEED: [u8; 32] = *b"printobserver-file-name-seed-001";

/// How many segment sequences the generation draws.
const CASES: u32 = 128;

/// One non-empty segment over the generation alphabet.
fn segment() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(ALPHABET.as_slice()), 1..7).prop_map(|characters| {
        let text: String = characters.into_iter().collect();
        // No generated segment is a parent-directory segment, so a
        // generated value is refused by what the walk inserts rather than
        // by what the generation produced.
        if text == "." || text == ".." {
            format!("{text}x")
        } else {
            text
        }
    })
}

/// One to three segments, which render as a value or as a joined path.
fn segments() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(segment(), 1..4)
}

/// Insert one character at one character index.
fn insert_at(value: &str, index: usize, inserted: char) -> String {
    let mut characters: Vec<char> = value.chars().collect();
    characters.insert(index, inserted);
    characters.into_iter().collect()
}

/// The action a candidate file name is carried into on the wire.
fn start_print_action(file_name: &str) -> Value {
    let mut action = serde_json::to_value(
        PrintAction::sample_alternates()
            .into_iter()
            .find(|action| matches!(action, PrintAction::StartPrint { .. }))
            .expect("the vocabulary declares start_print"),
    )
    .expect("the action serializes");
    action["file_name"] = Value::String(file_name.to_owned());
    action
}

/// Whether the wire refuses a start-a-print action naming this file name.
fn wire_refuses(candidate: &str) -> bool {
    serde_json::from_value::<PrintAction>(start_print_action(candidate)).is_err()
}

/// Assert one candidate is refused at construction and on the wire alike.
fn assert_refused(candidate: &str) -> Result<(), TestCaseError> {
    if FileName::new(candidate).is_ok() {
        return Err(TestCaseError::fail(format!(
            "construction accepted {candidate:?}"
        )));
    }
    if !wire_refuses(candidate) {
        return Err(TestCaseError::fail(format!(
            "the wire accepted {candidate:?}"
        )));
    }
    Ok(())
}

/// Assert one candidate is accepted and re-emitted exactly as it was given.
fn assert_accepted(candidate: &str) -> Result<(), TestCaseError> {
    let name = FileName::new(candidate)
        .map_err(|error| TestCaseError::fail(format!("construction refused: {error}")))?;
    if name.as_str() != candidate {
        return Err(TestCaseError::fail(format!(
            "{candidate:?} came back as {name}"
        )));
    }
    let parsed: PrintAction = serde_json::from_value(start_print_action(candidate))
        .map_err(|error| TestCaseError::fail(format!("the wire refused {candidate:?}: {error}")))?;
    let PrintAction::StartPrint { file_name, .. } = &parsed else {
        return Err(TestCaseError::fail(
            "the action is not a start_print".to_owned(),
        ));
    };
    if file_name.as_str() != candidate {
        return Err(TestCaseError::fail(format!(
            "the wire returned {file_name}"
        )));
    }
    Ok(())
}

/// Every value the refusal walk yields for one generated segment sequence.
fn drive_refusals(parts: &[String]) -> Result<(), TestCaseError> {
    let single = parts.join("");
    let length = single.chars().count();

    // A separator, and a NUL byte, at every index from zero to the length.
    for index in 0..=length {
        for separator in SEPARATORS {
            assert_refused(&insert_at(&single, index, separator))?;
        }
        assert_refused(&insert_at(&single, index, '\0'))?;
    }

    // A parent-directory segment at every segment boundary, under each
    // separator, and as the whole value.
    for separator in SEPARATORS {
        for dots in [".", ".."] {
            for position in 0..=parts.len() {
                let mut joined: Vec<String> = parts.to_vec();
                joined.insert(position, dots.to_owned());
                assert_refused(&joined.join(&separator.to_string()))?;
            }
            assert_refused(dots)?;
        }
    }

    // A drive prefix, with and without a following separator.
    for letter in ('a'..='z').chain('A'..='Z') {
        assert_refused(&format!("{letter}:{single}"))?;
        for separator in SEPARATORS {
            assert_refused(&format!("{letter}:{separator}{single}"))?;
        }
    }

    // A leading separator, and the one category that is not infinite.
    for separator in SEPARATORS {
        assert_refused(&format!("{separator}{single}"))?;
    }
    assert_refused("")
}

/// The generation refuses every forbidden property and accepts what is left.
#[test]
fn generated_names_are_refused_for_every_forbidden_property() {
    let seed = std::env::var("PRINTOBSERVER_FILE_NAME_SEED")
        .ok()
        .and_then(|text| {
            let bytes = text.into_bytes();
            <[u8; 32]>::try_from(bytes.as_slice()).ok()
        })
        .unwrap_or(DEFAULT_SEED);
    // The seed below is what reproduces a refusal, so there is no persistence
    // file to keep beside it.
    let config = Config {
        cases: CASES,
        failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
        ..Config::default()
    };
    let mut runner = TestRunner::new_with_rng(
        config,
        TestRng::from_seed(proptest::test_runner::RngAlgorithm::ChaCha, &seed),
    );
    let outcome = runner.run(&segments(), |parts| {
        // A one-segment sequence carries none of the forbidden properties, so
        // it is the accepted side; an implementation refusing everything fails
        // here rather than passing the refusal walk for having nothing to get
        // wrong.
        assert_accepted(&parts[0])?;
        drive_refusals(&parts)
    });
    assert!(
        outcome.is_ok(),
        "seed {}: {outcome:?}",
        String::from_utf8_lossy(&seed)
    );
}

/// The names the declaration says are accepted, which the alphabet cannot reach.
const ACCEPTED_CORPUS: [&str; 7] = [
    ".gitignore",
    "..hidden",
    "ab:file.gcode",
    "tab\there.gcode",
    "newline\nhere.gcode",
    " leading.gcode",
    "trailing.gcode ",
];

/// The fixed corpus pins each policy the declaration states.
#[test]
fn the_fixed_accepted_corpus_is_accepted() {
    for candidate in ACCEPTED_CORPUS {
        assert_accepted(candidate)
            .unwrap_or_else(|error| panic!("{candidate:?} was refused: {error}"));
    }
}

/// The shapes already known to escape, so that one cannot escape again.
const REGRESSION_CORPUS: [&str; 10] = [
    "",
    ".",
    "..",
    "/etc/passwd",
    "..\\..\\windows",
    "benchy/../../etc/passwd",
    "C:benchy.gcode",
    "c:\\benchy.gcode",
    "benchy\0.gcode",
    "/benchy.gcode",
];

/// The fixed regression corpus stays refused.
#[test]
fn the_fixed_regression_corpus_is_refused() {
    for candidate in REGRESSION_CORPUS {
        assert_refused(candidate)
            .unwrap_or_else(|error| panic!("{candidate:?} was accepted: {error}"));
    }
}

/// The generation ranges over every category the type declares.
///
/// A forbidden vocabulary that gains a category the walk does not range over is
/// refused here, so the coverage cannot fall behind the type.
#[test]
fn the_walk_ranges_over_every_declared_category() {
    let ranged_over = [
        FileNameRefusal::Empty,
        FileNameRefusal::Nul,
        FileNameRefusal::LeadingSeparator,
        FileNameRefusal::Separator,
        FileNameRefusal::DotSegment,
        FileNameRefusal::DrivePrefix,
    ];
    assert_eq!(FileNameRefusal::ALL.to_vec(), ranged_over.to_vec());
    assert_eq!(
        SEPARATORS,
        ['/', '\\'],
        "the separator set is not the two the type declares"
    );
}

/// Each declared category refuses at least one value, in its own words.
#[test]
fn each_declared_category_refuses_in_its_own_words() {
    let cases = [
        ("", FileNameRefusal::Empty),
        ("benchy\0.gcode", FileNameRefusal::Nul),
        ("/benchy.gcode", FileNameRefusal::LeadingSeparator),
        ("a/benchy.gcode", FileNameRefusal::Separator),
        ("..", FileNameRefusal::DotSegment),
        ("c:benchy.gcode", FileNameRefusal::DrivePrefix),
    ];
    for (candidate, expected) in cases {
        let error = FileName::new(candidate).expect_err("the candidate is refused");
        assert_eq!(error.refusal(), expected, "{candidate:?}");
        assert_eq!(error.value(), candidate);
        assert!(!error.to_string().is_empty());
    }
    assert_eq!(
        cases.len(),
        FileNameRefusal::ALL.len(),
        "a category is not reached"
    );
}

/// A canonical name is what a canonical value carries.
#[test]
fn the_canonical_name_is_accepted() {
    assert_accepted(FileName::sample_full().as_str()).expect("the canonical name is accepted");
}
