//! Tokenises Eta source with the compiler's own lexer.
//!
//! Nothing here reimplements lexing: the `Token` enum and the `Lexer` come
//! straight out of `rwetac`, so whatever this prints is exactly what the
//! compiler itself produces.
//!
//! With no arguments it walks a set of short lines. Give it source text or a
//! file to tokenise that instead.

use logos::Logos;
use rwetac::lexer;
use rwetac::token::Token;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        None => {
            println!("logos = the raw generated token stream");
            println!("lexer = after this compiler's wrapper; * marks an inserted semicolon\n");
            for (note, src) in SNIPPETS {
                println!("--- {note}");
                show(src);
            }
        }
        Some("--file") => {
            let path = args.get(1).expect("--file needs a path");
            let src = std::fs::read_to_string(path).expect("cannot read file");
            show(&src);
        }
        Some(_) => show(&args.join(" ")),
    }
}

/// Lines worth stopping on, each for a different reason.
const SNIPPETS: &[(&str, &str)] = &[
    ("one token per symbol", "a = 1 + 2"),
    ("longest match wins: `==` is one token, not two", "a == b"),
    ("keywords are not identifiers", "if while return true"),
    ("identifiers may contain keywords", "iffy whilst returnValue"),
    ("literals are parsed here, not later", "x = 42"),
    ("comments never reach the parser", "a = 1 // the rest of this line is gone"),
    ("a newline after `1` becomes a semicolon", "a = 1\nb = 2"),
    ("a newline after `+` does not", "a = 1 +\n2"),
    ("the lexer does not care that this cannot parse", ") } ; = =")
];

fn show(src: &str) {
    println!("source: {:?}", src);

    // The raw logos stream, before the compiler's wrapper touches it.
    let raw: Vec<String> = Token::lexer(src)
        .map(|t| match t {
            Ok(tok) => format!("{tok:?}"),
            Err(_) => "<invalid token>".to_string(),
        })
        .collect();
    print_wrapped("  logos : ", &raw);

    // After the compiler's own Lexer, which is where semicolons appear.
    let mut out = Vec::new();
    for item in lexer::tokenize(src) {
        match item {
            Ok((tok, span)) => {
                // An inserted semicolon has no `;` behind it in the source.
                let inserted = tok == Token::Semicolon && src.get(span.clone()) != Some(";");
                if inserted {
                    out.push("Semicolon*".to_string());
                } else {
                    out.push(format!("{tok:?}"));
                }
            }
            Err(e) => out.push(format!("<{e}>")),
        }
    }
    print_wrapped("  lexer : ", &out);
    println!();
}

/// Wraps a token list so a whole file stays readable on a projector.
fn print_wrapped(label: &str, items: &[String]) {
    let indent = " ".repeat(label.len());
    let mut line = String::new();
    let mut first = true;
    for item in items {
        if !line.is_empty() && line.len() + 1 + item.len() > 76 {
            println!("{}{}", if first { label } else { &indent }, line);
            first = false;
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(item);
    }
    println!("{}{}", if first { label } else { &indent }, line);
}
