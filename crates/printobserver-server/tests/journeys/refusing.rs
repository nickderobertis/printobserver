//! Every way this server will not come up, and what it says about each.
//!
//! The configuration walk covers the values a *field* can carry wrongly. These
//! are the refusals that are about the host rather than about a field: a port
//! already taken, a state directory that will not hold a store, an instance
//! that refuses the key it was given. Each of them is a supervisor that would
//! otherwise have come up half-working, and what each answers is the thing an
//! operator has to change.

use printobserver_oneharness::ModelName;
use printobserver_server::{ConfigField, Server, StartError};
use tempfile::TempDir;

use crate::probes::{base_url, refusing_host, silent_host};
use crate::world::{document, set, write};

/// One configuration, with one value changed, started.
async fn started_with(
    root: &std::path::Path,
    octoprint: &str,
    change: impl FnOnce(&mut toml::Value),
) -> Result<printobserver_server::Running, StartError> {
    let mut document = document(root, octoprint);
    change(&mut document);
    let path = write(root, &document);
    Server::start(&path).await
}

/// An address this adapter does not speak to is refused where it is configured.
#[tokio::test(flavor = "multi_thread")]
async fn an_address_this_adapter_does_not_speak_to_is_refused() {
    let root = TempDir::new().expect("a journey's own root");
    let started = started_with(root.path(), "http://127.0.0.1:1", |document| {
        set(
            document,
            "octoprint.url",
            toml::Value::String("https://octoprint.example".to_owned()),
        );
    })
    .await;

    let Err(refusal) = started else {
        panic!("a URL this adapter does not speak was accepted");
    };
    assert_eq!(refusal.field(), Some(ConfigField::OctoprintUrl));
    assert!(
        refusal.to_string().contains("https://octoprint.example"),
        "the refusal does not name the URL: {refusal}"
    );
}

/// An instance that refuses the key refuses the start, naming the key.
#[tokio::test(flavor = "multi_thread")]
async fn an_instance_that_refuses_the_key_refuses_the_start() {
    let refusing = refusing_host().await;
    let root = TempDir::new().expect("a journey's own root");

    let started = started_with(root.path(), &base_url(&refusing), |_| {}).await;

    let Err(refusal) = started else {
        panic!("an instance that refuses the key let the server start");
    };
    assert_eq!(refusal.field(), Some(ConfigField::OctoprintApiKey));
    assert!(
        refusal.to_string().contains("refused it"),
        "the refusal does not say the instance refused the key: {refusal}"
    );
}

/// An address already taken is refused naming it.
#[tokio::test(flavor = "multi_thread")]
async fn an_address_already_taken_is_refused_naming_it() {
    let answering = silent_host().await;
    let taken = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = taken.local_addr().expect("the bound address");
    let root = TempDir::new().expect("a journey's own root");

    let started = started_with(root.path(), &base_url(&answering), |document| {
        set(document, "listen", toml::Value::String(address.to_string()));
    })
    .await;

    let Err(refusal) = started else {
        panic!("a server came up on an address already taken");
    };
    assert!(
        matches!(refusal, StartError::Listen { .. }),
        "the refusal is not about the address: {refusal}"
    );
    assert!(
        refusal.to_string().contains(&address.to_string()),
        "the refusal does not name the address: {refusal}"
    );
}

/// A state directory that will not hold a store is refused naming the store.
#[tokio::test(flavor = "multi_thread")]
async fn a_state_directory_that_will_not_hold_a_store_is_refused() {
    use std::os::unix::fs::PermissionsExt as _;

    let answering = silent_host().await;
    let root = TempDir::new().expect("a journey's own root");
    let state = root.path().join("state");
    std::fs::create_dir_all(&state).expect("a state directory");
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o500))
        .expect("the state directory is unwritable");

    let started = started_with(root.path(), &base_url(&answering), |_| {}).await;

    // Put it back before asserting, so a failing journey still leaves a
    // directory its own scratch root can remove.
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .expect("the state directory is writable again");

    let Err(refusal) = started else {
        panic!("a server came up over a state directory that cannot hold a store");
    };
    assert!(
        matches!(refusal, StartError::Store { .. }),
        "the refusal is not about the store: {refusal}"
    );
}

/// A state directory that will not hold the agent's assets is refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_state_directory_that_will_not_hold_the_agents_assets_is_refused() {
    let answering = silent_host().await;
    let root = TempDir::new().expect("a journey's own root");
    let state = root.path().join("state");
    std::fs::create_dir_all(&state).expect("a state directory");
    // The agent's skill, template and schema are materialized under here, and
    // a file where that directory belongs is a host that cannot hold them.
    std::fs::write(
        state.join(printobserver_server::ASSETS_DIRECTORY),
        b"not a directory",
    )
    .expect("a file is writable");

    let started = started_with(root.path(), &base_url(&answering), |_| {}).await;

    let Err(refusal) = started else {
        panic!("a server came up with nowhere to put the agent's own assets");
    };
    assert!(
        matches!(refusal, StartError::State { .. }),
        "the refusal is not about the state directory: {refusal}"
    );
    assert!(
        refusal
            .to_string()
            .contains(printobserver_server::ASSETS_DIRECTORY),
        "the refusal does not name what could not be put in place: {refusal}"
    );
}

/// Every refusal says which it is, in its own words.
#[test]
fn every_refusal_says_which_it_is_in_its_own_words() {
    let refusals = [
        StartError::State {
            detail: "the disk is full".to_owned(),
        },
        StartError::Listen {
            address: "127.0.0.1:8420".parse().expect("an address"),
            detail: "already in use".to_owned(),
        },
        StartError::Store {
            detail: "the disk is full".to_owned(),
        },
        StartError::Supervisor {
            detail: "no such harness".to_owned(),
        },
        StartError::Reconciliation {
            detail: "the disk is full".to_owned(),
        },
    ];
    for refusal in refusals {
        assert!(
            !refusal.to_string().is_empty(),
            "a refusal said nothing: {refusal:?}"
        );
        assert_eq!(
            refusal.field(),
            None,
            "a refusal that is not about a field named one: {refusal}"
        );
    }

    let unreadable = printobserver_server::ConfigError::Unreadable {
        path: "/etc/printobserver/config.toml".into(),
        detail: "no such file".to_owned(),
    };
    assert!(unreadable.to_string().contains("config.toml"));
    assert_eq!(unreadable.field(), None);
    let unparsable = printobserver_server::ConfigError::Unparsable {
        path: "/etc/printobserver/config.toml".into(),
        detail: "expected a value".to_owned(),
    };
    assert!(unparsable.to_string().contains("config.toml"));
    assert_eq!(unparsable.field(), None);
    assert_eq!(
        StartError::from(unparsable).field(),
        None,
        "a document that is not this configuration is about no one field"
    );
}

/// Every field this program takes is spelled once, and spells itself.
#[test]
fn every_field_is_spelled_once_and_spells_itself() {
    let mut spellings: Vec<&str> = ConfigField::ALL.iter().map(|field| field.key()).collect();
    let declared = spellings.len();
    spellings.sort_unstable();
    spellings.dedup();
    assert_eq!(
        spellings.len(),
        declared,
        "two fields are spelled the same way"
    );
    for field in ConfigField::ALL {
        assert_eq!(field.to_string(), field.key());
    }
}

/// A server nobody stopped stops when the handle to it goes.
///
/// A supervisor whose serving task outlived its handle would be a process that
/// went on answering the API after the thing that owned it had gone — and a
/// tier that leaked one per journey would eventually be answering on a hundred
/// ports at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_server_nobody_stopped_stops_when_its_handle_goes() {
    let answering = silent_host().await;
    let root = TempDir::new().expect("a journey's own root");
    let running = started_with(root.path(), &base_url(&answering), |_| {})
        .await
        .expect("the server starts");
    let address = running.address();
    assert!(
        tokio::net::TcpStream::connect(address).await.is_ok(),
        "the server is not answering on the address it took"
    );

    drop(running);

    let deadline = std::time::Instant::now() + core::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if tokio::net::TcpStream::connect(address).await.is_err() {
            return;
        }
        tokio::time::sleep(core::time::Duration::from_millis(50)).await;
    }
    panic!("{address} was still answering after the handle to the server had gone");
}

/// A skill and a template the operator supplied are what the agent is built
/// with.
///
/// Editing either on a running host is a restart rather than a rebuild, which
/// is why they are paths; a configuration that names neither runs the ones this
/// program carries, and one that names them runs the operator's.
#[tokio::test(flavor = "multi_thread")]
async fn a_skill_and_a_template_the_operator_supplied_are_what_is_used() {
    let answering = silent_host().await;
    let root = TempDir::new().expect("a journey's own root");
    let skill = root.path().join("our-own-skill.md");
    let template = root.path().join("our-own-prompt.md");
    std::fs::write(&skill, b"# Our own skill\n").expect("a skill is writable");
    // A template declares the three slots one turn fills, and this is the
    // committed one, so what is changed is where it is read from.
    std::fs::write(&template, printobserver_server::TURN_PROMPT).expect("a template is writable");

    let running = started_with(root.path(), &base_url(&answering), |document| {
        set(
            document,
            "supervisor.skill_path",
            toml::Value::String(skill.display().to_string()),
        );
        set(
            document,
            "supervisor.prompt_template_path",
            toml::Value::String(template.display().to_string()),
        );
        set(
            document,
            "supervisor.model",
            toml::Value::String("claude-opus-5".to_owned()),
        );
    })
    .await
    .expect("a server runs on the skill and the template the operator supplied");

    assert_eq!(
        running.config().skill_path.as_deref(),
        Some(skill.as_path())
    );
    assert_eq!(
        running.config().prompt_template_path.as_deref(),
        Some(template.as_path())
    );
    assert_eq!(
        running.config().model.as_ref().map(ModelName::as_str),
        Some("claude-opus-5")
    );
    // The one this program carries is written into the state directory only
    // when the operator supplied none.
    assert!(
        !running
            .config()
            .assets_dir()
            .join(printobserver_server::SKILL_FILE)
            .exists(),
        "a skill the operator supplied was replaced by the one this program carries"
    );
    running.stop().await;
}

/// A template that is not a template refuses the start, in the harness's words.
#[tokio::test(flavor = "multi_thread")]
async fn a_template_that_is_not_a_template_refuses_the_start() {
    let answering = silent_host().await;
    let root = TempDir::new().expect("a journey's own root");
    let template = root.path().join("not-a-template.md");
    std::fs::write(&template, b"this names none of the slots a turn fills\n")
        .expect("a file is writable");

    let started = started_with(root.path(), &base_url(&answering), |document| {
        set(
            document,
            "supervisor.prompt_template_path",
            toml::Value::String(template.display().to_string()),
        );
    })
    .await;

    let Err(refusal) = started else {
        panic!("a server came up on a file that fills no turn");
    };
    assert!(
        matches!(refusal, StartError::Supervisor { .. }),
        "the refusal is not about the supervising agent: {refusal}"
    );
    assert!(
        refusal.to_string().contains("not-a-template.md"),
        "the refusal does not name the file: {refusal}"
    );
}

/// An assets directory nothing can be written into refuses the start.
#[tokio::test(flavor = "multi_thread")]
async fn an_assets_directory_nothing_can_be_written_into_refuses_the_start() {
    use std::os::unix::fs::PermissionsExt as _;

    let answering = silent_host().await;
    let root = TempDir::new().expect("a journey's own root");
    let assets = root
        .path()
        .join("state")
        .join(printobserver_server::ASSETS_DIRECTORY);
    std::fs::create_dir_all(&assets).expect("an assets directory");
    std::fs::set_permissions(&assets, std::fs::Permissions::from_mode(0o500))
        .expect("the assets directory is unwritable");

    let started = started_with(root.path(), &base_url(&answering), |_| {}).await;

    std::fs::set_permissions(&assets, std::fs::Permissions::from_mode(0o700))
        .expect("the assets directory is writable again");

    let Err(refusal) = started else {
        panic!("a server came up with nowhere to write the agent's own assets");
    };
    assert!(
        matches!(refusal, StartError::State { .. }),
        "the refusal is not about the state directory: {refusal}"
    );
}

/// One way a value can be written wrongly: what it is, the field it is about,
/// what the refusal has to say, and the change that makes it so.
struct Wrongly {
    /// What the case is, for a reader of a failure message.
    described: &'static str,
    /// The field the refusal must name.
    field: ConfigField,
    /// What the refusal must say.
    saying: &'static str,
    /// The one value it changes.
    change: Box<dyn Fn(&mut toml::Value)>,
}

/// Every value the safety envelope and the ingress bound refuse is refused
/// where it is configured.
///
/// The field walk gives each field one unacceptable value of its own kind;
/// these are the other ways the same two fields can be written wrongly, and
/// each of them is a supervisor that would otherwise have come up granting
/// nothing, or answering an alert Obico had already abandoned.
#[tokio::test(flavor = "multi_thread")]
async fn every_other_way_the_envelope_and_the_bound_can_be_wrong_is_refused() {
    let answering = silent_host().await;
    let base = base_url(&answering);
    let cases = [
        Wrongly {
            described: "an envelope allowing nothing",
            field: ConfigField::SafetyEnvelope,
            saying: "no adjustable at all",
            change: Box::new(|document: &mut toml::Value| {
                set(
                    document,
                    "safety.allowed",
                    toml::Value::Table(toml::Table::new()),
                );
            }),
        },
        Wrongly {
            described: "an agent interval that is not a count of seconds",
            field: ConfigField::SafetyEnvelope,
            saying: "count of seconds",
            change: Box::new(|document: &mut toml::Value| {
                set(
                    document,
                    "safety.agent_min_interval_s",
                    toml::Value::Integer(-30),
                );
            }),
        },
        Wrongly {
            described: "an answer bound of zero",
            field: ConfigField::IngressAnswerBoundMs,
            saying: "run out of time before it starts",
            change: Box::new(|document: &mut toml::Value| {
                set(document, "ingress.answer_bound_ms", toml::Value::Integer(0));
            }),
        },
    ];

    for case in cases {
        let root = TempDir::new().expect("a journey's own root");
        let started = started_with(root.path(), &base, case.change).await;

        let Err(refusal) = started else {
            panic!("{} was accepted", case.described);
        };
        assert_eq!(
            refusal.field(),
            Some(case.field),
            "{} was refused naming {:?}: {refusal}",
            case.described,
            refusal.field()
        );
        assert!(
            refusal.to_string().contains(case.saying),
            "{} was refused without saying why: {refusal}",
            case.described
        );
    }
}
