//! Reaching definitions analysis, use-def/def-use chains, and related optimizations.
//!
//! A definition *reaches* a program point if there is a path from the definition
//! to that point along which the variable is not redefined. This is a **forward may**
//! analysis.
//!
//! The analysis results are used to build use-def and def-use chains, which
//! in turn drive constant propagation and copy propagation.

use std::collections::{HashMap, HashSet};

use tracing::{debug, instrument};

use crate::opt::cfg::Cfg;
use crate::opt::liveness;
use crate::opt::solver::{Analysis, AnalysisResults};
use crate::wtac::wtac_ast::{self as W, BinaryOp, UnaryOp, Value};

/// A definition: `(instruction_id, variable_name)`.
type Def = (usize, String);

/// Forward may analysis: computes which definitions reach each program point.
pub struct ReachingAnalysis;
impl Analysis for ReachingAnalysis {
    /// Set of definitions `(instruction_id, variable_name)` that reach this point.
    type Fact = HashSet<Def>;

    /// Union of all predecessor out-facts (may analysis).
    fn combine(&self, facts: &[&Self::Fact]) -> Self::Fact {
        let mut result = HashSet::new();
        for fact in facts {
            result.extend(fact.iter().cloned());
        }
        result
    }
    /// Kills all previous definitions of the same variable, then adds the
    /// new definition.
    fn flow(&self, out_fact: &Self::Fact, lab_instr: (usize, &W::Instruction)) -> Self::Fact {
        let mut out_fact = out_fact.clone();

        // KILLS Remove all previouise defs of variable
        if let Some(def) = liveness::def_of_instr(lab_instr.1) {
            out_fact.retain(|(_, var)| *var != def);
        }
        // GEN : Add new definitions
        if let Some(var) = liveness::def_of_instr(lab_instr.1) {
            out_fact.insert((lab_instr.0, var));
        }
        out_fact
    }

    fn direction() -> super::solver::Direction {
        super::solver::Direction::Forwards
    }

    fn initial_fact(&self) -> Self::Fact {
        HashSet::new()
    }

    /// Seeds the entry block with globals and function parameters as initial definitions.
    fn init_facts(&self, cfg: &Cfg) -> (HashMap<usize, Self::Fact>, HashMap<usize, Self::Fact>) {
        let mut in_facts = HashMap::new();
        let mut out_facts = HashMap::new();

        // Compute initial facts for each block
        for block_id in cfg.blocks.keys() {
            in_facts.insert(*block_id, self.initial_fact());
            out_facts.insert(*block_id, self.initial_fact());
        }

        // add globals to in facts of entry.
        // Function arguments are also added here.
        let entry_id = cfg.get_entry();
        let entry_in = in_facts.get_mut(&entry_id).unwrap();
        for param_name in &cfg.params {
            entry_in.insert((0, param_name.clone()));
        }
        for glob_name in cfg.globals.keys() {
            entry_in.insert((0, glob_name.clone()));
        }
        (in_facts, out_facts)
    }
}

/// Maps each use to the set of definitions that may reach it.
pub type UseDef = HashMap<Def, HashSet<Def>>;
/// Maps each definition to the set of uses it may reach.
pub type DefUse = HashMap<Def, HashSet<Def>>;

/// Builds use-def and def-use chains from the reaching definitions results.
pub fn build_use_def(cfg: &Cfg, res_ana: &AnalysisResults<ReachingAnalysis>) -> (UseDef, DefUse) {
    let mut use_def: UseDef = HashMap::new();
    let mut def_use: DefUse = HashMap::new();
    for (block_id, block) in &cfg.blocks {
        for (instr_id, instr) in &block.lab_instructions {
            let reaching_defs = res_ana.instr_in(*block_id, *instr_id);
            // println!("Reaching Fact at Instruction {}: {:?}", instr_id, reaching_defs);
            let uses = liveness::uses_of_instr(instr);
            for var in uses {
                let use_key = (*instr_id, var.clone());

                let var_defs: Vec<_> = reaching_defs
                    .iter()
                    .filter(|(_, v)| *v == *var)
                    .cloned()
                    .collect();
                use_def
                    .entry(use_key.clone())
                    .or_default()
                    .extend(var_defs.iter().cloned());

                for def in &var_defs {
                    def_use
                        .entry(def.clone())
                        .or_default()
                        .insert(use_key.clone());
                }
            }

            // Handle defintions
            if let Some(var) = liveness::def_of_instr(instr) {
                let def_key = (*instr_id, var);
                def_use.entry(def_key).or_default();
            }
        }
    }

    (use_def, def_use)
}

/// Collects all instructions that assign a constant value, returning
/// a map from instruction ID to the constant [`Value`].
fn get_constants(cfg: &Cfg) -> HashMap<usize, W::Value> {
    let mut constant_map = HashMap::new();
    for block in cfg.blocks.values() {
        for (i, instr) in &block.lab_instructions {
            if let W::Instruction::Copy {
                src: v @ W::Value::Imm(_, _),
                ..
            } = instr
            {
                constant_map.insert(*i, v.clone());
            }
        }
    }

    constant_map
}

/// Rewrites an instruction in place, substituting all occurrences of
/// `sub_name` with `sub_val` in source operands.
///
/// Handles variables, memory operands (combining offsets when substituting
/// a `Mem` into another `Mem`), function call arguments, and branch conditions.
pub fn rewrite_instr(instr: &mut W::Instruction, sub_name: &String, sub_val: &W::Value) {
    let subst_val = |old_var: &mut Value| {
        match old_var {
            W::Value::Var(x) if x == sub_name => {
                *old_var = sub_val.clone();
            }
            W::Value::Mem {
                name: base, offset, ..
            } if base == sub_name => {
                // Need to combine offsets here if also mem
                match sub_val {
                    W::Value::Var(new_name) => *base = new_name.clone(), // Just replace name
                    W::Value::Mem {
                        name: new_base,
                        offset: new_offset,
                        ..
                    } => {
                        *base = new_base.clone();
                        *offset += new_offset; // extend offset
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    };
    match instr {
        W::Instruction::Return(Some(val)) => subst_val(val),
        W::Instruction::Unary { src, .. } => subst_val(src),
        W::Instruction::Binary { src1, src2, .. } => {
            subst_val(src1);
            subst_val(src2);
        }
        W::Instruction::Copy { src, dst, .. } => {
            subst_val(src);
            subst_val(dst);
        }
        W::Instruction::FCall { args, .. } => args.iter_mut().for_each(subst_val),
        W::Instruction::BrIf { cond, .. } => subst_val(cond),
        // Block and Loop are unfolded in our CFG. So we do not need to care about them
        _ => (),
    }
}

/// Replaces variable uses with constant values when all reaching definitions
/// assign the same constant.
#[instrument(skip(cfg, use_def), level = "Debug")]
pub fn constant_propagation(cfg: &mut Cfg, use_def: &UseDef) -> bool {
    let mut changed = false;
    let constant_map = get_constants(cfg);
    for (_, block) in cfg.blocks.iter_mut() {
        for (i, instr) in block.lab_instructions.iter_mut() {
            let uses = liveness::uses_of_instr(instr);
            for var in uses {
                let use_key = (*i, var.clone());
                if let Some(defs) = use_def.get(&use_key) {
                    // check these definitions of var if they are constant
                    // and if they define the same constant
                    let cons_vec: Vec<&W::Value> = defs
                        .iter()
                        .filter_map(|(def_id, _)| constant_map.get(def_id))
                        .collect();
                    // Number of denfitoins and constants match and they store the same constant value! So type matches as well
                    if !cons_vec.is_empty()
                        && cons_vec.len() == defs.len()
                        && cons_vec.iter().all(|&v| *v == *cons_vec[0])
                    {
                        let new_val = cons_vec[0];
                        let old_instr = instr.clone();
                        rewrite_instr(instr, &var, new_val);
                        debug!("{} -> {}", old_instr, instr);
                        changed = true;
                    }
                }
            }
        }
    }
    changed
}

/// Replaces variable uses with the source of a copy when all reaching definitions
/// are copies from the same variable.
#[instrument(skip(cfg, use_def), level = "Debug")]
pub fn copy_propagation(cfg: &mut Cfg, use_def: &UseDef) -> bool {
    let mut changed = false;
    let copy_map = build_copy_map(cfg);
    for (_, block) in cfg.blocks.iter_mut() {
        for (i, instr) in block.lab_instructions.iter_mut() {
            let uses = liveness::uses_of_instr(instr);
            for var in uses {
                let use_key = (*i, var.clone());
                if let Some(defs) = use_def.get(&use_key) {
                    let copy_vec: Vec<&String> = defs
                        .iter()
                        .filter_map(|(def_id, _)| copy_map.get(def_id))
                        .collect();
                    // Number of denfitoins and copies match and they are all copies from the same Source!
                    if !copy_vec.is_empty()
                        && copy_vec.len() == defs.len()
                        && copy_vec.iter().all(|&v| *v == *copy_vec[0])
                        && var != *copy_vec[0]
                    // Otherwise a = a is substituted infinitely
                    {
                        debug!(
                            "Instruction: {}:{}. Replacing {} With {}",
                            i, instr, var, copy_vec[0]
                        );
                        let old_instr = instr.clone();
                        rewrite_instr(instr, &var, &W::Value::Var(copy_vec[0].clone()));
                        debug!("{} -> {}", old_instr, instr);
                        changed = true;
                    }
                }
            }
        }
    }
    changed
}

/// Builds a map from instruction ID to the source variable name for all
/// simple copy instructions (e.g. `x = y`).
///
/// Used by [`copy_propagation`] to determine what to substitute.
fn build_copy_map(cfg: &Cfg) -> HashMap<usize, String> {
    let mut copy_map = HashMap::new();
    for block in cfg.blocks.values() {
        for (instr_id, instr) in &block.lab_instructions {
            if let W::Instruction::Copy {
                src: W::Value::Var(src_name),
                ..
            } = instr
            {
                copy_map.insert(*instr_id, src_name.clone());
            }
        }
    }
    copy_map
}

/// Replaces binary and unary operations on constant operands with their
/// computed result. Also simplifies identity operations like `x + 0` and `x - 0`.
///
/// Unlike [`constant_propagation`], this does not need use-def chains —
/// it just inspects each instruction's operands directly.
#[instrument(skip(cfg), level = "Debug")]
pub fn constant_folding(cfg: &mut Cfg) -> bool {
    let mut changed = false;

    for block in cfg.blocks.values_mut() {
        for (_, instr) in &mut block.lab_instructions {
            // We need a mutable reference to the instruction itself to replace it.
            let mut new_instr = None;

            match instr {
                // Fold Bianrty
                W::Instruction::Binary {
                    op,
                    src1: W::Value::Imm(i1, _),
                    src2: W::Value::Imm(i2, _),
                    dst,
                    t,
                } => {
                    if let Some(result) = perform_binary_op(op, *i1, *i2) {
                        new_instr = Some(W::Instruction::Copy {
                            src: W::Value::Imm(result, t.clone()),
                            dst: dst.clone(),
                            t: t.clone(),
                        });
                        changed = true;
                    };
                }
                // x + 0 / x - 0
                W::Instruction::Binary {
                    op: W::BinaryOp::Add | W::BinaryOp::Sub,
                    src1,
                    src2: W::Value::Imm(0, _),
                    dst,
                    t,
                } => {
                    // Replace the Binary instruction with a simple Copy.
                    new_instr = Some(W::Instruction::Copy {
                        src: src1.clone(),
                        dst: dst.clone(),
                        t: t.clone(),
                    });
                    changed = true;
                }
                _ => (),
            }

            // --- Fold Unary Operations ---
            if let W::Instruction::Unary { op, src, dst, t } = instr
                && let W::Value::Imm(i, _) = src
                && let Some(result) = perform_unary_op(op, *i)
            {
                // You need to implement this
                new_instr = Some(W::Instruction::Copy {
                    src: W::Value::Imm(result, t.clone()),
                    dst: dst.clone(),
                    t: t.clone(),
                });
                changed = true;
            }

            if let Some(replacement) = new_instr {
                debug!("{} -> {}", instr, replacement);
                *instr = replacement;
            }
        }
    }
    changed
}

/// Evaluates a binary operation on two constant operands at compile time.
///
/// Returns `None` when the instruction would trap at run time, so it is left in
/// place to trap there. Arithmetic wraps, because `i64.add` wraps.
fn perform_binary_op(op: &BinaryOp, i1: i64, i2: i64) -> Option<i64> {
    match op {
        BinaryOp::Add => Some(i1.wrapping_add(i2)),
        BinaryOp::Sub => Some(i1.wrapping_sub(i2)),
        BinaryOp::Mult => Some(i1.wrapping_mul(i2)),
        BinaryOp::Div => {
            if i2 == 0 || (i1 == i64::MIN && i2 == -1) {
                None
            } else {
                Some(i1 / i2)
            }
        }
        BinaryOp::Mod => {
            if i2 == 0 {
                None
            } else {
                // i64::MIN % -1 is 0 in WASM but overflows Rust's `%`.
                Some(i1.wrapping_rem(i2))
            }
        }
        BinaryOp::And => Some(((i1 != 0) && (i2 != 0)) as i64),
        BinaryOp::Or => Some(((i1 != 0) || (i2 != 0)) as i64),
        BinaryOp::Lt => Some((i1 < i2) as i64),
        BinaryOp::Leq => Some((i1 <= i2) as i64),
        BinaryOp::Gt => Some((i1 > i2) as i64),
        BinaryOp::Geq => Some((i1 >= i2) as i64),
        BinaryOp::Eq => Some((i1 == i2) as i64),
        BinaryOp::Neq => Some((i1 != i2) as i64),
    }
}

/// Evaluates a unary operation on a constant operand at compile time.
///
/// Returns `None` if the wrap would overflow an `i32`.
fn perform_unary_op(op: &UnaryOp, i1: i64) -> Option<i64> {
    match op {
        // Wrapping, because negating i64::MIN is representable in WASM but not in Rust.
        UnaryOp::Neg => Some(i1.wrapping_neg()),
        // if 1 its false so 0 then
        UnaryOp::Not => Some((i1 == 0) as i64),
        UnaryOp::Wrap => {
            if i1 < i32::MIN as i64 || i1 > i32::MAX as i64 {
                None
            } else {
                Some(i1 as i32 as i64)
            }
        }
    }
}

/// Folding must produce exactly what the instruction it replaces would have
/// produced at run time. The cases below cannot be written in Eta -- there is no
/// negative literal, so `i64::MIN` is unreachable from a source program -- which
/// is why they are pinned here rather than in an `.eta` test.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_wraps_like_wasm() {
        assert_eq!(
            perform_binary_op(&BinaryOp::Add, i64::MAX, 1),
            Some(i64::MIN)
        );
        assert_eq!(
            perform_binary_op(&BinaryOp::Sub, i64::MIN, 1),
            Some(i64::MAX)
        );
        assert_eq!(perform_binary_op(&BinaryOp::Mult, i64::MAX, 2), Some(-2));
        assert_eq!(perform_unary_op(&UnaryOp::Neg, i64::MIN), Some(i64::MIN));
    }

    #[test]
    fn negation_uses_its_operand() {
        assert_eq!(perform_unary_op(&UnaryOp::Neg, 7), Some(-7));
        assert_eq!(perform_unary_op(&UnaryOp::Neg, 0), Some(0));
    }

    /// `i64.div_s` traps on both, so folding must decline and let it trap.
    #[test]
    fn trapping_division_is_not_folded() {
        assert_eq!(perform_binary_op(&BinaryOp::Div, 1, 0), None);
        assert_eq!(perform_binary_op(&BinaryOp::Div, i64::MIN, -1), None);
    }

    /// `i64.rem_s` does not trap here -- it yields 0 -- so this one may fold.
    #[test]
    fn modulo_by_minus_one_folds_to_zero() {
        assert_eq!(perform_binary_op(&BinaryOp::Mod, i64::MIN, -1), Some(0));
        assert_eq!(perform_binary_op(&BinaryOp::Mod, 1, 0), None);
    }

    #[test]
    fn and_is_not_or() {
        assert_eq!(perform_binary_op(&BinaryOp::And, 0, 1), Some(0));
        assert_eq!(perform_binary_op(&BinaryOp::And, 1, 1), Some(1));
        assert_eq!(perform_binary_op(&BinaryOp::Or, 0, 1), Some(1));
        assert_eq!(perform_binary_op(&BinaryOp::Or, 0, 0), Some(0));
    }

    /// Division truncates toward zero and `%` takes the sign of the dividend,
    /// matching `div_s` / `rem_s`; `div_mod_negative.eta` checks the same values
    /// end to end.
    #[test]
    fn division_truncates_toward_zero() {
        assert_eq!(perform_binary_op(&BinaryOp::Div, -7, 2), Some(-3));
        assert_eq!(perform_binary_op(&BinaryOp::Mod, -7, 2), Some(-1));
        assert_eq!(perform_binary_op(&BinaryOp::Div, 7, -2), Some(-3));
        assert_eq!(perform_binary_op(&BinaryOp::Mod, 7, -2), Some(1));
    }
}
