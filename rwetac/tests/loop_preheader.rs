//! What the CFG guarantees to a pass that wants to hoist out of a loop.
//!
//! These are the invariants that let a licm pass treat the graph as a black box:
//! it finds loops and moves instructions, and never has to reshape anything.
//! They are checked here so that a change to `wtac_gen` or to reconstruction
//! cannot quietly take them away.
//!
//! Because Eta has only `while`, `wtac_gen` emits every loop as
//! `block $while_end.N { loop $while_loop.N { ... } }`. The `while_end` block is
//! therefore always present, always empty, dominates the header, and sits
//! outside the loop -- which is exactly a preheader, already built.

use rwetac::opt::{
    cfg::{BlockKind, Cfg},
    dominators, reconstruct_cfg,
};
use rwetac::resolver::Resolver;
use rwetac::typecheck::Typechecker;
use rwetac::wtac::wtac_ast as W;
use rwetac::wtac::wtac_gen::WtacGen;

fn cfgs_for(src: &str) -> Vec<Cfg> {
    let lexer = rwetac::lexer::Lexer::new(src);
    let mut parser = rwetac::parser::Parser::new(lexer);
    let mut ast = parser.parse().expect("parse failed");
    Resolver::new().resolve(&mut ast).ok();
    let mut tc = Typechecker::new();
    tc.typecheck(&ast).expect("typecheck failed");
    let prog = WtacGen::new(tc).generate(ast);

    let globals: Vec<_> = prog
        .decls
        .iter()
        .filter(|t| matches!(t, W::TopLevel::Data { .. }))
        .cloned()
        .collect();

    prog.decls
        .into_iter()
        .filter_map(|tl| match tl {
            W::TopLevel::Function {
                name,
                body,
                params,
                locals,
                ..
            } => Some(Cfg::new(name, &params, &body, locals, &globals)),
            W::TopLevel::Data { .. } => None,
        })
        .collect()
}

/// Returns `(header_id, latches, entry_preds)` for every natural loop.
fn loops_of(cfg: &Cfg) -> Vec<(usize, Vec<usize>, Vec<usize>)> {
    let doms = dominators::compute_dominators(cfg);
    let mut found = Vec::new();
    for (id, block) in &cfg.blocks {
        let latches: Vec<usize> = block
            .preds
            .iter()
            .copied()
            .filter(|p| doms.doms.get(p).is_some_and(|d| d.contains(id)))
            .collect();
        if latches.is_empty() {
            continue;
        }
        let entries = block
            .preds
            .iter()
            .copied()
            .filter(|p| !latches.contains(p))
            .collect();
        found.push((*id, latches, entries));
    }
    found.sort();
    found
}

const NESTED: &str = "\
main() : int {
    s : int = 0
    i : int = 0
    while (i < 4) {
        j : int = 0
        while (j < 4) {
            s = s + 1
            j = j + 1
        }
        i = i + 1
    }
    return s
}
";

/// One back edge per loop, so nothing downstream ever has to unify latches.
/// This is an invariant of the *compiler*, not an exercise: reconstruction
/// rebuilds a loop as a single `Loop` around one linearized body, which a second
/// latch would not describe. `find_innermost_header` asserts on it too; this test
/// is the cheaper place to notice. Adding `continue` (Project 2) is what would
/// break it.
#[test]
fn every_loop_has_exactly_one_latch() {
    for cfg in cfgs_for(NESTED) {
        for (header, latches, _) in loops_of(&cfg) {
            assert_eq!(
                latches.len(),
                1,
                "{}: loop header {header} has latches {latches:?}",
                cfg.fn_name
            );
        }
    }
}

/// The preheader is dedicated: a single entry predecessor whose only successor
/// is the header, so hoisting into it cannot affect any other path.
#[test]
fn every_loop_has_a_dedicated_preheader() {
    // Every function is checked, but only `main` has loops -- `length` is a
    // runtime builtin that comes along with each program.
    let mut seen = 0;
    for cfg in cfgs_for(NESTED) {
        let found = loops_of(&cfg);
        seen += found.len();
        for (header, _, entries) in found {
            assert_eq!(
                entries.len(),
                1,
                "{}: loop header {header} has entry preds {entries:?}",
                cfg.fn_name
            );
            let pre = cfg.get_block(entries[0]);
            assert_eq!(
                pre.succs,
                vec![header],
                "{}: preheader {} is shared",
                cfg.fn_name,
                pre.id
            );
            assert!(
                matches!(pre.kind, BlockKind::LoopContainer(_)),
                "{}: preheader {} is {:?}",
                cfg.fn_name,
                pre.id,
                pre.kind
            );
        }
    }
    assert_eq!(seen, 2, "expected to find both loops of the nest");
}

/// Reconstruction collapses the preheader into the loop's container block, so
/// anything hoisted into it has to be carried out explicitly. It used to be
/// dropped, which made hoisting silently do nothing.
#[test]
fn instructions_hoisted_into_the_preheader_survive_reconstruction() {
    for mut cfg in cfgs_for(NESTED) {
        let loops = loops_of(&cfg);
        if loops.is_empty() {
            continue;
        }
        for (i, (_, _, entries)) in loops.iter().enumerate() {
            let pre_id = entries[0];
            let marker = W::Instruction::Copy {
                src: W::Value::Imm(4242 + i as i64, W::WType::I64),
                dst: W::Value::Var(format!("hoisted.{i}")),
                t: W::WType::I64,
            };
            let instr_id = rwetac::opt::cfg::get_instr_id(pre_id, 900);
            cfg.get_block_mut(pre_id)
                .lab_instructions
                .push((instr_id, marker));
        }

        let mut doms = dominators::compute_dominators(&cfg);
        let out = reconstruct_cfg::reconstruct(
            &mut cfg,
            &mut doms.doms,
            &mut doms.ipdoms,
            std::path::Path::new("."),
        );
        let text = format!("{out:?}");
        for i in 0..loops.len() {
            assert!(
                text.contains(&format!("hoisted.{i}")),
                "hoisted.{i} was dropped by reconstruction"
            );
        }
    }
}
