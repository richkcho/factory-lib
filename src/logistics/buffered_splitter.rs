use std::slice;

use crate::logistics::{BeltInputConnection, BeltOutputConnection, Connection};
use crate::types::ItemType;

/**
 * Represents a splitter that divides incoming item stacks into multiple output belts. Inputs are prioritized
 * from the input belts in order, followed by round-robin distribution among remaining belts. Outputs are filled
 * in a similar manner.
 * TODO: Revisit belt ownership
 */
#[derive(Debug)]
pub struct BufferedSplitter {
    priority_inputs: Vec<BeltInputConnection>,
    rr_inputs: Vec<BeltInputConnection>,
    input_rr_index: usize,
    priority_outputs: Vec<BeltOutputConnection>,
    rr_outputs: Vec<BeltOutputConnection>,
    output_rr_index: usize,
}

/// Drains items from the given input connections and distributes them to the output connections based on priority
/// and round-robin strategy. Assumes input connections are all equal priority.
fn drain_connections(
    item_type: ItemType,
    rr_inputs: &mut [BeltInputConnection],
    input_rr_index: &mut usize,
    priority_outputs: &mut [BeltOutputConnection],
    rr_outputs: &mut [BeltOutputConnection],
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
    priority_outputs: &mut [BeltOutputConnection],
    rr_outputs: &mut [BeltOutputConnection],
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
    rr_inputs: &mut [BeltInputConnection],
    rr_outputs: &mut [BeltOutputConnection],
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
        priority_inputs: Vec<BeltInputConnection>,
        rr_inputs: Vec<BeltInputConnection>,
        priority_outputs: Vec<BeltOutputConnection>,
        rr_outputs: Vec<BeltOutputConnection>,
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
    use super::Connection;
    use super::*;

    /// A reference implementation of the buffered splitter logic for testing purposes.
    /// Processes items one at a time in the expected order.
    struct TestSplitter {
        priority_inputs: Vec<BeltInputConnection>,
        rr_inputs: Vec<BeltInputConnection>,
        priority_outputs: Vec<BeltOutputConnection>,
        rr_outputs: Vec<BeltOutputConnection>,
        input_rr_index: usize,
        output_rr_index: usize,
    }

    impl TestSplitter {
        fn new(
            priority_inputs: Vec<BeltInputConnection>,
            rr_inputs: Vec<BeltInputConnection>,
            priority_outputs: Vec<BeltOutputConnection>,
            rr_outputs: Vec<BeltOutputConnection>,
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
                        if connection.inc_item_count(priority_input.current_item_type().unwrap(), 1)
                            == 0
                        {
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
                        if self.rr_outputs[index]
                            .inc_item_count(priority_input.current_item_type().unwrap(), 1)
                            == 0
                        {
                            priority_input.dec_item_count(1);
                            self.output_rr_index = (index + 1) % self.rr_outputs.len();
                            break;
                        }
                    }
                }
            }

            // drain rr inputs in round robin fashion
            let num_inputs = self.rr_inputs.len();
            let mut blocked_inputs = vec![false; num_inputs];
            loop {
                if self
                    .rr_inputs
                    .iter()
                    .all(|c| c.is_empty() || blocked_inputs.iter().all(|b| *b))
                {
                    break;
                }

                let rr_input = &mut self.rr_inputs[self.input_rr_index];

                if rr_input.is_empty() {
                    blocked_inputs[self.input_rr_index] = true;
                    self.input_rr_index = (self.input_rr_index + 1) % num_inputs;
                    continue;
                }

                // first try priority outputs
                let mut ate_stack = false;
                for connection in self.priority_outputs.iter_mut() {
                    if connection.inc_item_count(rr_input.current_item_type().unwrap(), 1) == 0 {
                        assert_eq!(rr_input.dec_item_count(1), 0);
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
                    if self.rr_outputs[index]
                        .inc_item_count(rr_input.current_item_type().unwrap(), 1)
                        == 0
                    {
                        ate_stack = true;
                        assert_eq!(rr_input.dec_item_count(1), 0);
                        self.output_rr_index = (index + 1) % self.rr_outputs.len();
                        break;
                    }
                }

                if !ate_stack {
                    blocked_inputs[self.input_rr_index] = true;
                }
                self.input_rr_index = (self.input_rr_index + 1) % num_inputs;
            }
        }
    }

    #[test]
    fn test_buffered_splitter_rr_simple() {
        let input_limits = 10;
        let mut input_1 = BeltInputConnection::new(input_limits, None);
        let mut input_2 = BeltInputConnection::new(input_limits, None);
        let output_1 = BeltOutputConnection::new(input_limits, 1, None);
        let output_2 = BeltOutputConnection::new(input_limits, 1, None);

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
        let mut input_1 = BeltInputConnection::new(item_limit, None);
        let mut input_2 = BeltInputConnection::new(item_limit, None);
        let output_1 = BeltOutputConnection::new(item_limit, 1, None);
        let output_2 = BeltOutputConnection::new(item_limit, 1, None);

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
        let mut input_1 = BeltInputConnection::new(item_limit, None);
        let mut input_2 = BeltInputConnection::new(item_limit, None);
        let mut input_3 = BeltInputConnection::new(item_limit, None);
        let output_1 = BeltOutputConnection::new(item_limit, 1, None);
        let output_2 = BeltOutputConnection::new(item_limit, 1, None);

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

    #[test]
    fn test_buffered_splitter_priority_inputs_fill_before_rr_distribution() {
        const ITEM_TYPE: ItemType = 1;
        const PRIORITY_OUTPUT_LIMIT: u16 = 5;
        const RR_OUTPUT_LIMIT: u16 = 6;
        const PRIORITY_INPUT_COUNTS: [u16; 2] = [4, 3];
        const RR_INPUT_COUNTS: [u16; 2] = [5, 2];
        const EXPECTED_PRIORITY_OUTPUT_COUNTS: [u16; 2] = [5, 5];
        const EXPECTED_RR_OUTPUT_COUNTS: [u16; 2] = [2, 2];

        // Priority belts start with 4 and 3 items, guaranteeing that priority outputs
        // should fill completely before any round-robin outputs receive items.
        let mut priority_inputs = vec![
            BeltInputConnection::new(PRIORITY_OUTPUT_LIMIT, None),
            BeltInputConnection::new(PRIORITY_OUTPUT_LIMIT, None),
        ];
        assert_eq!(
            priority_inputs[0].inc_item_count(ITEM_TYPE, PRIORITY_INPUT_COUNTS[0]),
            0
        );
        assert_eq!(
            priority_inputs[1].inc_item_count(ITEM_TYPE, PRIORITY_INPUT_COUNTS[1]),
            0
        );

        // Round-robin belts are intentionally unbalanced (5 and 2 items) so we can confirm
        // that the splitter evens out the leftovers after priority outputs fill up.
        let mut rr_inputs = vec![
            BeltInputConnection::new(RR_OUTPUT_LIMIT, None),
            BeltInputConnection::new(RR_OUTPUT_LIMIT, None),
        ];
        assert_eq!(
            rr_inputs[0].inc_item_count(ITEM_TYPE, RR_INPUT_COUNTS[0]),
            0
        );
        assert_eq!(
            rr_inputs[1].inc_item_count(ITEM_TYPE, RR_INPUT_COUNTS[1]),
            0
        );

        let priority_outputs = vec![
            BeltOutputConnection::new(PRIORITY_OUTPUT_LIMIT, 1, None),
            BeltOutputConnection::new(PRIORITY_OUTPUT_LIMIT, 1, None),
        ];
        let rr_outputs = vec![
            BeltOutputConnection::new(RR_OUTPUT_LIMIT, 1, None),
            BeltOutputConnection::new(RR_OUTPUT_LIMIT, 1, None),
        ];

        let mut reference = TestSplitter::new(
            priority_inputs.clone(),
            rr_inputs.clone(),
            priority_outputs.clone(),
            rr_outputs.clone(),
        );
        reference.run();

        let mut splitter =
            BufferedSplitter::new(priority_inputs, rr_inputs, priority_outputs, rr_outputs);
        splitter.run();

        let actual_priority_outputs = [
            splitter.priority_outputs[0].buffered_item_count(),
            splitter.priority_outputs[1].buffered_item_count(),
        ];
        let actual_rr_outputs = [
            splitter.rr_outputs[0].buffered_item_count(),
            splitter.rr_outputs[1].buffered_item_count(),
        ];
        let rr_inputs_after = [
            splitter.rr_inputs[0].buffered_item_count(),
            splitter.rr_inputs[1].buffered_item_count(),
        ];

        // After a single run the priority outputs hold 5 items each and the remaining four
        // items are shared evenly between the round-robin outputs. All inputs are emptied.
        assert_eq!(actual_priority_outputs, EXPECTED_PRIORITY_OUTPUT_COUNTS);
        assert_eq!(actual_rr_outputs, EXPECTED_RR_OUTPUT_COUNTS);
        assert_eq!(
            rr_inputs_after,
            [0, 0],
            "rr inputs after {:?}",
            rr_inputs_after
        );

        let expected_priority_outputs = [
            reference.priority_outputs[0].buffered_item_count(),
            reference.priority_outputs[1].buffered_item_count(),
        ];
        let expected_rr_outputs = [
            reference.rr_outputs[0].buffered_item_count(),
            reference.rr_outputs[1].buffered_item_count(),
        ];
        let reference_rr_inputs = [
            reference.rr_inputs[0].buffered_item_count(),
            reference.rr_inputs[1].buffered_item_count(),
        ];

        assert_eq!(expected_priority_outputs, EXPECTED_PRIORITY_OUTPUT_COUNTS);
        assert_eq!(expected_rr_outputs, EXPECTED_RR_OUTPUT_COUNTS);
        assert_eq!(reference_rr_inputs, [0, 0]);

        assert_eq!(actual_priority_outputs, expected_priority_outputs);
        assert_eq!(actual_rr_outputs, expected_rr_outputs);
        assert_eq!(rr_inputs_after, reference_rr_inputs);
    }

    #[test]
    fn test_buffered_splitter_unbalanced_rr_output_capacity() {
        const ITEM_TYPE: ItemType = 2;
        const PRIORITY_OUTPUT_LIMIT: u16 = 4;
        const RR_OUTPUT_STRONG_LIMIT: u16 = 8;
        const RR_OUTPUT_WEAK_LIMIT: u16 = 3;
        const PRIORITY_INPUT_COUNT: u16 = 3;
        const RR_INPUT_HEAVY_COUNT: u16 = 7;
        const RR_INPUT_LIGHT_COUNT: u16 = 2;
        const EXPECTED_PRIORITY_OUTPUT_COUNT: [u16; 1] = [4];
        const EXPECTED_RR_OUTPUT_COUNTS: [u16; 2] = [5, 3];

        // Priority input begins with three items so it can fully occupy the first output
        // before any round-robin redistribution occurs.
        let mut priority_inputs = vec![BeltInputConnection::new(PRIORITY_OUTPUT_LIMIT, None)];
        assert_eq!(
            priority_inputs[0].inc_item_count(ITEM_TYPE, PRIORITY_INPUT_COUNT),
            0
        );

        // Round-robin inputs are unbalanced (7 vs. 2 items) to verify that the splitter
        // drains them proportionally even when the outputs have asymmetric capacities.
        let mut rr_inputs = vec![
            BeltInputConnection::new(RR_OUTPUT_STRONG_LIMIT, None),
            BeltInputConnection::new(RR_OUTPUT_WEAK_LIMIT, None),
        ];
        assert_eq!(
            rr_inputs[0].inc_item_count(ITEM_TYPE, RR_INPUT_HEAVY_COUNT),
            0
        );
        assert_eq!(
            rr_inputs[1].inc_item_count(ITEM_TYPE, RR_INPUT_LIGHT_COUNT),
            0
        );

        let priority_outputs = vec![BeltOutputConnection::new(PRIORITY_OUTPUT_LIMIT, 1, None)];
        let rr_outputs = vec![
            BeltOutputConnection::new(RR_OUTPUT_STRONG_LIMIT, 1, None),
            BeltOutputConnection::new(RR_OUTPUT_WEAK_LIMIT, 1, None),
        ];

        let mut reference = TestSplitter::new(
            priority_inputs.clone(),
            rr_inputs.clone(),
            priority_outputs.clone(),
            rr_outputs.clone(),
        );
        reference.run();

        let mut splitter =
            BufferedSplitter::new(priority_inputs, rr_inputs, priority_outputs, rr_outputs);
        splitter.run();

        let actual_priority_output = [splitter.priority_outputs[0].buffered_item_count()];
        let actual_rr_outputs = [
            splitter.rr_outputs[0].buffered_item_count(),
            splitter.rr_outputs[1].buffered_item_count(),
        ];
        let rr_inputs_after = [
            splitter.rr_inputs[0].buffered_item_count(),
            splitter.rr_inputs[1].buffered_item_count(),
        ];

        // The priority output absorbs four items, the stronger round-robin output ends with five,
        // and the weaker output tops out at three items. Both inputs are fully drained.
        assert_eq!(actual_priority_output, EXPECTED_PRIORITY_OUTPUT_COUNT);
        assert_eq!(actual_rr_outputs, EXPECTED_RR_OUTPUT_COUNTS);
        assert_eq!(rr_inputs_after, [0, 0]);

        let expected_priority_output = [reference.priority_outputs[0].buffered_item_count()];
        let expected_rr_outputs = [
            reference.rr_outputs[0].buffered_item_count(),
            reference.rr_outputs[1].buffered_item_count(),
        ];
        let reference_rr_inputs = [
            reference.rr_inputs[0].buffered_item_count(),
            reference.rr_inputs[1].buffered_item_count(),
        ];

        assert_eq!(expected_priority_output, EXPECTED_PRIORITY_OUTPUT_COUNT);
        assert_eq!(expected_rr_outputs, EXPECTED_RR_OUTPUT_COUNTS);
        assert_eq!(reference_rr_inputs, [0, 0]);

        assert_eq!(actual_priority_output, expected_priority_output);
        assert_eq!(actual_rr_outputs, expected_rr_outputs);
        assert_eq!(rr_inputs_after, reference_rr_inputs);
    }

    #[test]
    fn test_buffered_splitter_mixed_item_types() {
        const ITEM_A: ItemType = 1;
        const ITEM_B: ItemType = 2;
        const PRIORITY_OUTPUT_LIMIT: u16 = 3;
        const RR_OUTPUT_LIMIT: u16 = 3;
        const PRIORITY_INPUTS: [(ItemType, u16); 2] = [(ITEM_A, 2), (ITEM_B, 1)];
        const RR_INPUTS: [(ItemType, u16); 2] = [(ITEM_A, 2), (ITEM_B, 2)];
        const EXPECTED_PRIORITY_OUT_COUNTS: [u16; 2] = [3, 3];
        const EXPECTED_RR_OUT_COUNTS: [u16; 2] = [1, 1];
        const EXPECTED_RR_INPUT_REMAINDER: [u16; 2] = [0, 0];

        // Priority inputs start with a mix of ITEM_A and ITEM_B. Priority outputs should
        // be filled first, with the second output already primed to accept only ITEM_B.
        let priority_inputs: Vec<BeltInputConnection> = PRIORITY_INPUTS
            .iter()
            .map(|&(item_type, count)| {
                let mut connection = BeltInputConnection::new(PRIORITY_OUTPUT_LIMIT, None);
                assert_eq!(connection.inc_item_count(item_type, count), 0);
                connection
            })
            .collect();

        // Round-robin inputs continue the mixed scenario. They introduce more items of each type,
        // ensuring the splitter has to interleave ITEM_A and ITEM_B while respecting existing types.
        let rr_inputs: Vec<BeltInputConnection> = RR_INPUTS
            .iter()
            .map(|&(item_type, count)| {
                let mut connection = BeltInputConnection::new(RR_OUTPUT_LIMIT, None);
                assert_eq!(connection.inc_item_count(item_type, count), 0);
                connection
            })
            .collect();

        let mut priority_outputs = vec![
            BeltOutputConnection::new(PRIORITY_OUTPUT_LIMIT, 1, None),
            BeltOutputConnection::new(PRIORITY_OUTPUT_LIMIT, 1, None),
        ];
        assert_eq!(priority_outputs[1].inc_item_count(ITEM_B, 1), 0);

        let rr_outputs = vec![
            BeltOutputConnection::new(RR_OUTPUT_LIMIT, 1, None),
            BeltOutputConnection::new(RR_OUTPUT_LIMIT, 1, None),
        ];

        let mut reference = TestSplitter::new(
            priority_inputs.clone(),
            rr_inputs.clone(),
            priority_outputs.clone(),
            rr_outputs.clone(),
        );
        reference.run();

        let mut splitter =
            BufferedSplitter::new(priority_inputs, rr_inputs, priority_outputs, rr_outputs);
        splitter.run();

        let actual_priority_counts = [
            splitter.priority_outputs[0].buffered_item_count(),
            splitter.priority_outputs[1].buffered_item_count(),
        ];
        let actual_rr_counts = [
            splitter.rr_outputs[0].buffered_item_count(),
            splitter.rr_outputs[1].buffered_item_count(),
        ];
        let rr_inputs_after = [
            splitter.rr_inputs[0].buffered_item_count(),
            splitter.rr_inputs[1].buffered_item_count(),
        ];

        // After processing mixed inputs, priority outputs finish with ITEM_A and ITEM_B respectively,
        // while round-robin outputs absorb the remaining items and all inputs are drained.
        assert_eq!(actual_priority_counts, EXPECTED_PRIORITY_OUT_COUNTS);
        assert_eq!(actual_rr_counts, EXPECTED_RR_OUT_COUNTS);
        assert_eq!(rr_inputs_after, EXPECTED_RR_INPUT_REMAINDER);
        assert_eq!(
            splitter.priority_outputs[0].current_item_type(),
            Some(ITEM_A)
        );
        assert_eq!(
            splitter.priority_outputs[1].current_item_type(),
            Some(ITEM_B)
        );

        let expected_priority_counts = [
            reference.priority_outputs[0].buffered_item_count(),
            reference.priority_outputs[1].buffered_item_count(),
        ];
        let expected_rr_counts = [
            reference.rr_outputs[0].buffered_item_count(),
            reference.rr_outputs[1].buffered_item_count(),
        ];
        let reference_rr_inputs = [
            reference.rr_inputs[0].buffered_item_count(),
            reference.rr_inputs[1].buffered_item_count(),
        ];

        assert_eq!(expected_priority_counts, EXPECTED_PRIORITY_OUT_COUNTS);
        assert_eq!(expected_rr_counts, EXPECTED_RR_OUT_COUNTS);
        assert_eq!(reference_rr_inputs, EXPECTED_RR_INPUT_REMAINDER);

        assert_eq!(actual_priority_counts, expected_priority_counts);
        assert_eq!(actual_rr_counts, expected_rr_counts);
        assert_eq!(rr_inputs_after, reference_rr_inputs);
    }

    #[test]
    fn test_buffered_splitter_high_volume_partial_drain() {
        const ITEM_TYPE: ItemType = 3;
        const PRIORITY_INPUT_LIMIT: u16 = 220;
        const RR_INPUT_LIMIT: u16 = 260;
        const RR_INPUT_COUNTS: [u16; 2] = [220, 180];
        const PRIORITY_OUTPUT_LIMITS: [u16; 2] = [150, 150];
        const PRIORITY_OUTPUT_START: [u16; 2] = [50, 50];
        const RR_OUTPUT_LIMITS: [u16; 2] = [120, 90];
        const RR_OUTPUT_START: [u16; 2] = [20, 10];
        const EXPECTED_PRIORITY_OUTPUT_COUNTS: [u16; 2] = [150, 150];
        const EXPECTED_RR_OUTPUT_COUNTS: [u16; 2] = [120, 90];
        const EXPECTED_RR_INPUT_REMAINDER: [u16; 2] = [20, 0];

        // Priority inputs are intentionally empty so the rr inputs can demonstrate partial draining.
        let priority_inputs: Vec<BeltInputConnection> = (0..2)
            .map(|_| BeltInputConnection::new(PRIORITY_INPUT_LIMIT, None))
            .collect();

        // Round-robin inputs begin with large buffers so one of them still holds items after a single tick.
        let rr_inputs: Vec<BeltInputConnection> = RR_INPUT_COUNTS
            .iter()
            .map(|&count| {
                let mut connection = BeltInputConnection::new(RR_INPUT_LIMIT, None);
                assert_eq!(connection.inc_item_count(ITEM_TYPE, count), 0);
                connection
            })
            .collect();

        // Priority outputs start partially filled, leaving 100 free slots each for ITEM_TYPE.
        let priority_outputs: Vec<BeltOutputConnection> = PRIORITY_OUTPUT_LIMITS
            .iter()
            .zip(PRIORITY_OUTPUT_START.iter())
            .map(|(&limit, &start)| {
                let mut connection = BeltOutputConnection::new(limit, 1, None);
                assert_eq!(connection.inc_item_count(ITEM_TYPE, start), 0);
                connection
            })
            .collect();

        // Round-robin outputs also begin with inventory, constraining how many items can be drained overall.
        let rr_outputs: Vec<BeltOutputConnection> = RR_OUTPUT_LIMITS
            .iter()
            .zip(RR_OUTPUT_START.iter())
            .map(|(&limit, &start)| {
                let mut connection = BeltOutputConnection::new(limit, 1, None);
                assert_eq!(connection.inc_item_count(ITEM_TYPE, start), 0);
                connection
            })
            .collect();

        let mut reference = TestSplitter::new(
            priority_inputs.clone(),
            rr_inputs.clone(),
            priority_outputs.clone(),
            rr_outputs.clone(),
        );
        reference.run();

        let mut splitter =
            BufferedSplitter::new(priority_inputs, rr_inputs, priority_outputs, rr_outputs);
        splitter.run();

        let actual_priority_outputs = [
            splitter.priority_outputs[0].buffered_item_count(),
            splitter.priority_outputs[1].buffered_item_count(),
        ];
        let actual_rr_outputs = [
            splitter.rr_outputs[0].buffered_item_count(),
            splitter.rr_outputs[1].buffered_item_count(),
        ];
        let rr_inputs_after = [
            splitter.rr_inputs[0].buffered_item_count(),
            splitter.rr_inputs[1].buffered_item_count(),
        ];

        // Priority outputs top out at their configured limits, round-robin outputs consume the
        // remaining capacity, and one heavy rr input keeps 20 items because the system is saturated.
        assert_eq!(actual_priority_outputs, EXPECTED_PRIORITY_OUTPUT_COUNTS);
        assert_eq!(actual_rr_outputs, EXPECTED_RR_OUTPUT_COUNTS);
        assert_eq!(rr_inputs_after, EXPECTED_RR_INPUT_REMAINDER);

        let expected_priority_outputs = [
            reference.priority_outputs[0].buffered_item_count(),
            reference.priority_outputs[1].buffered_item_count(),
        ];
        let expected_rr_outputs = [
            reference.rr_outputs[0].buffered_item_count(),
            reference.rr_outputs[1].buffered_item_count(),
        ];
        let reference_rr_inputs = [
            reference.rr_inputs[0].buffered_item_count(),
            reference.rr_inputs[1].buffered_item_count(),
        ];

        assert_eq!(expected_priority_outputs, EXPECTED_PRIORITY_OUTPUT_COUNTS);
        assert_eq!(expected_rr_outputs, EXPECTED_RR_OUTPUT_COUNTS);
        assert_eq!(reference_rr_inputs, EXPECTED_RR_INPUT_REMAINDER);

        assert_eq!(actual_priority_outputs, expected_priority_outputs);
        assert_eq!(actual_rr_outputs, expected_rr_outputs);
        assert_eq!(rr_inputs_after, reference_rr_inputs);
    }
}
