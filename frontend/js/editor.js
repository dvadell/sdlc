//! The document editor: textarea, dirty state and save (ticket 0023).

import { state } from "./state.js";
import { getDoc, saveDoc } from "./api.js";
import { showError } from "./errors.js";

// The textarea of the selected document; null until a document is loaded.
let textarea = null;
// The save button of the toolbar; null until a document is loaded.
let saveButton = null;

// The save button is disabled while a save is in flight; `save` ignores a second
// submit on top of that.
let saveInFlight = false;

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

  textarea = document.createElement("textarea");
  textarea.id = "document-content";
  textarea.className = "document-content";
  textarea.addEventListener("input", () => {
    state.current.content = textarea.value;
    state.dirty = isDirty();
    renderDirtyIndicator();
  });

  element.appendChild(toolbar);
  element.appendChild(textarea);
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
  state.current.content = document.content;
  state.current.savedContent = document.content;
  state.current.hash = document.hash;
  state.dirty = false;

  renderEditor();
  textarea.value = document.content;
  textarea.focus();
  renderDirtyIndicator();
}

function setSaveButtonDisabled(disabled) {
  if (saveButton) {
    saveButton.disabled = disabled;
  }
}

// Saves the textarea content of the selected document. No-op when the document
// is clean or a save is already in flight; the stored hash is only trusted from
// the server response, never from an optimistic update.
export async function save() {
  if (saveInFlight || !isDirty()) {
    return;
  }

  saveInFlight = true;
  setSaveButtonDisabled(true);
  try {
    const path = state.current.path;
    const content = toSavedContent(textarea.value);
    const response = await saveDoc(path, content, state.current.hash);
    if (state.current.path !== path) {
      // The person switched documents while the save was in flight; the response
      // belongs to the old path and must not be applied to the new selection.
      return;
    }
    state.current.hash = response.hash;
    state.current.savedContent = content;
    state.current.content = content;
    state.dirty = false;
    renderDirtyIndicator();
  } catch (error) {
    // Keep the text and the dirty state; the banner shows the error.
    state.dirty = true;
    throw error;
  } finally {
    saveInFlight = false;
    setSaveButtonDisabled(false);
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