//! Tests for the built-in prompt catalog of the chat panel (tickets 0046 and 0049).

import { test } from "node:test";
import assert from "node:assert/strict";
import { PROMPTS, promptById, enabledPrompts, DISABLED_PROMPT_HINT } from "./prompts.js";

test("the catalog holds the six built-in documentation prompts", () => {
  assert.equal(PROMPTS.length, 6);
});

test("every prompt carries an id, a short label, a description, editable text and attachments", () => {
  const ids = new Set();
  for (const prompt of PROMPTS) {
    assert.ok(typeof prompt.id === "string" && prompt.id.length > 0, "every prompt has an id");
    assert.ok(!ids.has(prompt.id), `every prompt has a unique id: ${prompt.id}`);
    ids.add(prompt.id);
    assert.ok(typeof prompt.label === "string" && prompt.label.length > 0, `${prompt.id} has a label`);
    assert.ok(typeof prompt.title === "string" && prompt.title.length > 0, `${prompt.id} has a title`);
    assert.ok(typeof prompt.text === "string" && prompt.text.trim().length > 0, `${prompt.id} has text`);
    assert.ok(Array.isArray(prompt.attachments), `${prompt.id} has attachments`);
  }
});

test("the ready prompts ask for full files in FILE blocks and never for patches", () => {
  const ready = enabledPrompts();
  assert.equal(ready.length, 6);
  for (const prompt of ready) {
    assert.ok(
      prompt.text.includes("FILE"),
      `${prompt.id} must mention the FILE block format`,
    );
    assert.ok(
      /full file/i.test(prompt.text),
      `${prompt.id} must ask for full files, not patches`,
    );
    assert.ok(
      !/show( me)? a patch/i.test(prompt.text),
      `${prompt.id} must not ask for a patch`,
    );
  }
});

test("the documentation jobs attach the documents of the ticket", () => {
  const byId = new Map(PROMPTS.map((prompt) => [prompt.id, prompt]));
  assert.deepEqual(byId.get("split-prd").attachments, ["sdlc/PRD.md"]);
  assert.deepEqual(byId.get("generate-tickets").attachments, ["sdlc/conventions.md"]);
  assert.deepEqual(byId.get("feature-spec").attachments, ["sdlc/PRD.md"]);
  assert.deepEqual(byId.get("adr").attachments, ["sdlc/architecture.md", "sdlc/conventions.md"]);
  assert.deepEqual(
    byId.get("trim-agents").attachments,
    ["AGENTS.md", "sdlc/PRD.md", "sdlc/architecture.md", "sdlc/conventions.md"],
  );
  assert.deepEqual(
    byId.get("check-docs").attachments,
    ["sdlc/PRD.md", "sdlc/architecture.md", "sdlc/conventions.md", "AGENTS.md"],
  );
});

test("the trim prompt asks for short actionable rules and the full new file", () => {
  const byId = new Map(PROMPTS.map((prompt) => [prompt.id, prompt]));
  const trim = byId.get("trim-agents");
  assert.ok(trim.text.includes("Trim AGENTS.md and keep it consistent with the PRD, architecture and conventions"));
  assert.ok(/actionable and short/i.test(trim.text));
  assert.ok(/propose the full new file/i.test(trim.text));
  assert.ok(trim.text.includes("=== FILE: AGENTS.md ==="));
});

test("the ticket-generation prompt is enabled and asks for one ticket file per ticket", () => {
  const generation = promptById("generate-tickets");
  assert.ok(generation);
  assert.ok(enabledPrompts().includes(generation));
  assert.ok(generation.text.length > 0);
  assert.deepEqual(generation.attachments, ["sdlc/conventions.md"]);
  assert.ok(generation.text.includes("Generate one ticket file per unit of work"));
  assert.ok(generation.text.includes("=== FILE: sdlc/epics/<epic>/tickets/<kebab-slug>.md ==="));
  assert.ok(generation.text.includes("=== END FILE ==="));
});

test("the ticket-generation prompt forbids numeric prefixes and the epic front matter key", () => {
  const generation = promptById("generate-tickets");
  assert.ok(generation.text.includes("Do not use a numeric ticket prefix"));
  assert.ok(generation.text.includes("Do not add an `epic` key"));
});

test("the ticket-generation prompt asks for the title without a number and the ticket sections", () => {
  const generation = promptById("generate-tickets");
  assert.ok(generation.text.includes("# <Title>"));
  assert.ok(generation.text.includes("Description, Acceptance criteria, Tests, Gotchas and Dependencies"));
  assert.ok(generation.text.includes("Write dependencies as ticket titles in the body"));
});

test("promptById returns null for an unknown id", () => {
  assert.equal(promptById("nonexistent"), null);
});

test("the ticket-generation tooltip explains what to select", () => {
  assert.ok(DISABLED_PROMPT_HINT.length > 0);
});
