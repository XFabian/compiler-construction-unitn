# Demo 5.1 — Scope resolution with the StackMap

Six small Eta programs, each one from a slide of deck 5, and one script. The
resolver logs every StackMap operation, so what you see is the compiler doing
exactly what the `Example` slides do by hand.

Run everything from the repository root.

## The one command

```
RUST_LOG=rwetac::resolver=trace cargo run -q --bin rwetac \
    -- --validate demos/5.1-scopes/01_slides_example.eta 2>&1 \
  | python3 demos/5.1-scopes/trim_scopes.py --body
```

```
push              G::[]
lookup_curr(x)    -> free
add               G::[x ↦ x.0]
push              G::[x ↦ x.0]::[]
lookup(x)         -> x.0
lookup_curr(y)    -> free
add               G::[x ↦ x.0]::[y ↦ y.1]
lookup(glob)      -> glob
lookup_curr(x)    -> free
add               G::[x ↦ x.0]::[y ↦ y.1, x ↦ x.2]
push              G::[x ↦ x.0]::[y ↦ y.1, x ↦ x.2]::[]
...
pop               G::[x ↦ x.0]::[y ↦ y.1, x ↦ x.2]
lookup(y)         -> y.1
...
pop               G::[x ↦ x.0]
pop               G
```

Every line is one of the four operations from `StackMap: Four Operations`,
followed by the whole stack in the slides' notation, innermost scope last.
`lookup` answers with the unique name it found (or `undefined`),
`lookup_curr` with the name already declared in the current scope (or `free`).

- `G` is the bottom map: globals and functions, collected in a first pass
  before any body is resolved (`Missing Details`). `--globals` writes it out;
  it also holds `length`, the one built-in function. `--body` hides that first
  pass and starts at the first function.
- The unique names are the ones on `Implementation: the StackMap`: `x.0` for
  the parameter, `y.1`, `x.2` in the body, `y.3`, `z.4` in the block.
- After the block's `pop`, `lookup(y)` answers `y.1` where a moment ago it
  answered `y.3`. That one line is why the stack exists.

## The programs

| File | Slide | What to look at |
|---|---|---|
| `01_slides_example.eta` | `Example` (the long run) | the trace is the slides, operation for operation |
| `02_quiz.eta` | Quiz: which `a`? | `c + a` twice: `lookup(a) -> a.3` in the block, `-> a.0` after it |
| `03_quiz_fact.eta` | Quiz: is it valid? | `Undefined Variable result`; `x = x - 1` is fine |
| `04_by_hand.eta` | `Scope Resolution` | four names do not resolve; the compiler stops at the first |
| `05_param_shadow.eta` | `Scoping Rules` (parameters) | `lookup_curr(x) -> free`: the body's map is a new scope |
| `06_duplicate.eta` | `Scoping Rules Eta` | `lookup_curr(x) -> x.0`: declared twice in one scope |



## Renaming happens in the same pass

The arrow in `lookup(x) -> x.0` is not only an answer: the resolver writes
`x.0` into the AST in place of `x`. When resolution is done, every variable
has a unique name, which is what the `Renaming` slide shows. The type checker
never sees a scope again (demo 5.2).

## Two edits to make live

Both are one line in [`rwetac/src/resolver.rs`](../../rwetac/src/resolver.rs).
Undo them afterwards.

**Forbid shadowing** — the question on the `Example` slide. In
`add_local_var`, a declaration only checks the current scope:

```rust
if self.scope_map.find_curr(name).is_some() {    // change find_curr to find
```

With `find`, `01` fails at `x : int = glob` (`Duplicate Variable x`), and so
does `05`. Three tests fail too: `shadowing.eta` in the runtime tests,
`valid_cases`, and `block_scope_ends_at_closing_brace`. They all shadow on
purpose.



## Code tour

Everything is in [`rwetac/src/resolver.rs`](../../rwetac/src/resolver.rs).
Line numbers are as of this writing; the names are the stable reference.

| On the slides | In `resolver.rs` | Line |
|---|---|---|
| the StackMap: a stack of maps | `struct StackMap { stack: Vec<HashMap<String, IdEntry>> }` | 50 |
| `push()` | `StackMap::push` | 69 |
| `pop()` | `StackMap::pop` | 79 |
| `lookup(x)` | `StackMap::find`: searches from the top down | 100 |
| `lookup_curr(x)` | `StackMap::find_curr`: the top map only | 111 |
| the slides' notation `[glob]::[x]::…` | `impl Display for StackMap` | 122 |
| `Missing Details`: functions first | `resolve` calls `resolve_only_top_level` on every declaration, then the bodies | 185, 202 |
| parameters live in their own scope | `resolve_function`: push for the parameters, push for the body, pop twice | 264 |
| a declaration: `lookup_curr`, then add | `add_local_var`: duplicate check, fresh name `x.n` | 293 |
| a block `{ … }` | the `Compound` arm of `resolve_stmt`: push, resolve, pop | 362 |
| a use: `lookup` | the `Var` arm of `resolve_expr`, which also writes the unique name into the AST | 410 |

Walk it in that order. The `Var` arm is where renaming happens:
`*x = entry.unique_name.clone()` overwrites the name in the AST.

