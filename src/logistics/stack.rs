#[derive(Debug, Clone, PartialEq)]
pub struct Stack {
    pub item_type: u16,
    pub item_count: u16,
}

impl Stack {
    pub fn new(item_type: u16, item_count: u16) -> Self {
        Self {
            item_type,
            item_count,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.item_count == 0
    }

    pub fn split(&mut self, count: u16) -> Option<Stack> {
        if count >= self.item_count {
            return None;
        }

        self.item_count -= count;
        Some(Stack::new(self.item_type, count))
    }
}
