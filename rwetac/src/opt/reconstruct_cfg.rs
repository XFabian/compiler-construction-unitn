//! Reconstructs structured WTAC from an optimized CFG.
//!
//! After optimizations, the CFG needs to be converted back into the nested
//! `Block`/`Loop`/`BrIf` structure that WASM requires. This module uses
//! dominator and post-dominator information to identify if-then-else regions
//! and loop bodies, then collapses them back into structured instructions.
//!
//! This is the most complex part of the optimizer and is not expected to be
//! modified by students.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use tracing::debug;

use crate::opt::cfg::{BlockKind, Cfg};
use crate::wtac::wtac_ast as W;

type DominatorMap = HashMap<usize, HashSet<usize>>;
type IpostDomMap = HashMap<usize, usize>;
/// Replaces a set of nodes with a single supernode in the CFG.
// TOOD: Could have more clean up
pub fn reconstruct(
    cfg: &mut Cfg,
    doms: &mut DominatorMap,
    ipost_doms: &mut IpostDomMap,
    dump_path: &Path,
) -> Vec<W::Instruction> {
    // Output will be linear sequence of instructions that needs to be wrapped in func_exit!
    reconstruct_helper(cfg, doms, ipost_doms, dump_path);
    // Fixed Structure afterwards
    // Entry Node -> Func Exit blcok start -> Straightline -> merge_func_exit (Function Epilogue)
    // Entry Node -> Func Exit block with everything -> merge func_exit
    // Supernode does not exist if only straigthline code
    let entry_id = cfg.get_entry();
    let func_body_block_id = cfg.get_block(entry_id).succs[0];
    let func_body_block = cfg.get_block(func_body_block_id);
    assert_eq!(
        func_body_block.kind,
        BlockKind::FuncBody,
        "reconstruction failed: block after entry is {:?}, not the function body",
        func_body_block.kind
    );
    let func_body_name = func_body_block
        .label
        .clone()
        .expect("This block needs to have a name");
    assert!(func_body_block.succs.len() == 1);

    // Find End node
    let end_id = cfg
        .find_kind(BlockKind::FuncEpilogue)
        .expect("Has merge_func_exit has to exist");
    let end_block = cfg.get_block(end_id);
    let end_instrs: Vec<W::Instruction> = end_block
        .lab_instructions
        .clone()
        .into_iter()
        .map(|(_, instr)| instr)
        .collect();

    let mut current_id = func_body_block_id;
    let mut fun_body = Vec::new();
    // Add all to the function body
    while current_id != end_id {
        let cur_block = cfg.get_block(current_id);
        let cur_instructions: Vec<W::Instruction> = cur_block
            .lab_instructions
            .clone()
            .into_iter()
            .map(|(_, instr)| instr)
            .collect();
        // Needs to be linear
        assert!(cur_block.succs.len() == 1);
        current_id = cur_block.succs[0];
        fun_body.extend(cur_instructions);
    }

    let mut func_block_instr = vec![W::Instruction::Block {
        label: func_body_name,
        body: fun_body,
    }];
    func_block_instr.extend(end_instrs);
    func_block_instr
}

pub fn reconstruct_helper(
    cfg: &mut Cfg,
    doms: &mut DominatorMap,
    ipost_doms: &mut IpostDomMap,
    dump_path: &Path,
) {
    loop {
        // Do loop stuff here
        if let Some((header_id, kind)) = find_innermost_header(cfg, doms, ipost_doms) {
            debug!("Innermost header found: {:?}", kind);
            match kind {
                HeaderKind::DeadLoop => {
                    // very manually
                    let header_block = cfg.get_block(header_id);

                    let dloop_name = header_block
                        .label
                        .clone()
                        .expect("Name for Degenerate Loop must exist");
                    let BlockKind::LoopContainer(dloop_num) = header_block.kind else {
                        panic!(
                            "dead loop header {header_id} is {:?}, expected a loop container",
                            header_block.kind
                        )
                    };

                    assert!(header_block.succs.len() == 2);
                    // find the two branches similar to if and the merge node
                    debug!("=======Handling Dead Loop : {}================", dloop_name);

                    let merge_id = ipost_doms[&header_id];
                    let end_while_id = cfg.find_kind(BlockKind::LoopJoin(dloop_num)).unwrap();
                    let loop_body_start = header_block
                        .succs
                        .clone()
                        .into_iter()
                        .find(|id| *id != end_while_id)
                        .unwrap();
                    // Now get region between loop_body and merge_id
                    // Should be in cfg this functoin
                    let mut loop_body = cfg.get_region(loop_body_start, merge_id);

                    // Add header to simplify reconstruction here. COuld not be added before becuse other sude merge_while_end wold be added
                    loop_body.insert(header_id);
                    let body_subgraph = cfg.create_subgraph(&loop_body, loop_body_start, merge_id);
                    let reconstructed_body =
                        linearize_subgraph(body_subgraph, doms, ipost_doms, dump_path);

                    let mut new_dloop_instr = {
                        vec![W::Instruction::Block {
                            label: dloop_name.clone(),
                            body: reconstructed_body,
                        }]
                    };

                    let mut region_collapse = loop_body;
                    if let Some(dloop_end_id) = cfg.find_kind(BlockKind::LoopJoin(dloop_num)) {
                        let block = cfg.get_block(dloop_end_id);
                        let b_instrs: Vec<_> = block.owned_instructions();
                        new_dloop_instr.extend(b_instrs);
                        region_collapse.insert(dloop_end_id);
                    }

                    collapse_region(cfg, &region_collapse, header_id, new_dloop_instr);
                }
                HeaderKind::Loop(latch_id) => {
                    // Its the while_loop block!
                    let header_block = cfg.get_block(header_id);

                    let loop_name = header_block
                        .label
                        .clone()
                        .expect("Name for Loop must exist");
                    let BlockKind::LoopHeader(loop_num) = header_block.kind else {
                        panic!(
                            "loop header {header_id} is {:?}, expected a loop header",
                            header_block.kind
                        )
                    };
                    debug!("=======Handling Loop {}================", loop_name);

                    let mut loop_body = extract_loop_body(cfg, header_id, &[latch_id]);
                    loop_body.remove(&header_id);

                    // the entry is the first id in the successors of header_block which is part of the body
                    let entry_subgraph_id = header_block
                        .succs
                        .clone()
                        .into_iter()
                        .find(|id| loop_body.contains(id))
                        .unwrap();
                    // exit does not matter here
                    let body_subgraph =
                        cfg.create_subgraph(&loop_body, entry_subgraph_id, header_id);
                    let reconstructed_body =
                        linearize_subgraph(body_subgraph, doms, ipost_doms, dump_path);

                    let header_instrs: Vec<W::Instruction> =
                        cfg.get_block(header_id).owned_instructions();

                    let while_end_id = cfg.find_kind(BlockKind::LoopContainer(loop_num));

                    // The container is also the preheader, and is collapsed away
                    // below, so anything hoisted into it must be carried out here.
                    let preheader_instrs: Vec<W::Instruction> = while_end_id
                        .map(|id| cfg.get_block(id).owned_instructions())
                        .unwrap_or_default();

                    let mut new_loop_instr = {
                        let loop_instr = W::Instruction::Loop {
                            label: loop_name.clone(),
                            body: [header_instrs, reconstructed_body].concat(),
                        };
                        let loop_container = W::Instruction::Block {
                            label: format!("while_end.{}", loop_num),
                            body: [preheader_instrs, vec![loop_instr]].concat(),
                        };
                        vec![loop_container]
                    };

                    let mut region_collapse = loop_body;
                    region_collapse.insert(header_id);

                    // Before loop header we have the while end block
                    // This has to exist basically
                    if let Some(while_end_start) = while_end_id {
                        region_collapse.insert(while_end_start);
                    }
                    // Simplify merge_while_end as well

                    if let Some(loop_end_id) = cfg.find_kind(BlockKind::LoopJoin(loop_num)) {
                        let block = cfg.get_block(loop_end_id);
                        let b_instrs: Vec<_> = block.owned_instructions();
                        new_loop_instr.extend(b_instrs);
                        region_collapse.insert(loop_end_id);
                    }

                    collapse_region(cfg, &region_collapse, header_id, new_loop_instr);
                    debug!("=======Finished Loop {}================", loop_name);
                }
                HeaderKind::If => {
                    let header_if_block = cfg.get_block(header_id);
                    assert!(header_if_block.preds.len() == 1);
                    let IfInfo {
                        name: if_name,
                        num: if_num,
                        then_id,
                        else_id,
                        merge_id,
                        then_region,
                        else_region,
                    } = get_if_info(cfg, header_if_block.id, ipost_doms);

                    debug!("=======Handling {}================", if_name);
                    debug!(
                        "Then Region : {:?}, Else Region: {:?}",
                        then_region, else_region
                    );
                    // This is actually a subgraph
                    let then_subgraph = cfg.create_subgraph(&then_region, then_id, merge_id);
                    let reconstructed_then_body =
                        linearize_subgraph(then_subgraph, doms, ipost_doms, dump_path);
                    // We can add the header to the else graph is a little bit cleaner. Dont need the header instrs then

                    let else_subgraph = cfg.create_subgraph(&else_region, else_id, merge_id);
                    let reconstructed_else_body =
                        linearize_subgraph(else_subgraph, doms, ipost_doms, dump_path);
                    let mut new_if_instr = {
                        let header_instrs: Vec<W::Instruction> =
                            cfg.get_block(header_id).owned_instructions();
                        let new_else = W::Instruction::Block {
                            label: if_name.clone(),
                            body: [header_instrs, reconstructed_else_body].concat(),
                        };

                        let new_body = [vec![new_else], (reconstructed_then_body)].concat();

                        vec![W::Instruction::Block {
                            label: format!("if_end.{}", if_num), // Generate a suitable label
                            body: new_body,
                        }]
                    };
                    // We need to handle the merge_if_end node if it exists and add it to our instructions
                    // Create the new if instruction
                    // Then then goes until the merge node of the if!

                    // Collapse the region in the main graph
                    let mut region_collapse = then_region;
                    region_collapse.extend(else_region);
                    region_collapse.insert(header_id);

                    // Before header we have the basic block if_end.9 so this blcok is actually before
                    // However, this block does not exist beacuse of eliminate_dead_blcoks when both branches have a return
                    // Then no br_if_end are left in the program and the empty block is removed.

                    if let Some(if_end_start) = cfg.find_kind(BlockKind::IfContainer(if_num)) {
                        region_collapse.insert(if_end_start);
                    }

                    // If there is a dedicated merge_if_end instruction then we merge it as well. This is code after an if.
                    // If there is a second if afterwards then this node will be empty
                    // if not ist just contains the straighline code that will be after the if
                    // This node does not exist when both branches contain a return
                    // This happens when then branch contains a return and the else branch does not. Or reversed
                    // The merge point is the last block becasue of return. When we collapse the then branch has an edge to this blokc
                    // While else points to merge_if_end. Thus this is not fully linear right now.
                    // I do not want to just merge this merge_if block into it
                    if let Some(if_end_id) = cfg.find_kind(BlockKind::IfJoin(if_num)) {
                        let block = cfg.get_block(if_end_id);
                        let b_instrs: Vec<_> = block.owned_instructions();
                        new_if_instr.extend(b_instrs);
                        region_collapse.insert(if_end_id);
                    }

                    collapse_region(cfg, &region_collapse, header_id, new_if_instr);
                    // Drop the edge to exit so the region linearizes. This has to
                    // come off both endpoints: collapse_region rebuilds a
                    // neighbour's edges from the region's own edge lists, so a
                    // half-edge is silently kept and later names a removed block.
                    let exit_id = cfg.get_exit();
                    if cfg.get_block(header_id).succs.len() > 1 {
                        cfg.remove_edge(header_id, exit_id);
                    }
                }
            }
            // Because of Cfg changes recompute dominators
            debug!(
                "========== Computing Dominators after Handling : {}",
                header_id
            );
            // make cfg to dot file here
            let dominators = crate::opt::dominators::compute_dominators(cfg);
            *doms = dominators.doms;
            *ipost_doms = dominators.ipdoms;
            continue;
        }
        break;
    }
}

fn linearize_subgraph(
    mut cfg: Cfg,
    doms: &mut DominatorMap,
    ipost_doms: &mut IpostDomMap,
    dump_path: &Path,
) -> Vec<W::Instruction> {
    // 1. Call the helper to simplify the graph in-place.
    reconstruct_helper(&mut cfg, doms, ipost_doms, dump_path);

    // Now graph should be simple linear sequence
    let mut instructions = Vec::new();
    let mut current_id = Some(cfg.get_entry());
    let mut visited: HashSet<usize> = HashSet::new();
    while let Some(id) = current_id {
        // Some loop check I may need?
        if !visited.insert(id) {
            // This prevents infinite loops, which can happen if the last
            // block in a loop body incorrectly points back to the header.
            break;
        }
        let block = cfg.get_block(id);
        instructions.extend(
            block
                .lab_instructions
                .iter()
                .map(|(_, instr)| instr.clone()),
        );

        // In a linearized graph, there should only be one successor.
        current_id = block.succs.first().cloned();
    }

    instructions
}

fn collapse_region(
    cfg: &mut Cfg,
    region_nodes: &HashSet<usize>,
    header_id: usize, // This node ID will be reused for the supernode
    new_instr: Vec<W::Instruction>,
) {
    let mut external_preds = HashSet::new();
    let mut external_succs = HashSet::new();

    // Find all edges entering and leaving the region.
    for node_id in region_nodes {
        let block = &cfg.blocks[node_id];
        for pred_id in &block.preds {
            if !region_nodes.contains(pred_id) {
                external_preds.insert(*pred_id);
            }
        }
        for succ_id in &block.succs {
            if !region_nodes.contains(succ_id) {
                external_succs.insert(*succ_id);
            }
        }
    }

    // Remove all nodes in the region except the header, which we will reuse.
    for node_id in region_nodes {
        if *node_id != header_id {
            cfg.blocks.remove(node_id);
        }
    }

    // Update the neighbors of the collapsed region.
    for pred_id in &external_preds {
        let pred_block = cfg.blocks.get_mut(pred_id).unwrap();
        pred_block.succs.retain(|id| !region_nodes.contains(id));
        pred_block.succs.push(header_id);
    }
    for succ_id in &external_succs {
        let succ_block = cfg.blocks.get_mut(succ_id).unwrap();
        succ_block.preds.retain(|id| !region_nodes.contains(id));
        succ_block.preds.push(header_id);
    }

    // Finally, reconfigure the header as the new supernode.
    let supernode = cfg.blocks.get_mut(&header_id).unwrap();
    supernode.label = None;
    // The collapsed region is now one opaque node: it no longer plays the
    // structural role its label named, and must not be found by `find_kind`.
    supernode.kind = BlockKind::Plain;
    // Use the block_id number. Since I use the instruction number in dominator analysis to find ht ecoorect block...
    // And use my formula
    let i_num = crate::opt::cfg::get_instr_id(header_id, 0);
    supernode.lab_instructions = new_instr.into_iter().map(|i| (i_num, i)).collect();
    supernode.preds = external_preds.iter().cloned().collect();
    supernode.succs = external_succs.iter().cloned().collect();
    supernode.preds.sort();
    supernode.succs.sort();
}

struct IfInfo {
    pub name: String,
    pub num: u32,
    pub then_id: usize,
    pub else_id: usize,
    pub merge_id: usize,
    pub then_region: HashSet<usize>,
    pub else_region: HashSet<usize>,
}

// ..gets informatin about an if. Like its name the then_id, else_id, the if number etc
// Input : Id if block pointing to an if (if_else since it has the two branches)
fn get_if_info(cfg: &Cfg, if_header_id: usize, ipost_doms: &IpostDomMap) -> IfInfo {
    let header_if_block = cfg.get_block(if_header_id);
    assert!(header_if_block.preds.len() == 1);
    let if_name = header_if_block
        .label
        .clone()
        .expect("Name for If must exist");
    let BlockKind::IfHeader(if_num) = header_if_block.kind else {
        panic!(
            "if header {if_header_id} is {:?}, expected an if header",
            header_if_block.kind
        )
    };

    let merge_id = if let Some(id) = cfg.find_kind(BlockKind::IfJoin(if_num)) {
        id
    } else {
        // merge_if_end does not exist when both branches contain a return! But merge_if_end is sometimes better
        ipost_doms[&if_header_id]
    };
    // This marks the then branch of the if
    let then_id = cfg
        .find_kind(BlockKind::IfThenEntry(if_num))
        .expect("Merge if else has to exist!");
    // is the successor that is not the then branch
    let else_id = header_if_block
        .succs
        .iter()
        .find_map(|i| if *i != then_id { Some(*i) } else { None })
        .unwrap();

    // Nodes belonging to each branch
    let (then_region, else_region) = extract_branch_regions(cfg, then_id, else_id, merge_id);

    IfInfo {
        name: if_name,
        num: if_num,
        then_id,
        else_id,
        merge_id,
        then_region,
        else_region,
    }
}
#[derive(Debug)]
pub enum HeaderKind {
    If,          // should contain hte Ifinfo
    Loop(usize), // has the latch id
    DeadLoop,    // A loop with no backedge (e.g return inside)
}
// Finds innermost Header
// This is needed because create subgraph creates a copy of my cfg.
/// If nesting of ifs occur and the outer is ahndedl first. It recurses into the inner with a copied sugraph.
/// All collapsing os o  the copy and not on the original one.
/// Thus, an innermost header does not contain any other control structure
fn find_innermost_header(
    cfg: &Cfg,
    doms: &DominatorMap,
    ipost_doms: &IpostDomMap,
) -> Option<(usize, HeaderKind)> {
    for (id, block) in &cfg.blocks {
        let header_id = *id;

        // --- First check for loop headers ---
        //          Natural loop  is defined by a back edge
        //    A control flow edge B -> H where H dominates B
        //    Then : H Is the loop Header
        // So assume block is the Header and the predecessor is the latch. T
        // So when the header dominates a predecessor than it has to be a back edge
        let latches: Vec<usize> = block
            .preds
            .iter()
            .copied()
            .filter(|pred_id| doms.get(pred_id).is_some_and(|d| d.contains(&header_id)))
            .collect();
        // A loop is rebuilt as one `Loop` around one linearized body, which a
        // second back edge would not describe. Failing beats miscompiling.
        assert!(
            latches.len() <= 1,
            "loop header {header_id} in `{}` has {} back edges ({latches:?}). \
             Reconstruction only handles one; `break`/`continue` need it fixed first.",
            cfg.fn_name,
            latches.len()
        );
        if let Some(&latch_id) = latches.first() {
            let mut loop_body = extract_loop_body(cfg, header_id, &latches);
            // Remove header and only check body for other structures. Otherwise it finds itself in it
            loop_body.remove(&header_id);
            if !contains_other_header(cfg, doms, &loop_body) {
                return Some((header_id, HeaderKind::Loop(latch_id)));
            }
        }
        if block.succs.len() != 2 {
            continue;
        }

        // A loop container normally has one successor. Two means the loop it
        // wraps has no back edge -- the body always leaves early -- so it is not
        // a real loop and is reconstructed as a plain block.
        if let BlockKind::LoopContainer(dloop_num) = block.kind {
            assert!(block.succs.len() == 2);
            // find the two branches similar to if and the merge node

            let merge_id = ipost_doms[&header_id];
            let end_while_id = cfg.find_kind(BlockKind::LoopJoin(dloop_num)).unwrap();
            let loop_body_start = block
                .succs
                .clone()
                .into_iter()
                .find(|id| *id != end_while_id)
                .unwrap();
            let loop_body = cfg.get_region(loop_body_start, merge_id);
            if !contains_other_header(cfg, doms, &loop_body) {
                return Some((header_id, HeaderKind::DeadLoop));
            }
            // Need to check here as well that nothing is inside
        }
        if matches!(block.kind, BlockKind::IfHeader(_)) {
            let IfInfo {
                mut then_region,
                else_region,
                ..
            } = get_if_info(cfg, block.id, ipost_doms);

            then_region.extend(else_region);
            if !contains_other_header(cfg, doms, &then_region) {
                return Some((header_id, HeaderKind::If));
            }
        }
    }
    // Important since some are not the innermost headers!
    None
}

fn contains_other_header(
    cfg: &Cfg,
    doms: &DominatorMap,
    region: &std::collections::HashSet<usize>,
) -> bool {
    for &node_id in region {
        let block = &cfg.blocks[&node_id];

        // Nested if
        if block.succs.len() == 2 {
            return true;
        }

        // Nested loop (node dominates one of its preds)
        if block
            .preds
            .iter()
            .any(|pred_id| doms.get(pred_id).is_some_and(|d| d.contains(&node_id)))
        {
            return true;
        }
    }
    false
}

/// Identifies the sets of nodes that form the `then` and `else` branches of a conditional.
///
/// This function performs two separate graph traversals, one for each branch.
///
/// # Returns
/// A tuple containing two HashSets of node IDs: `(then_nodes, else_nodes)`.
/// Stops when
/// /// 1. It reaches the merge node.
/// 2. It reaches a block that terminates with an unconditional branch (`Br` or `Return`)
///    that does not lead to another node within the branch.
fn extract_branch_regions(
    cfg: &Cfg,
    then_entry_id: usize,
    else_entry_id: usize,
    merge_id: usize,
) -> (HashSet<usize>, HashSet<usize>) {
    // Helper function to perform a graph traversal for one branch.

    let then_nodes = cfg.get_region(then_entry_id, merge_id);
    let else_nodes = cfg.get_region(else_entry_id, merge_id);

    (then_nodes, else_nodes)
}

/// header ──► body1 ──► body2 ──► latch
//    ▲                          │
//    └──────────────────────────┘ (backedge)
// Finds all nodes belonging to a loop using a reverse traversal from the latch node.
// Adds the header as well!
/// Seeding the header first stops the walk from escaping before the loop. Only
/// finds what is *backward* reachable from a latch, so a block whose only
/// successor leaves the loop -- what `break` would produce -- is missed.
fn extract_loop_body(cfg: &Cfg, header_id: usize, latches: &[usize]) -> HashSet<usize> {
    let mut loop_nodes = HashSet::new();
    loop_nodes.insert(header_id);

    let mut stack: Vec<usize> = Vec::new();
    for &latch in latches {
        if loop_nodes.insert(latch) {
            stack.push(latch);
        }
    }
    while let Some(m) = stack.pop() {
        for p in &cfg.get_block(m).preds {
            if loop_nodes.insert(*p) {
                stack.push(*p);
            }
        }
    }

    loop_nodes
}
