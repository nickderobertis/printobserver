//! An answer the contracts forbid is refused as it is read, over a real socket.
//!
//! The generated walk drives every method against answers the contracts
//! admit. This drives the one shape whose two fields are held to each other —
//! a recovery allowance, whose `refusal` is sent exactly when `allowed` is
//! false — with each of the two documents that break that pairing, through the
//! published client's own `recover_status` against a stub host on a real
//! socket, and asserts each is answered as unreadable rather than handed on.

#[path = "support/host.rs"]
mod host;

use host::Host;
use printobserver_sdk::{Actor, Client, ClientError};
use serde_json::{Value, json};

/// A recovery read whose one allowance is `allowance`.
fn status_with(allowance: &Value) -> String {
    json!({
        "actions": [allowance],
        "last_print": null,
        "paused_print": null,
        "position": null,
        "state": "operational",
        "temperatures": {
            "bed": { "actual_c": 25.0, "target_c": 0.0 },
            "tools": [{ "actual_c": 25.0, "target_c": 0.0, "tool": 0 }],
        },
        "z_home": null,
        "z_trust": { "untrusted": "this server started and has homed nothing" },
    })
    .to_string()
}

/// Each agreeing allowance is read and handed on; each contradiction is
/// answered as unreadable, naming the pairing it broke.
#[test]
fn a_recovery_read_whose_allowance_contradicts_itself_is_unreadable() {
    for agreeing in [
        json!({ "action": "recover_lift", "allowed": true }),
        json!({ "action": "recover_heat", "allowed": false, "refusal": "z_untrusted" }),
    ] {
        let host = Host::answering(200, &status_with(&agreeing));
        let read = Client::new(host.address(), Actor::Operator).recover_status();
        assert!(read.is_ok(), "{agreeing}: {read:?}");
    }

    for contradiction in [
        json!({ "action": "recover_lift", "allowed": true, "refusal": "z_untrusted" }),
        json!({ "action": "recover_heat", "allowed": false }),
    ] {
        let host = Host::answering(200, &status_with(&contradiction));
        let read = Client::new(host.address(), Actor::Operator).recover_status();
        assert!(
            matches!(
                &read,
                Err(ClientError::Unreadable { status: 200, detail })
                    if detail.contains("they disagree")
            ),
            "{contradiction}: {read:?}"
        );
        assert_eq!(host.received().target, "/v1/printer/recovery");
        // An answer it could not read is not asked for again.
        assert_eq!(host.requests(), 0, "{contradiction}: the read was retried");
    }
}
