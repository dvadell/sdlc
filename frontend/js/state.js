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
  // Whether the sidebar lists only the tickets of the currently open epic through
  // the optional "Tickets for current epic" filter of ticket 0053; it defaults to
  // off and lives here, so it is a session-only display filter.
  ticketsForCurrentEpic: false,
  // Chat panel state: the messages of the in-page transcript, whether a reply
  // is streaming and the attached document paths of the conversation (ticket
  // 0039). The transcript and the attachments live here and nowhere else: no
  // localStorage, no server history, so a page reload starts a fresh conversation.
  chat: { messages: [], streaming: false, attachments: [], ticketContext: null },
  // The proposal currently awaiting a decision, if any.
  proposal: null,
};
