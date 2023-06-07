use aleo_rust::{Block, Network, ProgramID};
use snarkvm::synthesizer::Transition;

#[derive(Clone, Debug)]
pub struct TransitionFilter<N: Network> {
    program_ids: Vec<ProgramID<N>>,
    function_names: Vec<String>,
}

impl<N: Network> TransitionFilter<N> {
    pub fn add_program(mut self, program_id: ProgramID<N>) -> Self {
        self.program_ids.push(program_id);
        self
    }

    pub fn add_function(mut self, function_name: String) -> Self {
        self.function_names.push(function_name);
        self
    }

    pub fn new() -> Self {
        Self {
            program_ids: Vec::new(),
            function_names: Vec::new(),
        }
    }

    pub fn filter_block(&self, block: Block<N>) -> Vec<Transition<N>> {
        let ts = block
            .transactions()
            .clone()
            .into_iter()
            .filter(|tx| tx.is_accepted())
            .flat_map(|tx| tx.into_transaction().into_transitions())
            .collect::<Vec<Transition<N>>>();

        ts.into_iter().filter(|t| {
            let program_id = t.program_id();
            let function_name = t.function_name().to_string();
            self.program_ids.contains(program_id) && self.function_names.contains(&function_name)
        }).collect()
    }
}
