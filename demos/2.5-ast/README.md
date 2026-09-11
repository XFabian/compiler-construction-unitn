# Demo 2.5 — Eta programs and their ASTs

Six small Eta programs, and the commands to see what the compiler makes of
them.

Run everything from the repository root.

## The Eta language in one page

Eta comes from [Cornell CS 4120](https://www.cs.cornell.edu/courses/cs4120/2026sp/project/language.pdf).
The full grammar is in the [top-level README](../../README.md#the-eta-grammar);
this is enough to read the programs here.

A **function** is a name, parameters with types, a colon, and a return type.
There is no keyword introducing it.

```
add(a : int, b : int) : int {
    return a + b
}
```

A function returning nothing is a **procedure** — no colon, no return type.

The types are `int` (64-bit), `bool`, arrays `int[]`, and records. A **variable
declaration** is `name : type = value`, and the initialiser is optional:
`a : int[3]` gives you an array of three zeros.

Four things that differ from what you are probably used to:

- **Functions can return several values.** `divmod(a, b) : int, int`, and the
  caller writes `q, r = divmod(17, 5)`.
- **The right-hand side is evaluated first.** So `a, b = b, a` swaps, rather
  than assigning `b` to both.
- **Arrays know their own length**, via `length(a)`. They live on the heap.
- **Semicolons are inserted by the lexer**, so source rarely contains them.


## The programs

| File | What to look at |
|---|---|
| `01_functions.eta` | function vs procedure; where the types sit |
| `02_precedence.eta` | `1 + 2 * 3` and `(1 + 2) * 3` — same tokens, different trees |
| `03_control_flow.eta` | `if` with and without `else`; `while` |
| `04_multiple_returns.eta` | returning two values, and the swap |
| `05_arrays.eta` | `length`, indexing, an uninitialised array |
| `06_records.eta` | a record type, construction, field access |

## Files in the Compiler

| File | What to point at |
|---|---|
| [`rwetac/src/ast.rs`](../../rwetac/src/ast.rs) | the node types `--parse` prints |
| [`rwetac/src/types.rs`](../../rwetac/src/types.rs) | the `Type` enum, for the `: int` in the dump |

`ast.rs` is the one to spend time on. Read it in this order:

- `ExpressionKind` and `StatementKind` — the two enums the whole compiler
  matches on. This is exactly the `Expr` from the calculator, grown up.
- `Expression` and `Statement` — each is its own *kind* plus a `Span`. The
  kinds are split out so that matching does not have to destructure the span
  every time.
- `LVal` — the left-hand side of an assignment is not a `String`, because
  `a[j] = 0` assigns to an expression.
- `Function` and `Record` — `Record` is the extension this compiler adds.

## Commands

Run from the repository root. `PROG` is any of the files above.

```
cargo run --bin rwetac -- --parse    demos/2.5-ast/PROG.eta   // the AST
cargo run --bin rwetac -- --validate demos/2.5-ast/PROG.eta   // silence means it typechecks
cargo run --bin rwetac -- --wtac     demos/2.5-ast/PROG.eta   // the IR, deck 6
cargo run --bin rwetac --            demos/2.5-ast/PROG.eta   // compile to PROG.wat
```

The `--parse` dump is one long line, most of it the `Expression { kind: ...,
span: N..M }` wrapper around every node. `trim_ast.py` removes that:

```
cargo run --bin rwetac -- --parse demos/2.5-ast/02_precedence.eta \
    | python3 demos/2.5-ast/trim_ast.py
```

which leaves the three declarations in that file readable:

```
1 + 2 * 3     ->  Binary(Add, Int(1), Binary(Mult, Int(2), Int(3)))
(1 + 2) * 3   ->  Binary(Mult, Binary(Add, Int(1), Int(2)), Int(3))
10 - 4 - 3    ->  Binary(Sub, Binary(Sub, Int(10), Int(4)), Int(3))
```

Add `--indent` to break it across lines instead. `RUST_LOG=info` on any of
these shows the phases running underneath.

## Running a program

Compile first, then hand the `.wat` to the runtime:

```
cargo run --bin rwetac  -- demos/2.5-ast/01_functions.eta
cargo run --bin runtime -- demos/2.5-ast/01_functions.wat
```

The runtime prints `Result : [I64(42)]`. Expected values:

| Program | Result |
|---|---|
| `01_functions` | 42 |
| `02_precedence` | 19 |
| `03_control_flow` | 0 |
| `04_multiple_returns` | 23 |
| `05_arrays` | 16 |
| `06_records` | 24 |


