//! Available copies analysis and copy propagation.
//!
//! A copy `d = s` is *available* at a program point if on every path from the
//! entry to that point the copy was executed and neither `d` nor `s` was
//! assigned afterwards. This is a **forward must** analysis: facts flow from
//! predecessors to successors, and facts are combined with set intersection
//! at join points.
//!
//! The analysis results are used by [`copy_propagation`] to replace a use of
//! `d` with `s` wherever the copy `d = s` is available.
//!
//! Reaching definitions cannot answer this question: it can show that every
//! definition of `d` reaching a use is `d = s`, but not that `s` is unchanged
//! since the copy.

use std::collections::{HashMap, HashSet};

use tracing::{debug, instrument};

use crate::{opt::{cfg::Cfg, liveness, reaching::rewrite_instr, solver::{Analysis, AnalysisResults}}, wtac::wtac_ast::{Instruction, Value::Var}};
use crate::wtac::wtac_ast::{self as W};

/// Forward must analysis: computes which copies are available at each program point.
///
/// It holds the names of the globals, because a call may write any of them.
#[derive(Debug, Clone)]
pub struct AvailableCopiesAnalysis {
    globals : HashSet<String>,
}

impl AvailableCopiesAnalysis {

    /// Takes the globals from `cfg`.
    pub fn new(cfg : &Cfg) -> Self {
        AvailableCopiesAnalysis { globals: cfg.globals.keys().cloned().collect() }
    }
}

impl Analysis for AvailableCopiesAnalysis {
    /// Set of available copies `(dst, src)`, meaning `dst == src` holds here.
    /// There is at most one pair per `dst` in reachable code.
    type Fact = HashSet<(String, String)>;

    /// Intersection of all predecessor out-facts (must analysis).
    fn combine(&self, facts: &[&Self::Fact]) -> Self::Fact {
         let mut all_set = HashSet::new();

        // Start with the union of all facts, then keep only the copies that
        // are in every fact.
        for fact in facts {
            all_set.extend(fact.iter().cloned());
        }
        for fact in facts {
            all_set.retain(|x| fact.contains(x));
        }
        all_set
    }


    /// `OUT = (IN - kill) ∪ gen`
    ///
    /// Kills every copy that mentions the defined variable, and at a call every
    /// copy that mentions a global. Then adds the copy `d = s` itself.
    fn flow(&self, out_fact: &Self::Fact, lab_instr: (usize, &W::Instruction)) -> Self::Fact {
        let mut out_fact = out_fact.clone();

        // KILLS at a call: the callee may write any global, so a copy (y, g)
        // or (g, y) with a global g may no longer hold afterwards.
        if let Instruction::FCall { .. } = lab_instr.1 {
            out_fact.retain(|(dst, src)| !self.globals.contains(dst) && !self.globals.contains(src));
        }
        // KILLS: every copy that mentions the defined variable, on either side.
        if let Some(def) = liveness::def_of_instr(lab_instr.1) {
            out_fact.retain(|(dst, src)| *dst != def && *src != def);
        }
        // GEN: a copy between two variables. Loads and stores are not copies.
        if let Instruction::Copy { src : Var(s), dst: Var(d), .. } = lab_instr.1 {
            // x = x says nothing, and propagating it would never stop.
            if s != d {
                out_fact.insert((d.clone(), s.clone()));
            }
        }
        out_fact
    }

    fn direction() -> super::solver::Direction {
        super::solver::Direction::Forwards
    }

    fn initial_fact(&self) -> Self::Fact {
        HashSet::new()
    }

    /// Seeds the out-fact of every block except the entry with all copies of
    /// the function, and the entry with the empty set.
    ///
    /// Must analyses start from the top: with the empty set, a loop header
    /// would intersect with the empty out-fact of its back edge and never
    /// keep a copy. The iteration removes the copies that do not hold.
    fn init_facts(&self, cfg: &Cfg) -> (HashMap<usize, Self::Fact>, HashMap<usize, Self::Fact>) {
        let mut in_facts = HashMap::new();
        let mut out_facts = HashMap::new();

        let all_copies = all_copies(cfg);

        // The in-facts are recomputed from the predecessors, except at the entry.
        for block_id in cfg.blocks.keys() {
            in_facts.insert(*block_id, self.initial_fact());
            let out = if *block_id == cfg.get_entry() { HashSet::new() } else { all_copies.clone() };
            out_facts.insert(*block_id, out);
        }

        (in_facts, out_facts)
    }
}

/// Replaces a use of `d` with `s` wherever the copy `d = s` is available.
///
/// Returns `true` if any instruction was rewritten.
#[instrument(skip(cfg, res), level = "Debug")]
pub fn copy_propagation(cfg: &mut Cfg, res: &AnalysisResults<AvailableCopiesAnalysis>) -> bool {
    let mut changed = false;

    for (block_id, block) in cfg.blocks.iter_mut() {
        for (i, instr) in block.lab_instructions.iter_mut() {
            let uses = liveness::uses_of_instr(instr);
            let facts = res.instr_in(*block_id, *i);
            for var in uses {
                // A pair (d, s) means d == s, so a use of d is replaced by s.
                if let Some((_, src)) = facts.iter().find(|(dst, _)| *dst == var) {
                    debug!(
                        "Instruction: {}:{}. Replacing {} With {}",
                        i, instr, var, src
                    );
                    let old_instr = instr.clone();
                    rewrite_instr(instr, &var, &W::Value::Var(src.clone()));
                    debug!("{} -> {}", old_instr, instr);
                    changed = true;
                }
            }
        }
    }
    changed
}

/// Returns every copy `(dst, src)` between two variables in the function: the
/// start value of the must analysis.
fn all_copies(cfg : &Cfg) -> HashSet<(String, String)> {
    let mut all_copies = HashSet::new();
    for block in cfg.blocks.values() {
        for (_, instr) in &block.lab_instructions {
            if let W::Instruction::Copy {
                src: W::Value::Var(src_name),
                dst: W::Value::Var(dst_name),
                ..
            } = instr
            {
                if src_name != dst_name {
                    all_copies.insert((dst_name.clone(), src_name.clone()));
                }
            }
        }
    }
    all_copies
}