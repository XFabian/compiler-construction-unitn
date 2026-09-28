// Regression tests for scopes that were opened but never closed.
//
// The resolver pushed a map onto its StackMap for every block and function
// but never popped one, so a name stayed visible after its scope ended.

use rwetac::{lexer, parser, resolver, typecheck};

/// Resolves and typechecks `src`. `Err` holds the first message.
fn check(src: &str) -> Result<(), String> {
    let mut parser = parser::Parser::new(lexer::Lexer::new(src));
    let mut ast = parser.parse_program().expect("test program must parse");
    resolver::Resolver::new()
        .resolve(&mut ast)
        .map_err(|e| format!("resolve: {}", e.msg))?;
    typecheck::Typechecker::new()
        .typecheck(&ast)
        .map_err(|e| format!("typecheck: {:?}", e))
}

// After the block, `y` is the outer `int` again. The inner one is a `bool`
// so that picking the wrong `y` shows up as a type error.
#[test]
fn block_scope_ends_at_closing_brace() {
    let src = "
f() : int {
    y : int = 1
    {
        y : bool = true
    }
    return y
}
";
    assert_eq!(check(src), Ok(()));
}

// The block's `x` is gone, so declaring `x` again is not a duplicate.
#[test]
fn redeclare_after_block_is_allowed() {
    let src = "
f() : int {
    {
        x : int = 1
    }
    x : int = 2
    return x
}
";
    assert_eq!(check(src), Ok(()));
}

// Locals of `f` are out of scope once `f` ends.
#[test]
fn function_locals_not_visible_in_other_functions() {
    let src = "
f() : int {
    secret : int = 1
    return secret
}
g() : int {
    return secret
}
";
    assert_eq!(
        check(src),
        Err("resolve: Undefined Variable secret found!".to_string())
    );
}

// An error in an `else` branch was discarded, and the typechecker then
// panicked on the unresolved name instead of reporting it.
#[test]
fn error_in_else_branch_is_reported() {
    let src = "
f(b : bool) : int {
    if b {
        return 1
    } else {
        return nope
    }
}
";
    assert_eq!(
        check(src),
        Err("resolve: Undefined Variable nope found!".to_string())
    );
}
