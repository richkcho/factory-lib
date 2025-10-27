//! Core logistics primitives and data structures for moving item stacks.

pub mod belt;
pub mod stack;

// Re-export the main types for easier access
pub use belt::Belt;
pub use stack::Stack;
