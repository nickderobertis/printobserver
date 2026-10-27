//! A secret drawn fresh for one run, so that no test embeds one.
//!
//! A journey's server still needs a credential to admit, a token to send its
//! detector and a secret to check ingress against; drawing each from the
//! operating system's random source on every run keeps every one of them out
//! of the tree.

use core::fmt::Write as _;

/// A secret no other run shares: `label`, then 32 random hexadecimal digits.
#[must_use]
pub fn minted(label: &str) -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).expect("the operating system's random source answers");
    bytes.iter().fold(format!("{label}-"), |mut text, byte| {
        write!(text, "{byte:02x}").expect("writing to a string cannot fail");
        text
    })
}
