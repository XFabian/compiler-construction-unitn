# Demo 9.2 — Dataflow facts in rwetac

The lecture computed facts by hand, round by round. Here rwetac computes
them with the same solver its optimizations use, on the programs of the
slides, and writes them next to each WTAC instruction.

Run everything from the repository root.

## The programs

| File | Slides | Analysis |
|---|---|---|
| `01_gen_kill.eta` | What a Node Does to Definitions | reaching definitions, straight line |
| `02_running.eta` | the running example (nodes 1–8) | reaching definitions, then liveness |
| `03_straight_live.eta` | Liveness | liveness, straight line |
| `04_appel.eta` | Liveness in a Loop | liveness, loop |

## The files

The script writes all listings of the demo at once and prints where they
are:

```
bash demos/9.2-dataflow/facts.sh
```

| Open | For |
|---|---|
| `01_gen_kill_debug/01_gen_kill_f.reaching.facts` | gen and kill |
| `02_running_debug/02_running_fact.reaching.facts` | the running example, reaching definitions |
| `02_running_debug/02_running_fact.liveness.facts` | the running example, liveness |
| `03_straight_live_debug/03_straight_live_f.liveness.facts` | one name, two values |
| `04_appel_debug/04_appel_appel.liveness.facts` | liveness in a loop |

All in `demos/9.2-dataflow/`, named `<program>_<function>.<analysis>.facts`,
and ignored by git. By hand, one program and one analysis:

```
cargo run -q --bin rwetac -- --facts reaching --dump demos/9.2-dataflow/02_running.eta
```

Without `--dump`, the listing goes to the terminal instead.

## How to read a listing

`--facts` builds the CFG, runs one analysis on the program as written (no
optimization) and writes the function block by block: each WTAC instruction
with the facts that hold after it. The line under a block's header holds the
facts at its start. The order is program order, for liveness too.

- A definition is written `x.0@2003`: variable `x.0` (the resolver numbers
  names), defined by instruction 2003. That is the slides' x1, "x defined in
  node 1". Parameters and globals are defined at `@entry`.
- Compiler temporaries (`tmp.N`) are left out; `--facts-all` shows them too.

`01_gen_kill_f.reaching.facts`, the block with the code:

```
Block 2 func_exit   <- 1   -> 0
                                                           {}
  2000: tmp.0 : i32 = __stack_pointer                      {}
  2001: __stack_pointer : i32 = __stack_pointer - 0:i32    {}
  2002: tmp.1 : i32 = __stack_pointer                      {}
  2003: x.0 : i64 = 5:i64                                  {x.0@2003}
  2004: y.1 : i64 = x.0                                    {x.0@2003, y.1@2004}
  2005: tmp.2 : i64 = 7:i64                                {x.0@2003, y.1@2004}
  2006: x.0 : i64 = tmp.2                                  {x.0@2006, y.1@2004}
  2007: tmp.3 : i64 = x.0 + y.1                            {x.0@2006, y.1@2004}
  2008: z.2 : i64 = tmp.3                                  {x.0@2006, y.1@2004, z.2@2008}
```

## What to look at

**01_gen_kill.eta.** `x = 5; y = x; x = 7; z = x + y`.
Open `01_gen_kill_f.reaching.facts`.

**In class:** which definitions reach `z = x + y`? `y = x` and `x = 7`, but
not `x = 5`: `x = 7` killed it. The facts grow by one definition per line,
and `x = 7` swaps one x for the other (`x.0@2003` becomes `x.0@2006`).

**02_running.eta.** The running example: `x = 5; y = 1; z = x + 8`, then
`while (x > 1) { y = x * y; x = x - 1 }`, `x = -1`, `return y`.

Reaching definitions, `02_running_fact.reaching.facts`, **in class:**

- What reaches `z = x + 8`? Only `x = 5`: x is a constant there.
  `4005: tmp.2 : i64 = x.0 + 8:i64` comes right after
  `{x.0@4003, y.1@4004}`.
- What reaches `x > 1`? `x = 5` and `x = x - 1`, over the back edge: x is
  not a constant in the loop. The start line of `Block 2 while_loop.8` has
  both `x.0@4003` and `x.0@6008`.

Liveness, `02_running_fact.liveness.facts`, **in class:** which assignments
are dead? `z = x + 8` (nobody reads z: `z.2` is in no set) and `x = -1`
(after `1010: x.0 : i64 = tmp.11` only `{y.1}` is live).

Using the result:

```
cargo run -q --bin rwetac -- --passes cp --dump demos/9.2-dataflow/02_running.eta
cargo run -q --bin rwetac -- --passes dce --dump demos/9.2-dataflow/02_running.eta
```

Open `02_running_debug/02_running_opt.debug.wtac` after each. Constant
propagation turns `z = x + 8` into `tmp.2 = 5 + 8` and leaves the x inside
the loop alone. Dead code elimination removes `z.2 = tmp.2` and
`x.0 = tmp.11` (the `x = -1`); their temporaries go in the next round,
because only now nobody reads them.

**03_straight_live.eta.** `a = 1; b = a + 2; a = 7; return a + b`.
Open `03_straight_live_f.liveness.facts`.

**In class:** is a live between `b = a + 2` and `a = 7`? No: the next thing
that happens to a is a write. One name, two values, and only the second one
is read at the end: `{a.0}` after `a = 1`, `{b.1}` after `b = a + 2`,
`{a.0, b.1}` after `a = 7`.

**04_appel.eta.** Appel's loop, as a `while` (the slides have it as a
loop that tests at the end). Open `04_appel_appel.liveness.facts`.

**In class:** is a live at the end of the loop body? Yes, because the
condition `a < n` reads it on the next round: after
`6011: br while_loop.8` the set is `{a.2, c.0, n.1}`. That is only found once
the back edge has been followed: the solver visits the loop more than once.
c is live from the entry on (`Block 3 (entry)` starts with `{c.0, n.1}`),
because it is read before it is written; here it is a parameter. b is never
live at the loop header: each round writes it before reading it.
