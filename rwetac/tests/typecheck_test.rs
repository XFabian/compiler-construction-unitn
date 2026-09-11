use insta::assert_snapshot;
mod common;

#[test]
fn binary_wrong_type() {
    let out = common::typecheck_expr("1 + true");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Binary Operation with invalid Type! e1 : int e2 : bool",
        span: 0..1,
    }
    "#);
}

#[test]
fn binary_wrong_type2() {
    let out = common::typecheck_expr("1 < true");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Binary Operation with invalid Type! e1 : int e2 : bool",
        span: 0..1,
    }
    "#);
}

// This is a tricky case. I guess both decisions are fine. I decide to allow it {} has type Type::Unknonw and not type Type::Array{Type::Unknown}
#[test]
fn binary_correct_type3() {
    let out = common::typecheck_expr_succeed("{} + {{3, 4}}");
    assert_snapshot!(out, @"int[][]");
}

#[test]
fn binary_correct_type() {
    let out = common::typecheck_expr_succeed("{{1,2}} + {{3, 4}}");
    assert_snapshot!(out, @"int[][]");
}

#[test]
fn binary_correct_type2() {
    let out = common::typecheck_expr_succeed("{} + {3, 4}");
    assert_snapshot!(out, @"int[]");
}
#[test]
fn arr_lit_wrong() {
    let out = common::typecheck_expr("{1, true}");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Not all elements in array literal have same type. Types: [Int, Bool]",
        span: 0..9,
    }
    "#);
}

#[test]
fn assign_wrong_type() {
    let out = common::typecheck_stmt("x, y = 1 + 3, 4, 5");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Amount of Lvalues and rhs value mismatch. Lvals: [E(Expression { kind: Var(\"x\"), span: 0..1 }), E(Expression { kind: Var(\"y\"), span: 3..4 })], RVals: [Expression { kind: Binary(Add, Expression { kind: Int(1), span: 7..8 }, Expression { kind: Int(3), span: 11..12 }), span: 7..12 }, Expression { kind: Int(4), span: 14..15 }, Expression { kind: Int(5), span: 17..18 }]",
        span: 0..18,
    }
    "#);
}

#[test]
fn assign_wrong_type2() {
    let out = common::typecheck_stmt("a[2] = 1, 2");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Amount of Lvalues and rhs value mismatch. Lvals: [E(Expression { kind: Subscript(Expression { kind: Var(\"a\"), span: 0..1 }, Expression { kind: Int(2), span: 2..3 }), span: 0..4 })], RVals: [Expression { kind: Int(1), span: 7..8 }, Expression { kind: Int(2), span: 10..11 }]",
        span: 0..11,
    }
    "#);
}

#[test]
fn assign_wrong_type3() {
    let out = common::typecheck_stmt("x = test(1,2)");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Amount of Lvalues and rhs value mismatch. Lvals: [E(Expression { kind: Var(\"x\"), span: 0..1 })], RVals: [Expression { kind: Call { name: \"test\", args: [Expression { kind: Int(1), span: 9..10 }, Expression { kind: Int(2), span: 11..12 }] }, span: 4..13 }]",
        span: 0..13,
    }
    "#);
}

#[test]
fn assign_wrong_type4() {
    let out = common::typecheck_stmt("2 = 6");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Not all expression on LHS are Lvalues. LVals : [E(Expression { kind: Int(2), span: 0..1 })]",
        span: 0..5,
    }
    "#);
}

#[test]
fn assign_correct_type() {
    let out = common::typecheck_stmt_succeed("x, y, z = test(1,2), 2");
    assert_snapshot!(out, @"Ok(())");
}

#[test]
fn assign_correct_type2() {
    let _out = common::typecheck_stmt_succeed("_ = 2");
    let out = common::typecheck_stmt_succeed("_ = true");
    assert_snapshot!(out, @"Ok(())");
}

#[test]
fn arr_assign_correct_type() {
    let out = common::typecheck_stmt_succeed("a[2] = 2");
    assert_snapshot!(out, @"Ok(())");
}
#[test]
fn arr_assign_correct_type2() {
    let out = common::typecheck_stmt_succeed("a = {1,3,4}");
    assert_snapshot!(out, @"Ok(())");
}

#[test]
fn arr_assign_correct_type2_5() {
    let out = common::typecheck_stmt_succeed("a = {}");
    assert_snapshot!(out, @"Ok(())");
}

#[test]
fn arr_assign_correct_type3() {
    let out = common::typecheck_stmt_succeed("a = {1,3,4} + {3, 4, 5, 7}");
    assert_snapshot!(out, @"Ok(())");
}

#[test]
fn arr_assign_correct_type4() {
    let out = common::typecheck_stmt_succeed("a[a[3]] = 4");
    assert_snapshot!(out, @"Ok(())");
}

#[test]
fn arr_decl_correct_type() {
    let out = common::typecheck_stmt_succeed("g : int[] = {}");
    assert_snapshot!(out, @"Ok(())");
}

#[test]
fn arr_decl_wrong_type() {
    let out = common::typecheck_stmt("a : int[][4]");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "A specified dimension cannot follow after an unspecified one, Array a",
        span: 0..12,
    }
    "#);
}

#[test]
fn arr_decl_wrong_type2() {
    let out = common::typecheck_stmt("a : int[4][][5]");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "A specified dimension cannot follow after an unspecified one, Array a",
        span: 0..15,
    }
    "#);
}

#[test]
fn arr_decl_wrong_type3() {
    let out = common::typecheck_stmt("a : int[3] = {1,2}");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Cannot specify dimensions and initialize array. Not allowed.",
        span: 0..10,
    }
    "#);
}

#[test]
fn arr_decl_wrong_type4() {
    let out = common::typecheck_stmt("a : int[true]");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Non-Int value used as Index into Array!, Array a",
        span: 0..13,
    }
    "#);
}

#[test]
fn arr_decl_wrong_type5() {
    let out = common::typecheck_stmt("a : int[test(2, 3)]");
    assert_snapshot!(out, @r#"
    Failure: Generic {
        msg: "Non-Int value used as Index into Array!, Array a",
        span: 0..19,
    }
    "#);
}

#[test]
fn arr_decl_wrong_type6() {
    let out = common::typecheck_stmt("b_arr : int[][] = {1,2}");
    assert_snapshot!(out, @r#"
    Failure: MismatchedTypes {
        msg: "Initializer has wrong type for the array. Found int[]",
        expected: Array {
            elem_type: Array {
                elem_type: Int,
            },
        },
        span: 18..23,
    }
    "#);
}

#[test]
fn length_correct_type() {
    let out = common::typecheck_stmt_succeed("l : int= length(a)");
    assert_snapshot!(out, @"Ok(())");
}

#[test]
fn length_correct_type2() {
    let out = common::typecheck_stmt_succeed("l : int = length(b)");
    assert_snapshot!(out, @"Ok(())");
}

#[test]
fn if_guard_wrong_type() {
    let out = common::typecheck_stmt("if 2 {}");
    assert_snapshot!(out, @r#"
    Failure: MismatchedTypes {
        msg: "Guard of If has to have Type bool. Found int",
        expected: Bool,
        span: 3..4,
    }
    "#);
}

#[test]
fn while_guard_wrong_type() {
    let out = common::typecheck_stmt("while 2 {}");
    assert_snapshot!(out, @r#"
    Failure: MismatchedTypes {
        msg: "Guard of While has to have Type bool. Found int",
        expected: Bool,
        span: 6..7,
    }
    "#);
}
