// The document tree panel of the sidebar (ticket 0022).

import { state } from "./state.js";
import { getTree } from "./api.js";

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
  const previousHash = current.hash;
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
      latest.hash = previousHash;
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

// Rebuilds the sidebar from `state.tree`, highlighting the selected document.
export function renderSidebar() {
  const element = getContainer();
  if (element === null) {
    return;
  }

  element.replaceChildren();

  const tree = state.tree;
  if (!tree || !tree.layout_exists) {
    const paragraph = document.createElement("p");
    paragraph.textContent = PLACEHOLDER;
    element.appendChild(paragraph);
    return;
  }

  bindClickDelegation();

  for (const [group, entries] of groupEntries(tree.docs)) {
    const heading = document.createElement("h2");
    heading.textContent = GROUP_LABELS[group] ?? group;
    element.appendChild(heading);

    const list = document.createElement("ul");
    for (const entry of entries) {
      list.appendChild(createRow(group, entry, tree.docs));
    }
    element.appendChild(list);
  }
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
  return item;
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
