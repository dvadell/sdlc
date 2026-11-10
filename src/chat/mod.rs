//! Chat: upstream client, stream parser, prompt builder and service.

pub mod client;

pub use client::{ChatClient, ChatError, ChatMessage};
