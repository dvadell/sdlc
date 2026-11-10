//! The SSE reader for POST response bodies (ticket 0037).
//!
//! `EventSource` can only open GET streams, so a chat reply is read from the
//! fetch body stream with this async generator and turned into typed events.

/** A stream frame that could not be turned into a typed event (ticket 0037). */
export class SseError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "SseError";
    this.code = code;
  }
}

/** The reader was stopped before the stream ended (ticket 0037). */
export class SseStoppedError extends Error {
  constructor() {
    super("The stream was stopped");
    this.name = "SseStoppedError";
  }
}

// A frame ends with a blank line, either the LF pair `\n\n` or the CRLF pair
// `\r\n\r\n`; the alternation keeps a CRLF frame from being read as two frames.
const FRAME_END = /\r\n\r\n|\n\n/;

// Turns one frame into the typed event it describes: the `event` field (which
// defaults to `message`) plus the JSON payload of the joined `data` lines.
// Comment-only frames (heartbeats) carry no data and yield no event, and a
// frame whose data is not valid JSON fails with a typed error instead of an
// unhandled rejection.
function parseFrame(frame) {
  let event = "message";
  const dataLines = [];
  for (const rawLine of frame.split("\n")) {
    // A line may end with the CR of a CRLF pair; drop it before judging.
    const line = rawLine.endsWith("\r") ? rawLine.slice(0, -1) : rawLine;
    if (line.length === 0 || line.startsWith(":")) {
      continue;
    }
    if (line.startsWith("event:")) {
      event = line.slice(6).replace(/^ /, "");
    } else if (line.startsWith("data:")) {
      // `data:` may carry an optional single space before the payload, and
      // several data lines of one frame are joined with a newline.
      const value = line.slice(5).replace(/^ /, "");
      dataLines.push(value);
    }
  }

  // A frame without data lines is a comment or keep-alive, not an event.
  if (dataLines.length === 0) {
    return null;
  }

  let data;
  try {
    data = JSON.parse(dataLines.join("\n"));
  } catch (error) {
    throw new SseError(
      "invalid_json",
      `The stream frame was not valid JSON: ${error?.message ?? "unknown error"}`,
    );
  }

  return { event, data };
}

// Reads the body of a streamed response as SSE frames and yields one typed
// event per frame. `signal` aborts the read: the reader is cancelled and the
// iteration ends with a `SseStoppedError`, which callers treat as a stop instead
// of a failure (ticket 0037).
export async function* readSse(response, signal) {
  // A bodyless response (for example a 204) has nothing to read; end cleanly
  // instead of failing on a missing reader.
  if (response.body === null) {
    return;
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  // The stop promise settles once, when the stream stops; the reader race below
  // turns it into the rejection the consumer of the generator handles.
  let stopReject = null;
  const stop = new Promise((_, reject) => {
    stopReject = reject;
  });
  const onExternalAbort = () => stopReject(new SseStoppedError());
  let detachExternal = null;
  if (signal) {
    if (signal.aborted) {
      onExternalAbort();
    } else {
      detachExternal = () => signal.removeEventListener("abort", onExternalAbort);
      signal.addEventListener("abort", onExternalAbort, { once: true });
    }
  }

  let buffer = "";
  try {
    for (;;) {
      const { value, done } = await Promise.race([reader.read(), stop]);
      if (done) {
        break;
      }
      buffer += decoder.decode(value, { stream: true });
      let frameEnd;
      while ((frameEnd = FRAME_END.exec(buffer)) !== null) {
        const frame = buffer.slice(0, frameEnd.index);
        buffer = buffer.slice(frameEnd.index + frameEnd[0].length);
        const parsed = parseFrame(frame);
        if (parsed) {
          yield parsed;
        }
      }
    }
    // The stream may end without a trailing blank line; flush what is left as
    // the final frame instead of dropping it.
    buffer += decoder.decode();
    if (buffer.trim() !== "") {
      const parsed = parseFrame(buffer);
      if (parsed) {
        yield parsed;
      }
    }
  } finally {
    if (detachExternal) {
      detachExternal();
    }
    try {
      await reader.cancel();
    } catch {
      // The reader may already be closed after a clean end or a stop.
    }
  }
}
