//! Part 2: a calculator for arithmetic expressions -- no variables.
//!
//! An AST, a pretty printer, and an evaluator. These are the first three
//! pieces of a compiler; the parser that builds the AST from text comes later
//! in the course, so here we build it by hand.
//!
//! Run with `cargo run -p intro --bin part2`.
//!
//! Lines marked `ASK:` are discussion prompts, not instructions to the reader.

fn main() {
    for e in examples() {
        println!("{}", pretty(&e));
        match eval(&e) {
            Ok(v) => println!("  = {v}"),
            Err(e) => println!("  = error, {e:?}"),
        }
        println!();
    }
}

// ---------------------------------------------------------------------------
// The AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

/// An arithmetic expression.
///
/// `Box` is what makes this legal: an `Expr` that directly contained two more
/// `Expr`s would have no finite size, so the children live behind a pointer.
#[derive(Debug)]
enum Expr {
    Num(i64),
    Neg(Box<Expr>),
    Bin {
        op: Op,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

// ASK: why is the tree the right shape for this, rather than a list of tokens?
//      Where did the brackets from the source text go?

// Constructors, so the examples below read like the expressions they build.
fn num(n: i64) -> Expr {
    Expr::Num(n)
}
fn neg(e: Expr) -> Expr {
    Expr::Neg(Box::new(e))
}
fn bin(op: Op, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Bin {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

fn examples() -> Vec<Expr> {
    vec![
        // 1 + 2 * 3
        bin(Op::Add, num(1), bin(Op::Mul, num(2), num(3))),
        // (1 + 2) * 3
        bin(Op::Mul, bin(Op::Add, num(1), num(2)), num(3)),
        // (10 - 4) - 3
        bin(Op::Sub, bin(Op::Sub, num(10), num(4)), num(3)),
        // 10 - (4 - 3)
        bin(Op::Sub, num(10), bin(Op::Sub, num(4), num(3))),
        // -(2 + 3) * 4
        bin(Op::Mul, neg(bin(Op::Add, num(2), num(3))), num(4)),
        // 1 / 0
        bin(Op::Div, num(1), num(0)),
    ]
}

// ASK: the first two examples contain the same numbers and the same operators.
//      What distinguishes them, and why can no amount of re-reading the text
//      "1 + 2 * 3" tell you which one the programmer meant?

// ---------------------------------------------------------------------------
// Pretty printing
// ---------------------------------------------------------------------------

fn symbol(op: Op) -> &'static str {
    match op {
        Op::Add => "+",
        Op::Sub => "-",
        Op::Mul => "*",
        Op::Div => "/",
    }
}

/// Prints the tree back as text. Every node brackets itself, so the shape of
/// the tree is visible in the output.
fn pretty(e: &Expr) -> String {
    match e {
        Expr::Num(n) => n.to_string(),
        Expr::Neg(inner) => format!("(-{})", pretty(inner)),
        Expr::Bin { op, lhs, rhs } => {
            format!("({} {} {})", pretty(lhs), symbol(*op), pretty(rhs))
        }
    }
}

// ASK: nothing here looks at what the brackets in the source text were. Why can
//      the printer get away with that?

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum EvalError {
    DivisionByZero,
}

fn eval(e: &Expr) -> Result<i64, EvalError> {
    Ok(match e {
        Expr::Num(n) => *n,
        Expr::Neg(inner) => -eval(inner)?,
        Expr::Bin { op, lhs, rhs } => {
            let (l, r) = (eval(lhs)?, eval(rhs)?);
            match op {
                Op::Add => l + r,
                Op::Sub => l - r,
                Op::Mul => l * r,
                Op::Div if r == 0 => return Err(EvalError::DivisionByZero),
                Op::Div => l / r,
            }
        }
    })
}

// ASK: `pretty` and `eval` have the same shape -- one match, three arms, two
//      recursive calls. Why? What else could you write against this AST?

// ---------------------------------------------------------------------------
// Exercises
// ---------------------------------------------------------------------------
//
// Each one is a function to fill in and a line to add to `main` to see it run.
// The stubs are marked `allow(dead_code)` only so the file compiles before you
// have called them; delete that once you do.

/// Exercise 1: count the additions.
///
/// Return how many `Op::Add` nodes the expression contains. The shape is the
/// same as `eval`: match on the node, recurse into the children, combine.
///
/// On `(1 + (2 * 3))` the answer is 1, on `((1 + 2) * 3)` it is also 1, and on
/// a tree with no `+` at all it is 0.
///
/// Then in `main`:
///
///     println!("  additions: {}", count_additions(&e));
#[allow(dead_code, unused_variables)]
fn count_additions(e: &Expr) -> usize {
    todo!("exercise 1")
}

/// Exercise 2: simplify `x + 0` to `x`.
///
/// Return a new expression with every `x + 0` and `0 + x` replaced by `x`,
/// wherever they appear in the tree. Leave everything else alone.
///
/// Simplify the children *first*, then look at the node you are standing on.
/// Otherwise `((1 + 0) + 0)` only loses one of its two zeros -- the inner
/// `1 + 0` does not become a plain `1` until after you have already decided
/// what to do with the outer node.
///
/// Build yourself a test expression, since `examples()` has none:
///
///     let e = bin(Op::Add, bin(Op::Add, num(1), num(0)), num(0));
///     println!("{} simplifies to {}", pretty(&e), pretty(&simplify(&e)));
///
/// Can you think of other optimizations?
#[allow(dead_code, unused_variables)]
fn simplify(e: &Expr) -> Expr {
    todo!("exercise 2")
}

// ---------------------------------------------------------------------------
// Exercise 3: variables
// ---------------------------------------------------------------------------
//
// Touches every function in the file.
//
//   1. Give `Expr` a new variant, `Var(String)`.
//
//   2. `pretty` gains an arm: a variable prints as its name. The compiler will
//      tell you where to add it -- that is the exhaustiveness check from Part 1
//      doing your planning for you.
//
//   3. `eval` is the interesting one, because it can no longer work as written.
//      `Expr::Var("x".to_string())` has no value of its own. Nothing in the
//      tree says what `x` is, so the value has to come from outside, and it has
//      to reach every recursive call. That outside thing is an *environment*: a
//      mapping from name to value, threaded through the recursion as a
//      parameter.
//
//          use std::collections::HashMap;
//
//          fn eval(e: &Expr, env: &HashMap<String, i64>) -> Result<i64, EvalError>
//
//      Every recursive call passes `env` along; the `Var` arm looks the name up
//      in it.
//
//   4. A lookup can fail -- nothing stops you writing `x` with an empty
//      environment -- so `EvalError` needs a second variant for an unbound
//      name. Note that this is a *different* kind of failure from
//      `DivisionByZero`: it can be caught before running anything, just by
//      walking the tree. That check is a typechecker, and it is a later lecture.
//
// ASK: why must the environment be a parameter, rather than a global?

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// Unit tests live in the same file as the code they test. `#[cfg(test)]` means
// the module is compiled only by `cargo test`, so it costs nothing in a normal
// build, and `use super::*` brings in the file's items -- private ones included,
// which is why these tests can call `bin`, `num` and `pretty` directly.
//
// Run them with `cargo test -p intro`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplication_nested_on_the_right() {
        let e = bin(Op::Add, num(1), bin(Op::Mul, num(2), num(3)));
        assert_eq!(pretty(&e), "(1 + (2 * 3))");
        assert_eq!(eval(&e).unwrap(), 7);
    }

    #[test]
    fn the_same_numbers_in_a_different_tree() {
        let e = bin(Op::Mul, bin(Op::Add, num(1), num(2)), num(3));
        assert_eq!(pretty(&e), "((1 + 2) * 3)");
        assert_eq!(eval(&e).unwrap(), 9);
    }

    #[test]
    fn subtraction_groups_to_the_left() {
        let flat = bin(Op::Sub, bin(Op::Sub, num(10), num(4)), num(3));
        assert_eq!(pretty(&flat), "((10 - 4) - 3)");
        assert_eq!(eval(&flat).unwrap(), 3);

        let nested = bin(Op::Sub, num(10), bin(Op::Sub, num(4), num(3)));
        assert_eq!(pretty(&nested), "(10 - (4 - 3))");
        assert_eq!(eval(&nested).unwrap(), 9);
    }

    #[test]
    fn division_by_zero_is_an_error() {
        let e = bin(Op::Div, num(1), num(0));
        assert!(eval(&e).is_err());
    }
}
