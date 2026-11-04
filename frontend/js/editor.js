//! The document editor: textarea, dirty state, save and preview (tickets 0023
//! and 0024), plus the save conflict banner (ticket 0027).

import { state } from "./state.js";
import { ApiError, getDoc, saveDoc } from "./api.js";
import { showError } from "./errors.js";

// The textarea of the selected document; null until a document is loaded.
let textarea = null;
// The save button of the toolbar; null until a document is loaded.
let saveButton = null;
// The preview toggle button of the toolbar; null until a document is loaded.
let previewButton = null;

// The save button is disabled while a save is in flight; `save` ignores a second
// submit on top of that.
let saveInFlight = false;

// Whether the rendered preview is shown instead of the source textarea
// (ticket 0024).
let previewVisible = false;
// The caret of the textarea when it was hidden behind the preview; restored on
// switching back to source.
let previewCaret = null;

// The save conflict banner of the selected document (ticket 0027); null until a
// document is loaded and rebuilt by `renderEditor`.
let conflictBanner = null;

// The textarea normalizes line endings to `\n`; a document that was stored with
// CRLF has to be converted back before it reaches the server, or every save
// rewrites the file as LF and produces a whole-file diff.
function toSavedContent(value, saved = state.current?.savedContent ?? "") {
  if (saved.includes("\r\n")) {
    return value.replace(/\n/g, "\r\n");
  }
  return value;
}

function normalizeNewlines(value) {
  return value.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

// The dirty state is the textarea value against the content the document was
// loaded or last saved at; both are normalized so a CRLF document stays
// comparable. Without a connected textarea the comparison falls back to the
// stored content, so a dirty document can still be saved later.
function isDirty() {
  const current = (textarea?.isConnected ? textarea.value : (state.current?.content ?? ""));
  const saved = state.current?.savedContent ?? "";
  return normalizeNewlines(current) !== normalizeNewlines(saved);
}

function renderDirtyIndicator() {
  const indicator = document.querySelector("#doc-dirty");
  const path = state.current.path;
  if (!indicator) {
    return;
  }
  if (!state.dirty || !path) {
    indicator.hidden = true;
    return;
  }
  const name = path.split("/").pop() ?? path;
  indicator.textContent = `• ${name}`;
  indicator.hidden = false;
}

// The vendored markdown renderer and sanitizer; both must be loaded before a
// preview can render.
function librariesReady() {
  return (
    typeof globalThis.marked?.parse === "function" &&
    typeof globalThis.DOMPurify?.sanitize === "function"
  );
}

// Replaces the editor area with the save toolbar and the textarea.
export function renderEditor() {
  const element = document.querySelector("#editor");
  if (!element) {
    textarea = null;
    saveButton = null;
    previewButton = null;
    conflictBanner = null;
    return;
  }

  element.replaceChildren();
  previewVisible = false;
  previewCaret = null;

  // The conflict banner lives inside the editor area so it can sit above the
  // toolbar while the person keeps typing (ticket 0027); it is rebuilt on every
  // render, so it starts hidden again for each freshly loaded document.
  conflictBanner = document.createElement("div");
  conflictBanner.id = "conflict-banner";
  conflictBanner.className = "conflict-banner";
  conflictBanner.setAttribute("role", "alert");
  conflictBanner.setAttribute("aria-live", "assertive");
  conflictBanner.hidden = true;
  conflictBanner.setAttribute("aria-hidden", "true");

  const toolbar = document.createElement("div");
  toolbar.className = "editor-toolbar";
  saveButton = document.createElement("button");
  saveButton.type = "button";
  saveButton.id = "save-document";
  saveButton.textContent = "Save";
  saveButton.addEventListener("click", () => {
    save().catch((error) => showError(error));
  });
  toolbar.appendChild(saveButton);

  previewButton = document.createElement("button");
  previewButton.type = "button";
  previewButton.id = "toggle-preview";
  previewButton.textContent = "Preview";
  previewButton.addEventListener("click", () => {
    togglePreview();
  });
  toolbar.appendChild(previewButton);

  textarea = document.createElement("textarea");
  textarea.id = "document-content";
  textarea.className = "document-content";
  textarea.addEventListener("input", () => {
    state.current.content = textarea.value;
    state.dirty = isDirty();
    renderDirtyIndicator();
  });

  element.appendChild(conflictBanner);
  element.appendChild(toolbar);
  element.appendChild(textarea);
}

// Shows the non-modal save conflict banner: the disk changed since the person
// opened the document, so the local text is kept and the choice is theirs
// (ticket 0027). Never auto-merges and never retries.
function showConflictBanner() {
  if (!conflictBanner) {
    return;
  }
  conflictBanner.replaceChildren();

  const message = document.createElement("p");
  message.className = "conflict-banner-message";
  message.textContent = "This file changed on disk since you opened it.";

  let conflictActionInFlight = false;
  const setConflictButtonsDisabled = (disabled) => {
    reloadButton.disabled = disabled;
    overwriteButton.disabled = disabled;
  };

  const reloadButton = document.createElement("button");
  reloadButton.type = "button";
  reloadButton.id = "conflict-reload";
  reloadButton.textContent = "Reload from disk";
  reloadButton.addEventListener("click", () => {
    if (conflictActionInFlight) {
      return;
    }
    conflictActionInFlight = true;
    setConflictButtonsDisabled(true);
    void reloadFromDisk().finally(() => {
      conflictActionInFlight = false;
      setConflictButtonsDisabled(false);
    });
  });

  const overwriteButton = document.createElement("button");
  overwriteButton.type = "button";
  overwriteButton.id = "conflict-overwrite";
  overwriteButton.textContent = "Overwrite with mine";
  // Explicit and destructive; deliberately not the default focus.
  overwriteButton.addEventListener("click", () => {
    if (conflictActionInFlight) {
      return;
    }
    conflictActionInFlight = true;
    setConflictButtonsDisabled(true);
    void overwriteWithMine().finally(() => {
      conflictActionInFlight = false;
      setConflictButtonsDisabled(false);
    });
  });

  conflictBanner.append(message, reloadButton, overwriteButton);
  conflictBanner.hidden = false;
  conflictBanner.setAttribute("aria-hidden", "false");
}

// Hides the save conflict banner after a choice was applied or the conflict was
// resolved another way.
function hideConflictBanner() {
  if (!conflictBanner) {
    return;
  }
  conflictBanner.hidden = true;
  conflictBanner.setAttribute("aria-hidden", "true");
}

// Whether the document a save or conflict action ran against is still the
// selected one; repaints the dirty indicator when the selection moved.
function stillSelected(path) {
  if (state.current?.path !== path) {
    renderDirtyIndicator();
    return false;
  }
  return true;
}

// Renders the markdown of the selected document into the preview element: the
// marked parser turns it into HTML, DOMPurify sanitizes it, and the sanitized
// markup is what gets inserted. The sanitized string is the only thing ever
// inserted, so raw HTML in the document is escaped or stripped instead of being
// executed; `marked` does not sanitize, so the order matters.
function renderPreview() {
  const previewElement = document.querySelector("#preview");
  if (!previewElement) {
    return;
  }
  if (!librariesReady()) {
    previewElement.textContent = "Preview could not be rendered.";
    return;
  }
  const liveTextarea = document.querySelector("#document-content");
  const markdown = liveTextarea?.isConnected
    ? liveTextarea.value
    : (textarea?.isConnected ? textarea.value : (state.current.content ?? ""));
  let html;
  try {
    html = DOMPurify.sanitize(marked.parse(String(markdown ?? "")));
  } catch (error) {
    previewElement.textContent = "Preview could not be rendered.";
    return;
  }
  previewElement.innerHTML = html;
  // DOMPurify drops `target` in some configurations; force the new-tab behaviour
  // after sanitizing, not before.
  previewElement.querySelectorAll("a").forEach((link) => {
    link.setAttribute("target", "_blank");
    link.setAttribute("rel", "noopener noreferrer");
  });
}

function updatePreviewButton() {
  if (!previewButton) {
    return;
  }
  previewButton.textContent = previewVisible ? "Close preview" : "Preview";
  previewButton.setAttribute("aria-pressed", previewVisible ? "true" : "false");
}

// Switches the editor area between the source textarea and the rendered
// preview (ticket 0024). Renders on toggle; switching back to source restores
// the textarea and the caret and leaves the dirty state untouched.
export function togglePreview() {
  const element = document.querySelector("#editor");
  if (!element) {
    return;
  }

  if (!librariesReady()) {
    showError(new Error("Preview libraries failed to load."));
    return;
  }

  if (previewVisible) {
    previewVisible = false;
    const previewElement = document.querySelector("#preview");
    if (previewElement) {
      previewElement.hidden = true;
    }
    if (textarea) {
      textarea.hidden = false;
      const start = previewCaret?.start ?? textarea.selectionStart;
      const end = previewCaret?.end ?? textarea.selectionEnd;
      textarea.focus();
      textarea.setSelectionRange(start, end);
    }
    updatePreviewButton();
    return;
  }

  previewVisible = true;
  if (textarea) {
    previewCaret = { start: textarea.selectionStart, end: textarea.selectionEnd };
    textarea.hidden = true;
  }
  let previewElement = document.querySelector("#preview");
  if (!previewElement) {
    previewElement = document.createElement("div");
    previewElement.id = "preview";
    element.appendChild(previewElement);
  }
  previewElement.hidden = false;
  renderPreview();
  updatePreviewButton();
}

// Reloads the selected document from disk, discarding the local text after the
// person confirmed. Refetches through the API instead of trusting
// `state.current.content`, and keeps the selected document on success (ticket
// 0027).
async function reloadFromDisk() {
  const path = state.current?.path;
  if (!path) {
    return;
  }
  hideConflictBanner();
  try {
    await loadDocument(path);
  } catch (error) {
    if (error?.message === "load cancelled") {
      // The person declined the discard; keep the conflict choice available.
      showConflictBanner();
      return;
    }
    // The file may have been deleted externally while the conflict was shown; the
    // local text stays in the editor either way.
    showError(error);
  }
}

// Overwrites the stored document with the local textarea content, after fetching
// a fresh hash from the server. The person chose this explicitly; if the write
// conflicts again the banner is shown again and the flow stops instead of
// looping (ticket 0027).
async function overwriteWithMine() {
  const path = state.current?.path;
  if (!path) {
    return;
  }
  hideConflictBanner();
  const localText = textarea ? textarea.value : (state.current.content ?? "");
  const savedContent = state.current?.savedContent ?? "";
  try {
    if (!stillSelected(path)) {
      // The person switched documents while overwriting; the response belongs to
      // the old path and must not be applied to the new selection.
      return;
    }
    const document = await getDoc(path);
    if (!stillSelected(path)) {
      // The selection moved while the fresh hash was loading; do not write text
      // belonging to another document under this path.
      return;
    }
    const content = toSavedContent(localText, savedContent);
    const response = await saveDoc(path, content, document.hash);
    if (!stillSelected(path)) {
      // The person switched documents while the overwrite was in flight; the
      // response belongs to the old path and must not be applied to the new one.
      return;
    }
    state.current.hash = response.hash;
    state.current.savedContent = content;
    state.current.content = textarea ? textarea.value : localText;
    state.dirty = isDirty();
    renderDirtyIndicator();
    hideConflictBanner();
  } catch (error) {
    if (error instanceof ApiError && error.code === "conflict") {
      // The disk changed again while overwriting; report the conflict and stop.
      if (state.current?.path === path) {
        showConflictBanner();
      }
      return;
    }
    showError(error);
  }
}

// Loads a document into the editor, asking before discarding unsaved changes.
export async function loadDocument(path) {
  if (state.dirty) {
    const keep = window.confirm("Discard unsaved changes?");
    if (!keep) {
      // Reject so the caller rolls the selection back instead of loading over
      // the edits the person did not confirm.
      return Promise.reject(new Error("load cancelled"));
    }
  }

  const document = await getDoc(path);
  state.current.path = path;
  state.current.content = normalizeNewlines(document.content);
  state.current.savedContent = document.content;
  state.current.hash = document.hash;
  state.dirty = false;

  renderEditor();
  if (!textarea) {
    throw new Error("The editor is unavailable.");
  }
  textarea.value = document.content;
  textarea.focus();
  renderDirtyIndicator();
}

// Saves the textarea content of the selected document. No-op when the document
// is clean or a save is already in flight; the stored hash is only trusted from
// the server response, never from an optimistic update.
export async function save() {
  const path = state.current?.path;
  if (saveInFlight || !path || !isDirty()) {
    return Promise.resolve();
  }

  saveInFlight = true;
  const thisSaveButton = saveButton;
  if (thisSaveButton) {
    thisSaveButton.disabled = true;
  }
  const content = toSavedContent(textarea ? textarea.value : (state.current?.content ?? ""));
  try {
    const response = await saveDoc(path, content, state.current.hash);
    if (!stillSelected(path)) {
      // The person switched documents while the save was in flight; the response
      // belongs to the old path and must not be applied to the new selection.
      return;
    }
    state.current.hash = response.hash;
    state.current.savedContent = content;
    state.current.content = textarea ? textarea.value : (state.current.content ?? "");
    state.dirty = isDirty();
    renderDirtyIndicator();
  } catch (error) {
    // Keep the text and the dirty state; the conflict banner offers the choices,
    // every other failure falls through to the error banner. Only mark the
    // current document dirty when this save still belonged to it.
    if (state.current?.path === path) {
      state.dirty = true;
    }
    if (error instanceof ApiError && error.code === "conflict") {
      // The disk moved under the local text; keep it and let the person choose.
      if (state.current?.path === path) {
        showConflictBanner();
      }
      return;
    }
    throw error;
  } finally {
    saveInFlight = false;
    if (thisSaveButton) {
      thisSaveButton.disabled = false;
    }
  }
}

// Clears the editor after the selected document was deleted, so no stale content,
// hash or dirty state survives a mutation (ticket 0025).
export function clearEditor() {
  state.current.path = null;
  state.current.content = "";
  state.current.savedContent = "";
  state.current.hash = null;
  state.dirty = false;
  saveInFlight = false;
  renderEditor();
  renderDirtyIndicator();
}

// Warns before the tab closes with unsaved changes.
window.addEventListener("beforeunload", (event) => {
  if (!state.dirty) {
    return;
  }
  event.preventDefault();
  event.returnValue = "";
});
