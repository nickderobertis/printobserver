//! An answer the contracts forbid is refused as it is read, over a real socket.
//!
//! The generated walk drives every method against answers the contracts
//! admit. This drives three the contracts forbid, through the published
//! client's own methods against a stub host on a real socket, and asserts each
//! is answered as unreadable rather than handed on: a recovery allowance whose
//! `refusal` is not sent exactly when `allowed` is false, a machine refusal of
//! a recovery that did not fail at the printer, and a policy refusal of one
//! that carries no refusal the contracts declare.

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
        json!({ "action": "recover_lift", "allowed": true, "refusal": null }),
    ] {
        let host = Host::answering(200, &status_with(&contradiction));
        let read = Client::new(host.address(), Actor::Operator).recover_status();
        assert!(
            matches!(
                &read,
                Err(ClientError::Unreadable { status: 200, detail })
                    if detail.contains("they disagree") || detail.contains("never as null")
            ),
            "{contradiction}: {read:?}"
        );
        assert_eq!(host.received().target, "/v1/printer/recovery");
        // An answer it could not read is not asked for again.
        assert_eq!(host.requests(), 0, "{contradiction}: the read was retried");
    }
}

/// A recovery's answer, around one decision.
fn recovery_with(decision: &Value) -> String {
    json!({
        "action": { "action": "recover_fan", "percent": 50.0 },
        "decision": decision,
        "detail": "recover_fan reached the printer, which refused it: the fan is stuck",
        "events": [],
    })
    .to_string()
}

/// A recovery the machine refused arrives through `recover_fan` as the
/// supervisor's own sentence; the same status carrying a recovery that
/// succeeded, or one the policy refused, is no machine refusal and is
/// answered as unreadable.
#[test]
fn a_machine_refused_recovery_is_read_only_when_it_failed_at_the_printer() {
    let failed = json!({ "executed": { "failed": { "reason": "the fan is stuck" } } });
    let host = Host::answering(502, &recovery_with(&failed));
    let refused = Client::new(host.address(), Actor::Operator).recover_fan(50.0, "because");
    assert!(
        matches!(
            &refused,
            Err(ClientError::Refused { status: 502, detail }) if detail.contains("the fan is stuck")
        ),
        "{refused:?}"
    );
    assert_eq!(host.received().target, "/v1/printer/recovery/recover_fan");

    for decision in [
        json!({ "executed": "succeeded" }),
        json!({ "rejected": "z_untrusted" }),
    ] {
        let host = Host::answering(502, &recovery_with(&decision));
        let read = Client::new(host.address(), Actor::Operator).recover_fan(50.0, "because");
        assert!(
            matches!(&read, Err(ClientError::Unreadable { status: 502, .. })),
            "{decision}: {read:?}"
        );
    }
}

/// A recovery the policy refused arrives through `recover_fan` as a typed
/// refusal; the same status carrying a recovery that was carried out, or a
/// refusal that is no refusal the contracts declare, is answered as
/// unreadable rather than as a refusal.
#[test]
fn a_policy_refused_recovery_is_read_only_when_it_is_a_refusal() {
    let host = Host::answering(409, &recovery_with(&json!({ "rejected": "z_untrusted" })));
    let refused = Client::new(host.address(), Actor::Operator).recover_fan(50.0, "because");
    assert!(
        matches!(&refused, Err(ClientError::RecoveryRejected(_))),
        "{refused:?}"
    );
    assert_eq!(host.received().target, "/v1/printer/recovery/recover_fan");

    for decision in [
        json!({ "executed": "succeeded" }),
        json!({ "rejected": { "print_running": {} } }),
        json!({ "rejected": 7 }),
    ] {
        let host = Host::answering(409, &recovery_with(&decision));
        let read = Client::new(host.address(), Actor::Operator).recover_fan(50.0, "because");
        assert!(
            matches!(&read, Err(ClientError::Unreadable { status: 409, .. })),
            "{decision}: {read:?}"
        );
    }
}

/// A recovery answer with `field` replaced by `value`.
fn recovery_replacing(field: &str, value: &Value) -> String {
    let mut answer: Value =
        serde_json::from_str(&recovery_with(&json!({ "rejected": "z_untrusted" })))
            .expect("a recovery answer is a document");
    answer[field] = value.clone();
    answer.to_string()
}

/// Recovery answers the contracts forbid, each refused under `409` through
/// `recover_fan`: a recovery action missing its parameter, naming no declared
/// action, or carrying a parameter of the wrong type; a refusal no vocabulary
/// declares, or carrying a printer state no vocabulary names; and an event
/// entry of no declared shape. The same documents the Python and Node clients'
/// suites hold their readers to.
#[test]
fn a_recovery_refusal_no_contract_declares_is_unreadable() {
    let malformed = [
        recovery_replacing("action", &json!({ "action": "recover_lift" })),
        recovery_replacing("action", &json!({ "action": "recover_dance" })),
        recovery_replacing(
            "action",
            &json!({ "action": "recover_fan", "percent": "lots" }),
        ),
        recovery_with(&json!({ "rejected": "a_newer_reason" })),
        recovery_with(&json!({ "rejected": { "a_newer_reason": { "anything": 1 } } })),
        recovery_with(&json!({ "rejected": { "print_running": { "state": "melting" } } })),
        recovery_replacing("events", &json!([{ "kind": 7 }])),
    ];
    for answer in malformed {
        let host = Host::answering(409, &answer);
        let read = Client::new(host.address(), Actor::Operator).recover_fan(50.0, "because");
        assert!(
            matches!(&read, Err(ClientError::Unreadable { status: 409, .. })),
            "{answer}: {read:?}"
        );
    }
}

/// A spool command's refusal, around one spool refusal cause.
fn spool_refusal(cause: &Value) -> String {
    json!({
        "decision": { "rejected": { "spool_refused": { "cause": cause, "message": "refused" } } },
        "events": [],
    })
    .to_string()
}

/// A spool refusal arrives through `retire_spool` as a refusal only when its
/// cause is one the contracts declare, carrying what that cause carries.
#[test]
fn a_spool_refusal_is_read_only_with_a_cause_the_contracts_declare() {
    let spool = "0198f0a1-2b3c-7d4e-8f90-123456789abc";
    let host = Host::answering(
        409,
        &spool_refusal(&json!({ "cause": "unknown_spool", "spool_id": spool })),
    );
    let refused = Client::new(host.address(), Actor::Operator).retire_spool(spool, "because");
    assert!(
        matches!(&refused, Err(ClientError::Rejected(_))),
        "{refused:?}"
    );

    for cause in [
        json!({ "cause": "melted" }),
        json!({ "cause": "unknown_spool" }),
    ] {
        let host = Host::answering(409, &spool_refusal(&cause));
        let read = Client::new(host.address(), Actor::Operator).retire_spool(spool, "because");
        assert!(
            matches!(&read, Err(ClientError::Unreadable { status: 409, .. })),
            "{cause}: {read:?}"
        );
    }
}
