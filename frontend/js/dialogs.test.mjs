//! Focused tests for the small decision-create dialog helpers of the frontend
//! (ticket 0032): the supersedes choices of the dialog and the native dialog
//! close/cancel behaviour.

import { after, beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { STATUS_OPTIONS } from "./statuses.js";
import { openDecisionCreateDialog, supersedesOptions } from "./dialogs.js";

// The editor module wires browser globals while it loads; give Node the minimal
// surface the dialog helpers need, and restore the previous values when the
// suite is done (ticket 0030, final review round).
const savedWindow = globalThis.window;
const savedDocument = globalThis.document;
globalThis.window = globalThis.window ?? { addEventListener: () => {} };

// The dialog elements created by the helper, so a test can drive the one under
// test.
const createdDialogs = [];

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
    this.parent = null;
  }
  setAttribute(name, value) {
    this.attributes[name] = value;
  }
  getAttribute(name) {
    return this.attributes[name];
  }
  appendChild(child) {
    this.children.push(child);
    child.parent = this;
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
  close() {
    this.fire("close", {});
  }
  remove() {
    if (this.parent) {
      const index = this.parent.children.indexOf(this);
      if (index >= 0) {
        this.parent.children.splice(index, 1);
      }
      this.parent = null;
    }
  }
  showModal() {
    this.isOpen = true;
  }
}

globalThis.document = globalThis.document ?? {
  documentElement: { dataset: {} },
  querySelector: () => null,
  querySelectorAll: () => [],
  createElement: (tag) => {
    const element = new FakeElement(tag);
    if (tag === "dialog") {
      createdDialogs.push(element);
    }
    return element;
  },
  body: new FakeElement("body"),
};

// Finds a button by the id it carries, so the tests never depend on the
// positional layout of the dialog children.
function findButtonById(root, id) {
  for (const child of root.children ?? []) {
    if (child.id === id) {
      return child;
    }
    const nested = findButtonById(child, id);
    if (nested) {
      return nested;
    }
  }
  return null;
}

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
});

beforeEach(() => {
  createdDialogs.length = 0;
});

test("supersedesOptions lists the existing decisions in numeric order", () => {
  const decisions = [
    { kind: "decision", path: "sdlc/decisions/0002-second.md", title: "0002: Second" },
    { kind: "decision", path: "sdlc/decisions/0001-first.md", title: "0001: First" },
  ];
  const options = supersedesOptions(decisions);
  assert.equal(options.length, 3);
  assert.equal(options[0].value, "");
  assert.equal(options[0].label, "None");
  assert.equal(options[1].value, "1");
  assert.equal(options[1].label, "0001 First");
  assert.equal(options[2].value, "2");
  assert.equal(options[2].label, "0002 Second");
});

test("supersedesOptions skips duplicate decision numbers", () => {
  const decisions = [
    { kind: "decision", path: "sdlc/decisions/0001-first.md", title: "0001: First" },
    { kind: "decision", path: "sdlc/decisions/0001-first-again.md", title: "0001: First Again" },
  ];
  const options = supersedesOptions(decisions);
  assert.equal(options.length, 2);
  assert.equal(options[1].value, "1");
});

test("supersedesOptions keeps a no-supersedes choice without decisions", () => {
  assert.deepEqual(supersedesOptions([]), [{ value: "", label: "None" }]);
});

test("supersedesOptions marks a supersedes target that no longer exists as missing", () => {
  const options = supersedesOptions(
    [{ kind: "decision", path: "sdlc/decisions/0001-first.md", title: "0001: First" }],
    "9",
  );
  const missing = options.find((option) => option.value === "9");
  assert.ok(missing, "the missing supersedes target is kept visible");
  assert.equal(missing.label, "0009 (missing)");
});

test("the decision dialog resolves null when the person dismisses it with Escape", async () => {
  const pending = openDecisionCreateDialog([]);
  const dialog = createdDialogs[createdDialogs.length - 1];
  dialog.fire("cancel", { preventDefault: () => {} });
  assert.equal(await pending, null);
});

test("the decision dialog resolves null when Create is clicked with an empty title", async () => {
  const pending = openDecisionCreateDialog([]);
  const dialog = createdDialogs[createdDialogs.length - 1];
  dialog.children[0].value = "   ";
  const createButton = findButtonById(dialog, "decision-dialog-create");
  createButton.fire("click", {});
  assert.equal(await pending, null);
});

test("the decision dialog resolves null when the dialog cannot be shown", async () => {
  // The promise of the dialog settles while its setup runs, so the failure has
  // to happen while the dialog is being created, not after the fact.
  const originalShowModal = FakeElement.prototype.showModal;
  FakeElement.prototype.showModal = function () {
    throw new Error("showModal failed");
  };
  try {
    const pending = openDecisionCreateDialog([]);
    assert.equal(await pending, null);
  } finally {
    FakeElement.prototype.showModal = originalShowModal;
  }
});

test("the decision dialog resolves the outcome picked with Create", async () => {
  const pending = openDecisionCreateDialog([
    { kind: "decision", path: "sdlc/decisions/0001-first.md", title: "0001: First" },
  ]);
  const dialog = createdDialogs[createdDialogs.length - 1];
  const titleInput = dialog.children[0];
  titleInput.value = "Use Postgres";
  const statusSelect = dialog.children[1];
  statusSelect.value = STATUS_OPTIONS.decision[1];
  const supersedesSelect = dialog.children[2];
  supersedesSelect.value = "1";
  const createButton = findButtonById(dialog, "decision-dialog-create");
  createButton.fire("click", {});
  assert.deepEqual(await pending, {
    title: "Use Postgres",
    status: "accepted",
    supersedes: 1,
  });
});
