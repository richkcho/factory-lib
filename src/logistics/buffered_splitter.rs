use std::slice;

use crate::{logistics::BeltConnection, types::ItemType};

/**
 * Represents a splitter that divides incoming item stacks into multiple output belts. Inputs are prioritized
 * from the input belts in order, followed by round-robin distribution among remaining belts. Outputs are filled
 * in a similar manner.
 * TODO: Revisit belt ownership
 */
#[derive(Debug)]
pub struct BufferedSplitter {
    priority_inputs: Vec<BeltConnection>,
    rr_inputs: Vec<BeltConnection>,
    input_rr_index: usize,
    priority_outputs: Vec<BeltConnection>,
    rr_outputs: Vec<BeltConnection>,
    output_rr_index: usize,
}

/// Drains items from the given input connections and distributes them to the output connections based on priority
/// and round-robin strategy. Assumes input connections are all equal priority.
fn drain_connections(
    item_type: ItemType,
    rr_inputs: &mut [BeltConnection],
    input_rr_index: &mut usize,
    priority_outputs: &mut [BeltConnection],
    rr_outputs: &mut [BeltConnection],
    output_rr_index: &mut usize,
) -> Option<()> {
    if rr_inputs.is_empty() {
        return None;
    }

    let item_count: u16 = rr_inputs
        .iter()
        .filter(|c| c.current_item_type() == Some(item_type))
        .map(|c| c.buffered_item_count())
        .sum();
    // distribute items. This does not consume from the inputs, which will be done next.
    let remaining_item_count = distribute_items(
        item_count,
        item_type,
        priority_outputs,
        rr_outputs,
        output_rr_index,
    );

    /*
     * If we ended up yoinking from the rr inputs, we need to "fast forward" the round robin consumption
     * to maintain correctness.
     */
    let mut consumed_item_count = item_count - remaining_item_count;
    while consumed_item_count > 0 {
        let non_empty_inputs = rr_inputs
            .iter()
            .map(|c| c.buffered_item_count())
            .filter(|&count| count > 0);
        let num_non_empty = non_empty_inputs.clone().count() as u16;
        let amount_consumable_per_belt = non_empty_inputs.min().unwrap_or(0);
        if amount_consumable_per_belt == 0 {
            debug_assert_eq!(num_non_empty, 0);
            break;
        }

        let amount_to_take = consumed_item_count.min(amount_consumable_per_belt * num_non_empty);
        let amount_per_belt = amount_to_take / num_non_empty;
        let leftover = amount_to_take % num_non_empty;

        for i in 0..rr_inputs.len() {
            let index = (*input_rr_index + i) % rr_inputs.len();
            if rr_inputs[index].current_item_type() != Some(item_type) {
                continue;
            }

            let to_take = if i < leftover as usize {
                *input_rr_index = (index + 1) % rr_inputs.len();
                amount_per_belt + 1
            } else {
                amount_per_belt
            };
            debug_assert_eq!(rr_inputs[index].dec_item_count(to_take), 0);
        }

        consumed_item_count -= amount_to_take;
    }

    None
}

/// Distributes the given number of items of the specified type to the output connections based on priority
/// and round-robin strategy. Returns the number of items that could not be distributed.
fn distribute_items(
    mut remaining_item_count: u16,
    item_type: ItemType,
    priority_outputs: &mut [BeltConnection],
    rr_outputs: &mut [BeltConnection],
    rr_index: &mut usize,
) -> u16 {
    // first attempt to fill priority outputs in order
    for output in priority_outputs.iter_mut() {
        remaining_item_count = output.inc_item_count(item_type, remaining_item_count);
        if remaining_item_count == 0 {
            return remaining_item_count;
        }
    }

    if rr_outputs.is_empty() {
        return remaining_item_count;
    }

    /*
     * Round robin distribution can be "fast forwarded" with the following reasoning:
     * 1. Round robin distribution will first evenly fill all output belts that can accept the item type
     *    until a belt gets full.
     * 2. This repeats until we can't distribute any more items.
     * 3. On the last iteration, we may have some `leftover` items that can't be evenly distributed.
     *    Distributing these as if they started from the current rr_index maintains correctness.
     *    Implementation-wise, this means that the first `leftover` belts in the round robin order
     *    will receive one extra item.
     */
    while remaining_item_count > 0 {
        let non_full_outputs = rr_outputs
            .iter()
            .filter(|c| c.can_take_item_type(item_type))
            .map(|c| c.max_acceptable_item_count())
            .filter(|&count| count > 0);
        let num_rr_outputs = non_full_outputs.clone().count() as u16;
        let amount_acceptable_per_belt = non_full_outputs.min().unwrap_or(0);
        if amount_acceptable_per_belt == 0 {
            break;
        }

        let amount_to_distribute =
            remaining_item_count.min(amount_acceptable_per_belt * num_rr_outputs);
        let amount_per_belt = amount_to_distribute / num_rr_outputs;
        let leftover = amount_to_distribute % num_rr_outputs;

        for i in 0..rr_outputs.len() {
            let index = (*rr_index + i) % rr_outputs.len();
            if !rr_outputs[index].can_take_item_type(item_type) {
                continue;
            }

            let to_give = if i < leftover as usize {
                *rr_index = (index + 1) % rr_outputs.len();
                amount_per_belt + 1
            } else {
                amount_per_belt
            };
            debug_assert_eq!(rr_outputs[index].inc_item_count(item_type, to_give), 0);
        }

        remaining_item_count -= amount_to_distribute;
    }

    remaining_item_count
}

/// Runs the round robin loop once.
fn rr_loop_once(
    rr_inputs: &mut [BeltConnection],
    rr_outputs: &mut [BeltConnection],
    input_rr_index: &mut usize,
    output_rr_index: &mut usize,
) {
    if rr_inputs.is_empty() || rr_outputs.is_empty() {
        return;
    }

    // simulate 1-item at a time round robin assignment until everything is assigned or we looped through all inputs
    for i in 0..rr_inputs.len() {
        let input_index = (*input_rr_index + i) % rr_inputs.len();
        let input_connection = &mut rr_inputs[input_index];
        let item_type = if let Some(item_type) = input_connection.current_item_type() {
            item_type
        } else {
            continue;
        };

        // find the next output that can accept this item type, starting from output_rr_index
        for j in 0..rr_outputs.len() {
            let output_index = (*output_rr_index + j) % rr_outputs.len();
            let output_connection = &mut rr_outputs[output_index];
            if output_connection.can_take_item_type(item_type) {
                // assign item type
                output_connection.inc_item_count(item_type, 1);
                input_connection.dec_item_count(1);
                *output_rr_index = (output_index + 1) % rr_outputs.len();
                break;
            }
        }
    }

    // at this point every slot MUST have a slot assigned if the input belts are not empty
    if rr_inputs.iter().any(|c| !c.is_empty()) {
        debug_assert!(rr_outputs.iter().all(|c| !c.is_empty()))
    }
    // dont need to update input_rr_index here as we ran through each input once
}

impl BufferedSplitter {
    pub fn new(
        priority_inputs: Vec<BeltConnection>,
        rr_inputs: Vec<BeltConnection>,
        priority_outputs: Vec<BeltConnection>,
        rr_outputs: Vec<BeltConnection>,
    ) -> Self {
        Self {
            priority_inputs,
            rr_inputs,
            input_rr_index: 0,
            priority_outputs,
            rr_outputs,
            output_rr_index: 0,
        }
    }

    /// Runs a single "tick" of the buffered splitter, processing inputs and distributing items to outputs.
    /// The algorithm first drains from priority inputs to priority outputs, then to rr outputs,
    /// and finally drains from rr inputs to priority outputs and rr outputs.
    ///
    /// This occurs in four steps:
    /// 1. Drain from priority inputs to priority outputs and rr outputs.
    /// 2. Drain from rr inputs to priority outputs
    /// 3. Assign item types based on rr inputs and rr outputs
    /// 4. Drain from rr inputs to rr outputs
    pub fn run(&mut self) {
        // First drain priority inputs
        for input in self.priority_inputs.iter_mut() {
            // filter output connections by item type, skip if none
            let item_type = if let Some(item_type) = input.current_item_type() {
                item_type
            } else {
                continue;
            };

            drain_connections(
                item_type,
                slice::from_mut(input),
                &mut self.input_rr_index,
                self.priority_outputs.as_mut_slice(),
                self.rr_outputs.as_mut_slice(),
                &mut self.output_rr_index,
            );
        }

        /*
         * Next drain rr inputs to priority outputs. As long as types match, this can proceed in any order.
         * We have to process all inputs of the same time simultaneously to keep it round robin.
         */
        let mut types: Vec<_> = self
            .rr_inputs
            .iter()
            .filter_map(|c| c.current_item_type())
            .collect::<Vec<_>>();
        // TODO: does this actually help speed
        types.sort_unstable();
        types.dedup();
        for item_type in types {
            let mut temp = 0;
            drain_connections(
                item_type,
                self.rr_inputs.as_mut_slice(),
                &mut self.input_rr_index,
                self.priority_outputs.as_mut_slice(),
                &mut [],
                &mut temp,
            );
            debug_assert_eq!(temp, 0);
        }

        /*
         * Before we can drain rr inputs to rr outputs, we need to ensure that rr outputs have their item types
         * assigned based on the current rr inputs.
         */
        rr_loop_once(
            self.rr_inputs.as_mut_slice(),
            self.rr_outputs.as_mut_slice(),
            &mut self.input_rr_index,
            &mut self.output_rr_index,
        );

        /*
         * Finally, drain rr inputs to rr outputs. We have to process all inputs of the same time
         * simultaneously to keep it round robin.
         */
        types = self
            .rr_inputs
            .iter()
            .filter_map(|c| c.current_item_type())
            .collect::<Vec<_>>();
        // TODO: does this actually help speed
        types.sort_unstable();
        types.dedup();
        for item_type in types {
            drain_connections(
                item_type,
                self.rr_inputs.as_mut_slice(),
                &mut self.input_rr_index,
                self.priority_outputs.as_mut_slice(),
                self.rr_outputs.as_mut_slice(),
                &mut self.output_rr_index,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::logistics::{BeltConnectionKind, Stack};

    use super::*;

    struct TestSplitter {
        priority_inputs: Vec<BeltConnection>,
        rr_inputs: Vec<BeltConnection>,
        priority_outputs: Vec<BeltConnection>,
        rr_outputs: Vec<BeltConnection>,
        input_rr_index: usize,
        output_rr_index: usize,
    }

    impl TestSplitter {
        fn new(
            priority_inputs: Vec<BeltConnection>,
            rr_inputs: Vec<BeltConnection>,
            priority_outputs: Vec<BeltConnection>,
            rr_outputs: Vec<BeltConnection>,
        ) -> Self {
            Self {
                priority_inputs,
                rr_inputs,
                priority_outputs,
                rr_outputs,
                input_rr_index: 0,
                output_rr_index: 0,
            }
        }

        fn run(&mut self) {
            // Drain priority inputs
            for priority_input in self.priority_inputs.iter_mut() {
                // drain each stack in order
                for _ in 0..priority_input.buffered_item_count() {
                    // first try priority outputs
                    let mut ate_stack = false;
                    for connection in self.priority_outputs.iter_mut() {
                        if connection.inc_item_count(priority_input.current_item_type().unwrap(), 1) == 0 {
                            priority_input.dec_item_count(1);
                            ate_stack = true;
                            break;
                        }
                    }

                    if ate_stack {
                        continue;
                    }

                    // otherwise try rr outputs
                    for i in 0..self.rr_outputs.len() {
                        let index = (self.output_rr_index + i) % self.rr_outputs.len();
                        if self.rr_outputs[index].inc_item_count(priority_input.current_item_type().unwrap(), 1) == 0 {
                            priority_input.dec_item_count(1);
                            self.output_rr_index = (index + 1) % self.rr_outputs.len();
                            break;
                        }
                    }
                }
            }

            // drain rr inputs in round robin fashion
            let num_inputs = self.rr_inputs.len();
            loop {
                if self.rr_inputs.iter().all(|c| c.is_empty()) {
                    break;
                }

                let rr_input = &mut self.rr_inputs[self.input_rr_index];

                if rr_input.is_empty() {
                    self.input_rr_index = (self.input_rr_index + 1) % num_inputs;
                    continue;
                }

                // first try priority outputs
                let mut ate_stack = false;
                for connection in self.priority_outputs.iter_mut() {
                    if connection.inc_item_count(rr_input.current_item_type().unwrap(), 1) == 0 {
                        rr_input.dec_item_count(1);
                        ate_stack = true;
                        break;
                    }
                }

                if ate_stack {
                    self.input_rr_index = (self.input_rr_index + 1) % num_inputs;
                    continue;
                }

                // otherwise try rr outputs
                for i in 0..self.rr_outputs.len() {
                    let index = (self.output_rr_index + i) % self.rr_outputs.len();
                    if self.rr_outputs[index].inc_item_count(rr_input.current_item_type().unwrap(), 1) == 0 {
                        rr_input.dec_item_count(1);
                        self.output_rr_index = (index + 1) % self.rr_outputs.len();
                        break;
                    }
                }

                self.input_rr_index = (self.input_rr_index + 1) % num_inputs;
            }
        }
    }

    #[test]
    fn test_buffered_splitter_rr_simple() {
        let input_limits = 10;
        let mut input_1 = BeltConnection::new(BeltConnectionKind::Input, input_limits, 1, None);
        let mut input_2 = BeltConnection::new(BeltConnectionKind::Input, input_limits, 1, None);
        let output_1 = BeltConnection::new(BeltConnectionKind::Output, input_limits, 1, None);
        let output_2 = BeltConnection::new(BeltConnectionKind::Output, input_limits, 1, None);

        let item_type = 1;
        let item_count = 5;
        input_1.inc_item_count(item_type, item_count);
        input_2.inc_item_count(item_type, item_count);

        // simple test where we have even distribution from rr inputs to rr outputs
        let mut splitter = BufferedSplitter::new(
            vec![],
            vec![input_1, input_2],
            vec![],
            vec![output_1, output_2],
        );

        splitter.run();

        assert_eq!(splitter.rr_inputs[0].buffered_item_count(), 0);
        assert_eq!(splitter.rr_inputs[1].buffered_item_count(), 0);
        assert_eq!(splitter.rr_outputs[0].buffered_item_count(), item_count);
        assert_eq!(splitter.rr_outputs[1].buffered_item_count(), item_count);
    }

    #[test]
    fn test_buffered_splitter_rr_simple_2() {
        let item_type = 1;
        let item_count: u16 = 6;
        let item_limit = item_count * 2;
        let mut input_1 = BeltConnection::new(BeltConnectionKind::Input, item_limit, 1, None);
        let mut input_2 = BeltConnection::new(BeltConnectionKind::Input, item_limit, 1, None);
        let output_1 = BeltConnection::new(BeltConnectionKind::Output, item_limit, 1, None);
        let output_2 = BeltConnection::new(BeltConnectionKind::Output, item_limit, 1, None);

        input_1.inc_item_count(item_type, item_count);
        input_2.inc_item_count(item_type, item_count * 2);

        // simple test where we have even distribution from rr inputs to rr outputs
        let mut splitter = BufferedSplitter::new(
            vec![],
            vec![input_1, input_2],
            vec![],
            vec![output_1, output_2],
        );

        splitter.run();

        let rr_item_count = item_count * 3 / 2;
        assert_eq!(splitter.rr_inputs[0].buffered_item_count(), 0);
        assert_eq!(splitter.rr_inputs[1].buffered_item_count(), 0);
        assert_eq!(splitter.rr_outputs[0].buffered_item_count(), rr_item_count);
        assert_eq!(splitter.rr_outputs[1].buffered_item_count(), rr_item_count);
    }

    #[test]
    fn test_buffered_splitter_rr_simple_3() {
        let item_type = 1;
        let item_count: u16 = 6;
        let item_limit = item_count * 2;
        let mut input_1 = BeltConnection::new(BeltConnectionKind::Input, item_limit, 1, None);
        let mut input_2 = BeltConnection::new(BeltConnectionKind::Input, item_limit, 1, None);
        let mut input_3 = BeltConnection::new(BeltConnectionKind::Input, item_limit, 1, None);
        let output_1 = BeltConnection::new(BeltConnectionKind::Output, item_limit, 1, None);
        let output_2 = BeltConnection::new(BeltConnectionKind::Output, item_limit, 1, None);

        input_1.inc_item_count(item_type, item_count);
        input_2.inc_item_count(item_type, item_count);
        input_3.inc_item_count(item_type, item_count * 2);

        // simple test where we have even distribution from rr inputs to rr outputs
        let mut splitter = BufferedSplitter::new(
            vec![],
            vec![input_1, input_2, input_3],
            vec![],
            vec![output_1, output_2],
        );

        splitter.run();

        let rr_item_count = item_count * 2;
        assert_eq!(splitter.rr_inputs[0].buffered_item_count(), 0);
        assert_eq!(splitter.rr_inputs[1].buffered_item_count(), 0);
        assert_eq!(splitter.rr_inputs[2].buffered_item_count(), 0);
        assert_eq!(splitter.rr_outputs[0].buffered_item_count(), rr_item_count);
        assert_eq!(splitter.rr_outputs[1].buffered_item_count(), rr_item_count);
    }
}
