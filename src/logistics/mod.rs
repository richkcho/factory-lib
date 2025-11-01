//! Core logistics primitives and data structures for moving item stacks.

pub mod belt;
pub mod belt_connection;
pub mod buffered_splitter;
pub mod splitter;
pub mod stack;

// Re-export the main types for easier access
pub use belt::Belt;
pub use belt_connection::{BeltConnection, BeltConnectionKind};
pub use buffered_splitter::BufferedSplitter;
pub use stack::Stack;
pub use splitter::Splitter;