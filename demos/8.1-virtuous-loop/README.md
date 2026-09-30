# Demo 8.1 — The virtuous loop in rwetac

The program from the Virtuous Loops slides, run through rwetac's optimizer one
round at a time. On the slides two rounds of constant folding, constant
propagation and dead code elimination reduce it to `return 13`. rwetac needs
five, and the demo is about why, and about how the compiler knows when to stop.

Run everything from the repository root.

## The commands

```
cargo run -q --bin rwetac -- --passes cf,cp,dce --count --dump demos/8.1-virtuous-loop/01_virtuous.eta
```

`--passes` runs exactly these passes, in this order, once (`--report-passes`
lists them). `--count` prints the WTAC instructions per function before and
after. `--dump` writes the optimized IR to
`demos/8.1-virtuous-loop/01_virtuous_debug/01_virtuous_opt.debug.wtac`; keep it
open next to the source, the editor reloads it after every run.

Then add one round at a time:

```
cargo run -q --bin rwetac -- --passes cf,cp,dce,cf,cp,dce --count --dump demos/8.1-virtuous-loop/01_virtuous.eta
cargo run -q --bin rwetac -- --passes cf,cp,dce,cf,cp,dce,cf,cp,dce --count --dump demos/8.1-virtuous-loop/01_virtuous.eta
...
```

and finally let the compiler decide:

```
cargo run -q --bin rwetac -- --passes 'fixpoint(cf,cp,dce)' --count --dump demos/8.1-virtuous-loop/01_virtuous.eta
cargo run -q --bin rwetac -- --opt --count demos/8.1-virtuous-loop/01_virtuous.eta
```

You can play around with these values and look at the output for the different options.

## The output, one round at a time

Instructions in `f` (the `f` line of `--count`), and the interesting part of
the dump:

| Rounds of `cf,cp,dce` | `f` | What is left |
|---|---|---|
| 0 | 15 | the lowering |
| 1 | 10 | `b.1 = 6`, `tmp.3 = b.1 + 7`, `d.3 = tmp.3`, `__ret_val = d.3` |
| 2 | 8 | `tmp.3 = 6 + 7`, `d.3 = tmp.3`, `__ret_val = d.3` |
| 3 | 7 | `d.3 = 13`, `__ret_val = d.3` |
| 4 | 6 | `__ret_val = 13` |
| 5 | 5 | `return 13` |
| 6 | 5 | nothing changed: the fixpoint |

`fixpoint(cf,cp,dce)` and `--opt` both end at 5, `return 13`.

**Why five rounds and not two?** Lowering adds copies the slides do not have:
`b.1 = tmp.2`, `d.3 = tmp.3`, `__ret_val = d.3`. Constant propagation looks at
the definitions as they were when the pass started, so a constant moves one
copy further per round. Round 1 turns `b.1 = tmp.2` into `b.1 = 6`, but
`tmp.3 = b.1 + a.0` only gets the `7` from `a.0`, which was already a constant.

**The order matters for the number of rounds, not the result.** Every order
ends in `return 13`:

| Order | Rounds until `return 13` |
|---|---|
| `cf,cp,dce` | 5 |
| `cp,cf,dce` | 6 |
| `dce,cp,cf` | 7 |

This is the lead-in to the order quiz.

## Ignore for now

| Line | What it is |
|---|---|
| `tmp.0 = __stack_pointer`, `__stack_pointer = tmp.0` | the empty stack frame (TAC lecture); a global, so the optimizer leaves it |
| `length(...)` | the built-in `length`, always emitted; unchanged by every pass |
| the count of 15 vs 14 | an empty pipeline already drops the duplicated `br func_exit` when it rebuilds the WTAC from the CFG |

## The programs

| File | Used for |
|---|---|
| `01_virtuous.eta` | the whole demo |

Delete `01_virtuous_debug/` and `01_virtuous.wat` afterwards; both are ignored
by git.
