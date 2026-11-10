//! Focused tests for the AI chat panel of the frontend (tickets 0038 and 0039):
//! the transcript bubbles, the unconfigured setup state, the composer rules, the
//! stream outcomes and the document attachments.

import { after, beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { state } from "./state.js";

// The chat module wires browser globals while it loads; give Node the minimal
// surface it needs before importing it, and restore the previous values when the
// suite is done so no later suite or test depends on this file's browser-global or
// fetch stubs (ticket 0030, final review round).
const savedWindow = globalThis.window;
const savedDocument = globalThis.document;
const savedFetch = globalThis.fetch;
const savedMarked = globalThis.marked;
const savedPurify = globalThis.DOMPurify;
globalThis.window = globalThis.window ?? { addEventListener: () => {} };

// The chat panel element tree the module queries; the messages container is the
// transcript, the rest are the composer and the guidance of the panel.
class FakeElement {
  constructor(tag) {
    this.tagName = tag;
    this.children = [];
    this.hidden = false;
    this.disabled = false;
    this.value = "";
    this.textContent = "";
    this.innerHTML = "";
    this.type = "";
    this.id = "";
    this.className = "";
    this.dataset = {};
    this.isConnected = true;
    this.attributes = {};
    this.listeners = {};
    this.scrollHeight = 0;
    this.scrollTop = 0;
    this.clientHeight = 0;
  }
  setAttribute(name, value) {
    this.attributes[name] = value;
  }
  getAttribute(name) {
    return this.attributes[name];
  }
  appendChild(child) {
    this.children.push(child);
    return child;
  }
  append(...children) {
    this.children.push(...children);
  }
  replaceChildren() {
    this.children = [];
  }
  addEventListener(type, handler) {
    (this.listeners[type] ??= []).push(handler);
  }
  fire(type, event) {
    for (const handler of this.listeners[type] ?? []) {
      handler(event);
    }
  }
  focus() {}
  setSelectionRange() {}
  querySelector(selector) {
    const id = selector.startsWith("#") ? selector.slice(1) : null;
    const found = [];
    const walk = (element) => {
      for (const child of element.children) {
        if (child.id === id) {
          found.push(child);
        }
        walk(child);
      }
    };
    walk(this);
    return found[0] ?? null;
  }
  querySelectorAll() {
    return [];
  }
}

const chatElement = new FakeElement("aside");
chatElement.id = "chat";
const messagesElement = new FakeElement("div");
messagesElement.id = "chat-messages";
const unconfiguredElement = new FakeElement("div");
unconfiguredElement.id = "chat-unconfigured";
const formElement = new FakeElement("div");
formElement.id = "chat-form";
const noDocElement = new FakeElement("p");
noDocElement.id = "chat-no-doc";
const inputElement = new FakeElement("textarea");
inputElement.id = "chat-input";
const sendElement = new FakeElement("button");
sendElement.id = "chat-send";
const stopElement = new FakeElement("button");
stopElement.id = "chat-stop";
const hostElement = new FakeElement("p");
hostElement.id = "chat-ai-host";
const noticeElement = new FakeElement("p");
noticeElement.id = "chat-notice";
const chipsElement = new FakeElement("div");
chipsElement.id = "chat-attachment-chips";
const attachButtonElement = new FakeElement("button");
attachButtonElement.id = "chat-attach";
const attachPanelElement = new FakeElement("div");
attachPanelElement.id = "chat-attach-panel";
attachPanelElement.hidden = true;
const attachListElement = new FakeElement("ul");
attachListElement.id = "chat-attach-list";

chatElement.appendChild(messagesElement);
chatElement.appendChild(unconfiguredElement);
chatElement.appendChild(formElement);
formElement.appendChild(noDocElement);
formElement.appendChild(inputElement);
const actionsElement = new FakeElement("div");
actionsElement.id = "chat-actions";
formElement.appendChild(actionsElement);
actionsElement.appendChild(sendElement);
actionsElement.appendChild(stopElement);
chatElement.appendChild(hostElement);
chatElement.appendChild(noticeElement);
chatElement.appendChild(chipsElement);
chatElement.appendChild(attachButtonElement);
chatElement.appendChild(attachPanelElement);
chatElement.appendChild(attachListElement);

globalThis.document = globalThis.document ?? {
  documentElement: { dataset: {} },
  querySelector: (selector) => {
    switch (selector) {
      case "#chat":
        return chatElement;
      case "#chat-messages":
        return messagesElement;
      case "#chat-unconfigured":
        return unconfiguredElement;
      case "#chat-form":
        return formElement;
      case "#chat-no-doc":
        return noDocElement;
      case "#chat-input":
        return inputElement;
      case "#chat-send":
        return sendElement;
      case "#chat-stop":
        return stopElement;
      case "#chat-ai-host":
        return hostElement;
      case "#chat-notice":
        return noticeElement;
      case "#chat-attachment-chips":
        return chipsElement;
      case "#chat-attach":
        return attachButtonElement;
      case "#chat-attach-panel":
        return attachPanelElement;
      case "#chat-attach-list":
        return attachListElement;
      default:
        return null;
    }
  },
  createElement: (tag) => new FakeElement(tag),
};

after(() => {
  if (savedWindow === undefined) {
    delete globalThis.window;
  } else {
    globalThis.window = savedWindow;
  }
  if (savedDocument === undefined) {
    delete globalThis.document;
  } else {
    globalThis.document = savedDocument;
  }
  if (savedFetch === undefined) {
    delete globalThis.fetch;
  } else {
    globalThis.fetch = savedFetch;
  }
  if (savedMarked === undefined) {
    delete globalThis.marked;
  } else {
    globalThis.marked = savedMarked;
  }
  if (savedPurify === undefined) {
    delete globalThis.DOMPurify;
  } else {
    globalThis.DOMPurify = savedPurify;
  }
});

const { renderChat, sendChat, addAttachment, removeAttachment, availableAttachments } = await import(
  "./chat.js"
);

function sseResponse(chunks, { close = true } = {}) {
  const encoder = new TextEncoder();
  const stream = new ReadableStream({
    start(controller) {
      for (const chunk of chunks) {
        controller.enqueue(typeof chunk === "string" ? encoder.encode(chunk) : chunk);
      }
      if (close) {
        controller.close();
      }
    },
  });
  return { ok: true, status: 200, body: stream, json: async () => ({}) };
}

let fetchCalls = [];
let response = null;
beforeEach(() => {
  fetchCalls = [];
  response = null;
  inputElement.value = "";
  globalThis.fetch = async (url, options) => {
    fetchCalls.push({ url, options });
    return response;
  };
});

beforeEach(() => {
  state.config = {
    project_name: "sdlc",
    port: 9000,
    ai_configured: true,
    model: "llama3.2",
    ai_host: "localhost:11434",
    version: "0",
  };
  state.current = { path: "sdlc/PRD.md", content: "# PRD\n", savedContent: "# PRD\n", hash: "hash-1" };
  state.tree = null;
  state.chat = { messages: [], streaming: false, attachments: [] };
});

function bubbles() {
  const list = chatElement.querySelector("#chat-messages");
  return list.children.map((child) => ({
    className: child.className,
    label: child.children[0].textContent,
    body: child.children[1].textContent,
    innerHTML: child.children[1].innerHTML,
  }));
}

test("an unconfigured panel shows setup instructions and no send control", () => {
  state.config = { project_name: "sdlc", port: 9000, ai_configured: false, model: null, ai_host: null };
  state.current = { path: null, content: "", savedContent: "", hash: null };
  state.chat = { messages: [], streaming: false, attachments: [] };

  renderChat();

  const setup = chatElement.querySelector("#chat-unconfigured");
  const form = chatElement.querySelector("#chat-form");
  const send = chatElement.querySelector("#chat-send");
  const stop = chatElement.querySelector("#chat-stop");
  assert.equal(setup.hidden, false);
  assert.equal(form.hidden, true);
  assert.equal(send.disabled, true);
  assert.equal(stop.hidden, true);
  assert.ok(hostElement.textContent.includes("not configured"));
  assert.ok(noticeElement.textContent.includes("SDLC_AI_BASE_URL"));
  assert.ok(noticeElement.textContent.includes("restart sdlc"));
});

test("a configured panel with an open document shows the AI host and the privacy notice", () => {
  state.current = { path: "sdlc/PRD.md", content: "# PRD\n", savedContent: "# PRD\n", hash: "hash-1" };

  renderChat();

  assert.equal(hostElement.textContent, "AI host: localhost:11434");
  assert.equal(noticeElement.textContent, "Document content is sent to localhost:11434.");
  const noDoc = chatElement.querySelector("#chat-no-doc");
  assert.equal(noDoc.hidden, true);
});

test("the send control is disabled while the composer is empty", () => {
  renderChat();

  assert.equal(inputElement.value, "");
  assert.equal(sendElement.disabled, true);
});

test("enter sends the composer text and shift+enter keeps the newline", async () => {
  response = sseResponse(["event: done\ndata: {}\n\n"]);
  inputElement.value = "Hello panel";
  const sent = sendChat();
  await sent;

  assert.equal(fetchCalls.length, 1);
  assert.equal(fetchCalls[0].url, "/api/chat");
  assert.equal(fetchCalls[0].options.method, "POST");
  const body = JSON.parse(fetchCalls[0].options.body);
  assert.deepEqual(body.messages, [{ role: "user", content: "Hello panel" }]);
  assert.equal(body.open_doc, "sdlc/PRD.md");
  assert.deepEqual(body.attachments, []);
  assert.equal(inputElement.value, "");
  assert.equal(state.chat.streaming, false);
});

test("the request carries the open document and every attached document", async () => {
  state.chat.attachments = ["sdlc/conventions.md"];
  state.tree = { docs: [{ kind: "core", path: "sdlc/conventions.md", title: "Conventions" }] };
  inputElement.value = "Hello panel";
  await sendChat();

  assert.equal(fetchCalls.length, 1);
  const body = JSON.parse(fetchCalls[0].options.body);
  assert.equal(body.open_doc, "sdlc/PRD.md");
  assert.deepEqual(body.attachments, ["sdlc/conventions.md"]);
});

test("attaching the open document or repeating an attachment is a no-op", () => {
  addAttachment("sdlc/PRD.md");
  addAttachment("sdlc/conventions.md");
  addAttachment("sdlc/conventions.md");

  assert.deepEqual(state.chat.attachments, ["sdlc/conventions.md"]);
});

test("removing an attachment omits it from the next request", async () => {
  state.chat.attachments = ["sdlc/conventions.md"];
  state.tree = { docs: [{ kind: "core", path: "sdlc/conventions.md", title: "Conventions" }] };
  inputElement.value = "Hello panel";
  await sendChat();
  assert.deepEqual(JSON.parse(fetchCalls[0].options.body).attachments, ["sdlc/conventions.md"]);

  removeAttachment("sdlc/conventions.md");
  inputElement.value = "Hello again";
  await sendChat();
  assert.deepEqual(JSON.parse(fetchCalls[fetchCalls.length - 1].options.body).attachments, []);
});

test("a deleted attachment is dropped with a visible note and not sent", async () => {
  state.chat.attachments = ["sdlc/features/gone.md"];
  state.tree = { docs: [{ kind: "feature", path: "sdlc/features/export.md", title: "Export" }] };
  inputElement.value = "Hello panel";
  await sendChat();

  const body = JSON.parse(fetchCalls[0].options.body);
  assert.deepEqual(body.attachments, []);
  const userTurn = state.chat.messages.find((entry) => entry.role === "user");
  assert.ok(userTurn.content.includes("sdlc/features/gone.md"));
  assert.ok(userTurn.content.includes("left out of the context"));
});

test("the open document chip is fixed and attachment chips can be removed", () => {
  state.chat.attachments = ["sdlc/conventions.md"];
  renderChat();

  const chips = document.querySelector("#chat-attachment-chips");
  assert.equal(chips.children.length, 2);
  assert.equal(chips.children[0].className, "chat-chip chat-chip-open");
  const attachmentChip = chips.children[1];
  assert.equal(attachmentChip.className, "chat-chip chat-chip-attachment");
  attachmentChip.children[0].fire("click", {});
  assert.deepEqual(state.chat.attachments, []);
});

test("the request never sends a duplicate attachment path", async () => {
  state.chat.attachments = ["sdlc/conventions.md", "sdlc/conventions.md"];
  state.tree = { docs: [{ kind: "core", path: "sdlc/conventions.md", title: "Conventions" }] };
  inputElement.value = "Hello panel";
  await sendChat();

  const body = JSON.parse(fetchCalls[0].options.body);
  assert.deepEqual(body.attachments, ["sdlc/conventions.md"]);
});

test("switching documents keeps the attachments and updates the open chip", () => {
  state.chat.attachments = ["sdlc/conventions.md"];
  state.current.path = "sdlc/features/export.md";
  renderChat();

  const chips = document.querySelector("#chat-attachment-chips");
  assert.equal(chips.children.length, 2);
  assert.equal(chips.children[0].textContent, "sdlc/features/export.md");
  assert.equal(chips.children[1].textContent, "sdlc/conventions.md");
});

test("the attach checklist lists the tree except the open document and toggles attachments", () => {
  state.tree = {
    docs: [
      { kind: "core", path: "sdlc/PRD.md", title: "PRD" },
      { kind: "feature", path: "sdlc/features/export.md", title: "Export" },
      { kind: "feature", path: "sdlc/features/import.md", title: "Import" },
    ],
  };
  renderChat();

  const attachButton = document.querySelector("#chat-attach");
  assert.equal(attachButton.hidden, false);
  attachButton.fire("click", {});
  const list = document.querySelector("#chat-attach-list");
  assert.equal(list.children.length, 2);
  const checkbox = list.children[0].children[0];
  assert.equal(checkbox.type, "checkbox");
  checkbox.checked = true;
  checkbox.fire("change", { target: { checked: true } });

  assert.deepEqual(availableAttachments(), ["sdlc/features/export.md", "sdlc/features/import.md"]);
  assert.deepEqual(state.chat.attachments, ["sdlc/features/export.md"]);
});

test("shift+enter does not send, and enter twice quickly sends only one request", async () => {
  response = sseResponse(["event: done\ndata: {}\n\n"]);
  inputElement.value = "Hello panel";
  inputElement.fire("keydown", { key: "Enter", shiftKey: true, preventDefault() {} });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(fetchCalls.length, 0);

  inputElement.value = "Hello panel";
  const first = sendChat();
  inputElement.fire("keydown", { key: "Enter", shiftKey: false, preventDefault() {} });
  await first;
  assert.equal(fetchCalls.length, 1);
});

test("a streamed reply lands in one assistant bubble and renders markdown once complete", async () => {
  response = sseResponse([
    'event: delta\ndata: {"text": "Hello"}\n\n',
    'event: delta\ndata: {"text": " world"}\n\n',
    "event: done\ndata: {}\n\n",
  ]);
  state.chat.messages = [];
  inputElement.value = "Hello panel";

  await sendChat();

  assert.equal(state.chat.messages.length, 2);
  assert.deepEqual(state.chat.messages[0], { role: "user", content: "Hello panel" });
  assert.equal(state.chat.messages[1].role, "assistant");
  assert.equal(state.chat.messages[1].content, "Hello world");
  assert.equal(state.chat.messages[1].complete, true);
  const list = chatElement.querySelector("#chat-messages");
  assert.equal(list.children.length, 2);
  assert.equal(list.children[0].className, "chat-message chat-message-user");
  assert.equal(list.children[1].className, "chat-message chat-message-assistant");
});

test("an incomplete assistant reply renders plain text and a completed one renders sanitized markdown", () => {
  state.chat.messages = [{ role: "assistant", content: "# Hello <script>", complete: false }];
  renderChat();
  assert.equal(bubbles()[0].body, "# Hello <script>");
  assert.equal(bubbles()[0].innerHTML, "");

  state.chat.messages = [{ role: "assistant", content: "# Hello <script>", complete: true }];
  globalThis.marked = { parse: (markdown) => `<h1>${markdown}</h1>` };
  globalThis.DOMPurify = { sanitize: (html) => `<sanitized>${html}</sanitized>` };
  renderChat();
  assert.equal(bubbles()[0].body, "");
  assert.equal(bubbles()[0].innerHTML, "<sanitized><h1># Hello <script></h1></sanitized>");
});

test("a stream error becomes one readable error bubble and frees the composer", async () => {
  response = {
    ok: false,
    status: 401,
    json: async () => ({ error: { code: "unauthorized", message: "The AI key was rejected." } }),
  };
  inputElement.value = "Hello panel";

  await sendChat();

  assert.equal(state.chat.streaming, false);
  assert.equal(state.chat.messages.length, 2);
  assert.equal(state.chat.messages[1].role, "error");
  assert.equal(state.chat.messages[1].content, "The AI key was rejected.");
  // The composer accepts a new message again after the error.
  inputElement.value = "Retry please";
  renderChat();
  assert.equal(sendElement.disabled, false);
});

test("stop keeps the partial reply and allows a new message", async () => {
  response = sseResponse(['event: delta\ndata: {"text": "He"}\n\n'], { close: false });
  inputElement.value = "Hello panel";
  const pending = sendChat();
  await new Promise((resolve) => setImmediate(resolve));
  stopElement.fire("click", {});
  await pending;

  assert.equal(state.chat.streaming, false);
  assert.equal(state.chat.messages.length, 2);
  assert.equal(state.chat.messages[1].content, "He");
  assert.equal(state.chat.messages[1].complete, false);
  // The partial reply stayed plain text and the panel is idle again.
  const list = chatElement.querySelector("#chat-messages");
  assert.equal(list.children[1].children[1].textContent, "He");
});

test("the auto-scroll only runs while the person is already at the bottom", async () => {
  response = sseResponse(['event: delta\ndata: {"text": "Hi"}\n\n', "event: done\ndata: {}\n\n"]);
  messagesElement.scrollHeight = 1000;
  messagesElement.clientHeight = 500;
  messagesElement.scrollTop = 0; // 500px from the bottom: not at the bottom.
  messagesElement.fire("scroll", {});
  inputElement.value = "Hello panel";

  await sendChat();

  // Scrolled away from the bottom mid-stream: the reply must not yank the view.
  assert.equal(messagesElement.scrollTop, 0);

  // Back at the bottom, the next reply scrolls the transcript down again.
  messagesElement.scrollTop = 500;
  messagesElement.fire("scroll", {});
  response = sseResponse(["event: done\ndata: {}\n\n"]);
  inputElement.value = "Hello panel";
  const second = sendChat();
  await second;
  assert.equal(messagesElement.scrollTop, 1000);
});
