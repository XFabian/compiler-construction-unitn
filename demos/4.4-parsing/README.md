# Demo 3.4 — Parsing with the compiler's own parser

Five small Eta programs and one script. Nothing here reimplements parsing:
everything is the `rwetac` binary you already have, so what it prints is what
the compiler does. This is the demo deck 3 slide 43 asks for.

Run everything from the repository root.

## The one command worth memorising

`parse_expression` is the only instrumented function in the parser, so every
`tracing` span in the log is one call of it, and the span nesting *is* the
recursion. `trim_trace.py` turns that nesting back into indentation:

```
RUST_LOG=rwetac::parser=debug cargo run -q --bin rwetac \
    -- --parse demos/3.4-parsing/01_precedence.eta 2>&1 \
  | python3 demos/3.4-parsing/trim_trace.py
```

```
parse_expression(min_prec=0)   tok=Int(1)
  parse_expression(min_prec=6)   tok=Int(2)
    parse_expression(min_prec=7)   tok=Int(3)
parse_expression(min_prec=0)   tok=LParen
  parse_expression(min_prec=0)   tok=Int(1)
    parse_expression(min_prec=6)   tok=Int(2)
  parse_expression(min_prec=7)   tok=Int(3)
```

Three things to point at, in this order:

- **`1 + 2 * 3`** climbs `0 → 6 → 7`. The floor rises as the operators bind
  tighter, and the deepest call is the one that ends up deepest in the tree.
- **`(1 + 2) * 3`** resets the floor to `0` *inside* the parentheses. That reset
  is the only thing parentheses do, and here you can watch it happen.
- **`1 + 2 < 4 & true`** goes `6 → 5 → 3` as **siblings**, not nested. Loosening
  operators do not recurse; the loop picks them up.

The trace is on stderr and the AST on stdout, so `2>&1` is needed. The script
passes the AST through with spans stripped — add `--indent` to spread it out.

## The programs

| File | What to look at |
|---|---|
| `01_precedence.eta` | the climb, the parenthesis reset, siblings vs nesting |
| `02_associativity.eta` | `prec + 1` is the whole of left-associativity |
| `03_dangling_else.eta` | which `if` the `else` binds to, and who decided |
| `04_syntax_error.eta` | lexes perfectly, parses not at all |
| `05_not_a_syntax_error.eta` | parses perfectly, means nothing — slide 47 |

### `02_associativity` — where associativity actually lives

`10 - 4 - 3` traces as two calls at the *same* depth:

```
parse_expression(min_prec=0)   tok=Int(10)
  parse_expression(min_prec=6)   tok=Int(4)
  parse_expression(min_prec=6)   tok=Int(3)
```

The recursive call is made with `prec + 1`, so the second `-` fails the
`prec < min_prec` test, returns, and is picked up by the loop one level up —
which hangs the new node *above* the old one. The tree leans left:

```
Binary(Sub, Binary(Sub, Int(10), Int(4)), Int(3))
```

Change `prec + 1` to `prec` in `parser.rs` and the same trace nests instead,
giving `10 - (4 - 3)`.

### `03_dangling_else` — a grammar that does not decide

```
if x > 0
    if y > 0
        return 1
    else
        return 2
```

Nothing in the grammar says which `if` owns the `else`. `parse_if` decides:
after parsing the then-branch it checks `peek() == Else` immediately, so the
innermost call still on the stack claims it first. Nearest `if` wins.

```
cargo run -q --bin rwetac -- --parse demos/3.4-parsing/03_dangling_else.eta \
  | python3 demos/2.5-ast/trim_ast.py --indent
```

The inner `If` has `else_br: Some(...)`, the outer has `else_br: None`.

This is the canonical shift/reduce conflict. A generator would report it and
resolve it by preferring shift — which is the same answer, reached by a
different route. Recursive descent does not report anything, because the code
has no way to express the ambiguity in the first place.
**the hand-written parser is not unambiguous, it is silently opinionated.**

### `04` and `05` — the two neighbours of a parse error

They bracket what parsing is responsible for.

```
cargo run -q --bin rwetac -- --parse demos/3.4-parsing/04_syntax_error.eta
Error: Parsing failed at 7:24. Unexpected Token RParen! Message: Primary Expression not possible!
```

Every token in `04` is legal Eta — run it through `cargo run -p lexdemo --
--file demos/3.4-parsing/04_syntax_error.eta` and the lexer is perfectly happy.
`7:24` comes from the span the lexer attached, which is the payoff for carrying
spans at all.

`05` is the opposite and is deck 3 slide 47 verbatim. `x : int = a + true`
parses — `+` takes two expressions and `true` is one — so `--parse` prints a
tree. `--validate` is what rejects it:

```
Error: Typechecking failed: Type Error at 9:15.  Binary Operation with invalid Type! e1 : int e2 : bool
```

That is the handoff to deck 4.

## Files in the compiler

| File | What to point at |
|---|---|
| [`rwetac/src/parser.rs`](../../rwetac/src/parser.rs) | `parse_expression`, then `binop_precedence`, then `parse_if` |
| [`README.md`](../../README.md#the-eta-grammar) | the grammar, in both forms |

Read `parse_expression` and `binop_precedence` together — they are twenty lines
and they are the entire answer to "how does precedence get into the tree".

The grammar section of the top-level README gives the expression grammar
**twice**: once stratified (`OrExpr`, `AndExpr`, `EqExpr`, …, one nonterminal
per precedence level) and once as the precedence table the parser actually
climbs. Those are the two ways to remove ambiguity, and this compiler takes the
second. Show both on the same slide.

Note what the parser is *not*: only expressions use precedence climbing.
Declarations, types and statements are ordinary recursive descent, one function
per production — `parse_function`, `parse_block`, `parse_statement`,
`parse_while`. Those functions are not instrumented, so they do not appear in
the trace.

## Running them

```
cargo run --bin rwetac  -- demos/3.4-parsing/PROG.eta   // compile to PROG.wat
cargo run --bin runtime -- demos/3.4-parsing/PROG.wat   // run it
```

| Program | Result |
|---|---|
| `01_precedence` | 16 |
| `02_associativity` | 18 |
| `03_dangling_else` | 2 |
| `04_syntax_error` | parse error, on purpose |
| `05_not_a_syntax_error` | type error, on purpose |
