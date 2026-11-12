//! Focused tests for the file-change SSE client of the frontend (ticket 0056).

import { after, test } from "node:test";
import assert from "node:assert/strict";
import { state } from "./state.js";


// The events, editor and error modules wire browser globals while they load; give
// Node the minimal surface it needs before importing them, and restore the previous
// values when the suite is done (ticket 0030, final review round).
const savedWindow = globalThis.window;
const savedDocument = globalThis.document;
const savedFetch = globalThis.fetch;
globalThis.window = globalThis.window ?? { addEventListener: () => {} };

// The fake element tree of the page parts the file-change client and the editor
// render into (ticket 0056).
class FakeElement {
  constructor(tag) {
    this.tagName = tag;
    this.children = [];
    this.hidden = false;
    this.disabled = false;
    this.value = "";
    this.textContent = "";
    this.type = "";
    this.id = "";
    this.className = "";
    this.dataset = {};
    this.isConnected = true;
    this.attributes = {};
    this.listeners = {};
    this.classList = {
      add: () => {},
      remove: () => {},
      contains: () => false,
      toggle: () => {},
    };
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

const editorElement = new FakeElement("div");
const sidebarElement = new FakeElement("aside");
const pausedHintElement = new FakeElement("p");
pausedHintElement.id = "events-paused";
pausedHintElement.hidden = true;
pausedHintElement.setAttribute("role", "status");
const documentElement = { dataset: {} };
globalThis.document = globalThis.document ?? {
  documentElement,
  querySelector: (selector) => {
    if (selector === "#editor") {
      return editorElement;
    }
    if (selector === "#events-paused") {
      return pausedHintElement;
    }
    if (selector === "#sidebar") {
      return sidebarElement;
    }
    return null;
  },
  querySelectorAll: () => [],
  createElement: (tag) => new FakeElement(tag),
};

after(() => {
  // Restore the browser globals and the fetch stub this suite installed, so the
  // next suite starts from the real (or absent) Node surface again.
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
});

const { renderEditor, save } = await import("./editor.js");
const { renderPausedHint, startEvents, stopEvents } = await import("./events.js");

// The fake EventSource the events module opens; it records the listeners the way
// the browser dispatches them (ticket 0056).
class FakeEventSource {
  constructor() {
    this.listeners = {};
  }
  addEventListener(type, handler) {
    (this.listeners[type] ??= []).push(handler);
  }
  fire(type, event) {
    for (const handler of this.listeners[type] ?? []) {
      handler(event);
    }
  }
  close() {}
}

// Lets the microtask queue of the event handling drain before the assertions read
// the state the reload updated.
function flush() {
  return new Promise((resolve) => {
    setImmediate(resolve);
  });
}

// Loads a clean open document into the shared state and renders the editor for it.
function loadCleanDoc(path, content, hash) {
  state.current.path = path;
  state.current.content = content;
  state.current.savedContent = content;
  state.current.hash = hash;
  state.dirty = false;
  renderEditor();
}

test("a clean open document reloads when it changes externally", async () => {
  loadCleanDoc("sdlc/PRD.md", "# PRD\n", "hash-1");
  globalThis.fetch = async () =>
    ({ ok: true, json: async () => ({ path: "sdlc/PRD.md", content: "# PRD v2\n", hash: "server-hash" }) });

  const source = new FakeEventSource();
  startEvents(source);
  source.fire("file-changed", { data: JSON.stringify({ path: "sdlc/PRD.md" }) });
  await flush();

  assert.equal(state.current.hash, "server-hash");
  assert.equal(state.current.content, "# PRD v2\n");
  assert.equal(state.current.savedContent, "# PRD v2\n");
  assert.equal(state.dirty, false);

  stopEvents();
});

test("a malformed file-change frame is ignored without reloading", async () => {
  loadCleanDoc("sdlc/PRD.md", "# PRD\n", "hash-1");
  globalThis.fetch = async () =>
    ({ ok: true, json: async () => ({ path: "sdlc/PRD.md", content: "# PRD v2\n", hash: "server-hash" }) });

  const source = new FakeEventSource();
  startEvents(source);
  source.fire("file-changed", { data: "not-json" });
  await flush();

  assert.equal(state.current.hash, "hash-1", "the malformed frame reloads nothing");
  assert.equal(state.current.content, "# PRD\n");

  stopEvents();
});

test("a dirty open document keeps the local text and shows the conflict banner", async () => {
  state.current.path = "sdlc/PRD.md";
  state.current.content = "# Local text\n";
  state.current.savedContent = "# PRD\n";
  state.current.hash = "hash-1";
  state.dirty = true;
  renderEditor();

  const source = new FakeEventSource();
  startEvents(source);
  source.fire("file-changed", { data: JSON.stringify({ path: "sdlc/PRD.md" }) });
  await flush();

  const banner = editorElement.querySelector("#conflict-banner");
  assert.ok(banner, "the conflict banner should be rendered");
  assert.equal(banner.hidden, false, "the dirty document should ask before reloading");
  assert.equal(banner.getAttribute("aria-hidden"), "false");
  // The local text is kept; the disk content never replaces it here.
  assert.equal(state.current.content, "# Local text\n");
  assert.equal(state.current.savedContent, "# PRD\n");

  stopEvents();
});

test("a change of another document refreshes the tree without touching the editor", async () => {
  state.current.path = "sdlc/PRD.md";
  state.current.content = "# PRD\n";
  state.current.savedContent = "# PRD\n";
  state.current.hash = "hash-1";
  state.dirty = false;
  state.tree = { layout_exists: true, docs: [{ kind: "feature", path: "sdlc/features/export.md", title: "Export" }] };

  const source = new FakeEventSource();
  startEvents(source);
  source.fire("file-changed", { data: JSON.stringify({ path: "sdlc/features/export.md" }) });
  await flush();

  assert.equal(state.current.content, "# PRD\n", "the open document is not reloaded");
  assert.equal(state.current.path, "sdlc/PRD.md");

  stopEvents();
});

test("a file change of the open document is ignored while a save is in flight", async () => {
  state.current.path = "sdlc/features/export.md";
  state.current.content = "# Export\n";
  state.current.savedContent = "# Export\n";
  state.current.hash = "hash-1";
  state.dirty = true;
  renderEditor();

  const calls = [];
  let releaseSave;
  globalThis.fetch = (url) => {
    calls.push(url);
    return new Promise((resolve) => {
      releaseSave = resolve;
    }).then((response) => response);
  };
  const inFlightSave = save();
  await flush();
  assert.equal(calls.length, 1);

  const source = new FakeEventSource();
  startEvents(source);
  source.fire("file-changed", { data: JSON.stringify({ path: "sdlc/features/export.md" }) });
  await flush();
  const banner = editorElement.querySelector("#conflict-banner");
  assert.equal(banner.hidden, true, "the in-flight save owns the hash, so the event is ignored");
  assert.equal(calls.length, 1, "the in-flight save owns the reload too");

  releaseSave({ ok: true, json: async () => ({ hash: "server-hash" }) });
  await inFlightSave;

  stopEvents();
});

test("the paused hint is rendered by the events client", async () => {
  renderPausedHint(true);
  assert.equal(pausedHintElement.hidden, false);
  assert.equal(pausedHintElement.textContent, "Live updates paused; reconnecting…");

  renderPausedHint(false);
  assert.equal(pausedHintElement.hidden, true);
});
