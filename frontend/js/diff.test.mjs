//! Focused tests for the proposal diff view of the frontend (ticket 0045):
//! the per-file sections, the line coloring, the accept and reject actions and
//! their outcomes.

import { after, beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { ApiError } from "./api.js";
import { state } from "./state.js";

// The editor and error modules wire browser globals while they load; give Node
// the minimal surface it needs before importing the diff module, and restore the
// previous values when the suite is done (ticket 0030, final review round).
const savedWindow = globalThis.window;
const savedDocument = globalThis.document;
const savedFetch = globalThis.fetch;
globalThis.window = globalThis.window ?? { addEventListener: () => {} };

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
globalThis.document = globalThis.document ?? {
  documentElement: { dataset: {} },
  querySelector: (selector) => (selector === "#editor" ? editorElement : null),
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
});

beforeEach(() => {
  state.proposal = null;
  state.current = { path: null, content: "", savedContent: "", hash: null };
  state.dirty = false;
});

function flush() {
  return new Promise((resolve) => setImmediate(resolve));
}

const { showProposal } = await import("./diff.js");

// The descendants of an element, the element itself excluded.
function descendants(element) {
  const found = [];
  const walk = (child) => {
    for (const item of child.children) {
      found.push(item);
      walk(item);
    }
  };
  walk(element);
  return found;
}

// The elements of the editor area whose class is one class name.
function byClass(className) {
  return descendants(editorElement).filter((element) => element.className === className);
}

// The first descendant of the editor area carrying one id.
function byId(id) {
  const found = descendants(editorElement).filter((element) => element.id === id);
  return found[0] ?? null;
}

// The fetch stub of a proposal decision: the accept answers the applied paths,
// the reject answers the rejection, and the follow-up document reads answer the
// fresh content of the server.
function stubDecision({ accept } = {}) {
  const calls = [];
  const answer = (url) => {
    if (url === "/api/proposals/p1/accept") {
      if (accept === "conflict") {
        throw new ApiError("conflict", "The proposal conflicts with the current documents", 409);
      }
      if (accept === "partial") {
        throw new ApiError(
          "partial_apply",
          "The proposal apply stopped midway",
          500,
          ["sdlc/PRD.md"],
          "sdlc/features/gamma.md",
        );
      }
      if (accept === "not_found") {
        throw new ApiError("not_found", "not found", 404);
      }
      return { ok: true, json: async () => ({ applied: ["sdlc/features/export.md"], warnings: [] }) };
    }
    if (url === "/api/proposals/p1/reject") {
      if (accept === "not_found") {
        throw new ApiError("not_found", "not found", 404);
      }
      return { ok: true, json: async () => ({ rejected: true }) };
    }
    if (url === "/api/tree") {
      return { ok: true, json: async () => ({ docs: [] }) };
    }
    if (url === "/api/docs/sdlc/features/export.md") {
      return { ok: true, json: async () => ({ content: "# Exported\n", hash: "hash-2" }) };
    }
    return { ok: true, json: async () => ({}) };
  };
  globalThis.fetch = async (url, options) => {
    calls.push({ url, options });
    return answer(url);
  };
  return { calls };
}

test("showProposal renders one section per file with the path and the action and the warnings above the diff", () => {
  state.current.path = "sdlc/features/export.md";
  state.current.content = "# Export\n";
  state.current.savedContent = "# Export\n";
  state.current.hash = "hash-1";
  showProposal({
    id: "p1",
    changes: [
      {
        path: "sdlc/features/export.md",
        action: "modify",
        diff: "--- a/sdlc/features/export.md\n+++ b/sdlc/features/export.md\n@@ -1,3 +1,3 @@\n-# Old\n+# New\n context\n",
      },
      { path: "sdlc/features/new.md", action: "create", diff: "--- /dev/null\n+++ b/sdlc/features/new.md\n@@ -0,0 +1,2 @@\n+# New\n+Body\n" },
      { path: "sdlc/features/old.md", action: "delete", diff: "--- a/sdlc/features/old.md\n+++ b/sdlc/features/old.md\n@@ -1,2 +0,0 @@\n-# Old\n-# Body\n" },
    ],
    warnings: ["the block was skipped"],
  });

  const view = editorElement.children[0];
  assert.equal(view.className, "proposal");
  // The pending indicator and the warnings sit above the per-file diff.
  assert.equal(editorElement.querySelector("#proposal-pending").textContent, "A proposal is pending your review.");
  assert.equal(view.children[1].className, "proposal-warnings");
  assert.equal(view.children[1].children.length, 1);
  assert.equal(view.children[1].children[0].textContent, "the block was skipped");
  const files = view.children[2];
  assert.equal(files.className, "proposal-files");
  const sections = byClass("proposal-file");
  assert.equal(sections.length, 3);
  assert.deepEqual(
    sections.map((section) => section.dataset.path),
    ["sdlc/features/export.md", "sdlc/features/new.md", "sdlc/features/old.md"],
  );
  assert.deepEqual(
    sections.map((section) => section.children[0].children[1].textContent),
    ["modify", "create", "delete"],
  );
});

test("the diff colors added and removed lines and treats headers and hunks as context", () => {
  showProposal({
    id: "p1",
    changes: [
      {
        path: "sdlc/features/export.md",
        action: "modify",
        diff: "--- a/sdlc/features/export.md\n+++ b/sdlc/features/export.md\n@@ -1,3 +1,3 @@\n-# Old\n+# New\n context\n",
      },
    ],
    warnings: [],
  });

  const file = byClass("proposal-file")[0];
  const diff = file.children[1];
  const adds = descendants(diff).filter((element) => element.className === "add");
  assert.equal(adds.length, 1);
  assert.equal(adds[0].textContent, "+# New");
  const deletes = descendants(diff).filter((element) => element.className === "del");
  assert.equal(deletes.length, 1);
  assert.equal(deletes[0].textContent, "-# Old");
  const contexts = descendants(diff).filter((element) => element.className === "ctx");
  assert.equal(contexts.length, 4);
  assert.deepEqual(
    contexts.map((row) => row.textContent),
    [
      "--- a/sdlc/features/export.md",
      "+++ b/sdlc/features/export.md",
      "@@ -1,3 +1,3 @@",
      " context",
    ],
  );
});

test("the accept and reject actions are disabled while a decision is in flight", async () => {
  const { calls } = stubDecision();
  state.current.path = "sdlc/features/export.md";
  showProposal({ id: "p1", changes: [{ path: "sdlc/features/export.md", action: "modify", diff: "" }], warnings: [] });

  const accept = byId("proposal-accept");
  const reject = byId("proposal-reject");
  assert.equal(accept.disabled, false);
  assert.equal(reject.disabled, false);

  let resolveAccept;
  globalThis.fetch = async (url, options) => {
    calls.push({ url, options });
    if (url === "/api/proposals/p1/accept") {
      return new Promise((resolve) => {
        resolveAccept = resolve;
      });
    }
    return { ok: true, json: async () => ({}) };
  };
  accept.fire("click", {});
  await flush();
  assert.equal(accept.disabled, true);
  assert.equal(reject.disabled, true);

  resolveAccept({ ok: true, json: async () => ({ applied: ["sdlc/features/export.md"] }) });
  await flush();
});

test("accepting the proposal refreshes the tree reloads the open affected document and returns to the editor", async () => {
  const { calls } = stubDecision();
  state.current.path = "sdlc/features/export.md";
  state.current.content = "# Export\n";
  state.current.savedContent = "# Export\n";
  state.current.hash = "hash-1";
  state.dirty = false;
  showProposal({ id: "p1", changes: [{ path: "sdlc/features/export.md", action: "modify", diff: "" }], warnings: [] });

  byId("proposal-accept").fire("click", {});
  await flush();
  await flush();
  await flush();

  assert.equal(calls[0].url, "/api/proposals/p1/accept");
  assert.equal(calls[0].options.method, "POST");
  assert.equal(state.proposal, null);
  assert.equal(state.current.path, "sdlc/features/export.md");
  assert.equal(state.current.content, "# Exported\n");
  assert.equal(state.current.hash, "hash-2");
  assert.equal(state.dirty, false);
  // The editor area is the editor again after the accept.
  assert.ok(byId("document-content"));
});

test("rejecting the proposal discards it and restores the editor without losing unsaved text", async () => {
  const { calls } = stubDecision();
  state.current.path = "sdlc/features/export.md";
  state.current.content = "# Unsaved local text\n";
  state.current.savedContent = "# Export\n";
  state.current.hash = "hash-1";
  state.dirty = true;
  // The buffer of the person is untouched while the diff is on screen, and the
  // editor textarea carries the unsaved text exactly like the stored content
  // does while no proposal diff is on screen.
  const buffer = byId("document-content");
  assert.ok(buffer);
  buffer.value = "# Unsaved local text\n";
  showProposal({ id: "p1", changes: [{ path: "sdlc/features/export.md", action: "modify", diff: "" }], warnings: [] });
  assert.equal(state.current.content, "# Unsaved local text\n");

  byId("proposal-reject").fire("click", {});
  await flush();
  await flush();

  assert.equal(calls[0].url, "/api/proposals/p1/reject");
  assert.equal(state.proposal, null);
  // The unsaved text survived the reject.
  assert.equal(state.current.content, "# Unsaved local text\n");
  assert.equal(state.dirty, true);
  const textarea = byId("document-content");
  assert.ok(textarea);
  assert.equal(textarea.value, "# Unsaved local text\n");
});

test("an accept that conflicts keeps the diff visible with the explanation and offers only the reject", async () => {
  stubDecision({ accept: "conflict" });
  state.current.path = "sdlc/features/export.md";
  showProposal({ id: "p1", changes: [{ path: "sdlc/features/export.md", action: "modify", diff: "" }], warnings: [] });

  byId("proposal-accept").fire("click", {});
  await flush();
  await flush();

  const conflict = byId("proposal-conflict");
  assert.equal(conflict.hidden, false);
  assert.equal(conflict.textContent, "Files changed since this proposal was made. Ask the AI again.");
  const accept = byId("proposal-accept");
  const reject = byId("proposal-reject");
  assert.equal(accept.hidden, true);
  assert.equal(reject.hidden, false);
  assert.equal(reject.disabled, false);
  // The diff stays visible.
  assert.ok(byClass("proposal-file").length > 0);
});

test("an accept that stops midway lists the written and the failed files and keeps the reject", async () => {
  stubDecision({ accept: "partial" });
  state.current.path = "sdlc/features/export.md";
  showProposal({ id: "p1", changes: [{ path: "sdlc/features/export.md", action: "modify", diff: "" }], warnings: [] });

  byId("proposal-accept").fire("click", {});
  await flush();
  await flush();

  const partial = byId("proposal-partial");
  assert.equal(partial.hidden, false);
  assert.ok(partial.textContent.includes("sdlc/PRD.md"));
  assert.ok(partial.textContent.includes("sdlc/features/gamma.md"));
  assert.equal(byId("proposal-accept").hidden, true);
  assert.equal(byId("proposal-reject").hidden, false);
});

test("an accept of an unknown proposal id after a restart clears the proposal and restores the editor", async () => {
  stubDecision({ accept: "not_found" });
  state.current.path = "sdlc/features/export.md";
  showProposal({ id: "p1", changes: [{ path: "sdlc/features/export.md", action: "modify", diff: "" }], warnings: [] });

  byId("proposal-accept").fire("click", {});
  await flush();
  await flush();

  assert.equal(state.proposal, null);
  // The editor area is the editor again instead of the diff.
  assert.ok(byId("document-content"));
});
