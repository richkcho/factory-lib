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
/// and round-robin strategy. Assumes input connections are all equal priority and hold the same item type.
fn drain_connections(
    rr_inputs: &mut [&mut BeltConnection],
    input_rr_index: &mut usize,
    priority_outputs: &mut [&mut BeltConnection],
    rr_outputs: &mut [&mut BeltConnection],
    output_rr_index: &mut usize,
) -> Option<()> {
    if rr_inputs.is_empty() {
        return None;
    }

    let item_type = rr_inputs[0].current_item_type()?;

    debug_assert!(
        rr_inputs
            .iter()
            .all(|c| c.current_item_type().unwrap() == item_type)
    );

    let item_count: u16 = rr_inputs.iter().map(|c| c.buffered_item_count()).sum();
    let remaining_item_count = distribute_items(
        item_count,
        item_type,
        priority_outputs,
        rr_outputs,
        output_rr_index,
    );

    let mut remaining_to_drain = item_count - remaining_item_count;
    if remaining_to_drain == 0 {
        return None;
    }

    while remaining_to_drain > 0 {
        let amount_acceptable_per_belt = rr_inputs
            .iter()
            .map(|c| c.max_acceptable_item_count())
            .min()
            .unwrap_or(0);
        if amount_acceptable_per_belt == 0 {
            break;
        }

        let amount_to_take =
            remaining_item_count.min(amount_acceptable_per_belt * rr_inputs.len() as u16);
        let amount_per_belt = amount_to_take / rr_inputs.len() as u16;
        let leftover = amount_to_take % rr_inputs.len() as u16;

        for i in 0..rr_inputs.len() {
            let index = (*input_rr_index + i) % rr_inputs.len();
            let to_take = if i < leftover as usize {
                amount_per_belt + 1
            } else {
                amount_per_belt
            };
            assert_eq!(rr_inputs[index].dec_item_count(to_take), 0);
        }
        *input_rr_index = (*input_rr_index + leftover as usize) % rr_inputs.len();

        remaining_to_drain -= amount_to_take;
    }

    None
}

/// Distributes the given number of items of the specified type to the output connections based on priority
/// and round-robin strategy. Returns the number of items that could not be distributed.
fn distribute_items(
    mut remaining_item_count: u16,
    item_type: ItemType,
    priority_outputs: &mut [&mut BeltConnection],
    rr_outputs: &mut [&mut BeltConnection],
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
    let rr_output_count = rr_outputs
        .iter()
        .filter(|c| c.can_take_item_type(item_type))
        .count() as u16;
    while remaining_item_count > 0 {
        let amount_acceptable_per_belt = rr_outputs
            .iter()
            .map(|c| c.max_acceptable_item_count())
            .min()
            .unwrap_or(0);
        if amount_acceptable_per_belt == 0 {
            break;
        }

        let amount_to_distribute =
            remaining_item_count.min(amount_acceptable_per_belt * rr_output_count);
        let amount_per_belt = amount_to_distribute / rr_output_count;
        let leftover = amount_to_distribute % rr_output_count;

        for i in 0..rr_outputs.len() {
            let index = (*rr_index + i) % rr_outputs.len();
            if !rr_outputs[index].can_take_item_type(item_type) {
                continue;
            }

            let to_give = if i < leftover as usize {
                amount_per_belt + 1
            } else {
                amount_per_belt
            };
            assert_eq!(rr_outputs[index].inc_item_count(item_type, to_give), 0);
        }
        *rr_index = (*rr_index + leftover as usize) % rr_outputs.len();

        remaining_item_count -= amount_to_distribute;
    }

    remaining_item_count
}

/// Runs the round robin loop once.
fn rr_loop_once(
    rr_inputs: &mut [&mut BeltConnection],
    rr_outputs: &mut [&mut BeltConnection],
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
                *output_rr_index = (*output_rr_index + 1) % rr_outputs.len();
                *input_rr_index = (*input_rr_index + 1) % rr_inputs.len();
                break;
            }
        }

        // at this point every slot MUST have a slot assigned if the input belts are not empty
        if rr_inputs.iter().any(|c| !c.is_empty()) {
            debug_assert!(rr_outputs.iter().all(|c| !c.is_empty()))
        }
    }
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

            // create a &mut slice of outputs that can accept this item type
            let mut compatible_priority_outputs: Vec<&mut BeltConnection> = self
                .priority_outputs
                .iter_mut()
                .filter(|c| c.can_take_item_type(item_type))
                .collect();
            let mut compatible_rr_outputs: Vec<&mut BeltConnection> = self
                .rr_outputs
                .iter_mut()
                .filter(|c| c.can_take_item_type(item_type))
                .collect();

            if compatible_priority_outputs.is_empty() && compatible_rr_outputs.is_empty() {
                continue;
            }

            drain_connections(
                &mut [input],
                &mut self.input_rr_index,
                compatible_priority_outputs.as_mut_slice(),
                compatible_rr_outputs.as_mut_slice(),
                &mut self.output_rr_index,
            );
        }

        /*
         * Next drain rr inputs to priority outputs. As long as types match, this can proceed in any order.
         * We have to process all inputs of the same time simultaneously to keep it round robin.
         */
        let mut remaining_inputs: Vec<_> = self.rr_inputs.iter_mut().collect();
        for item_type in remaining_inputs
            .iter()
            .filter_map(|c| c.current_item_type())
            .collect::<Vec<_>>()
        {
            // partition inputs by item type
            let (mut inputs, rest): (Vec<_>, Vec<_>) = remaining_inputs
                .into_iter()
                .partition(|c| c.can_take_item_type(item_type));
            remaining_inputs = rest;

            if inputs.is_empty() {
                continue;
            }

            // create a &mut slice of outputs that can accept this item type
            let mut compatible_priority_outputs: Vec<&mut BeltConnection> = self
                .priority_outputs
                .iter_mut()
                .filter(|c| c.can_take_item_type(item_type))
                .collect();

            if compatible_priority_outputs.is_empty() {
                continue;
            }

            let mut temp = 0;
            drain_connections(
                inputs.as_mut_slice(),
                &mut self.input_rr_index,
                compatible_priority_outputs.as_mut_slice(),
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
            self.rr_inputs.iter_mut().collect::<Vec<_>>().as_mut_slice(),
            self.rr_outputs
                .iter_mut()
                .collect::<Vec<_>>()
                .as_mut_slice(),
            &mut self.input_rr_index,
            &mut self.output_rr_index,
        );

        /*
         * Finally, drain rr inputs to rr outputs. We have to process all inputs of the same time
         * simultaneously to keep it round robin.
         */
        drain_connections(
            self.rr_inputs.iter_mut().collect::<Vec<_>>().as_mut_slice(),
            &mut self.input_rr_index,
            self.priority_outputs
                .iter_mut()
                .collect::<Vec<_>>()
                .as_mut_slice(),
            self.rr_outputs
                .iter_mut()
                .collect::<Vec<_>>()
                .as_mut_slice(),
            &mut self.output_rr_index,
        );
    }
}
