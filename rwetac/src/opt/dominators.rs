//! Dominator and post-dominator analysis.
//!
//! A node `d` *dominates* a node `n` if every path from the entry to `n`
//! passes through `d`. A node `d` *post-dominates* `n` if every path from
//! `n` to the exit passes through `d`.
//!
//! Both are **forward/backward must** analyses (intersection at join points).
//! The results are used to compute immediate dominators, which are needed
//! by [`reconstruct_cfg`](super::reconstruct_cfg) to recover structured
//! control flow from the optimized CFG.
use std::collections::{HashMap, HashSet};

use crate::opt::cfg;
use crate::opt::cfg::Cfg;
use crate::opt::solver::{Analysis, Solver};
use crate::wtac::wtac_ast as W;

/// Forward must analysis: computes the set of blocks that dominate each block.
pub struct DominatorAnalysis;

// Empty Blocks that have predecssors do have themselves for domination. I do not think that this is a problem.
// Well this means as well that they do not dominate other blocks as well
// Entry is sepcial and correctly handled
impl Analysis for DominatorAnalysis {
    /// Set of block IDs that dominate this block.
    type Fact = HashSet<usize>;

    /// Intersection of all predecessor facts (must analysis).
    fn combine(&self, facts: &[&Self::Fact]) -> Self::Fact {
        let mut all_set = HashSet::new();

        // Step 1: start with a union of all sets as the initial set
        for fact in facts {
            all_set.extend(fact.iter().cloned());
        }
        // This is now the intersection. So we retain only the domniators that are in each of the predecessors!
        for fact in facts {
            all_set.retain(|x| fact.contains(x));
        }
        // Then intersect them all
        all_set
    }
    /// Adds the current block to its own dominator set.
    fn flow(&self, out_fact: &Self::Fact, lab_instr: (usize, &W::Instruction)) -> Self::Fact {
        let mut out_fact = out_fact.clone();
        let block_id = cfg::block_id_from_instr_id(lab_instr.0);
        out_fact.insert(block_id);
        out_fact
    }

    fn empty_flow(&self, out_fact: &Self::Fact, block_id: usize) -> Self::Fact {
        let mut out_fact = out_fact.clone();
        out_fact.insert(block_id);
        out_fact
    }

    fn direction() -> super::solver::Direction {
        super::solver::Direction::Forwards
    }

    fn initial_fact(&self) -> Self::Fact {
        HashSet::new()
    }

    /// Initializes entry with `{entry}` and all other blocks with the full
    /// set of block IDs (which is then narrowed during iteration).
    fn init_facts(
        &self,
        cfg: &cfg::Cfg,
    ) -> (
        std::collections::HashMap<usize, Self::Fact>,
        std::collections::HashMap<usize, Self::Fact>,
    ) {
        let mut in_facts = HashMap::new();
        let mut out_facts = HashMap::new();

        let all_blocks: HashSet<usize> = cfg.blocks.keys().cloned().collect();
        for &block_id in cfg.blocks.keys() {
            if block_id == cfg.get_entry() {
                in_facts.insert(block_id, HashSet::from([block_id]));
            } else {
                in_facts.insert(block_id, all_blocks.clone());
            }
            // Out needs to equal In. Because of loops!
            out_facts.insert(block_id, in_facts.get(&block_id).unwrap().clone());
        }
        (in_facts, out_facts)
    }
}

/// Backward must analysis: computes the set of blocks that post-dominate each block.
pub struct PostDominatorAnalysis;

impl Analysis for PostDominatorAnalysis {
    type Fact = HashSet<usize>;

    fn combine(&self, facts: &[&Self::Fact]) -> Self::Fact {
        let mut all_set = HashSet::new();

        // Step 1: start with a union of all sets as the initial set
        for fact in facts {
            all_set.extend(fact.iter().cloned()); //union
        }
        // This is now the intersection. So we retain only the domniators that are in each of the predecessors!
        for fact in facts {
            all_set.retain(|x| fact.contains(x)); //intersectoin
        }
        // Then intersect them all
        all_set
    }

    fn flow(&self, in_fact: &Self::Fact, lab_instr: (usize, &W::Instruction)) -> Self::Fact {
        let mut in_fact = in_fact.clone();
        let block_id = cfg::block_id_from_instr_id(lab_instr.0);
        in_fact.insert(block_id);
        in_fact
    }

    fn empty_flow(&self, in_fact: &Self::Fact, block_id: usize) -> Self::Fact {
        let mut in_fact = in_fact.clone();
        in_fact.insert(block_id);
        in_fact
    }

    fn direction() -> super::solver::Direction {
        super::solver::Direction::Backwards
    }

    fn initial_fact(&self) -> Self::Fact {
        HashSet::new()
    }

    // Exit Post dominates itself
    // Otherwise we init out here since it flows backwards
    fn init_facts(
        &self,
        cfg: &cfg::Cfg,
    ) -> (
        std::collections::HashMap<usize, Self::Fact>,
        std::collections::HashMap<usize, Self::Fact>,
    ) {
        let mut in_facts = HashMap::new();
        let mut out_facts = HashMap::new();

        let all_blocks: HashSet<usize> = cfg.blocks.keys().cloned().collect();
        for &block_id in cfg.blocks.keys() {
            if block_id == cfg.get_exit() {
                out_facts.insert(block_id, HashSet::from([block_id]));
            } else {
                out_facts.insert(block_id, all_blocks.clone());
            }

            in_facts.insert(block_id, out_facts.get(&block_id).unwrap().clone());
        }

        (in_facts, out_facts)
    }
}

/// Computes the immediate dominator for each block from the full dominator sets.
///
/// The immediate dominator of `n` is the unique strict dominator of `n` that
/// does not strictly dominate any other strict dominator of `n`.
pub fn immediate_dominators(doms: &HashMap<usize, HashSet<usize>>) -> HashMap<usize, usize> {
    let mut idoms: HashMap<usize, usize> = HashMap::new();
    // The entry node is the only node with one dominator
    // All other nodes are dominated by entry and themselves
    for (block_id, bl_dominators) in doms.iter() {
        // THis is the Entry (or Exit) node so we do nothing
        if bl_dominators.len() == 1 {
            idoms.insert(*block_id, *block_id);
            continue;
        }
        let strict_doms: Vec<_> = bl_dominators
            .iter()
            .filter(|&id| *block_id != *id)
            .collect();
        let idom = strict_doms
            .iter()
            .find(|&&candidate| {
                // The Candidate should not strictly dominate any other node in strict_doms
                strict_doms
                    .iter()
                    .filter(|&&other| other != candidate) // does not strictly dominate any other node...
                    .all(|&other| !doms[other].contains(candidate)) // So cannot be contained in Dom(other)
            })
            .expect("Every Node should have an immediate Dominator!");

        idoms.insert(*block_id, **idom);
    }
    idoms
}

/// Results of both dominator and post-dominator analyses
pub struct Dominators {
    /// Full dominator sets for each block.
    pub doms: HashMap<usize, HashSet<usize>>,
    /// Immediate dominator for each block.
    pub idoms: HashMap<usize, usize>,
    /// Immediate post-dominator for each block.
    pub ipdoms: HashMap<usize, usize>,
}

/// Runs both dominator and post-dominator analyses and computes immediate (post-)dominators.
pub fn compute_dominators(cfg: &Cfg) -> Dominators {
    let mut solver = Solver::new(cfg, DominatorAnalysis);
    solver.solve();
    // println!("Dominator Analysis: {:#?}", solver.out_facts);
    let idoms = immediate_dominators(&solver.out_facts);

    let mut solver_post = Solver::new(cfg, PostDominatorAnalysis);
    solver_post.solve();
    // println!("PostDominator Analysis: {:#?}", solver_post.in_facts);
    let ipdoms = immediate_dominators(&solver_post.in_facts);

    Dominators {
        doms: solver.out_facts,
        idoms,
        ipdoms,
    }
}
