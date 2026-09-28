# Demo 6.1 — What an IR looks like

One Eta program: the three examples of `AST to IR : Ideas`, in one function.
The point is only to show that rwetac has an IR, how to get it printed, and
that it answers the three questions on that slide. What the instructions mean
is the next lecture (TAC).

Run everything from the repository root.

## The commands

```
cargo run -q --bin rwetac -- --wtac demos/6.1-ir/01_ideas.eta          # print the IR
cargo run -q --bin rwetac -- --wtac --dump demos/6.1-ir/01_ideas.eta   # and write it to a file
```

`--wtac` stops after lowering to WTAC, rwetac's IR, and prints it. `--dump`
also writes it to `demos/6.1-ir/01_ideas_debug/01_ideas.debug.wtac`, to open
next to the source. (Without `--wtac`, `--opt --dump` also writes the IR after
optimization and the control-flow graphs there; that is for the optimization
lectures.)

## The output, one question at a time

Put the source and the dump side by side. The body of `f` is three blocks, one
per example on the slide.

**Handling of arbitrary nested expressions** — `c : int = 2 + 3 + a + b`

```
tmp.2 : i64 = 2:i64 + 3:i64
tmp.3 : i64 = tmp.2 + a.2
tmp.4 : i64 = tmp.3 + b.3
c.4 : i64 = tmp.4
```

The tree is gone. Every `+` is its own line with two operands, and every
intermediate result has a name (`tmp.2`, `tmp.3`, …). Evaluation order is now
the line order.

**In which order do we evaluate and assign?** — `a, b = 2 + 3, c`

```
tmp.5 : i64 = 2:i64 + 3:i64
tmp.6 : i64 = tmp.5
tmp.7 : i64 = c.4
a.2 : i64 = tmp.6
b.3 : i64 = tmp.7
```

First every right-hand side, then every assignment. Nothing on the right can
see a variable that the left already changed.

**Address calculation and memory explicit** — `x : int = A[i]`

```
tmp.8 : i64 = i.1 * 8:i64
tmp.9: i32 = (i32) tmp.8
tmp.10 : i32 = A.0 + tmp.9
x.5 : i64 = [tmp.10 + 0]%i64
```

`A[i]` is gone too: the offset `i * 8` (an `int` is 8 bytes), the start of the
array plus the offset, and then a load from that address, `[…]`.

Things to notice across all three:

- **Types are machine types.** `int` is `i64`; the array `A` is an address,
  `i32`.
- **It is longer, and a bit silly.** `tmp.6 = tmp.5` and then `a.2 = tmp.6` is a
  copy nobody needs. The translation is kept simple on purpose; removing the
  copies is the optimizer's job.

## Ignore for now

Students will ask about these lines. Each belongs to a later lecture:

| Line | What it is |
|---|---|
| `x.5`, `a.2`, `A.0` | the unique names from scope resolution (demo 5.1) |
| `__stack_pointer`, `tmp.0`, `tmp.1` | the function's stack frame, empty here (TAC lecture, records) |
| `block func_exit`, `br func_exit`, `__ret_val` | every function has one exit (TAC lecture, control flow) |
| `length(...)` | the built-in `length`, always emitted |

## The programs

| File | Used for |
|---|---|
| `01_ideas.eta` | the whole demo |

Delete `01_ideas_debug/` afterwards; it is ignored by git either way.
