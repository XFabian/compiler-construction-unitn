//! The facts of an analysis, ready to print (`--facts`).
//!
//! [`collect`] turns the result of an analysis into a [`FactDump`]: plain
//! strings per block and per instruction, in program order. A printer only
//! reads the dump, so it works the same for every analysis, whatever its fact
//! type is.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use crate::opt::cfg::Cfg;
use crate::opt::liveness::LivenessAnalysis;
use crate::opt::reaching::ReachingAnalysis;
use crate::opt::solver::{Analysis, AnalysisResults, Direction, Solver};
use crate::setting::{FactsKind, FactsRequest};

/// The facts of one analysis for one function.
#[derive(Debug)]
pub struct FactDump {
    pub function: String,
    /// The name of the analysis, e.g. "reaching definitions".
    pub analysis: &'static str,
    pub direction: Direction,
    pub entry: usize,
    pub exit: usize,
    /// The blocks in reverse postorder.
    pub blocks: Vec<BlockFacts>,
}

/// The facts of one basic block. Sets are sorted, and in program order:
/// `start` holds before the first instruction, `end` after the last, for a
/// backward analysis too.
#[derive(Debug)]
pub struct BlockFacts {
    pub id: usize,
    pub label: Option<String>,
    pub preds: Vec<usize>,
    pub succs: Vec<usize>,
    pub start: Vec<String>,
    pub end: Vec<String>,
    pub instrs: Vec<InstrFacts>,
}

/// The facts just before and just after one instruction.
#[derive(Debug)]
pub struct InstrFacts {
    pub id: usize,
    /// The instruction as the WTAC printer writes it.
    pub text: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// Runs the analysis `request` asks for on `cfg` and collects its facts.
pub fn run(cfg: &Cfg, request: FactsRequest) -> FactDump {
    match request.kind {
        FactsKind::Reaching => {
            let reach_ana = ReachingAnalysis::new(cfg);
            let results = solve(cfg, reach_ana.clone());
            collect(cfg, &reach_ana, &results, "reaching definitions", request.all)
        }
        FactsKind::Liveness => {
            let live_ana = LivenessAnalysis::new(cfg);
            let results = solve(cfg, live_ana.clone());
            collect(cfg, &live_ana, &results, "live variables", request.all)
        }
    }
}

fn solve<A: Analysis>(cfg: &Cfg, analysis: A) -> AnalysisResults<A> {
    let mut solver = Solver::new(cfg, analysis);
    solver.solve();
    solver.into_results()
}

/// Describes every fact in `results` with `analysis`. Unless `all` is set,
/// the compiler's temporaries are left out.
pub fn collect<A: Analysis>(
    cfg: &Cfg,
    analysis: &A,
    results: &AnalysisResults<A>,
    name: &'static str,
    all: bool,
) -> FactDump {
    let describe = |fact: &A::Fact| -> Vec<String> {
        let mut items: Vec<String> = analysis
            .describe_fact(fact)
            .into_iter()
            .filter(|item| all || !is_temporary(item))
            .collect();
        items.sort();
        items
    };
    let blocks = cfg
        .reverse_postorder()
        .into_iter()
        .map(|id| {
            let block = cfg.get_block(id);
            BlockFacts {
                id,
                label: block.label.clone(),
                preds: block.preds.clone(),
                succs: block.succs.clone(),
                start: describe(results.block_in(id)),
                end: describe(results.block_out(id)),
                instrs: block
                    .lab_instructions
                    .iter()
                    .map(|(instr_id, instr)| InstrFacts {
                        id: *instr_id,
                        text: instr.to_string().trim().to_string(),
                        before: describe(results.instr_in(id, *instr_id)),
                        after: describe(results.instr_out(id, *instr_id)),
                    })
                    .collect(),
            }
        })
        .collect();
    FactDump {
        function: cfg.fn_name.clone(),
        analysis: name,
        direction: A::direction(),
        entry: cfg.get_entry(),
        exit: cfg.get_exit(),
        blocks,
    }
}

/// Names the compiler makes up: temporaries and the stack pointer. An item
/// starts with the variable's name (`tmp.4`, `tmp.4@3001`).
fn is_temporary(item: &str) -> bool {
    item.starts_with("tmp.") || item.starts_with("__stack_pointer")
}

/// Writes `dump` next to the other debug files, as
/// `<program>_<function>.<kind>.facts`: `02_running_fact.reaching.facts`.
pub fn write_file(dump: &FactDump, dump_path: &Path, kind: FactsKind) -> io::Result<()> {
    let parent = dump_path.parent().unwrap_or_else(|| Path::new("."));
    let program = dump_path
        .file_stem()
        .expect("File Name should exist")
        .to_string_lossy();
    let file = format!("{program}_{}.{}.facts", dump.function, kind.name());
    fs::write(parent.join(file), dump.to_string())
}

/// The annotated listing: block by block, each instruction with the facts
/// that hold after it. The line under a block's header holds the facts at
/// its start.
///
/// ```text
/// Block 2 func_exit   <- 1   -> 0
///                                       {}
///   2003: x.0 : i64 = 5:i64             {x.0@2003}
///   2004: y.1 : i64 = x.0               {x.0@2003, y.1@2004}
/// ```
impl fmt::Display for FactDump {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let direction = match self.direction {
            Direction::Forwards => "forward",
            Direction::Backwards => "backward",
        };
        writeln!(f, "{}: {} ({direction})", self.function, self.analysis)?;
        // The facts start in one column, right of the longest instruction.
        let column = self
            .blocks
            .iter()
            .flat_map(|block| &block.instrs)
            .map(|instr| instr_line(instr).chars().count())
            .max()
            .unwrap_or(0)
            + 4;
        for block in &self.blocks {
            writeln!(f)?;
            writeln!(f, "{}", self.block_header(block))?;
            writeln!(f, "{:column$}{}", "", set(&block.start))?;
            for instr in &block.instrs {
                writeln!(f, "{:column$}{}", instr_line(instr), set(&instr.after))?;
            }
        }
        Ok(())
    }
}

impl FactDump {
    /// `Block 2 while_loop.8   <- 1, 3   -> 4, 0`
    fn block_header(&self, block: &BlockFacts) -> String {
        let mut line = format!("Block {}", block.id);
        if let Some(label) = &block.label {
            line += &format!(" {label}");
        }
        if block.id == self.entry {
            line += " (entry)";
        }
        if block.id == self.exit {
            line += " (exit)";
        }
        if !block.preds.is_empty() {
            line += &format!("   <- {}", ids(&block.preds));
        }
        if !block.succs.is_empty() {
            line += &format!("   -> {}", ids(&block.succs));
        }
        line
    }
}

fn instr_line(instr: &InstrFacts) -> String {
    format!("  {}: {}", instr.id, instr.text)
}

fn set(items: &[String]) -> String {
    format!("{{{}}}", items.join(", "))
}

fn ids(ids: &[usize]) -> String {
    ids.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
