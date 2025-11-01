use crate::logistics::{Belt, Stack};

/**
 * A splitter that interacts directly with belts instead of intermediate buffers.
 * Belts are grouped into priority and round-robin (RR) sets for both inputs and outputs.
 * Priority groups are processed greedily in order, while RR groups advance internal indices
 * to maintain round-robin fairness between ticks.
 */
#[derive(Debug, Default)]
pub struct Splitter {
    input_rr_index: usize,
    output_rr_index: usize,
}

impl Splitter {
    pub fn new() -> Self {
        Self {
            input_rr_index: 0,
            output_rr_index: 0,
        }
    }

    /// Runs a single tick of the splitter. Only the belt ends are touched: items are read from
    /// the head of input belts and appended to the tail of output belts.
    /// Priority inputs feed priority outputs first, then RR outputs. Round-robin inputs fill
    /// any remaining priority outputs before participating in RR distribution.
    pub fn run(
        &mut self,
        priority_inputs: &mut [&mut Belt],
        rr_inputs: &mut [&mut Belt],
        priority_outputs: &mut [&mut Belt],
        rr_outputs: &mut [&mut Belt],
    ) {
        if rr_inputs.is_empty() {
            self.input_rr_index = 0;
        } else if self.input_rr_index >= rr_inputs.len() {
            self.input_rr_index %= rr_inputs.len();
        }

        if rr_outputs.is_empty() {
            self.output_rr_index = 0;
        } else if self.output_rr_index >= rr_outputs.len() {
            self.output_rr_index %= rr_outputs.len();
        }

        self.drain_priority_inputs(priority_inputs, priority_outputs, rr_outputs);
        self.drain_rr_inputs_to_priority(rr_inputs, priority_outputs);
        self.drain_rr_inputs_to_rr(rr_inputs, rr_outputs);
    }

    fn drain_priority_inputs(
        &mut self,
        priority_inputs: &mut [&mut Belt],
        priority_outputs: &mut [&mut Belt],
        rr_outputs: &mut [&mut Belt],
    ) {
        for input in priority_inputs.iter_mut() {
            let belt = &mut **input;
            loop {
                let Some((stack, _)) = belt.peek_front_stack() else {
                    break;
                };

                if !self.try_assign_full(&stack, priority_outputs, rr_outputs) {
                    break;
                }

                let removed = belt.remove_item();
                debug_assert!(removed.is_some());
            }
        }
    }

    fn drain_rr_inputs_to_priority(
        &mut self,
        rr_inputs: &mut [&mut Belt],
        priority_outputs: &mut [&mut Belt],
    ) {
        if priority_outputs.is_empty() {
            return;
        }

        let mut progress = true;
        while progress {
            progress = false;
            for input in rr_inputs.iter_mut() {
                let belt = &mut **input;
                loop {
                    let Some((stack, _)) = belt.peek_front_stack() else {
                        break;
                    };

                    if !Self::try_assign_priority(&stack, priority_outputs) {
                        break;
                    }

                    let removed = belt.remove_item();
                    debug_assert!(removed.is_some());
                    progress = true;
                }
            }
        }
    }

    fn drain_rr_inputs_to_rr(&mut self, rr_inputs: &mut [&mut Belt], rr_outputs: &mut [&mut Belt]) {
        let input_len = rr_inputs.len();
        if input_len == 0 || rr_outputs.is_empty() {
            return;
        }

        if self.input_rr_index >= input_len {
            self.input_rr_index %= input_len;
        }

        let mut progress = true;
        while progress {
            progress = false;

            for _ in 0..input_len {
                let idx = self.input_rr_index;
                let belt_slot = rr_inputs
                    .get_mut(idx)
                    .expect("index must be within rr_inputs bounds");
                let belt = &mut **belt_slot;

                if let Some((stack, _)) = belt.peek_front_stack() {
                    if self.try_assign_rr(&stack, rr_outputs) {
                        let removed = belt.remove_item();
                        debug_assert!(removed.is_some());
                        progress = true;
                    }
                }

                self.input_rr_index = (self.input_rr_index + 1) % input_len;
            }
        }
    }

    fn try_assign_full(
        &mut self,
        stack: &Stack,
        priority_outputs: &mut [&mut Belt],
        rr_outputs: &mut [&mut Belt],
    ) -> bool {
        if Self::try_assign_priority(stack, priority_outputs) {
            return true;
        }

        self.try_assign_rr(stack, rr_outputs)
    }

    fn try_assign_priority(stack: &Stack, priority_outputs: &mut [&mut Belt]) -> bool {
        let item_type = stack.item_type;
        let item_count = stack.item_count;

        for output in priority_outputs.iter_mut() {
            if output.add_item(Stack::new(item_type, item_count)) {
                return true;
            }
        }

        false
    }

    fn try_assign_rr(&mut self, stack: &Stack, rr_outputs: &mut [&mut Belt]) -> bool {
        let len = rr_outputs.len();
        if len == 0 {
            return false;
        }

        if self.output_rr_index >= len {
            self.output_rr_index %= len;
        }

        let item_type = stack.item_type;
        let item_count = stack.item_count;

        for offset in 0..len {
            let idx = (self.output_rr_index + offset) % len;
            if rr_outputs[idx].add_item(Stack::new(item_type, item_count)) {
                self.output_rr_index = (idx + 1) % len;
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ITEM_WIDTH;

    fn stack(item_type: u16, item_count: u16) -> Stack {
        Stack::new(item_type, item_count)
    }

    #[test]
    fn priority_input_to_priority_output() {
        let mut splitter = Splitter::new();
        let mut input = Belt::new(ITEM_WIDTH, 1);
        let mut output = Belt::new(ITEM_WIDTH * 2, 1);

        assert!(input.add_item(stack(10, 3)));

        let mut priority_inputs = vec![&mut input];
        let mut rr_inputs: Vec<&mut Belt> = Vec::new();
        let mut priority_outputs = vec![&mut output];
        let mut rr_outputs: Vec<&mut Belt> = Vec::new();

        splitter.run(
            priority_inputs.as_mut_slice(),
            rr_inputs.as_mut_slice(),
            priority_outputs.as_mut_slice(),
            rr_outputs.as_mut_slice(),
        );

        drop(priority_inputs);
        drop(rr_inputs);
        drop(priority_outputs);
        drop(rr_outputs);

        assert!(input.is_empty());
        assert_eq!(output.item_count(), 1);
    }

    #[test]
    fn priority_input_round_robins_outputs() {
        let mut splitter = Splitter::new();
        let mut input = Belt::new(ITEM_WIDTH, 1);
        let mut output_a = Belt::new(ITEM_WIDTH, 1);
        let mut output_b = Belt::new(ITEM_WIDTH, 1);

        for _ in 0..2 {
            assert!(input.add_item(stack(1, 1)));

            let mut priority_inputs = vec![&mut input];
            let mut rr_inputs: Vec<&mut Belt> = Vec::new();
            let mut priority_outputs: Vec<&mut Belt> = Vec::new();
            let mut rr_outputs = vec![&mut output_a, &mut output_b];

            splitter.run(
                priority_inputs.as_mut_slice(),
                rr_inputs.as_mut_slice(),
                priority_outputs.as_mut_slice(),
                rr_outputs.as_mut_slice(),
            );

            drop(priority_inputs);
            drop(rr_inputs);
            drop(priority_outputs);
            drop(rr_outputs);
        }

        assert!(input.is_empty());
        assert_eq!(output_a.item_count(), 1);
        assert_eq!(output_b.item_count(), 1);
    }

    #[test]
    fn rr_inputs_fill_priority_outputs_first() {
        let mut splitter = Splitter::new();
        let mut input_a = Belt::new(ITEM_WIDTH, 1);
        let mut input_b = Belt::new(ITEM_WIDTH, 1);
        let mut priority_output_a = Belt::new(ITEM_WIDTH, 1);
        let mut priority_output_b = Belt::new(ITEM_WIDTH, 1);

        assert!(input_a.add_item(stack(5, 1)));
        assert!(input_b.add_item(stack(5, 1)));

        let mut priority_inputs: Vec<&mut Belt> = Vec::new();
        let mut rr_inputs = vec![&mut input_a, &mut input_b];
        let mut priority_outputs = vec![&mut priority_output_a, &mut priority_output_b];
        let mut rr_outputs: Vec<&mut Belt> = Vec::new();

        splitter.run(
            priority_inputs.as_mut_slice(),
            rr_inputs.as_mut_slice(),
            priority_outputs.as_mut_slice(),
            rr_outputs.as_mut_slice(),
        );

        drop(priority_inputs);
        drop(rr_inputs);
        drop(priority_outputs);
        drop(rr_outputs);

        assert!(input_a.is_empty());
        assert!(input_b.is_empty());
        assert_eq!(priority_output_a.item_count(), 1);
        assert_eq!(priority_output_b.item_count(), 1);
    }

    #[test]
    fn rr_inputs_round_robin_to_rr_outputs() {
        let mut splitter = Splitter::new();
        let mut input_a = Belt::new(ITEM_WIDTH, 1);
        let mut input_b = Belt::new(ITEM_WIDTH, 1);
        let mut rr_output_a = Belt::new(ITEM_WIDTH, 1);
        let mut rr_output_b = Belt::new(ITEM_WIDTH, 1);

        assert!(input_a.add_item(stack(7, 2)));
        assert!(input_b.add_item(stack(7, 2)));

        let mut priority_inputs: Vec<&mut Belt> = Vec::new();
        let mut rr_inputs = vec![&mut input_a, &mut input_b];
        let mut priority_outputs: Vec<&mut Belt> = Vec::new();
        let mut rr_outputs = vec![&mut rr_output_a, &mut rr_output_b];

        splitter.run(
            priority_inputs.as_mut_slice(),
            rr_inputs.as_mut_slice(),
            priority_outputs.as_mut_slice(),
            rr_outputs.as_mut_slice(),
        );

        drop(priority_inputs);
        drop(rr_inputs);
        drop(priority_outputs);
        drop(rr_outputs);

        assert!(input_a.is_empty());
        assert!(input_b.is_empty());
        assert_eq!(rr_output_a.item_count(), 1);
        assert_eq!(rr_output_b.item_count(), 1);
    }

    #[test]
    fn input_stalls_when_outputs_full() {
        let mut splitter = Splitter::new();
        let mut input = Belt::new(ITEM_WIDTH, 1);
        let mut output = Belt::new(ITEM_WIDTH, 1);

        assert!(input.add_item(stack(3, 2)));
        assert!(output.add_item(stack(3, 2)));

        let mut priority_inputs = vec![&mut input];
        let mut rr_inputs: Vec<&mut Belt> = Vec::new();
        let mut priority_outputs = vec![&mut output];
        let mut rr_outputs: Vec<&mut Belt> = Vec::new();

        splitter.run(
            priority_inputs.as_mut_slice(),
            rr_inputs.as_mut_slice(),
            priority_outputs.as_mut_slice(),
            rr_outputs.as_mut_slice(),
        );

        drop(priority_inputs);
        drop(rr_inputs);
        drop(priority_outputs);
        drop(rr_outputs);

        assert_eq!(input.item_count(), 1);
        assert_eq!(output.item_count(), 1);
    }
}
