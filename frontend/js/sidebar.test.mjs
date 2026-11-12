//! Focused tests for the document-tree grouping of the sidebar (ticket 0022,
//! 0032 and 0053).

import { after, test } from "node:test";
import assert from "node:assert/strict";
import { state } from "./state.js";
import { entryStatus } from "./statuses.js";

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

const { groupEntries, ticketCount, ticketsOfEpic } = await import("./sidebar.js");

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

test("the current-epic filter of ticket 0053 defaults to off", () => {
  assert.equal(state.ticketsForCurrentEpic, false);
});

test("groupEntries groups tickets under their epic instead of a flat ticket group", () => {
  const docs = [
    { kind: "ticket", path: "sdlc/epics/0007-ticket-generation/tickets/0052-ticket-crud-ui.md", epic: "0007-ticket-generation", status: "todo" },
    { kind: "epic", path: "sdlc/epics/0007-ticket-generation/description.md", title: "0007: Ticket generation", status: "planned" },
  ];
  const groups = groupEntries(docs);
  const epicGroup = groups.find(([group]) => group === "epic");
  assert.ok(epicGroup, "the epic is rendered");
  const entries = epicGroup[1];
  assert.equal(entries.length, 1);
  assert.equal(entries[0].kind, "epic");
  // The ticket is grouped under the epic, so it never forms a ticket group of its
  // own while its epic directory has a description (ticket 0053).
  assert.equal(groups.find(([group]) => group === "ticket"), undefined);
});

test("groupEntries puts orphaned tickets under the unassigned group", () => {
  const docs = [
    { kind: "ticket", path: "sdlc/epics/0002-ghost/tickets/0001-orphan.md", epic: "0002-ghost", status: "todo", orphaned: true },
  ];
  const groups = groupEntries(docs);
  const unassigned = groups.find(([group]) => group === "unassigned");
  assert.ok(unassigned, "the unassigned group is rendered");
  assert.equal(unassigned[1].length, 1);
  assert.equal(unassigned[1][0].path, "sdlc/epics/0002-ghost/tickets/0001-orphan.md");
});

test("groupEntries orders epics by number and tickets within an epic by number", () => {
  const docs = [
    { kind: "epic", path: "sdlc/epics/0009-late/description.md", title: "0009: Late", status: "planned" },
    { kind: "epic", path: "sdlc/epics/0002-early/description.md", title: "0002: Early", status: "planned" },
    { kind: "ticket", path: "sdlc/epics/0009-late/tickets/0010-late.md", epic: "0009-late", status: "todo" },
    { kind: "ticket", path: "sdlc/epics/0002-early/tickets/0008-early.md", epic: "0002-early", status: "todo" },
  ];
  const groups = groupEntries(docs);
  const epicGroup = groups.find(([group]) => group === "epic");
  assert.ok(epicGroup, "the epic group is rendered");
  const entries = epicGroup[1];
  assert.equal(entries.length, 2);
  assert.equal(entries[0].path, "sdlc/epics/0002-early/description.md");
  assert.equal(entries[1].path, "sdlc/epics/0009-late/description.md");
  assert.deepEqual(
    ticketsOfEpic(docs, "0009-late").map((ticket) => ticket.path),
    ["sdlc/epics/0009-late/tickets/0010-late.md"],
  );
});

test("entryStatus renders the front matter status and drops unknown ones", () => {
  assert.equal(
    entryStatus({ kind: "ticket", path: "sdlc/epics/0007-ticket-generation/tickets/0053-ticket-list-ui.md", status: "in-progress" }),
    "in-progress",
  );
  assert.equal(
    entryStatus({ kind: "epic", path: "sdlc/epics/0007-ticket-generation/description.md", status: "done" }),
    "done",
  );
  assert.equal(
    entryStatus({ kind: "ticket", path: "sdlc/epics/0007-ticket-generation/tickets/0053-ticket-list-ui.md", status: "bogus" }),
    null,
    "an unknown status shows no badge",
  );
  assert.equal(
    entryStatus({ kind: "ticket", path: "sdlc/epics/0007-ticket-generation/tickets/0053-ticket-list-ui.md" }),
    null,
    "an absent status shows no badge",
  );
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
