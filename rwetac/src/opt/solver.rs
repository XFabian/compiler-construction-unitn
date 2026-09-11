//! Generic dataflow analysis solver.
//!
//! Provides the [`Analysis`] trait and a worklist-based [`Solver`] that finds
//! the fixed point of any dataflow analysis over a [`Cfg`]. Concrete analyses
//! (like [`LivenessAnalysis`](super::liveness::LivenessAnalysis) or
//! [`ReachingAnalysis`](super::reaching::ReachingAnalysis)) implement [`Analysis`]
//! by defining their fact type, transfer function, combine operation, and direction.

use core::fmt;
use std::collections::HashMap;
use std::fmt::Debug;

use crate::opt::cfg::Cfg;
use crate::wtac::wtac_ast as W;

/// Whether a dataflow analysis propagates information forward or backward.
pub enum Direction {
    Forwards,
    Backwards,
}

/// Defines a dataflow analysis that can be solved by the [`Solver`].
///
/// Implementors specify the type of dataflow facts, how facts are combined
/// at join points, and how individual instructions transform facts.
pub trait Analysis {
    /// The type of dataflow facts (e.g. a set of live variables, a set of
    /// reaching definitions, a set of dominator block IDs).
    type Fact: Debug + Clone + PartialEq + Eq;

    /// Combines facts at a join point (merge of control flow paths).
    /// This is the meet or join operation of the lattice, depending on the analysis.
    fn combine(&self, facts: &[&Self::Fact]) -> Self::Fact;

    /// The transfer function for a single instruction.
    /// Computes the output fact from the input fact (forward) or vice versa (backward).
    fn flow(&self, in_fact: &Self::Fact, lab_instr: (usize, &W::Instruction)) -> Self::Fact;

    /// Transfer function for empty blocks. Defaults to identity.
    ///
    /// Overridden by dominator analysis, which still needs to add the
    /// block ID even when the block has no instructions.
    fn empty_flow(&self, in_fact: &Self::Fact, _block_id: usize) -> Self::Fact {
        in_fact.clone()
    }

    fn direction() -> Direction;

    /// The initial (bottom/top) fact for all blocks before iteration begins.
    fn initial_fact(&self) -> Self::Fact;

    /// Initializes the in/out fact maps for the entire CFG.
    ///
    /// Override this to set up special initial conditions, such as seeding
    /// the entry block for dominator analysis or the exit block for liveness.
    fn init_facts(&self, cfg: &Cfg) -> (HashMap<usize, Self::Fact>, HashMap<usize, Self::Fact>) {
        let mut in_facts = HashMap::new();
        let mut out_facts = HashMap::new();

        // Compute initial facts for each block
        for block_id in cfg.blocks.keys() {
            in_facts.insert(*block_id, self.initial_fact());
            out_facts.insert(*block_id, self.initial_fact());
        }
        (in_facts, out_facts)
    }
}

/// The results of a solved dataflow analysis, with per-block and per-instruction facts.
pub struct AnalysisResults<A: Analysis> {
    /// In-facts for each basic block.
    pub in_facts: HashMap<usize, A::Fact>,
    /// Out-facts for each basic block.
    pub out_facts: HashMap<usize, A::Fact>,
    /// In-facts for each instruction, keyed by `(block_id, instr_id)`.
    pub instr_in: HashMap<(usize, usize), A::Fact>,
    /// Out-facts for each instruction, keyed by `(block_id, instr_id)`.
    pub instr_out: HashMap<(usize, usize), A::Fact>,
}

/// Worklist-based iterative dataflow solver.
///
/// Repeatedly applies transfer functions and combines facts at join points
/// until no facts change (the fixed point is reached).
pub struct Solver<'a, A: Analysis> {
    pub cfg: &'a Cfg,
    pub analysis: A,
    pub in_facts: HashMap<usize, A::Fact>,
    pub out_facts: HashMap<usize, A::Fact>,
}

impl<'a, A: Analysis> Solver<'a, A> {
    pub fn new(cfg: &'a Cfg, analysis: A) -> Self {
        let (in_facts, out_facts) = analysis.init_facts(cfg);
        Solver {
            cfg,
            analysis,
            in_facts,
            out_facts,
        }
    }

    /// Runs the worklist algorithm to find the fixed point.
    ///
    /// For forward analyses, combines predecessor out-facts to compute in-facts,
    /// then applies the block transfer function to compute out-facts.
    /// For backward analyses, the roles of in/out and predecessors/successors are swapped.
    /// When a fact changes, the affected neighbors are added back to the worklist.
    pub fn solve(&mut self) {
        let mut worklist: Vec<usize> = self.cfg.blocks.keys().cloned().collect();
        while let Some(block_id) = worklist.pop() {
            let block = self.cfg.get_block(block_id);
            match A::direction() {
                Direction::Forwards => {
                    // Combine all Out Facts from Incoming nodes to get In fact of current node
                    let pred_outs: Vec<&A::Fact> = block
                        .preds
                        .iter()
                        .map(|pred_id| self.out_facts.get(pred_id).unwrap())
                        .collect();
                    // Special case for entry. Since empty has no predecessors but we sometimes have information in entry we do not want to throwaway
                    // That is the case for Dominator analysis
                    let new_in = if !pred_outs.is_empty() {
                        self.analysis.combine(&pred_outs)
                    } else {
                        self.in_facts.get(&block_id).unwrap().clone()
                    };
                    self.in_facts.insert(block_id, new_in.clone());

                    // Now apply Block transfer function to get new OUT for this block
                    let new_out = self.flow_block(block_id, &new_in, &block.lab_instructions);

                    // 3. If the OUT fact changed, add successors to the worklist.
                    let old_out = self.out_facts.get_mut(&block_id).unwrap();
                    if new_out != *old_out {
                        *old_out = new_out;
                        for succ_id in &block.succs {
                            if !worklist.contains(succ_id) {
                                worklist.push(*succ_id);
                            }
                        }
                    }
                }
                Direction::Backwards => {
                    // Combine all In Facts from Incoming nodes to get Out fact of current node
                    let succs_ins: Vec<&A::Fact> = block
                        .succs
                        .iter()
                        .map(|succ_id| self.in_facts.get(succ_id).unwrap())
                        .collect();
                    // See comment in forward. Special case for exit
                    let new_out = if !succs_ins.is_empty() {
                        self.analysis.combine(&succs_ins)
                    } else {
                        self.out_facts.get(&block_id).unwrap().clone()
                    };

                    self.out_facts.insert(block_id, new_out.clone());

                    // Now apply Block transfer function to get new OUT for this block
                    let new_in = self.flow_block(block_id, &new_out, &block.lab_instructions);
                    // 3. If the IN fact changed, add predecessors to the worklist.
                    let old_in = self.in_facts.get_mut(&block_id).unwrap();
                    if new_in != *old_in {
                        *old_in = new_in; // Actually change the fact
                        for pred_id in &block.preds {
                            if !worklist.contains(pred_id) {
                                worklist.push(*pred_id);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Applies the transfer function across all instructions in a block.
    ///
    /// For forward analyses, folds left-to-right. For backward, folds right-to-left.
    /// For empty blocks, delegates to [`Analysis::empty_flow`].
    fn flow_block(
        &mut self,
        block_id: usize,
        initial_fact: &A::Fact,
        lab_instrs: &[(usize, W::Instruction)],
    ) -> A::Fact {
        // For empty Block flows. Mostly just copies the given fact. However, Dominator still need it and add the block_id to facts
        if lab_instrs.is_empty() {
            let new_fact = self.analysis.empty_flow(initial_fact, block_id);
            return new_fact;
        }
        match A::direction() {
            Direction::Forwards => lab_instrs
                .iter()
                .fold(initial_fact.clone(), |fact, (i, instr)| {
                    self.analysis.flow(&fact, (*i, instr))
                }),
            Direction::Backwards => {
                // Fold from end to start
                lab_instrs
                    .iter()
                    .rfold(initial_fact.clone(), |fact, (i, instr)| {
                        self.analysis.flow(&fact, (*i, instr))
                    })
            }
        }
    }

    /// Computes the in-fact for every instruction in a block.
    pub fn instruction_in_facts_map(&self, block_id: usize) -> HashMap<usize, A::Fact> {
        let block = self.cfg.get_block(block_id);
        match A::direction() {
            Direction::Forwards => {
                let initial_fact = self.block_in(block_id);
                let (map, _) = block.lab_instructions.iter().fold(
                    (HashMap::new(), initial_fact.clone()),
                    |(mut map, fact), (i, instr)| {
                        // Store In Fact for current isntruction
                        map.insert(*i, fact.clone());
                        let next_fact = self.analysis.flow(&fact, (*i, instr));
                        (map, next_fact)
                    },
                );
                map
            }
            Direction::Backwards => {
                let initial_fact = self.block_out(block_id);
                let (map, _) = block.lab_instructions.iter().rfold(
                    (HashMap::new(), initial_fact.clone()),
                    |(mut map, fact), (i, instr)| {
                        // Compute IN from Given OUT
                        let in_fact = self.analysis.flow(&fact, (*i, instr));
                        map.insert(*i, in_fact.clone());
                        (map, in_fact)
                    },
                );
                map
            }
        }
    }
    // Computes a map from instruction index to Out_fact for entire block

    fn instruction_out_facts_map(&self, block_id: usize) -> HashMap<usize, A::Fact> {
        let block = self.cfg.get_block(block_id);

        match A::direction() {
            Direction::Forwards => {
                let initial_fact = self.block_out(block_id);
                let (map, _) = block.lab_instructions.iter().rfold(
                    (HashMap::new(), initial_fact.clone()),
                    |(mut map, fact), (i, instr)| {
                        // Compute OUT Fact for current instruction
                        let out_fact = self.analysis.flow(&fact, (*i, instr));
                        map.insert(*i, fact.clone());
                        (map, out_fact)
                    },
                );
                map
            }
            Direction::Backwards => {
                let initial_fact = self.block_out(block_id);
                let (map, _) = block.lab_instructions.iter().rfold(
                    (HashMap::new(), initial_fact.clone()),
                    |(mut map, fact), (i, instr)| {
                        map.insert(*i, fact.clone());
                        // Compute IN from Given OUT. This is the out of the previous instruction
                        let in_fact = self.analysis.flow(&fact, (*i, instr));
                        (map, in_fact)
                    },
                );
                map
            }
        }
    }

    pub fn block_in(&self, block_id: usize) -> &A::Fact {
        self.in_facts.get(&block_id).unwrap()
    }

    pub fn block_out(&self, block_id: usize) -> &A::Fact {
        self.out_facts.get(&block_id).unwrap()
    }

    /// Consumes the solver and returns [`AnalysisResults`] with per-instruction facts.
    pub fn into_results(self) -> AnalysisResults<A> {
        // Precompute all results
        let mut instr_in = HashMap::new();
        let mut instr_out = HashMap::new();
        for block_id in self.cfg.blocks.keys() {
            let instr_in_facts = self.instruction_in_facts_map(*block_id);
            let mut instr_out_facts = self.instruction_out_facts_map(*block_id);
            // Same keys
            for (instr_id, in_fact) in instr_in_facts.into_iter() {
                instr_in.insert((*block_id, instr_id), in_fact);
                let out_fact = instr_out_facts
                    .remove(&instr_id)
                    .expect("Key has to exist!");
                instr_out.insert((*block_id, instr_id), out_fact);
            }
        }
        AnalysisResults {
            in_facts: self.in_facts,
            out_facts: self.out_facts,
            instr_in,
            instr_out,
            // analysis: self.analysis,
        }
    }
}

impl<A: Analysis> AnalysisResults<A> {
    /// Returns the in-fact for a basic block.
    pub fn block_in(&self, block_id: usize) -> &A::Fact {
        self.in_facts.get(&block_id).unwrap()
    }

    /// Returns the out-fact for a basic block.
    pub fn block_out(&self, block_id: usize) -> &A::Fact {
        self.out_facts.get(&block_id).unwrap()
    }

    /// Returns the in-fact for a specific instruction.
    pub fn instr_in(&self, block_id: usize, instr_id: usize) -> &A::Fact {
        self.instr_in.get(&(block_id, instr_id)).unwrap()
    }

    /// Returns the out-fact for a specific instruction.
    pub fn instr_out(&self, block_id: usize, instr_id: usize) -> &A::Fact {
        self.instr_out.get(&(block_id, instr_id)).unwrap()
    }

    // ALl instruction facts for the block
    pub fn block_instr_in(&self, block_id: usize) -> HashMap<usize, &A::Fact> {
        let mut block_instr_in = HashMap::new();

        for ((block_id_in, instr_id), fact) in &self.instr_in {
            if block_id != *block_id_in {
                continue;
            }
            block_instr_in.insert(*instr_id, fact);
        }
        block_instr_in
    }

    pub fn block_instr_out(&self, block_id: usize) -> HashMap<usize, &A::Fact> {
        let mut block_instr_out = HashMap::new();

        for ((block_id_out, instr_id), fact) in &self.instr_out {
            if block_id != *block_id_out {
                continue;
            }
            block_instr_out.insert(*instr_id, fact);
        }
        block_instr_out
    }
}

impl<A: Analysis> fmt::Display for AnalysisResults<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut keys: Vec<_> = self.instr_in.keys().cloned().collect();
        keys.sort();

        for (block_id, instr_id) in keys {
            let in_fact = self.instr_in(block_id, instr_id);
            let out_fact = self.instr_out(block_id, instr_id);
            writeln!(f, "IN: {:?}", in_fact)?;
            writeln!(f, "Instruction ID: {}", instr_id)?;
            writeln!(f, "Out: {:?}", out_fact)?;
        }
        Ok(())
    }
}
