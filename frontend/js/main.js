import { state } from "./state.js";
import { getConfig, getTree } from "./api.js";
import { renderSidebar, setEditorLoadDocument } from "./sidebar.js";
import { loadDocument, save } from "./editor.js";
import { hideError, installErrorBanner, showError } from "./errors.js";

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

function renderEditorPlaceholder() {
  editor.replaceChildren();
  const paragraph = document.createElement("p");
  paragraph.textContent = "Select a document from the sidebar to start editing.";
  editor.appendChild(paragraph);
}

function renderChatPlaceholder() {
  chatPlaceholder.textContent = state.chat.placeholder;
}

// The global save shortcut: Ctrl/Cmd+S saves and suppresses the browser save
// dialog. Ctrl/Cmd+Enter is deliberately not bound.
function installSaveShortcut() {
  window.addEventListener("keydown", (event) => {
    const key = event.key?.toLowerCase?.() ?? "";
    if (
      (event.ctrlKey || event.metaKey) &&
      !event.shiftKey &&
      !event.altKey &&
      key === "s"
    ) {
      event.preventDefault();
      save().catch((error) => showError(error));
    }
  });
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
  renderSidebar();
  renderEditorPlaceholder();
  renderChatPlaceholder();
  installErrorBanner();
  setEditorLoadDocument((path) => loadDocument(path));
  installSaveShortcut();

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
    renderSidebar();
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
