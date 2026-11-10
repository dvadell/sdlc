//! Focused tests for the SSE reader of POST response bodies (ticket 0037).

import { test } from "node:test";
import assert from "node:assert/strict";
import { readSse, SseError, SseStoppedError } from "./sse.js";

// A response-shaped object over a `ReadableStream` of hand-built chunks, so the
// tests do not need a server or npm.
function sseResponse(chunks) {
  const encoder = new TextEncoder();
  const stream = new ReadableStream({
    start(streamController) {
      for (const chunk of chunks) {
        streamController.enqueue(typeof chunk === "string" ? encoder.encode(chunk) : chunk);
      }
      streamController.close();
    },
  });
  return { body: stream };
}

// Splits a frame into byte chunks of an odd size, so splits can fall inside a
// multi-byte UTF-8 character as well as between ASCII characters.
function byteChunks(text, size) {
  const bytes = new TextEncoder().encode(text);
  const chunks = [];
  for (let index = 0; index < bytes.length; index += size) {
    chunks.push(bytes.slice(index, index + size));
  }
  return chunks;
}

async function collectEvents(response, signal) {
  const events = [];
  for await (const event of readSse(response, signal)) {
    events.push(event);
  }
  return events;
}

test("readSse yields typed events from LF frames", async () => {
  const events = await collectEvents(
    sseResponse(['event: delta\ndata: {"text": "Hello"}\n\nevent: done\ndata: {}\n\n']),
  );
  assert.deepEqual(events, [
    { event: "delta", data: { text: "Hello" } },
    { event: "done", data: {} },
  ]);
});

test("readSse parses frames split at arbitrary byte boundaries", async () => {
  const events = await collectEvents(sseResponse(byteChunks('event: delta\ndata: {"text": "héllo"}\n\n', 7)));
  assert.deepEqual(events, [{ event: "delta", data: { text: "héllo" } }]);
});

test("readSse parses CRLF frames", async () => {
  const events = await collectEvents(sseResponse(['event: delta\r\ndata: {"text": "Hello"}\r\n\r\n']));
  assert.deepEqual(events, [{ event: "delta", data: { text: "Hello" } }]);
});

test("readSse joins multiple data lines with a newline", async () => {
  const events = await collectEvents(sseResponse(['event: message\ndata: {\ndata:   "text": "Hello"\ndata: }\n\n']));
  assert.deepEqual(events, [{ event: "message", data: { text: "Hello" } }]);
});

test("readSse ignores comments and defaults the event to message", async () => {
  const events = await collectEvents(sseResponse([': keep-alive\nevent: delta\ndata: {"text": "Hi"}\n\n']));
  assert.deepEqual(events, [{ event: "delta", data: { text: "Hi" } }]);
});

test("readSse ignores comment-only frames such as heartbeats", async () => {
  const events = await collectEvents(sseResponse([": keep-alive\n\n"]));
  assert.deepEqual(events, []);
});

test("readSse defaults the event to message without an event field", async () => {
  const events = await collectEvents(sseResponse(['data: {"text": "Hi"}\n\n']));
  assert.deepEqual(events, [{ event: "message", data: { text: "Hi" } }]);
});

test("readSse yields a final frame that has no trailing blank line", async () => {
  const events = await collectEvents(sseResponse(["event: done\ndata: {}"]));
  assert.deepEqual(events, [{ event: "done", data: {} }]);
});

test("readSse surfaces an invalid JSON frame as a typed error", async () => {
  const pending = collectEvents(sseResponse(["event: delta\ndata: not-json\n\n"]));
  await assert.rejects(pending, (error) => error instanceof SseError && error.code === "invalid_json");
});

test("readSse stops on abort with a stopped error, not a network failure", async () => {
  const controller = new AbortController();
  const stream = new ReadableStream({
    start(streamController) {
      streamController.enqueue(new TextEncoder().encode('event: delta\ndata: {"text": "Hello"}\n\n'));
    },
  });
  const pending = (async () => {
    const events = [];
    for await (const event of readSse({ body: stream }, controller.signal)) {
      events.push(event);
    }
    return events;
  })();
  setTimeout(() => controller.abort(), 10);
  await assert.rejects(pending, (error) => error instanceof SseStoppedError);
});
