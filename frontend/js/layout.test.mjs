//! Focused tests for the resizable sidebars of the app shell (ticket 0070).

import { test } from "node:test";
import assert from "node:assert/strict";
import { SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH, CHAT_MIN_WIDTH, CHAT_MAX_WIDTH, clamp, sidebarChatTemplate, applySidebarChatWidths } from "./layout.js";

test("clamp narrows a value into the given bounds", () => {
  assert.equal(clamp(5, 0, 10), 5);
  assert.equal(clamp(-5, 0, 10), 0);
  assert.equal(clamp(15, 0, 10), 10);
});

test("sidebarChatTemplate names the sidebar and AI chat widths around the editor", () => {
  assert.equal(sidebarChatTemplate(260, 360), "260px 1fr 360px");
});

test("applySidebarChatWidths writes the sidebar and AI chat widths into the app shell", () => {
  const app = { style: { gridTemplateColumns: "" } };
  applySidebarChatWidths(app, 300, 420);
  assert.equal(app.style.gridTemplateColumns, "300px 1fr 420px");
  applySidebarChatWidths(app, SIDEBAR_MAX_WIDTH, CHAT_MAX_WIDTH);
  assert.equal(app.style.gridTemplateColumns, `${SIDEBAR_MAX_WIDTH}px 1fr ${CHAT_MAX_WIDTH}px`);
});
