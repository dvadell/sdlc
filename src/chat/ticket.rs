//! The ticket-generation context of one chat turn (tickets 0049 and 0051).

use serde::Deserialize;

/// One ticket that already exists for the feature in the selected epic, as the
/// regeneration context of ticket 0051.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ExistingTicket {
    /// The project-relative path of the ticket file, numbered as on disk.
    pub path: String,
    /// The full content of the ticket.
    pub content: String,
}

/// The feature and the epic a ticket-generation turn drafts new tickets for,
/// plus the tickets that already exist for the feature in the selected epic.
///
/// The feature is the slug of the feature spec the tickets are generated from,
/// without the `.md` suffix; the epic is the exact epic directory name, such as
/// `0007-ticket-generation`. Both are optional, but a ticket-generation turn
/// carries both (the generation prompt asks for an open feature or a chosen
/// feature and a chosen epic), and the server validates them before they reach
/// the prompt builder and the proposal creation. The existing tickets ride along
/// as the regeneration context of ticket 0051; they are empty while the turn
/// drafts fresh tickets.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TicketContext {
    /// The slug of the feature the tickets are generated from, without `.md`.
    pub feature: Option<String>,
    /// The exact epic directory name the tickets are drafted into.
    pub epic: Option<String>,
    /// The tickets that already exist for the feature in the selected epic, as
    /// included regeneration context; empty when the turn drafts new tickets.
    #[serde(default)]
    pub existing: Vec<ExistingTicket>,
}
