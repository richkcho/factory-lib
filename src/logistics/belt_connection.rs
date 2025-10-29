use crate::logistics::Stack;
use crate::types::ItemType;

/// Denotes whether a [`BeltConnection`] acts as a source feeding items onto a belt
/// or as a sink receiving items from a belt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeltConnectionKind {
    Input,
    Output,
}

/// Models a buffer that can either provide stacks to a belt or accept stacks from it.
///
/// A connection holds at most one stack entry with `multiplicity == 1`, but it may
/// aggregate additional items into that entry up to the configured `item_limit` as
/// long as the incoming stacks match the stored item type. Connections can also
/// restrict which item types are accepted via an optional filter.
#[derive(Debug, Clone)]
pub struct BeltConnection {
    kind: BeltConnectionKind,
    item_limit: u16,
    output_stack_size: u16,
    item_filter: Option<Vec<ItemType>>,
    buffer: Option<Stack>,
}

#[derive(Debug, Clone)]
pub(crate) struct OutputBatch {
    pub(crate) full_stack: Option<Stack>,
    pub(crate) partial_stack: Option<Stack>,
}

impl OutputBatch {
    pub(crate) fn num_stacks(&self) -> u32 {
        let mut used = 0;
        if let Some(full) = &self.full_stack {
            used += full.multiplicity;
        }
        if self.partial_stack.is_some() {
            used += 1;
        }
        used
    }
}

impl BeltConnection {
    /// Creates a new belt connection with the provided configuration.
    pub fn new(
        kind: BeltConnectionKind,
        item_limit: u16,
        output_stack_size: u16,
        item_filter: Option<Vec<ItemType>>,
    ) -> Self {
        debug_assert!(output_stack_size > 0, "output stack size must be non-zero");

        Self {
            kind,
            item_limit,
            output_stack_size,
            item_filter,
            buffer: None,
        }
    }

    /// Returns the orientation of this connection.
    pub fn kind(&self) -> BeltConnectionKind {
        self.kind
    }

    /// Returns the maximum number of items that can be buffered.
    pub fn item_limit(&self) -> u16 {
        self.item_limit
    }

    /// Returns the desired size of emitted stacks when this connection provides items.
    pub fn output_stack_size(&self) -> u16 {
        self.output_stack_size
    }

    /// Returns the item filter, if any, limiting accepted item types.
    pub fn item_filter(&self) -> Option<&[ItemType]> {
        self.item_filter.as_deref()
    }

    /// Replaces the item filter with a new value.
    pub fn set_item_filter(&mut self, filter: Option<Vec<ItemType>>) {
        self.item_filter = filter;
    }

    /// Returns the number of items currently buffered in this connection.
    pub fn buffered_item_count(&self) -> u16 {
        self.buffer
            .as_ref()
            .map(|stack| stack.item_count)
            .unwrap_or(0)
    }

    /// Returns `true` if the connection currently holds no items.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_none()
    }

    /// Returns `true` if the provided stack can be accepted without violating the
    /// connection's constraints.
    pub fn can_accept_stack(&self, stack: &Stack) -> bool {
        if stack.multiplicity != 1 {
            return false;
        }

        if let Some(filter) = &self.item_filter {
            if !filter.contains(&stack.item_type) {
                return false;
            }
        }

        let stack_items = stack.item_count as u32;
        if stack_items == 0 {
            return true;
        }

        match &self.buffer {
            None => stack_items <= self.item_limit as u32,
            Some(existing) => {
                if existing.item_type != stack.item_type {
                    return false;
                }

                let total = existing.item_count as u32 + stack_items;
                total <= self.item_limit as u32
            }
        }
    }

    /// Attempts to accept the provided stack, returning `true` if it was consumed.
    pub fn accept_stack(&mut self, mut stack: Stack) -> bool {
        if !self.can_accept_stack(&stack) {
            return false;
        }

        stack.multiplicity = 1;

        match self.buffer.as_mut() {
            Some(existing) => {
                existing.item_count += stack.item_count;
            }
            None => {
                self.buffer = Some(stack);
            }
        }

        true
    }

    pub(crate) fn max_acceptable_stacks(&self, stack: &Stack) -> u32 {
        if stack.multiplicity != 1 {
            return 0;
        }

        if let Some(filter) = &self.item_filter {
            if !filter.contains(&stack.item_type) {
                return 0;
            }
        }

        let per_stack_items = stack.item_count as u32;
        if per_stack_items == 0 {
            return u32::MAX;
        }

        match &self.buffer {
            None => {
                let limit = self.item_limit as u32;
                if per_stack_items > limit {
                    0
                } else {
                    limit / per_stack_items
                }
            }
            Some(existing) => {
                if existing.item_type != stack.item_type {
                    return 0;
                }

                let limit = self.item_limit as u32;
                if existing.item_count as u32 >= limit {
                    return 0;
                }

                let remaining = limit - existing.item_count as u32;
                remaining / per_stack_items
            }
        }
    }

    pub(crate) fn accept_stacks(&mut self, stack: &Stack, count: u32) -> bool {
        if count == 0 {
            return true;
        }

        if stack.multiplicity != 1 {
            return false;
        }

        let max = self.max_acceptable_stacks(stack);
        if count > max {
            return false;
        }

        let total_items = count * stack.item_count as u32;
        if total_items == 0 {
            return true;
        }

        match self.buffer.as_mut() {
            Some(existing) => {
                debug_assert_eq!(existing.item_type, stack.item_type);
                existing.item_count = (existing.item_count as u32 + total_items) as u16;
            }
            None => {
                self.buffer = Some(Stack {
                    item_type: stack.item_type,
                    item_count: total_items as u16,
                    multiplicity: 1,
                });
            }
        }

        true
    }

    pub(crate) fn take_output_batch(&mut self, max_stacks: u32) -> Option<OutputBatch> {
        if max_stacks == 0 {
            return None;
        }

        let buffer = self.buffer.as_ref()?;
        if buffer.item_count == 0 {
            return None;
        }

        let output_size = self.output_stack_size as u32;
        let mut items_available = buffer.item_count as u32;
        let mut slots_remaining = max_stacks;

        let mut full_stack_count = 0u32;
        if output_size > 0 {
            let possible_full = items_available / output_size;
            full_stack_count = possible_full.min(slots_remaining);
            items_available -= full_stack_count * output_size;
            slots_remaining -= full_stack_count;
        }

        let mut partial_stack_items = 0u16;
        if slots_remaining > 0 && items_available > 0 {
            partial_stack_items = items_available as u16;
        }

        if full_stack_count == 0 && partial_stack_items == 0 {
            return None;
        }

        let consumed_items = (full_stack_count * output_size) + partial_stack_items as u32;

        let full_stack = if full_stack_count > 0 {
            Some(Stack {
                item_type: buffer.item_type,
                item_count: self.output_stack_size,
                multiplicity: full_stack_count,
            })
        } else {
            None
        };

        let partial_stack = if partial_stack_items > 0 {
            Some(Stack {
                item_type: buffer.item_type,
                item_count: partial_stack_items,
                multiplicity: 1,
            })
        } else {
            None
        };

        let remaining = buffer.item_count as u32 - consumed_items;
        if remaining == 0 {
            self.buffer = None;
        } else {
            if let Some(existing) = self.buffer.as_mut() {
                existing.item_count = remaining as u16;
            }
        }

        Some(OutputBatch {
            full_stack,
            partial_stack,
        })
    }

    /// Returns a snapshot of the next stack that would be emitted when acting as an input.
    pub fn peek_next_output(&self) -> Option<Stack> {
        let buffer = self.buffer.as_ref()?;
        let count = buffer.item_count.min(self.output_stack_size);

        if count == 0 {
            return None;
        }

        Some(Stack {
            item_type: buffer.item_type,
            item_count: count,
            multiplicity: 1,
        })
    }

    /// Removes and returns the next stack that should be emitted when feeding a belt.
    pub fn take_next_output(&mut self) -> Option<Stack> {
        let count;
        {
            let buffer = self.buffer.as_ref()?;
            if buffer.item_count == 0 {
                return None;
            }
            count = buffer.item_count.min(self.output_stack_size);
        }

        let mut buffer = self.buffer.take().expect("buffer existed");
        let emitted = Stack {
            item_type: buffer.item_type,
            item_count: count,
            multiplicity: 1,
        };

        buffer.item_count -= count;
        if buffer.item_count > 0 {
            buffer.multiplicity = 1;
            self.buffer = Some(buffer);
        }

        Some(emitted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logistics::Stack;

    #[test]
    fn accept_stack_respects_limit_and_type() {
        let mut connection = BeltConnection::new(BeltConnectionKind::Output, 10, 5, None);
        let stack_a = Stack::new(1, 6);
        let stack_a_small = Stack::new(1, 4);
        let stack_b = Stack::new(2, 1);

        assert!(connection.accept_stack(stack_a));
        assert_eq!(connection.buffered_item_count(), 6);

        // Accepting a matching stack within the limit should succeed.
        assert!(connection.accept_stack(stack_a_small));
        assert_eq!(connection.buffered_item_count(), 10);

        // Further stacks would exceed the limit.
        assert!(!connection.accept_stack(Stack::new(1, 1)));

        // Different item types are rejected.
        assert!(!connection.accept_stack(stack_b));
    }

    #[test]
    fn item_filter_blocks_disallowed_items() {
        let mut connection = BeltConnection::new(BeltConnectionKind::Input, 5, 3, Some(vec![1]));

        assert!(connection.accept_stack(Stack::new(1, 2)));
        assert_eq!(connection.buffered_item_count(), 2);
        assert!(!connection.accept_stack(Stack::new(2, 1)));
    }

    #[test]
    fn taking_output_consumes_items() {
        let mut connection = BeltConnection::new(BeltConnectionKind::Input, 6, 2, None);
        assert!(connection.accept_stack(Stack::new(3, 5)));

        let first = connection.take_next_output().expect("stack available");
        assert_eq!(first.item_type, 3);
        assert_eq!(first.item_count, 2);
        assert_eq!(connection.buffered_item_count(), 3);

        let second = connection.take_next_output().expect("stack available");
        assert_eq!(second.item_count, 2);
        assert_eq!(connection.buffered_item_count(), 1);

        let third = connection.take_next_output().expect("stack available");
        assert_eq!(third.item_count, 1);
        assert!(connection.is_empty());
    }
}
