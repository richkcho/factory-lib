use crate::logistics::Stack;
use crate::types::ItemType;

#[derive(Debug, Clone)]
struct ConnectionState {
    item_limit: u16,
    item_filter: Option<Vec<ItemType>>,
    buffer: Option<Stack>,
}

impl ConnectionState {
    fn new(item_limit: u16, item_filter: Option<Vec<ItemType>>) -> Self {
        Self {
            item_limit,
            item_filter,
            buffer: None,
        }
    }

    fn item_filter(&self) -> Option<&[ItemType]> {
        self.item_filter.as_deref()
    }

    fn buffered_item_count(&self) -> u16 {
        self.buffer
            .as_ref()
            .map(|stack| stack.item_count)
            .unwrap_or(0)
    }

    fn is_empty(&self) -> bool {
        self.buffer.is_none()
    }

    fn current_item_type(&self) -> Option<ItemType> {
        self.buffer.as_ref().map(|stack| stack.item_type)
    }

    fn can_take_item_type(&self, item_type: ItemType) -> bool {
        if let Some(filter) = &self.item_filter {
            return filter.contains(&item_type);
        } else if let Some(buffer) = &self.buffer {
            return (buffer.item_type == item_type) && (buffer.item_count < self.item_limit);
        }

        true
    }

    fn can_take_item_count(&self, item_count: u16) -> bool {
        if let Some(buffer) = &self.buffer {
            return (buffer.item_count + item_count) <= self.item_limit;
        }

        item_count <= self.item_limit
    }

    fn can_accept_stack(&self, stack: &Stack) -> bool {
        if let Some(filter) = &self.item_filter
            && !filter.contains(&stack.item_type)
        {
            return false;
        }

        let stack_items = stack.item_count as u32 * stack.multiplicity;
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

    fn accept_stack(&mut self, stack: &Stack) -> bool {
        if !self.can_accept_stack(stack) {
            return false;
        }

        match self.buffer.as_mut() {
            Some(existing) => {
                existing.item_count += stack.item_count;
            }
            None => {
                self.buffer = Some(stack.clone());
            }
        }

        true
    }

    fn inc_item_count(&mut self, item_type: ItemType, item_count: u16) -> u16 {
        let buffer = if let Some(buffer) = self.buffer.as_mut() {
            if buffer.item_type != item_type {
                return item_count;
            }
            buffer
        } else {
            self.buffer = Some(Stack {
                item_type,
                item_count: 0,
                multiplicity: 1,
            });
            self.buffer.as_mut().expect("buffer just initialized")
        };

        let current = buffer.item_count;
        let allowed = self.item_limit - current;
        let amount_to_add = item_count.min(allowed);
        buffer.item_count += amount_to_add;
        item_count - amount_to_add
    }

    fn dec_item_count(&mut self, item_count: u16) -> u16 {
        let buffer = if let Some(buffer) = self.buffer.as_mut() {
            buffer
        } else {
            return item_count;
        };

        let current = buffer.item_count;
        let amount_to_remove = item_count.min(current);
        buffer.item_count -= amount_to_remove;

        if buffer.item_count == 0 {
            self.buffer = None;
        }

        item_count - amount_to_remove
    }

    fn max_acceptable_item_count(&self) -> u16 {
        if let Some(buffer) = &self.buffer {
            self.item_limit - buffer.item_count
        } else {
            self.item_limit
        }
    }

    fn max_acceptable_stacks(&self, stack: &Stack) -> u32 {
        if stack.multiplicity != 1 {
            return 0;
        }

        if let Some(filter) = &self.item_filter
            && !filter.contains(&stack.item_type)
        {
            return 0;
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
}

pub trait Connection {
    fn item_limit(&self) -> u16;
    fn item_filter(&self) -> Option<&[ItemType]>;
    fn set_item_filter(&mut self, filter: Option<Vec<ItemType>>);
    fn buffered_item_count(&self) -> u16;
    fn is_empty(&self) -> bool;
    fn current_item_type(&self) -> Option<ItemType>;
    fn can_take_item_type(&self, item_type: ItemType) -> bool;
    fn can_take_item_count(&self, item_count: u16) -> bool;
    fn can_accept_stack(&self, stack: &Stack) -> bool;
    fn accept_stack(&mut self, stack: &Stack) -> bool;
    fn inc_item_count(&mut self, item_type: ItemType, item_count: u16) -> u16;
    fn dec_item_count(&mut self, item_count: u16) -> u16;
    fn max_acceptable_item_count(&self) -> u16;
    fn max_acceptable_stacks(&self, stack: &Stack) -> u32;
}

#[derive(Debug, Clone)]
pub struct BeltInputConnection {
    state: ConnectionState,
}

impl BeltInputConnection {
    pub fn new(item_limit: u16, item_filter: Option<Vec<ItemType>>) -> Self {
        Self {
            state: ConnectionState::new(item_limit, item_filter),
        }
    }
}

impl Connection for BeltInputConnection {
    fn item_limit(&self) -> u16 {
        self.state.item_limit
    }

    fn item_filter(&self) -> Option<&[ItemType]> {
        self.state.item_filter()
    }

    fn set_item_filter(&mut self, filter: Option<Vec<ItemType>>) {
        self.state.item_filter = filter;
    }

    fn buffered_item_count(&self) -> u16 {
        self.state.buffered_item_count()
    }

    fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    fn current_item_type(&self) -> Option<ItemType> {
        self.state.current_item_type()
    }

    fn can_take_item_type(&self, item_type: ItemType) -> bool {
        self.state.can_take_item_type(item_type)
    }

    fn can_take_item_count(&self, item_count: u16) -> bool {
        self.state.can_take_item_count(item_count)
    }

    fn can_accept_stack(&self, stack: &Stack) -> bool {
        self.state.can_accept_stack(stack)
    }

    fn accept_stack(&mut self, stack: &Stack) -> bool {
        self.state.accept_stack(stack)
    }

    fn inc_item_count(&mut self, item_type: ItemType, item_count: u16) -> u16 {
        self.state.inc_item_count(item_type, item_count)
    }

    fn dec_item_count(&mut self, item_count: u16) -> u16 {
        self.state.dec_item_count(item_count)
    }

    fn max_acceptable_item_count(&self) -> u16 {
        self.state.max_acceptable_item_count()
    }

    fn max_acceptable_stacks(&self, stack: &Stack) -> u32 {
        self.state.max_acceptable_stacks(stack)
    }
}

#[derive(Debug, Clone)]
pub struct BeltOutputConnection {
    state: ConnectionState,
    output_stack_size: u16,
}

impl BeltOutputConnection {
    pub fn new(
        item_limit: u16,
        output_stack_size: u16,
        item_filter: Option<Vec<ItemType>>,
    ) -> Self {
        debug_assert!(output_stack_size > 0, "output stack size must be non-zero");

        Self {
            state: ConnectionState::new(item_limit, item_filter),
            output_stack_size,
        }
    }

    pub fn output_stack_size(&self) -> u16 {
        self.output_stack_size
    }

    pub fn take_output_batch(&mut self, max_stacks: u32) -> Option<OutputBatch> {
        if max_stacks == 0 {
            return None;
        }

        let buffer = self.state.buffer.as_ref()?;
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
            self.state.buffer = None;
        } else if let Some(existing) = self.state.buffer.as_mut() {
            existing.item_count = remaining as u16;
        }

        Some(OutputBatch {
            full_stack,
            partial_stack,
        })
    }

    pub fn peek_next_output(&self) -> Option<Stack> {
        let buffer = self.state.buffer.as_ref()?;
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

    pub fn take_next_output(&mut self) -> Option<Stack> {
        let count;
        {
            let buffer = self.state.buffer.as_ref()?;
            if buffer.item_count == 0 {
                return None;
            }
            count = buffer.item_count.min(self.output_stack_size);
        }

        let mut buffer = self.state.buffer.take().expect("buffer existed");
        let emitted = Stack {
            item_type: buffer.item_type,
            item_count: count,
            multiplicity: 1,
        };

        buffer.item_count -= count;
        if buffer.item_count > 0 {
            buffer.multiplicity = 1;
            self.state.buffer = Some(buffer);
        }

        Some(emitted)
    }
}

impl Connection for BeltOutputConnection {
    fn item_limit(&self) -> u16 {
        self.state.item_limit
    }

    fn item_filter(&self) -> Option<&[ItemType]> {
        self.state.item_filter()
    }

    fn set_item_filter(&mut self, filter: Option<Vec<ItemType>>) {
        self.state.item_filter = filter;
    }

    fn buffered_item_count(&self) -> u16 {
        self.state.buffered_item_count()
    }

    fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    fn current_item_type(&self) -> Option<ItemType> {
        self.state.current_item_type()
    }

    fn can_take_item_type(&self, item_type: ItemType) -> bool {
        self.state.can_take_item_type(item_type)
    }

    fn can_take_item_count(&self, item_count: u16) -> bool {
        self.state.can_take_item_count(item_count)
    }

    fn can_accept_stack(&self, stack: &Stack) -> bool {
        self.state.can_accept_stack(stack)
    }

    fn accept_stack(&mut self, stack: &Stack) -> bool {
        self.state.accept_stack(stack)
    }

    fn inc_item_count(&mut self, item_type: ItemType, item_count: u16) -> u16 {
        self.state.inc_item_count(item_type, item_count)
    }

    fn dec_item_count(&mut self, item_count: u16) -> u16 {
        self.state.dec_item_count(item_count)
    }

    fn max_acceptable_item_count(&self) -> u16 {
        self.state.max_acceptable_item_count()
    }

    fn max_acceptable_stacks(&self, stack: &Stack) -> u32 {
        self.state.max_acceptable_stacks(stack)
    }
}

#[derive(Debug, Clone)]
pub struct OutputBatch {
    pub full_stack: Option<Stack>,
    pub partial_stack: Option<Stack>,
}

impl OutputBatch {
    pub fn num_stacks(&self) -> u32 {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_stack(item_type: u16, count: u16) -> Stack {
        Stack::new(item_type, count)
    }

    #[test]
    fn accept_stack_respects_limit_and_type_for_output() {
        let mut connection = BeltInputConnection::new(10, None);

        let stack_a = sample_stack(1, 6);
        let stack_a_small = sample_stack(1, 4);
        let stack_b = sample_stack(2, 1);

        assert!(connection.accept_stack(&stack_a));
        assert_eq!(connection.buffered_item_count(), 6);

        assert!(connection.accept_stack(&stack_a_small));
        assert_eq!(connection.buffered_item_count(), 10);

        assert!(!connection.accept_stack(&sample_stack(1, 1)));
        assert!(!connection.accept_stack(&stack_b));
    }

    #[test]
    fn item_filter_blocks_disallowed_items_for_input() {
        let mut connection = BeltOutputConnection::new(5, 3, Some(vec![1]));

        assert!(connection.accept_stack(&sample_stack(1, 2)));
        assert_eq!(connection.buffered_item_count(), 2);
        assert!(!connection.accept_stack(&sample_stack(2, 1)));
    }

    #[test]
    fn taking_output_consumes_items() {
        let mut connection = BeltOutputConnection::new(6, 2, None);
        assert!(connection.accept_stack(&sample_stack(3, 5)));

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
