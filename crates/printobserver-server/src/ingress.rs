//! The one endpoint `Obico`'s webhook notification plugin posts to.
//!
//! # It answers before it has handled anything
//!
//! `Obico` posts best-effort: its plugin calls `requests.post` with a five
//! second timeout and raises on the answer, and nothing retries. So this
//! endpoint does the two things that must precede an answer — check the shared
//! secret, and take the body — and then answers, leaving the handling to a
//! worker of its own. The bound the answer must arrive inside is
//! [`ServerConfig::ingress_answer_bound`](crate::ServerConfig), and it is
//! load-bearing rather than decorative: the queue between the endpoint and the
//! worker is bounded, and a body the worker is too busy to take **inside that
//! bound** is refused inside it rather than held past it.
//!
//! # The secret is carried in the request, not in the body
//!
//! Anything that can post here can pause a printer, so a post that carries no
//! valid secret is refused. `Obico`'s plugin is configured with a URL and
//! nothing else — `custom_webhook_URL` is the whole of its configuration — so
//! the secret is accepted in the query as well as in a header, and the query
//! form is the one a real `Obico` can carry.
//!
//! # An unauthenticated post is recorded
//!
//! Under the malformed-external-event kind, carrying the bytes that arrived.
//! That is what that kind means — a body arrived and this system did not read
//! it — and it is the only kind that carries bytes with no print behind them.
//! Dropping it silently would leave the one class of post an operator most
//! needs to see written down nowhere.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use printobserver_core::Supervisor;
use printobserver_obico::ObicoVision;
use printobserver_store_api::{EventDraft, StorePort};
use printobserver_types::serde::Deserialize;
use printobserver_types::{
    EventPayload, EventSource, MalformedExternalEventPayload, RawBytes, Timestamp,
};
use printobserver_vision_api::VisionPort;
use tokio::sync::{mpsc, watch};

use crate::config::SharedSecret;
use crate::wire::{ErrorAnswer, IngressAnswer};

/// The header a caller that can set headers carries the shared secret in.
pub const TOKEN_HEADER: &str = "x-printobserver-token";

/// The query parameter a caller that can only set a URL carries it in.
pub const TOKEN_PARAM: &str = "token";

/// How many bodies may be waiting on the worker at once.
///
/// Small on purpose. The queue is not a buffer for a backlog — it is the seam
/// that lets the answer precede the handling — and a deep one would hide a
/// worker that had stopped keeping up until far more alerts had been taken than
/// could be acted on.
pub const QUEUE_DEPTH: usize = 8;

/// One body taken for handling, with what arrived beside it.
#[derive(Debug, Clone)]
struct Received {
    /// The bytes exactly as posted.
    body: RawBytes,
    /// The content type they were posted under, when one was declared.
    content_type: Option<String>,
}

/// The shared secret, as a post carried it.
#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct TokenParam {
    /// The secret in the query, which is the form `Obico`'s own plugin carries.
    #[serde(default)]
    pub token: Option<String>,
}

/// What the ingress endpoint is given.
#[derive(Clone)]
pub struct IngressState {
    /// Where an unauthenticated post is written down.
    store: Arc<dyn StorePort>,
    /// The secret every post must carry, which is a value nothing can read out
    /// of this state: what it offers is the comparison rather than the value.
    secret: Arc<SharedSecret>,
    /// How long taking a body may take before the post is refused.
    bound: core::time::Duration,
    /// The seam between the answer and the handling.
    queue: mpsc::Sender<Received>,
    /// How many bodies the worker has finished handling.
    completed: watch::Sender<u64>,
}

impl core::fmt::Debug for IngressState {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("IngressState")
            .field("bound", &self.bound)
            .finish_non_exhaustive()
    }
}

impl IngressState {
    /// The endpoint's state, and the worker that handles what it takes.
    ///
    /// The worker runs until the sender is dropped, which is what stops it when
    /// the server it belongs to is.
    pub fn start(
        supervisor: Arc<Supervisor>,
        store: Arc<dyn StorePort>,
        vision: Arc<ObicoVision>,
        secret: SharedSecret,
        bound: core::time::Duration,
    ) -> Self {
        let (queue, mut incoming) = mpsc::channel::<Received>(QUEUE_DEPTH);
        let (completed, _) = watch::channel(0);
        let worker_store = Arc::clone(&store);
        let counter = completed.clone();
        tokio::spawn(async move {
            while let Some(received) = incoming.recv().await {
                handle(&supervisor, &worker_store, &vision, received).await;
                counter.send_modify(|count| *count += 1);
            }
        });
        Self {
            store,
            secret: Arc::new(secret),
            bound,
            queue,
            completed,
        }
    }

    /// How many bodies the worker has finished handling, watchable while it
    /// works. This is the seam a caller sees the answer precede the handling
    /// through.
    #[must_use]
    pub fn completions(&self) -> watch::Receiver<u64> {
        self.completed.subscribe()
    }
}

/// Handle one body the endpoint took: read it, and give it to the loop.
async fn handle(
    supervisor: &Arc<Supervisor>,
    store: &Arc<dyn StorePort>,
    vision: &Arc<ObicoVision>,
    received: Received,
) {
    match vision
        .normalize(received.body.clone(), received.content_type)
        .await
    {
        // Core's own loop is the one place an alert becomes a supervised turn:
        // it appends the event, fetches and stores the image the alert names,
        // runs the turn, and closes the print out when the machine says it has
        // ended. Nothing here repeats any of that.
        Ok(alert) => {
            let _ = supervisor.handle_event(alert).await;
        }
        Err(refusal) => {
            record_unread(store, received.body, refusal.to_string()).await;
        }
    }
}

/// Write down one body this system did not read, and the reason it did not.
async fn record_unread(store: &Arc<dyn StorePort>, body: RawBytes, detail: String) {
    let _ = store
        .append_event(EventDraft {
            print_id: None,
            source: EventSource::Obico,
            received_at: Timestamp::now(),
            payload: EventPayload::MalformedExternalEvent(MalformedExternalEventPayload { detail }),
            raw: Some(body),
        })
        .await;
}

/// Take one body `Obico` posted.
pub(crate) async fn receive(
    State(state): State<IngressState>,
    headers: HeaderMap,
    Query(params): Query<TokenParam>,
    body: axum::body::Bytes,
) -> Response {
    let offered = headers
        .get(TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or(params.token);
    let body = RawBytes::new(body.to_vec());
    if !state.secret.matches(offered.as_deref()) {
        record_unread(
            &state.store,
            body,
            "the post carried no valid shared secret, so this system did not read it".to_owned(),
        )
        .await;
        return rendered(
            StatusCode::UNAUTHORIZED,
            &ErrorAnswer::saying("this ingress requires the shared secret it is configured with"),
        );
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let taken = tokio::time::timeout(
        state.bound,
        state.queue.send(Received { body, content_type }),
    )
    .await;
    match taken {
        Ok(Ok(())) => rendered(StatusCode::ACCEPTED, &IngressAnswer),
        // Either the worker is too far behind to take this inside the bound, or
        // it has stopped. Refusing inside the bound is the answer Obico can act
        // on; holding past it is an alert Obico has already abandoned.
        Ok(Err(_)) | Err(_) => rendered(
            StatusCode::SERVICE_UNAVAILABLE,
            &ErrorAnswer::saying(format!(
                "this ingress could not take the body inside its {}ms answer bound",
                state.bound.as_millis()
            )),
        ),
    }
}

/// One answer, rendered as JSON.
fn rendered<T: printobserver_types::serde::Serialize>(status: StatusCode, body: &T) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, crate::operations::MEDIA_TYPE)],
        Json(printobserver_types::serde_json::to_value(body).unwrap_or_default()),
    )
        .into_response()
}
