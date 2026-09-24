use std::collections::HashMap;

// Exercise 3
fn main() {
    for mut e in examples() {
        println!("{}", pretty(&e));
        let mut hm : HashMap<String, i64> = HashMap::new();
        hm.insert("x".to_string(), 8);
        match eval(&e, &hm) {
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
    Var(String)
}


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
fn var(s: String) -> Expr {
    Expr::Var(s)
}

fn examples() -> Vec<Expr> {
    vec![
        // 1 + 2 * x
        bin(Op::Add, num(1), bin(Op::Mul, num(2), var("x".to_string()))),
        // (x + x) * 3
        bin(Op::Mul, bin(Op::Add, var("x".to_string()), var("x".to_string())), num(3)),
        // (10 - x) - y
        bin(Op::Sub, bin(Op::Sub, num(10), var("x".to_string())), var("y".to_string())),
    ]
}


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
        Expr::Var(s) => s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum EvalError {
    DivisionByZero,
    NoBinding
}

fn eval(e: &Expr, env: &HashMap<String, i64>) -> Result<i64, EvalError> {
    Ok(match e {
        Expr::Num(n) => *n,
        Expr::Neg(inner) => -eval(inner, env)?,
        Expr::Bin { op, lhs, rhs } => {
            let (l, r) = (eval(lhs, env)?, eval(rhs, env)?);
            match op {
                Op::Add => l + r,
                Op::Sub => l - r,
                Op::Mul => l * r,
                Op::Div if r == 0 => return Err(EvalError::DivisionByZero),
                Op::Div => l / r,
            }
        }
        Expr::Var(s) => {
            let binding = env.get(s);
            match binding {
                None => {return Err(EvalError::NoBinding)}
                Some(ss) => {*ss}
            }
        }
    })
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
