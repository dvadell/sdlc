//! The AI chat panel: transcript bubbles, composer, streaming, stop and error
//! states (tickets 0038 and 0039).

import { state } from "./state.js";
import { ApiError } from "./api.js";
import { readSse } from "./sse.js";

// The abort controller of the reply in flight; null when the panel is idle.
let abortController = null;
// The id of the reply a stream belongs to; a stream aborted just before a newer
// reply starts may still deliver one buffered chunk, and only the newest reply
// owns the panel (ticket 0038 gotcha).
let streamRequestId = 0;
// Whether the person was at the bottom of the transcript on the last render; the
// auto-scroll only scrolls while they are already at the bottom.
let atBottom = true;
// The element the auto-scroll listener is bound to; rebound when it is replaced.
let scrollElement = null;

const USER_LABEL = "You";
const ASSISTANT_LABEL = "AI";
const ERROR_LABEL = "Error";
const SCROLL_BOTTOM_THRESHOLD = 40;

// Updates the body of the streaming bubble in place, so a long reply streams
// into one bubble without rebuilding the whole transcript per delta.
function updateStreamingBubble(entry) {
  const list = document.querySelector("#chat-messages");
  const bubble = list?.querySelector(`[data-chat-stream="${entry.streamId}"]`);
  const body = bubble?.querySelector(".chat-message-body");
  if (body) {
    body.textContent = String(entry.content ?? "");
  }
}

// The AI reply in flight: POST the conversation, relay the SSE stream and turn
// the three outcomes (`done`, `error`, connection loss / stop) into transcript
// updates. Returns `{ stopped }` for a stop and `{ error }` for a failure.
async function runAssistantStream(docPath, assistantEntry, attachments) {
  const controller = new AbortController();
  abortController = controller;
  try {
    const response = await postChat(docPath, attachments, controller.signal);
    let failure = null;
    for await (const event of readSse(response, controller.signal)) {
      if (event.event === "delta") {
        // Only the reply that owns the panel renders; a stale stream may still
        // deliver a buffered chunk after a newer reply started, and that chunk
        // must not touch the transcript.
        if (assistantEntry.streamId === streamRequestId) {
          assistantEntry.content += event.data?.text ?? "";
          updateStreamingBubble(assistantEntry);
          autoScroll();
        }
      } else if (event.event === "error") {
        failure = event.data ?? null;
      }
    }
    if (failure) {
      throw new Error(failure.message ?? "The AI reply failed.");
    }
    assistantEntry.complete = true;
    return { stopped: false, error: null };
  } catch (error) {
    // A stop is an abort, not a failure: the partial reply stays in the
    // transcript and the panel goes back to the idle composer.
    if (controller.signal.aborted) {
      return { stopped: true, error: null };
    }
    return { stopped: false, error };
  } finally {
    state.chat.streaming = false;
    abortController = null;
  }
}

// The chat request: the conversation turns, the open document and the attached
// documents, and nothing else in scope. The server reads the document contents
// from disk, so the browser never sends document text of its own; the open
// document is always part of the context, the attachments ride along (ticket
// 0039).
async function postChat(docPath, attachments, signal) {
  const history = state.chat.messages
    .filter(
      (entry) =>
        (entry.role === "user" || entry.role === "assistant") &&
        typeof entry.content === "string" &&
        entry.content.length > 0,
    )
    .map((entry) => ({ role: entry.role, content: entry.content }));
  const response = await fetch("/api/chat", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ messages: history, open_doc: docPath, attachments }),
    signal,
  });
  if (!response.ok) {
    let payload = null;
    try {
      payload = await response.json();
    } catch {
      payload = null;
    }
    const error = payload?.error;
    throw new ApiError(
      error?.code ?? "chat_failed",
      error?.message ?? `The AI request failed with status ${response.status}`,
      response.status,
    );
  }
  return response;
}

// The panel may send only when the AI is configured, no reply streams, a document
// is open and the composer holds text; the open document is always part of the
// context.
function canSend() {
  if (!state.config || !state.config.ai_configured) {
    return false;
  }
  if (state.chat.streaming) {
    return false;
  }
  const input = document.querySelector("#chat-input");
  if (!(input?.value ?? "").trim()) {
    return false;
  }
  return Boolean(state.current && state.current.path);
}

// The one-line notice of the panel: document content is sent to the configured
// AI host, and the host itself comes from `GET /api/config` (never the full base
// URL, so credentials never reach the page).
function renderNotice() {
  const host = document.querySelector("#chat-ai-host");
  const notice = document.querySelector("#chat-notice");
  const configured = Boolean(state.config && state.config.ai_configured);
  const aiHost = state.config?.ai_host;
  if (host) {
    host.textContent = configured ? `AI host: ${aiHost ?? "the configured host"}` : "AI: not configured";
  }
  if (notice) {
    notice.textContent = configured
      ? `Document content is sent to ${aiHost ?? "the configured host"}.`
      : "AI chat is not configured. Set SDLC_AI_BASE_URL and SDLC_AI_MODEL, optionally SDLC_AI_API_KEY, then restart sdlc.";
  }
}

// The attach control of the panel: the chips of the open and the attached
// documents, plus the toggle of the checklist of attachable documents (ticket
// 0039).
function renderAttachments() {
  const chips = document.querySelector("#chat-attachment-chips");
  const attachButton = document.querySelector("#chat-attach");
  const openPath = state.current?.path ?? null;
  const attached = (state.chat.attachments ?? []).filter((path) => path && path !== openPath);

  const visible = Boolean(openPath) || attached.length > 0;
  if (chips) {
    chips.replaceChildren();
    if (openPath) {
      chips.appendChild(renderOpenChip(openPath));
    }
    for (const path of attached) {
      chips.appendChild(renderAttachmentChip(path));
    }
    chips.hidden = !visible;
  }
  if (attachButton) {
    attachButton.hidden = !openPath;
  }
}

// The fixed chip of the open document: it is never removable, so the person
// always sees that the open document is part of the context (ticket 0039).
function renderOpenChip(path) {
  const chip = document.createElement("div");
  chip.className = "chat-chip chat-chip-open";
  chip.textContent = path;
  return chip;
}

// One removable chip of an attached document; its remove button detaches the
// document again (ticket 0039).
function renderAttachmentChip(path) {
  const chip = document.createElement("div");
  chip.className = "chat-chip chat-chip-attachment";
  chip.textContent = path;
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "chat-chip-remove";
  remove.textContent = "Remove";
  remove.addEventListener("click", () => {
    removeAttachment(path);
  });
  chip.appendChild(remove);
  return chip;
}

// The attach button toggles the checklist of the documents the person may
// attach, minus the open document; the checklist is a plain list of checkboxes,
// so it stays keyboard accessible (ticket 0039).
function bindAttachButton(button) {
  if (!button || button.__chatAttachBound) {
    return;
  }
  button.__chatAttachBound = true;
  button.addEventListener("click", () => {
    const panel = document.querySelector("#chat-attach-panel");
    const open = Boolean(panel) && !panel.hidden;
    const nextOpen = !open;
    if (panel) {
      panel.hidden = !nextOpen;
      button.setAttribute("aria-expanded", String(nextOpen));
    }
    if (nextOpen) {
      renderChecklist();
    }
  });
}

// The checklist of attachable documents, rebuilt from the tree whenever it is
// opened; a checked entry attaches, an unchecked one detaches (ticket 0039).
function renderChecklist() {
  const list = document.querySelector("#chat-attach-list");
  if (!list) {
    return;
  }
  list.replaceChildren();
  const openPath = state.current?.path ?? null;
  for (const path of availableAttachments()) {
    const item = document.createElement("li");
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = (state.chat.attachments ?? []).includes(path);
    checkbox.addEventListener("change", (event) => {
      toggleAttachment(path, Boolean(event?.target?.checked));
    });
    const label = document.createElement("label");
    label.textContent = path;
    item.appendChild(checkbox);
    item.appendChild(label);
    list.appendChild(item);
  }
}

// The checklist of the attach control: every tree document except the open
// one, in tree order (ticket 0039).
export function availableAttachments() {
  const openPath = state.current?.path ?? null;
  const paths = [];
  for (const entry of state.tree?.docs ?? []) {
    const path = entry?.path;
    if (typeof path !== "string" || path.length === 0) {
      continue;
    }
    if (path === openPath || paths.includes(path)) {
      continue;
    }
    paths.push(path);
  }
  return paths;
}

// Attaches a document to the conversation; attaching the open document or
// repeating an attachment is a no-op, so the context never holds the same path
// twice (ticket 0039).
export function addAttachment(path) {
  const openPath = state.current?.path ?? null;
  const attachments = state.chat.attachments ?? (state.chat.attachments = []);
  if (!path || path === openPath) {
    return;
  }
  if (attachments.includes(path)) {
    return;
  }
  attachments.push(path);
}

// Detaches a document: it leaves the conversation context and stops being part
// of the next chat request (ticket 0039).
export function removeAttachment(path) {
  state.chat.attachments = (state.chat.attachments ?? []).filter((entry) => entry !== path);
}

// Checks or unchecks one checklist entry: a check attaches, an uncheck detaches
// (ticket 0039).
export function toggleAttachment(path, checked) {
  if (checked) {
    addAttachment(path);
    return;
  }
  removeAttachment(path);
}

// The attachments a chat turn sends, open document first: the open document is
// always the focus, the planned attachments are the still-existing ones after a
// deletion or a rename, without duplicates (ticket 0039). Returns the paths to
// send and the paths that no longer exist and were dropped.
function planAttachments(docPath) {
  const openPath = state.current?.path;
  const known = new Set((state.tree?.docs ?? []).map((doc) => doc?.path));
  const sent = [];
  const dropped = [];
  for (const path of state.chat.attachments ?? []) {
    if (path === openPath || path === docPath) {
      // The open document is the context on its own; it is never a duplicate
      // attachment (ticket 0039).
      continue;
    }
    if (sent.includes(path)) {
      continue;
    }
    if (!known.has(path)) {
      dropped.push(path);
      continue;
    }
    sent.push(path);
  }
  return { sent, dropped };
}

// The visible note of a turn whose attachment no longer exists: the drop keeps
// the context to documents that are still there, and the person is told which one
// was left out (ticket 0039).
function attachmentDropNote(paths) {
  const list = paths.join(", ");
  if (paths.length === 1) {
    return `The attached document ${list} no longer exists and was left out of the context.`;
  }
  return `The attached documents ${list} no longer exist and were left out of the context.`;
}

// Rebuilds the chat panel from `state.chat`; the composer and the setup guidance
// are replaced or hidden here, the transcript bubbles are rebuilt from the state.
export function renderChat() {
  const element = document.querySelector("#chat");
  if (!element) {
    return;
  }

  const messages = element.querySelector("#chat-messages");
  messages.replaceChildren();
  bindScroll(messages);
  for (const entry of state.chat.messages) {
    messages.appendChild(renderMessage(entry));
  }

  const configured = Boolean(state.config && state.config.ai_configured);
  const setup = element.querySelector("#chat-unconfigured");
  const form = element.querySelector("#chat-form");
  const input = element.querySelector("#chat-input");
  const send = element.querySelector("#chat-send");
  const stop = element.querySelector("#chat-stop");
  const noDoc = element.querySelector("#chat-no-doc");

  // The panel stays visible when AI is unconfigured; only the composer is
  // replaced by the setup instructions (ticket 0038 gotcha).
  if (setup) {
    setup.hidden = configured;
  }
  if (form) {
    form.hidden = !configured;
  }
  if (noDoc) {
    noDoc.hidden = Boolean(state.current && state.current.path);
  }
  if (input) {
    bindInput(input);
  }
  if (send) {
    send.disabled = !canSend();
  }
  if (stop) {
    stop.disabled = !state.chat.streaming;
    stop.hidden = !state.chat.streaming;
  }
  bindComposer(element);

  const attachButton = element.querySelector("#chat-attach");
  bindAttachButton(attachButton);

  renderAttachments();
  renderNotice();
}

// Binds the composer once per element instance; the flags track the element
// they are set on, so a replaced element is bound again.
function bindInput(input) {
  if (!input || input.__chatInputBound) {
    return;
  }
  input.__chatInputBound = true;
  input.addEventListener("keydown", onInputKeydown);
}

function bindComposer(element) {
  if (!element || element.__chatComposerBound) {
    return;
  }
  element.__chatComposerBound = true;
  const send = element.querySelector("#chat-send");
  const stop = element.querySelector("#chat-stop");
  if (send) {
    send.addEventListener("click", () => {
      void sendChat();
    });
  }
  if (stop) {
    stop.addEventListener("click", () => {
      stopChat();
    });
  }
}

function onInputKeydown(event) {
  if (event.key === "Enter" && !event.shiftKey) {
    // Enter sends; Shift+Enter inserts a newline, exactly like the textarea the
    // browser would insert for the default behaviour.
    event.preventDefault();
    void sendChat();
  }
}

// The stop action aborts the reply in flight; the stream ends with the stopped
// outcome and the partial text stays in the transcript.
export function stopChat() {
  abortController?.abort();
}

// Sends the composer text as one user turn and streams the reply into the next
// assistant bubble of the transcript.
export async function sendChat() {
  if (!canSend()) {
    return;
  }
  const input = document.querySelector("#chat-input");
  const text = (input?.value ?? "").trim();
  const docPath = state.current && state.current.path;
  if (!text || !docPath) {
    return;
  }

  const { sent, dropped } = planAttachments(docPath);
  const content = dropped.length > 0 ? `${text}\n\n${attachmentDropNote(dropped)}` : text;

  state.chat.messages.push({ role: "user", content });
  const assistantEntry = { role: "assistant", content: "", streamId: ++streamRequestId, complete: false };
  state.chat.messages.push(assistantEntry);
  if (input) {
    input.value = "";
  }
  state.chat.streaming = true;
  renderChat();
  autoScroll();

  const outcome = await runAssistantStream(docPath, assistantEntry, sent);
  renderChat();
  autoScroll();
  if (outcome.error) {
    // The failed reply leaves no empty assistant bubble behind; the error
    // replaces it as the single readable error bubble of the transcript.
    state.chat.messages.pop();
    appendError(outcome.error);
    renderChat();
    autoScroll();
  }
}

// Turns a failure of the reply (stream error, HTTP error, connection loss) into
// one error bubble of the transcript; the panel never leaves the spinner state.
function appendError(error) {
  let content = "The AI reply failed.";
  if (error instanceof ApiError) {
    content = error.message;
  } else if (error && typeof error === "object" && typeof error.message === "string") {
    content = error.message;
  } else if (error) {
    content = String(error);
  }
  state.chat.messages.push({ role: "error", content });
}

// The bubble of one transcript entry; user and error bubbles are text surfaces,
// assistant bubbles are plain while streaming and sanitized Markdown once the
// reply completed.
function renderMessage(entry) {
  let kind = "error";
  if (entry.role === "user") {
    kind = "user";
  } else if (entry.role === "assistant") {
    kind = "assistant";
  }
  const element = document.createElement("div");
  element.className = `chat-message chat-message-${kind}`;
  const label = document.createElement("span");
  label.className = "chat-message-label";
  let labelText = ERROR_LABEL;
  if (kind === "user") {
    labelText = USER_LABEL;
  } else if (kind === "assistant") {
    labelText = ASSISTANT_LABEL;
  }
  label.textContent = labelText;
  const body = document.createElement("div");
  body.className = "chat-message-body";
  if (kind === "assistant" && entry.complete) {
    renderAssistantMarkdown(entry, body);
  } else {
    body.textContent = String(entry.content ?? "");
  }
  if (kind === "assistant") {
    element.dataset.chatStream = String(entry.streamId ?? "");
  }
  element.appendChild(label);
  element.appendChild(body);
  return element;
}

// The assistant bubble renders the completed reply through the vendored markdown
// renderer and the sanitizer; the sanitized string is the only thing inserted, so
// a model reply containing HTML or script is never executed (ticket 0038).
function renderAssistantMarkdown(entry, body) {
  const markdown = String(entry.content ?? "");
  const marked = globalThis.marked;
  const purify = globalThis.DOMPurify;
  if (typeof marked?.parse !== "function" || typeof purify?.sanitize !== "function") {
    body.textContent = markdown;
    return;
  }
  try {
    body.innerHTML = purify.sanitize(marked.parse(markdown));
  } catch {
    body.textContent = markdown;
  }
}

function bindScroll(list) {
  if (!list || list === scrollElement) {
    return;
  }
  scrollElement = list;
  list.addEventListener("scroll", () => {
    const distance = list.scrollHeight - list.scrollTop - list.clientHeight;
    atBottom = distance <= SCROLL_BOTTOM_THRESHOLD;
  });
}

function autoScroll() {
  const list = document.querySelector("#chat-messages");
  if (list && atBottom) {
    list.scrollTop = list.scrollHeight;
  }
}
