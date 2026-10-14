# Demo 11.1 — Lowering WTAC to WASM

The lowering rules of the lecture, applied by rwetac. Three programs:

1. `01_straight` — demo 7.1's straight-line program. You know its WTAC
   already, so only the step to WASM is new.
2. `02_control` — demo 7.2's `if`, `while` and `return`.
3. `03_module` — a new program with a global, to see a whole module.

At the end, the WASM is run.

Run everything from the repository root. **In class** marks the questions
to ask before showing the next output.

## The commands

```
cargo run -q --bin rwetac -- --wtac demos/11.1-lowering/01_straight.eta   # the WTAC
cargo run -q --bin rwetac -- demos/11.1-lowering/01_straight.eta          # the WASM
```

Without a stage flag rwetac compiles all the way and writes
`demos/11.1-lowering/01_straight.wat`, next to the source. A successful
compile prints nothing. Open the `.wat` next to the WTAC: every WTAC line
becomes a few WASM lines, in the same order.

Every function starts with three `__stack_pointer` lines and ends by
restoring it. They are explained in demo 11.2. Ignore them for now, and the
function `length` at the end too.

## 01_straight — one rule per line

**In class:** before compiling, take the WTAC from demo 7.1 and let the
students write the WASM for the first three lines with the rules of the
slides.

**Variables become locals.** At the top of `$f`:

```
(func $f (param $a.0 i64) (param $b.1 i64) (param $A.2 i32) (param $i.3 i64) (result i64)
    (local $tmp.0 i32)
    ...
    (local $z.4 i64)
    (local $tmp.2 i64)
```

Every WTAC variable, temporaries included, becomes one `local` with the
type WTAC already gave it: `int` is `i64`, `bool` and arrays are `i32`. The
parameters are locals too. rwetac keeps the WTAC names, so you can find each
line again.

**Binary operations** — push, push, op, store:

```
tmp.2 : i64 = b.1 * 2:i64        local.get $b.1
                                 i64.const 2
                                 i64.mul
                                 local.set $tmp.2
```

A variable is pushed with `local.get`, a constant with `i64.const`. The type
of the instruction picks `i64.mul` or `i32.mul`. `<` becomes `i64.lt_s`: the
`_s` means signed, because an Eta `int` can be negative.

**Copy** — push, store:

```
z.4 : i64 = tmp.3                local.get $tmp.3
                                 local.set $z.4
```

**Unary:**

```
tmp.4 : i64 = - b.1              i64.const 0
                                 local.get $b.1
                                 i64.sub
                                 local.set $tmp.4

tmp.6 : i32 = ! tmp.5            local.get $tmp.5
                                 i32.eqz
                                 local.set $tmp.6
```

WASM has no negation for integers, so `-b` is `0 - b`. The 0 is pushed
first because `i64.sub` computes *second from the top minus top*. `!` is
"equal to zero", and the result is an `i32` like every `bool`.

**Cast:**

```
tmp.8 : i32 = (i32) tmp.7        local.get $tmp.7
                                 i32.wrap_i64
                                 local.set $tmp.8
```

The index is an `i64`, an address an `i32`: `i32.wrap_i64` keeps the low
32 bits.

**Load** — `x : int = A[i]`, last line:

```
x.6 : i64 = [tmp.9 + 0]%i64      local.get $tmp.9
                                 i32.const 0
                                 i32.add
                                 i64.load
                                 local.set $x.6
```

Push the address, load, store the value. `%i64` becomes `i64.load` (8 bytes).
rwetac adds the offset with `i32.add`, even when it is 0. WASM could do it
for free: `i64.load offset=0`.



**Call and return** — in `main` and at the end of `$f`:

```
tmp.21 : i64 = f(1:i64, 2:i64, A.7, 1:i64)
                                 i64.const 1
                                 i64.const 2
                                 local.get $A.7
                                 i64.const 1
                                 call $f
                                 local.set $tmp.21
```

Arguments are pushed in order and `call` takes them from the stack, so the
first argument is the deepest one. The result is left on the stack and
stored like any other result.

```
    (block $func_exit
      ...
      local.get $tmp.16
      local.set $__ret_val
      br $func_exit
      br $func_exit
    )
    local.get $tmp.0
    global.set $__stack_pointer
    local.get $__ret_val
    return
```

WTAC's single exit carries over: `return e` stores into `__ret_val` and
leaves `func_exit`, and the only real `return` is after the block. The
second `br $func_exit` can never run (demo 7.2 explains where it comes
from). `--opt` removes it.

## 02_control — blocks and branches

```
cargo run -q --bin rwetac -- --wtac demos/11.1-lowering/02_control.eta
cargo run -q --bin rwetac -- demos/11.1-lowering/02_control.eta
```

This is the Control Flow slide: WTAC's `block`, `loop`, `br` and `br_if`
are WASM's, so they are copied over almost word for word. `choose`:

```
block if_end.5: {                    (block $if_end.5
    block if_else.5: {                 (block $if_else.5
        tmp.2 : i32 = a.0 < b.1          local.get $a.0
                                         local.get $b.1
                                         i64.lt_s
                                         local.set $tmp.2
        br_if tmp.2 if_else.5            local.get $tmp.2
                                         br_if $if_else.5
        ...                              ...
        br if_end.5                      br $if_end.5
    }                                  )
    ...                                ...
}                                    )
```

`br_if` is the only branch that needs a value: the condition is pushed
first. The labels stay, with a `$` in front. In `count` the
`loop $while_loop.11` works the same way: `br $while_loop.11` jumps back to
its start.

**In class:** the lecture spent a whole slide on the WTAC of `if` and
`while`, but here there is nothing new to learn. Why? (WTAC was designed
with WASM's structured control flow in mind. A target with only jumps, like
x86, would need labels and `jmp` instead.)

## 03_module — the whole module

**In class:** show the source and let the students predict where `calls`
ends up, and what reading and writing it looks like.

```
calls : int = 0
add(a : int, b : int) : int { calls = calls + 1  return a + b }
main() : int { x : int = add(1, 2)  return add(x, calls) }
```

```
cargo run -q --bin rwetac -- demos/11.1-lowering/03_module.eta
```

The module, top to bottom:

```
(module
(import "env" "eta_malloc" (func $eta_malloc (param i32) (result i32)))
(memory (export "memory") 17)
(global $calls (mut i64) (i64.const 0))
(func $add ...)
(func $main ...)
(func $length ...)
(global $__stack_pointer (mut i32) (i32.const 66636) )
(global $__data_end i32 (i32.const 100) )
(global $__heap_base (export "__heap_base") (mut i32) (i32.const 66636) )
(export "add" (func $add))
(export "main" (func $main))
(export "length" (func $length))
)
```

- **The import** — every module imports `eta_malloc`, even one that never
  allocates. Demo 11.2 shows what it does.
- **The memory** — 17 pages of 64 KiB, exported so the runtime can read it.
- **The globals** — `calls` became a `global`, `mut` because it is assigned.
  Inside `add`, reading it is `global.get $calls` and writing it
  `global.set $calls`, where a local would use `local.get` and `local.set`.
   The three `__` globals are rwetac's own (demo 11.2).
- **The exports** — every function is exported, so the runtime can call
  `main`.

## Running it

```
cargo run -q --bin runtime -- demos/11.1-lowering/01_straight.wat    # Result : [I64(85)]
cargo run -q --bin runtime -- demos/11.1-lowering/02_control.wat     # Result : [I64(159)]
cargo run -q --bin runtime -- demos/11.1-lowering/03_module.wat      # Result : [I64(4)]
```

The runtime (`runtime/src/main.rs`) loads the `.wat` into Wasmer, which
accepts the text format directly, provides `eta_malloc`, calls the exported
`main` and prints what it returns. `01_straight`: `z = 5`, `x = 20` and then
`x = 20 * 4 = 80`. `03_module`: `add(1, 2) = 3`, then `calls` is 1 and
`add(3, 1) = 4`.

To get a binary `.wasm` instead, for a browser or another runtime:
`wat2wasm demos/11.1-lowering/03_module.wat` (from WABT).
