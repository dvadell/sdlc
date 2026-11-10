//! One place for every HTTP call the frontend makes.

/** The error shape the API answers with: `{ "error": { "code", "message" } }`. */
export class ApiError extends Error {
  constructor(code, message, status, applied = null, failed = null) {
    super(message);
    this.name = "ApiError";
    this.code = code;
    this.status = status;
    // The applied and failing paths of a proposal accept that stopped midway;
    // both are null for every other error, and the partial apply answers with
    // them beside the error envelope (ticket 0043, ticket 0045).
    this.applied = applied;
    this.failed = failed;
  }
}

// The server requires an `application/json` content type on every state-changing
// method, including deletes that carry no body (ticket 0006).
const MUTATIONS = new Set(["POST", "PUT", "PATCH", "DELETE"]);

async function request(method, path, body, { timeout = 30_000 } = {}) {
  const headers = new Headers();
  const hasBody = body !== undefined && body !== null;
  if (hasBody || MUTATIONS.has(method)) {
    headers.set("Content-Type", "application/json");
  }

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeout);

  try {
    const response = await fetch(path, {
      method,
      headers,
      body: hasBody ? JSON.stringify(body) : undefined,
      signal: controller.signal,
    });

    let payload = null;
    try {
      payload = await response.json();
    } catch (error) {
      if (error instanceof DOMException && error.name === "AbortError") {
        throw new ApiError("timeout", "The request timed out", 408);
      }
      if (response.ok) {
        throw new ApiError("invalid_response", "the server returned invalid JSON", response.status);
      }
    }

    if (!response.ok) {
      const error = payload?.error;
      throw new ApiError(
        error?.code ?? "request_failed",
        error?.message ?? `request failed with status ${response.status}`,
        response.status,
        payload?.applied,
        payload?.failed,
      );
    }

    return payload;
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new ApiError("timeout", "The request timed out", 408);
    }
    throw error;
  } finally {
    clearTimeout(timer);
  }
}

function validateDocPath(path) {
  if (!path || typeof path !== "string") {
    throw new ApiError("invalid_path", "no document path given");
  }

  const segments = path.split("/");
  const invalid = (segment) =>
    segment === ".." ||
    segment === "." ||
    segment === "" ||
    segment.includes("\0") ||
    segment.includes("\\");
  if (segments.some(invalid)) {
    throw new ApiError("invalid_path", "path must not contain invalid segments");
  }

  return segments.map(encodeURIComponent).join("/");
}

// Encodes each path segment, preserving the `/` separators, for the wildcard
// doc route. Segments are encoded one by one so the slashes survive. Dot
// segments are rejected so a caller can never steer the server at `..`.
export function docUrl(path) {
  const encoded = validateDocPath(path);
  return `/api/docs/${encoded}`;
}

export async function getConfig() {
  return request("GET", "/api/config");
}

export async function getTree() {
  return request("GET", "/api/tree");
}

export async function getDoc(path) {
  return request("GET", docUrl(path));
}

export async function saveDoc(path, content, expectedHash) {
  return request("PUT", docUrl(path), { content, expected_hash: expectedHash });
}

export async function createDoc(kind, title, epic, feature, status, supersedes) {
  const body = { kind, title };
  if (epic) {
    body.epic = epic;
  }
  if (feature) {
    body.feature = feature;
  }
  if (kind === "decision") {
    // Decisions create with an ADR status (default `proposed`) and an optional
    // supersedes pointer to another decision number (ticket 0031).
    body.status = status ?? "proposed";
    if (supersedes !== undefined && supersedes !== null && supersedes !== "") {
      body.supersedes = supersedes;
    }
  }
  return request("POST", "/api/docs", body);
}

export async function moveDoc(from, to) {
  // Validate only; the raw paths are sent in the JSON body (URL-encoding is for
  // the path route, where the server re-validates via `DocPath::parse`). The
  // response carries the validated destination path, which the caller uses to
  // follow a rename.
  void validateDocPath(from);
  void validateDocPath(to);
  return request("POST", "/api/docs/move", { from, to });
}

export async function deleteDoc(path) {
  return request("DELETE", docUrl(path));
}

export async function scaffold() {
  return request("POST", "/api/scaffold", {});
}

// The accept of a proposal: writes the files it proposes, or answers why it
// cannot (a conflict, a midway partial apply or an unknown id after a restart).
export function proposalUrl(id) {
  return `/api/proposals/${encodeURIComponent(id)}`;
}

export async function acceptProposal(id) {
  return request("POST", `${proposalUrl(id)}/accept`, {});
}

// The reject of a proposal: discards it without touching the documents.
export async function rejectProposal(id) {
  return request("POST", `${proposalUrl(id)}/reject`, {});
}
