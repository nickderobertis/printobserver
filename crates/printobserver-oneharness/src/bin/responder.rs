//! The deterministic responder `OneHarness` resolves to in this crate's tests.
//!
//! It delegates to `oneharness::mock_harness`, the responder `OneHarness` itself
//! publishes: everything from the run request down through argument
//! construction, parsing, the session store and schema validation is the real
//! engine, and only the paid provider process is this. Scripted entirely
//! through the `MOCK_*` environment the run request carries.

fn main() {
    oneharness::mock_harness::run();
}
