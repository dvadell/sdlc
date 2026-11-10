//! Chat: upstream client, stream parser, prompt builder and service.

pub mod client;
pub mod stream;

pub use client::{ChatClient, ChatError, ChatMessage};
pub use stream::parse;
