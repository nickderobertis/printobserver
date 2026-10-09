//! An event log and an image store that can refuse what a periodic
//! observation writes, under a running server.
//!
//! Everything else is the durable store's own answer: this stands between the
//! server and that store, passing every call through, so that a journey can
//! refuse one print's observation, hold one in flight while an alert arrives
//! for the same print, or refuse the frame an observation takes — and watch
//! the running server carry on once the store answers again.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use printobserver_core::ImageRecord;
use printobserver_core::store::{
    AuditPage, BoxFuture, EventDraft, EventStore, HistoryQuery, ImageLookup, ImageStore,
    StoreError, Stores,
};
use printobserver_types::{EventId, EventRecord, ImageId, PrintId, RawBytes};
use tokio::sync::Notify;

/// What the store says when it refuses a write.
pub const DETAIL: &str = "the disk is full";

/// The kind of event a periodic observation is written down as.
const OBSERVATION: &str = "periodic_observation";

/// An event log and an image store passing every call to the durable store's,
/// but for the writes a journey has told it to refuse.
pub struct HeldEvents {
    /// The event log every call is passed to.
    events: Arc<dyn EventStore>,
    /// The image store every call is passed to.
    images: Arc<dyn ImageStore>,
    /// The prints whose observations are refused.
    refused: Mutex<HashSet<PrintId>>,
    /// Whether the next observation is held until released, and then refused.
    holding: AtomicBool,
    /// Signalled when an observation is being held.
    held: Notify,
    /// Signalled to let a held observation go, refused.
    released: Notify,
    /// Whether every image is refused.
    refusing_images: AtomicBool,
}

impl HeldEvents {
    /// Stand between the server and `stores`' event log and image store,
    /// refusing nothing yet.
    #[must_use]
    pub fn over(mut stores: Stores) -> (Stores, Arc<Self>) {
        let held = Arc::new(Self {
            events: Arc::clone(&stores.events),
            images: Arc::clone(&stores.images),
            refused: Mutex::new(HashSet::new()),
            holding: AtomicBool::new(false),
            held: Notify::new(),
            released: Notify::new(),
            refusing_images: AtomicBool::new(false),
        });
        stores.events = Arc::clone(&held) as Arc<dyn EventStore>;
        stores.images = Arc::clone(&held) as Arc<dyn ImageStore>;
        (stores, held)
    }

    /// Refuse one print's observations from now on, or stop refusing them.
    pub fn refusing_observations_of(&self, print_id: PrintId, refusing: bool) {
        let mut refused = self.refused.lock().expect("the set is not poisoned");
        if refusing {
            refused.insert(print_id);
        } else {
            refused.remove(&print_id);
        }
    }

    /// Hold the next observation written until [`Self::release`], then refuse
    /// it; answer once one is being held.
    pub async fn hold_the_next_observation(&self) {
        self.holding.store(true, Ordering::SeqCst);
        self.held.notified().await;
    }

    /// Let the observation being held go, refused, and hold no more.
    pub fn release(&self) {
        self.holding.store(false, Ordering::SeqCst);
        self.released.notify_one();
    }

    /// Refuse every image from now on, or stop refusing them.
    pub fn refusing_images(&self, refusing: bool) {
        self.refusing_images.store(refusing, Ordering::SeqCst);
    }
}

/// The refusal this store answers a refused write with.
fn refusal<T>() -> Result<T, StoreError> {
    Err(StoreError::Io {
        detail: DETAIL.to_owned(),
    })
}

impl EventStore for HeldEvents {
    fn append_event(&self, draft: EventDraft) -> BoxFuture<'_, Result<EventRecord, StoreError>> {
        if draft.kind().as_str() != OBSERVATION {
            return self.events.append_event(draft);
        }
        if self.holding.swap(false, Ordering::SeqCst) {
            return Box::pin(async {
                self.held.notify_one();
                self.released.notified().await;
                refusal()
            });
        }
        let refused = draft.print_id.is_some_and(|print_id| {
            self.refused
                .lock()
                .expect("the set is not poisoned")
                .contains(&print_id)
        });
        if refused {
            return Box::pin(async { refusal() });
        }
        self.events.append_event(draft)
    }

    fn history(&self, query: HistoryQuery) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        self.events.history(query)
    }

    fn audit_page(
        &self,
        print_id: PrintId,
        after: Option<EventId>,
        page_size: u32,
    ) -> BoxFuture<'_, Result<AuditPage, StoreError>> {
        self.events.audit_page(print_id, after, page_size)
    }
}

impl ImageStore for HeldEvents {
    fn put_image(
        &self,
        print_id: PrintId,
        event_id: EventId,
        source_url: Option<String>,
        content_type: String,
        bytes: RawBytes,
    ) -> BoxFuture<'_, Result<ImageRecord, StoreError>> {
        if self.refusing_images.load(Ordering::SeqCst) {
            return Box::pin(async { refusal() });
        }
        self.images
            .put_image(print_id, event_id, source_url, content_type, bytes)
    }

    fn image(&self, image_id: ImageId) -> BoxFuture<'_, Result<ImageLookup, StoreError>> {
        self.images.image(image_id)
    }
}
