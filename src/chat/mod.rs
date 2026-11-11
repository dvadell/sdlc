//! Chat: upstream client, stream parser, prompt builder and service.

pub mod blocks;
pub mod client;
pub mod prompt;
pub mod stream;
pub mod ticket;

pub use blocks::{Block, Parsed};
pub use client::{ChatClient, ChatError, ChatMessage};
pub use prompt::{Message, SYSTEM_PROMPT, build};
pub use stream::parse;
pub use ticket::TicketContext;
