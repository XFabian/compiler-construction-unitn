# Demo 3.3 — Lexing with logos

Tokenising short lines of Eta with the compiler's own lexer. Nothing here
reimplements anything: the `Token` enum and the `Lexer` are imported from
`rwetac`, so what this prints is what the compiler itself produces.

Run from the repository root.

```
cargo run -p lexdemo                    // walk the built-in lines
cargo run -p lexdemo -- "a[i] = p.x"    // tokenise a line you type
cargo run -p lexdemo -- --file demos/2.5-ast/05_arrays.eta
```

Each line is shown twice:

```
--- a newline after `1` becomes a semicolon
source: "a = 1\nb = 2"
  logos : Var("a") Equal Int(1) Newline Var("b") Equal Int(2)
  lexer : Var("a") Equal Int(1) Semicolon* Var("b") Equal Int(2) Semicolon*
```

`logos` is the raw generated stream. `lexer` is after this compiler's wrapper,
which is where `Newline` turns into `Semicolon` — marked `*` because there is no
`;` in the source. That difference is the whole point of the demo.

## Files to show

| File | What to point at |
|---|---|
| [`rwetac/src/token.rs`](../../rwetac/src/token.rs) | the `Token` enum. Every variant is annotated, and those annotations *are* the lexer |
| [`rwetac/src/lexer.rs`](../../rwetac/src/lexer.rs) | `is_semicolon_candidate`, then the `Iterator` impl that uses it |

Read them in that order: what the tokens are, then what this compiler adds on
top of what logos gives you.

### `token.rs` — the lexer is a derive

```rust
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\f]+")]
pub enum Token {
    #[regex("[_a-zA-Z][_0-9a-zA-Z]*", |lex| lex.slice().to_string())]
    Var(String),

    #[regex("[0-9]+", |lex| lex.slice().parse::<i64>().unwrap())]
    Int(i64),

    #[token("+")]
    Plus,
    ...
}
```

Four things worth drawing out, each visible in the demo output:

- **`#[token]` is a literal, `#[regex]` is a pattern.** There are about 40 of
  them and together they are the entire lexical specification.
- **The callback converts.** `Int(i64)` holds a number, not the text `"42"` —
  `parse::<i64>()` has already run by the time the parser sees it.
- **Longest match wins.** `==` lexes as one `DoubleEquals`, never two `Equal`.
  Run the demo line to see it.
- **Skipping is declarative.** Whitespace is skipped by the `#[logos(skip ...)]`
  attribute and comments by `#[regex(r"//[^\n]*", logos::skip)]`, so neither
  ever reaches the parser.

### `lexer.rs` — what logos does not do

logos gives a token stream. It has no idea what a statement is, so semicolon
insertion is written by hand:

```rust
fn is_semicolon_candidate(tok: &Token) -> bool {
    matches!(tok, Token::Var(_) | Token::Int(_) | Token::RParen | Token::RBrace | ...)
}
```

A newline after a token in that list becomes a `Semicolon`; a newline anywhere
else is dropped. That is the whole rule, and the two demo lines `a = 1\nb = 2`
and `a = 1 +\n2` are the two cases.

This is the same rule Go uses, and it is why Eta source has almost no
semicolons in it — look back at `demos/2.5-ast/`.

## What lexing does not catch

The `) } ; = =` line tokenises perfectly and is not a program. The lexer's job
ends at "these are the words"; whether they form a sentence is deck 3's parser.
