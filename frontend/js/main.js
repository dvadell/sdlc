import { state } from "./state.js";
import { getConfig, getTree } from "./api.js";
import {
  renderSidebar,
  setEditorLoadDocument,
  starterNeedsScaffolding,
  refreshTree,
} from "./sidebar.js";
import { loadDocument, save, setRefreshTree } from "./editor.js";
import { hideError, installErrorBanner, showError } from "./errors.js";
import { renderChat } from "./chat.js";

const projectName = document.querySelector("#project-name");
const aiStatus = document.querySelector("#ai-status");
const sidebar = document.querySelector("#sidebar");
const editor = document.querySelector("#editor");
const banner = document.querySelector("#error-banner");
const app = document.querySelector("#app");
const chat = document.querySelector("#chat");

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
    !chat ||
    !projectName ||
    !aiStatus
  ) {
    console.error("The frontend shell is missing required elements; skipping boot.");
    return;
  }

  hideError();
  renderSidebar();
  renderEditorPlaceholder();
  renderChat();
  installErrorBanner();
  setEditorLoadDocument((path) => loadDocument(path));
  // The editor refreshes the document tree after status-changing saves through
  // the hook the editor owns; the sidebar owns the refresh implementation.
  setRefreshTree(() => refreshTree());
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
    // The chat panel reads the AI config, so it re-renders once it is known
    // whether the AI is configured (ticket 0038).
    renderChat();
  }

  if (tree) {
    state.tree = tree;
    renderSidebar();
  }

  if (tree && starterNeedsScaffolding(tree)) {
    // The empty-layout onboarding replaces the editor content until the person
    // creates the starter documents (ticket 0026).
    editor.replaceChildren();
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
