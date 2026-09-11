//! Memory-related analyses and optimizations.
//!
//! This module contains two analyses and their corresponding optimizations:
//!
//! [`PointerAnalysis`] tracks which base pointer and offset each variable refers to
//! (a forward may analysis). [`pointer_propagation`] uses this to replace derived
//! pointer variables in memory operations with their canonical base + offset,
//! enabling further optimizations.
//!
//! [`AvailableLoadAnalysis`] tracks which memory locations have been loaded and are
//! still valid (a forward must analysis). [`store_to_load_forwarding`] uses this
//! to replace redundant loads with the previously loaded value.

use std::collections::HashMap;

use tracing::{debug, instrument, warn};

use crate::opt::cfg::Cfg;
use crate::opt::liveness;
use crate::opt::solver::{Analysis, AnalysisResults};
use crate::wtac::wtac_ast as W;

// tmp.11 : i32 = tmp.8 + 8:i64
// [tmp.10 + 0] : i64 = [tmp.11 + 0]
// tmp.18 : i32 = tmp.8 + 8:i64
// [tmp.18 + 0] : i64 = 6:i64
// Propagates Memory accesses on rhs
// offset aware copy propagation or redundant load elimination
// Before
// [base + 8] = 6;
// ... // (no other stores to [base+8])
// y = [base + 8];

// // After
// [base + 8] = 6;
// ...
// y = 6; // The load is eliminated

/// Forward may analysis: tracks pointer aliasing information.
///
/// Facts map variable names to `(canonical_base, byte_offset)`. When a variable
/// is derived from a pointer via addition, it inherits the base and accumulates
/// the offset. New pointers are introduced by `eta_malloc` calls.
pub struct PointerAnalysis;

impl Analysis for PointerAnalysis {
    // key derived pointer tmp.34
    // value tuple(base_name, offset) representing canonical form
    type Fact = HashMap<String, (String, usize)>;

    // intersection
    fn combine(&self, facts: &[&Self::Fact]) -> Self::Fact {
        if facts.is_empty() {
            return HashMap::new();
        }

        let mut result = facts[0].clone();
        result.retain(|loc, val| {
            // Keep only the key-value pairs that are present and identical in all other facts.
            facts
                .iter()
                .skip(1)
                .all(|other_fact| other_fact.get(loc) == Some(val))
        });

        result
    }

    // Kill instruction is the insert method
    // Since when a ptr var is reassigned insert kills the oldbinding
    fn flow(&self, in_fact: &Self::Fact, lab_instr: (usize, &W::Instruction)) -> Self::Fact {
        let mut out_fact = in_fact.clone();

        match lab_instr.1 {
            // Offset calculations dest = base + offset
            W::Instruction::Binary {
                op: W::BinaryOp::Add,
                src1: W::Value::Var(base),
                src2: W::Value::Imm(offset, _),
                dst: W::Value::Var(derived),
                ..
            } => {
                // check if base is a pointer
                if let Some((canonical, offset_base)) = out_fact.get(base) {
                    out_fact.insert(
                        derived.clone(),
                        (canonical.clone(), *offset_base + (*offset as usize)),
                    );
                }
            }
            W::Instruction::Copy {
                src: W::Value::Var(base),
                dst: W::Value::Var(derived),
                ..
            } => {
                if let Some((canonical, offset_base)) = out_fact.get(base) {
                    out_fact.insert(derived.clone(), (canonical.clone(), *offset_base));
                }
            }
            // Base case
            W::Instruction::FCall {
                name,
                dst: Some(val),
                ..
            } => {
                // New Pointer is created
                if name == "eta_malloc"
                    && let W::Value::Var(ptr) = val
                {
                    out_fact.insert(ptr.clone(), (ptr.clone(), 0));
                }
            }
            _ => (),
        }
        out_fact
    }

    fn direction() -> super::solver::Direction {
        super::solver::Direction::Forwards
    }

    fn initial_fact(&self) -> Self::Fact {
        HashMap::new()
    }
}

// Rewrites in place
// Similar to rewrite_instr in reaching.rs
pub fn rewrite_mem_instr(
    instr: &mut W::Instruction,
    sub_name: &str,
    sub_base_name: &str,
    sub_offset: usize,
) {
    let subst_mem = |old_var: &mut W::Value| {
        if let W::Value::Mem { name, offset, .. } = old_var
            && name == sub_name
        {
            *name = sub_base_name.to_string();
            *offset += sub_offset;
        }
    };
    match instr {
        W::Instruction::Unary { src, .. } => subst_mem(src),
        W::Instruction::Binary { src1, src2, .. } => {
            subst_mem(src1);
            subst_mem(src2);
        }
        W::Instruction::Copy { src, dst, .. } => {
            subst_mem(src);
            subst_mem(dst);
        }
        // Block and Loop anre unfolded in our CFG. So we do not need to care about them
        _ => (),
    }
}

// Checks if a copy has a memory value as src or dst and if that is the case if the name matches base
// used in Poitner analysis
pub fn contains_copy_mem(instr: &W::Instruction, base: &str) -> bool {
    match instr {
        W::Instruction::Copy { src, dst, .. } => {
            if let W::Value::Mem { name: mem_base, .. } = src
                && mem_base == base
            {
                return true;
            }
            if let W::Value::Mem { name: mem_base, .. } = dst
                && mem_base == base
            {
                return true;
            }
            false
        }
        _ => false,
    }
}
// tmp.32 = eta_malloc(32:i32)
//                 [tmp.32 + 0] : i64 = 3:i64
//                 tmp.33 : i32 = tmp.32 + 8:i32
//                 [tmp.33 + 0] : i64 = 1:i64
//                 [tmp.33 + 8] : i64 = 2:i64
//                 [tmp.33 + 16] : i64 = 3:i64
//                 tmp.34 : i32 = tmp.33 + 8:i64
// Would like to see that tmp34 is an alias to that array.
// So we normlaize th epointer recursively
// tmp.33 + 8 = tmp32 + 8 + 8 = tmp32 + 16

/// Replaces derived pointer variables in memory operations with their
/// canonical base pointer and combined offset.
///
/// Returns `true` if any instruction was rewritten.
#[instrument(skip(cfg, ptr_res), level = "Debug")]
pub fn pointer_propagation(cfg: &mut Cfg, ptr_res: &AnalysisResults<PointerAnalysis>) -> bool {
    let mut changed = false;

    for (block_id, block) in cfg.blocks.iter_mut() {
        for (instr_id, instr) in block.lab_instructions.iter_mut() {
            // Get the analysis fact valid *right before* this instruction executes.
            let instr_in_fact = ptr_res.instr_in(*block_id, *instr_id);

            // Find all variables this instruction reads (its uses).
            let uses = liveness::uses_of_instr(instr);

            for used_var in uses {
                // Check if the used variable has a known canonical form.
                if let Some((base, offset)) = instr_in_fact.get(&used_var) {
                    // Since that exist "tmp.32": ("tmp.32", 0),
                    // we check if name is equal to not run into infinte loop
                    if *base == used_var {
                        continue;
                    }

                    if !contains_copy_mem(instr, base) {
                        continue;
                    }
                    // For debugging and nice visual
                    // Now subst mem should always be true
                    let old_instr = instr.clone();
                    rewrite_mem_instr(instr, &used_var, base, *offset);
                    debug!(
                        "Replacing {} with {}! {} -> {}",
                        used_var, base, old_instr, instr
                    );
                    changed = true;
                }
            }
        }
    }

    changed
}

// Dead store elimination
// Your example after CSE
// [tmp.10 + 0] = [tmp.11 + 0]; // Store 1: This value is never read
// [tmp.11 + 0] = 6;            // Store 2: This overwrites Store 1

// // After Dead Store Elimination
// // The first store is gone, but we still need the load.
// some_val = [tmp.11 + 0];
// [tmp.11 + 0] = 6;
// // The store to [tmp.10 + 0] would now use `some_val`.

// Key for MemLocation Map
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemLocation {
    base: String,
    offset: usize,
    wt: W::WType,
}
// Map from base, offset -> Value
// Tracks the array pointers
// Function calls are black boxes. (I do not think they can change the memory in our case but lets be conservative here)
/// Forward must analysis: tracks available loads.
///
/// A load from a memory location is *available* if the value has already been
/// loaded and the memory location has not been written to since.
pub struct AvailableLoadAnalysis;
impl Analysis for AvailableLoadAnalysis {
    // Set of variables that are live
    type Fact = HashMap<MemLocation, W::Value>;

    // Intersectoin
    fn combine(&self, facts: &[&Self::Fact]) -> Self::Fact {
        if facts.is_empty() {
            return HashMap::new();
        }

        let mut result = facts[0].clone();
        result.retain(|loc, val| {
            // Keep only the key-value pairs that are present and identical in all other facts.
            facts
                .iter()
                .skip(1)
                .all(|other_fact| other_fact.get(loc) == Some(val))
        });

        result
    }
    //
    fn flow(&self, out_fact: &Self::Fact, lab_instr: (usize, &W::Instruction)) -> Self::Fact {
        let mut out_fact = out_fact.clone();

        // KILLS
        // Any Assignment to Variable x mut kill all memory facts taht use x
        // Right now independent of base
        if let Some(def) = liveness::def_of_instr(lab_instr.1) {
            out_fact.retain(|loc, _| loc.base != def);
        }

        // A callee can store through any pointer it was handed and through any
        // global, so no load survives a call. Narrowing this to the locations
        // actually reachable from the arguments needs a real alias analysis.
        if matches!(lab_instr.1, W::Instruction::FCall { .. }) {
            out_fact.clear();
        }

        // GEN Set
        // A store generates new load fact but also kills old ones!
        if let W::Instruction::Copy {
            src: val,
            dst: W::Value::Mem { name, offset, t },
            ..
        } = lab_instr.1
        {
            let loc = MemLocation {
                base: name.clone(),
                offset: *offset,
                wt: t.clone(),
            };

            // KILL all loads to specific locatoin
            out_fact.remove(&loc);
            // Conservative: Kill al facts with same base pointer and diffrent offsets
            // because of aliasing
            out_fact.retain(|l, _| l.base != *name);

            // Insert the new load
            out_fact.insert(loc, val.clone());
        }
        out_fact
    }

    fn direction() -> super::solver::Direction {
        super::solver::Direction::Forwards
    }

    fn initial_fact(&self) -> Self::Fact {
        HashMap::new()
    }

    // Add globals to analysis
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

/// Replaces redundant memory loads with the variable that already holds
/// the loaded value.
///
/// Returns `true` if any instruction was rewritten.
// It is very weak right now because of how conservative the available load analysis is
#[instrument(skip(cfg, available_loads), level = "Debug")]
pub fn store_to_load_forwarding(
    cfg: &mut Cfg,
    available_loads: &AnalysisResults<AvailableLoadAnalysis>,
) -> bool {
    let mut changed = false;

    for (block_id, block) in &mut cfg.blocks {
        for (instr_id, instr) in &mut block.lab_instructions {
            // Looking for load instructions
            if let W::Instruction::Copy {
                src: W::Value::Mem { name, offset, t },
                dst,
                t: _copy_t,
            } = instr
            {
                let loc = MemLocation {
                    base: name.clone(),
                    offset: *offset,
                    wt: t.clone(),
                };

                let fact = available_loads.instr_in(*block_id, *instr_id);

                // We found a value that we can forward
                if let Some(known_val) = fact.get(&loc) {
                    let src = W::Value::Mem {
                        name: name.clone(),
                        offset: *offset,
                        t: t.clone(),
                    };
                    debug!(
                        "Instruction Copy {} = {}. Replacing {} with known value {}",
                        dst, src, src, known_val
                    );
                    *instr = W::Instruction::Copy {
                        src: known_val.clone(),
                        dst: dst.clone(),
                        t: t.clone(),
                    };
                    changed = true
                }
            }
        }
    }
    changed
}
