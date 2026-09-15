//! Part 1: enums and match.
//!
//! Run with `cargo run -p intro --bin part1`.
//!
//! Lines marked `ASK:` are discussion prompts, not instructions to the reader.

fn main() {
    simple_enum();
    enums_with_data();
    options();
    results();
}

// ---------------------------------------------------------------------------
// 1. An enum is a closed set of alternatives
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

fn symbol(op: Op) -> &'static str {
    match op {
        Op::Add => "+",
        Op::Sub => "-",
        Op::Mul => "*",
        Op::Div => "/",
    }
}

fn apply(op: Op, a: i64, b: i64) -> i64 {
    match op {
        Op::Add => a + b,
        Op::Sub => a - b,
        Op::Mul => a * b,
        Op::Div => a / b, // panics on zero; fixed in section 4
    }
}


fn simple_enum() {
    println!("=== 1. A simple enum ===");

    for op in [Op::Add, Op::Sub, Op::Mul, Op::Div] {
        println!("  10 {} 3 = {}", symbol(op), apply(op, 10, 3));
    }

    println!();
}

// ---------------------------------------------------------------------------
// 2. Variants can carry data
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Token {
    Int(i64),
    Operator(Op),
    LParen,
    RParen,
}

fn describe(tok: &Token) -> String {
    match tok {
        Token::Int(0) => "the literal zero".to_string(),
        Token::Int(n) if *n < 0 => format!("a negative literal, {n}"),
        Token::Int(n) => format!("a literal, {n}"),
        Token::Operator(op) => format!("the operator {}", symbol(*op)),
        Token::LParen | Token::RParen => "a bracket".to_string(),
    }
}

fn enums_with_data() {
    println!("=== 2. Variants carrying data ===");

    let tokens = [
        Token::Int(12),
        Token::Operator(Op::Mul),
        Token::LParen,
        Token::Int(-3),
        Token::RParen,
        Token::Int(0),
    ];

    for tok in &tokens {
        println!("  {:<20} is {}", format!("{tok:?}"), describe(tok));
    }

    println!();
}

// ---------------------------------------------------------------------------
// 3. Option is an ordinary enum
// ---------------------------------------------------------------------------
//
//     enum Option<T> { None, Some(T) }

fn lookup(table: &[(&str, i64)], name: &str) -> Option<i64> {
    for (key, value) in table {
        if *key == name {
            return Some(*value);
        }
    }
    None
}


fn options() {
    println!("=== 3. Option ===");

    let table = [("x", 1), ("y", 2)];

    // The long form: match, and handle both cases.
    match lookup(&table, "x") {
        Some(v) => println!("  x is bound to {v}"),
        None => println!("  x is not bound"),
    }

    // The short forms, for when one case is uninteresting.
    if let Some(v) = lookup(&table, "y") {
        println!("  y is bound to {v}");
    }
    println!("  z defaults to {}", lookup(&table, "z").unwrap_or(0));
    println!("  x doubled is {:?}", lookup(&table, "x").map(|v| v * 2));

    println!();
}

// ---------------------------------------------------------------------------
// 4. Result is an ordinary enum too
// ---------------------------------------------------------------------------
//
//     enum Result<T, E> { Ok(T), Err(E) }

fn checked_apply(op: Op, a: i64, b: i64) -> Result<i64, String> {
    if op == Op::Div && b == 0 {
        return Err("division by zero".to_string());
    }
    Ok(apply(op, a, b))
}

/// `?` returns early on `Err`, so the happy path stays flat.
fn parse_and_add(a: &str, b: &str) -> Result<i64, std::num::ParseIntError> {
    let a: i64 = a.parse()?;
    let b: i64 = b.parse()?;
    Ok(a + b)
}

fn results() {
    println!("=== 4. Result ===");

    for (a, b) in [(10, 3), (10, 0)] {
        match checked_apply(Op::Div, a, b) {
            Ok(v) => println!("  {a} / {b} = {v}"),
            Err(e) => println!("  {a} / {b} failed: {e}"),
        }
    }

    println!(
        "  parse_and_add(\"2\", \"40\")  = {:?}",
        parse_and_add("2", "40")
    );
    println!(
        "  parse_and_add(\"2\", \"forty\") = {:?}",
        parse_and_add("2", "forty")
    );

    println!();
}
