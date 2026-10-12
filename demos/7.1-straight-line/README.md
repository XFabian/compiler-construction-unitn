# Demo 7.1 — Straight-line code in WTAC

The straight-line examples of the TAC lecture, in one function: a nested
expression, a unary operator, an array access, the multi-assign quiz and a
swap. Before running it, let the students write down the TAC they expect.

Run everything from the repository root.

## The commands

```
cargo run -q --bin rwetac -- --wtac demos/7.1-straight-line/01_straight.eta
cargo run -q --bin rwetac -- --wtac --dump demos/7.1-straight-line/01_straight.eta
```

`--wtac` stops after lowering and prints the WTAC. `--dump` also writes it to
`demos/7.1-straight-line/01_straight_debug/01_straight.debug.wtac`, to open
next to the source.

Two things are in every function and are explained later (demo 7.3):
the three `__stack_pointer` lines at the start and the restore at the end,
and the function `length`, which rwetac adds to every program. Ignore them
for now.

## The output, line by line

**Nested expressions** — `z : int = a + b * 2`

```
tmp.2 : i64 = b.1 * 2:i64
tmp.3 : i64 = a.0 + tmp.2
z.4 : i64 = tmp.3
```

One operator per line. `b * 2` comes first because it binds tighter. Every
variable got a `.N` suffix: names are unique per function now, so scopes are
gone.

**Unary** — `c : bool = !(a < -b)`

```
tmp.4 : i64 = - b.1
tmp.5 : i32 = a.0 < tmp.4
tmp.6 : i32 = ! tmp.5
c.5 : i32 = tmp.6
```

A unary operator has one operand, so it is a two-address instruction. Note
the types: `bool` became `i32`, `int` is `i64`.

**Array access** — `x : int = A[i]`

```
tmp.7 : i64 = i.3 * 8:i64
tmp.8 : i32 = (i32) tmp.7
tmp.9 : i32 = A.2 + tmp.8
x.6 : i64 = [tmp.9 + 0]%i64
```

The address is computed by hand: index times 8 bytes, cast to `i32` (an
address in WebAssembly is 32 bits), plus the base.

`[tmp.9 + 0]%i64` is a memory operand: *address + constant offset*, and
after `%` how many bytes to read. `%i64` loads 8 bytes, `%i32` loads 4 (a
`bool`, or a pointer). On the right of `=` it is a load, on the left a store.
It is the only kind of operand that touches memory.

**The multi-assign quiz** — `x, x = x + 1, x * 4`

```
tmp.10 : i64 = x.6 + 1:i64
tmp.11 : i64 = x.6 * 4:i64
tmp.12 : i64 = tmp.10
tmp.13 : i64 = tmp.11
x.6 : i64 = tmp.12
x.6 : i64 = tmp.13
```

All right-hand sides first, then the assignments from left to right. The last
one wins: `x` ends up as `x * 4`, as in Rust.

**Swap** — `a, b = b, a`

```
tmp.14 : i64 = b.1
tmp.15 : i64 = a.0
a.0 : i64 = tmp.14
b.1 : i64 = tmp.15
```

Because the right-hand sides are copied into temporaries first, the swap
works. Assigning `a = b; b = a` directly would not.

## Running it

`f` takes an array, so it needs a caller. Add

```
main() : int {
    A : int[] = {10, 20}
    return f(1, 2, A, 1)
}
```

and run `cargo run -q --bin runtime -- demos/7.1-straight-line/01_straight.wat`
after compiling without `--wtac`. It prints `Result : [I64(85)]`: `z = 5`,
`x = 20`, then `x = 20 * 4 = 80`.
