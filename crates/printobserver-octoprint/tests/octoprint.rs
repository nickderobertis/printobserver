//! Every journey this crate is held to that needs no printer, in one binary.
//!
//! One binary rather than one per journey, because the support modules below are
//! shared and a helper one binary did not happen to use would be dead code in
//! that binary — answerable only by a whole-file suppression, which this
//! repository refuses.
//!
//! Nothing here mocks the layer under test. Every journey drives the adapter's
//! own HTTP client over a real socket against a real server; what the server
//! replaces is `OctoPrint`'s *behaviour*, so that the far side can be put into
//! states a healthy `OctoPrint` will not enter on demand. The states a healthy
//! one does enter are the integration tier's, against the real thing.

#[path = "support/block_on.rs"]
mod block_on;
#[path = "support/numbers.rs"]
mod numbers;
#[path = "support/received.rs"]
mod received;
#[path = "support/records.rs"]
mod records;
#[path = "support/stub.rs"]
mod stub;
#[path = "support/surface.rs"]
mod surface;

#[path = "journeys/acting.rs"]
mod acting;
#[path = "journeys/configuring.rs"]
mod configuring;
#[path = "journeys/coverage.rs"]
mod coverage;
#[path = "journeys/failing.rs"]
mod failing;
#[path = "journeys/reading.rs"]
mod reading;
#[path = "journeys/redaction.rs"]
mod redaction;
