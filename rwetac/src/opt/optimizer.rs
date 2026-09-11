//! The optimization driver.
//!
//! [`optmize`] is the entry point for the optimization pipeline. For each function,
//! it cleans up dead branches, builds a CFG, runs dataflow-based optimizations in a
//! loop until a fixed point, and then reconstructs structured WTAC from the optimized CFG.

use std::collections::HashSet;

use tracing::{debug, info, warn};

use crate::opt::cfg::{self, Cfg};
use crate::opt::dominators::{self};
use crate::opt::liveness::LivenessAnalysis;
use crate::opt::memory::{AvailableLoadAnalysis, PointerAnalysis};
use crate::opt::reaching::ReachingAnalysis;
use crate::opt::solver::Solver;
use crate::opt::{liveness, memory, reaching, reconstruct_cfg};
use crate::setting::{Pass, Pipeline, PipelineItem};
use crate::wtac::wtac_ast::{self as W, Program, TopLevel};

// Main entry Point

// Remove pattern
// br func_exit
// br if_end.5
// Happens with early return insde if and while
// If a block has no more corresponding br instructions because of this (happens when returns are inside both ifs for example)
//
// block if_end.13: {
//         block if_else.13: {
//                 tmp.12 : i64 = x > 0:i64
//                 br tmp.12 if_else.13
//                 __ret_val : i32 = 0:i32
//                 br func_exit
//                 br if_end.13
//         }
//         __ret_val : i32 = 1:i32
//         br func_exit
//         br if_end.13
// }
// Then the whole block is removed
// This ensures that on reconstructing the blcok_stack is balanced (It means the merge nodes always exist)
// For now that just tereminates block

/// Removes `Block` and `Loop` wrappers whose labels are no longer targeted
/// by any branch, inlining their bodies. This simplifies the structure
/// before CFG construction.
fn eliminate_dead_br(
    instrs: &[W::Instruction],
    target_blocks: &mut HashSet<String>,
) -> Vec<W::Instruction> {
    let mut clean_instrs = Vec::new();
    let mut terminated = false;
    for instr in instrs {
        if terminated {
            // Skip all instructions after unconditaionl Br or return
            continue;
        }
        match instr {
            W::Instruction::Block { label, body } => {
                let clean_body = eliminate_dead_br(body, target_blocks);
                clean_instrs.push(W::Instruction::Block {
                    label: label.clone(),
                    body: clean_body,
                });
            }
            W::Instruction::Loop { label, body } => {
                let clean_body = eliminate_dead_br(body, target_blocks);
                clean_instrs.push(W::Instruction::Loop {
                    label: label.clone(),
                    body: clean_body,
                });
            }
            _ => {
                clean_instrs.push(instr.clone());
                // Record which blocks are targeted by br and brIf
                match instr {
                    W::Instruction::Br(name) | W::Instruction::BrIf { l: name, .. } => {
                        target_blocks.insert(name.clone());
                    }
                    _ => (),
                }
                // Stop when undoncitoianl found for block
                match instr {
                    W::Instruction::Br(_) | W::Instruction::Return(_) => terminated = true,
                    _ => (),
                }
            }
        }
    }
    // println!("Targetted Blcoks: {:?}", target_blocks);
    clean_instrs
}

/// Removes instructions after unconditional branches or returns within a block,
/// and records which block/loop labels are still targeted by branches.
fn eliminate_dead_blocks(
    instrs: &[W::Instruction],
    target_blocks: &HashSet<String>,
) -> (Vec<W::Instruction>, bool) {
    let mut final_instrs = Vec::new();
    let mut changed = false;
    // Iterate again....
    for instr in instrs {
        match instr {
            W::Instruction::Block { label, body } => {
                let (rem_body, inner_changed) = eliminate_dead_blocks(body, target_blocks);
                if !target_blocks.contains(label) {
                    // Remove the block
                    debug!("Eliminated : {}", label);
                    changed = true;
                    final_instrs.extend(rem_body);
                } else {
                    final_instrs.push(W::Instruction::Block {
                        label: label.clone(),
                        body: rem_body,
                    });
                }
                // Propagate through recursion
                changed |= inner_changed;
            }
            W::Instruction::Loop { label, body } => {
                let (rem_body, inner_changed) = eliminate_dead_blocks(body, target_blocks);
                if !target_blocks.contains(label) {
                    // Remove the block
                    debug!("Eliminated : {}", label);
                    changed = true;
                    final_instrs.extend(rem_body);
                } else {
                    final_instrs.push(W::Instruction::Loop {
                        label: label.clone(),
                        body: rem_body,
                    });
                }
                // Propagate through recursion
                changed |= inner_changed;
            }
            _ => final_instrs.push(instr.clone()),
        }
    }
    (final_instrs, changed)
}

/// Runs reaching definitions analysis and builds use-def/def-use chains.
fn run_reach(cfg: &cfg::Cfg) -> (reaching::UseDef, reaching::DefUse) {
    let mut solver_reach = Solver::new(cfg, ReachingAnalysis);
    solver_reach.solve();
    let res_reach = solver_reach.into_results();

    let (use_def, def_use) = reaching::build_use_def(cfg, &res_reach);

    let mut s_use_def: Vec<_> = use_def.clone().into_iter().collect();
    s_use_def.sort_by_key(|(lbl, _)| lbl.0);

    (use_def, def_use)
}

/// Guards against a non-monotone pass spinning a `fixpoint(...)` group forever.
const MAX_FIXPOINT_ITERATIONS: usize = 1000;

/// Runs a single pass, computing whatever analysis it needs first.
///
/// Returns whether the pass changed the CFG.
fn run_pass(cfg: &mut Cfg, pass: Pass) -> bool {
    match pass {
        Pass::Cp => {
            let (use_def, _def_use) = run_reach(cfg);
            reaching::constant_propagation(cfg, &use_def)
        }
        Pass::Copy => {
            let (use_def, _def_use) = run_reach(cfg);
            reaching::copy_propagation(cfg, &use_def)
        }
        Pass::Cf => reaching::constant_folding(cfg),
        Pass::Dce => {
            let mut solver = Solver::new(cfg, LivenessAnalysis);
            solver.solve();
            let res = solver.into_results();
            liveness::dce(cfg, &res)
        }
        Pass::Ptr => {
            let mut solver = Solver::new(cfg, PointerAnalysis);
            solver.solve();
            let res = solver.into_results();
            memory::pointer_propagation(cfg, &res)
        }
        Pass::Stl => {
            let mut solver = Solver::new(cfg, AvailableLoadAnalysis);
            solver.solve();
            let res = solver.into_results();
            memory::store_to_load_forwarding(cfg, &res)
        }
    }
}

/// Names the pass that corrupted the graph, instead of letting it surface later
/// as a `None.unwrap()` in the solver. Debug builds only.
fn debug_validate(cfg: &Cfg, pass: Pass) {
    if !cfg!(debug_assertions) {
        return;
    }
    if let Err(errors) = cfg.validate() {
        panic!(
            "pass `{}` left the CFG of `{}` invalid:\n  {}",
            pass.name(),
            cfg.fn_name,
            errors.join("\n  ")
        );
    }
}

/// Runs `pipeline` over one function's CFG, in the order given.
fn run_pipeline(cfg: &mut Cfg, pipeline: &Pipeline) {
    for item in &pipeline.0 {
        match item {
            PipelineItem::Pass(pass) => {
                let changed = run_pass(cfg, *pass);
                debug_validate(cfg, *pass);
                debug!("pass {pass}: changed = {changed}");
            }
            PipelineItem::Fixpoint(passes) => {
                let mut iterations = 0;
                loop {
                    let mut changed = false;
                    for pass in passes {
                        changed |= run_pass(cfg, *pass);
                        debug_validate(cfg, *pass);
                    }
                    if !changed {
                        break;
                    }
                    iterations += 1;
                    if iterations >= MAX_FIXPOINT_ITERATIONS {
                        warn!(
                            "fixpoint group did not converge after {MAX_FIXPOINT_ITERATIONS} \
                             iterations, giving up. One of these passes is not monotone: {}",
                            passes
                                .iter()
                                .map(|p| p.name())
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                        break;
                    }
                }
            }
        }
    }
}

/// Optimizes a single function.
///
/// Steps: clean up dead branches → build CFG → run [`run_pipeline`] → compute
/// dominators → reconstruct structured WTAC.
fn optimize_function(
    tl: TopLevel,
    globals: &[TopLevel],
    dump: bool,
    dump_path: &std::path::Path,
    pipeline: &Pipeline,
) -> TopLevel {
    match tl {
        TopLevel::Function {
            name,
            body,
            params,
            f_type,
            locals,
        } => {
            info!(
                "=================Creating CFG for {}============================",
                name
            );
            let target_blocks = &mut HashSet::new();
            // Called in a loop to remove unnessecary br after a blcok was removed
            let mut clean_body = eliminate_dead_br(&body, target_blocks);
            let mut change = true;
            while change {
                (clean_body, change) = eliminate_dead_blocks(&clean_body, target_blocks);
                target_blocks.clear();
                clean_body = eliminate_dead_br(&clean_body, target_blocks);
            }

            let mut cfg =
                cfg::Cfg::new(name.clone(), &params, &clean_body, locals.clone(), globals);

            if let Err(errors) = cfg.validate() {
                for e in &errors {
                    warn!("CFG invariant violated in {name}: {e}");
                }
            }

            if dump {
                cfg.to_dot(dump_path, "initial")
                    .expect("File creation failed in CFG");
            }

            run_pipeline(&mut cfg, pipeline);
            // Reconstruct
            let mut dominators = dominators::compute_dominators(&cfg);
            let fn_body = reconstruct_cfg::reconstruct(
                &mut cfg,
                &mut dominators.doms,
                &mut dominators.ipdoms,
                dump_path,
            );

            TopLevel::Function {
                name,
                body: fn_body,
                params,
                f_type,
                locals,
            }
        }
        TopLevel::Data { .. } => unreachable!("Should be partitioned"),
    }
}

/// Entry point for optimizing a full WTAC program.
///
/// Partitions declarations into globals and functions, optimizes each function
/// independently, and reassembles the program.
pub fn optmize(
    prog: W::Program,
    dump: bool,
    dump_path: &std::path::Path,
    pipeline: &Pipeline,
) -> W::Program {
    info!("Starting optimizations with pipeline: {pipeline}");
    let (mut globals, functions): (Vec<_>, Vec<_>) = prog
        .decls
        .into_iter()
        .partition(|tl| matches!(tl, TopLevel::Data { .. }));
    // Hacky but compiler globals can not show up as Data. So add them manually.... Do not use value
    let stack_global = TopLevel::Data {
        name: "__stack_pointer".to_string(),
        t: W::WType::I32,
        v: W::Value::Imm(0, W::WType::I32),
    };
    globals.push(stack_global);

    let opt_code: Vec<TopLevel> = functions
        .into_iter()
        .map(|f| optimize_function(f, &globals, dump, dump_path, pipeline))
        .collect();
    // Pop stack pointer...
    globals.pop();
    Program {
        decls: globals.into_iter().chain(opt_code).collect(),
    }
}
