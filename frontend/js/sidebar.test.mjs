//! Focused tests for the document-tree grouping of the sidebar (ticket 0022 and
//! 0032).

import { after, test } from "node:test";
import assert from "node:assert/strict";

// The editor and error modules wire browser globals while they load; give Node
// the minimal surface they need before importing the sidebar (ticket 0030).
const savedWindow = globalThis.window;
const savedDocument = globalThis.document;
globalThis.window = globalThis.window ?? { addEventListener: () => {} };
globalThis.document = globalThis.document ?? {
  documentElement: { dataset: {} },
  querySelector: () => null,
  querySelectorAll: () => [],
  createElement: (tag) => ({ tagName: tag }),
};

const { groupEntries, ticketCount } = await import("./sidebar.js");

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

test("groupEntries keeps decisions in numeric decision-number order", () => {
  const docs = [
    { kind: "decision", path: "sdlc/decisions/0002-second.md", title: "0002: Second" },
    { kind: "decision", path: "sdlc/decisions/0001-first.md", title: "0001: First" },
  ];
  const groups = groupEntries(docs);
  const decisionGroup = groups.find(([group]) => group === "decision");
  assert.ok(decisionGroup, "the decisions group is rendered");
  const entries = decisionGroup[1];
  assert.equal(entries.length, 2);
  assert.equal(entries[0].path, "sdlc/decisions/0001-first.md");
  assert.equal(entries[1].path, "sdlc/decisions/0002-second.md");
});

test("groupEntries keeps decision entries without a number after the numbered ones", () => {
  const docs = [
    { kind: "decision", path: "sdlc/decisions/0002-second.md", title: "0002: Second" },
    { kind: "decision", path: "sdlc/decisions/malformed.md", title: "Malformed" },
  ];
  const groups = groupEntries(docs);
  const decisionGroup = groups.find(([group]) => group === "decision");
  assert.ok(decisionGroup, "the decisions group is rendered");
  const entries = decisionGroup[1];
  assert.equal(entries.length, 2);
  assert.equal(entries[0].path, "sdlc/decisions/0002-second.md");
  assert.equal(entries[1].path, "sdlc/decisions/malformed.md");
});

test("groupEntries keeps other groups untouched", () => {
  const docs = [
    { kind: "feature", path: "sdlc/features/export.md", title: "Export" },
    { kind: "core", path: "sdlc/PRD.md", title: "PRD" },
  ];
  const groups = groupEntries(docs);
  assert.deepEqual(groups.map(([group]) => group), ["core", "feature"]);
});

test("ticketCount counts the tickets of one epic directory", () => {
  const docs = [
    { kind: "ticket", path: "sdlc/epics/0007-ticket-generation/tickets/0052-ticket-crud-ui.md", epic: "0007-ticket-generation" },
    { kind: "ticket", path: "sdlc/epics/0007-ticket-generation/tickets/0053-ticket-filters.md", epic: "0007-ticket-generation" },
    { kind: "ticket", path: "sdlc/epics/0001-foundation/tickets/0001-project.md", epic: "0001-foundation" },
  ];
  assert.equal(ticketCount(docs, "0007-ticket-generation"), 2);
  assert.equal(ticketCount(docs, "0001-foundation"), 1);
  assert.equal(ticketCount([], "0007-ticket-generation"), 0);
});
