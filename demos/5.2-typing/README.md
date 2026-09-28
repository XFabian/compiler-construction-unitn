# Demo 5.2 — From typing rules to the type checker

Five small Eta programs, one per rule, and one script. The deck shows the rules
(`Eta Rules: Expressions`, `Eta Rules: Statements`) and one of them as code
(`From Rule to Code`). This demo walks through the rest the same way: open the
program, show the rule, open the matching code, run it. This is the demo the
`Demo` slide after `Γ in rwetac` asks for.

Run everything from the repository root. All the code is in
[`rwetac/src/typecheck.rs`](../../rwetac/src/typecheck.rs); line numbers are
as of this writing, function names are the stable reference.

## The one command

```
RUST_LOG=rwetac::typecheck=debug cargo run -q --bin rwetac \
    -- --validate demos/5.2-typing/02_gamma.eta 2>&1 \
  | python3 demos/5.2-typing/show_rules.py
```

```
Γ before the bodies
    length ↦ fn unknown[] -> int
    positive ↦ fn int -> bool

positive: returns bool, so its body must be void
    (VARINIT)  y.1 : bool
    (SEQ)      a block
        (VARINIT)  x.2 : bool
        (ASSIGN)   y.1 = ...
    (RETURN)   checked against ρ = ret_t
Γ after everything
    length ↦ fn unknown[] -> int
    positive ↦ fn int -> bool
    x.0 ↦ var int
    y.1 ↦ var bool
    x.2 ↦ var bool
```

The indented lines are the statement rules the checker applied, nested the way
the derivation is. `Γ` is printed from the symbol table itself, before the
function bodies are checked and after.

## Code tour: where the rules live

| In the rules | In `typecheck.rs` | Line |
|---|---|---|
| Γ | `Typechecker { symtab }`: one flat `SymbolTable` | 80 |
| `Γ ⊢ e : t` | `typecheck_expression` returns a `Type`, a `match` on the AST | 725 |
| `Γ ⊢ s : R ⊣ Γ′` | `typecheck_statement` returns a `StatementType` | 412 |
| R = unit / void, lub | `enum StatementType`, `StatementType::lub` | 38, 50 |
| Γ(ρ) = ret T | the `ret_t` argument of `typecheck_statement`, passed down | 412 |
| functions first (`Missing Details`) | `typecheck`: `add_top_level` for every declaration, then the bodies | 109 |

The overall shape to show first, in `typecheck` (109) and `typecheck_fun` (264):

1. Every function signature goes into Γ before any body is checked, so calls
   can go forward and functions can be mutually recursive.
2. For each function, the parameters are added to Γ (`add_local_var`, 305),
   and the body is checked with ρ = the declared return type.
3. A function with a return type needs a `void` body (315). That single `if`
   is "every path returns".

## Rule by rule

### (VAR) and Γ is flat — `02_gamma.eta`

```
Γ(x) = var t
────────────
 Γ ⊢ x : t
```

```rust
// typecheck_expression, 727
ExpressionKind::Var(name) => self.symtab.get(name).t.clone(),
```

One line, and no scopes: `name` is already `x.0` or `x.2`, because the resolver
renamed it (demo 5.1). That is the answer to "is Γ a stack map too?". It is
not. The StackMap is gone after resolution; all Γ gets from it is the unique
names. Two `x`s of different types sit side by side in the final Γ above.

`symtab.get` panics if the name is missing. After resolution that cannot
happen, so a panic there is a compiler bug, not a user error (demo 5.3).

### (ARITH) — `01_two_plus_true.eta`

```
Γ ⊢ e₁ : int    Γ ⊢ e₂ : int    ⊕ ∈ {+, -, *, /, %}
───────────────────────────────────────────────────
                 Γ ⊢ e₁ ⊕ e₂ : int
```

```rust
// typecheck_binary, 928, condensed
let e1_ty = self.typecheck_expression(e1)?;          // premise Γ ⊢ e₁ : t₁
let e2_ty = self.typecheck_expression(e2)?;          // premise Γ ⊢ e₂ : t₂
let res_ty = match op {
    BinOp::Add | BinOp::Sub | BinOp::Mult | BinOp::Div | BinOp::Mod
        if e1_ty == Type::Int && e2_ty == Type::Int => e1_ty,   // conclusion
    ...
    _ => return Err(TypeError::Generic { .. }),      // no rule applies
};
```

This is `From Rule to Code` on the real file. Run it:

```
Error: Typechecking failed: Type Error at 2:15.  Binary Operation with invalid Type! e1 : int e2 : bool
```

The message on the `Example` slide, and `2:15` is where `2` starts.

### (VARINIT) — `02_gamma.eta`

```
x ∉ dom(Γ)    Γ ⊢ e : t
────────────────────────────────
Γ ⊢ x:t = e : unit ⊣ Γ[x ↦ var t]
```

```rust
// typecheck_statement, LocalDecl arm, 419
StatementKind::LocalDecl(vd) => {
    self.symtab.add_local_var(vd.name.clone(), vd.var_type.clone());   // Γ[x ↦ var t]
    if vd.dims.is_empty() {
        self.typecheck_scalar_decl(vd)                                 // Γ ⊢ e : t
    } ...
```

Two things to point at:

- **`x ∉ dom(Γ)` is not checked here.** It cannot fail: after renaming every
  declaration has a fresh name. The duplicate check happened in the resolver,
  per scope, which is why rwetac allows shadowing and Cornell's Eta does not.
- **Γ′ is never undone.** The rule for a block hands the *outer* Γ back; the
  code just keeps adding. With unique names that is harmless, and code
  generation later needs the type of every variable anyway.

### (IFELSE) and lub — `03_falls_through.eta`

```
Γ ⊢ e : bool    Γ ⊢ s₁ : R₁ ⊣ Γ′    Γ ⊢ s₂ : R₂ ⊣ Γ″
─────────────────────────────────────────────────────
   Γ ⊢ if (e) s₁ else s₂ : lub(R₁, R₂) ⊣ Γ
```

```rust
// typecheck_statement, If arm, 432
let then_t = self.typecheck_statement(then_br, ret_t)?;
if let Some(s) = else_br.as_ref() {
    let else_t = self.typecheck_statement(s, ret_t)?;
    Ok(StatementType::lub(&then_t, &else_t))
} else {
    Ok(StatementType::Unit)          // (IF): the branch can be skipped
}
```

and `lub` itself (50): `Unit` wins, `Void` only if both are `Void`. The
then-branch returns, the else-branch does not, so the `if` is `unit` and the
check in `typecheck_fun` fires:

```
Error: Typechecking failed: Type Error at 4:1.  Function block does not have void as its type!
```

Live: add `return x` after `x = 0 - x`. Both branches are `void`, and it
type-checks.

### (SEQ) — `04_after_return.eta`

```
Γ ⊢ s₁ : unit ⊣ Γ₁  …  Γₙ₋₁ ⊢ sₙ : R ⊣ Γₙ
─────────────────────────────────────────
     Γ ⊢ { s₁ … sₙ } : R ⊣ Γ
```

```rust
// typecheck_block, 387, condensed
let (last_stmt, preceding_stmts) = block.stmts.split_last().unwrap();
for s in preceding_stmts {
    if self.typecheck_statement(s, ret_t)? != StatementType::Unit {
        return Err(..);                            // only the last may be void
    }
}
self.typecheck_statement(last_stmt, ret_t)         // the block's R
```

A nested block `{ ... }` goes through the same function (the `Compound` arm of
`typecheck_statement` calls `typecheck_block`), so the rule lives in one place.
Run it:

```
Error: Typechecking failed: Type Error at 4:5.  This statement always returns, so the statements after it are unreachable
```

`4:5` is the `return`, the statement that was `void` but not last.

### (RETURN) and ρ — `05_wrong_return.eta`

```
Γ(ρ) = ret (t₁, …, tₙ)    Γ ⊢ eᵢ : tᵢ
─────────────────────────────────────
  Γ ⊢ return e₁, …, eₙ : void ⊣ Γ
```

```rust
// typecheck_return, 486, condensed -- ret_t is ρ, passed down
let es_ty = es.iter().map(|e| self.typecheck_expression(e)).collect()?;
...
else if elems1 != *ret_elems { return Err(..) }    // tᵢ must match
else { Ok(StatementType::Void) }                    // conclusion: void
```

The spec keeps ρ inside Γ. rwetac passes it as an argument through every
`typecheck_statement` call instead: the same information, and it can never be
confused with a variable.

## The programs

| File | Rule | Result |
|---|---|---|
| `01_two_plus_true.eta` | (ARITH) | type error at 2:15 |
| `02_gamma.eta` | (VAR), (VARINIT), (SEQ), (RETURN) | type-checks; shows Γ |
| `03_falls_through.eta` | (IFELSE), lub | function body is not void |
| `04_after_return.eta` | (SEQ) | statement after a return |
| `05_wrong_return.eta` | (RETURN) | returns bool, ρ says int |

Each file also runs without the script, for the plain error:

```
cargo run -q --bin rwetac -- --validate demos/5.2-typing/03_falls_through.eta
```
