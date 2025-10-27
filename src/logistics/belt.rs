use crate::logistics::Stack;
use std::collections::VecDeque;

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
    // how many repeats of this item this item represents
    // TODO: implement multiplicity logic
    #[allow(unused)]
    multiplicity: u32,
}

/// Models a Satisfactory-style conveyor belt that primarily supports pushing items on the back
/// and popping them from the front in FIFO order. Random access is intentionally deprioritized
/// because the belt is expected to be consumed from its ends.
pub struct Belt {
    length: u32,
    speed: u32,
    // Consider moving from VecDeque to YCQueue
    items: VecDeque<BeltItem>,
    // how many empty spaces in the queue until we hit a stack
    empty_space_front: u32,
    // how many trailing empty spaces in the belt
    empty_space_back: u32,
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
        }
    }

    /// Adds an item to the back of the belt without advancing the belt.
    /// Returns `false` if there is no trailing space left for another stack.
    pub fn add_item(&mut self, stack: Stack) -> bool {
        if self.empty_space_back == 0 {
            return false;
        }

        let mut is_group_head = true;
        let mut group_size = 1;
        match self.items.back_mut() {
            Some(item) => {
                debug_assert_eq!(item.next_item_dist, None);
                debug_assert!(item.is_group_tail);
                item.next_item_dist = Some(self.empty_space_back - 1);
                // check if we are extending a group
                if self.empty_space_back == 1 {
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
                self.empty_space_front -= 1;
                self.empty_space_back = 0;
            }
        }

        self.items.push_back(BeltItem {
            stack,
            next_item_dist: None,
            group_size,
            is_group_head,
            is_group_tail: true,
            multiplicity: 1,
        });

        true
    }

    /// Removes and returns the next item that reached the front without simulating belt movement.
    /// The call fails with `None` if the belt currently has leading empty space and no stack at the head.
    pub fn remove_item(&mut self) -> Option<Stack> {
        if self.empty_space_front > 0 {
            return None;
        }

        let item = self.items.pop_front()?;
        debug_assert!(item.is_group_head);
        if item.group_size > 1
            && let Some(next_item) = self.items.front_mut()
        {
            // Promotion logic: the next physical item becomes the new group head and inherits the shrunk group size.
            next_item.is_group_head = true;
            next_item.group_size = item.group_size - 1;
        }

        self.empty_space_front = match item.next_item_dist {
            Some(offset) => offset + 1,
            None => self.length,
        };

        if self.items.is_empty() {
            debug_assert_eq!(self.empty_space_back, self.length - 1);
            self.empty_space_back = self.length;
        }

        Some(item.stack)
    }

    /// Advances the belt by `ticks` and returns every stack that would leave the belt in that time.
    /// This consumes the simulated distance by first closing front gaps and then popping
    /// complete items.
    pub fn remove_items(&mut self, ticks: u32) -> Vec<Stack> {
        let mut distance_to_move = ticks * self.speed;
        let mut removed_items = Vec::new();

        // Consume the run distance by first skipping empty front space, then pulling full items.
        while distance_to_move > 0 {
            if distance_to_move < self.empty_space_front {
                self.empty_space_front -= distance_to_move;
                return removed_items;
            }

            // eat the empty space at the front first
            distance_to_move -= self.empty_space_front;
            self.empty_space_front = 0;

            match self.items.pop_front() {
                Some(item) => {
                    removed_items.push(item.stack);
                    // Reset the amount of free space before the new head; if there is no gap the belt is now empty.
                    self.empty_space_front = match item.next_item_dist {
                        Some(offset) => offset + 1,
                        None => self.length,
                    };
                }
                None => break,
            }
        }

        removed_items
    }

    /// Runs the belt forward for `ticks`, compacting item groups until they can no longer move.
    /// Returns `None` to mirror other APIs, but keeps the internal spacing state up to date.
    pub fn run(&mut self, ticks: u32) -> Option<()> {
        // if the belt is full or empty we also can't do anything
        if self.item_count() == self.length as usize || self.is_empty() {
            debug_assert_eq!(self.empty_space_front, 0);
            debug_assert_eq!(self.empty_space_back, 0);
            return None;
        }

        let mut total_distance_to_move = ticks * self.speed;

        // sufficient distance at the front of the belt means everything slides together
        if total_distance_to_move < self.empty_space_front {
            self.empty_space_front -= total_distance_to_move;
            self.empty_space_back += total_distance_to_move;
            return None;
        }

        // eat the empty space at the front first
        total_distance_to_move -= self.empty_space_front;
        self.empty_space_back += self.empty_space_front;
        self.empty_space_front = 0;

        let mut items_mut_iter = self.items.iter_mut();
        /*
         * This loop effectively compacts the belt by "moving" items forward until they stop.
         * Impl: we are shrinking the gaps between groups in series for a total of
         * `total_distance_to_move` units.
         */
        while total_distance_to_move > 0 {
            /*
             * Consume one contiguous group from the iterator; nth(0) gives the head, further nth() calls walk to the tail.
             * This looks weird but:
             * Assuming the iterator returned the head of a group, in order to get to the end of a
             * group, we need to advance the iterator by group_size - 2 because we already consumed
             * the head which adds -1, and then because we need to stop at the tail, so another -1.
             * This makes sense because in the group_size = 2 case, we do just call nth(0), since
             * nth(0) just removed the next item from the iterator.
             */
            let current_group_head = items_mut_iter.nth(0).unwrap();
            let mut current_group_tail = if current_group_head.group_size > 1 {
                let val = items_mut_iter
                    .nth(current_group_head.group_size as usize - 2)
                    .unwrap();
                debug_assert_eq!(current_group_head.group_size, val.group_size);
                debug_assert!(val.is_group_tail);
                Some(val)
            } else {
                None
            };

            let distance_to_next_head = current_group_tail
                .as_deref_mut()
                .unwrap_or(current_group_head)
                // Every tail stores the gap to the next group. If none, there is no next group
                .next_item_dist?;

            // if distance == 0, they should be in the same group
            debug_assert!(distance_to_next_head > 0);

            // if the next group is too far away, ezpz
            if distance_to_next_head > total_distance_to_move {
                current_group_tail
                    .as_deref_mut()
                    .unwrap_or(current_group_head)
                    .next_item_dist = Some(distance_to_next_head - total_distance_to_move);
                self.empty_space_back += total_distance_to_move;
                return None;
            }

            total_distance_to_move -= distance_to_next_head;
            self.empty_space_back += distance_to_next_head;

            // now we have to merge current group and next group
            let next_group_head = items_mut_iter.nth(0).unwrap();
            let next_group_tail = if next_group_head.group_size > 1 {
                let val = items_mut_iter
                    .nth(next_group_head.group_size as usize - 2)
                    .unwrap();
                debug_assert_eq!(next_group_head.group_size, val.group_size);
                debug_assert!(val.is_group_tail);
                Some(val)
            } else {
                None
            };

            // merge current tail and next head
            let new_group_size = current_group_head.group_size + next_group_head.group_size;
            current_group_head.group_size = new_group_size;
            current_group_tail
                .unwrap_or(current_group_head)
                .is_group_tail = false;
            next_group_head.is_group_head = false;
            next_group_tail.unwrap_or(next_group_head).group_size = new_group_size;
        }

        None
    }

    /// Returns `true` when the belt contains no stacks.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns the number of stack entries currently tracked on the belt.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    #[cfg(debug_assertions)]
    /// Verifies the internal invariants of the belt, panicking in debug builds when something is inconsistent.
    pub fn sanity_check(&self) {
        debug_assert!(self.empty_space_front <= self.length);
        debug_assert!(self.empty_space_back <= self.length);
        debug_assert!(self.empty_space_front + self.empty_space_back <= self.length);
        debug_assert!(self.item_count() < self.length as usize);

        let mut cur_pos = self.empty_space_front;
        for item in self.items.iter() {
            debug_assert!(cur_pos + item.next_item_dist.unwrap_or(0) < self.length);
            cur_pos += item.next_item_dist.unwrap_or(0) + 1;
        }

        debug_assert!(cur_pos <= self.length);
        debug_assert!((cur_pos + self.empty_space_back) == self.length);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_stack(id: u16) -> Stack {
        Stack::new(id, 1)
    }

    #[test]
    fn add_run_remove_single_item() {
        let mut belt = Belt::new(5, 1);

        assert!(belt.add_item(sample_stack(42)));
        belt.sanity_check();

        assert_eq!(belt.item_count(), 1);
        assert_eq!(belt.empty_space_front, belt.length - 1);
        assert_eq!(belt.empty_space_back, 0);

        let head = belt.items.front().expect("item present");
        assert_eq!(head.stack, sample_stack(42));
        assert!(head.is_group_head);
        assert!(head.is_group_tail);
        assert_eq!(head.group_size, 1);
        assert_eq!(head.next_item_dist, None);

        let steps_to_front = belt.length - 1;
        belt.run(steps_to_front);
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
    fn multiple_items_progress_individually() {
        let mut belt = Belt::new(6, 1);

        assert!(belt.add_item(sample_stack(1)));
        belt.sanity_check();

        // move two ticks - need to create space to not group items
        belt.run(2);
        belt.sanity_check();
        assert!(
            belt.empty_space_back > 0,
            "expected trailing space after moving existing items"
        );

        assert!(belt.add_item(sample_stack(2)));
        belt.sanity_check();

        let to_front = belt.empty_space_front;
        belt.run(to_front);
        belt.sanity_check();

        let first = belt.remove_item().expect("first item available");
        assert_eq!(first, sample_stack(1));

        assert_eq!(belt.item_count(), 1);
        let head = belt.items.front().unwrap();
        assert_eq!(head.stack, sample_stack(2));
        assert!(head.is_group_head);

        let to_front = belt.empty_space_front;
        belt.run(to_front);
        let second = belt.remove_item().expect("second item available");
        assert_eq!(second, sample_stack(2));
        assert!(belt.is_empty());
    }

    #[test]
    fn multiple_items_progress_grouped_creation() {
        let mut belt = Belt::new(6, 1);

        assert!(belt.add_item(sample_stack(1)));
        belt.sanity_check();

        // move 1 ticks - this should group them together
        belt.run(1);
        belt.sanity_check();
        assert!(
            belt.empty_space_back > 0,
            "expected trailing space after moving existing items"
        );

        assert!(belt.add_item(sample_stack(2)));
        belt.sanity_check();

        let to_front = belt.empty_space_front;
        belt.run(to_front);
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

        let to_front = belt.empty_space_front;
        belt.run(to_front);
        let second = belt.remove_item().expect("second item available");
        assert_eq!(second, sample_stack(2));
        assert!(belt.is_empty());
    }

    #[test]
    fn fast_belt_moves_in_chunks() {
        let mut belt = Belt::new(20, 7);

        assert!(belt.add_item(sample_stack(11)));
        belt.run(1);
        #[cfg(debug_assertions)]
        belt.sanity_check();

        assert!(belt.add_item(sample_stack(13)));
        #[cfg(debug_assertions)]
        belt.sanity_check();

        assert_eq!(belt.item_count(), 2);

        belt.run(2);
        #[cfg(debug_assertions)]
        belt.sanity_check();

        let first = belt.remove_item();
        assert_eq!(first, Some(sample_stack(11)));

        belt.run(1);
        #[cfg(debug_assertions)]
        belt.sanity_check();

        let second = belt.remove_item();
        assert_eq!(second, Some(sample_stack(13)));
        assert!(belt.is_empty());
    }
}
