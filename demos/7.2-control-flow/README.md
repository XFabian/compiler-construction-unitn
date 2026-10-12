# Demo 7.2 — Control flow in WTAC

The `if` and `while` examples of the lecture, plus a function with two
`return`s. WTAC has no jumps to arbitrary labels: only `block`, `loop` and
`br`, as in WebAssembly.

Run everything from the repository root.

## The commands

```
cargo run -q --bin rwetac -- --wtac demos/7.2-control-flow/01_control.eta
cargo run -q --bin rwetac -- --wtac --dump demos/7.2-control-flow/01_control.eta
```

`--dump` writes the WTAC to
`demos/7.2-control-flow/01_control_debug/01_control.debug.wtac`.
Ignore the `__stack_pointer` lines and `length` (demo 7.3).

## The output, one function at a time

**If/else** — `choose`

```
block if_end.5: {
        block if_else.5: {
                tmp.2 : i32 = a.0 < b.1
                br_if tmp.2 if_else.5
                tmp.4 : i64 = 0:i64
                x.2 : i64 = tmp.4
                br if_end.5
        }
        tmp.3 : i64 = 1:i64
        x.2 : i64 = tmp.3
        br if_end.5
}
```

`br label` jumps to the *end* of the block with that label. The conditional
`br_if tmp.2 if_else.5` leaves `if_else` when the condition holds, and what
follows that block is the *then* branch. So the else branch is written inside
and the then branch after it: the reverse of the source. Ask why (the
conditional branch can only jump out, so the code it jumps to must come after
the block).

**While** — `count`

```
block while_end.11: {
        loop while_loop.11: {
                tmp.8 : i32 = i.4 < n.3
                tmp.12 : i32 = ! tmp.8
                br_if tmp.12 while_end.11
                tmp.9 : i64 = i.4 + 1:i64
                tmp.10 : i64 = tmp.9
                i.4 : i64 = tmp.10
                br while_loop.11
        }
}
```

A `br` to a `loop` jumps to its *start*, a `br` to a `block` to its end. The
condition is negated (`! tmp.8`) because the branch *exits* the loop.

**Return** — `max`

Here the whole function matters, not only the `if`:

```
max(a.5 : i64, b.6 : i64) -> i64 {
        block func_exit: {
                tmp.13 : i32 = __stack_pointer                  // frame setup (demo 7.3)
                __stack_pointer : i32 = __stack_pointer - 0:i32
                tmp.14 : i32 = __stack_pointer
                block if_end.16: {
                        block if_else.16: {
                                tmp.15 : i32 = a.5 > b.6
                                br_if tmp.15 if_else.16
                                br if_end.16
                        }
                        __ret_val : i64 = a.5                   // return a
                        br func_exit
                        br if_end.16
                }
                __ret_val : i64 = b.6                           // return b
                br func_exit
                br func_exit
        }
        __stack_pointer : i32 = tmp.13                          // frame end (demo 7.3)
        return __ret_val
}
```

The whole body sits in `block func_exit`. `return e` does not return: it
stores `e` in `__ret_val` and leaves that block with `br func_exit`. Both
`return`s land at the same place after the block, where the stack pointer is
restored and the single `return __ret_val` hands the value back. So a
function has exactly one exit, whatever the source looks like.

Without an `else`, the `if_else` block holds only the jump over the then
branch (`br if_end.16`). The `br if_end.16` after the first `br func_exit`
can never run: the lowering emits it at the end of every then branch. In the
same way every function body ends with one more `br func_exit`, so a `return`
at the end gives two in a row. The optimizer drops everything after an
unconditional `br` before it builds the CFG (`--opt --dump` shows the
result).

## Running it

Add

```
main() : int {
    return choose(1, 2) * 100 + count(5) * 10 + max(3, 9)
}
```

compile without `--wtac` and run
`cargo run -q --bin runtime -- demos/7.2-control-flow/01_control.wat`.
It prints `Result : [I64(159)]`.
