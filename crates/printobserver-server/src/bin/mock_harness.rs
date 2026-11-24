//! `OneHarness`'s published deterministic harness, and nothing else.
//!
//! Its whole body is `oneharness::mock_harness::run`, the entry point the
//! published `oneharness mock-harness` command dispatches to, so a journey that
//! names this program as the harness process reads what `OneHarness` handed a
//! harness from that harness's own record of it, with no code of this
//! repository between the two.

fn main() {
    oneharness::mock_harness::run();
}
