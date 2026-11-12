//! The document editor: textarea, dirty state, save and preview (tickets 0023
//! and 0024), plus the save conflict banner (ticket 0027) and the status control
//! of features, epics, decisions and tickets (tickets 0030, 0032 and 0052), and
//! the read-only supersedes line of decisions (ticket 0032) and the read-only
//! epic and optional feature links of tickets (ticket 0052).

import { state } from "./state.js";
import { ApiError, getDoc, saveDoc } from "./api.js";
import { showError, showWarning, showWarnings } from "./errors.js";
import { STATUS_OPTIONS, statusKindOf, decisionNumberOf, epicDirectoryOf } from "./statuses.js";

// The textarea of the selected document; null until a document is loaded.
let textarea = null;
// The save button of the toolbar; null until a document is loaded.
let saveButton = null;
// The preview toggle button of the toolbar; null until a document is loaded.
let previewButton = null;

// The save button is disabled while a save is in flight; `save` ignores a second
// submit on top of that. The in-flight saves are tracked per document path
// together with the owner token of the operation that owns the entry, so a save
// of another document may take the slot while a save of a different document is in
// flight, and only the save that owns a path clears that path's entry
// (ticket 0030, final review round).
const saveInFlightOwners = new Map();
// The owner token issued to every save operation; a save stores the token of its
// entry and only clears the entry of a path in its `finally` while its token still
// owns it, so a save that settles cannot clear the ownership of a newer save of the
// same document (ticket 0030, final review round).
let saveOwnerToken = 0;

// Whether the rendered preview is shown instead of the source textarea
// (ticket 0024).
let previewVisible = false;
// The caret of the textarea when it was hidden behind the preview; restored on
// switching back to source.
let previewCaret = null;

// The save conflict banner of the selected document (ticket 0027); null until a
// document is loaded and rebuilt by `renderEditor`.
let conflictBanner = null;

// The refresh of the document tree after a save that can change a status
// (ticket 0030); the sidebar owns the refresh implementation and hands it to the
// editor through this hook, so this module keeps a single import direction and
// never imports the sidebar module directly. Decisions are status documents too
// (ticket 0032), so their saves refresh the tree and update the sidebar badge.
let refreshTreeHandler = null;

// Installs the tree refresh the sidebar owns; called once from the app bootstrap
// so the editor asks the sidebar to refresh through the hook.
export function setRefreshTree(handler) {
  refreshTreeHandler = handler;
}

// The tree refresh after a save that can change a status; the sidebar owns the
// refresh and installs it through the hook.
export function refreshTree() {
  const handler = refreshTreeHandler;
  if (handler) {
    // The registration contract is a handler value, so a best-effort handler may
    // return nothing or a non-Promise; coerce before attaching handlers, so a
    // successful save is never turned into a TypeError by a bare `.catch` on a
    // non-thenable (ticket 0030, final review round). The handler runs lazily
    // inside the promise callback, so a handler that throws synchronously rejects
    // the promise and the best-effort `.catch` below can absorb it instead of the
    // exception escaping before any promise exists (ticket 0030, final review round).
    return Promise.resolve().then(() => handler());
  }
  return Promise.resolve();
}

// The status select of the selected document; null until a document of a
// status kind is loaded and rebuilt by `renderEditor`.
let statusSelect = null;
// The document's status as read from its content; the select restores to it
// when a status save failed and the text stays dirty (ticket 0030). Null when
// the document has no valid status yet; the select then shows the placeholder.
let documentStatus = null;
// Whether a status save is in flight; the select is disabled while it runs, so
// a second change cannot stack a save on top of the first. While one is in
// flight no new status save may start, whatever document it belongs to, so two
// status saves of different documents cannot overlap and clobber each other
// (ticket 0030, final review round). `statusSavingPath` records the document the
// in-flight save belongs to, so a manual save of a different document is not
// blocked by it.
let statusSaving = false;
// The path of the document the in-flight status save was started against; null
// when no status save is in flight.
let statusSavingPath = null;
// The operation token of the status save that owns the in-flight state; a newer
// status save supersedes an older one, and only the completing operation clears
// the state when its token still matches (ticket 0030, fifth review round).
let statusSaveToken = 0;
// The read-only epic and optional feature links of an open ticket, rebuilt by
// the editor render and cleared with it (ticket 0052); null until a ticket is
// open.
let ticketLinks = null;

// Clears the in-flight status save state, but only while the completing
// operation is still the newest one; a stale operation settling after a newer
// one cannot clear the newer slot (ticket 0030, final review round).
function releaseStatusSlot(token) {
  if (token === statusSaveToken) {
    statusSaving = false;
    statusSavingPath = null;
  }
}

// The textarea normalizes line endings to `\n`; a document that was stored with
// CRLF has to be converted back before it reaches the server, or every save
// rewrites the file as LF and produces a whole-file diff.
function toSavedContent(value, saved = state.current?.savedContent ?? "") {
  if (saved.includes("\r\n")) {
    return value.replace(/\n/g, "\r\n");
  }
  return value;
}

function normalizeNewlines(value) {
  return value.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

// Splits `text` into lines that keep their own line terminator, so a rewrite
// that touches only the status line can rebuild the document without normalizing
// the line endings of the untouched lines. Only `\r\n` and `\n` are line
// endings; a lone `\r` is content, exactly like the server's splitter, which
// splits on `\n` and strips one trailing `\r` of a CRLF pair.
function splitLines(text) {
  const lines = [];
  const parts = text.split(/(\r\n|\n)/);
  const endsWithEol = /(?:\r\n|\n)$/.test(text);
  for (let index = 0; index + 1 < parts.length; index += 2) {
    lines.push({ value: parts[index], eol: parts[index + 1] });
  }
  if (!endsWithEol) {
    const last = parts.length - 1;
    if (last % 2 === 0) {
      lines.push({ value: parts[last], eol: "" });
    }
  }
  return lines;
}

// The dirty state is the textarea value against the content the document was
// loaded or last saved at; both are normalized so a CRLF document stays
// comparable. Without a connected textarea the comparison falls back to the
// stored content, so a dirty document can still be saved later.
function isDirty() {
  const current = (textarea?.isConnected ? textarea.value : (state.current?.content ?? ""));
  const saved = state.current?.savedContent ?? "";
  return normalizeNewlines(current) !== normalizeNewlines(saved);
}

function renderDirtyIndicator() {
  const indicator = document.querySelector("#doc-dirty");
  const path = state.current?.path;
  if (!indicator) {
    return;
  }
  if (!path || !state.dirty) {
    indicator.hidden = true;
    return;
  }
  const name = path.split("/").pop() ?? path;
  indicator.textContent = `• ${name}`;
  indicator.hidden = false;
}

// The vendored markdown renderer and sanitizer; both must be loaded before a
// preview can render.
function librariesReady() {
  return (
    typeof globalThis.marked?.parse === "function" &&
    typeof globalThis.DOMPurify?.sanitize === "function"
  );
}

// The front matter status key line: an optional indentation, the `status` key,
// the value and nothing else. The key may be indented with spaces like any other
// YAML mapping key (YAML forbids tab indentation, so only spaces are accepted,
// exactly like the server's YAML parser), and the value may carry a trailing
// comment that the YAML parser treats as not part of the value, so both shapes
// are accepted here exactly like the server's YAML parser does; whether an
// indented key is the top-level status of the block is decided by its
// indentation against the block's root indentation (ticket 0030, final review
// round).
const STATUS_LINE = /^([ ]*)status:(?:[ \t]+(.*))?$/;

// A front matter mapping key line: an optional indentation, the key and the
// key's colon, so the root indentation of a block is discovered from its first
// key line whatever key the server's YAML parser accepts; YAML permits arbitrary
// mapping keys, not only keys that start with a letter or underscore, so the
// discovery must not narrow the key start (ticket 0030, final review round).
const KEY_LINE = /^([ ]*)(?:"[^"]*"|'[^']*'|[^\s:#][^:]*)?:/;

// The YAML scalar of a status line: a trailing comment (`status: planned # note`)
// is not part of the value, so it is stripped before the value is unquoted and
// validated, just like the server's YAML parser does; a `#` starts a comment when
// it begins the scalar or follows a space or a tab, and not inside a quoted
// scalar.
function stripYamlComment(value) {
  let quote = null;
  for (let index = 0; index < value.length; index += 1) {
    const char = value[index];
    if (quote) {
      // An escaped character inside a quoted scalar (for example `\"`) is not the
      // end of the scalar, so the character after the backslash is skipped and
      // cannot close the quote or start a comment (ticket 0030, final review round).
      if (char === "\\") {
        index += 1;
      } else if (char === quote) {
        quote = null;
      }
    } else if (char === "#" && (index === 0 || value[index - 1] === " " || value[index - 1] === "\t")) {
      return value.slice(0, index);
    } else if (char === '"' || char === "'") {
      quote = char;
    }
  }
  return value;
}

// The root indentation of a front matter block: the indentation of its first
// key line, so a block whose root keys are indented (`  status: planned`) still
// has a top-level status while a key indented under an earlier key stays nested
// (ticket 0030, final review round). Shared by the read and rewrite paths so a
// fix in one cannot leave the other discovering a different root indentation.
function rootIndentOf(lines, from, to) {
  for (let index = from; index < to; index += 1) {
    const keyLine = KEY_LINE.exec(lines[index].value);
    if (keyLine) {
      return keyLine[1];
    }
  }
  return null;
}

// The front matter status of a document, or null when the block is absent or
// carries no valid status; `allowed` is the status vocabulary of the kind
// (ticket 0030).
export function readStatus(content, allowed) {
  const text = String(content ?? "").replace(/^\uFEFF/, "");
  const lines = splitLines(text);
  if (!lines.length || lines[0].value !== "---" || lines[0].eol === "") {
    // No front matter block, so there is no status to read; the opening
    // delimiter must be a line of its own with a line terminator after it, like
    // the server's splitter requires, so a bare `---` is body text.
    return null;
  }
  let status = null;
  // The number of status keys at the block's root indentation; a block carrying
  // more than one of them is ambiguous, exactly like a YAML mapping with two
  // status keys, which the server reads as carrying no status at all.
  let statusCount = 0;
  // The closing delimiter ends the front matter block; a later `status:` line
  // would be body text, not front matter, so the root indentation is discovered
  // only over the block's own key lines (ticket 0030, final review round). The
  // delimiter is only real when followed by a line terminator, like the server's
  // splitter requires, so a closing delimiter that ends the text without one
  // leaves the block absent.
  let closing = -1;
  for (let index = 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.value === "---") {
      if (line.eol === "") {
        // The block never closes, so there is no front matter to read.
        return null;
      }
      closing = index;
      break;
    }
  }
  if (closing === -1) {
    // An unterminated block is treated as absent, like the server does.
    return null;
  }
  // The root indentation of the block: the indentation of its first key line, so
  // a block whose root keys are indented (`  status: planned`) still has a
  // top-level status while a key indented under an earlier key stays nested
  // (ticket 0030, final review round). The first key may start with any character
  // a YAML key may start with, so the discovery agrees with the server's YAML
  // parser for any mapping of the block; the scan stops at the closing delimiter
  // so body content is never mistaken for front matter (ticket 0030, final review
  // round).
  const rootIndent = rootIndentOf(lines, 1, closing);
  for (let index = 1; index < closing; index += 1) {
    const line = lines[index];
    const match = STATUS_LINE.exec(line.value);
    const indent = /^[ ]*/.exec(line.value)[0];
    if (match && indent === rootIndent) {
      // Only a status key at the block's root indentation is the top-level
      // status; a key indented under an earlier key is a nested mapping key, not
      // the status of the block, and cannot conflict with it.
      statusCount += 1;
      if (statusCount > 1) {
        // A YAML mapping with two status keys is invalid, so the server reads no
        // status from the block; the client must not pick one either.
        return null;
      }
      const raw = (match[2] ?? "").trim();
      // A trailing comment is not part of the YAML value, so strip it before the
      // value is unquoted and validated; the server reads
      // `status: planned # note` as the status `planned`.
      const scalar = stripYamlComment(raw).trim();
      // YAML removes one balanced pair of surrounding quotes and nothing else,
      // so a quoted status is only a status when the quotes are a balanced pair.
      const value = /^(['"])(.*)\1$/.test(scalar) ? scalar.slice(1, -1) : scalar;
      status = allowed.includes(value) ? value : null;
    }
  }
  return status;
}

// The status value as a valid YAML scalar for the `status:` line: the helper is
// exported and therefore takes arbitrary or multi-line input, so the value is
// escaped before it is interpolated into the line, and a value that cannot stay a
// single-line plain scalar is quoted as a double-quoted YAML scalar (ticket
// 0030, final review round).
function statusYamlValue(status) {
  const value = String(status ?? "");
  // A plain single-line kebab slug is a valid YAML plain scalar and is
  // interpolated without quoting; anything else is written as a double-quoted
  // YAML scalar, escaping the value exactly once, so quoted or multi-line input
  // cannot desync or double-escape (ticket 0030, final review round).
  if (/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(value)) {
    return value;
  }
  return JSON.stringify(value);
}

// The front matter feature key line of a ticket: an optional indentation, the
// `feature` key and the key's value, so a hand-edited feature binding of a ticket
// (ticket 0052) can be read before it is validated against the existing features.
const FEATURE_LINE = /^([ ]*)feature:(?:[ \t]+(.*))?$/;

// The feature binding a ticket's front matter names, or null when the ticket
// carries no feature key or no front matter block; the value is the slug without
// the `.md` suffix, exactly like the server's ticket feature binding
// (ticket 0052).
export function ticketFeatureOf(content) {
  const text = String(content ?? "").replace(/^\uFEFF/, "");
  const lines = splitLines(text);
  if (!lines.length || lines[0].value !== "---" || lines[0].eol === "") {
    // No front matter block, so the ticket carries no feature binding.
    return null;
  }
  let closing = -1;
  for (let index = 1; index < lines.length; index += 1) {
    if (lines[index].value === "---" && lines[index].eol !== "") {
      closing = index;
      break;
    }
  }
  if (closing === -1) {
    // An unterminated block is treated as absent, like the server's parser.
    return null;
  }
  const rootIndent = rootIndentOf(lines, 1, closing);
  let feature = null;
  for (let index = 1; index < closing; index += 1) {
    const line = lines[index];
    const match = FEATURE_LINE.exec(line.value);
    const indent = /^[ ]*/.exec(line.value)[0];
    if (match && indent === rootIndent) {
      // Only a feature key at the block's root indentation is the ticket's own
      // binding; a key indented under another key is another mapping's subfield,
      // so only the root key of the block can carry the ticket's binding.
      const raw = (match[2] ?? "").trim();
      const scalar = stripYamlComment(raw).trim();
      const value = /^(['"])(.*)\1$/.test(scalar) ? scalar.slice(1, -1) : scalar;
      if (feature === null) {
        feature = value.length > 0 ? value : null;
      }
    }
  }
  return feature;
}

// The document content with its front matter status set to `status`, replacing
// the one status line or inserting a block when the document has none
// (ticket 0030). Everything outside the status line is preserved, and a newly
// created block carries the same single blank line the server's generated
// templates use. The helper takes the status of the kind vocabulary contract, a
// single-line lowercase kebab-case slug; the value is quoted or escaped before it
// is written, so arbitrary or multi-line input cannot emit malformed YAML
// (ticket 0030, final review round).
export function withStatus(content, status) {
  const bom = String(content ?? "").startsWith("\uFEFF") ? "\uFEFF" : "";
  const text = String(content ?? "").replace(/^\uFEFF/, "");
  const lines = splitLines(text);
  let closing = -1;
  if (lines[0] && lines[0].value === "---" && lines[0].eol !== "") {
    for (let index = 1; index < lines.length; index += 1) {
      if (lines[index].value === "---" && lines[index].eol !== "") {
        closing = index;
        break;
      }
    }
  }
  if (closing === -1) {
    // No front matter block: prepend a valid one before the body, with the same
    // single blank line the server's generated templates use and no extra one.
    const firstBreak = /(?:\r\n|\n)/.exec(text);
    const eol = firstBreak ? firstBreak[0] : "\n";
    const block = [`---`, `status: ${statusYamlValue(status)}`, `---`].join(eol) + eol;
    const separator = eol;
    // The body is preserved verbatim after the separating blank line; stripping
    // the leading whitespace here would drop a body that legitimately begins with
    // indentation or blank lines.
    return `${bom}${block}${separator}${text}`;
  }

  const updated = [...lines];
  let statusIndex = -1;
  let statusEol = updated[closing].eol || "\n";
  // The root indentation of the block: the indentation of its first key line, so
  // only a status key at that level is the top-level status the writer may
  // replace; one nested under another key is that key's subfield and is left
  // alone (ticket 0030, final review round). The first key may start with any
  // character a YAML key may start with, so the discovery agrees with the
  // server's YAML parser for any mapping of the block, exactly like the read path.
  const rootIndent = rootIndentOf(updated, 1, closing);
  // Fall back to the root indentation of the block's other keys when it carried
  // no top-level status, and keep the indentation of the replaced status key, so
  // the rewrite neither drops a nested status key nor reformats a key the person
  // indented by hand.
  let statusIndent = rootIndent ?? "";
  for (let index = 1; index < closing; index += 1) {
    const statusMatch = STATUS_LINE.exec(updated[index].value);
    if (statusMatch && statusMatch[1] === rootIndent) {
      statusIndex = index;
      statusEol = updated[index].eol || "\n";
      statusIndent = statusMatch[1];
      break;
    }
  }
  // Drop every top-level status key of the block and insert a single fresh status
  // line in its place (or before the closing marker when the block carried none),
  // so a block that held duplicates is rewritten with one valid status key again;
  // a duplicate key would be invalid YAML that reads back as no status at all
  // (ticket 0030, sixth review round).
  const block = [];
  for (let index = 0; index < updated.length; index += 1) {
    if (index === 0 || index === closing) {
      block.push(updated[index]);
      continue;
    }
    const statusMatch = STATUS_LINE.exec(updated[index].value);
    const isTopLevelStatus = statusMatch !== null && statusMatch[1] === rootIndent;
    if (index < closing && isTopLevelStatus) {
      // Only front matter status keys at the block's root indentation are the
      // top-level status; a nested one is another key's subfield and must survive,
      // and a `status:` line after the closing delimiter belongs to the body and
      // must survive.
      continue;
    }
    block.push(updated[index]);
  }
  // Keep the indentation of the replaced status key, when there was one, so
  // the rewrite does not reformat a key the person indented by hand.
  block.splice(
    statusIndex === -1 ? closing : statusIndex,
    0,
    { value: `${statusIndent}status: ${statusYamlValue(status)}`, eol: statusEol },
  );
  return `${bom}${block.map((line) => line.value + line.eol).join("")}`;
}

// Restores the captured status select to the captured document status; shared by
// every status-change outcome that gives the select back, so none of the branches
// can drift between them (ticket 0030, final review round).
function restoreStatusSelect(select, status) {
  if (select?.isConnected) {
    select.value = status ?? "";
  }
}

// Applies a status picked in the editor toolbar: it edits the front matter of
// the textarea content (adding a block when absent), marks the document dirty
// and saves through the normal save path (ticket 0030).
export async function changeStatus(status) {
  // Capture the document at entry: the change event already identifies the
  // document through the selected path, so derive the vocabulary from it
  // instead of one captured before a possible concurrent render.
  const changedPath = state.current?.path;
  const options = STATUS_OPTIONS[statusKindOf(changedPath)] ?? null;
  // Capture the select at entry: a document switch while the save is in flight
  // rebuilds the editor and replaces the module state, so the body must not
  // keep reaching for the (possibly stale or detached) global select or write
  // the status of another document.
  const thisStatusSelect = statusSelect;
  const thisDocumentStatus = documentStatus;
  // Capture the save button and the textarea at entry too: a document switch
  // while the save is in flight rebuilds the editor and replaces the module
  // state, so the disable/enable and the content paths must not reach for the
  // (possibly stale or detached) globals.
  const thisSaveButton = saveButton;
  const thisTextarea = textarea;
  if (status === "") {
    // The "No status" option is outside the vocabulary and never saved; give
    // the select back to the document status instead of leaving it stuck there.
    restoreStatusSelect(thisStatusSelect, thisDocumentStatus);
    return;
  }
  if (
    thisStatusSelect === null ||
    !options ||
    !options.includes(status) ||
    statusSaving ||
    saveInFlightOwners.has(changedPath)
  ) {
    // A locked or invalid pick leaves the select sitting on the value the person
    // just chose instead of the persisted status; give it back before bailing.
    restoreStatusSelect(thisStatusSelect, thisDocumentStatus);
    return;
  }
  if (!changedPath || !isSelected(changedPath)) {
    // The person moved to another document while this change was handled; do
    // not write the status of a document that is no longer the selected one, and
    // give the captured select back to the captured document status instead of
    // leaving it stuck on the value the person just picked.
    restoreStatusSelect(thisStatusSelect, thisDocumentStatus);
    return;
  }
  statusSaving = true;
  statusSavingPath = changedPath;
  statusSaveToken += 1;
  const statusSaveId = statusSaveToken;
  if (thisStatusSelect?.isConnected) {
    thisStatusSelect.disabled = true;
  }
  if (thisSaveButton?.isConnected) {
    // A manual save must not race the status save that owns the write; the nodes
    // are guarded because a document switch replaces and detaches them while the
    // promise is outstanding.
    thisSaveButton.disabled = true;
  }
  try {
    const content = withStatus(
      thisTextarea?.isConnected ? thisTextarea.value : (state.current?.content ?? ""),
      status,
    );
    if (thisTextarea?.isConnected) {
      thisTextarea.value = content;
    }
    if (isSelected(changedPath)) {
      // The status change belongs to the document that was selected; the writes
      // below must not land on a document that becomes selected while the save is
      // in flight (ticket 0030, sixth review round).
      state.current.content = content;
      state.dirty = isDirty();
      renderDirtyIndicator();
    }
    if (!isSelected(changedPath)) {
      // The person switched documents while the status change was applied; the
      // status change belonged to the document that was selected, so do not save
      // it on behalf of whatever document is selected now.
      return;
    }
    const saveResult = await save({ allowDuringStatusSave: true });
    if (!isSelected(changedPath)) {
      // The person switched documents while the save was in flight; the status
      // change belonged to the document that was selected, so leave the newly
      // selected document alone.
      return;
    }
    if (saveResult?.skipped) {
      // The save did not persist (for example on a conflict, which resolves
      // without throwing); the disk keeps the old status, so give the select
      // back and leave the local text dirty for the person to retry.
      restoreStatusSelect(thisStatusSelect, thisDocumentStatus);
      return;
    }
    // The save completed through the normal save path, which adopted the saved
    // content and cleared the dirty flag itself; adopt the picked status without
    // re-reading the dirty flag, which a typing edit during the in-flight save
    // would otherwise mistake for a failed save. Write through the select
    // captured at entry, so a document switch that rebuilt the editor while the
    // save was in flight cannot leave the module state pointing at another
    // document's control.
    documentStatus = status;
    if (thisStatusSelect?.isConnected) {
      thisStatusSelect.value = status;
    }
  } catch (error) {
    // The save failed, so the document on disk keeps its status; give the
    // select back and leave the local text dirty for the person to retry.
    restoreStatusSelect(thisStatusSelect, thisDocumentStatus);
    showError(error);
  } finally {
    // Only the operation that is still the newest clears the in-flight state
    // and releases the controls, so an older operation settling after a newer
    // one cannot clobber the newer in-flight state or re-enable the controls
    // the newer operation disabled.
    if (statusSaveToken === statusSaveId) {
      releaseStatusSlot(statusSaveId);
      if (thisStatusSelect?.isConnected) {
        thisStatusSelect.disabled = false;
      }
      if (thisSaveButton?.isConnected) {
        thisSaveButton.disabled = false;
      }
    }
  }
}

// The pending-proposal note of the editor area: an AI proposal replaces the
// whole document, so when the proposal carries an AI rules file change the
// person is told to review the full-file diff before accepting (ticket 0047).
export function proposalPendingNote(proposal) {
  const changes = Array.isArray(proposal?.changes) ? proposal.changes : [];
  const touchesAgents = changes.some((change) => change?.path === "AGENTS.md");
  return touchesAgents
    ? "AI proposals replace the whole file; review the diff."
    : "A proposal is pending your review.";
}

// Replaces the editor area with the save toolbar and the textarea.
export function renderEditor() {
  const element = document.querySelector("#editor");
  if (!element) {
    textarea = null;
    saveButton = null;
    previewButton = null;
    conflictBanner = null;
    statusSelect = null;
    documentStatus = null;
    ticketLinks = null;
    // The in-flight saves track their own entries per document path, so a
    // teardown with no #editor element must not erase an in-flight save that
    // belongs to another document; the save that owns a path clears its own
    // entry in its `finally`.
    return;
  }

  element.replaceChildren();
  previewVisible = false;
  previewCaret = null;
  // The editor is being rebuilt for another document (or for none); a save may
  // still be in flight from the previous one, and its own `finally` clears its
  // own entry once the awaited request settles, so a rebuild must not reset it
  // here.

  // The conflict banner lives inside the editor area so it can sit above the
  // toolbar while the person keeps typing (ticket 0027); it is rebuilt on every
  // render, so it starts hidden again for each freshly loaded document.
  conflictBanner = document.createElement("div");
  conflictBanner.id = "conflict-banner";
  conflictBanner.className = "conflict-banner";
  conflictBanner.setAttribute("role", "alert");
  conflictBanner.setAttribute("aria-live", "assertive");
  conflictBanner.hidden = true;
  conflictBanner.setAttribute("aria-hidden", "true");

  const toolbar = document.createElement("div");
  toolbar.className = "editor-toolbar";
  saveButton = document.createElement("button");
  saveButton.type = "button";
  saveButton.id = "save-document";
  saveButton.textContent = "Save";
  saveButton.addEventListener("click", () => {
    save().catch((error) => showError(error));
  });
  toolbar.appendChild(saveButton);

  previewButton = document.createElement("button");
  previewButton.type = "button";
  previewButton.id = "toggle-preview";
  previewButton.textContent = "Preview";
  previewButton.addEventListener("click", () => {
    togglePreview();
  });
  toolbar.appendChild(previewButton);

  // The status select of the selected document lives in the editor toolbar for
  // every kind whose status vocabulary the kind maps to (features, epics,
  // decisions and tickets, tickets 0030, 0032 and 0052); every other kind gets no
  // control.
  const kind = statusKindOf(state.current?.path);
  let supersedesLine = null;
  if (kind && STATUS_OPTIONS[kind]) {
    const select = document.createElement("select");
    select.id = "document-status";
    select.className = "document-status";
    // The control is a plain select, so the accessible name cannot come from a
    // wrapping label; set it directly (ticket 0030, fourth review round).
    select.setAttribute("aria-label", "Document status");
    const noStatus = document.createElement("option");
    noStatus.value = "";
    noStatus.textContent = "No status";
    select.appendChild(noStatus);
    for (const value of STATUS_OPTIONS[kind]) {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = value;
      select.appendChild(option);
    }
    documentStatus = readStatus(state.current?.content ?? "", STATUS_OPTIONS[kind]);
    select.value = documentStatus ?? "";
    statusSelect = select;
    select.addEventListener("change", () => {
      void changeStatus(select.value);
    });
    toolbar.appendChild(select);
  } else {
    statusSelect = null;
    documentStatus = null;
  }

  if (kind === "decision") {
    // The open decision carries a read-only supersedes line naming the decision
    // it supersedes, or that decision's number marked missing when the target no
    // longer exists (ticket 0032); the pointer is never edited from here, the
    // status is, through the toolbar select above.
    const supersedes = openSupersedes();
    if (supersedes) {
      supersedesLine = document.createElement("p");
      supersedesLine.id = "decision-supersedes";
      supersedesLine.className = "decision-supersedes";
      supersedesLine.textContent = supersedes.missing
        ? `Supersedes: ${String(supersedes.number).padStart(4, "0")} (missing)`
        : `Supersedes: ${String(supersedes.number).padStart(4, "0")}`;
    }
  }

  if (kind === "ticket") {
    // The open ticket carries the read-only epic link derived from its path and
    // the optional feature link of its binding, so the person can open the epic
    // or the feature the ticket belongs to without leaving the editor
    // (ticket 0052); neither is editable here, the epic membership is the path.
    ticketLinks = renderTicketLinks();
  } else {
    ticketLinks = null;
  }

  textarea = document.createElement("textarea");
  textarea.id = "document-content";
  textarea.className = "document-content";
  textarea.addEventListener("input", () => {
    state.current.content = textarea.value;
    state.dirty = isDirty();
    renderDirtyIndicator();
  });

  element.appendChild(conflictBanner);
  if (state.proposal) {
    // A proposal is pending a decision, so the editor area carries the pending
    // indicator even while the person is not reading the diff (ticket 0045); the
    // diff view itself carries the same marker while it replaces the editor.
    const pending = document.createElement("div");
    pending.id = "proposal-pending";
    pending.className = "proposal-pending";
    pending.textContent = proposalPendingNote(state.proposal);
    element.appendChild(pending);
  }
  element.appendChild(toolbar);
  if (supersedesLine) {
    element.appendChild(supersedesLine);
  }
  if (ticketLinks) {
    element.appendChild(ticketLinks);
  }
  element.appendChild(textarea);
}

// The supersedes pointer of the open decision, as a number and a missing flag;
// null when the open document is not a decision or carries no pointer (ticket
// 0032).
export function openSupersedes() {
  const path = state.current?.path;
  if (statusKindOf(path) !== "decision") {
    return null;
  }
  const entries = state.tree?.docs ?? [];
  const entry = entries.find((doc) => doc?.path === path);
  const number = entry?.supersedes ?? null;
  if (number === null || number === undefined) {
    return null;
  }
  const exists = entries.some(
    (doc) => doc?.kind === "decision" && decisionNumberOf(doc.path) === Number(number),
  );
  return { number, missing: !exists };
}

// The read-only epic and optional feature links of an open ticket, derived from
// the ticket path and its front matter binding (ticket 0052): the epic link
// navigates to the epic document, the optional feature link to the feature the
// ticket names, and a feature that no longer resolves stays visible as a missing
// label instead of breaking the epic link. The epic membership is never edited
// here; the path is the only binding.
export function renderTicketLinks() {
  const element = document.createElement("p");
  element.id = "ticket-links";
  element.className = "ticket-links";

  const epicDirectory = epicDirectoryOf(state.current?.path);
  if (epicDirectory) {
    const epicLabel = document.createElement("span");
    epicLabel.textContent = "Epic: ";
    const epicLink = document.createElement("a");
    epicLink.id = "ticket-epic-link";
    epicLink.className = "ticket-link";
    epicLink.textContent = epicDirectory;
    epicLink.href = `sdlc/epics/${epicDirectory}/description.md`;
    epicLink.addEventListener("click", (event) => {
      event.preventDefault();
      void loadDocument(`sdlc/epics/${epicDirectory}/description.md`).catch((error) => {
        console.error("Loading the ticket epic failed:", error);
        showError(error);
      });
    });
    element.append(epicLabel, epicLink);
  }

  const entry = (state.tree?.docs ?? []).find((doc) => doc?.path === state.current?.path);
  const feature = entry?.kind === "ticket" ? entry?.feature ?? null : null;
  if (feature) {
    const featureSlug = String(feature).replace(/\.md$/, "");
    const featurePath = `sdlc/features/${featureSlug}.md`;
    const featureLabel = document.createElement("span");
    featureLabel.textContent = "Feature: ";
    if (featureResolves(featureSlug)) {
      const featureLink = document.createElement("a");
      featureLink.id = "ticket-feature-link";
      featureLink.className = "ticket-link";
      featureLink.textContent = featureSlug;
      featureLink.href = featurePath;
      featureLink.addEventListener("click", (event) => {
        event.preventDefault();
        void loadDocument(featurePath).catch((error) => {
          console.error("Loading the ticket feature failed:", error);
          showError(error);
        });
      });
      element.append(featureLabel, featureLink);
      return element;
    }
    const missing = document.createElement("span");
    missing.id = "ticket-feature-link";
    missing.className = "ticket-link";
    missing.textContent = `Feature: ${featureSlug} (unknown)`;
    // The missing label carries the full text, so the label is not appended
    // again and the prefix is not rendered twice (ticket 0052).
    element.append(missing);
    return element;
  }
  return element;
}

// Shows the non-modal save conflict banner: the disk changed since the person
// opened the document, so the local text is kept and the choice is theirs
// (ticket 0027). Never auto-merges and never retries.
function showConflictBanner() {
  if (!conflictBanner) {
    return;
  }
  conflictBanner.replaceChildren();

  const message = document.createElement("p");
  message.className = "conflict-banner-message";
  message.textContent = "This file changed on disk since you opened it.";

  let conflictActionInFlight = false;

  const reloadButton = document.createElement("button");
  reloadButton.type = "button";
  reloadButton.id = "conflict-reload";
  reloadButton.textContent = "Reload from disk";
  reloadButton.addEventListener("click", () => {
    if (conflictActionInFlight) {
      return;
    }
    conflictActionInFlight = true;
    setConflictButtonsDisabled(true);
    void reloadFromDisk().finally(() => {
      conflictActionInFlight = false;
      setConflictButtonsDisabled(false);
    });
  });

  const overwriteButton = document.createElement("button");
  overwriteButton.type = "button";
  overwriteButton.id = "conflict-overwrite";
  overwriteButton.textContent = "Overwrite with mine";
  // Explicit and destructive; deliberately not the default focus.
  overwriteButton.addEventListener("click", () => {
    if (conflictActionInFlight) {
      return;
    }
    conflictActionInFlight = true;
    setConflictButtonsDisabled(true);
    void overwriteWithMine().finally(() => {
      conflictActionInFlight = false;
      setConflictButtonsDisabled(false);
    });
  });

  const setConflictButtonsDisabled = (disabled) => {
    reloadButton.disabled = disabled;
    overwriteButton.disabled = disabled;
  };

  conflictBanner.append(message, reloadButton, overwriteButton);
  conflictBanner.hidden = false;
  conflictBanner.setAttribute("aria-hidden", "false");
}

// Hides the save conflict banner after a choice was applied or the conflict was
// resolved another way.
function hideConflictBanner(bannerToHide = conflictBanner) {
  if (!bannerToHide) {
    return;
  }
  bannerToHide.hidden = true;
  bannerToHide.setAttribute("aria-hidden", "true");
}

// Whether the document a save or conflict action ran against is still the
// selected one.
function isSelected(path) {
  return state.current?.path === path;
}

// Renders the markdown of the selected document into the preview element: the
// marked parser turns it into HTML, DOMPurify sanitizes it, and the sanitized
// markup is what gets inserted. The sanitized string is the only thing ever
// inserted, so raw HTML in the document is escaped or stripped instead of being
// executed; `marked` does not sanitize, so the order matters.
function renderPreview() {
  const previewElement = document.querySelector("#preview");
  if (!previewElement) {
    return;
  }
  if (!librariesReady()) {
    previewElement.textContent = "Preview could not be rendered.";
    return;
  }
  const liveTextarea = document.querySelector("#document-content");
  const markdown = liveTextarea?.isConnected
    ? liveTextarea.value
    : (textarea?.isConnected ? textarea.value : (state.current.content ?? ""));
  let html;
  try {
    html = DOMPurify.sanitize(marked.parse(String(markdown ?? "")));
  } catch (error) {
    previewElement.textContent = "Preview could not be rendered.";
    return;
  }
  previewElement.innerHTML = html;
  // DOMPurify drops `target` in some configurations; force the new-tab behaviour
  // after sanitizing, not before.
  previewElement.querySelectorAll("a").forEach((link) => {
    link.setAttribute("target", "_blank");
    link.setAttribute("rel", "noopener noreferrer");
  });
}

function updatePreviewButton() {
  if (!previewButton) {
    return;
  }
  previewButton.textContent = previewVisible ? "Close preview" : "Preview";
  previewButton.setAttribute("aria-pressed", previewVisible ? "true" : "false");
}

// Switches the editor area between the source textarea and the rendered
// preview (ticket 0024). Renders on toggle; switching back to source restores
// the textarea and the caret and leaves the dirty state untouched.
export function togglePreview() {
  const element = document.querySelector("#editor");
  if (!element) {
    return;
  }

  if (!librariesReady()) {
    showError(new Error("Preview libraries failed to load."));
    return;
  }

  if (previewVisible) {
    previewVisible = false;
    const previewElement = document.querySelector("#preview");
    if (previewElement) {
      previewElement.hidden = true;
    }
    if (textarea) {
      textarea.hidden = false;
      const start = previewCaret?.start ?? textarea.selectionStart;
      const end = previewCaret?.end ?? textarea.selectionEnd;
      textarea.focus();
      textarea.setSelectionRange(start, end);
    }
    updatePreviewButton();
    return;
  }

  previewVisible = true;
  if (textarea) {
    previewCaret = { start: textarea.selectionStart, end: textarea.selectionEnd };
    textarea.hidden = true;
  }
  let previewElement = document.querySelector("#preview");
  if (!previewElement) {
    previewElement = document.createElement("div");
    previewElement.id = "preview";
    element.appendChild(previewElement);
  }
  previewElement.hidden = false;
  renderPreview();
  updatePreviewButton();
}

// Reloads the selected document from disk, discarding the local text after the
// person confirmed. Refetches through the API instead of trusting
// `state.current.content`, and keeps the selected document on success (ticket
// 0027).
async function reloadFromDisk() {
  const path = state.current?.path;
  if (!path) {
    return;
  }
  hideConflictBanner();
  try {
    await loadDocument(path);
  } catch (error) {
    if (error?.message === "load cancelled") {
      // The person declined the discard; keep the conflict choice available.
      showConflictBanner();
      return;
    }
    // The file may have been deleted externally while the conflict was shown; the
    // local text stays in the editor either way.
    showError(error);
  }
}

// Overwrites the stored document with the local textarea content, after fetching
// a fresh hash from the server. The person chose this explicitly; if the write
// conflicts again the banner is shown again and the flow stops instead of
// looping (ticket 0027).
async function overwriteWithMine() {
  const path = state.current?.path;
  if (!path) {
    return;
  }
  // Capture the banner at entry: a document switch while the overwrite is in
  // flight rebuilds the editor and replaces the global banner, so the `finally`
  // must hide the banner of the overwritten document and not the one that the
  // newly selected document renders afterwards.
  const banner = conflictBanner;
  if (!isSelected(path)) {
    renderDirtyIndicator();
    return;
  }
  const localText = textarea?.isConnected ? textarea.value : (state.current?.content ?? "");
  const savedContent = state.current?.savedContent ?? "";
  // Whether the overwrite just reported a conflict; a conflict re-shows the
  // banner in the `catch` below, so the `finally` must not hide it again.
  let conflict = false;
  // The owner token of this overwrite's in-flight slot; the slot is only cleared
  // while this overwrite still owns it, so a stale overwrite settling cannot clear
  // the ownership of a newer overwrite of the same document (ticket 0030, final
  // review round).
  let overwriteToken = null;
  // The overwrite is a write of the selected document, so it takes the same
  // in-flight slot the ordinary save path owns; only the operation that owns a
  // path clears that path's entry (ticket 0030, final review round). The conflict
  // banner is hidden only once the overwrite may proceed, so an overwrite that
  // bails on an in-flight save keeps the conflict choice available (ticket 0030,
  // final closing review round).
  if (saveInFlightOwners.has(path) || (statusSaving && statusSavingPath === path)) {
    // A save or status save of this document is already in flight, so the
    // overwrite must not overlap it on the document contents or the precondition
    // hash; the in-flight operation owns the write from here on, and the conflict
    // banner stays visible for the person to choose again once it settles.
    return;
  }
  hideConflictBanner(banner);
  try {
    if (!isSelected(path)) {
      renderDirtyIndicator();
      return;
    }
    // The overwrite is a write of the selected document, so it takes the same
    // in-flight slot the ordinary save path owns; only the operation that owns a
    // path clears that path's entry (ticket 0030, final review round).
    overwriteToken = ++saveOwnerToken;
    saveInFlightOwners.set(path, overwriteToken);
    const freshDocument = await getDoc(path);
    if (!isSelected(path)) {
      renderDirtyIndicator();
      return;
    }
    const content = toSavedContent(localText, savedContent);
    const response = await saveDoc(path, content, freshDocument.hash);
    // The overwrite itself succeeded, so refresh the status outcome before any
    // document-switch guard: the helper starts the tree refresh and then re-checks
    // whether the saved document is still the selected one, so a save that lands
    // after the person switched documents still refreshes the written document.
    refreshStatusAfterSave(path, content, response);
    if (!isSelected(path)) {
      renderDirtyIndicator();
      return;
    }
    state.current.hash = response.hash;
    state.current.savedContent = content;
    state.current.content = localText;
    state.dirty = isDirty();
    renderDirtyIndicator();
    if (!isSelected(path)) {
      // The person switched documents while the overwrite was applied; the
      // overwrite belonged to the document that was selected, so leave the newly
      // selected document alone instead of warning, refreshing the tree or
      // syncing the status of the document that is no longer selected.
      return;
    }
  } catch (error) {
    if (error instanceof ApiError && error.code === "conflict") {
      // The disk changed again while overwriting; report the conflict and stop.
      conflict = true;
      if (state.current?.path === path) {
        showConflictBanner();
      }
      return;
    }
    showError(error);
  } finally {
    // Release the in-flight slot this overwrite owns, so a later save or status
    // save of the same document may start once it settles; only the operation that
    // still owns the slot clears it, so a stale overwrite settling cannot clear the
    // ownership of a newer overwrite of the same document.
    if (overwriteToken !== null && saveInFlightOwners.get(path) === overwriteToken) {
      saveInFlightOwners.delete(path);
    }
    // Hide the conflict banner once the overwrite settled, whichever way the
    // flow stopped; a conflict re-shows it in the `catch` above and keeps it
    // visible through the flag, and an overwrite that completes after the person
    // switched documents settles here instead of returning before the refresh.
    if (!conflict) {
      hideConflictBanner(banner);
    }
  }
}

// Whether a feature slug the ticket names resolves to an existing feature
// document, so an edited ticket feature can be validated on save (ticket 0052).
function featureResolves(feature) {
  const slug = String(feature ?? "").replace(/\.md$/, "");
  const featurePath = `sdlc/features/${slug}.md`;
  return (state.tree?.docs ?? []).some(
    (doc) => doc?.kind === "feature" && doc?.path === featurePath,
  );
}

// The content of the editor buffer right now: the live textarea value when the
// textarea is connected, the stored document content otherwise, so a diff view
// that detached the textarea still reads the text the person is editing.
export function editorText() {
  return textarea?.isConnected ? textarea.value : (state.current?.content ?? "");
}

// Restores the editor area after a proposal decision: re-renders the editor
// chrome and puts the buffer back, so the unsaved text of the person is kept
// while a proposal diff was on screen and restored after the decision
// (ticket 0045).
export function restoreEditor() {
  const text = editorText();
  renderEditor();
  if (!textarea) {
    throw new Error("The editor is unavailable.");
  }
  textarea.value = text;
  textarea.focus();
  renderDirtyIndicator();
}

// Loads a document into the editor, asking before discarding unsaved changes.
export async function loadDocument(path) {
  if (state.dirty) {
    const keep = window.confirm("Discard unsaved changes?");
    if (!keep) {
      // Reject so the caller rolls the selection back instead of loading over
      // the edits the person did not confirm.
      return Promise.reject(new Error("load cancelled"));
    }
  }

  const freshDocument = await getDoc(path);
  state.current.path = path;
  state.current.content = normalizeNewlines(freshDocument.content);
  state.current.savedContent = freshDocument.content;
  state.current.hash = freshDocument.hash;
  state.dirty = false;

  renderEditor();
  if (!textarea) {
    throw new Error("The editor is unavailable.");
  }
  textarea.value = freshDocument.content;
  textarea.focus();
  renderDirtyIndicator();
}

// After a successful save: surface the feature-index refresh outcome as
// warnings and, for a document whose save may have changed a status (feature,
// epic, decision or ticket), refresh the document tree and re-read the saved
// status to sync the status select (tickets 0030, 0032 and 0052). The person may
// have switched
// documents while the save was in flight, and a status save may itself be applying
// the result, so the helper bails before writing state that no longer belongs to
// the selected document; it returns whether the saved document is still the
// selected one. The helper is shared by the ordinary save path and the overwrite-
// with-mine conflict resolution so the two cannot drift.
function refreshStatusAfterSave(path, savedContent, response) {
  const savedKind = statusKindOf(path);
  const hasStatus = Boolean(savedKind && STATUS_OPTIONS[savedKind]);
  if (hasStatus) {
    // The saved document is a feature, epic, decision or ticket, so its sidebar
    // badge may have changed and the PRD feature index was refreshed by the server
    // during the save (ticket 0029); refresh the tree for those saves alone. The
    // refresh is
    // best effort and must not turn a successful save into a reported failure, so
    // a failure surfaces as a warning while the save itself stays successful.
    refreshTree().catch((error) => {
      showWarning(`The document changed on disk during the refresh: ${error?.message ?? error}`);
    });
  }
  if (!isSelected(path)) {
    // The person switched documents while the save was in flight; the saved
    // content belongs to the document that was selected, so re-check before
    // surfacing the outcome or writing its status, whatever the saved document's
    // kind; the tree refresh of the saved document still stands.
    return false;
  }
  // The server refreshes the PRD feature index after a feature save and
  // reports the outcome as warnings; the save itself succeeded, so surface the
  // refresh outcome here instead of dropping it in one save path only. The
  // outcome is surfaced only once the saved document is still the selected one,
  // the way the save path does, so a document switch cannot surface a warning of
  // a document that is no longer selected.
  showWarnings(response?.warnings);
  if (!hasStatus) {
    // Only features and epics carry a status the sidebar badge or the select
    // can reflect; every other kind completes without a status refresh.
    return true;
  }
  // The save can come from a status change or from a manual edit of the status
  // line; either way the saved content carries the new truth, so refresh the
  // read status and sync the select unless the status change itself is applying
  // the result (ticket 0030).
  documentStatus = readStatus(savedContent, STATUS_OPTIONS[savedKind]);
  if (!(statusSaving && statusSavingPath === path) && statusSelect?.isConnected) {
    statusSelect.value = documentStatus ?? "";
  }
  return true;
}

// Saves the textarea content of the selected document. No-op when the document
// is clean, when a save of the same document is already in flight, or when a
// status save is in flight and this call is not the status save itself
// (`allowDuringStatusSave`); the stored hash is only trusted from the server
// response, never from an optimistic update. A skipped save resolves with
// `{ skipped: true }` so the ordinary callers (toolbar button, Ctrl/Cmd+S
// shortcut) can tell a no-op save from a completed one instead of silently
// dropping it; a completed save resolves with `{ skipped: false }` so callers can
// tell the two apart. In-flight saves are tracked per document path, so only a
// save of a document that already has one in flight is skipped.
export async function save({ allowDuringStatusSave = false } = {}) {
  const path = state.current?.path;
  if (
    (!allowDuringStatusSave && statusSaving && statusSavingPath === path) ||
    // The in-flight saves are tracked per document path, so a save of another
    // document may run while a save of a different document is in flight instead
    // of being skipped behind that save's ownership; a save is skipped only while
    // its own document still has one in flight.
    saveInFlightOwners.has(path) ||
    !path ||
    !isDirty()
  ) {
    return { skipped: true };
  }

  const saveToken = ++saveOwnerToken;
  saveInFlightOwners.set(path, saveToken);
  const thisSaveButton = saveButton;
  const thisTextarea = textarea;
  if (thisSaveButton && !allowDuringStatusSave) {
    // When this save drives a status change (`allowDuringStatusSave`) the
    // surrounding `changeStatus` operation owns the save-button lifecycle, so
    // the save itself must not toggle the button.
    thisSaveButton.disabled = true;
  }
  // Capture the content, the saved content and the precondition hash of the
  // document at entry: a document switch while the save is in flight replaces
  // `state.current`, so reading them once the request is issued could adopt
  // another document's text, saved content or hash and turn a successful write
  // into a conflict or save the wrong content (ticket 0030, final review round).
  const content = state.current?.content ?? "";
  const savedContent = state.current?.savedContent ?? "";
  const hash = state.current?.hash;
  try {
    if (!isSelected(path)) {
      // The person switched documents before the save was started; do not issue
      // a request on behalf of the now-deselected document.
      renderDirtyIndicator();
      return { skipped: true };
    }
    const text = thisTextarea?.isConnected ? thisTextarea.value : content;
    const contentToSave = toSavedContent(text, savedContent);
    const response = await saveDoc(path, contentToSave, hash);
    // Route the saved response through the shared outcome helper before the
    // document-switch guard, so both save paths share one outcome handling (ticket
    // 0030, final review round): a save that resolved just as the person switched
    // documents still refreshes the written document's tree and applies the same
    // consistency rules instead of dropping both.
    if (!refreshStatusAfterSave(path, contentToSave, response)) {
      // The person switched documents while the save was in flight; the response
      // belongs to the old path and must not be applied to the new selection, so
      // bail out before capturing or writing any of the saved state.
      renderDirtyIndicator();
      return { skipped: true };
    }
    if (statusKindOf(path) === "ticket") {
      // A ticket's `feature` line may have been edited by hand, which the save
      // keeps; an unresolvable feature is surfaced as a warning on save instead of
      // blocking the edit (ticket 0052). The warning rides the shared outcome
      // handling, so it is only surfaced once the save resolved for a document
      // that is still the selected one.
      const feature = ticketFeatureOf(contentToSave);
      if (feature && !featureResolves(feature)) {
        showWarning(
          `The ticket feature "${feature}" does not resolve; create or rename the feature first.`,
        );
      }
    }
    // Capture the state that belongs to the saved document before applying it,
    // so a response that resolves just as the person switches documents cannot
    // land on the newly selected document instead (ticket 0030, sixth review).
    const savedHash = response.hash;
    const savedText = thisTextarea?.isConnected ? thisTextarea.value : content;
    state.current.hash = savedHash;
    state.current.savedContent = contentToSave;
    state.current.content = savedText;
    state.dirty = isDirty();
    renderDirtyIndicator();
    return { skipped: false };
  } catch (error) {
    // Keep the text and the dirty state; the conflict banner offers the choices,
    // every other failure falls through to the error banner. Only mark the
    // current document dirty when this save still belonged to it.
    if (state.current?.path === path) {
      state.dirty = true;
      renderDirtyIndicator();
    }
    if (error instanceof ApiError && error.code === "conflict") {
      // The disk moved under the local text; keep it and let the person choose.
      if (state.current?.path === path) {
        showConflictBanner();
      }
      return { skipped: true };
    }
    throw error;
  } finally {
    // Only the save that owns the path clears its own entry, so a save that
    // settles cannot clear the ownership of a newer save of the same document
    // that started before the older one settled, and saves of other documents
    // keep their own entries.
    if (saveInFlightOwners.get(path) === saveToken) {
      saveInFlightOwners.delete(path);
    }
    if (!allowDuringStatusSave && thisSaveButton) {
      // When this save drives a status change (`allowDuringStatusSave`) the
      // surrounding `changeStatus` operation owns the save-button lifecycle and
      // re-enables the button when its own turn settles; releasing it here would
      // be earlier than that.
      thisSaveButton.disabled = false;
    }
  }
}

// Clears the editor after the selected document was deleted, so no stale content,
// hash or dirty state survives a mutation (ticket 0025).
export function clearEditor() {
  state.current.path = null;
  state.current.content = "";
  state.current.savedContent = "";
  state.current.hash = null;
  state.dirty = false;
  // The editor state is being torn down, so no save may stay blocked behind it;
  // the save that owns the write still clears its own in-flight entry in its
  // `finally`, and only the save or status save that belongs to the deleted
  // document has to be released by its own settling; leave the in-flight ownership
  // of a deleted document in place until the pending operation settles, so a new
  // operation cannot start against that path before the stale one releases it
  // through its token guard (ticket 0030, final closing review round).
  renderEditor();
  renderDirtyIndicator();
}

// Warns before the tab closes with unsaved changes.
window.addEventListener("beforeunload", (event) => {
  if (!state.dirty) {
    return;
  }
  event.preventDefault();
  event.returnValue = "";
});
