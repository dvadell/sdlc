//! The status vocabularies of the document kinds that carry a status (tickets
//! 0030, 0032 and 0052).

// A feature offers planned/in-progress/shipped; an epic planned/in-progress/done;
// a decision the ADR statuses proposed/accepted/superseded (ticket 0031); a ticket
// todo/in-progress/done (ticket 0052). Every other kind has no status control at
// all. The decision vocabulary is decision-only, so a status sent for another kind
// is a misplaced field the server refuses instead of a status of that kind.
export const STATUS_OPTIONS = {
  feature: ["planned", "in-progress", "shipped"],
  epic: ["planned", "in-progress", "done"],
  decision: ["proposed", "accepted", "superseded"],
  ticket: ["todo", "in-progress", "done"],
};

// Whether `value` is a lowercase kebab-case slug: alphanumeric words joined by
// single hyphens, with no empty word, exactly like the server's `is_kebab`.
function isKebabSlug(value) {
  if (typeof value !== "string") {
    return false;
  }
  const words = value.split("-");
  if (words.some((word) => word.length === 0)) {
    return false;
  }
  return words.every((word) => /^[a-z0-9]+$/.test(word));
}

// Whether `value` is a four-digit-prefixed numbered document name such as
// `0007-add-export`, exactly like the server's `is_numbered_slug`.
function isNumberedSlug(value) {
  if (typeof value !== "string") {
    return false;
  }
  const name = value;
  const dash = name.indexOf("-");
  if (dash <= 0) {
    return false;
  }
  const prefix = name.slice(0, dash);
  const slug = name.slice(dash + 1);
  return prefix.length === 4 && /^[0-9]{4}$/.test(prefix) && isKebabSlug(slug);
}

// The status vocabulary kind of a document path, or null for kinds that carry
// no status control (ticket 0030). Features and epics carry a status, decisions
// carry the ADR status of ticket 0031, and tickets carry the todo/in-progress/
// done status of ticket 0052; every other kind has no status control at all. A
// feature is a single file directly under `sdlc/features` named after a lowercase
// kebab-case slug, and an epic the `description.md` of its own four-digit-prefixed
// epic directory, so deeper paths never match; a ticket is the
// `tickets/<number>-<slug>.md` file of one such epic directory. The path is
// normalized the way the server normalizes a captured path and validates it
// (`parse_captured_path` drops a leading slash, separators are `/` only and the
// layout is lowercase kebab-case), so an equivalent path still resolves to the
// same kind; a path that carries a backslash or a dot segment the server rejects
// never resolves to a status kind here either, so a client-constructed path the
// server would refuse is never treated as one the server would accept.
function pathSegments(path) {
  return String(path ?? "").replace(/^\//, "").split("/");
}

export function statusKindOf(path) {
  const segments = pathSegments(path);
  const invalidSegment = (segment) =>
    segment === ".." ||
    segment === "." ||
    segment === "" ||
    segment.includes("\0") ||
    segment.includes("\\");
  if (segments.some((segment) => invalidSegment(segment))) {
    // A path that the server's document path rules reject never carries a status
    // control, so judge it strictly and return no kind for it instead of being
    // lenient about backslash or dot-segmented paths the server would refuse
    // (ticket 0030, final review round).
    return null;
  }
  const file = segments[segments.length - 1] ?? "";
  if (segments[0] === "sdlc" && segments[1] === "features" && segments.length === 3) {
    // `sdlc/features/<slug>.md` is the whole feature; deeper paths are not, and
    // the slug must be a lowercase kebab-case slug the server accepts.
    if (file.length > 3 && file.endsWith(".md") && isKebabSlug(file.slice(0, -3))) {
      return "feature";
    }
    return null;
  }
  if (
    segments[0] === "sdlc" &&
    segments[1] === "epics" &&
    segments.length === 4 &&
    file === "description.md"
  ) {
    // `sdlc/epics/<number>-<slug>/description.md` is the whole epic; the epic
    // directory must be a four-digit-prefixed kebab-case name.
    if (isNumberedSlug(segments[2] ?? "")) {
      return "epic";
    }
  }
  if (segments[0] === "sdlc" && segments[1] === "decisions" && segments.length === 3) {
    // `sdlc/decisions/<number>-<slug>.md` is the whole decision; the decision
    // file must be a four-digit-prefixed kebab-case name, exactly like the
    // server's `is_numbered_slug`, and the decision is the kind that carries
    // the ADR status vocabulary (ticket 0032).
    if (file.endsWith(".md") && file.length > 4 && isNumberedSlug(file.slice(0, -3))) {
      return "decision";
    }
  }
  if (
    segments[0] === "sdlc" &&
    segments[1] === "epics" &&
    segments.length === 5 &&
    segments[3] === "tickets" &&
    isNumberedSlug(segments[2] ?? "") &&
    file.endsWith(".md") &&
    isNumberedSlug(file.slice(0, -3))
  ) {
    // `sdlc/epics/<number>-<slug>/tickets/<number>-<slug>.md` is the whole
    // ticket; the epic directory and the ticket file both have to be
    // backend-valid numbered documents, and the ticket vocabulary is
    // todo/in-progress/done (ticket 0052), so an open ticket gets the status
    // control of the toolbar while its epic membership stays the path, never a
    // front matter key.
    return "ticket";
  }
  return null;
}

// The decision number a decision path names, as a number without leading zeros,
// or null for paths that are not decisions; the file name keeps the four-digit
// padding while the pointer is stored without it (ticket 0031).
export function decisionNumberOf(path) {
  // The number of a decision only names a decision, so a path that is not a
  // backend-valid decision path never resolves to a decision number (ticket
  // 0032, final closing review round).
  if (statusKindOf(path) !== "decision") {
    return null;
  }
  const segments = pathSegments(path);
  const file = segments[segments.length - 1] ?? "";
  const stem = file.slice(0, -3); // The .md suffix is guaranteed by the statusKindOf check above.
  if (!isNumberedSlug(stem)) {
    return null;
  }
  const dash = stem.indexOf("-");
  const prefix = stem.slice(0, dash);
  return Number(prefix);
}

// The epic directory a document path names, or null when the path is neither an
// epic document (`sdlc/epics/<number>-<slug>/description.md`) nor a ticket of
// one (`sdlc/epics/<number>-<slug>/tickets/<kebab>.md`); the directory is the
// source of truth for the epic a ticket belongs to (ticket 0049).
export function epicDirectoryOf(path) {
  const segments = pathSegments(path);
  const invalidSegment = (segment) =>
    segment === ".." || segment === "." || segment === "" || segment.includes("\0") || segment.includes("\\");
  if (segments.some((segment) => invalidSegment(segment))) {
    // A path that the server's document path rules reject never names an epic.
    return null;
  }
  const file = segments[segments.length - 1] ?? "";
  const isEpicDocument =
    segments[0] === "sdlc" && segments[1] === "epics" && segments.length === 4 && file === "description.md";
  const isTicket =
    segments[0] === "sdlc" && segments[1] === "epics" && segments.length === 5 && segments[3] === "tickets";
  if (!isEpicDocument && !isTicket) {
    return null;
  }
  const epic = segments[2] ?? "";
  if (epic.length === 0 || !isNumberedSlug(epic)) {
    return null;
  }
  return epic;
}
