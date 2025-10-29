//! Core logistics primitives and data structures for moving item stacks.

pub mod belt;
pub mod belt_connection;
pub mod stack;

// Re-export the main types for easier access
pub use belt::Belt;
pub use belt_connection::{BeltConnection, BeltConnectionKind};
pub use stack::Stack;
