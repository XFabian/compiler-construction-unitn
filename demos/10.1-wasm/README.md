# Demo 10.1 — Running WebAssembly, and finding bugs with wat2wasm

The modules from the slides, written by hand: run them with Wasmer, look at
the binary, and fix a type error with the help of `wat2wasm`.

You need [Wasmer](https://wasmer.io) (`curl https://get.wasmer.io -sSfL | sh`)
and [WABT](https://github.com/WebAssembly/wabt) for `wat2wasm`
(`apt install wabt`). Run everything from the repository root. **In class**
marks the questions to ask before showing the next output.

## Step 1: 01_add — run a function

The add module of the slide "Text and Binary Format":

```wasm
(module
  (func $add (param $a i64)
             (param $b i64)
             (result i64)
    local.get $a
    local.get $b
    i64.add)
  (export "add" (func $add)))
```

```
wasmer run demos/10.1-wasm/01_add.wat --invoke add 3 4      # 7
```

`--invoke` calls an exported function with the arguments that follow.
Wasmer reads the text format directly: it turns it into the binary, checks
it and compiles it to machine code before calling `add`. Only exported
functions can be invoked: remove the `export` line and Wasmer reports that
there is no function `add`.

**In class:** what does `--invoke add 3` do? (Wasmer refuses: `add` takes
two parameters.)

## Step 2: the binary

```
wat2wasm demos/10.1-wasm/01_add.wat -o demos/10.1-wasm/01_add.wasm
xxd demos/10.1-wasm/01_add.wasm
```

```
00000000: 0061 736d 0100 0000 0107 0160 027e 7e01  .asm.......`.~~.
00000010: 7e03 0201 0007 0701 0361 6464 0000 0a09  ~........add....
00000020: 0107 0020 0020 017c 0b                   ... . .|.
```

The 41 bytes of the slide. `wat2wasm -v` explains every byte, section by
section:

```
wat2wasm -v demos/10.1-wasm/01_add.wat -o demos/10.1-wasm/01_add.wasm
```

```
0000000: 0061 736d                                 ; WASM_BINARY_MAGIC
0000004: 0100 0000                                 ; WASM_BINARY_VERSION
; section "Type" (1)
...
000000d: 7e                                        ; i64
...
0000023: 20                                        ; local.get
0000024: 00                                        ; local index
0000025: 20                                        ; local.get
0000026: 01                                        ; local index
0000027: 7c                                        ; i64.add
0000028: 0b                                        ; end
```

The way back is `wasm2wat demos/10.1-wasm/01_add.wasm`. The names `$a`,
`$b` and `$add` are gone: the binary only has indices (`local.get 0`).

The binary runs just like the text:
`wasmer run demos/10.1-wasm/01_add.wasm --invoke add 3 4`.

## Step 3: 02_memory — the full example

The `$run` function of the slides "Full Example", with a memory:

```
wasmer run demos/10.1-wasm/02_memory.wat --invoke run 4     # 47
```

`run(4)` stores 42 at address 4, loads it back and adds 5.

**In class:** what does `--invoke run 65535` do? (It traps: an `i32.store`
writes 4 bytes, and 65535 + 4 is beyond the one page of 64 KiB. WebAssembly
checks every memory access.)

## Step 4: 03_bug — a type error

`$main` calls `$add` with one wrong constant, as on the slide "A Type
Error". Don't show the bug yet: run it first.

```
wasmer run demos/10.1-wasm/03_bug.wat --invoke main
```

```
error: Unable to compile "03_bug.wat"
╰─▶ 1: compile error: Validate("type mismatch: expected i64, found i32 (at offset 0x35)")
```

Wasmer refuses to run the module: it is validated before it runs. But the
message points to a byte offset in the binary, not to a line. Ask `wat2wasm`:

```
wat2wasm demos/10.1-wasm/03_bug.wat -o demos/10.1-wasm/03_bug.wasm
```

```
demos/10.1-wasm/03_bug.wat:9:5: error: type mismatch in call, expected [i64, i64] but got [i64, i32]
    call $add)
    ^^^^
```

A line, a column, and both stack types. The error is reported at the
`call`, because that is where the types are checked, but the cause is a
line above: the second argument is an `i32.const`.

**In class:** which line is wrong? Fix it live: `i32.const 4` becomes
`i64.const 4`. Then:

```
wat2wasm demos/10.1-wasm/03_bug.wat -o demos/10.1-wasm/03_bug.wasm   # no output: valid
wasmer run demos/10.1-wasm/03_bug.wat --invoke main                  # 7
```

`wat2wasm` prints nothing when the module is valid. Undo the fix afterwards
(`git checkout demos/10.1-wasm/03_bug.wat`).

This is how we will debug rwetac: when its output does not run, `wat2wasm`
on the emitted `.wat` points to the line where the stack has the wrong
types.

**In class (the quiz):** delete the `i32.const 4` line instead. What does
`wat2wasm` say? (`type mismatch in call, expected [i64, i64] but got
[i64]`.)

## Why rwetac has its own runtime

```
cargo run -q --bin rwetac -- demos/2.5-ast/01_functions.eta
wasmer run demos/2.5-ast/01_functions.wat --invoke main
```

```
Error while importing "env"."eta_malloc": unknown import.
```

Every module rwetac emits imports `eta_malloc`, and the `wasmer` command
cannot provide it. Our runtime (`cargo run --bin runtime`) is a small Rust
program that uses Wasmer as a library and provides the import. More on
that in the Code Generation lecture.
