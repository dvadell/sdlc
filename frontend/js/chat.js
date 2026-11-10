//! The AI chat panel: transcript bubbles, composer, streaming, stop and error
//! states (ticket 0038).

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
async function runAssistantStream(docPath, assistantEntry) {
  const controller = new AbortController();
  abortController = controller;
  try {
    const response = await postChat(docPath, controller.signal);
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

// The chat request: the conversation turns, the open document and nothing else
// in scope. The server reads the document contents from disk, so the browser
// never sends document text of its own.
async function postChat(docPath, signal) {
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
    body: JSON.stringify({ messages: history, open_doc: docPath }),
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

  state.chat.messages.push({ role: "user", content: text });
  const assistantEntry = { role: "assistant", content: "", streamId: ++streamRequestId, complete: false };
  state.chat.messages.push(assistantEntry);
  if (input) {
    input.value = "";
  }
  state.chat.streaming = true;
  renderChat();
  autoScroll();

  const outcome = await runAssistantStream(docPath, assistantEntry);
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
