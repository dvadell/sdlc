//! The SSE client for file-change events (ticket 0056).
//!
//! One `EventSource("/api/events")` per page opens at startup and subscribes to
//! the `file-changed` events of the document store. A clean open document is
//! reloaded from disk, a dirty one keeps the local text behind the save conflict
//! banner, and changes of other documents refresh the tree without stealing
//! focus. The browser owns the retry loop of the stream, so a connection error
//! only surfaces the small paused hint and the next automatic retry is enough.

import { state } from "./state.js";
import { showConflictBanner } from "./editor.js";
import {
  editorRestoreSelection,
  editorSelection,
  isSaveInFlight,
  loadDocument,
} from "./editor.js";
import { refreshTree } from "./sidebar.js";

// The EventSource of the file-change stream of the page (ticket 0056); the
// browser owns its retry loop, so only the initial connection is opened here.
let source = null;

// The small "live updates paused" hint element of the header (ticket 0056).
let pausedHint = null;

// Opens one `EventSource("/api/events")` and wires the event listeners (ticket
// 0056). The source may be injected, which is how the module is tested; the
// default opens the endpoint of the running server.
export function startEvents(sourceOverride) {
  stopEvents();
  const connection = sourceOverride ?? new EventSource("/api/events");
  source = connection;
  connection.addEventListener("file-changed", (event) => {
    void handleFileChanged(event);
  });
  connection.addEventListener("open", () => {
    renderPausedHint(false);
  });
  connection.addEventListener("error", () => {
    // The browser reconnects on its own after an error; the hint is a status of
    // the stream, not an error bubble, and it clears again on the next open
    // (ticket 0056).
    renderPausedHint(true);
  });
  return connection;
}

// Closes the events connection; the page owns one connection for its lifetime.
export function stopEvents() {
  if (source) {
    source.close();
    source = null;
  }
}

// The small "live updates paused" hint of the header; the browser retries after
// an error, and the hint clears again on a successful (re)connection (ticket 0056).
export function renderPausedHint(visible) {
  const element = pausedElement();
  element.hidden = !visible;
  if (visible) {
    element.textContent = "Live updates paused; reconnecting…";
  }
}

// The element the paused hint renders into; created once, because the header
// persists across renders while the element itself is never rebuilt.
function pausedElement() {
  if (pausedHint === null) {
    const existing = document.querySelector("#events-paused");
    if (existing) {
      pausedHint = existing;
      return pausedHint;
    }
    const element = document.createElement("p");
    element.id = "events-paused";
    element.className = "events-paused";
    element.setAttribute("role", "status");
    element.hidden = true;
    pausedHint = element;
  }
  return pausedHint;
}

// Turns one `file-changed` event into the app behaviour (ticket 0056): reload
// the open document when it is clean, ask through the conflict banner when it is
// dirty, refresh the tree when another document changed.
async function handleFileChanged(event) {
  const path = event?.data?.path;
  if (typeof path !== "string" || path.length === 0) {
    // A malformed event carries no document; nothing to do for it.
    return;
  }
  if (path === state.current?.path) {
    await handleOpenDocument(path);
    return;
  }
  // The change belongs to another document, so the open editor is untouched and
  // the tree is refreshed without stealing the editor focus (ticket 0056).
  refreshTree().catch(() => {
    // The silent refresh is a fallback of the stream; a failure keeps the last
    // rendered tree and never surfaces as an error bubble (ticket 0056).
  });
}

// Applies one file change of the open document (ticket 0056): a save in flight
// owns the hash, so the event is ignored until it settles; a dirty document keeps
// the local text behind the conflict banner; a clean document reloads from disk
// and keeps the caret (tickets 0027 and 0056).
async function handleOpenDocument(path) {
  if (isSaveInFlight(path)) {
    // The save response owns the hash of the in-flight write, so the event must
    // not reload or conflict on top of it (ticket 0055, ticket 0056).
    return;
  }
  if (state.dirty) {
    // The disk changed under unsaved text; the local text is kept and the choice
    // is left to the person through the conflict banner (ticket 0027).
    showConflictBanner();
    return;
  }
  const selection = editorSelection();
  try {
    await loadDocument(path);
  } catch (error) {
    // The document may have been deleted between the event and the reload; the
    // open state is left untouched either way (ticket 0056).
    return;
  }
  editorRestoreSelection(selection);
}
