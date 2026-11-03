//! Focused tests for the client-side document path encoding and validation
//! (ticket 0021): the wildcard doc route must never be steered at dot segments
//! or other unsafe content.

import { test } from "node:test";
import assert from "node:assert/strict";
import { docUrl } from "./api.js";

test("docUrl encodes each segment and preserves separators", () => {
  assert.equal(docUrl("sdlc/PRD.md"), "/api/docs/sdlc/PRD.md");
  assert.equal(
    docUrl("sdlc/features/my cool doc.md"),
    "/api/docs/sdlc/features/my%20cool%20doc.md",
  );
  assert.equal(
    docUrl("sdlc/tickets/12-use the force.md"),
    "/api/docs/sdlc/tickets/12-use%20the%20force.md",
  );
});

test("docUrl rejects paths that are not safe document paths", () => {
  assert.throws(() => docUrl("../Cargo.toml"), (error) => error.code === "invalid_path");
  assert.throws(() => docUrl("/../Cargo.toml"), (error) => error.code === "invalid_path");
  assert.throws(() => docUrl("sdlc/features/../PRD.md"), (error) => error.code === "invalid_path");
  assert.throws(() => docUrl("sdlc//features/x.md"), (error) => error.code === "invalid_path");
  assert.throws(() => docUrl("sdlc/features/a\0b.md"), (error) => error.code === "invalid_path");
  assert.throws(() => docUrl("sdlc\\PRD.md"), (error) => error.code === "invalid_path");
  assert.throws(() => docUrl(null), (error) => error.code === "invalid_path");
  assert.throws(() => docUrl(""), (error) => error.code === "invalid_path");
});
