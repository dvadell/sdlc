//! The user-facing error banner of the frontend (tickets 0021 and 0023).

// When true the user-facing banner is the only error surface and the browser's
// native error reporting is suppressed; derived from the document element so a
// deployment can opt in without a rebuild. Native reporting stays on while it is
// not set to "suppress".
const SUPPRESS_NATIVE_ERRORS =
  document.documentElement.dataset.errorBanner === "suppress";

const banner = document.querySelector("#error-banner");
const app = document.querySelector("#app");

export function showError(error) {
  if (!banner || !app) {
    return;
  }
  const detail = error instanceof Error ? error.message : String(error);
  const code = error && error.code ? ` (${error.code})` : "";
  banner.textContent = `The request failed${code}: ${detail}`;
  renderDismiss();
  banner.hidden = false;
  app.style.paddingTop = `${banner.offsetHeight}px`;
  app.classList.add("error-banner-open");
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
  app.style.paddingTop = "";
  app.classList.remove("error-banner-open");
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
