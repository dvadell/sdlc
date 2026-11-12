//! The small native-dialog helper of the frontend (ticket 0032).

import { STATUS_OPTIONS, decisionNumberOf, epicSlugOf, featureSlugOf } from "./statuses.js";

// The pending supersedes value of the decision-create dialog: the raw string
// and whether it names a finite decision number (ticket 0032).
function pendingSupersedes(value) {
  const raw = value === null || value === undefined ? "" : String(value);
  const number = raw.length > 0 ? Number(raw) : null;
  const isPositiveInteger = number !== null && Number.isInteger(number) && number > 0;
  return {
    raw: isPositiveInteger ? String(number) : "",
    number: isPositiveInteger ? number : null,
    hasNumber: isPositiveInteger,
  };
}

// The optional supersedes choices of the decision-create dialog: the existing
// decisions in numeric order with their number and title, the "no supersedes"
// choice, and, when the pending value names a decision that no longer exists, a
// `missing` choice that keeps the value visible instead of dropping it (ticket
// 0032).
export function supersedesOptions(decisions, value) {
  const options = [{ value: "", label: "None" }];
  const { raw: wanted, number: wantedNumber, hasNumber } = pendingSupersedes(value);
  const existing = new Map();
  const entries = [...(decisions ?? [])].sort(
    (a, b) => (decisionNumberOf(a?.path) ?? 0) - (decisionNumberOf(b?.path) ?? 0),
  );
  for (const entry of entries) {
    const number = decisionNumberOf(entry?.path);
    if (number === null || existing.has(number)) {
      continue;
    }
    existing.set(number, entry);
    options.push({
      value: String(number),
      label: `${String(number).padStart(4, "0")} ${decisionTitle(entry)}`,
    });
  }
  if (hasNumber && !existing.has(wantedNumber)) {
    // The target of the pending pointer was deleted, so it is not part of the
    // list of existing decisions; it is still offered, marked missing, so the
    // person never loses the pointer they picked (ticket 0032).
    options.push({
      value: wanted,
      label: `${String(wantedNumber).padStart(4, "0")} (missing)`,
    });
  }
  return options;
}

// The display title of a decision entry, without its four-digit number prefix.
function decisionTitle(entry) {
  const raw = String(entry?.title ?? "");
  const withoutNumber = raw.replace(/^\d{4}:\s*/, "");
  return withoutNumber.length > 0 ? withoutNumber : raw;
}

// Opens the small decision-create dialog: title, status and an optional
// supersedes target, then resolves with the outcome the person picked or with
// null when they dismissed it. The dialog is a native `<dialog>`, so the browser
// owns the focus trap and the Escape handling; the Create button closes it with
// the outcome and the close event settles the promise, and a cancelled or closed
// without an outcome dialog settles null.
export function openDecisionCreateDialog(decisions, value) {
  return new Promise((resolve) => {
    let settled = false;
    let outcome = null;
    const settle = (result) => {
      if (settled) {
        return;
      }
      settled = true;
      resolve(result);
    };

    const dialog = document.createElement("dialog");
    dialog.className = "decision-dialog";
    dialog.setAttribute("aria-label", "New decision");
    dialog.setAttribute("aria-modal", "true");

    const titleInput = document.createElement("input");
    titleInput.type = "text";
    titleInput.id = "decision-dialog-title";
    titleInput.className = "decision-dialog-title";
    titleInput.setAttribute("aria-label", "Decision title");

    const statusSelect = document.createElement("select");
    statusSelect.id = "decision-dialog-status";
    statusSelect.className = "decision-dialog-status";
    statusSelect.setAttribute("aria-label", "Decision status");
    for (const status of STATUS_OPTIONS.decision) {
      const option = document.createElement("option");
      option.value = status;
      option.textContent = status;
      statusSelect.appendChild(option);
    }
    statusSelect.value = STATUS_OPTIONS.decision[0];

    const supersedesSelect = document.createElement("select");
    supersedesSelect.id = "decision-dialog-supersedes";
    supersedesSelect.className = "decision-dialog-supersedes";
    supersedesSelect.setAttribute("aria-label", "Supersedes decision");
    for (const option of supersedesOptions(decisions, value)) {
      const choice = document.createElement("option");
      choice.value = option.value;
      choice.textContent = option.label;
      supersedesSelect.appendChild(choice);
    }
    const { raw: wanted, hasNumber } = pendingSupersedes(value);
    supersedesSelect.value = wanted === "" || !hasNumber ? "" : wanted;

    const actions = document.createElement("div");
    actions.className = "decision-dialog-actions";
    const cancelButton = document.createElement("button");
    cancelButton.type = "button";
    cancelButton.id = "decision-dialog-cancel";
    cancelButton.className = "decision-dialog-cancel";
    cancelButton.textContent = "Cancel";
    cancelButton.addEventListener("click", () => {
      dialog.close();
    });
    actions.appendChild(cancelButton);
    const createButton = document.createElement("button");
    createButton.type = "button";
    createButton.id = "decision-dialog-create";
    createButton.className = "decision-dialog-create";
    createButton.textContent = "Create";
    createButton.addEventListener("click", () => {
      const pickedTitle = String(titleInput.value ?? "").trim();
      if (pickedTitle.length === 0) {
        // An empty title never names a decision, so dismiss instead of resolving
        // a create outcome the caller would have to re-validate.
        dialog.close();
        return;
      }
      const rawSupersedes = supersedesSelect.value;
      const pickedSupersedes =
        rawSupersedes === "" || !/^\d+$/.test(rawSupersedes) ? undefined : Number(rawSupersedes);
      outcome = {
        title: pickedTitle,
        status: statusSelect.value,
        supersedes: pickedSupersedes,
      };
      dialog.close();
    });
    actions.appendChild(createButton);

    dialog.appendChild(titleInput);
    dialog.appendChild(statusSelect);
    dialog.appendChild(supersedesSelect);
    dialog.appendChild(actions);

    dialog.addEventListener("close", () => {
      // The Create button closes with the outcome it just set; a cancel or a
      // close without an outcome settles the dismissed value.
      settle(outcome);
      dialog.remove();
    });
    // The fallback Escape handling of the native dialog: a cancelled dialog is
    // closed by hand so the close event settles the dismissed outcome; the
    // default action of cancel is to close, so only the close is needed.
    dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      dialog.close();
    });

    try {
      document.body.appendChild(dialog);
      dialog.showModal();
    } catch {
      dialog.remove();
      // The dialog could not be attached or shown; settle the dismissed
      // outcome so the awaiting create flow cannot hang forever (ticket 0032,
      // final closing review round).
      settle(null);
      return;
    }
  });
}

// The ticket-create outcome: the title the person picked, the epic directory the
// ticket is written under, the optional feature slug and the initial status.
// The epic is required and the feature optional; the status of a new ticket is
// `todo` (ticket 0052).
export function ticketCreateOutcome(outcome) {
  const title = String(outcome?.title ?? "").trim();
  const epic = String(outcome?.epic ?? "").trim();
  if (title.length === 0 || epic.length === 0) {
    // The dialog only resolves a create outcome for a title and an epic; the
    // person has to pick both before a ticket exists.
    return null;
  }
  const feature = outcome?.feature ? String(outcome.feature) : undefined;
  const status = STATUS_OPTIONS.ticket.includes(String(outcome?.status ?? "")) ? String(outcome.status) : STATUS_OPTIONS.ticket[0];
  return { title, epic, feature, status };
}

// Opens the ticket-create dialog: title, epic select (existing epics, required)
// and optional feature select, then resolves with the outcome the person picked
// or with null when they dismissed it. The dialog is a native `<dialog>`, so the
// browser owns the focus trap and the Escape handling; a cancelled or closed
// without a valid outcome dialog settles null, and a project without any epic
// cannot submit (ticket 0052).
export function openTicketCreateDialog(epics, features) {
  return new Promise((resolve) => {
    let settled = false;
    let outcome = null;
    const settle = (result) => {
      if (settled) {
        return;
      }
      settled = true;
      resolve(result);
    };

    const dialog = document.createElement("dialog");
    dialog.className = "ticket-dialog";
    dialog.setAttribute("aria-label", "New ticket");
    dialog.setAttribute("aria-modal", "true");

    const titleInput = document.createElement("input");
    titleInput.type = "text";
    titleInput.id = "ticket-dialog-title";
    titleInput.className = "ticket-dialog-title";
    titleInput.setAttribute("aria-label", "Ticket title");

    const epicSelect = document.createElement("select");
    epicSelect.id = "ticket-dialog-epic";
    epicSelect.className = "ticket-dialog-epic";
    epicSelect.setAttribute("aria-label", "Epic");
    const epicPlaceholder = document.createElement("option");
    epicPlaceholder.value = "";
    epicPlaceholder.textContent = "Select an epic";
    epicPlaceholder.selected = true;
    epicSelect.appendChild(epicPlaceholder);
    const epicChoices = (epics ?? [])
      .filter((epic) => epic?.kind === "epic")
      .map((epic) => epicSlugOf(epic?.path))
      .filter((epic) => epic);
    for (const epic of epicChoices) {
      const option = document.createElement("option");
      option.value = epic;
      option.textContent = epic;
      epicSelect.appendChild(option);
    }

    const featureSelect = document.createElement("select");
    featureSelect.id = "ticket-dialog-feature";
    featureSelect.className = "ticket-dialog-feature";
    featureSelect.setAttribute("aria-label", "Feature");
    const noFeature = document.createElement("option");
    noFeature.value = "";
    noFeature.textContent = "None";
    featureSelect.appendChild(noFeature);
    const featureChoices = (features ?? [])
      .filter((feature) => feature?.kind === "feature")
      .map((feature) => featureSlugOf(feature?.path))
      .filter((feature) => feature);
    for (const feature of featureChoices) {
      const option = document.createElement("option");
      option.value = feature;
      option.textContent = feature;
      featureSelect.appendChild(option);
    }

    const statusSelect = document.createElement("select");
    statusSelect.id = "ticket-dialog-status";
    statusSelect.className = "ticket-dialog-status";
    statusSelect.setAttribute("aria-label", "Ticket status");
    for (const status of STATUS_OPTIONS.ticket) {
      const option = document.createElement("option");
      option.value = status;
      option.textContent = status;
      statusSelect.appendChild(option);
    }
    statusSelect.value = STATUS_OPTIONS.ticket[0];

    const actions = document.createElement("div");
    actions.className = "ticket-dialog-actions";
    const cancelButton = document.createElement("button");
    cancelButton.type = "button";
    cancelButton.id = "ticket-dialog-cancel";
    cancelButton.className = "ticket-dialog-cancel";
    cancelButton.textContent = "Cancel";
    cancelButton.addEventListener("click", () => {
      dialog.close();
    });
    actions.appendChild(cancelButton);
    const createButton = document.createElement("button");
    createButton.type = "button";
    createButton.id = "ticket-dialog-create";
    createButton.className = "ticket-dialog-create";
    createButton.textContent = "Create";
    createButton.disabled = epicChoices.length === 0;
    createButton.addEventListener("click", () => {
      const pickedTitle = String(titleInput.value ?? "").trim();
      if (pickedTitle.length === 0) {
        // An empty title never names a ticket, so dismiss instead of resolving
        // a create outcome the caller would have to re-validate.
        dialog.close();
        return;
      }
      const pickedEpic = String(epicSelect.value ?? "").trim();
      if (pickedEpic.length === 0) {
        // A ticket needs an epic; without one the submit is rejected and the
        // dialog dismisses without an outcome (ticket 0052).
        dialog.close();
        return;
      }
      outcome = {
        title: pickedTitle,
        epic: pickedEpic,
        feature: String(featureSelect.value ?? "").trim() || undefined,
        status: String(statusSelect.value ?? ""),
      };
      dialog.close();
    });
    actions.appendChild(createButton);

    const explanation = document.createElement("p");
    explanation.className = "ticket-dialog-explanation";
    explanation.textContent = epicChoices.length === 0
      ? "No epic exists yet, so a ticket cannot be created. Create an epic first, then open this dialog again."
      : "";

    dialog.appendChild(titleInput);
    dialog.appendChild(epicSelect);
    dialog.appendChild(featureSelect);
    dialog.appendChild(statusSelect);
    dialog.appendChild(explanation);
    dialog.appendChild(actions);

    dialog.addEventListener("close", () => {
      // The Create button closes with the outcome it just set; a cancel or a
      // close without an outcome settles the dismissed value.
      settle(ticketCreateOutcome(outcome));
      dialog.remove();
    });
    // The fallback Escape handling of the native dialog: a cancelled dialog is
    // closed by hand so the close event settles the dismissed outcome; the
    // default action of cancel is to close, so only the close is needed.
    dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      dialog.close();
    });

    try {
      document.body.appendChild(dialog);
      dialog.showModal();
    } catch {
      dialog.remove();
      // The dialog could not be attached or shown; settle the dismissed
      // outcome so the awaiting create flow cannot hang forever (ticket 0032,
      // final closing review round).
      settle(null);
      return;
    }
  });
}
