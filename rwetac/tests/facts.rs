//! The `--facts` listing, on the straight-line examples of the dataflow
//! lecture. The snapshots pin down the format and the facts themselves.

use rwetac::opt::{cfg::Cfg, facts};
use rwetac::resolver::Resolver;
use rwetac::setting::{FactsKind, FactsRequest};
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

/// The listing of `function` in `src`, temporaries hidden.
fn facts_of(src: &str, function: &str, kind: FactsKind) -> String {
    let cfg = cfgs_for(src)
        .into_iter()
        .find(|cfg| cfg.fn_name == function)
        .expect("no such function");
    facts::run(&cfg, FactsRequest { kind, all: false }).to_string()
}

#[test]
fn reaching_definitions_gen_and_kill() {
    let src = "
f() : int {
    x : int = 5
    y : int = x
    x = 7
    z : int = x + y
    return z
}
";
    insta::assert_snapshot!(facts_of(src, "f", FactsKind::Reaching));
}

#[test]
fn liveness_one_name_two_values() {
    let src = "
f() : int {
    a : int = 1
    b : int = a + 2
    a = 7
    return a + b
}
";
    insta::assert_snapshot!(facts_of(src, "f", FactsKind::Liveness));
}
