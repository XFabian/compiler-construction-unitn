//! Control Flow Graph construction and representation.
//!
//! The [`Cfg`] is built from a function's WTAC instructions and is the structure
//! on which all dataflow analyses and optimizations operate. Each [`BasicBlock`]
//! contains a sequence of labeled instructions, predecessor/successor edges,
//! and an optional label (for blocks corresponding to WASM `block`/`loop` constructs).
//!
//! The CFG can be exported to Graphviz `.dot` format via [`Cfg::to_dot`] for
//! visual inspection (enabled with `--dump`).

use pretty::RcDoc;
use tracing::{debug, instrument};

use crate::wtac::wtac_ast::{self as W, Instruction, WType};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, File},
    io::{self, BufWriter},
    path::Path,
};

// Block IDs are allocated per CFG by CfgBuilder, not from a global counter:
// compiling two functions concurrently must not share numbering.

/// The structural role of a block, parsed once from its label so that
/// reconstruction never matches name substrings. The paired labels of one
/// construct share a number, which is what the `u32` ties together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Plain,
    /// `func_exit`
    FuncBody,
    /// `merge_func_exit`
    FuncEpilogue,
    /// `if_else.N` -- evaluates the guard and branches
    IfHeader(u32),
    /// `if_end.N` -- the block an `if` branches out of
    IfContainer(u32),
    /// `merge_if_else.N` -- entry to the `then` branch
    IfThenEntry(u32),
    /// `merge_if_end.N` -- code after the `if`
    IfJoin(u32),
    /// `while_loop.N` -- the back edge points here
    LoopHeader(u32),
    /// `while_end.N` -- the block a loop branches out of, and its preheader
    LoopContainer(u32),
    /// `merge_while_end.N` -- code after the loop
    LoopJoin(u32),
}

impl BlockKind {
    /// An unrecognised label is `Plain`, so blocks a pass creates have no
    /// structural meaning to reconstruction.
    pub fn from_label(label: Option<&String>) -> Self {
        let Some(label) = label else {
            return BlockKind::Plain;
        };
        match label.as_str() {
            "func_exit" => return BlockKind::FuncBody,
            "merge_func_exit" => return BlockKind::FuncEpilogue,
            _ => (),
        }
        let Some((prefix, num)) = label.rsplit_once('.') else {
            return BlockKind::Plain;
        };
        let Ok(num) = num.parse::<u32>() else {
            return BlockKind::Plain;
        };
        match prefix {
            "if_else" => BlockKind::IfHeader(num),
            "if_end" => BlockKind::IfContainer(num),
            "merge_if_else" => BlockKind::IfThenEntry(num),
            "merge_if_end" => BlockKind::IfJoin(num),
            "while_loop" => BlockKind::LoopHeader(num),
            "while_end" => BlockKind::LoopContainer(num),
            "merge_while_end" => BlockKind::LoopJoin(num),
            _ => BlockKind::Plain,
        }
    }
}

/// A basic block in the control flow graph.
///
/// Each instruction is labeled with a unique ID derived from the block ID
/// and its position. This ID is used as a key in dataflow analysis results.
#[derive(Debug)]
pub struct BasicBlock {
    pub id: usize,
    pub lab_instructions: Vec<(usize, W::Instruction)>,
    pub label: Option<String>, // label for block/ loop headers
    /// The structural role of this block, derived from `label` at construction.
    pub kind: BlockKind,
    pub preds: Vec<usize>,
    pub succs: Vec<usize>,
}

// A basic block does not necessarily end with a terminator in our implementation
// Well eseentially it does but we add some fallthrough edges

//
// positive(x : i64) -> i32 {
//         block func_exit: {
//                 tmp.2 : i32 = __stack_pointer
//                 __stack_pointer : i32 = __stack_pointer - 0:i32
//                 tmp.3 : i32 = __stack_pointer //----------- Here we stop a basic blcok and add a fall through edge
// We do this aid in reconstruction. Otherwise these instructions are folded into the if_end block later
// This is semantically correct but looks not so nice
// Note that this falltrough edeges do not corresponf to a br or ret instruction
// This is fine since we store the termiantor instruction so reconstruction is easy
//                 block if_end.5: {
//                         block if_else.5: {
//                                 tmp.4 : i64 = x > 0:i64
//                                 br tmp.4 if_else.5
//                                 br if_end.5
//                         }
//                         __ret_val : i32 = 1:i32
//                         br func_exit
//                         br if_end.5
//                 }
//                 __ret_val : i32 = 0:i32
//                 br func_exit
//                 br func_exit
//         }
//         __stack_pointer : i32 = tmp.2
//         return __ret_val
// }
impl BasicBlock {
    pub fn new(id: usize, instrs: &[W::Instruction], label: &Option<String>) -> Self {
        let lab_instructions: Vec<(usize, W::Instruction)> = instrs
            .iter()
            .enumerate()
            .map(|(i, instr)| (get_instr_id(id, i), instr.clone()))
            .collect();
        BasicBlock {
            id,
            lab_instructions,
            kind: BlockKind::from_label(label.as_ref()),
            label: label.clone(),
            preds: Vec::new(),
            succs: Vec::new(),
        }
    }

    pub fn update_succs(&mut self, block_id: usize) {
        self.succs.push(block_id);
    }

    pub fn update_preds(&mut self, block_id: usize) {
        self.preds.push(block_id);
    }

    /// Returns an iterator over the instructions (without IDs).
    pub fn instructions(&self) -> impl DoubleEndedIterator<Item = &W::Instruction> {
        self.lab_instructions.iter().map(|(_, instr)| instr)
    }

    /// Returns a cloned list of instructions (without IDs)
    pub fn owned_instructions(&self) -> Vec<W::Instruction> {
        self.lab_instructions
            .iter()
            .map(|(_, instr)| instr.clone())
            .collect()
    }
}

/// The control flow graph for a single function.
#[derive(Debug)]
pub struct Cfg {
    /// All basic blocks, keyed by block ID.
    pub blocks: HashMap<usize, BasicBlock>,
    /// Name of the function this CFG represents.
    pub fn_name: String,
    entry_id: usize,
    exit_id: usize,
    /// Global variables visible to this function (name → type).
    pub globals: HashMap<String, WType>,
    /// Parameter names of the function.
    pub params: Vec<String>,
}

/// Builds the CFG by recursively walking WTAC instructions.
///
/// Used internally by [`Cfg::new`].
pub struct CfgBuilder {
    // Next block ID to hand out. Local to this builder.
    next_id: usize,
    // All blocks being built
    blocks: HashMap<usize, BasicBlock>,
    // Maps a label name to the ID of the block that should be branched to.
    // For a Block, this is the merge-block *after* it.
    // For a Loop, this is the header-block *at the start* of it.
    label_map: HashMap<String, usize>,
}

impl Cfg {
    #[instrument(
        skip(params, body, globals, _locals),
        level = "Debug",
        name = "CFG Create"
    )]
    pub fn new(
        fn_name: String,
        params: &[String],
        body: &[W::Instruction],
        _locals: Vec<(String, WType)>,
        globals: &[W::TopLevel],
    ) -> Self {
        let mut builder = CfgBuilder {
            next_id: 0,
            blocks: HashMap::new(),
            label_map: HashMap::new(),
        };

        builder.discover_labels(body);
        debug!("Discovered Labels: {:?}", builder.label_map);
        // Create entry block
        let entry_block = builder.new_block(&[], &None);
        let entry_id = entry_block.id;
        builder.blocks.insert(entry_block.id, entry_block);

        let exit_id = builder.build_recursive(body, entry_id);

        let mut globals_map = HashMap::new();
        for topl in globals {
            if let W::TopLevel::Data { name, t, .. } = topl {
                globals_map.insert(name.clone(), t.clone());
            }
        }

        Cfg {
            blocks: builder.blocks,
            fn_name,
            entry_id,
            exit_id,
            globals: globals_map,
            params: params.to_vec(),
        }
    }

    pub fn get_block(&self, block_id: usize) -> &BasicBlock {
        self.blocks.get(&block_id).unwrap()
    }

    pub fn get_block_mut(&mut self, block_id: usize) -> &mut BasicBlock {
        self.blocks.get_mut(&block_id).unwrap()
    }

    pub fn get_entry(&self) -> usize {
        self.entry_id
    }

    pub fn get_exit(&self) -> usize {
        self.exit_id
    }

    pub fn add_edge(from: &mut BasicBlock, to: &mut BasicBlock) {
        from.update_succs(to.id);
        to.update_preds(from.id);
    }

    /// Removes an edge from both endpoints.
    ///
    /// Dropping it from only one side leaves a half-edge that survives until
    /// the other endpoint is collapsed away, and then names a block that no
    /// longer exists.
    pub fn remove_edge(&mut self, from: usize, to: usize) {
        if let Some(from_block) = self.blocks.get_mut(&from) {
            from_block.succs.retain(|id| *id != to);
        }
        if let Some(to_block) = self.blocks.get_mut(&to) {
            to_block.preds.retain(|id| *id != from);
        }
    }

    /// Checks the graph invariants the dataflow solver relies on: every edge
    /// endpoint must exist, and every edge must be recorded on both sides.
    ///
    /// The solver seeds its fact maps from `blocks`, so a predecessor that is
    /// not a block panics there instead of being reported here.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let mut ids: Vec<_> = self.blocks.keys().copied().collect();
        ids.sort();

        for id in ids {
            let block = &self.blocks[&id];
            for pred in &block.preds {
                match self.blocks.get(pred) {
                    None => errors.push(format!("block {id}: predecessor {pred} does not exist")),
                    Some(p) if !p.succs.contains(&id) => errors.push(format!(
                        "block {id}: lists predecessor {pred}, but {pred} has successors {:?}",
                        p.succs
                    )),
                    _ => (),
                }
            }
            for succ in &block.succs {
                match self.blocks.get(succ) {
                    None => errors.push(format!("block {id}: successor {succ} does not exist")),
                    Some(s) if !s.preds.contains(&id) => errors.push(format!(
                        "block {id}: lists successor {succ}, but {succ} has predecessors {:?}",
                        s.preds
                    )),
                    _ => (),
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Each kind is unique within a function, since the numbers come from one
    /// counter, so the first match is the only match.
    pub fn find_kind(&self, kind: BlockKind) -> Option<usize> {
        self.blocks.values().find(|b| b.kind == kind).map(|b| b.id)
    }

    /// Extracts a subgraph containing only the blocks in `region`.
    /// Edges to blocks outside the region are removed.
    pub fn create_subgraph(&self, region: &HashSet<usize>, entry: usize, exit: usize) -> Self {
        let mut new_blocks = HashMap::new();

        for &id in region {
            let old_block = &self.blocks[&id];

            let new_succs = old_block
                .succs
                .iter()
                .cloned()
                .filter(|s| region.contains(s))
                .collect();
            let new_preds = old_block
                .preds
                .iter()
                .cloned()
                .filter(|s| region.contains(s))
                .collect();

            new_blocks.insert(
                id,
                BasicBlock {
                    id,
                    lab_instructions: old_block.lab_instructions.clone(),
                    label: old_block.label.clone(),
                    kind: old_block.kind,
                    preds: new_preds,
                    succs: new_succs,
                },
            );
        }

        Cfg {
            blocks: new_blocks,
            fn_name: self.fn_name.clone(),
            entry_id: entry,
            exit_id: exit,
            globals: self.globals.clone(), // ehh not super nice
            params: self.params.clone(),
        }
    }

    /// It collects all nodes reachable from the entry of a branch up until it hits the
    /// common merge node. The merge node itself is *not* included in either region.
    pub fn get_region(&self, start_node: usize, end_id: usize) -> HashSet<usize> {
        let mut region_nodes = HashSet::new();
        let mut worklist = VecDeque::from([start_node]);
        let mut visited = HashSet::new();

        while let Some(current_id) = worklist.pop_front() {
            if current_id == end_id || !visited.insert(current_id) {
                continue; // Stop traversal if we hit the merge point or a visited node.
            }

            region_nodes.insert(current_id);

            let current_block = self.get_block(current_id);

            // Check the terminator instruction to see if we should continue the traversal.
            let last_instr = current_block.lab_instructions.last().map(|(_, i)| i);

            let is_early_exit = match last_instr {
                // An unconditional Br or Return terminates the path within this region.
                Some(W::Instruction::Br(_)) | Some(W::Instruction::Return(_)) => true,
                _ => false,
            };

            // Only add successors to the worklist if this block doesn't have an early exit.
            if !is_early_exit {
                for succ_id in &current_block.succs {
                    worklist.push_back(*succ_id);
                }
            }
        }
        region_nodes
    }

    pub fn to_dot(&self, dump_path: &std::path::Path, ext_name: &str) -> io::Result<()> {
        let mut docs: Vec<RcDoc<'_>> = Vec::new();
        // Graph header
        docs.push(RcDoc::text(format!(
            "digraph CFG {{ label=\"Function: {} \";\n labelloc=t;\n ",
            self.fn_name
        )));
        docs.push(RcDoc::hardline());
        docs.push(RcDoc::text("  node [shape=box fontname=\"monospace\"];"));
        docs.push(RcDoc::hardline());

        for (block_id, block) in &self.blocks {
            let mut label_lines = vec![format!("Block {}:\\l", block_id)];
            if let Some(name) = &block.label {
                label_lines.push(format!("Name: {}\\l", name));
            }

            for (i, instr) in &block.lab_instructions {
                label_lines.push(format!("  {}: {}\\l", i, instr));
            }
            let label = label_lines.join("");

            docs.push(
                RcDoc::text(format!("\"Block {}\" [label=\"{}\"];", block.id, label))
                    .append(RcDoc::hardline()),
            );
        }

        // Edges
        for block in self.blocks.values() {
            for succ_id in &block.succs {
                docs.push(
                    RcDoc::text(format!("\"Block {}\" -> \"Block {}\";", block.id, succ_id))
                        .append(RcDoc::hardline()),
                );
            }
        }

        // Graph footer
        docs.push(RcDoc::text("}"));
        let full_doc = RcDoc::intersperse(docs, RcDoc::hardline());
        let dump_parent = dump_path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = dump_path
            .file_name()
            .expect("File Name should exist")
            .to_string_lossy();
        let dot_path = dump_parent.join("Dot_files");
        fs::create_dir_all(&dot_path)?;
        let dot_path = dot_path.join(file_name.to_string());
        let dot_path = dot_path.with_extension("dot");
        let dot_path =
            crate::util::extend_file_name(&dot_path, &format!("_{}_{}", &self.fn_name, ext_name))
                .unwrap();
        let file = File::create(dot_path).unwrap();
        let mut buf_writer = BufWriter::new(file);
        full_doc.render(80, &mut buf_writer)
    }
}

impl CfgBuilder {
    /// Creates a block with the next ID from this builder's own counter.
    fn new_block(&mut self, instrs: &[W::Instruction], label: &Option<String>) -> BasicBlock {
        let id = self.next_id;
        self.next_id += 1;
        BasicBlock::new(id, instrs, label)
    }

    /// Pre-creates blocks for all `Block` and `Loop` labels so that
    /// branch targets can be resolved during construction.
    pub fn discover_labels(&mut self, instrs: &[W::Instruction]) {
        for instr in instrs {
            match instr {
                Instruction::Block { label, body } => {
                    // Basic Block after this whole block of instructions
                    let merge_block = self.new_block(&[], &Some(format!("merge_{}", label)));
                    self.label_map.insert(label.clone(), merge_block.id);
                    self.blocks.insert(merge_block.id, merge_block);
                    self.discover_labels(body);
                }

                Instruction::Loop { label, body } => {
                    // Basic Block will be the header at the start of loop
                    let header_block = self.new_block(&[], &Some(label.clone()));
                    self.label_map.insert(label.clone(), header_block.id);
                    self.blocks.insert(header_block.id, header_block);
                    self.discover_labels(body);
                }

                _ => {}
            }
        }
    }

    /// Recursively walks instructions, adding them to the current block and
    /// splitting at terminators or structured control flow constructs.
    /// Returns the ID of the last block in the sequence.
    pub fn build_recursive(&mut self, instrs: &[W::Instruction], mut cur_block_id: usize) -> usize {
        for (instr_idx, instr) in instrs.iter().enumerate() {
            // If current block is finished, we create a new one!
            match instr {
                W::Instruction::Block { label, body } => {
                    // 1. Get the merge block ID, which is the target for any `br` to this label.
                    let merge_block_id = *self.label_map.get(label).unwrap();
                    // 2. Create the entry block for the body of this construct.
                    let body_entry_block = self.new_block(&[], &Some(label.clone()));
                    let body_entry_id = body_entry_block.id;
                    self.blocks.insert(body_entry_id, body_entry_block);
                    // Add the edge
                    self.add_edge(cur_block_id, body_entry_id);

                    // Finish inner
                    let last_body_block_id = self.build_recursive(body, body_entry_id);
                    let last_body_block = self.blocks.get(&last_body_block_id).unwrap();

                    // 5. If the body doesn't end with a terminator, it falls through to the merge block.
                    if !last_body_block
                        .lab_instructions
                        .last()
                        .is_some_and(|(_, i)| is_terminator(i))
                    {
                        self.add_edge(last_body_block_id, merge_block_id);
                    }
                    // Next instruction should be in the merge block
                    cur_block_id = merge_block_id;
                }

                W::Instruction::Loop { label, body } => {
                    // 1. Get the loop header block ID, which is the target for `br`s to this label.
                    let header_block_id = *self.label_map.get(label).unwrap();

                    // 2. Add a fallthrough edge from the current block to the loop header.
                    self.add_edge(cur_block_id, header_block_id);

                    // 3. Recursively build the body, starting *from* the header.
                    self.build_recursive(body, header_block_id);

                    // The current block is the header, since loop body is finished
                    cur_block_id = header_block_id;
                }
                i if is_terminator(i) => {
                    // Add terminator to current block
                    let current_block = self.blocks.get_mut(&cur_block_id).unwrap();
                    current_block
                        .lab_instructions
                        .push((get_instr_id(cur_block_id, instr_idx), i.clone()));

                    match i {
                        W::Instruction::Br(label) => {
                            let target_id = *self.label_map.get(label).expect("Label not found");
                            self.add_edge(cur_block_id, target_id);
                        }
                        W::Instruction::BrIf { l, .. } => {
                            // Edge for the "true" case (branch is taken)
                            let target_id = *self.label_map.get(l).expect("Label not found");
                            self.add_edge(cur_block_id, target_id);

                            // Edge for the "false" case (fallthrough)
                            let fallthrough_block = self.new_block(&[], &None);
                            let fallthrough_id = fallthrough_block.id;
                            self.blocks.insert(fallthrough_id, fallthrough_block);
                            self.add_edge(cur_block_id, fallthrough_id);

                            // Subsequent instructions go into the fallthrough block.
                            cur_block_id = fallthrough_id;
                        }
                        W::Instruction::Return(_) => {}
                        _ => unreachable!(),
                    }
                }
                other => {
                    let current_block = self.blocks.get_mut(&cur_block_id).unwrap();
                    current_block
                        .lab_instructions
                        .push((get_instr_id(cur_block_id, instr_idx), other.clone()));
                }
            }
        }

        cur_block_id
    }

    fn add_edge(&mut self, from_id: usize, to_id: usize) {
        if let Some(from_block) = self.blocks.get_mut(&from_id)
            && !from_block.succs.contains(&to_id)
        {
            from_block.succs.push(to_id);
        }

        // Add predecessor to the 'to' block
        if let Some(to_block) = self.blocks.get_mut(&to_id)
            && !to_block.preds.contains(&from_id)
        {
            to_block.preds.push(from_id);
        }
    }
}

/// Computes a unique instruction ID from a block ID and instruction index.
///
/// Uses a heuristic of `block_id * 1000 + instr_index`, supporting up to
/// 1000 instructions per block.
pub fn get_instr_id(block_id: usize, instr_number: usize) -> usize {
    block_id * 1000 + instr_number
}

/// Recovers the block ID from an instruction ID (inverse of [`get_instr_id`]).
pub fn block_id_from_instr_id(instr_id: usize) -> usize {
    // Inverse
    instr_id / 1000
}

/// Returns `true` if the instruction is a terminator (`Return`, `Br`, or `BrIf`)
fn is_terminator(i: &W::Instruction) -> bool {
    matches!(
        i,
        Instruction::Return(_) | Instruction::Br(_) | Instruction::BrIf { .. }
    )
}
