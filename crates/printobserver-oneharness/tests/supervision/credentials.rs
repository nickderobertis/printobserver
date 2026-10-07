//! A turn's runs are handed the credential issued for them through their
//! environment alone.
//!
//! Driven through `OneHarness`'s own run path: what is asserted is the run
//! request the port built — the environment the harness process is started
//! with — and the state directory the turn left behind.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use printobserver_supervisor_api::{
    CREDENTIAL_ENV, SERVER_ENV, SupervisorError, SupervisorPort, TurnAccess, TurnPass,
};
use printobserver_types::PrintId;

use crate::support::{
    Fixture, HARNESS, ISSUED_SERVER, OTHER_HARNESS, RecordingAccess, Watch, always, assessment,
    block_on, config, event, generated_assessment_schema, port, schema_read_lock, turn, unreadable,
};

/// The value one variable is given in a run request's environment.
fn assigned<'a>(env: &'a [String], name: &str) -> Vec<&'a str> {
    env.iter()
        .filter_map(|line| line.strip_prefix(&format!("{name}=")))
        .collect()
}

/// Every file under a directory whose bytes carry `needle`.
fn carrying(root: &Path, needle: &str) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(carrying(&path, needle));
        } else if fs::read(&path).is_ok_and(|bytes| {
            bytes
                .windows(needle.len())
                .any(|at| at == needle.as_bytes())
        }) {
            found.push(path.display().to_string());
        }
    }
    found
}

/// The run carries the server's address and the credential issued for the
/// session it runs in, once each, and the context command names no file.
#[test]
fn a_run_is_handed_its_own_credential_through_its_environment_alone() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("credentials-env");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            always("SID-CREDENTIAL", &assessment("the print is fine", "high")),
        ),
        &watch,
    );
    let access = Arc::new(RecordingAccess::default());
    let print_id = PrintId::new();

    block_on(supervisor.run_turn(
        turn(print_id, event(print_id, unreadable("credential")), None),
        Arc::clone(&access) as Arc<dyn TurnAccess>,
    ))
    .expect("the turn runs");

    let issued = access.issued();
    assert_eq!(
        issued,
        vec![(
            format!("print-{print_id}"),
            "a-turn-credential-1".to_owned()
        )],
        "the run was not issued one credential for the session it ran in"
    );
    let request = watch.requests().into_iter().next().expect("a run request");
    assert_eq!(assigned(&request.env, SERVER_ENV), vec![ISSUED_SERVER]);
    assert_eq!(
        assigned(&request.env, CREDENTIAL_ENV),
        vec!["a-turn-credential-1"]
    );
    let prompt = request.prompt.join("\n");
    let context = prompt
        .lines()
        .find(|line| line.starts_with("printobserver context "))
        .expect("the prompt hands the turn its context command");
    assert!(
        !context.contains("--config"),
        "the turn's context command names a configuration file: {context}"
    );
    assert!(
        carrying(&fixture.state_dir(), "a-turn-credential-1").is_empty(),
        "the turn wrote its credential under the state directory: {:?}",
        carrying(&fixture.state_dir(), "a-turn-credential-1")
    );
}

/// A turn the harness moves to a new session is issued a new credential for
/// it, and the run in that session carries the new one rather than the first.
#[test]
fn a_turn_moved_to_a_new_session_is_issued_that_sessions_credential() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("credentials-moved");
    let schema = generated_assessment_schema();
    let print_id = PrintId::new();
    let answering = || {
        always(
            "SID-MOVED-CREDENTIAL",
            &assessment("the print is fine", "high"),
        )
    };
    let first = port(
        config(&schemas, &fixture, HARNESS, &schema, answering()),
        &Arc::new(Watch::default()),
    );
    block_on(first.run_turn(
        turn(print_id, event(print_id, unreadable("first")), None),
        crate::support::access(),
    ))
    .expect("the first turn runs");
    drop(first);

    let watch = Arc::new(Watch::default());
    let moved = port(
        config(&schemas, &fixture, OTHER_HARNESS, &schema, answering()),
        &watch,
    );
    let access = Arc::new(RecordingAccess::default());
    block_on(moved.run_turn(
        turn(print_id, event(print_id, unreadable("second")), None),
        Arc::clone(&access) as Arc<dyn TurnAccess>,
    ))
    .expect("the moved turn runs");

    assert_eq!(
        access.issued(),
        vec![
            (
                format!("print-{print_id}"),
                "a-turn-credential-1".to_owned()
            ),
            (
                format!("print-{print_id}-2"),
                "a-turn-credential-2".to_owned()
            ),
        ]
    );
    let credentials: Vec<Vec<String>> = watch
        .requests()
        .iter()
        .map(|request| {
            assigned(&request.env, CREDENTIAL_ENV)
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .collect();
    assert_eq!(
        credentials,
        vec![
            vec!["a-turn-credential-1".to_owned()],
            vec!["a-turn-credential-2".to_owned()],
        ]
    );
}

/// Access that will issue nothing.
struct Refusing;

impl TurnAccess for Refusing {
    fn issue(&self, _session_name: &str) -> Result<TurnPass, SupervisorError> {
        Err(SupervisorError::Unavailable {
            detail: "no credential could be minted".to_owned(),
        })
    }
}

/// A run nothing could be issued a credential for is not started at all.
#[test]
fn a_run_with_no_credential_is_never_started() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("credentials-refused");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            always("SID-REFUSED", &assessment("the print is fine", "high")),
        ),
        &watch,
    );
    let print_id = PrintId::new();

    let refused = block_on(supervisor.run_turn(
        turn(print_id, event(print_id, unreadable("credential")), None),
        Arc::new(Refusing),
    ))
    .expect_err("a turn with no credential does not run");

    assert_eq!(
        refused,
        SupervisorError::Unavailable {
            detail: "no credential could be minted".to_owned()
        }
    );
    assert!(watch.requests().is_empty(), "a run was started anyway");
}

/// A configuration that assigns either variable itself is overridden: the run
/// sees the pass it was issued, once each, and nothing configured beside it.
#[test]
fn a_configured_server_or_credential_never_reaches_a_run() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("credentials-configured");
    let watch = Arc::new(Watch::default());
    let mut configured = config(
        &schemas,
        &fixture,
        HARNESS,
        &generated_assessment_schema(),
        always("SID-CONFIGURED", &assessment("the print is fine", "high")),
    );
    configured.harness_env.push(crate::support::assignment(
        "PRINTOBSERVER_CREDENTIAL=a-stale-operator-credential",
    ));
    configured.harness_env.push(crate::support::assignment(
        "PRINTOBSERVER_SERVER=http://127.0.0.1:9",
    ));
    let supervisor = port(configured, &watch);
    let print_id = PrintId::new();

    block_on(supervisor.run_turn(
        turn(print_id, event(print_id, unreadable("configured")), None),
        crate::support::access(),
    ))
    .expect("the turn runs");

    let request = watch.requests().into_iter().next().expect("a run request");
    assert_eq!(
        assigned(&request.env, CREDENTIAL_ENV),
        vec!["a-turn-credential-1"]
    );
    assert_eq!(assigned(&request.env, SERVER_ENV), vec![ISSUED_SERVER]);
}
