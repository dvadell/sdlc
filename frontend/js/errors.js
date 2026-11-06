//! The user-facing error and warning banner of the frontend (tickets 0021 and
//! 0023, plus the status-save warnings of ticket 0030).

// When true the user-facing banner is the only error surface and the browser's
// native error reporting is suppressed; derived from the document element so a
// deployment can opt in without a rebuild. Native reporting stays on while it is
// not set to "suppress".
const SUPPRESS_NATIVE_ERRORS =
  document.documentElement.dataset.errorBanner === "suppress";

const banner = document.querySelector("#error-banner");
const app = document.querySelector("#app");

// The safe string coercion shared by the banner surfaces: the text is a text
// surface, so a hostile or malformed payload must never throw or render the
// literals "null"/"undefined" as if they were content (ticket 0030, final closing
// review round).
function safeString(value, fallback = "") {
  try {
    return String(value ?? fallback);
  } catch {
    return fallback;
  }
}

function renderBanner(text, kind) {
  if (!banner || !app) {
    return;
  }
  // The banner text is a text surface, so coerce it through the shared safe
  // helper as the last line of defense; a hostile or malformed payload (a proxy, an
  // accessor or an object whose `toString` throws) must never break the render and
  // leave a half-updated banner behind (ticket 0030, final review round).
  const bannerText = safeString(text);
  // Clear the banner before re-rendering so a repeated warning or error replaces
  // the previous content instead of stacking another dismiss button on top of it.
  banner.replaceChildren();
  banner.textContent = bannerText;
  banner.classList.toggle("warning", kind === "warning");
  renderDismiss();
  banner.hidden = false;
  // The reserved space is the banner CSS variable the open state applies, so it
  // never depends on a height measured after the DOM mutation and class toggle
  // (ticket 0030, final review round).
  app.style.paddingTop = "";
  app.classList.add("banner-open");
}

export function showError(error) {
  if (!banner || !app) {
    return;
  }
  // The error may be hostile or malformed (ticket 0030, final review round); read
  // the message and the code in guarded blocks so a throwing accessor or a
  // malformed code cannot turn a reported failure into an unhandled exception or
  // discard a message that was parsed successfully, and fall back to a client-safe
  // message when the message coercion fails.
  let detail = null;
  let code = "";
  let rawMessage = null;
  try {
    let raw;
    if (error instanceof Error) {
      raw = error.message;
    } else if (error !== null && typeof error === "object" && "message" in error) {
      raw = error.message;
    } else {
      raw = error;
    }
    rawMessage = raw === null || raw === undefined ? null : String(raw);
  } catch {
    rawMessage = null;
  }
  detail = rawMessage;
  try {
    const rawCode = error && error.code !== undefined && error.code !== null ? error.code : null;
    // The code is checked by presence, so a legitimate falsy code such as `0` is
    // reported instead of dropped by a truthiness check (ticket 0030, final
    // review round).
    if (rawCode !== undefined && rawCode !== null) {
      code = ` (${String(rawCode)})`;
    }
  } catch {
    // A malformed code never discards the message that was parsed.
  }
  renderBanner(detail === null ? "The request failed." : `The request failed${code}: ${detail}`, "error");
}

// Shows a non-fatal warning through the same banner surface, for outcomes the
// person should know about without the request having failed (ticket 0030).
export function showWarning(text) {
  if (!banner || !app) {
    return;
  }
  // The warning text is a text surface, so coerce it through the shared safe
  // helper before interpolating it; a non-string payload must never render as
  // `[object Object]` or throw on its way into the banner, so the coercion lives
  // in a `try` and falls back like the error and warning text coercion of the
  // other surfaces (ticket 0030, final review round).
  const warningText = safeString(text);
  renderBanner(`Warning: ${warningText}`, "warning");
}

// Shows the warnings of a response when there is a message left: the coercion
// lives in `warningsToText` and the empty/absent case never opens the banner, so
// every mutation path keeps a single call site for the warning outcome (ticket
// 0030, final review round).
export function showWarnings(warnings) {
  const text = warningsToText(warnings);
  if (text) {
    showWarning(text);
  }
}

// Shows the warnings of a successful mutation response: the create and rename
// outcomes surface the feature-index refresh outcome the same way, so one helper
// keeps their warning handling from drifting if the warning structure changes
// (ticket 0030).
export function showResponseWarnings(response) {
  showWarnings(response?.warnings);
}

// The warning text of a response: the server reports non-fatal outcomes (for
// example a feature-index refresh) through a warning list. Each payload is
// coerced to a string first so object payloads never render as
// `[object Object]`, an `Error` instance keeps its message instead of
// serializing to an empty object, and a circular or otherwise unserializable
// payload falls back to its string form; blank entries are dropped and an
// absent or empty list warns nothing.
export function warningsToText(warnings) {
  if (!Array.isArray(warnings)) {
    return "";
  }
  const texts = [];
  for (const warning of warnings) {
    let message = "";
    try {
      // The whole per-entry coercion is guarded, so an exotic payload (a proxy,
      // an accessor or an object whose `toString`/`message` getter throws) cannot
      // throw out of the loop and break the mutation path that called this helper.
      let text;
      if (warning instanceof Error || Object.prototype.toString.call(warning) === "[object Error]") {
        // `instanceof Error` is realm-specific, so an Error from another realm fails
        // the check and would be JSON-serialized, usually losing its message; the
        // toString tag identifies errors of any realm.
        text = warning.message ?? String(warning);
      } else if (warning !== null && typeof warning === "object") {
        try {
          text = JSON.stringify(warning);
        } catch {
          text = String(warning);
        }
        // `JSON.stringify` returns `undefined` for values it cannot serialize to
        // JSON (a function or symbol input, or an object whose `toJSON` returns
        // undefined) without throwing; fall back to the string form so the entry is
        // not silently dropped as a blank warning.
        if (text === undefined) {
          text = String(warning);
        }
      } else {
        // A missing, null or undefined entry is blank like an empty one; do not
        // fall through to the string coercion, which would render the literals
        // "null" and "undefined" as if they were warnings.
        text = warning === null || warning === undefined ? "" : String(warning);
      }
      message = safeString(text).trim();
    } catch {
      // The coercion of this entry failed; fall back to an empty entry so the
      // banner can still be rendered with the remaining warnings.
      message = "";
    }
    if (message.length > 0) {
      texts.push(message);
    }
  }
  return texts.join(" ");
}

export function renderDismiss() {
  if (!banner || !app) {
    return;
  }
  const dismiss = document.createElement("button");
  dismiss.type = "button";
  dismiss.textContent = "Dismiss";
  dismiss.addEventListener("click", () => hideError());
  banner.appendChild(dismiss);
}

export function hideError() {
  if (!banner || !app) {
    return;
  }
  banner.hidden = true;
  banner.classList.remove("warning");
  app.style.paddingTop = "";
  app.classList.remove("banner-open");
}

function onError(event) {
  console.error("Unhandled error:", event.error ?? event.message, event.filename, event.lineno);
  if (SUPPRESS_NATIVE_ERRORS) {
    event.preventDefault();
  }
  showError(normalizeError(event.error ?? event.message));
}

function onUnhandledRejection(event) {
  console.error("Unhandled rejection:", event.reason);
  if (SUPPRESS_NATIVE_ERRORS) {
    event.preventDefault();
  }
  showError(normalizeError(event.reason, "a request failed"));
}

function normalizeError(value, fallback = "a request failed") {
  if (value instanceof Error) {
    return value;
  }
  const message = typeof value === "string" ? value : value?.message ?? fallback;
  return new Error(message);
}

export function installErrorBanner() {
  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onUnhandledRejection);
}
