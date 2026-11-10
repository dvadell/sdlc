//! The single shared state object of the frontend.

export const state = {
  // Response of `GET /api/config`.
  config: null,
  // Response of `GET /api/tree`.
  tree: null,
  // The selected document: its validated path, content and the hash the content
  // was loaded at.
  current: {
    path: null,
    content: "",
    // The content the textarea is compared against; it only changes on load and
    // after a successful save, never while the person types.
    savedContent: "",
    hash: null,
  },
  // Whether the current document has unsaved edits.
  dirty: false,
  // Chat panel state: the messages of the in-page transcript and whether a
  // reply is streaming. The transcript lives here and nowhere else: no
  // localStorage, no server history (ticket 0038).
  chat: { messages: [], streaming: false },
  // The proposal currently awaiting a decision, if any.
  proposal: null,
};
