use crate::logistics::Stack;
use crate::logistics::belt_connection::{BeltConnection, BeltConnectionKind, OutputBatch};
use crate::types::{ITEM_WIDTH, ItemType};
use std::collections::VecDeque;

// Physical width of a single stack on the belt measured in belt distance units.
//
// This value should remain a power of two for performance reasons when integrating with
// simulation backends that rely on bit masking.

/**
 * Represents an item on a conveyor belt. Each item keeps track of what it is carrying, if it is
 * part of a group (a series of contiguous items), and if it is at the end of a group (meaning
 * there is a gap to the next item on the belt) it also contains the distance to the next item.
 * This allows for efficient tracking and management of items on the belt in the typical cases
 * where items will be processed in groups, and most accesses are at the ends of the belt.
 */
#[derive(Debug, Clone)]
struct BeltItem {
    stack: Stack,
    // distance to the next item on the belt
    next_item_dist: Option<u32>,
    // if we are the head of the group
    is_group_head: bool,
    // if we are the tail of the group
    is_group_tail: bool,
    // if we are head or tail of the group, track the group size
    group_size: u32,
}

/// Models a Satisfactory-style conveyor belt that primarily supports pushing items on the back
/// and popping them from the front in FIFO order. Random access is intentionally deprioritized
/// because the belt is expected to be consumed from its ends.
#[derive(Debug)]
pub struct Belt {
    length: u32,
    speed: u32,
    // Consider moving from VecDeque to YCQueue
    items: VecDeque<BeltItem>,
    // how many empty spaces in the queue until we hit a stack
    empty_space_front: u32,
    // how many trailing empty spaces in the belt
    empty_space_back: u32,
    input_connection: Option<BeltConnection>,
    output_connection: Option<BeltConnection>,
}

impl Belt {
    /// Creates a belt with the provided physical `length` and movement `speed`.
    /// Initially the belt is empty, so the entire length is available as empty space.
    pub fn new(length: u32, speed: u32) -> Self {
        Self {
            length,
            speed,
            items: VecDeque::new(),
            empty_space_front: length,
            empty_space_back: length,
            input_connection: None,
            output_connection: None,
        }
    }

    /// Attaches an input connection to the back of the belt. Passing `None` detaches the
    /// existing connection.
    pub fn set_input_connection(&mut self, connection: Option<BeltConnection>) {
        if let Some(ref conn) = connection {
            assert_eq!(
                conn.kind(),
                BeltConnectionKind::Input,
                "expected an input connection at the belt's tail",
            );
        }

        self.input_connection = connection;
    }

    /// Attaches an output connection to the front of the belt. Passing `None` detaches the
    /// existing connection.
    pub fn set_output_connection(&mut self, connection: Option<BeltConnection>) {
        if let Some(ref conn) = connection {
            assert_eq!(
                conn.kind(),
                BeltConnectionKind::Output,
                "expected an output connection at the belt's head",
            );
        }

        self.output_connection = connection;
    }

    /// Returns an immutable reference to the attached input connection, if any.
    pub fn input_connection(&self) -> Option<&BeltConnection> {
        self.input_connection.as_ref()
    }

    /// Returns a mutable reference to the attached input connection, if any.
    pub fn input_connection_mut(&mut self) -> Option<&mut BeltConnection> {
        self.input_connection.as_mut()
    }

    /// Returns an immutable reference to the attached output connection, if any.
    pub fn output_connection(&self) -> Option<&BeltConnection> {
        self.output_connection.as_ref()
    }

    /// Returns a mutable reference to the attached output connection, if any.
    pub fn output_connection_mut(&mut self) -> Option<&mut BeltConnection> {
        self.output_connection.as_mut()
    }

    /// Adds an item to the back of the belt without advancing the belt.
    /// Returns `false` if there is no trailing space left for another stack.
    pub fn add_item(&mut self, stack: Stack) -> bool {
        if stack.multiplicity != 1 {
            return false;
        }

        if self.empty_space_back < ITEM_WIDTH {
            return false;
        }

        let mut is_group_head = true;
        let mut group_size = 1;
        match self.items.back_mut() {
            Some(item) => {
                debug_assert_eq!(item.next_item_dist, None);
                debug_assert!(item.is_group_tail);
                let spacing = self.empty_space_back - ITEM_WIDTH;
                if spacing == 0 && item.stack == stack {
                    item.stack.multiplicity += stack.multiplicity;
                    self.empty_space_back = 0;
                    return true;
                }
                item.next_item_dist = Some(spacing);
                // check if we are extending a group
                if spacing == 0 {
                    item.is_group_tail = false;
                    is_group_head = false;
                    group_size = item.group_size + 1;

                    // now update the group head's record of group size
                    // We work backwards from the previous tail (current len - group_size) to reach the group head.
                    // Adding 1 accounts for the item we just pushed to the queue.
                    let group_head_index = 1 + self.items.len() - group_size as usize;
                    self.items[group_head_index].group_size = group_size;
                }
                self.empty_space_back = 0;
            }
            None => {
                debug_assert!(self.empty_space_front >= ITEM_WIDTH);
                self.empty_space_front -= ITEM_WIDTH;
                self.empty_space_back = 0;
            }
        }

        self.items.push_back(BeltItem {
            stack,
            next_item_dist: None,
            group_size,
            is_group_head,
            is_group_tail: true,
        });

        true
    }

    /// Removes and returns the next item that reached the front without simulating belt movement.
    /// The call fails with `None` if the belt currently has leading empty space and no stack at the head.
    pub fn remove_item(&mut self) -> Option<Stack> {
        if self.empty_space_front > 0 {
            return None;
        }

        let front_item = self.items.front_mut()?;
        debug_assert!(front_item.is_group_head);
        debug_assert!(front_item.stack.multiplicity > 0);
        let mut stack = front_item.stack.clone();
        stack.multiplicity = 1;
        front_item.stack.multiplicity -= 1;
        self.empty_space_front = ITEM_WIDTH;
        if front_item.stack.multiplicity == 0 {
            self.pop_front_entry(true).unwrap();
        }
        Some(stack)
    }

    /// Advances the belt by `ticks` and returns every stack that would leave the belt in that time.
    /// This consumes the simulated distance by first closing front gaps and then popping
    /// complete items.
    /// TODO: Is this useful? I'm not sure what the API should be for pushing / pulling bulk items. (multiple ticks)
    pub fn remove_while_run(
        &mut self,
        ticks: u32,
        items_filter: Option<&[ItemType]>,
        total_items_limit: Option<u32>,
    ) -> Vec<Stack> {
        let mut distance_to_move = ticks * self.speed;
        let mut removed_items = Vec::new();

        let mut total_removed: u32 = 0;
        // Consume the run distance by first skipping empty front space, then pulling full items.
        while distance_to_move > 0 {
            if self.empty_space_front > 0 {
                if distance_to_move < self.empty_space_front {
                    self.empty_space_front -= distance_to_move;
                    self.empty_space_back += distance_to_move;
                    break;
                }

                distance_to_move -= self.empty_space_front;
                self.empty_space_back += self.empty_space_front;
                self.empty_space_front = 0;
                continue;
            }

            let Some(front_snapshot) = self.items.front() else {
                break;
            };

            if let Some(filter) = items_filter
                && !filter.contains(&front_snapshot.stack.item_type)
            {
                break;
            }

            let multiplicity = front_snapshot.stack.multiplicity;
            debug_assert!(multiplicity > 0);
            let max_by_distance = distance_to_move / ITEM_WIDTH;
            if max_by_distance == 0 {
                self.empty_space_front = distance_to_move;
                self.empty_space_back += distance_to_move;
                break;
            }

            let removable = max_by_distance.min(multiplicity);
            let mut stack = front_snapshot.stack.clone();
            stack.multiplicity = removable;

            removed_items.push(stack);
            distance_to_move -= removable * ITEM_WIDTH;
            self.empty_space_back += removable * ITEM_WIDTH;

            if removable < multiplicity {
                if let Some(front_item) = self.items.front_mut() {
                    front_item.stack.multiplicity -= removable;
                }
                if distance_to_move > 0 {
                    self.empty_space_front = distance_to_move;
                    self.empty_space_back += distance_to_move;
                    distance_to_move = 0;
                } else {
                    self.empty_space_front = 0;
                }
            } else if let Some(removed_item) = self.pop_front_entry(true) {
                self.empty_space_front = match removed_item.next_item_dist {
                    Some(offset) => offset,
                    None => self.length,
                };
            }

            // respect the total items limit if provided
            if let Some(limit) = total_items_limit {
                total_removed += removable;
                if total_removed >= limit {
                    break;
                }
            }
        }

        removed_items
    }

    fn pop_front_entry(&mut self, update_back_space: bool) -> Option<BeltItem> {
        let item = self.items.pop_front()?;

        if item.group_size > 1
            && let Some(next_item) = self.items.front_mut()
        {
            // Promotion logic: the next physical item becomes the new group head and inherits the shrunk group size.
            next_item.is_group_head = true;
            next_item.group_size = item.group_size - 1;
        }

        self.empty_space_front = match item.next_item_dist {
            Some(offset) => offset + ITEM_WIDTH,
            None => self.length,
        };

        if update_back_space && self.items.is_empty() {
            debug_assert!(self.empty_space_back <= self.length);
            self.empty_space_back = self.length;
        }

        Some(item)
    }

    /// Runs the belt forward for `ticks`, compacting item groups while coordinating with
    /// attached connections. Returns `None` to mirror other APIs while updating internal state.
    pub fn run(&mut self, ticks: u32) -> Option<()> {
        // The main belt update loop has three phases:
        //   1. Hand the front of the belt to the output connection while distance and output connection allows.
        //   2. Advance any remaining belt distance locally, merging adjacent groups.
        //   3. Feed new stacks from the input connection into the space that opened up.
        let total_distance = ticks * self.speed;

        let mut distance_remaining = total_distance;
        let mut output_connection = self.output_connection.take();

        if let Some(connection) = output_connection.as_mut() {
            let (consumed, blocked) = self.drain_to_output(distance_remaining, connection);
            distance_remaining = distance_remaining.saturating_sub(consumed);

            if blocked && distance_remaining > 0 {
                self.advance_without_connections(distance_remaining);
                distance_remaining = 0;
            }
        }

        if distance_remaining > 0 {
            self.advance_without_connections(distance_remaining);
        }

        self.output_connection = output_connection;

        let total_back_space = self.empty_space_back;
        self.empty_space_back = 0;

        self.apply_input_connection(total_back_space);

        None
    }

    fn drain_to_output(
        &mut self,
        mut distance_to_move: u32,
        connection: &mut BeltConnection,
    ) -> (u32, bool) {
        let mut consumed = 0u32;
        let mut blocked = false;

        loop {
            // Phase 1: spend movement closing any leading gap before we can present a stack.
            if self.empty_space_front > 0 && distance_to_move > 0 {
                if distance_to_move < self.empty_space_front {
                    self.empty_space_front -= distance_to_move;
                    self.empty_space_back += distance_to_move;
                    consumed += distance_to_move;
                    break;
                }

                distance_to_move -= self.empty_space_front;
                self.empty_space_back += self.empty_space_front;
                consumed += self.empty_space_front;
                self.empty_space_front = 0;
                continue;
            }

            let Some(front_snapshot) = self.items.front() else {
                break;
            };

            let multiplicity = front_snapshot.stack.multiplicity;
            let mut stack = Stack {
                item_type: front_snapshot.stack.item_type,
                item_count: front_snapshot.stack.item_count,
                multiplicity: 1,
            };

            let allow_immediate = distance_to_move == 0 && self.empty_space_front == 0;
            let max_by_distance = if allow_immediate {
                multiplicity
            } else {
                distance_to_move / ITEM_WIDTH
            };

            if max_by_distance == 0 && !allow_immediate {
                self.empty_space_front = distance_to_move;
                self.empty_space_back += distance_to_move;
                consumed += distance_to_move;
                break;
            }

            // Phase 2: ask the output connection how many stacks it can accept right now.
            let max_by_connection = connection.max_acceptable_stacks(&stack);
            if max_by_connection == 0 {
                blocked = true;
                break;
            }

            let removable = if allow_immediate {
                multiplicity.min(max_by_connection)
            } else {
                max_by_distance.min(multiplicity).min(max_by_connection)
            };

            if removable == 0 {
                blocked = true;
                break;
            }

            stack.multiplicity = removable;
            let accepted = connection.accept_stack(stack);
            debug_assert!(
                accepted,
                "connection rejected stack batch after capacity check"
            );
            if !accepted {
                blocked = true;
                break;
            }

            let moved = removable * ITEM_WIDTH;
            self.empty_space_back += moved;
            consumed += moved;
            distance_to_move = distance_to_move.saturating_sub(moved);

            if removable < multiplicity {
                if let Some(front_item) = self.items.front_mut() {
                    front_item.stack.multiplicity -= removable;
                }

                // When distance remains after partially removing a stack, the leftover
                // distance turns into a leading gap that will be consumed.
                if distance_to_move > 0 {
                    self.empty_space_front = distance_to_move;
                    self.empty_space_back += distance_to_move;
                    consumed += distance_to_move;
                }

                break;
            }

            self.pop_front_entry(false);
            if self.items.is_empty() {
                self.empty_space_back = self.length;
            }
        }

        (consumed, blocked)
    }

    fn advance_without_connections(&mut self, mut distance_to_move: u32) {
        // This helper models belt motion when neither input nor output connections participate.
        // It consumes leading empty space first, then repeatedly merges the head group with the
        // next group when they become adjacent. This mirrors the physical behavior where stacks
        // of the same item type "stick" together while different types simply collapse spacing.
        if distance_to_move == 0 || self.is_empty() {
            return;
        }

        // First, burn any empty slots at the belt head. No stacks move until the gap is gone.
        if distance_to_move <= self.empty_space_front {
            self.empty_space_front -= distance_to_move;
            self.empty_space_back += distance_to_move;
            return;
        }

        distance_to_move -= self.empty_space_front;
        self.empty_space_back += self.empty_space_front;
        self.empty_space_front = 0;

        if self.items.is_empty() {
            return;
        }

        let group_start = 0usize;

        while distance_to_move > 0 && group_start < self.items.len() {
            let group_size = self.items[group_start].group_size;
            let group_tail_index = group_start + (group_size as usize - 1);

            // The tail tells us how far the next physical group sits from the head group.
            let distance_to_next = match self.items[group_tail_index].next_item_dist {
                Some(dist) => dist,
                None => break,
            };

            if distance_to_next > distance_to_move {
                if let Some(tail) = self.items.get_mut(group_tail_index) {
                    tail.next_item_dist = Some(distance_to_next - distance_to_move);
                }
                self.empty_space_back += distance_to_move;
                break;
            }

            distance_to_move -= distance_to_next;
            self.empty_space_back += distance_to_next;

            let next_group_start = group_tail_index + 1;
            if next_group_start >= self.items.len() {
                if let Some(tail) = self.items.get_mut(group_tail_index) {
                    tail.next_item_dist = None;
                }
                break;
            }

            let next_group_size = self.items[next_group_start].group_size;
            let next_group_tail = next_group_start + (next_group_size as usize - 1);
            let tail_next_dist = self.items[next_group_tail].next_item_dist;

            let should_merge =
                self.items[group_tail_index].stack == self.items[next_group_start].stack;

            if should_merge {
                // Same stack type: fold the next group into the existing multiplicity.
                let addition = self.items[next_group_start].stack.multiplicity;
                if let Some(tail) = self.items.get_mut(group_tail_index) {
                    tail.stack.multiplicity += addition;
                }

                let remaining = next_group_size - 1;
                self.items.remove(next_group_start);

                if remaining == 0 {
                    // Entire neighbor collapsed into the head stack; rewrite metadata to
                    // describe the new single group that now spans the gap we just closed.
                    for idx in group_start..=group_tail_index {
                        let item = &mut self.items[idx];
                        item.group_size = group_size;
                        item.is_group_head = idx == group_start;
                        item.is_group_tail = idx == group_tail_index;
                        if idx < group_tail_index {
                            item.next_item_dist = Some(0);
                        } else {
                            item.next_item_dist = tail_next_dist;
                        }
                    }
                } else {
                    let new_tail_index = next_group_start + remaining as usize - 1;
                    let new_group_size = group_size + remaining;
                    for idx in group_start..=new_tail_index {
                        // The first item grows to include the surviving trailing stacks, while
                        // the trailing items remain individual belt slots.
                        let item = &mut self.items[idx];
                        item.group_size = new_group_size;
                        item.is_group_head = idx == group_start;
                        item.is_group_tail = idx == new_tail_index;
                        if idx < new_tail_index {
                            item.next_item_dist = Some(0);
                        } else {
                            item.next_item_dist = tail_next_dist;
                        }
                    }
                }
            } else {
                let new_tail_index = next_group_tail;
                let new_group_size = group_size + next_group_size;
                for idx in group_start..=new_tail_index {
                    // Different stack types: we only shrink the distance between the two groups.
                    // Every slot now belongs to one larger group so the head continues marching.
                    let item = &mut self.items[idx];
                    item.group_size = new_group_size;
                    item.is_group_head = idx == group_start;
                    item.is_group_tail = idx == new_tail_index;
                    if idx < new_tail_index {
                        item.next_item_dist = Some(0);
                    } else {
                        item.next_item_dist = tail_next_dist;
                    }
                }
            }
        }
    }

    fn apply_input_connection(&mut self, total_space: u32) {
        let mut input_connection = self.input_connection.take();

        if let Some(connection) = input_connection.as_mut() {
            let available_slots = total_space / ITEM_WIDTH;
            let leftover_units = total_space % ITEM_WIDTH;

            let mut leftover_space = leftover_units;
            if available_slots > 0 {
                // Pull a batch from the connection sized to the free slots; any unused slots
                // convert back into empty trailing space.
                if let Some(batch) = connection.take_output_batch(available_slots) {
                    let used_slots = batch.num_stacks();
                    self.append_output_batch(batch);
                    let unused_slots = available_slots.saturating_sub(used_slots);
                    leftover_space += unused_slots * ITEM_WIDTH;
                } else {
                    leftover_space += available_slots * ITEM_WIDTH;
                }
            }

            self.empty_space_back += leftover_space;
        } else {
            self.empty_space_back += total_space;
        }

        self.input_connection = input_connection;
    }

    fn append_output_batch(&mut self, batch: OutputBatch) {
        if let Some(full_stack) = batch.full_stack {
            // Full stacks match the belt slot granularity exactly.
            self.append_stack_from_connection(full_stack);
        }

        if let Some(partial_stack) = batch.partial_stack {
            // Partial stacks represent leftover items that occupy the final slot.
            self.append_stack_from_connection(partial_stack);
        }
    }

    fn append_stack_from_connection(&mut self, stack: Stack) {
        if self.items.is_empty() {
            // Empty belt: drop the incoming stack directly at the head position.
            let occupied = stack.multiplicity * ITEM_WIDTH;
            self.empty_space_front = self.empty_space_front.saturating_sub(occupied);
            self.items.push_back(BeltItem {
                stack,
                next_item_dist: None,
                group_size: 1,
                is_group_head: true,
                is_group_tail: true,
            });
            return;
        }

        let tail_group_size = self.items.back().map(|item| item.group_size).unwrap_or(1);

        if let Some(tail) = self.items.back_mut() {
            // Keep the existing group contiguous; this new stack slides in immediately after.
            tail.next_item_dist = Some(0);
            tail.is_group_tail = false;
        }

        let group_head_index = self.items.len() - tail_group_size as usize;
        let new_group_size = tail_group_size + 1;
        for idx in group_head_index..self.items.len() {
            let item = &mut self.items[idx];
            item.group_size = new_group_size;
            item.is_group_tail = false;
            item.is_group_head = idx == group_head_index;
        }

        self.items.push_back(BeltItem {
            stack,
            next_item_dist: None,
            group_size: new_group_size,
            is_group_head: false,
            is_group_tail: true,
        });
    }

    /// Returns `true` when the belt contains no stacks.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns the number of stack entries currently tracked on the belt.
    pub fn item_count(&self) -> usize {
        self.items
            .iter()
            .map(|item| item.stack.multiplicity as usize)
            .sum()
    }

    #[cfg(debug_assertions)]
    /// Verifies the internal invariants of the belt, panicking in debug builds when something is inconsistent.
    pub fn sanity_check(&self) {
        debug_assert!(self.empty_space_front <= self.length);
        debug_assert!(self.empty_space_back <= self.length);
        let occupied_length = self
            .items
            .iter()
            .fold(0u32, |acc, item| acc + item.stack.multiplicity * ITEM_WIDTH);
        debug_assert!(occupied_length <= self.length);

        if self.items.is_empty() {
            debug_assert_eq!(self.empty_space_front, self.length);
            debug_assert_eq!(self.empty_space_back, self.length);
            return;
        }

        debug_assert!(self.empty_space_front + self.empty_space_back <= self.length);

        let mut cur_pos = self.empty_space_front;
        for item in self.items.iter() {
            cur_pos += item.stack.multiplicity * ITEM_WIDTH;
            if let Some(distance) = item.next_item_dist {
                cur_pos += distance;
            } else {
                debug_assert_eq!(self.length - cur_pos, self.empty_space_back);
            }
            debug_assert!(cur_pos <= self.length);
        }

        debug_assert_eq!(cur_pos + self.empty_space_back, self.length);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_stack(id: u16) -> Stack {
        Stack::new(id, 1)
    }

    fn belt_with_slots(slots: u32, speed: u32) -> Belt {
        Belt::new(slots * ITEM_WIDTH, speed)
    }

    fn slot_distance(slots: u32) -> u32 {
        slots * ITEM_WIDTH
    }

    fn ticks_for_distance(belt: &Belt, distance: u32) -> u32 {
        if distance == 0 {
            0
        } else {
            (distance + belt.speed - 1) / belt.speed
        }
    }

    fn run_distance(belt: &mut Belt, distance: u32) {
        let ticks = ticks_for_distance(belt, distance);
        if ticks > 0 {
            belt.run(ticks);
        }
    }

    #[test]
    fn add_run_remove_single_item() {
        let mut belt = belt_with_slots(5, 1);
        // Start: empty length-5 belt (speed 1) awaiting a single stack insertion.

        assert!(belt.add_item(sample_stack(42)));
        belt.sanity_check();

        assert_eq!(belt.item_count(), 1);
        assert_eq!(belt.empty_space_front, belt.length - ITEM_WIDTH);
        assert_eq!(belt.empty_space_back, 0);

        let head = belt.items.front().expect("item present");
        assert_eq!(head.stack, sample_stack(42));
        assert!(head.is_group_head);
        assert!(head.is_group_tail);
        assert_eq!(head.group_size, 1);
        assert_eq!(head.next_item_dist, None);

        let steps_to_front = belt.length - ITEM_WIDTH;
        run_distance(&mut belt, steps_to_front);
        belt.sanity_check();
        assert_eq!(belt.empty_space_front, 0);

        let removed = belt.remove_item();
        assert_eq!(removed, Some(sample_stack(42)));
        assert!(belt.is_empty());
        assert_eq!(belt.item_count(), 0);
        assert_eq!(belt.empty_space_front, belt.length);
        assert_eq!(belt.empty_space_back, belt.length);
    }

    #[test]
    fn add_item_rejects_multiplicity_stacks() {
        let mut belt = belt_with_slots(3, 1);
        let mut stack = sample_stack(7);
        stack.multiplicity = 2;

        assert!(!belt.add_item(stack));
        assert!(belt.is_empty());
        assert_eq!(belt.empty_space_front, belt.length);
        assert_eq!(belt.empty_space_back, belt.length);
    }

    #[test]
    fn input_connection_feeds_belt() {
        let mut belt = belt_with_slots(5, ITEM_WIDTH);
        let mut connection = BeltConnection::new(BeltConnectionKind::Input, 10, 3, None);

        assert!(connection.accept_stack(Stack::new(42, 6)));
        belt.set_input_connection(Some(connection));

        belt.run(1);

        assert_eq!(belt.item_count(), 2);
        let head = belt.items.front().expect("expected head stack");
        assert_eq!(head.stack.item_type, 42);
        assert_eq!(head.stack.item_count, 3);
        assert_eq!(head.stack.multiplicity, 2);
        assert!(belt.input_connection().unwrap().is_empty());
    }

    #[test]
    fn output_connection_respects_item_limit() {
        let mut belt = belt_with_slots(5, ITEM_WIDTH);
        let stack = Stack::new(7, 2);

        assert!(belt.add_item(stack.clone()));
        run_distance(&mut belt, slot_distance(1));
        assert!(belt.add_item(stack.clone()));

        let to_front = belt.empty_space_front;
        run_distance(&mut belt, to_front);
        assert_eq!(belt.empty_space_front, 0);

        let connection = BeltConnection::new(BeltConnectionKind::Output, 3, 2, None);
        belt.set_output_connection(Some(connection));

        belt.run(0);
        assert_eq!(
            belt.output_connection()
                .expect("output connection attached")
                .buffered_item_count(),
            2
        );
        assert_eq!(belt.item_count(), 1);

        let to_front = belt.empty_space_front;
        run_distance(&mut belt, to_front);
        assert_eq!(
            belt.output_connection()
                .expect("output connection attached")
                .buffered_item_count(),
            2
        );
        assert_eq!(belt.item_count(), 1);
    }

    #[test]
    fn multiple_items_progress_individually() {
        let mut belt = belt_with_slots(6, 1);
        // Start: empty length-6 belt (speed 1) before adding staggered unique stacks.

        assert!(belt.add_item(sample_stack(1)));
        belt.sanity_check();

        // move two ticks - need to create space to not group items
        run_distance(&mut belt, slot_distance(2));
        belt.sanity_check();
        assert!(
            belt.empty_space_back > 0,
            "expected trailing space after moving existing items"
        );

        assert!(belt.add_item(sample_stack(2)));
        belt.sanity_check();

        let to_front = belt.empty_space_front;
        run_distance(&mut belt, to_front);
        belt.sanity_check();

        let first = belt.remove_item().expect("first item available");
        assert_eq!(first, sample_stack(1));

        assert_eq!(belt.item_count(), 1);
        let head = belt.items.front().unwrap();
        assert_eq!(head.stack, sample_stack(2));
        assert!(head.is_group_head);

        let to_front = belt.empty_space_front;
        run_distance(&mut belt, to_front);
        let second = belt.remove_item().expect("second item available");
        assert_eq!(second, sample_stack(2));
        assert!(belt.is_empty());
    }

    #[test]
    fn multiple_items_progress_grouped_creation() {
        let mut belt = belt_with_slots(6, 1);
        // Start: empty length-6 belt (speed 1) then add stacks close enough to form a group.

        assert!(belt.add_item(sample_stack(1)));
        belt.sanity_check();

        // move one stack width - this should group them together
        run_distance(&mut belt, slot_distance(1));
        belt.sanity_check();
        assert!(
            belt.empty_space_back > 0,
            "expected trailing space after moving existing items"
        );

        assert!(belt.add_item(sample_stack(2)));
        belt.sanity_check();

        let to_front = belt.empty_space_front;
        run_distance(&mut belt, to_front);
        belt.sanity_check();

        assert!(belt.items[0].is_group_head);
        assert!(!belt.items[0].is_group_tail);
        assert!(!belt.items[1].is_group_head);
        assert!(belt.items[1].is_group_tail);
        assert_eq!(belt.items[0].group_size, 2);
        assert_eq!(belt.items[1].group_size, 2);

        let first = belt.remove_item().expect("first item available");
        assert_eq!(first, sample_stack(1));

        assert_eq!(belt.item_count(), 1);
        let head = belt.items.front().unwrap();
        assert_eq!(head.stack, sample_stack(2));
        assert!(head.is_group_head);

        // no item to remove, should fail
        assert_eq!(belt.remove_item(), None);

        let to_front = belt.empty_space_front;
        run_distance(&mut belt, to_front);
        let second = belt.remove_item().expect("second item available");
        assert_eq!(second, sample_stack(2));
        assert!(belt.is_empty());
    }

    #[test]
    fn fast_belt_moves_in_chunks() {
        let mut belt = belt_with_slots(20, 7);
        // Start: empty length-20 belt (speed 7) to observe large movement quanta.

        assert!(belt.add_item(sample_stack(11)));
        run_distance(&mut belt, slot_distance(1));
        #[cfg(debug_assertions)]
        belt.sanity_check();

        assert!(belt.add_item(sample_stack(13)));
        #[cfg(debug_assertions)]
        belt.sanity_check();

        assert_eq!(belt.item_count(), 2);

        let gap_to_front = belt.empty_space_front;
        run_distance(&mut belt, gap_to_front);
        #[cfg(debug_assertions)]
        belt.sanity_check();

        let first = belt.remove_item();
        assert_eq!(first, Some(sample_stack(11)));

        let gap_to_front = belt.empty_space_front;
        run_distance(&mut belt, gap_to_front);
        #[cfg(debug_assertions)]
        belt.sanity_check();

        let second = belt.remove_item();
        assert_eq!(second, Some(sample_stack(13)));
        assert!(belt.is_empty());
    }

    #[test]
    fn near_full_belt_capacity_behavior() {
        let mut belt = belt_with_slots(5, 1);
        // Start: empty length-5 belt (speed 1) that we quickly pack to the brim.

        assert!(belt.add_item(sample_stack(1)));
        run_distance(&mut belt, slot_distance(1));
        assert!(belt.add_item(sample_stack(2)));
        run_distance(&mut belt, slot_distance(1));
        assert!(belt.add_item(sample_stack(3)));
        run_distance(&mut belt, slot_distance(1));
        assert!(belt.add_item(sample_stack(4)));

        assert_eq!(belt.item_count(), 4);
        assert_eq!(belt.empty_space_front, slot_distance(1));
        assert_eq!(belt.empty_space_back, 0);
        assert!(
            !belt.add_item(sample_stack(99)),
            "belt with no trailing space should refuse new items"
        );

        run_distance(&mut belt, slot_distance(1));
        assert_eq!(belt.empty_space_front, 0);
        assert_eq!(belt.empty_space_back, slot_distance(1));

        let removed = belt.remove_item().expect("front item to remove");
        assert_eq!(removed, sample_stack(1));
        assert_eq!(belt.item_count(), 3);

        // Create additional trailing space so the next insertion does not extend the existing group.
        run_distance(&mut belt, slot_distance(1));
        assert_eq!(belt.empty_space_front, 0);
        assert!(belt.empty_space_back > slot_distance(1));

        assert!(
            belt.add_item(sample_stack(42)),
            "removing from near-full belt should make room for a new item"
        );
        assert_eq!(belt.item_count(), 4);
    }

    #[test]
    fn half_full_belt_gap_propagation_and_compaction() {
        let mut belt = belt_with_slots(12, 1);
        // Start: empty length-12 belt (speed 1); build interleaved groups and gaps to compact later.

        // add a group of two
        assert!(belt.add_item(sample_stack(1)));
        run_distance(&mut belt, slot_distance(1));
        assert!(belt.add_item(sample_stack(2)));

        // add individual item two spaces away
        run_distance(&mut belt, slot_distance(3));
        assert!(belt.add_item(sample_stack(3)));

        // add a group of two, separated from the previous item by 1 space
        run_distance(&mut belt, slot_distance(2));
        assert!(belt.add_item(sample_stack(4)));
        run_distance(&mut belt, slot_distance(1));
        assert!(belt.add_item(sample_stack(5)));
        run_distance(&mut belt, slot_distance(4));
        assert_eq!(belt.empty_space_front, 0);
        assert!(
            belt.empty_space_back >= slot_distance(4),
            "expect trailing space before adding the final item"
        );
        // final individual item, three spaces away
        assert!(belt.add_item(sample_stack(6)));

        assert_eq!(belt.item_count(), 6);
        assert_eq!(belt.empty_space_back, 0);

        // Validate the initial pattern has mixed groups and gaps.
        // as before, group of two
        assert_eq!(belt.items[0].stack, sample_stack(1));
        assert!(belt.items[0].is_group_head);
        assert_eq!(belt.items[0].group_size, 2);
        assert_eq!(belt.items[1].stack, sample_stack(2));
        assert!(belt.items[1].is_group_tail);
        assert_eq!(belt.items[1].next_item_dist, Some(slot_distance(2)));

        // then a lone item, two spaces away
        assert_eq!(belt.items[2].stack, sample_stack(3));
        assert!(belt.items[2].is_group_head);
        assert_eq!(belt.items[2].group_size, 1);
        assert_eq!(belt.items[2].next_item_dist, Some(slot_distance(1)));

        // then a group of two, one space away
        assert_eq!(belt.items[3].stack, sample_stack(4));
        assert!(belt.items[3].is_group_head);
        assert_eq!(belt.items[3].group_size, 2);
        assert_eq!(belt.items[4].stack, sample_stack(5));
        assert!(belt.items[4].is_group_tail);
        assert_eq!(belt.items[4].next_item_dist, Some(slot_distance(3)));

        // final item, three spaces away
        assert_eq!(belt.items[5].stack, sample_stack(6));
        assert!(belt.items[5].is_group_head);
        assert_eq!(belt.items[5].group_size, 1);

        // drain the first two items (two ticks should do it)
        let drained = belt.remove_while_run(slot_distance(2), None, None);
        assert_eq!(drained, vec![sample_stack(1), sample_stack(2)]);
        assert_eq!(belt.empty_space_front, slot_distance(2));
        assert_eq!(belt.items[0].stack, sample_stack(3));
        assert!(belt.items[0].is_group_head);
        assert_eq!(belt.items[0].group_size, 1);
        assert_eq!(
            belt.items[0].next_item_dist,
            Some(slot_distance(1)),
            "gap between first remaining item and next group should have propagated without compaction"
        );
        assert_eq!(belt.item_count(), 4);

        // now squeeze the residual 4 items into one group
        belt.run(belt.length);
        assert_eq!(belt.item_count(), 4);
        assert_eq!(belt.empty_space_front, 0);
        assert_eq!(belt.items[0].group_size, 4);
        assert!(belt.items[0].is_group_head);
        assert!(!belt.items[3].is_group_head);
        assert!(belt.items[3].is_group_tail);
        assert_eq!(belt.items[3].next_item_dist, None);
        assert_eq!(belt.empty_space_back, slot_distance(8));
    }

    #[test]
    fn identical_items_merge_into_multiplicity() {
        let mut belt = belt_with_slots(6, 1);
        // Start: empty length-6 belt (speed 1) with two identical stacks that drift together.
        let stack = sample_stack(99);

        assert!(belt.add_item(stack.clone()));
        run_distance(&mut belt, slot_distance(2));
        assert!(belt.add_item(stack.clone()));

        // Compact the belt so the two stacks meet and merge.
        belt.run(belt.length);
        #[cfg(debug_assertions)]
        belt.sanity_check();

        assert_eq!(belt.items.len(), 1);
        let head = belt.items.front().unwrap();
        assert_eq!(head.stack.multiplicity, 2);
        assert_eq!(belt.item_count(), 2);

        // Removing the first stack should leave a gap at the front and reduce multiplicity.
        let removed_first = belt.remove_item().expect("expected first identical stack");
        assert_eq!(removed_first, stack);
        assert_eq!(belt.items.front().unwrap().stack.multiplicity, 1);
        assert_eq!(belt.empty_space_front, slot_distance(1));

        // Advance the belt to close the front gap, then remove the remaining stack.
        let gap_to_front = belt.empty_space_front;
        run_distance(&mut belt, gap_to_front);
        #[cfg(debug_assertions)]
        belt.sanity_check();
        let removed_second = belt.remove_item().expect("expected second identical stack");
        assert_eq!(removed_second, stack);
        assert!(belt.is_empty());
        assert_eq!(belt.item_count(), 0);
    }

    #[test]
    fn remove_items_partially_consumes_multiplicity() {
        let mut belt = belt_with_slots(8, 1);
        // Start: empty length-8 belt (speed 1) where a duplicated stack will only be partially removed.
        let stack = sample_stack(77);

        assert!(belt.add_item(stack.clone()));
        run_distance(&mut belt, slot_distance(1));
        assert!(belt.add_item(stack.clone()));

        // Compact and bring the merged stack to the front.
        belt.run(belt.length);
        let to_front = belt.empty_space_front;
        if to_front > 0 {
            run_distance(&mut belt, to_front);
        }

        let head = belt.items.front().expect("expected merged head");
        assert_eq!(head.stack.multiplicity, 2);
        assert_eq!(belt.empty_space_front, 0);

        let prior_back = belt.empty_space_back;
        let removed = belt.remove_while_run(slot_distance(1), None, None);
        assert_eq!(removed, vec![stack.clone()]);

        let head = belt.items.front().expect("expected remaining stack");
        assert_eq!(head.stack.multiplicity, 1);
        assert_eq!(belt.empty_space_front, 0);
        assert_eq!(belt.empty_space_back, prior_back + slot_distance(1));
    }

    #[test]
    fn remove_items_consumes_entire_multiplicity_stack() {
        let mut belt = belt_with_slots(10, 1);
        // Start: empty length-10 belt (speed 1) with two identical stacks followed by a distinct one.
        let stack_a = sample_stack(55);
        let stack_b = sample_stack(56);

        assert!(belt.add_item(stack_a.clone()));
        run_distance(&mut belt, slot_distance(1));
        assert!(belt.add_item(stack_a.clone()));
        run_distance(&mut belt, slot_distance(3));
        assert!(belt.add_item(stack_b.clone()));

        // Merge identical stacks and position them at the front.
        belt.run(belt.length);
        let to_front = belt.empty_space_front;
        if to_front > 0 {
            run_distance(&mut belt, to_front);
        }

        let head = belt.items.front().expect("expected merged head");
        assert_eq!(head.stack.multiplicity, 2);
        assert_eq!(belt.item_count(), 3);

        let prior_back = belt.empty_space_back;
        let removed = belt.remove_while_run(slot_distance(2), None, None);
        let mut expected_removed = stack_a.clone();
        expected_removed.multiplicity = 2;
        assert_eq!(removed, vec![expected_removed]);

        let next = belt.items.front().expect("expected trailing stack");
        assert_eq!(next.stack, stack_b);
        assert!(next.is_group_head);
        assert_eq!(belt.empty_space_back, prior_back + slot_distance(2));
        assert_eq!(belt.item_count(), 1);
    }

    #[test]
    fn separated_identical_items_merge_into_single_entry() {
        let mut belt = belt_with_slots(12, 1);
        // Start: empty length-12 belt (speed 1); insert identical stacks with gaps to confirm full merge.
        let stack = sample_stack(88);

        assert!(belt.add_item(stack.clone()));
        run_distance(&mut belt, slot_distance(2));
        assert!(belt.add_item(stack.clone()));
        run_distance(&mut belt, slot_distance(3));
        assert!(belt.add_item(stack.clone()));

        // The identical stacks start life as independent entries with gaps between them.
        assert_eq!(belt.items.len(), 3);
        assert!(belt.items.iter().all(|item| item.stack.multiplicity == 1));
        assert!(belt.items.iter().any(|item| item.next_item_dist.is_some()));

        // Compact the belt so that the three stacks meet and merge into one multiplicity group.
        belt.run(belt.length);
        let to_front = belt.empty_space_front;
        if to_front > 0 {
            run_distance(&mut belt, to_front);
        }

        #[cfg(debug_assertions)]
        belt.sanity_check();

        assert_eq!(belt.items.len(), 1);
        let head = belt.items.front().expect("expected merged stack");
        assert_eq!(head.stack.multiplicity, 3);
        assert_eq!(head.group_size, 1);
        assert!(head.is_group_head);
        assert!(head.is_group_tail);
        assert_eq!(belt.item_count(), 3);
    }

    #[test]
    fn separated_identical_group_merges_with_trailing_items() {
        let mut belt = belt_with_slots(14, 1);
        // Start: empty length-14 belt (speed 1) with spaced identical stacks and a distinct trailer.
        let stack_identical = sample_stack(91);
        let stack_other = sample_stack(92);

        assert!(belt.add_item(stack_identical.clone()));
        run_distance(&mut belt, slot_distance(2));
        assert!(belt.add_item(stack_identical.clone()));
        run_distance(&mut belt, slot_distance(3));
        assert!(belt.add_item(stack_identical.clone()));
        run_distance(&mut belt, slot_distance(4));
        assert!(belt.add_item(stack_other.clone()));

        // Ensure the identical stacks were inserted as distinct entries and the trailing stack remains separate.
        assert_eq!(belt.items.len(), 4);
        assert_eq!(belt.items.front().unwrap().stack, stack_identical);
        assert_eq!(belt.items.front().unwrap().stack.multiplicity, 1);
        assert_eq!(belt.items[1].stack, stack_identical);
        assert_eq!(belt.items[1].stack.multiplicity, 1);
        assert_eq!(belt.items[2].stack, stack_identical);
        assert_eq!(belt.items[3].stack, stack_other);

        // Compact and merge the separated identical stacks at the front while keeping the trailing stack intact.
        belt.run(belt.length);
        let to_front = belt.empty_space_front;
        if to_front > 0 {
            run_distance(&mut belt, to_front);
        }

        #[cfg(debug_assertions)]
        belt.sanity_check();

        assert_eq!(belt.items.len(), 2);
        let head = belt.items.front().expect("expected merged head");
        assert_eq!(head.stack, stack_identical);
        assert_eq!(head.stack.multiplicity, 3);
        assert_eq!(belt.items[1].stack, stack_other);
        assert_eq!(belt.items[1].stack.multiplicity, 1);
        assert_eq!(belt.item_count(), 4);
    }

    #[test]
    fn gapped_identical_groups_merge_into_three_entries() {
        let mut belt = belt_with_slots(24, 1);
        // Start: empty length-24 belt (speed 1); stage six separated stacks that should collapse into three entries.
        let large_stack = Stack::new(123, 4);
        let small_stack = Stack::new(123, 1);
        // All stacks share item type 123; only their item_count differs.

        // Front block: three size-4 stacks, each separated by one empty slot.
        assert!(belt.add_item(large_stack.clone()));
        run_distance(&mut belt, slot_distance(2));
        assert!(belt.add_item(large_stack.clone()));
        run_distance(&mut belt, slot_distance(2));
        assert!(belt.add_item(large_stack.clone()));

        // Middle block: single size-1 stack with a gap ahead and behind, so it cannot merge by multiplicity.
        run_distance(&mut belt, slot_distance(2));
        assert!(belt.add_item(small_stack.clone()));

        // Tail block: two more size-4 stacks, again buffered by single-slot gaps.
        run_distance(&mut belt, slot_distance(2));
        assert!(belt.add_item(large_stack.clone()));
        run_distance(&mut belt, slot_distance(2));
        assert!(belt.add_item(large_stack.clone()));

        assert_eq!(belt.items.len(), 6);
        assert!(belt.items.iter().all(|item| item.stack.multiplicity == 1));
        for idx in 0..belt.items.len() - 1 {
            let distance = belt.items[idx]
                .next_item_dist
                .expect("expected gaps between initial items");
            assert!(
                distance >= slot_distance(1),
                "expected at least one empty slot between items"
            );
        }

        belt.run(belt.length);
        let to_front = belt.empty_space_front;
        if to_front > 0 {
            run_distance(&mut belt, to_front);
        }

        #[cfg(debug_assertions)]
        belt.sanity_check();

        assert_eq!(belt.items.len(), 3);

        // The first three stacks collapse into a single entry with multiplicity three.
        let front = &belt.items[0];
        assert_eq!(front.stack, large_stack);
        assert_eq!(front.stack.multiplicity, 3);
        assert!(front.is_group_head);
        assert!(!front.is_group_tail);
        assert_eq!(front.next_item_dist, Some(0));

        // The middle singleton remains its own entry but becomes part of the contiguous group.
        let middle = &belt.items[1];
        assert_eq!(middle.stack, small_stack);
        assert_eq!(middle.stack.multiplicity, 1);
        assert!(!middle.is_group_head);
        assert!(!middle.is_group_tail);
        assert_eq!(middle.next_item_dist, Some(0));

        // The final two stacks merge into a multiplicity-two tail that shares the same group.
        let tail = &belt.items[2];
        assert_eq!(tail.stack, large_stack);
        assert_eq!(tail.stack.multiplicity, 2);
        assert!(!tail.is_group_head);
        assert!(tail.is_group_tail);
        assert_eq!(tail.next_item_dist, None);

        // Every entry now shares a single group of length three despite their different multiplicities.
        for item in belt.items.iter() {
            assert_eq!(item.group_size, 3);
        }
        assert_eq!(belt.item_count(), 6);
    }
}
