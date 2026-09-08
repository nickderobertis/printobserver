//! Comparing two numbers exactly, where exactness is the point.
//!
//! Every float this crate answers is the result of a conversion rather than of a
//! measurement: a percentage divided by a hundred, a temperature carried through
//! unchanged. The answer is exactly the number or the conversion is wrong, so
//! these compare bit for bit — which says that, and says it without asking the
//! linter for an exception to a rule it is right about everywhere a measurement
//! is being compared.

/// Whether two values are the same number, bit for bit.
pub fn same_number(found: f64, expected: f64) -> bool {
    found.to_bits() == expected.to_bits()
}

/// Assert that a named value is exactly the number it should be.
///
/// # Panics
///
/// Panics when it is not, naming what was found and what was expected.
pub fn exactly(what: &str, found: f64, expected: f64) {
    assert!(
        same_number(found, expected),
        "{what} is {found}, and it should be exactly {expected}"
    );
}
