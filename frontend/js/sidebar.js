// The document tree panel of the sidebar (ticket 0022).

import { state } from "./state.js";
import { createDoc, deleteDoc, getTree, moveDoc, scaffold } from "./api.js";
import { showError } from "./errors.js";
import { clearEditor } from "./editor.js";

// The groups of the document tree in the order the sidebar renders them. Tickets
// will be grouped under their epic from ticket 0053 on; the flat per-group entry
// lists keep that change a small one.
export const GROUP_ORDER = ["core", "epic", "feature", "decision", "ticket"];

const GROUP_LABELS = {
  core: "Core",
  epic: "Epics",
  feature: "Features",
  decision: "Decisions",
  ticket: "Tickets",
};

const PLACEHOLDER = "The document tree will appear here once the project has a layout.";

// The document kinds the sidebar can create, one creation affordance per group.
// The ticket 0052 replaces the free-text epic and feature selectors of the v1
// prompts with proper pickers.
const CREATABLE_GROUPS = {
  epic: { kind: "epic", label: "epic" },
  feature: { kind: "feature", label: "feature" },
  decision: { kind: "decision", label: "decision" },
  ticket: { kind: "ticket", label: "ticket" },
};

// The starter layout of a project: the core documents the scaffold creates, in
// the order the server writes them (ticket 0017). The first one missing from the
// tree is the path the empty state offers to create, so a partial project only
// fills the gaps and never touches files that already exist.
const STARTER_CORE_PATHS = [
  "sdlc/PRD.md",
  "sdlc/architecture.md",
  "sdlc/conventions.md",
  "AGENTS.md",
];

// The entry document a successful scaffold opens: the PRD leads the scaffolded
// project (ticket 0026).
const STARTER_PRD_PATH = "sdlc/PRD.md";

// The starter core documents the project is still missing, in scaffold order.
export function missingStarterPaths(tree) {
  const present = new Set((tree?.docs ?? []).map((doc) => doc.path));
  return STARTER_CORE_PATHS.filter((path) => !present.has(path));
}

// Whether the project still lacks the starter layout and should show the empty
// state instead of a document tree (ticket 0026).
export function starterNeedsScaffolding(tree) {
  return missingStarterPaths(tree).length > 0;
}

// One mutation runs at a time; its buttons are disabled while the request is in
// flight and re-enabled afterwards. The flag lives at module level because the
// tree refresh inside a mutation replaces the buttons mid-flight.
let actionsDisabled = false;

function setActionsDisabled(disabled) {
  actionsDisabled = disabled;
  const buttons = document.querySelectorAll(
    "#sidebar .new-doc, #sidebar .scaffold, #sidebar .doc-action",
  );
  for (const button of buttons) {
    button.disabled = disabled;
  }
}

// The editor loads the selected document through this hook. The editor module
// owns it from ticket 0023 on, so the sidebar keeps a stub until then.
let loadDocumentHandler = () => {};

export function setEditorLoadDocument(handler) {
  loadDocumentHandler = handler;
}

function loadDocument(path) {
  try {
    const result = loadDocumentHandler(path);
    if (result && typeof result.then === "function") {
      return result.catch((error) => {
        console.error("Loading document failed:", error);
        throw error;
      });
    }
    return Promise.resolve(result);
  } catch (error) {
    console.error("Loading document failed:", error);
    return Promise.reject(error);
  }
}

// The slug of a document title, mirroring the server's slugify: lowercase ASCII
// words joined by single hyphens, punctuation dropped and no empty words.
function slugifyTitle(title) {
  const slug = String(title)
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .map((word) => word.replace(/[^a-z0-9]/g, ""))
    .filter((word) => word.length > 0)
    .join("-");
  return slug;
}

// The destination of a rename, derived from the new title while keeping the
// document number of numbered kinds and the epic of a ticket.
function renameDestination(entry, newTitle) {
  const slug = slugifyTitle(newTitle);
  const segments = entry.path.split("/");
  if (entry.kind === "epic") {
    const epic = segments[2] ?? "";
    const number = epic.split("-")[0] ?? "";
    return `sdlc/epics/${number}-${slug}/description.md`;
  }
  if (entry.kind === "ticket") {
    const epic = segments[2] ?? "";
    const ticket = segments[4] ?? "";
    const number = ticket.replace(/\.md$/, "").split("-")[0] ?? "";
    return `sdlc/epics/${epic}/tickets/${number}-${slug}.md`;
  }
  if (entry.kind === "decision") {
    const decision = segments[2] ?? "";
    const number = decision.replace(/\.md$/, "").split("-")[0] ?? "";
    return `sdlc/decisions/${number}-${slug}.md`;
  }
  return `sdlc/features/${slug}.md`;
}

// The feature slug an entry belongs to: tickets carry their normalized
// `feature` binding, features take it from their own path stem.
function entryFeatureSlug(entry) {
  const binding = entry?.feature;
  if (typeof binding === "string" && binding.length > 0) {
    return binding.replace(/\.md$/, "");
  }
  const segments = entry?.path?.split("/") ?? [];
  const stem = segments[segments.length - 1] ?? "";
  return stem.replace(/\.md$/, "");
}

// How many tickets still name the given feature, for the delete confirmation.
function orphanedTicketCount(feature) {
  const docs = state.tree?.docs ?? [];
  return docs.filter((doc) => doc?.kind === "ticket" && doc?.feature === feature).length;
}

// The confirmation text of a delete, including the path and the ticket count
// of an epic or feature that would be affected.
function deleteMessage(entry) {
  const path = entry.path;
  let message = `Delete ${entry.kind} "${entry.title}" at ${path}? This cannot be undone.`;
  if (entry.kind === "epic") {
    const epic = path.split("/")[2] ?? "";
    const count = ticketCount(state.tree?.docs ?? [], epic);
    if (count > 0) {
      message += ` ${count} ${count === 1 ? "ticket is" : "tickets are"} still assigned; delete or move them first.`;
    }
  }
  if (entry.kind === "feature") {
    const feature = entryFeatureSlug(entry);
    const orphaned = orphanedTicketCount(feature);
    if (orphaned > 0) {
      message += ` ${orphaned} ${orphaned === 1 ? "ticket still references it" : "tickets still reference it"}.`;
    }
  }
  return message;
}

// After a mutation: refresh the tree, select the created or renamed document and
// reload it, so the editor always follows the path the server answered with.
async function selectAfterMutation(path) {
  state.current.path = path;
  await refreshTree();
  await loadDocument(path);
}

// Creates a document of one group through the prompt flow of the v1 actions.
async function createFromGroup(group) {
  const kind = CREATABLE_GROUPS[group].kind;
  const title = window.prompt(`New ${kind} title:`);
  if (title === null) {
    return;
  }
  const trimmedTitle = String(title).trim();
  if (trimmedTitle.length === 0) {
    // window.prompt cannot validate inline; an empty title never reaches the server.
    return;
  }

  let epic;
  let feature;
  if (kind === "ticket") {
    const epicPrompt = window.prompt("Epic directory (for example 0001-foundation):", epicDefault());
    if (epicPrompt === null) {
      return;
    }
    epic = String(epicPrompt).trim();
    if (epic.length === 0) {
      showError(new Error("A ticket needs an epic directory."));
      return;
    }
    const featurePrompt = window.prompt("Feature slug (optional):", "");
    if (featurePrompt === null) {
      return;
    }
    feature = String(featurePrompt).trim() || undefined;
  }

  setActionsDisabled(true);
  try {
    const created = await createDoc(kind, trimmedTitle, epic, feature);
    if (typeof created?.path !== "string" || created.path.length === 0) {
      throw new Error("The server did not return a document path.");
    }
    await selectAfterMutation(created.path);
  } catch (error) {
    showError(error);
  } finally {
    setActionsDisabled(false);
  }
}

// Renames a document through the prompt flow, keeping the content and following
// the returned path after the move.
async function renameFromEntry(entry) {
  const newTitle = window.prompt(`Rename ${entry.kind} "${entry.title}" to:`, entry.title);
  if (newTitle === null) {
    return;
  }
  const title = String(newTitle).trim();
  if (title.length === 0) {
    // An empty title is refused by the server; never send it.
    return;
  }
  const slug = slugifyTitle(title);
  if (slug.length === 0) {
    // A title that slugifies to nothing can never name a document; refuse it
    // before building a destination path the server would reject.
    showError(new Error("The new title does not produce a usable document name."));
    return;
  }
  const to = renameDestination(entry, title);

  setActionsDisabled(true);
  try {
    const moved = await moveDoc(entry.path, to);
    if (typeof moved?.path !== "string" || moved.path.length === 0) {
      throw new Error("The server did not return the renamed document path.");
    }
    await selectAfterMutation(moved.path);
  } catch (error) {
    showError(error);
  } finally {
    setActionsDisabled(false);
  }
}

// Deletes a document after the person confirmed, clearing the editor when the
// open document was the deleted one.
async function deleteFromEntry(entry) {
  if (!window.confirm(deleteMessage(entry))) {
    return;
  }

  setActionsDisabled(true);
  try {
    await deleteDoc(entry.path);
    if (state.current?.path === entry.path) {
      // The open document is gone, so no stale content, hash or dirty state may
      // survive the deletion (ticket 0025).
      clearEditor();
    }
    await refreshTree();
  } catch (error) {
    showError(error);
  } finally {
    setActionsDisabled(false);
  }
}

// The epic directory offered as the default target of a ticket creation.
function epicDefault() {
  const epics = (state.tree?.docs ?? []).filter((doc) => doc?.kind === "epic");
  return epics.length > 0 ? epics[0].path.split("/")[2] ?? "" : "";
}

function createCurrent() {
  return { path: null, content: "", savedContent: "", hash: null };
}

let container = null;
let boundElement = null;
let refreshGeneration = 0;

function getContainer() {
  if (container !== null && !container.isConnected) {
    if (boundElement === container) {
      container.removeEventListener("click", onClick);
    }
    container = null;
  }
  if (container === null) {
    container = document.querySelector("#sidebar");
    boundElement = null;
  }
  return container;
}

function bindClickDelegation() {
  const element = getContainer();
  if (element === null || element === boundElement) {
    return;
  }
  if (boundElement !== null) {
    boundElement.removeEventListener("click", onClick);
  }
  element.addEventListener("click", onClick);
  boundElement = element;
}

function onClick(event) {
  const element = event.currentTarget;
  const row =
    event.target instanceof Element ? event.target.closest("[data-doc-path]") : null;
  if (!row) {
    return;
  }

  const entry = (state.tree?.docs ?? []).find((doc) => doc.path === row.dataset.docPath);
  if (!entry) {
    return;
  }

  const current = state.current ?? (state.current = createCurrent());
  const previousPath = current.path;
  const previousContent = current.content;
  const previousSavedContent = current.savedContent;
  const previousHash = current.hash;
  const previousDirty = state.dirty;
  current.path = entry.path;
  const rows = element.querySelectorAll(".doc-row");
  for (const candidate of rows) {
    candidate.classList.toggle("selected", candidate === row);
  }

  loadDocument(entry.path).catch(() => {
    const latest = state.current ?? (state.current = createCurrent());
    if (latest.path === entry.path) {
      latest.path = previousPath;
      latest.content = previousContent;
      latest.savedContent = previousSavedContent;
      latest.hash = previousHash;
      state.dirty = previousDirty;
    }
    row.classList.remove("selected");
    const freshRows = element.querySelectorAll(".doc-row");
    const stillPresent = [...freshRows].find((candidate) => candidate.dataset.docPath === previousPath);
    if (stillPresent) {
      stillPresent.classList.add("selected");
    }
  });
}

// The group a document kind renders under, or `null` for an unknown kind.
export function groupOf(kind) {
  return GROUP_ORDER.includes(kind) ? kind : null;
}

// The tree entries of every non-empty group, in [`GROUP_ORDER`] order.
export function groupEntries(docs) {
  const groups = new Map();
  for (const doc of docs ?? []) {
    const group = groupOf(doc?.kind);
    if (group === null) {
      continue;
    }
    const entries = groups.get(group);
    if (entries) {
      entries.push(doc);
    } else {
      groups.set(group, [doc]);
    }
  }

  const ordered = [];
  for (const group of GROUP_ORDER) {
    const entries = groups.get(group);
    if (entries && entries.length > 0) {
      ordered.push([group, entries]);
    }
  }

  return ordered;
}

// How many tickets the tree lists under the given epic directory.
export function ticketCount(docs, epic) {
  return (docs ?? []).filter((doc) => doc?.kind === "ticket" && doc?.epic === epic).length;
}

// The numeric prefix of a numbered document stem such as `0007-add-export`.
function numberedPrefix(stem) {
  const dash = stem.indexOf("-");
  if (dash <= 0) {
    return null;
  }
  const prefix = stem.slice(0, dash);
  return /^\d{4}$/.test(prefix) ? prefix : null;
}

// The document number and the display title of a tree entry, the number being
// the `0007` of an epic or ticket directory or decision file name.
export function entryTitle(entry) {
  if (!entry || typeof entry.path !== "string") {
    return { number: null, text: entry?.title ?? "" };
  }
  const segments = entry.path.split("/");
  const stem = (segments[segments.length - 1] ?? "").replace(/\.md$/, "");
  let number = null;
  if (entry.kind === "epic" && segments.length >= 4 && segments[1] === "epics") {
    number = numberedPrefix(segments[2] ?? "");
  } else if (entry.kind === "decision" || entry.kind === "ticket") {
    number = numberedPrefix(stem);
  }
  const rawTitle = entry?.title == null ? "" : String(entry.title);
  const text = number ? rawTitle.replace(/^\d{4}:\s*/, "") : rawTitle;

  return { number, text };
}

// The empty-layout onboarding of the sidebar: one sentence, the path that would
// be created and the explicit create action. Nothing is written until the person
// clicks the button, and scaffold failures surface in the global banner.
function renderOnboarding(tree) {
  const element = getContainer();
  if (element === null) {
    return;
  }

  element.replaceChildren();

  const message = document.createElement("p");
  message.textContent =
    "This project has no starter document layout yet, so there is nothing to edit.";
  const path = document.createElement("p");
  const missing = missingStarterPaths(tree);
  path.textContent = `The starter documents would be created at ${missing[0]}.`;
  const button = document.createElement("button");
  button.type = "button";
  button.className = "scaffold";
  button.textContent = "Create starter documents";
  button.disabled = actionsDisabled;
  button.addEventListener("click", () => {
    void createStarterDocuments();
  });

  element.append(message, path, button);
}

// Creates the starter layout on an explicit click, then refreshes the tree and
// opens the PRD, so a successful scaffold replaces the empty state with the
// scaffolded project instead of leaving the onboarding behind (ticket 0026).
async function createStarterDocuments() {
  setActionsDisabled(true);
  try {
    await scaffold();
    await refreshTree();
    await loadDocument(STARTER_PRD_PATH);
  } catch (error) {
    showError(error);
  } finally {
    setActionsDisabled(false);
  }
}

// Rebuilds the sidebar from `state.tree`, highlighting the selected document.
export function renderSidebar() {
  const element = getContainer();
  if (element === null) {
    return;
  }

  element.replaceChildren();

  const tree = state.tree;
  if (!tree) {
    const paragraph = document.createElement("p");
    paragraph.textContent = PLACEHOLDER;
    element.appendChild(paragraph);
    return;
  }

  if (starterNeedsScaffolding(tree)) {
    renderOnboarding(tree);
    return;
  }

  bindClickDelegation();

  const groups = new Map();
  for (const [group, entries] of groupEntries(tree.docs)) {
    groups.set(group, entries);
  }

  for (const group of GROUP_ORDER) {
    const entries = groups.get(group);
    const creatable = CREATABLE_GROUPS[group] ?? null;
    const hasEntries = entries !== undefined && entries.length > 0;
    // Empty groups are hidden, except the creatable ones, whose purpose is the
    // creation of the very first document of that kind.
    if (!hasEntries && !creatable) {
      continue;
    }

    const heading = document.createElement("h2");
    heading.textContent = GROUP_LABELS[group] ?? group;
    element.appendChild(heading);

    if (creatable) {
      element.appendChild(createNewButton(group));
    }

    const list = document.createElement("ul");
    for (const entry of entries ?? []) {
      list.appendChild(createRow(group, entry, tree.docs));
    }
    element.appendChild(list);
  }
}

// The per-group creation button; it prompts for the title and delegates to the
// create action of the group (ticket 0025).
function createNewButton(group) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "new-doc";
  button.textContent = `New ${CREATABLE_GROUPS[group].label}`;
  button.disabled = actionsDisabled;
  button.addEventListener("click", () => {
    void createFromGroup(group);
  });
  return button;
}

function createRow(group, entry, docs) {
  const item = document.createElement("li");
  const row = document.createElement("button");
  row.type = "button";
  row.className = "doc-row";
  const currentPath = state.current?.path ?? null;
  if (currentPath === entry.path) {
    row.classList.add("selected");
  }

  const title = entryTitle(entry);
  if (title.number) {
    const number = document.createElement("span");
    number.className = "doc-number";
    number.textContent = title.number;
    row.appendChild(number);
  }

  const label = document.createElement("span");
  label.className = "doc-title";
  label.textContent = title.text;
  row.appendChild(label);

  if (entry.status) {
    const badge = document.createElement("span");
    badge.className = "doc-badge";
    badge.textContent = entry.status;
    row.appendChild(badge);
  }

  if (group === "epic") {
    // The backend sets a ticket's `epic` to this directory's name, so the two
    // must stay aligned if the layout ever changes.
    const epic = entry.path.split("/")[2] ?? "";
    const count = ticketCount(docs, epic);
    const countLabel = document.createElement("span");
    countLabel.className = "doc-count";
    countLabel.textContent = `${count} ${count === 1 ? "ticket" : "tickets"}`;
    row.appendChild(countLabel);
  }

  row.dataset.docPath = entry.path;
  row.title = entry.path;

  item.appendChild(row);
  if (group !== "core") {
    item.appendChild(createRowActions(entry));
  }
  return item;
}

// The rename and delete actions of one document row; core documents expose no
// mutation actions and only the editor can change them.
function createRowActions(entry) {
  const actions = document.createElement("div");
  actions.className = "doc-actions";

  const rename = document.createElement("button");
  rename.type = "button";
  rename.className = "doc-action";
  rename.textContent = "Rename";
  rename.disabled = actionsDisabled;
  rename.addEventListener("click", () => {
    void renameFromEntry(entry);
  });

  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "doc-action";
  remove.textContent = "Delete";
  remove.disabled = actionsDisabled;
  remove.addEventListener("click", () => {
    void deleteFromEntry(entry);
  });

  actions.appendChild(rename);
  actions.appendChild(remove);
  return actions;
}

function treeContains(tree, path) {
  return (tree.docs ?? []).some((doc) => doc.path === path);
}

// Fetches a fresh tree and re-renders, preserving the selected path when it
// still exists and clearing it when the selected document was deleted.
export async function refreshTree() {
  const generation = ++refreshGeneration;
  let tree;
  try {
    tree = await getTree();
  } catch (error) {
    if (generation !== refreshGeneration) {
      // A newer refresh superseded this one; swallow the stale failure.
      return state.tree;
    }
    // A failed refresh keeps the last rendered tree and selection; the caller
    // decides how to surface the failure.
    throw error;
  }

  if (generation !== refreshGeneration) {
    return state.tree;
  }

  if (!tree || !Array.isArray(tree.docs)) {
    throw new Error("The server returned an invalid document tree.");
  }

  state.tree = tree;
  const current = state.current ?? (state.current = createCurrent());
  if (current.path && !treeContains(tree, current.path)) {
    if (state.dirty) {
      // Keep in-memory edits for the editor to reconcile instead of silently
      // discarding them (the editor owns dirty-state reconciliation from 0023).
      return tree;
    }
    current.path = null;
    current.content = "";
    current.savedContent = "";
    current.hash = null;
    state.dirty = false;
  }

  renderSidebar();
  return tree;
}
