# Demo 12.1 — Register Allocation, for Real

Who allocates registers for real? Two compilers:

1. `01_slots.c` — the running example in C, on Compiler Explorer: `-O0`
   is "Skip It: Everything in Memory", `-O2` keeps everything in
   registers.
2. `02_example.eta` — the same example through rwetac and Wasmtime: the
   engine allocates registers for our WASM locals ("Who Does This?").
   Wasmtime's explorer shows the WAT, Cranelift's IR and the x86 side by
   side, for two allocators: backtracking and single-pass.
3. `03_pressure.eta` — 20 values live at once. Even the good allocator has
   to spill, and its spill code looks like the spill slides.

Run everything from the repository root. **In class** marks the questions
to ask before showing the next output.

## Setup

Compiler Explorer needs only a browser: the prepared window is linked.

Wasmtime (not wasmer: wasmer cannot print the machine code it makes):

```
curl https://wasmtime.dev/install.sh -sSf | bash     # installs to ~/.wasmtime
wasmtime --version                                    # tested with 49.0.4
```

Then build all explorer pages at once:

```
bash demos/12.1-regalloc/explore.sh
```

It compiles both programs with rwetac and writes four pages to
`demos/12.1-regalloc/explore_debug/` (ignored by git), one per program and
allocator: `02_example_backtracking.html`, `02_example_single-pass.html`,
`03_pressure_backtracking.html`, `03_pressure_single-pass.html`. Open them
in a browser.

## 01_slots — Compiler Explorer

The prepared window: https://godbolt.org/z/q8q7sWd8s.

To build it again: paste `01_slots.c` into godbolt.org, language C,
compiler `x86-64 gcc 13.x`, options `-O0 -masm=intel`. Open a second
compiler pane with `-O2 -masm=intel`. Then **Share → Short Link**. The
listings below are gcc 13's. Pick a fixed version, not `trunk`, so that the
output keeps matching this README.

**In class:** before showing `-O0`, ask: what code does the plan of "Skip
It: Everything in Memory" give for these six lines?

`-O0`, shortened:

```
mov  QWORD PTR -56[rbp], rdi       x arrives in rdi and goes to its slot
mov  rax, QWORD PTR -56[rbp]       a = x + 1: load
add  rax, 1                        compute
mov  QWORD PTR -40[rbp], rax       store
mov  rax, QWORD PTR -56[rbp]       b = x * 2: load again
add  rax, rax
mov  QWORD PTR -32[rbp], rax
...
```

Every variable has a slot `-N[rbp]` (gcc uses rbp, the slides use sp: the
same idea). Every line loads, computes in `rax`, stores. `x` comes in a
register and is immediately put into memory. Nothing stays in a register
from one line to the next.

`-O2`:

```
lea  rax, 2[0+rdi*4]
lea  rax, [rax+rdi*2]
lea  rax, 1[rdi+rax]
ret
```

Three instructions, no memory. Be honest about this: most of the win
here is the optimizer (it computes `7 * x + 3` directly), not only the
register allocator. The point is the contrast: 24 instructions with 14
memory accesses vs. 4. (gcc on Ubuntu adds an `endbr64` at the top; godbolt's
gcc does not.)

Things to try live: hover a line in the C source to see its instructions;
switch to `x86-64 clang` or `armv8-a clang` and ask which registers ARM64
uses (`x0`, `w8`, …, see "Which ones depends on the CPU").

## The explorer pages

Three columns: the WAT, Cranelift's IR (CLIF), and the x86 that Wasmtime
makes. Matching colours link them: hover a WASM instruction to see the IR
and the machine code it became. `-O opt-level=0` turns off Cranelift's
optimizer, so that what we see is mostly the register allocator's work.

Two quirks:

- `$f` is shown as `<float>`. The explorer runs function names through a
  C++ demangler, and `f` is C++'s mangled name for `float`.
- The CLIF column is filled by `add_clif.py`. Wasmtime 49 leaves it empty:
  the explorer looks for `wasm_func_1.clif`, while Cranelift writes
  `wasm[0]--function[1]--f.clif`.

By hand, for one program and one allocator, it is:

```
cargo run -q --bin rwetac -- demos/12.1-regalloc/02_example.eta
D=demos/12.1-regalloc/explore_debug
mkdir -p $D
wasmtime explore -O opt-level=0 -O regalloc-algorithm=backtracking \
    demos/12.1-regalloc/02_example.wat -o $D/02_example_backtracking.html
wasmtime compile -O opt-level=0 -O regalloc-algorithm=backtracking \
    --emit-clif $D/02_example_backtracking_clif \
    demos/12.1-regalloc/02_example.wat -o $D/02_example.cwasm
python3 demos/12.1-regalloc/add_clif.py $D/02_example_backtracking.html \
    $D/02_example_backtracking_clif
```

For the other allocator, replace `backtracking` by `single-pass`
everywhere; for the other program, `02_example` by `03_pressure`. Wasmtime
reads the `.wat` directly, `wat2wasm` is not needed.

## 02_example — what the engine does with our locals

Open `02_example_backtracking.html`. The WAT column is rwetac's output:
`$f` has 14 locals (`$a.1`, `$tmp.2`, …), one per WTAC variable, as in
demo 11.1.

**In class:** how many registers will Wasmtime need for `$f`?

**The middle column, Cranelift's IR.** The locals are gone:

```
block0(v0: i64, v1: i64, v2: i64):      v2 is x
    v11 = iadd v2, v10   ; v10 = 1      a = x + 1
    v13 = imul v2, v12   ; v12 = 2      b = x * 2
    v15 = iadd v13, v14  ; v14 = 1      c = b + 1
    v17 = imul v15, v16  ; v16 = 2      d = c * 2
    v18 = iadd v13, v17                 e = b + d
    v19 = iadd v11, v18                 a + e
    ...
    return v19
```

Cranelift turns every local into SSA values (next lecture): each `v` is
written once. The copies `local.set $a.1`, `local.get $tmp.2`, … are not
in it at all: in the WAT column they have no colour, because they became
no IR and no machine code. A copy is just a new name for the same value.
This column is the same for both allocators: register allocation comes
after it.

**The right column, x86:**

```
push    rbp
mov     rbp, rsp
mov     r8d, dword ptr [rdi + 0x70]   rwetac's __stack_pointer (see below)
sub     dword ptr [rdi + 0x70], 0
imul    r9, rdx, 2                    b = x * 2        x arrives in rdx
lea     r10, [r9 + 1]                 c = b + 1
imul    r10, r10, 2                   d = c * 2        d reuses c's register
add     r9, r10                       e = b + d        e reuses b's register
lea     rax, [rdx + r9 + 1]           a + e = x + 1 + e, into rax: the result
mov     dword ptr [rdi + 0x70], r8d
mov     rsp, rbp
pop     rbp
ret
```

What to point out:

- **14 locals, 4 registers** (`rdx`, `r9`, `r10`, `rax`; `r8` only holds
  rwetac's stack pointer), no stack slots.
- **d takes c's register, e takes b's**: exactly "Allocation: Pack the
  Bars".
- **Fixed registers**: Wasmtime's calling convention passes a hidden
  pointer to the instance in `rdi`, the first WASM argument in `rdx`,
  and the result goes in `rax` ("Some Registers Are Fixed").
- The `[rdi + 0x70]` lines are rwetac's `__stack_pointer` global from demo
  11.2. It lives in the instance; rwetac moves it by 0 here, because `$f`
  has no arrays.

Now open `02_example_single-pass.html`, the fast allocator. Same WAT, same
IR, but 40 instructions instead of 13. It makes a stack frame
(`sub rsp, 0x50`), saves all five callee-saved registers (`rbx`,
`r12`–`r15`) "just in case", and stores values only to load them right
back:

```
mov     qword ptr [rsp + 0x10], r13
mov     rax, qword ptr [rsp + 0x10]
```

That is "Who Uses What" live: single-pass compiles faster, backtracking
(Cranelift's regalloc2, the default) makes better code.

## 03_pressure — spills

Open `03_pressure_backtracking.html`. `return a + (b + (c + ...))` keeps
all 20 values live until the additions at the end. x86-64 has 16
registers, minus `rsp`, `rbp` and `rdi` (the instance).

**In class:** which values will be spilled? (Remember linear scan: the
one that ends last.)

```
imul    r13, rdx, 2                   a = x * 2
mov     qword ptr [rsp + 0x30], r13   spill a: store right after the definition
imul    rsi, rdx, 3                   b = x * 3
mov     qword ptr [rsp + 0x28], rsi   spill b
...                                   c to g too
imul    rbx, rdx, 9                   h to t stay in registers
...
add     rax, rbx
mov     rcx, qword ptr [rsp]          reload g right before its use
add     rax, rcx
...
mov     r13, qword ptr [rsp + 0x30]   reload a, the last one used
add     rax, r13
```

- a to g are spilled: stored right after their definition, loaded right
  before the addition. The spill slides, line by line.
- They are the values defined first and read last: the longest ranges,
  the same choice as linear scan.
- The prologue saves `rbx` and `r12`–`r15`: callee-saved registers that
  `$f` uses ("Calls Destroy Registers", seen from the callee).

Then `03_pressure_single-pass.html`: 100 instructions and 44 stack
accesses instead of 81 and 26.
