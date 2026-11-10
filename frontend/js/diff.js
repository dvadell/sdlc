//! The reviewable diff of one proposal and its whole-proposal actions (ticket
//! 0045).
//!
//! A proposal arrives through the chat stream and replaces the editor area with
//! one section per proposed file; the person accepts or rejects the whole thing.
//! The diff is plain text: every line is inserted with `textContent`, so text the
//! model wrote can never become markup.

import { state } from "./state.js";
import { ApiError, acceptProposal, rejectProposal } from "./api.js";
import { showError } from "./errors.js";
import { loadDocument, refreshTree, restoreEditor } from "./editor.js";

// The proposal whose diff is on screen; null when the view is dismissed or the
// decision settled.
let viewProposal = null;
// Whether the on-screen diff was refused because the documents changed since the
// proposal was made; a refused diff offers the reject only (ticket 0045).
let viewConflict = false;
// The outcome of an accept that stopped midway; the view lists what was written
// and what failed (ticket 0045).
let viewPartial = null;
// Whether a whole-proposal accept or reject is in flight; the actions of the
// diff are disabled while it runs (ticket 0045).
let actionInFlight = false;
// The accept and reject buttons of the on-screen diff; the actions render from
// them, so a decision can re-render the very buttons the person clicked.
let viewAcceptButton = null;
let viewRejectButton = null;
// The conflict and partial notices of the on-screen diff; they are built with the
// rest of the view in `showProposal` and re-rendered in place by a decision that
// changes their outcome, so the decision must not re-query the editor area.
let viewConflictElement = null;
let viewPartialElement = null;

// Shows the diff of one proposal in the editor area and arms the accept and
// reject actions of the whole proposal (ticket 0045).
export function showProposal(proposal) {
  state.proposal = proposal ?? state.proposal;
  viewProposal = state.proposal;
  viewConflict = false;
  viewPartial = null;
  const element = document.querySelector("#editor");
  if (!element) {
    return;
  }

  element.replaceChildren();
  const view = document.createElement("div");
  view.className = "proposal";
  const pending = document.createElement("p");
  pending.id = "proposal-pending";
  pending.className = "proposal-pending";
  pending.textContent = "A proposal is pending your review.";
  const warnings = document.createElement("div");
  warnings.className = "proposal-warnings";
  const files = document.createElement("div");
  files.className = "proposal-files";
  const conflict = document.createElement("div");
  conflict.id = "proposal-conflict";
  conflict.className = "proposal-conflict";
  conflict.hidden = true;
  const partial = document.createElement("div");
  partial.id = "proposal-partial";
  partial.className = "proposal-partial";
  partial.hidden = true;
  viewConflictElement = conflict;
  viewPartialElement = partial;
  const actions = document.createElement("div");
  actions.className = "proposal-actions";
  const acceptButton = document.createElement("button");
  acceptButton.type = "button";
  acceptButton.id = "proposal-accept";
  acceptButton.textContent = "Accept all";
  acceptButton.addEventListener("click", () => {
    void acceptAll();
  });
  const rejectButton = document.createElement("button");
  rejectButton.type = "button";
  rejectButton.id = "proposal-reject";
  rejectButton.textContent = "Reject";
  rejectButton.addEventListener("click", () => {
    void rejectDecision();
  });
  actions.appendChild(acceptButton);
  actions.appendChild(rejectButton);
  view.appendChild(pending);
  view.appendChild(warnings);
  view.appendChild(files);
  view.appendChild(conflict);
  view.appendChild(partial);
  view.appendChild(actions);
  element.appendChild(view);

  viewAcceptButton = acceptButton;
  viewRejectButton = rejectButton;
  renderProposalWarnings(warnings, viewProposal);
  renderProposalFiles(files, viewProposal.changes ?? []);
  renderProposalConflict(conflict, viewConflict);
  renderProposalPartial(partial, viewPartial);
  renderProposalActions();
}

// The warnings of the proposal, rendered above the diff: one text surface per
// skipped block, never markup.
function renderProposalWarnings(warningsElement, proposal) {
  warningsElement.replaceChildren();
  for (const warning of proposal?.warnings ?? []) {
    const message = document.createElement("p");
    message.className = "proposal-warning";
    message.textContent = String(warning);
    warningsElement.appendChild(message);
  }
}

// One section per proposed file, with the path and the action of the change and
// the diff lines of the change, colored by prefix. The `---`/`+++` headers and
// the `@@` hunk markers are context, not add or delete (ticket 0045).
function renderProposalFiles(filesElement, changes) {
  filesElement.replaceChildren();
  for (const change of changes) {
    const section = document.createElement("div");
    section.className = "proposal-file";
    section.dataset.path = change.path;
    const header = document.createElement("div");
    header.className = "proposal-file-header";
    const path = document.createElement("span");
    path.className = "proposal-file-path";
    path.textContent = change.path;
    const action = document.createElement("span");
    action.className = `proposal-file-action proposal-file-action-${change.action}`;
    action.textContent = change.action;
    header.appendChild(path);
    header.appendChild(action);
    const diff = document.createElement("div");
    diff.className = "proposal-file-diff";
    const lines = String(change.diff ?? "").split("\n");
    // The split keeps an empty final element for the line terminator that ends
    // the last line; a line terminator is not an empty line of its own, so drop
    // it instead of rendering a context row out of nothing.
    if (lines.length > 0 && lines[lines.length - 1] === "") {
      lines.pop();
    }
    for (const line of lines) {
      const row = document.createElement("div");
      row.className = diffLineClass(line);
      row.textContent = line;
      diff.appendChild(row);
    }
    section.appendChild(header);
    section.appendChild(diff);
    filesElement.appendChild(section);
  }
}

// The color of one diff line by prefix; unified-diff headers and hunk markers
// are context, exactly like the surrounding context lines.
function diffLineClass(line) {
  const text = String(line ?? "");
  if (text.startsWith("@@") || text.startsWith("--- ") || text.startsWith("+++ ")) {
    return "ctx";
  }
  if (text.startsWith("+")) {
    return "add";
  }
  if (text.startsWith("-")) {
    return "del";
  }
  return "ctx";
}

// The conflict notice of a refused diff, kept on screen with the reject only.
function renderProposalConflict(conflictElement, conflict) {
  conflictElement.hidden = !conflict;
  if (conflict) {
    conflictElement.textContent = "Files changed since this proposal was made. Ask the AI again.";
  }
}

// The partial-apply notice of a refused accept: the files that were written and
// the one that failed are listed apart.
function renderProposalPartial(partialElement, partial) {
  if (!partial) {
    partialElement.hidden = true;
    return;
  }
  partialElement.hidden = false;
  const written = Array.isArray(partial.applied) ? partial.applied : [];
  const failed = partial.failed ? String(partial.failed) : "";
  const text = [
    "The proposal apply stopped midway.",
    `Written: ${written.length > 0 ? written.join(", ") : "nothing"}.`,
    `Failed: ${failed || "unknown"}.`,
  ].join(" ");
  partialElement.textContent = text;
}

// The accept and reject actions: disabled while a decision request is in
// flight, and a refused (conflicted) diff offers the reject only.
function renderProposalActions() {
  const acceptButton = viewAcceptButton;
  const rejectButton = viewRejectButton;
  if (!acceptButton || !rejectButton) {
    return;
  }
  acceptButton.disabled = actionInFlight || viewConflict;
  rejectButton.disabled = actionInFlight;
  acceptButton.hidden = actionInFlight || viewConflict;
  if (viewPartial) {
    acceptButton.disabled = true;
    acceptButton.hidden = true;
  }
}

// Accepts the whole proposal: the route writes the files, and on success the
// tree is refreshed, the open affected document is reloaded, the proposal is
// cleared and the editor is restored (ticket 0045).
async function acceptAll() {
  if (!viewProposal || actionInFlight) {
    return;
  }
  actionInFlight = true;
  renderProposalActions();
  try {
    const proposalId = viewProposal.id;
    const openPath = state.current?.path;
    const report = await acceptProposal(proposalId);
    state.proposal = null;
    viewProposal = null;
    viewConflict = false;
    viewPartial = null;
    await refreshTree();
    if (openPath && Array.isArray(report?.applied) && report.applied.includes(openPath)) {
      // The accepted proposal wrote the open document, so it is reloaded from
      // the server with fresh content; the editor area is the editor again.
      await loadDocument(openPath);
    } else {
      restoreEditor();
    }
  } catch (error) {
    if (error instanceof ApiError) {
      if (error.code === "conflict") {
        // The documents changed since the proposal was made; the diff stays
        // visible with the explanation and the reject only.
        viewConflict = true;
        viewPartial = null;
        renderProposalConflict(viewConflictElement, viewConflict);
        renderProposalActions();
        return;
      }
      if (error.code === "partial_apply") {
        viewConflict = false;
        viewPartial = error;
        renderProposalActions();
        renderProposalPartial(viewPartialElement, viewPartial);
        return;
      }
      if (error.code === "not_found") {
        // The server restarted and forgot the proposal; the person is told and
        // the editor is restored.
        state.proposal = null;
        viewProposal = null;
        restoreEditor();
        showError(error);
        return;
      }
    }
    showError(error);
  } finally {
    actionInFlight = false;
    renderProposalActions();
  }
}

// Rejects the proposal: it is discarded and the editor is restored with the
// buffer of the person, unsaved text included (ticket 0045).
async function rejectDecision() {
  if (!viewProposal || actionInFlight) {
    return;
  }
  actionInFlight = true;
  renderProposalActions();
  try {
    await rejectProposal(viewProposal.id);
    state.proposal = null;
    viewProposal = null;
    viewConflict = false;
    viewPartial = null;
    restoreEditor();
  } catch (error) {
    if (error instanceof ApiError && error.code === "not_found") {
      state.proposal = null;
      viewProposal = null;
      restoreEditor();
      showError(error);
      return;
    }
    showError(error);
  } finally {
    actionInFlight = false;
    renderProposalActions();
  }
}
