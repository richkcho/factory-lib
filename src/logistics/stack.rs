/// Represents a stack of homogeneous items traveling through factory logistics.
#[derive(Debug, Clone, PartialEq)]
pub struct Stack {
    /// Item identifier representing the type in this stack.
    pub item_type: u16,
    /// How many items are contained in this stack.
    pub item_count: u16,
}

impl Stack {
    /// Creates a new stack for the given item type with the provided quantity.
    pub fn new(item_type: u16, item_count: u16) -> Self {
        Self {
            item_type,
            item_count,
        }
    }

    /// Returns `true` if the stack holds no items.
    pub fn is_empty(&self) -> bool {
        self.item_count == 0
    }

    /// Splits `count` items off this stack into a new stack, shrinking the original in place.
    /// Returns `None` when `count` is not strictly smaller than the current stack size.
    pub fn split(&mut self, count: u16) -> Option<Stack> {
        if count >= self.item_count {
            return None;
        }

        self.item_count -= count;
        Some(Stack::new(self.item_type, count))
    }
}
