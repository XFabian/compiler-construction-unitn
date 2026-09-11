//! Live variable analysis and dead code elimination.
//!
//! A variable is *live* at a program point if its current value may be
//! read before being overwritten. This is a **backward may** analysis:
//! facts flow from successors to predecessors, and facts are combined
//! with set union at join points.
//!
//! The analysis results are used by [`dce`] to remove instructions
//! that define a variable which is never read afterwards.

use std::collections::{HashMap, HashSet};

use tracing::{debug, instrument};

use crate::opt::cfg::Cfg;
use crate::opt::solver::{Analysis, AnalysisResults};
use crate::wtac::wtac_ast as W;

/// Returns the set of variables used (read) by an instruction.
pub fn uses_of_instr(instr: &W::Instruction) -> Vec<String> {
    let mut uses = Vec::new();
    let mut extract_var = |v: &W::Value| {
        if let W::Value::Var(v) = v {
            uses.push(v.clone());
        }
        // tmp.25 : i64 = [tmp.22 + 0]
        // Using memory
        if let W::Value::Mem { name: v, .. } = v {
            uses.push(v.clone());
        }
    };
    // Array accesses
    // [tmp.7 + 0] : i64 = 3:i64
    // Here tmp 7 is a use!
    let extract_mem = |v: &W::Value, uses: &mut Vec<String>| {
        if let W::Value::Mem { name: v, .. } = v {
            uses.push(v.clone());
        }
    };
    match instr {
        W::Instruction::Return(Some(v)) => extract_var(v),
        W::Instruction::Unary { src, .. } => extract_var(src),
        W::Instruction::Binary { src1, src2, .. } => {
            extract_var(src1);
            extract_var(src2);
        }
        W::Instruction::Copy { src, dst, .. } => {
            extract_var(src);
            extract_mem(dst, &mut uses);
        }
        W::Instruction::FCall { args, .. } => {
            for arg in args {
                extract_var(arg)
            }
        }
        W::Instruction::BrIf { cond, .. } => extract_var(cond),
        _ => {}
    };

    uses
}

/// Returns the variable defined (killed) by an instruction, if any.
pub fn def_of_instr(instr: &W::Instruction) -> Option<String> {
    match instr {
        W::Instruction::Unary { dst, .. }
        | W::Instruction::Binary { dst, .. }
        | W::Instruction::Copy { dst, .. }
        | W::Instruction::FCall { dst: Some(dst), .. } => {
            match dst {
                W::Value::Var(v) => Some(v.clone()),
                _ => None, //  I may need to add stores here as well. For now works without as well
            }
        }
        _ => None,
    }
}

/// Backward may analysis: computes the set of live variables at each program point.
pub struct LivenessAnalysis;
// Probably dont need to own the strings inside here &'a str may be better
impl Analysis for LivenessAnalysis {
    /// The set of variable names that are live.
    type Fact = HashSet<String>;

    /// Union of all successor in-facts (may analysis).
    fn combine(&self, facts: &[&Self::Fact]) -> Self::Fact {
        let mut result = HashSet::new();
        for fact in facts {
            result.extend(fact.iter().cloned());
        }
        result
    }
    /// `IN = (OUT - def) ∪ use`
    ///
    /// Removes the defined variable from the live set, then adds all used variables.
    fn flow(&self, out_fact: &Self::Fact, lab_instr: (usize, &W::Instruction)) -> Self::Fact {
        let mut in_fact = out_fact.clone();

        // KILLS defined variable from fact
        if let Some(def) = def_of_instr(lab_instr.1) {
            in_fact.remove(&def);
        }
        // GEN set
        let used_vars = uses_of_instr(lab_instr.1);
        in_fact.extend(used_vars);
        in_fact
    }

    fn direction() -> super::solver::Direction {
        super::solver::Direction::Backwards
    }

    fn initial_fact(&self) -> Self::Fact {
        HashSet::new()
    }

    /// Seeds the exit block with global variables, since they are
    /// considered live after the function returns.
    fn init_facts(&self, cfg: &Cfg) -> (HashMap<usize, Self::Fact>, HashMap<usize, Self::Fact>) {
        let mut in_facts = HashMap::new();
        let mut out_facts: HashMap<usize, HashSet<String>> = HashMap::new();

        // Compute initial facts for each block
        for block_id in cfg.blocks.keys() {
            in_facts.insert(*block_id, self.initial_fact());
            out_facts.insert(*block_id, self.initial_fact());
        }

        // add globals to out facts of exit since backwards
        let exit_id = cfg.get_exit();
        let exit_out = out_facts.get_mut(&exit_id).unwrap();
        for glob_name in cfg.globals.keys() {
            exit_out.insert(glob_name.clone());
        }
        (in_facts, out_facts)
    }
}

/// Removes instructions that define a variable which is dead after that point.
///
/// Returns `true` if any instructions were removed (the caller should re-run
/// the analysis since facts may have changed).
#[instrument(skip(cfg, res_ana), level = "Debug")]
pub fn dce(cfg: &mut Cfg, res_ana: &AnalysisResults<LivenessAnalysis>) -> bool {
    let mut changed = false;
    for (block_id, block) in cfg.blocks.iter_mut() {
        // let instr_facts = res_ana.block_instr_out(*block_id);

        // Remove elements
        block.lab_instructions.retain(|(instr_id, instr)| {
            let out_fact = res_ana.instr_out(*block_id, *instr_id);
            // println!("Liveness Fact at Instruction {} : {:?}", instr_id, out_fact);
            if let Some(var) = def_of_instr(instr) {
                // If it is not in live set afterwards the var is never used!
                // println!("DCE: At instruction : {}", instr);
                if !out_fact.contains(&var) {
                    changed = true;
                    debug!("Deleting {}", instr);
                    // false removes it in retain
                    return false;
                }
            }
            true
        });
    }

    changed
}
