//! The served route set, held to the contracts rather than to a list here.
//!
//! The required operation set is **derived from the contracts' own
//! `PrintAction`** — one operation per variant it declares, read out of the
//! schema that type generates in this process — plus the six reads this server
//! is required to serve. Growing the declared list and the routes together
//! cannot satisfy this, because the side it is compared against is another
//! crate's closed type.
//!
//! Coverage is not the whole of the contract, so [`audit`] also rules on the
//! shape every route is served under: one versioned prefix for all of them, a
//! JSON body where a body is taken at all, and a JSON answer. Every one of its
//! refusals is driven against a doctored set beside the served one it accepts,
//! so a check that reported nothing whatever it was handed cannot pass.

use std::collections::BTreeSet;

use printobserver_server::{Effect, MEDIA_TYPE, Method, OPERATIONS, Operation, READS};
use printobserver_types::PrintAction;
use printobserver_types::serde_json::{Value, json};

use crate::world::World;

/// One route as the server serves it: the whole path, and its two media types.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Served {
    /// The operation's own name.
    name: String,
    /// The method it is reached by.
    method: Method,
    /// Its whole path, prefix included.
    path: String,
    /// The media type it takes a body in, when it takes one.
    accepts: Option<String>,
    /// The media type it answers in.
    answers: String,
}

impl Served {
    /// The route one declared operation is served as.
    fn of(operation: &Operation) -> Self {
        Self {
            name: operation.name.to_owned(),
            method: operation.method,
            path: operation.full_path(),
            accepts: operation.accepts.map(str::to_owned),
            answers: operation.answers.to_owned(),
        }
    }
}

/// The route set this server serves, read off its own declaration.
fn served() -> Vec<Served> {
    OPERATIONS.iter().map(Served::of).collect()
}

/// One operation per variant the contracts' own `PrintAction` declares, plus
/// the six reads this server is required to serve.
///
/// Read out of the schema that type generates rather than out of a list here,
/// so a variant added to the contracts enters this set the moment it is added.
fn required() -> BTreeSet<String> {
    let schema = printobserver_types::contract::schema_of::<PrintAction>();
    let mut found: BTreeSet<String> = schema["oneOf"]
        .as_array()
        .expect("the action vocabulary is a union")
        .iter()
        .map(|variant| {
            variant["properties"]["action"]["const"]
                .as_str()
                .expect("every variant of the vocabulary is tagged with its own name")
                .to_owned()
        })
        .collect();
    assert!(
        found.len() >= 10,
        "the contracts declare only {} actions, which is not the vocabulary",
        found.len()
    );
    found.extend(READS.iter().map(|name| (*name).to_owned()));
    found
}

/// Every way a served set can disagree with what this server is required to be.
fn audit(served: &[Served], required: &BTreeSet<String>) -> Vec<String> {
    let mut findings = Vec::new();
    let names: BTreeSet<String> = served.iter().map(|route| route.name.clone()).collect();

    findings.extend(
        served
            .iter()
            .filter(|route| !required.contains(&route.name))
            .map(|route| {
                format!(
                    "`{} {}` serves `{}`, which is no operation this server is required \
                     to have",
                    route.method, route.path, route.name
                )
            }),
    );
    findings.extend(
        required
            .iter()
            .filter(|name| !names.contains(*name))
            .map(|name| format!("`{name}` is an operation with no served route")),
    );

    let prefixes: BTreeSet<&str> = served
        .iter()
        .map(|route| route.path.split('/').nth(1).map_or("", |segment| segment))
        .collect();
    if prefixes.len() > 1 {
        let spellings: Vec<String> = prefixes.iter().map(|prefix| format!("/{prefix}")).collect();
        findings.push(format!(
            "two versioned prefixes are in use at once: {}",
            spellings.join(" and ")
        ));
    }
    for route in served {
        if !route.path.starts_with(printobserver_server::VERSION_PREFIX) {
            findings.push(format!(
                "`{}` is served at {}, outside the one versioned prefix {}",
                route.name,
                route.path,
                printobserver_server::VERSION_PREFIX
            ));
        }
        if route
            .accepts
            .as_deref()
            .is_some_and(|media| media != MEDIA_TYPE)
        {
            findings.push(format!(
                "`{}` takes a body as {:?} rather than as {MEDIA_TYPE}",
                route.name, route.accepts
            ));
        }
        if route.answers != MEDIA_TYPE {
            findings.push(format!(
                "`{}` answers as {} rather than as {MEDIA_TYPE}",
                route.name, route.answers
            ));
        }
    }
    findings
}

/// The served set is exactly one route per operation, under one prefix, in JSON.
#[test]
fn the_served_routes_are_exactly_the_operations_the_contracts_declare() {
    assert_eq!(
        audit(&served(), &required()),
        Vec::<String>::new(),
        "the served route set is not the operation set the contracts declare"
    );
    assert_eq!(
        served().len(),
        required().len(),
        "the served set and the required set are the same names in different numbers"
    );
}

/// The audit refuses each way a served set can be wrong.
///
/// Driven against the served set with one thing changed in it, so what refuses
/// the fixture is what reads the real pair.
#[test]
fn the_audit_refuses_each_way_a_served_set_can_be_wrong() {
    let required = required();

    let mut extra = served();
    extra.push(Served {
        name: "reboot".to_owned(),
        method: Method::Post,
        path: "/v1/prints/{print_id}/actions/reboot".to_owned(),
        accepts: Some(MEDIA_TYPE.to_owned()),
        answers: MEDIA_TYPE.to_owned(),
    });
    refuses(
        &audit(&extra, &required),
        "is no operation this server is required",
    );

    let mut missing = served();
    missing.retain(|route| route.name != "pause");
    refuses(
        &audit(&missing, &required),
        "`pause` is an operation with no served route",
    );

    let mut grown = required.clone();
    grown.insert("reboot".to_owned());
    refuses(
        &audit(&served(), &grown),
        "`reboot` is an operation with no served route",
    );

    let mut two_prefixes = served();
    two_prefixes[0].path = two_prefixes[0].path.replace("/v1/", "/v2/");
    refuses(&audit(&two_prefixes, &required), "two versioned prefixes");

    let mut unprefixed = served();
    unprefixed[0].path = unprefixed[0].path.replace("/v1", "");
    refuses(
        &audit(&unprefixed, &required),
        "outside the one versioned prefix",
    );

    let mut form_encoded = served();
    let index = form_encoded
        .iter()
        .position(|route| route.accepts.is_some())
        .expect("some operation takes a body");
    form_encoded[index].accepts = Some("application/x-www-form-urlencoded".to_owned());
    refuses(&audit(&form_encoded, &required), "takes a body as");

    let mut html = served();
    html[0].answers = "text/html".to_owned();
    refuses(&audit(&html, &required), "answers as text/html");
}

/// Fail unless some finding names what was expected.
fn refuses(findings: &[String], naming: &str) {
    assert!(
        findings.iter().any(|finding| finding.contains(naming)),
        "expected a finding naming {naming:?}; got {findings:?}"
    );
}

/// Every declared route is really served, and answers JSON.
///
/// The declaration and the router are one thing by construction — the router is
/// built by folding over the declaration — and this is what proves the fold
/// reached the network: each route is driven over HTTP and answers something
/// other than "there is no such route".
#[tokio::test(flavor = "multi_thread")]
async fn every_declared_route_is_served_and_answers_json() {
    let world = World::open().await;
    let print_id = world.open_print().await;

    for operation in OPERATIONS {
        let url = world.operation_url(&operation.full_path(), print_id);
        // `world.get`, `world.post` and `world.put` refuse an answer that is
        // not JSON, and a path no route is served at answers nothing at all —
        // so reaching a JSON answer at every declared path is what says the
        // fold from the declaration to the router reached the network.
        let (status, _) = match operation.method {
            Method::Get => world.get(&url.replace("{image_id}", &image_id())).await,
            Method::Post => world.post(&url, &body_for(&operation)).await,
            Method::Put => world.put(&url, &manifest()).await,
        };
        assert_ne!(
            status,
            reqwest::StatusCode::METHOD_NOT_ALLOWED,
            "`{}` is declared as {} and `{url}` does not take it",
            operation.name,
            operation.method
        );
    }

    // A path this server declares no operation for is served by nothing, which
    // is the other half of "one route per operation and no more".
    let (status, media, body) = world.raw_get(&world.url("/prints/reboot")).await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
    assert!(
        !media.starts_with(MEDIA_TYPE) && body.is_empty(),
        "a path this server declares no operation for reached a handler: {media} {body}"
    );
    world.server.stop().await;
}

/// What a running server says about itself carries no secret.
///
/// A long-running host writes down what it came up as, and the two things that
/// would be worst to find in that record are the `OctoPrint` key and the
/// ingress secret. Neither is in any of these renderings.
#[tokio::test(flavor = "multi_thread")]
async fn what_a_running_server_says_about_itself_carries_no_secret() {
    let world = World::open().await;
    let state = printobserver_server::ApiState {
        supervisor: std::sync::Arc::clone(world.server.supervisor()),
        store: std::sync::Arc::clone(world.server.store()),
    };
    let rendered = format!(
        "{state:?} {:?} {:?}",
        world.server,
        printobserver_server::Ports {
            printer: std::sync::Arc::clone(&world.printer)
                as std::sync::Arc<dyn printobserver_printer_api::PrinterPort>,
            store: std::sync::Arc::clone(world.server.store()),
            vision: std::sync::Arc::new(
                printobserver_obico::ObicoVision::new(
                    printobserver_obico::ObicoVisionConfig::default()
                )
                .expect("the adapter is built")
            ),
            agent: std::sync::Arc::clone(&world.agent)
                as std::sync::Arc<dyn printobserver_supervisor_api::SupervisorPort>,
        }
    );

    assert!(
        !rendered.contains(crate::world::SECRET),
        "a rendering of this server carries the ingress secret: {rendered}"
    );
    assert!(
        rendered.contains("ApiState") && rendered.contains("Running") && rendered.contains("Ports"),
        "a rendering of this server says nothing about what it is: {rendered}"
    );
    world.server.stop().await;
}

/// A route that takes a body refuses one that is not JSON.
#[tokio::test(flavor = "multi_thread")]
async fn a_body_that_is_not_json_is_refused() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let operation = printobserver_server::operation("pause").expect("pause is served");

    let response = world
        .client
        .post(world.operation_url(&operation.full_path(), print_id))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body("reason=because")
        .send()
        .await
        .expect("the server answers");

    assert_eq!(
        response.status(),
        reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "a body that is not JSON was taken"
    );
    world.server.stop().await;
}

/// An identifier of an image nothing holds, spelled the one way this system
/// spells one.
fn image_id() -> String {
    printobserver_types::ImageId::new().to_string()
}

/// A manifest a write can carry.
fn manifest() -> Value {
    printobserver_types::serde_json::to_value(
        <printobserver_types::JobManifest as printobserver_types::contract::Sample>::sample_full(),
    )
    .expect("a manifest renders")
}

/// A body one mutating operation takes, carrying whatever that action needs.
fn body_for(operation: &Operation) -> Value {
    let Effect::Mutating(kind) = operation.effect else {
        return json!({});
    };
    let mut body = json!({
        "reason": "a journey is asking",
        "actor": "operator",
    });
    let object = body.as_object_mut().expect("the body is an object");
    match kind {
        printobserver_types::ActionKind::StartPrint => {
            object.insert("file_name".to_owned(), json!("benchy.gcode"));
            object.insert("manifest".to_owned(), manifest());
        }
        printobserver_types::ActionKind::SetFeedrateFactor
        | printobserver_types::ActionKind::SetFlowrateFactor => {
            object.insert("factor".to_owned(), json!(1.0));
        }
        printobserver_types::ActionKind::SetToolTargetC => {
            object.insert("tool".to_owned(), json!(0));
            object.insert("target_c".to_owned(), json!(215.0));
        }
        printobserver_types::ActionKind::SetBedTargetC => {
            object.insert("target_c".to_owned(), json!(60.0));
        }
        printobserver_types::ActionKind::SetFanPercent => {
            object.insert("percent".to_owned(), json!(40.0));
        }
        printobserver_types::ActionKind::AcknowledgeFailure => {
            object.insert(
                "event_id".to_owned(),
                json!(printobserver_types::EventId::new()),
            );
            object.insert("disposition".to_owned(), json!("continue"));
        }
        _ => {}
    }
    body
}
