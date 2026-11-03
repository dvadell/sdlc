import { state } from "./state.js";
import { getConfig, getTree } from "./api.js";

// When true the user-facing banner is the only error surface and the browser's
// native error reporting is suppressed; derived from the document element so a
// deployment can opt in without a rebuild. Native reporting stays on while it is
// not set to "suppress".
const SUPPRESS_NATIVE_ERRORS =
  document.documentElement.dataset.errorBanner === "suppress";

const projectName = document.querySelector("#project-name");
const aiStatus = document.querySelector("#ai-status");
const sidebar = document.querySelector("#sidebar");
const editor = document.querySelector("#editor");
const banner = document.querySelector("#error-banner");
const app = document.querySelector("#app");
const chatPlaceholder = document.querySelector("#chat-placeholder");

function renderHeader() {
  const config = state.config;
  if (!config) {
    return;
  }
  projectName.textContent = config.project_name ?? "sdlc";
  const model = config.model ?? "unknown";
  aiStatus.textContent = config.ai_configured ? `AI ready (${model})` : "AI not configured";
}

function renderSidebarPlaceholder() {
  sidebar.replaceChildren();
  const paragraph = document.createElement("p");
  paragraph.textContent = "The document tree will appear here once the project has a layout.";
  sidebar.appendChild(paragraph);
}

function renderEditorPlaceholder() {
  editor.replaceChildren();
  const paragraph = document.createElement("p");
  paragraph.textContent = "Select a document from the sidebar to start editing.";
  editor.appendChild(paragraph);
}

function showError(error) {
  const detail = error instanceof Error ? error.message : String(error);
  const code = error && error.code ? ` (${error.code})` : "";
  banner.textContent = `The request failed${code}: ${detail}`;
  renderDismiss();
  banner.hidden = false;
  app.style.paddingTop = `${banner.offsetHeight}px`;
  app.classList.add("error-banner-open");
}

function renderDismiss() {
  const dismiss = document.createElement("button");
  dismiss.type = "button";
  dismiss.textContent = "Dismiss";
  dismiss.addEventListener("click", () => hideError());
  banner.appendChild(dismiss);
}

function hideError() {
  banner.hidden = true;
  app.style.paddingTop = "";
  app.classList.remove("error-banner-open");
}

function renderChatPlaceholder() {
  chatPlaceholder.textContent = state.chat.placeholder;
}

function installErrorBanner() {
  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onUnhandledRejection);
}

function normalizeError(value, fallback = "a request failed") {
  if (value instanceof Error) {
    return value;
  }
  const message = typeof value === "string" ? value : value?.message ?? fallback;
  return new Error(message);
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

async function boot() {
  if (
    !banner ||
    !app ||
    !sidebar ||
    !editor ||
    !chatPlaceholder ||
    !projectName ||
    !aiStatus
  ) {
    console.error("The frontend shell is missing required elements; skipping boot.");
    return;
  }

  hideError();
  renderSidebarPlaceholder();
  renderEditorPlaceholder();
  renderChatPlaceholder();
  installErrorBanner();

  const results = await Promise.allSettled([getConfig(), getTree()]);
  const failures = results
    .filter((result) => result.status === "rejected")
    .map((result) => result.reason);

  const [configResult, treeResult] = results;
  const config = configResult.status === "fulfilled" ? configResult.value : null;
  const tree = treeResult.status === "fulfilled" ? treeResult.value : null;

  if (config) {
    state.config = config;
    renderHeader();
  }

  if (tree) {
    state.tree = tree;
  }

  if (failures.length > 0) {
    console.error("Boot failed:", failures);
    showError(
      failures.length === 1
        ? failures[0]
        : new Error(
            failures
              .map((failure) =>
                failure instanceof Error ? failure.message : String(failure),
              )
              .join("; "),
          ),
    );
  }
}

boot().catch((error) => {
  console.error("Boot failed:", error);
  // Surface post-bootstrap errors to the user via the banner when it is
  // already installed, otherwise fall back to logging.
  if (banner && app) {
    showError(error instanceof Error ? error : new Error(String(error)));
  }
});
