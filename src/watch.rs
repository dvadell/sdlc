//! File watcher and event hub (ticket 0054).
//!
//! The watcher notices changes sdlc did not make itself and publishes one
//! debounced event per document save into a broadcast hub. Every SSE subscriber
//! drains the hub through [`EventHub::subscribe`]; a subscriber that falls behind
//! gets a resync instead of a failure.

use std::path::Path;
use std::time::Duration;

use notify::{Error as NotifyError, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use tokio::sync::broadcast;

/// The debounce window of the watcher: one editor save within it produces one
/// event, and writes separated by more than the window produce two.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// The number of events the hub keeps for subscribers that lag behind.
const EVENT_CAPACITY: usize = 64;

/// One file change that subscribers of the event hub hear about (ticket 0054).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChanged {
    /// The project-relative path of the changed document.
    pub path: String,
}

/// The hub that fans file changes out to every subscriber (ticket 0054).
#[derive(Debug, Clone)]
pub struct EventHub {
    sender: broadcast::Sender<FileChanged>,
    /// The number of events the hub keeps for a subscriber that lags behind.
    capacity: usize,
}

impl EventHub {
    /// Creates a hub that keeps [`EVENT_CAPACITY`] events for lagging subscribers.
    pub fn new() -> Self {
        Self::with_capacity(EVENT_CAPACITY)
    }

    /// Creates a hub that keeps `capacity` events for lagging subscribers.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            sender: broadcast::Sender::new(capacity),
            capacity,
        }
    }

    /// The number of events the hub keeps for a subscriber that lags behind.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Subscribes to the next file changes of the hub (ticket 0054).
    ///
    /// A dropped subscriber only stops its own reception; the watcher keeps
    /// publishing for the subscribers that stay.
    pub fn subscribe(&self) -> broadcast::Receiver<FileChanged> {
        self.sender.subscribe()
    }

    /// Publishes one file change to every subscriber (ticket 0054).
    ///
    /// The send never blocks: a subscriber that has not drained the hub yet is
    /// marked as lagging, and never stalls the watcher.
    pub fn publish(&self, path: &str) {
        let _ = self.sender.send(FileChanged {
            path: path.to_string(),
        });
    }

    /// Receives one file change, resyncing a lagging subscriber (ticket 0054).
    ///
    /// A subscriber that falls behind receives [`broadcast::error::RecvError::Lagged`]
    /// instead of a failure; it resubscribes from the current tail and keeps
    /// receiving, so a slow or reconnecting client never breaks the stream. Each
    /// call opens a fresh subscription, so it receives one event and not a
    /// continuous stream.
    pub async fn receive_once(&self) -> Option<FileChanged> {
        let mut receiver = self.subscribe();

        loop {
            match receiver.recv().await {
                Ok(changed) => return Some(changed),
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "the file event subscriber lagged behind; resyncing");
                    receiver = receiver.resubscribe();
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

/// Starts the debounced file watcher on `root`, publishing filtered document
/// changes into `hub` (ticket 0054).
///
/// The watcher uses the recommended backend of the platform with a 300 ms
/// debounce and watches `root` recursively; events that do not name a validated
/// document are dropped, including `target/`, `.git/` and temp files. The
/// returned task holds the watcher open until the caller aborts it.
///
/// # Errors
///
/// Returns the watcher error when the debouncer or the watch itself fails to
/// start; the caller logs it and keeps serving, because editing must still work
/// without the watcher.
pub async fn start(
    root: &Path,
    hub: &EventHub,
) -> Result<tokio::task::JoinHandle<()>, NotifyError> {
    let root = root.to_path_buf();
    let handler_hub = hub.clone();
    let handler_root = root.clone();

    let mut debouncer = new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| {
        handle_result(&handler_hub, &handler_root, result);
    })?;
    debouncer.watch(&root, RecursiveMode::Recursive)?;

    Ok(tokio::spawn(async move {
        // The debouncer owns its event thread, which runs until it is stopped;
        // the task parks here and the server shutdown aborts it, which drops the
        // debouncer and stops the thread.
        let _debouncer = debouncer;

        std::future::pending::<()>().await;
    }))
}

/// Handles one debounced batch of watcher events into hub publishes (ticket 0054).
fn handle_result(hub: &EventHub, root: &Path, result: DebounceEventResult) {
    match result {
        Ok(events) => {
            // One editor save surfaces as several debounced events of the same
            // path, a modify beside its access events, so one batch keeps one
            // event per path (ticket 0054).
            let mut published = std::collections::HashSet::new();
            for debounced in events {
                for path in &debounced.event.paths {
                    if !published.insert(path.clone()) {
                        continue;
                    }
                    if let Some(path) = event_path(root, path) {
                        hub.publish(&path);
                    }
                }
            }
        }
        Err(errors) => {
            tracing::error!(errors = ?errors, "the debounced file watcher reported an error");
        }
    }
}

/// The project-relative document path of a changed file, when it is a document.
///
/// The path is validated through the same [`DocPath`] the store and the API
/// use, so changes outside the project layout, under `target/` or `.git/`, to
/// temp files or to non-document files never become events.
fn document_path(raw: &str) -> Option<String> {
    if crate::store::DocPath::parse(raw).is_err() {
        return None;
    }

    Some(raw.to_string())
}

/// The project-relative document path of a watched event path, if it is one.
///
/// The watcher reports absolute paths below the watched root, so the event is
/// resolved against the root before it is validated. A rename event may carry
/// two paths, and each of them is checked (ticket 0054).
fn event_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    // The slash form is what the document path validation expects, so a
    // platform that separates paths with a backslash does not lose every event
    // to the rejection of the backslash (ticket 0054).
    let raw = relative.to_string_lossy().into_owned().replace('\\', "/");

    document_path(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_every_valid_document_kind() {
        let cases = [
            "sdlc/PRD.md",
            "sdlc/architecture.md",
            "sdlc/conventions.md",
            "AGENTS.md",
            "sdlc/epics/0001-foundation/description.md",
            "sdlc/epics/0001-foundation/tickets/0001-project.md",
            "sdlc/decisions/0003-use-axum.md",
            "sdlc/features/export.md",
        ];

        for raw in cases {
            assert_eq!(document_path(raw), Some(raw.to_string()));
        }
    }

    #[test]
    fn rejects_every_non_document_path() {
        let rejected = [
            "Cargo.toml",
            "target/debug/sdlc",
            "target/release/sdlc",
            ".git/HEAD",
            ".sdlc-tmp-1234-5678",
            "sdlc/PRD.txt",
            "sdlc/features/export.markdown",
            "../AGENTS.md",
        ];

        for raw in rejected {
            assert_eq!(document_path(raw), None, "{raw} should not be a document");
        }
    }
}
