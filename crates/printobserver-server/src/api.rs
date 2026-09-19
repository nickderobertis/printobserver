//! The HTTP surface, built by folding over the declared operations.
//!
//! # The router is the declaration
//!
//! Nothing here writes a route out. [`router`] walks
//! [`OPERATIONS`](crate::operations::OPERATIONS) and registers one route per
//! entry beneath the one versioned prefix, so the set served and the set
//! declared cannot come apart — and the ten mutating operations share one
//! handler which is given the [`ActionKind`] its own entry declares, so a
//! request cannot reach an action the operation it was sent to does not name.
//!
//! # Every action takes the same one path
//!
//! An action asked for here goes to
//! [`Supervisor::request_action`](printobserver_core::Supervisor::request_action),
//! which is the same call an operator's action makes and an agent's action
//! makes. The policy is not re-implemented here and is not bypassed here: a
//! rejected request answers the rejection the policy made, carrying its reason,
//! the value asked for and the range allowed, and reaches no action method of
//! the printer port at all.
//!
//! # Nothing is served to a caller that did not present the credential
//!
//! Every route beneath the versioned prefix — and the prefix's own fallback, so
//! that a path nothing serves is refused the same way — sits behind
//! [`authenticate`], which runs before any extractor reads a path, a query or a
//! body. A request whose `Authorization` header is not exactly
//! `Bearer <the credential in force>` is answered `401` there, so it reaches no
//! handler, no store, no printer port and no record. There is no route that
//! skips it and no configuration that turns it off: [`ApiState`] cannot be
//! built without the credential it is checked against.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Router, routing::MethodRouter};
use printobserver_core::store::{
    EventStore, HistoryQuery, ImageStore, PrintStore, SessionStore, StoreError,
};
use printobserver_core::{ActionKind, ExecutionOutcome, PolicyDecision};
use printobserver_core::{CoreError, Supervisor, effective_bounds};
use printobserver_types::serde::Deserialize;
use printobserver_types::{ImageId, PrintId};

use crate::config::ApiCredential;
use crate::operations::{Effect, Method, OPERATIONS, Operation, VERSION_PREFIX};
use crate::wire::{
    ActionAnswer, ActionBody, ContextAnswer, ErrorAnswer, HistoryAnswer, ImageAnswer,
    ManifestAnswer, ManifestBody, PrintsAnswer, StatusAnswer,
};

/// What every handler is given: the supervisor, and the stores the reads
/// answer out of — the four aggregates a route reads, and not the actions,
/// which reach the store through the supervisor alone.
#[derive(Clone)]
pub struct ApiState {
    /// The supervision core, which every action passes through.
    pub supervisor: Arc<Supervisor>,
    /// The print records, their manifests and their narrowings.
    pub prints: Arc<dyn PrintStore>,
    /// The event log.
    pub events: Arc<dyn EventStore>,
    /// The images stored beside the events.
    pub images: Arc<dyn ImageStore>,
    /// The supervision sessions.
    pub sessions: Arc<dyn SessionStore>,
    /// The credential every request must present before anything above is
    /// reached.
    pub credential: Arc<ApiCredential>,
}

impl core::fmt::Debug for ApiState {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("ApiState").finish_non_exhaustive()
    }
}

/// How many events a history read asks for, when it asks for a number.
#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct HistoryParams {
    /// The limit asked for; absent takes the port's own default window.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Every public operation, beneath the one versioned prefix.
///
/// # Panics
///
/// Panics when [`OPERATIONS`] declares an entry this function has no route for,
/// which is a declaration and a router that have come apart — the one state
/// this server must not come up in quietly.
pub fn router(state: ApiState) -> Router {
    let mut api = Router::new();
    for operation in OPERATIONS {
        api = api.route(operation.path, route_for(&operation));
    }
    // The layer is added after every route and the fallback, which is what
    // puts all of them behind it.
    let api = api
        .fallback(nothing_here)
        .layer(from_fn_with_state(
            Arc::clone(&state.credential),
            authenticate,
        ))
        .layer(DefaultBodyLimit::max(BODY_BOUND))
        .with_state(state);
    Router::new().nest(VERSION_PREFIX, api)
}

/// The scheme the credential is presented under.
const BEARER: &[u8] = b"Bearer ";

/// How much of a request's body this server reads, admitted or refused.
///
/// One bound for both: the router's body limit is set from it, so it is what
/// every extractor reads an admitted body to, and what [`authenticate`] drains
/// a refused one to — a body the server would have taken is one it drains,
/// and a larger one is left where it is either way.
pub const BODY_BOUND: usize = 2 * 1024 * 1024;

/// How long [`authenticate`] waits for a refused request's body to arrive.
///
/// A caller that sent a head and then nothing would otherwise hold its refusal
/// open for as long as it liked; after this the refusal goes out over whatever
/// arrived, and the connection ends as it did before there was a drain.
pub const DRAIN_BOUND: Duration = Duration::from_secs(2);

/// Admit a request that presented the credential in force, and refuse every
/// other before anything reads it.
///
/// Exactly one `Authorization` header, spelled `Bearer ` and then the
/// credential. Two headers, another scheme, another spelling of this one, and a
/// credential that is not the one in force are all the same refusal, which says
/// what to present and nothing about what was presented.
///
/// The refusal is decided on the head alone, and the body is then drained, up
/// to [`BODY_BOUND`], before it is answered. Nothing reads what is
/// drained: a connection closed with a body still unread on it is closed with
/// a reset rather than an end, and a caller on Windows — where a reset discards
/// everything received and not yet read — then reads the abort in place of the
/// `401` that was already on its way. Linux hands over what was queued first,
/// which is why the same race is only ever seen there.
pub async fn authenticate(
    State(credential): State<Arc<ApiCredential>>,
    request: Request,
    next: Next,
) -> Response {
    let mut presented = request.headers().get_all(header::AUTHORIZATION).iter();
    let admitted = match (presented.next(), presented.next()) {
        (Some(only), None) => only
            .as_bytes()
            .strip_prefix(BEARER)
            .is_some_and(|offered| credential.admits(offered)),
        _ => false,
    };
    if !admitted {
        let mut refused = refusal(
            StatusCode::UNAUTHORIZED,
            "this server's API requires the credential it is configured with, presented as \
             `Authorization: Bearer <credential>`",
        );
        refused.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            header::HeaderValue::from_static("Bearer"),
        );
        // A body past the bound, or one that has not arrived inside
        // `DRAIN_BOUND`, is left where it is: the refusal is the answer either
        // way, and what a caller then meets on the connection is its own doing.
        let _ = tokio::time::timeout(
            DRAIN_BOUND,
            axum::body::to_bytes(request.into_body(), BODY_BOUND),
        )
        .await;
        return refused;
    }
    next.run(request).await
}

/// A path beneath the versioned prefix that no operation serves.
async fn nothing_here() -> Response {
    refusal(
        StatusCode::NOT_FOUND,
        "no operation of this server is served at that path",
    )
}

/// The route one declared operation is served by.
fn route_for(operation: &Operation) -> MethodRouter<ApiState> {
    if let Effect::Mutating(kind) = operation.effect {
        return post(
            move |state: State<ApiState>, print_id: Path<PrintId>, body: Json<ActionBody>| async move {
                act(kind, state, print_id, body).await
            },
        );
    }
    match (operation.name, operation.method) {
        ("prints", Method::Get) => get(list_and_adopt_prints),
        ("status", Method::Get) => get(status),
        ("context", Method::Get) => get(context),
        ("image", Method::Get) => get(image),
        ("history", Method::Get) => get(history),
        ("manifest_get", Method::Get) => get(manifest_get),
        ("manifest_set", Method::Put) => put(manifest_set),
        (name, method) => panic!(
            "the operation `{name}` is declared as {method} and this router serves no such route"
        ),
    }
}

/// One answer, rendered as JSON under the media type every operation answers in.
fn answer<T: printobserver_types::serde::Serialize>(status: StatusCode, body: &T) -> Response {
    let rendered = match printobserver_types::serde_json::to_vec(body) {
        Ok(bytes) => bytes,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, crate::operations::MEDIA_TYPE)],
                format!("{{\"error\":\"{error}\"}}"),
            )
                .into_response();
        }
    };
    (
        status,
        [(header::CONTENT_TYPE, crate::operations::MEDIA_TYPE)],
        rendered,
    )
        .into_response()
}

/// One refusal, rendered as JSON under the same media type.
fn refusal(status: StatusCode, detail: impl core::fmt::Display) -> Response {
    answer(status, &ErrorAnswer::saying(detail))
}

/// The status a store refusal is answered under.
fn store_status(error: &StoreError) -> StatusCode {
    match error {
        StoreError::NotFound { .. } => StatusCode::NOT_FOUND,
        StoreError::LimitRefused { .. } | StoreError::ConstraintRefused { .. } => {
            StatusCode::BAD_REQUEST
        }
        StoreError::Database { .. } | StoreError::Io { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// The status a core refusal is answered under.
fn core_status(error: &CoreError) -> StatusCode {
    match error {
        CoreError::NoSuchPrint { .. } => StatusCode::NOT_FOUND,
        CoreError::Store(store) => store_status(store),
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Ask for one action of the vocabulary against one print.
async fn act(
    kind: ActionKind,
    State(state): State<ApiState>,
    Path(print_id): Path<PrintId>,
    Json(body): Json<ActionBody>,
) -> Response {
    let action = match body.into_action(kind) {
        Ok(action) => action,
        Err(rejected) => return refusal(StatusCode::BAD_REQUEST, rejected),
    };
    let outcome = match state.supervisor.request_action(print_id, action).await {
        Ok(outcome) => outcome,
        Err(error) => return refusal(core_status(&error), error),
    };
    let printer_refusal = match &outcome.record.outcome {
        Some(ExecutionOutcome::Failed { reason }) => Some(reason.clone()),
        _ => None,
    };
    let status = match outcome.record.decision {
        PolicyDecision::Accepted if printer_refusal.is_none() => StatusCode::OK,
        // The policy accepted it and the machine did not. That is a request
        // that was made and refused rather than one that never happened, so
        // the record and the printer's own words are the answer.
        PolicyDecision::Accepted => StatusCode::BAD_GATEWAY,
        // The rejection itself is the answer: its reason, the value asked for
        // and the range allowed are what let a caller ask again correctly.
        PolicyDecision::Rejected(_) => StatusCode::CONFLICT,
    };
    answer(
        status,
        &ActionAnswer {
            record: outcome.record,
            intervention: outcome.intervention,
            printer_refusal,
        },
    )
}

/// List every print, and name the one the printer's job belongs to.
///
/// Reading this adopts that job when no open print carries its file name, which
/// is how a print started at the printer gets an identifier before anything has
/// reported on it. A printer that cannot be read still answers the stored
/// prints, with nothing active.
async fn list_and_adopt_prints(State(state): State<ApiState>) -> Response {
    match state.supervisor.list_and_adopt_prints().await {
        Ok(listing) => answer(
            StatusCode::OK,
            &PrintsAnswer {
                prints: listing.prints,
                active: listing.active,
            },
        ),
        Err(error) => refusal(core_status(&error), error),
    }
}

/// Read one print's status.
async fn status(State(state): State<ApiState>, Path(print_id): Path<PrintId>) -> Response {
    let print = match state.prints.print(print_id).await {
        Ok(Some(print)) => print,
        Ok(None) => {
            return refusal(
                StatusCode::NOT_FOUND,
                format!("there is no print {print_id}"),
            );
        }
        Err(error) => return refusal(store_status(&error), error),
    };
    let context = match state.supervisor.context(print_id).await {
        Ok(context) => context,
        Err(error) => return refusal(core_status(&error), error),
    };
    let session = match state.sessions.session(print_id).await {
        Ok(session) => session,
        Err(error) => return refusal(store_status(&error), error),
    };
    answer(
        StatusCode::OK,
        &StatusAnswer {
            print,
            printer: context.printer,
            job: context.job,
            session,
            interventions: context.interventions,
        },
    )
}

/// Read everything a supervision turn is given about one print, with the
/// latest image materialized.
///
/// The path is looked up through the same store read the image operation
/// answers from, so the two operations cannot disagree about where an image
/// is. An image whose record is intact and whose file is gone answers no path,
/// which is what the caller's own missing-file failure is about.
async fn context(State(state): State<ApiState>, Path(print_id): Path<PrintId>) -> Response {
    let context = match state.supervisor.context(print_id).await {
        Ok(context) => context,
        Err(error) => return refusal(core_status(&error), error),
    };
    let mut image_path = None;
    if let Some(latest) = context.latest_image.as_ref() {
        match state.images.image(latest.id).await {
            Ok(lookup) => image_path = ImageAnswer::from(lookup).path,
            Err(error) => return refusal(store_status(&error), error),
        }
    }
    answer(
        StatusCode::OK,
        &ContextAnswer {
            context,
            image_path,
        },
    )
}

/// Materialize one image: its record, and the absolute path its bytes are at.
async fn image(State(state): State<ApiState>, Path(image_id): Path<ImageId>) -> Response {
    match state.images.image(image_id).await {
        Ok(lookup) => answer(StatusCode::OK, &ImageAnswer::from(lookup)),
        Err(error) => refusal(store_status(&error), error),
    }
}

/// Read one print's events, newest first.
async fn history(
    State(state): State<ApiState>,
    Path(print_id): Path<PrintId>,
    Query(params): Query<HistoryParams>,
) -> Response {
    let query = HistoryQuery {
        print_id,
        kinds: Vec::new(),
        since: None,
        until: None,
        limit: params.limit,
    };
    match state.events.history(query).await {
        Ok(events) => answer(StatusCode::OK, &HistoryAnswer { events }),
        Err(error) => refusal(store_status(&error), error),
    }
}

/// Read one print's manifest.
async fn manifest_get(State(state): State<ApiState>, Path(print_id): Path<PrintId>) -> Response {
    let manifest = match state.prints.manifest(print_id).await {
        Ok(manifest) => manifest,
        Err(error) => return refusal(store_status(&error), error),
    };
    let narrowings = match state.prints.print(print_id).await {
        Ok(print) => print.map(|record| record.narrowings).unwrap_or_default(),
        Err(error) => return refusal(store_status(&error), error),
    };
    answer(
        StatusCode::OK,
        &ManifestAnswer {
            manifest,
            narrowings,
        },
    )
}

/// Write one print's manifest.
///
/// A manifest narrows what any actor may ask for, so replacing one is a change
/// to the bounds a print runs under and carries a reason like every other
/// change. The reason is ruled on **before** anything is written, so a request
/// without one leaves the stored manifest and the print's narrowings as they
/// were — and every range it asked wider than the envelope allows is recorded
/// on the print, exactly as it is when a start attaches one.
async fn manifest_set(
    State(state): State<ApiState>,
    Path(print_id): Path<PrintId>,
    Json(body): Json<ManifestBody>,
) -> Response {
    if let Err(rejected) = body.reason() {
        return refusal(StatusCode::BAD_REQUEST, rejected);
    }
    let narrowed = effective_bounds(&state.supervisor.config().envelope, Some(&body.manifest));
    if let Err(error) = state
        .prints
        .put_manifest(print_id, body.manifest.clone())
        .await
    {
        return refusal(store_status(&error), error);
    }
    for narrowing in narrowed.narrowings.clone() {
        if let Err(error) = state.prints.record_narrowing(print_id, narrowing).await {
            return refusal(store_status(&error), error);
        }
    }
    answer(
        StatusCode::OK,
        &ManifestAnswer {
            manifest: Some(body.manifest),
            narrowings: narrowed.narrowings,
        },
    )
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use printobserver_core::CoreError;
    use printobserver_core::store::StoreError;

    use super::{core_status, store_status};

    /// Every refusal the store can make is answered under a status a caller can
    /// act on: what it asked for, what is not there, and what went wrong here.
    #[test]
    fn every_store_refusal_is_answered_under_a_status_a_caller_can_act_on() {
        for (error, expected) in [
            (
                StoreError::NotFound {
                    what: "print".to_owned(),
                },
                StatusCode::NOT_FOUND,
            ),
            (
                StoreError::LimitRefused {
                    limit: 1_000,
                    asked_for: 2_000,
                },
                StatusCode::BAD_REQUEST,
            ),
            (
                StoreError::ConstraintRefused {
                    constraint: "actions.decision".to_owned(),
                },
                StatusCode::BAD_REQUEST,
            ),
            (
                StoreError::Database {
                    detail: "the disk is full".to_owned(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                StoreError::Io {
                    detail: "the disk is full".to_owned(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ] {
            assert_eq!(store_status(&error), expected, "{error}");
        }
    }

    /// A print nothing holds is not found; a port that failed is this server's
    /// own failure rather than the caller's.
    #[test]
    fn a_print_nothing_holds_is_not_found_and_a_port_failure_is_this_servers_own() {
        assert_eq!(
            core_status(&CoreError::NoSuchPrint {
                print_id: printobserver_types::PrintId::new(),
            }),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            core_status(&CoreError::Store(StoreError::NotFound {
                what: "print".to_owned(),
            })),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            core_status(&CoreError::Unrepresentable {
                detail: "an instant".to_owned(),
            }),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
