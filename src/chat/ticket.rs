//! The ticket-generation context of one chat turn (ticket 0049).

use serde::Deserialize;

/// The feature and the epic a ticket-generation turn drafts new tickets for.
///
/// The feature is the slug of the feature spec the tickets are generated from,
/// without the `.md` suffix; the epic is the exact epic directory name, such as
/// `0007-ticket-generation`. Both are optional, but a ticket-generation turn
/// carries both (the generation prompt asks for an open feature or a chosen
/// feature and a chosen epic), and the server validates them before they reach
/// the prompt builder and the proposal creation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TicketContext {
    /// The slug of the feature the tickets are generated from, without `.md`.
    pub feature: Option<String>,
    /// The exact epic directory name the tickets are drafted into.
    pub epic: Option<String>,
}
