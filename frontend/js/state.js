//! The single shared state object of the frontend.

export const state = {
  // Response of `GET /api/config`.
  config: null,
  // Response of `GET /api/tree`.
  tree: null,
  // The selected document: its validated path, content and the hash the content
  // was loaded at.
  current: { path: null, content: "", hash: null },
  // Whether the current document has unsaved edits.
  dirty: false,
  // Chat panel state: the messages and whether a stream is running. The
  // placeholder shown before the chat wires up in milestone 3.
  chat: { messages: [], streaming: false, placeholder: "AI chat arrives in milestone 3" },
  // The proposal currently awaiting a decision, if any.
  proposal: null,
};
