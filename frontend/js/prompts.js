//! The built-in documentation prompts of the chat panel (ticket 0046).
//!
//! The whole prompt catalog lives here; the server has no prompt catalog. Each
//! prompt carries the text the person can edit and the documents it attaches, so
//! one click fills the composer and the person adjusts before sending.

// The tooltip of the one prompt that is not wired yet; ticket 0049 replaces this
// disabled entry with the real ticket-generation prompt.
export const DISABLED_PROMPT_HINT = "Ticket generation from a feature arrives in sdlc ticket 0049.";

// A built-in prompt: the id the panel keys on, a short label, the short
// description shown as the button title, the prompt text, and the documents it
// attaches. Attachments follow the add-only rule of ticket 0039, so a prompt
// adds its documents and never replaces the ones already attached.
export const PROMPTS = [
  {
    id: "split-prd",
    label: "Split PRD",
    title: "Split the PRD into focused documents by concern.",
    text: [
      "Split this PRD by concern and propose the full new documents.",
      "",
      "The PRD mixes product intent, architecture, library choices and code style, so separate the concerns into focused documents.",
      "",
      "Propose sdlc/architecture.md for the architecture concerns, sdlc/conventions.md for the conventions and one sdlc/features/<feature>.md per remaining feature, each with one FILE block carrying the complete new content:",
      "",
      "=== FILE: sdlc/architecture.md ===",
      "The complete new content of the architecture document.",
      "=== END FILE ===",
      "",
      "Ask clarifying questions instead of guessing. Propose full files, not patches.",
    ].join("\n"),
    attachments: ["sdlc/PRD.md"],
  },
  {
    id: "feature-spec",
    label: "Draft feature spec",
    title: "Draft a feature spec from this idea.",
    text: [
      "Draft a feature spec from this idea and propose it as a full file.",
      "",
      "Collect the problem, the goal, the scope and the non-goals of the idea, then propose the complete content of sdlc/features/<feature>.md as one FILE block:",
      "",
      "=== FILE: sdlc/features/<feature>.md ===",
      "The complete new content of the feature spec.",
      "=== END FILE ===",
      "",
      "Ask clarifying questions instead of guessing. Propose full files, not patches.",
    ].join("\n"),
    attachments: ["sdlc/PRD.md"],
  },
  {
    id: "adr",
    label: "Draft ADR",
    title: "Draft an architecture decision record for this choice.",
    text: [
      "Draft an ADR for this choice and propose it as a full file.",
      "",
      "Collect the context, the decision and its consequences, then propose the complete content of sdlc/decisions/NNNN-<slug>.md as one FILE block:",
      "",
      "=== FILE: sdlc/decisions/0001-use-example.md ===",
      "The complete new content of the decision.",
      "=== END FILE ===",
      "",
      "Ask clarifying questions instead of guessing. Propose full files, not patches.",
    ].join("\n"),
    attachments: ["sdlc/architecture.md", "sdlc/conventions.md"],
  },
  {
    id: "trim-agents",
    label: "Trim AGENTS.md",
    title: "Trim AGENTS.md to short actionable rules.",
    text: [
      "Trim AGENTS.md and keep it consistent with the PRD, architecture and conventions. Keep the rules actionable and short; propose the full new file, not a patch:",
      "",
      "=== FILE: AGENTS.md ===",
      "The complete new content of the AI rules file.",
      "=== END FILE ===",
      "",
      "Ask clarifying questions instead of guessing. Propose full files, not patches.",
    ].join("\n"),
    attachments: ["AGENTS.md", "sdlc/PRD.md", "sdlc/architecture.md", "sdlc/conventions.md"],
  },
  {
    id: "check-docs",
    label: "Check docs",
    title: "Check the documents for contradictions or duplication.",
    text: [
      "Check the attached documents for contradictions or duplication.",
      "",
      "List every contradiction or duplicated fact you find across the PRD, the architecture, the conventions and AGENTS.md.",
      "",
      "For every document that needs a fix, propose the complete new content as one FILE block per document:",
      "",
      "=== FILE: sdlc/architecture.md ===",
      "The complete new content of the architecture document.",
      "=== END FILE ===",
      "",
      "Ask clarifying questions instead of guessing. Propose full files, not patches.",
    ].join("\n"),
    attachments: ["sdlc/PRD.md", "sdlc/architecture.md", "sdlc/conventions.md", "AGENTS.md"],
  },
  {
    id: "generate-tickets",
    label: "Generate tickets",
    title: "Ticket generation from a feature arrives in sdlc ticket 0049.",
    text: "Generating tickets from this feature will be wired in sdlc ticket 0049; it is not available yet.",
    attachments: [],
    disabled: true,
  },
];

// The built-in prompt with the given id, or null when the catalog has none.
export function promptById(id) {
  return PROMPTS.find((prompt) => prompt.id === id) ?? null;
}

// The built-in prompts that are ready to use; the disabled ticket-generation
// entry is left out until ticket 0049 wires it (ticket 0046).
export function enabledPrompts() {
  return PROMPTS.filter((prompt) => !prompt.disabled);
}
