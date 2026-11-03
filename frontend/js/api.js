//! One place for every HTTP call the frontend makes.

/** The error shape the API answers with: `{ "error": { "code", "message" } }`. */
export class ApiError extends Error {
  constructor(code, message, status) {
    super(message);
    this.name = "ApiError";
    this.code = code;
    this.status = status;
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

export async function createDoc(kind, title, epic, feature) {
  const body = { kind, title };
  if (epic) {
    body.epic = epic;
  }
  if (feature) {
    body.feature = feature;
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
