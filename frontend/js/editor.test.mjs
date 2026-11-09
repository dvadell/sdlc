//! Focused tests for the front matter status helpers of the editor (ticket
//! 0030): the client must read and rewrite the status exactly like the server's
//! `src/store/frontmatter.rs` splitter and parser do.

import { after, test } from "node:test";
import assert from "node:assert/strict";
import { ApiError } from "./api.js";
import { STATUS_OPTIONS, statusKindOf } from "./statuses.js";
import { state } from "./state.js";

// The editor module wires browser globals while it loads; give Node the minimal
// surface it needs before importing it, and restore the previous values when the
// suite is done so no later suite or test depends on this file's browser-global or
// fetch stubs (ticket 0030, final review round).
const savedWindow = globalThis.window;
const savedDocument = globalThis.document;
const savedFetch = globalThis.fetch;
globalThis.window = globalThis.window ?? { addEventListener: () => {} };

// The editor module renders editor chrome (toolbar, status select, textarea) and
// owns save/status-save state; give Node a minimal fake element tree for the
// parts the status save flow exercises.
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

const { readStatus, withStatus, save, clearEditor, changeStatus, renderEditor, setRefreshTree } =
  await import("./editor.js");

const FEATURE = STATUS_OPTIONS.feature;

// The status select rendered for a feature document; `renderEditor` builds it
// inside the fake editor element, so it is reachable through the tree.
function statusSelect() {
  return editorElement.querySelector("#document-status");
}

test("readStatus returns null when the document has no front matter block", () => {
  assert.equal(readStatus("# 0001: Foundation\n\nbody\n", FEATURE), null);
});

test("readStatus reads the status of a valid front matter block", () => {
  assert.equal(
    readStatus("---\nstatus: planned\n---\n\n# 0001: Foundation\n", FEATURE),
    "planned",
  );
});

test("readStatus takes a BOM into account", () => {
  assert.equal(readStatus("\uFEFF---\nstatus: planned\n---\nbody", FEATURE), "planned");
});

test("readStatus reads CRLF front matter", () => {
  assert.equal(
    readStatus("---\r\nstatus: planned\r\n---\r\n\r\n# Title\r\n", FEATURE),
    "planned",
  );
});

test("readStatus ignores a horizontal rule as the closing marker", () => {
  assert.equal(
    readStatus("---\nstatus: planned\n---\n\n# Title\n\n---\n\nbody\n", FEATURE),
    "planned",
  );
});

test("readStatus treats an unterminated block as absent", () => {
  assert.equal(readStatus("---\nstatus: planned\n\n# Title\n", FEATURE), null);
});

test("readStatus treats an unterminated opening delimiter as absent", () => {
  // The opening delimiter must be a line of its own with a line terminator
  // after it, like the server's splitter requires.
  assert.equal(readStatus("---", FEATURE), null);
});

test("readStatus returns null for an empty front matter block", () => {
  assert.equal(readStatus("---\n---\n\n# Title\n", FEATURE), null);
});

test("readStatus rejects a duplicate status key", () => {
  assert.equal(
    readStatus("---\nstatus: planned\nstatus: done\n---\n", FEATURE),
    null,
  );
});

test("readStatus rejects an unknown status value", () => {
  assert.equal(readStatus("---\nstatus: bogus\n---\n", FEATURE), null);
});

test("readStatus is case sensitive over status values", () => {
  assert.equal(readStatus("---\nstatus: Planned\n---\n", FEATURE), null);
});

test("readStatus removes one balanced pair of surrounding quotes", () => {
  assert.equal(readStatus("---\nstatus: \"planned\"\n---\n", FEATURE), "planned");
  assert.equal(readStatus("---\nstatus: 'planned'\n---\n", FEATURE), "planned");
});

test("readStatus strips a trailing comment from a status value", () => {
  // A `#` after a space or tab starts a YAML comment, so the server reads the
  // status and the client must not treat the comment as part of the value.
  assert.equal(readStatus("---\nstatus: planned # note\n---\n\n# Title\n", FEATURE), "planned");
  assert.equal(readStatus("---\nstatus: \"planned\" # note\n---\n", FEATURE), "planned");
});

test("readStatus accepts an indented status key", () => {
  // An indented `status` key is a valid YAML mapping key like any other, so the
  // server reads it and the client must read it too.
  assert.equal(readStatus("---\n  status: planned\n---\n\n# Title\n", FEATURE), "planned");
});

test("readStatus ignores a status key nested under another key", () => {
  // A key indented under an earlier key of the block is a nested mapping key, so
  // the server reads no top-level status and neither must the client.
  assert.equal(readStatus("---\nfeature:\n  status: planned\n---\n\n# Title\n", FEATURE), null);
});

test("withStatus inserts a front matter block when the document has none", () => {
  assert.equal(withStatus("# Export\n", "planned"), "---\nstatus: planned\n---\n\n# Export\n");
});

test("withStatus replaces the status line of an existing block", () => {
  assert.equal(
    withStatus("---\nstatus: planned\n---\n\n# Export\n", "shipped"),
    "---\nstatus: shipped\n---\n\n# Export\n",
  );
});

test("withStatus preserves unknown front matter keys", () => {
  assert.equal(
    withStatus("---\nstatus: planned\nfeature: export\n---\n\n# Export\n", "shipped"),
    "---\nstatus: shipped\nfeature: export\n---\n\n# Export\n",
  );
});

test("withStatus rewrites duplicate status keys into one fresh status line", () => {
  assert.equal(
    withStatus("---\nstatus: planned\nstatus: done\n---\n\n# Export\n", "shipped"),
    "---\nstatus: shipped\n---\n\n# Export\n",
  );
});

test("withStatus keeps the indentation of an indented status line", () => {
  assert.equal(
    withStatus("---\n  status: planned\n---\n\n# Export\n", "shipped"),
    "---\n  status: shipped\n---\n\n# Export\n",
  );
});

test("withStatus preserves the line endings of a CRLF block", () => {
  assert.equal(
    withStatus("---\r\nstatus: planned\r\n---\r\n\r\n# Export\r\n", "shipped"),
    "---\r\nstatus: shipped\r\n---\r\n\r\n# Export\r\n",
  );
});

test("readStatus treats empty content as having no front matter", () => {
  assert.equal(readStatus("", FEATURE), null);
});

test("readStatus ignores lone CR line endings like the server", () => {
  // The server's splitter splits on `\n` and strips one trailing `\r` of a CRLF
  // pair, so lone CR endings leave no front matter block behind.
  assert.equal(readStatus("---\rstatus: planned\r---\r", FEATURE), null);
});

test("readStatus reads mixed CRLF and LF line endings", () => {
  assert.equal(readStatus("---\r\nstatus: planned\n---\n\n# Title\n", FEATURE), "planned");
});

test("withStatus inserts a front matter block into empty content", () => {
  assert.equal(withStatus("", "planned"), "---\nstatus: planned\n---\n\n");
});

test("withStatus preserves mixed CRLF and LF line endings", () => {
  assert.equal(
    withStatus("---\r\nstatus: planned\n---\n\n# Export\n", "shipped"),
    "---\r\nstatus: shipped\n---\n\n# Export\n",
  );
});

// The status kind mapping against the backend document layout (`src/store/layout.rs`
// and its `is_kebab`/`is_numbered_slug`): the client must keep the path rules of
// the server in lockstep, so a regression here surfaces a status control for a path
// the server rejects or drops one it accepts (ticket 0030, final review round).
test("statusKindOf maps backend-valid feature paths to the feature kind", () => {
  assert.equal(statusKindOf("sdlc/features/export.md"), "feature");
  assert.equal(statusKindOf("/sdlc/features/export.md"), "feature");
  assert.equal(statusKindOf("sdlc/features/0007-add-export.md"), "feature");
});

test("statusKindOf maps backend-valid epic paths to the epic kind", () => {
  assert.equal(statusKindOf("sdlc/epics/0001-foundation/description.md"), "epic");
  assert.equal(statusKindOf("sdlc/epics/0000-foundation/description.md"), "epic");
  assert.equal(statusKindOf("sdlc/epics/9999-foundation/description.md"), "epic");
});

test("statusKindOf maps backend-valid numbered documents without a status control to null", () => {
  assert.equal(statusKindOf("sdlc/epics/0001-foundation/tickets/0001-project.md"), null);
  assert.equal(statusKindOf("sdlc/PRD.md"), null);
});

test("statusKindOf maps backend-valid decision paths to the decision kind", () => {
  // Decisions carry the ADR status vocabulary (ticket 0032), so an open
  // decision gets the status select of the toolbar.
  assert.equal(statusKindOf("sdlc/decisions/0001-use-axum.md"), "decision");
  assert.equal(statusKindOf("/sdlc/decisions/0001-use-axum.md"), "decision");
  assert.equal(statusKindOf("sdlc/decisions/9999-use-axum.md"), "decision");
});

test("statusKindOf rejects backend-invalid decision paths", () => {
  assert.equal(statusKindOf("sdlc/decisions/use-axum.md"), null);
  assert.equal(statusKindOf("sdlc/decisions/0001-Use-Axum.md"), null);
  assert.equal(statusKindOf("sdlc/decisions/0001.md"), null);
  assert.equal(statusKindOf("sdlc/decisions/nested/0001-use-axum.md"), null);
});

test("the decision status vocabulary is the ADR statuses", () => {
  // The decision vocabulary of ticket 0031 and the toolbar select of ticket
  // 0032 (same pattern as the features of ticket 0030).
  assert.deepEqual(STATUS_OPTIONS.decision, ["proposed", "accepted", "superseded"]);
});

test("statusKindOf rejects backend-invalid feature paths", () => {
  assert.equal(statusKindOf("sdlc/features/Export.md"), null);
  assert.equal(statusKindOf("sdlc/features/nested/export.md"), null);
  assert.equal(statusKindOf("sdlc/features/a--b.md"), null);
});

test("statusKindOf rejects backend-invalid epic paths", () => {
  assert.equal(statusKindOf("sdlc/epics/foundation/description.md"), null);
  assert.equal(statusKindOf("sdlc/epics/10000-foundation/description.md"), null);
  assert.equal(statusKindOf("sdlc/epics/0001-Foundation/description.md"), null);
  assert.equal(statusKindOf("sdlc/epics/0001-foundation/0001-project.md"), null);
});

// The save and status-save state machine of the editor (ticket 0030, final
// review round): the owner-token clearing, the status-save supersession and the
// teardown release of an in-flight save.

function createDeferred() {
  let resolvePromise;
  let rejectPromise;
  const promise = new Promise((resolve, reject) => {
    resolvePromise = resolve;
    rejectPromise = reject;
  });
  return { promise, resolve: resolvePromise, reject: rejectPromise, calls: [] };
}

function stubFetch() {
  const deferred = createDeferred();
  globalThis.fetch = async (url, options) => {
    deferred.calls.push({ url, options });
    return { ok: true, json: async () => ({ hash: "server-hash" }) };
  };
  return deferred;
}

function loadFeatureDoc(path, content, hash) {
  state.current.path = path;
  state.current.content = content;
  state.current.savedContent = content;
  state.current.hash = hash;
  state.dirty = true;
}

test("changeStatus blocks a second status save while one is in flight and restores the select", async () => {
  loadFeatureDoc("sdlc/features/export.md", "---\nstatus: planned\n---\n\n# 0001: Export\n", "hash-1");
  renderEditor();
  assert.equal(statusSelect().value, "planned");

  const deferred = stubFetch();
  const first = changeStatus("in-progress");
  assert.equal(deferred.calls.length, 1);

  // A newer status save cannot supersede the older one while the older is still
  // in flight; the in-flight slot keeps the first save.
  void changeStatus("shipped");
  assert.equal(deferred.calls.length, 1);
  assert.equal(statusSelect().value, "planned");
  assert.equal(statusSelect().disabled, true);

  deferred.resolve({ hash: "hash-2" });
  await first;
  assert.equal(statusSelect().disabled, false);
  assert.equal(statusSelect().value, "in-progress");
});

test("clearEditor keeps an in-flight status save of the deleted document until it settles", async () => {
  loadFeatureDoc("sdlc/features/export.md", "---\nstatus: planned\n---\n\n# 0001: Export\n", "hash-1");
  renderEditor();
  const deferred = stubFetch();
  const statusSave = changeStatus("in-progress");
  assert.equal(deferred.calls.length, 1);

  // The person deletes the document whose status save is in flight.
  clearEditor();

  loadFeatureDoc("sdlc/features/other.md", "# 0002: Other\n", "hash-2");
  renderEditor();
  // The in-flight status save of the deleted document keeps the in-flight slot
  // until it settles, so a new status save of another document must not start
  // alongside the stale operation.
  const secondDeferred = stubFetch();
  const statusSave2 = changeStatus("planned");
  assert.equal(secondDeferred.calls.length, 0);
  deferred.resolve({ hash: "hash-2" });
  await statusSave;
  const appliedStatusSave = changeStatus("planned");
  assert.equal(secondDeferred.calls.length, 1);
  secondDeferred.resolve({ hash: "hash-3" });
  await appliedStatusSave;
});

test("a manual save of another document is not skipped while a status save is in flight", async () => {
  loadFeatureDoc("sdlc/features/export.md", "---\nstatus: planned\n---\n\n# 0001: Export\n", "hash-1");
  renderEditor();
  const deferred = stubFetch();
  const statusSave = changeStatus("in-progress");
  assert.equal(deferred.calls.length, 1);

  // A manual save of the document the status save belongs to is still skipped
  // while the status save owns the in-flight slot.
  assert.deepEqual(await save(), { skipped: true });

  // The person moves to another document and saves it while the status save
  // runs; the foreign manual save must not be blocked behind the status save.
  loadFeatureDoc("sdlc/features/other.md", "# 0002: Other\n", "hash-2");
  const manualSave = save();
  assert.equal(deferred.calls.length, 2);
  assert.deepEqual(await manualSave, { skipped: false });
  deferred.resolve({ hash: "hash-3" });
  await statusSave;
});

// Loads a decision document into the editor state, like `loadFeatureDoc` does for
// a feature (ticket 0032).
function loadDecisionDoc(path, content, hash) {
  state.current.path = path;
  state.current.content = content;
  state.current.savedContent = content;
  state.current.hash = hash;
  state.dirty = true;
}

// The read-only supersedes line of the open decision and the toolbar status
// select it renders next to (ticket 0032).
test("renderEditor shows the decision status select and the supersedes line", () => {
  state.tree = {
    layout_exists: true,
    docs: [
      {
        kind: "decision",
        path: "sdlc/decisions/0001-use-axum.md",
        title: "0001: Use axum",
        status: "accepted",
        supersedes: null,
      },
      {
        kind: "decision",
        path: "sdlc/decisions/0002-use-redis.md",
        title: "0002: Use redis",
        status: "proposed",
        supersedes: 1,
      },
    ],
  };
  loadDecisionDoc("sdlc/decisions/0002-use-redis.md", "---\nstatus: proposed\nsupersedes: 1\n---\n\n# 0002: Use redis\n", "hash-3");
  renderEditor();
  // The decision carries the ADR status vocabulary, so the toolbar select is
  // rendered with it (ticket 0032).
  const select = statusSelect();
  assert.ok(select);
  assert.equal(select.value, "proposed");
  const line = editorElement.querySelector("#decision-supersedes");
  assert.ok(line);
  // The open decision supersedes decision 0001, which still exists, so the line
  // is the plain read-only pointer.
  assert.equal(line.textContent, "Supersedes: 0001");
});

test("the supersedes line marks a decision target that no longer exists", () => {
  state.tree = {
    layout_exists: true,
    docs: [
      {
        kind: "decision",
        path: "sdlc/decisions/0002-use-redis.md",
        title: "0002: Use redis",
        status: "proposed",
        supersedes: 9,
      },
    ],
  };
  loadDecisionDoc("sdlc/decisions/0002-use-redis.md", "---\nstatus: proposed\nsupersedes: 9\n---\n\n# 0002: Use redis\n", "hash-4");
  renderEditor();
  const line = editorElement.querySelector("#decision-supersedes");
  assert.ok(line);
  assert.equal(line.textContent, "Supersedes: 0009 (missing)");
});

test("changing the status of a decision saves through the normal path and updates the select", async () => {
  loadDecisionDoc("sdlc/decisions/0002-use-redis.md", "---\nstatus: proposed\nsupersedes: 1\n---\n\n# 0002: Use redis\n", "hash-3");
  renderEditor();
  assert.equal(statusSelect().value, "proposed");
  const deferred = stubFetch();
  const decisionStatusSave = changeStatus("accepted");
  assert.equal(deferred.calls.length, 1);
  deferred.resolve({ hash: "hash-4" });
  await decisionStatusSave;
  assert.equal(statusSelect().value, "accepted");
});

// A fetch stub that answers the first request with a save conflict and holds the
// overwrite's read and save until the test releases them, so a test can switch
// documents while the overwrite is in flight.
function stubConflictThenOk() {
  const deferred = createDeferred();
  const gates = [];
  globalThis.fetch = async (url, options) => {
    deferred.calls.push({ url, options });
    if (deferred.calls.length === 1) {
      throw new ApiError("conflict", "The document changed on disk.", 409);
    }
    const callLength = deferred.calls.length;
    if (callLength === 2 || callLength === 3) {
      // The overwrite's read (call 2) and save (call 3) are held until released.
      return new Promise((resolve) =>
        gates.push(() =>
          resolve({ ok: true, json: async () => ({ hash: "server-hash" }) }),
        ),
      );
    }
    return Promise.resolve({ ok: true, json: async () => ({ hash: "server-hash" }) });
  };
  return { deferred, gates };
}

test("an overwrite that completes after a document switch hides the banner and refreshes the tree", async () => {
  let refreshes = 0;
  setRefreshTree(() => {
    refreshes += 1;
    return Promise.resolve();
  });
  loadFeatureDoc("sdlc/features/export.md", "---\nstatus: planned\n---\n\n# 0001: Export\n", "hash-1");
  renderEditor();
  const { deferred, gates } = stubConflictThenOk();
  editorElement.querySelector("#save-document").fire("click", {});
  await new Promise((resolve) => setImmediate(resolve));
  const banner = editorElement.querySelector("#conflict-banner");
  assert.equal(banner.hidden, false);
  editorElement.querySelector("#conflict-overwrite").fire("click", {});
  await new Promise((resolve) => setImmediate(resolve));
  // The overwrite issued its read against the server and is now holding it.
  assert.equal(deferred.calls.length, 2);
  gates[0]();
  await new Promise((resolve) => setImmediate(resolve));
  // The overwrite issued its save and is now holding it too.
  assert.equal(deferred.calls.length, 3);
  // The person switches documents while the overwrite is in flight.
  loadFeatureDoc("sdlc/features/other.md", "# 0002: Other\n", "hash-2");
  gates[1]();
  await new Promise((resolve) => setImmediate(resolve));
  // The overwrite settled after the switch, so it must have hidden the banner
  // and refreshed the tree of the document that was overwritten.
  assert.equal(banner.hidden, true);
  assert.equal(refreshes, 1);
});
