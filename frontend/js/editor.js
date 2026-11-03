//! The document editor: textarea, dirty state, save and preview (tickets 0023
//! and 0024).

import { state } from "./state.js";
import { getDoc, saveDoc } from "./api.js";
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
// Debounce timer for preview re-renders while the preview is visible.
let previewTimer = null;
// The caret of the textarea when it was hidden behind the preview; restored on
// switching back to source.
let previewCaret = null;

// The textarea normalizes line endings to `\n`; a document that was stored with
// CRLF has to be converted back before it reaches the server, or every save
// rewrites the file as LF and produces a whole-file diff.
function toSavedContent(value) {
  const saved = state.current.savedContent ?? "";
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
// comparable.
function isDirty() {
  if (!textarea) {
    return false;
  }
  const saved = state.current.savedContent ?? "";
  return normalizeNewlines(textarea.value) !== normalizeNewlines(saved);
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

// Replaces the editor area with the save toolbar and the textarea.
export function renderEditor() {
  const element = document.querySelector("#editor");
  if (!element) {
    return;
  }

  element.replaceChildren();
  clearTimeout(previewTimer);
  previewTimer = null;
  previewVisible = false;

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
    schedulePreview();
  });

  element.appendChild(toolbar);
  element.appendChild(textarea);
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
  const html = DOMPurify.sanitize(marked.parse(state.current.content));
  previewElement.innerHTML = html;
  // DOMPurify drops `target` in some configurations; force the new-tab behaviour
  // after sanitizing, not before.
  previewElement.querySelectorAll("a").forEach((link) => {
    link.target = "_blank";
    link.rel = "noopener noreferrer";
  });
}

// Renders the preview after a 150 ms debounce once the person stopped typing
// while the preview was already visible.
function schedulePreview() {
  if (!previewVisible) {
    return;
  }
  clearTimeout(previewTimer);
  previewTimer = setTimeout(() => {
    renderPreview();
  }, 150);
}

function updatePreviewButton() {
  if (!previewButton) {
    return;
  }
  previewButton.textContent = previewVisible ? "Close preview" : "Preview";
  previewButton.setAttribute("aria-pressed", previewVisible ? "true" : "false");
}

// Switches the editor area between the source textarea and the rendered
// preview (ticket 0024). Renders on toggle and, while the preview is visible,
// again on a 150 ms debounce after input; switching back to source restores the
// textarea and the caret and leaves the dirty state untouched.
export function togglePreview() {
  const element = document.querySelector("#editor");
  if (!element) {
    return;
  }

  if (previewVisible) {
    previewVisible = false;
    clearTimeout(previewTimer);
    previewTimer = null;
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
  clearTimeout(previewTimer);
  previewTimer = null;
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
  textarea.value = document.content;
  textarea.focus();
  renderDirtyIndicator();
}

// Saves the textarea content of the selected document. No-op when the document
// is clean or a save is already in flight; the stored hash is only trusted from
// the server response, never from an optimistic update.
export async function save() {
  if (saveInFlight || !isDirty()) {
    return Promise.resolve();
  }

  saveInFlight = true;
  const thisSaveButton = saveButton;
  if (thisSaveButton) {
    thisSaveButton.disabled = true;
  }
  const path = state.current.path;
  try {
    const content = toSavedContent(textarea.value);
    const response = await saveDoc(path, content, state.current.hash);
    if (state.current.path !== path) {
      // The person switched documents while the save was in flight; the response
      // belongs to the old path and must not be applied to the new selection.
      return;
    }
    state.current.hash = response.hash;
    state.current.savedContent = content;
    state.current.content = textarea.value;
    state.dirty = isDirty();
    renderDirtyIndicator();
  } catch (error) {
    // Keep the text and the dirty state; the banner shows the error. Only mark
    // the current document dirty when this save still belonged to it.
    if (state.current.path === path) {
      state.dirty = true;
    }
    throw error;
  } finally {
    saveInFlight = false;
    if (thisSaveButton) {
      thisSaveButton.disabled = false;
    }
  }
}

// Warns before the tab closes with unsaved changes.
window.addEventListener("beforeunload", (event) => {
  if (!state.dirty) {
    return;
  }
  event.preventDefault();
  event.returnValue = "";
});
