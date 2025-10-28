use crate::types::ItemType;

/// Represents a stack of homogeneous items traveling through factory logistics.
#[derive(Debug, Clone)]
pub struct Stack {
    /// Item identifier representing the type in this stack.
    pub item_type: ItemType,
    /// How many items are contained in this stack.
    pub item_count: u16,
    /// How many identical stacks are represented by this entry.
    pub multiplicity: u32,
}

impl Stack {
    /// Creates a new stack for the given item type with the provided quantity.
    pub fn new(item_type: ItemType, item_count: u16) -> Self {
        Self {
            item_type,
            item_count,
            multiplicity: 1,
        }
    }

    /// Returns `true` if the stack holds no items.
    pub fn is_empty(&self) -> bool {
        self.item_count == 0
    }
}

impl PartialEq for Stack {
    fn eq(&self, other: &Self) -> bool {
        self.item_type == other.item_type && self.item_count == other.item_count
    }
}

impl Eq for Stack {}
