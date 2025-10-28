use crate::logistics::{Belt, Stack};

const SPLITTER_BUFFER_SIZE: usize = 4;

/**
 * Represents a splitter that divides incoming item stacks into multiple output belts. Inputs are prioritized
 * from the input belts in order, followed by round-robin distribution among remaining belts. Outputs are filled
 * in a similar manner.
 * TODO: Revisit belt ownership 
 */
#[derive(Debug)]
pub struct Splitter {
    priority_inputs: Vec<Belt>,
    rr_inputs: Vec<Belt>,
    input_rr_index: usize,
    priority_outputs: Vec<Belt>,
    rr_outputs: Vec<Belt>,
    output_rr_index: usize,
    item_buffer: Vec<Stack>,
}

impl Splitter {
    pub fn new(
        priority_inputs: Vec<Belt>,
        rr_inputs: Vec<Belt>,
        priority_outputs: Vec<Belt>,
        rr_outputs: Vec<Belt>,
    ) -> Self {
        Self {
            priority_inputs,
            rr_inputs,
            input_rr_index: 0,
            priority_outputs,
            rr_outputs,
            output_rr_index: 0,
            item_buffer: Vec::with_capacity(SPLITTER_BUFFER_SIZE),
        }
    }

    pub fn run(&mut self, ticks: u32) {
        // Process input item stacks and distribute them to output belts
    }

}