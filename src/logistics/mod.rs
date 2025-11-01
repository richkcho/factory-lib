//! Core logistics primitives and data structures for moving item stacks.

pub mod belt;
pub mod belt_connection;
pub mod buffered_splitter;
pub mod splitter;
pub mod stack;

// Re-export the main types for easier access
pub use belt::Belt;
pub use belt_connection::{BeltInputConnection, BeltOutputConnection, Connection, OutputBatch};
pub use buffered_splitter::BufferedSplitter;
pub use splitter::Splitter;
pub use stack::Stack;
