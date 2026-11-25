pub mod claude;
pub mod codex;
mod model;
mod pairing;
mod server;
pub use model::{Conversation, Library};
pub use server::Reader;
