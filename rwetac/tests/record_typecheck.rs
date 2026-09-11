use insta::assert_snapshot;
use rwetac::types;
mod common;

#[test]
fn record_not_mem() {
    let out = common::typecheck_stmt("a : int = p.z");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Field z does not exist in Record Point",
        span: 10..13,
    }
    "#);
}

#[test]
fn record_not_mem2() {
    let out = common::typecheck_stmt("a : int = p.x.z");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Expression in Dot has to be Record Type!",
        span: 10..15,
    }
    "#);
}

// #[test]
// fn record_undef() {
//     let out = expect_failure(|| {
//         let ast = parse_stmt("p : Point5D");
//         // simulate resolve + typecheck
//         let _resolved = resolve_stmt(&ast);
//         typecheck_stmt(&ast)
//     });
//     assert_snapshot!(out, @r###"
//     Failure: Struct type is not defined
//     "###);
// }

#[test]
fn record_wrong_init() {
    let out = common::typecheck_stmt("p : Point = Point()");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Number of arguments provided does not fit with function type.\n Function Point Param Type : [Int, Int] and Arguments []",
        span: 12..19,
    }
    "#);
}
#[test]
fn record_correct_init2() {
    let out = common::typecheck_stmt_succeed("p : Point = Point(2, 3)");
    assert_snapshot!(out, @"Ok(())");
}

#[test]
fn record_wrong_assign() {
    let out = common::typecheck_stmt("p.x = true");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Types of LHS and RHS are mismatched. LHS: [Int] RHS: [Bool]",
        span: 0..10,
    }
    "#);
}

// #[test]
// fn record_dupl_member() {
//     let out = expect_failure(|| {
//         let ast = parse_stmt("record Point3 { x : int; x : int }");
//         let _resolved = resolve_stmt(&ast);
//         typecheck_stmt(&ast)
//     });
//     assert_snapshot!(out, @r###"
//     Failure: Member already declared in record
//     "###);
// }

#[test]
fn record_correct_assign() {
    let mut env = common::setup("p.x = 2");
    // This one should succeed, so we don’t wrap it in expect_failure.
    let ast = env.parser.parse_statement().unwrap();
    let result = env.tchecker.typecheck_statement(&ast, &types::Type::Bool);
    assert_snapshot!(format!("{result:?}"), @"Ok(Unit)");
}
