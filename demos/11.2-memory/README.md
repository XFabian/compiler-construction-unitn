# Demo 11.2 — Memory: where values live, the stack pointer, eta_malloc

Four steps, in the order of the lecture:

1. `01_where` — the quiz "Where does X live?", answered by the WASM.
2. `02_stack` — how `__stack_pointer` makes room for records.
3. `03_malloc` — what `eta_malloc` hands out. We change the allocator so
   that it reports every allocation.
4. Adding `print_i32` — a second host function, used to watch the stack
   pointer move while the program runs.

Steps 3 and 4 edit the runtime, `runtime/src/main.rs`, live. The runtime
is an ordinary Rust program, and it is ours to change.

Run everything from the repository root. **In class** marks the questions
to ask before showing the next output.

## The commands

```
cargo run -q --bin rwetac -- demos/11.2-memory/01_where.eta
cargo run -q --bin runtime -- demos/11.2-memory/01_where.wat
```

The first command writes `demos/11.2-memory/01_where.wat`, the second runs
it. The same goes for `02_stack` and `03_malloc`.

## The memory layout

Every module rwetac emits ends with three globals of its own:

```wasm
(global $__stack_pointer (mut i32) (i32.const 66636) )
(global $__data_end i32 (i32.const 100) )
(global $__heap_base (export "__heap_base") (mut i32) (i32.const 66636) )
```

They divide the linear memory, as on the slide "Who Owns Linear Memory?":

```
0            100                         66636
| static data |  stack, grows down  <-  |  ->  heap, grows up  ...
               (64 KiB)                 ^ __stack_pointer starts here
                                        ^ __heap_base, where eta_malloc starts
```

Stack and heap start at the same address and grow apart, so neither can run
into the other. The static data is empty: Eta globals are WASM globals (step
1). `__heap_base` is exported because the runtime has to know where its heap
starts.

## Step 1: 01_where — where does X live?

**In class:** this is the quiz program. Ask the quiz first, then show where
rwetac put each variable.

```
g : int = 10
foo(x : int) : int {
    y : int = x + 1
    p : Point = Point(x, y)
    a : int[] = {1, 2, 3}
    return y
}
```

Open `01_where.wat`:

| Variable | Where | In the WASM |
|---|---|---|
| `g` | WASM global | `(global $g (mut i64) (i64.const 10))`; `main` reads it with `global.get $g` |
| `x` | WASM local | `(param $x.0 i64)`: a parameter is a local |
| `y` | WASM local | `(local $y.1 i64)` |
| `p` | stack | `(local $p.2 i32)` holds an address. The 16 bytes of the `Point` are in `foo`'s stack frame |
| `a` | heap | `(local $a.3 i32)` holds an address. The 32 bytes come from `call $eta_malloc` |

Look at the locals' types: `p` and `a` are `i32`, not `Point` or `int[]`.
WASM only has numbers. A record or an array is a piece of linear memory,
and the local holds its address. Only the data lives on the stack or the
heap.

The record, inside `foo`:

```wasm
global.get $__stack_pointer
i32.const 16                  ;; foo's frame: one Point = 16 bytes
i32.sub
global.set $__stack_pointer
global.get $__stack_pointer
local.set $tmp.1              ;; tmp.1 = start of the frame
...
local.get $tmp.4              ;; p.x = x, at offset 0
i32.const 0
i32.add
local.get $x.0
i64.store
local.get $tmp.4              ;; p.y = y, at offset 8
i32.const 8
i32.add
local.get $y.1
i64.store
```

The array:

```wasm
i32.const 32                  ;; 8 for the length + 3 × 8
call $eta_malloc
local.set $tmp.5
local.get $tmp.5              ;; the length first
i32.const 0
i32.add
i64.const 3
i64.store
local.get $tmp.5              ;; a points behind the length
i32.const 8
i32.add
local.set $tmp.6
...
```

**In class:** `foo` returns, and its frame is freed. What happens to the
`Point`, and what to the array? (The `Point`'s bytes are left behind and
overwritten by the next call. The array stays on the heap. Nobody ever
frees it.)

## Step 2: 02_stack — the stack pointer

```
inner(n)  has  r : Point
outer(n)  has  q : Point, calls inner
main()    has  p : Point, calls outer
```

```
cargo run -q --bin rwetac -- demos/11.2-memory/02_stack.eta
cargo run -q --bin runtime -- demos/11.2-memory/02_stack.wat    # Result : [I64(7)]
```

Every function has the same frame code at its start and its end:

```wasm
global.get $__stack_pointer   ;; save the old stack pointer
local.set $tmp.0
global.get $__stack_pointer   ;; reserve the frame: sp = sp - 16
i32.const 16
i32.sub
global.set $__stack_pointer
global.get $__stack_pointer   ;; the frame starts at the new sp
local.set $tmp.1
...
local.get $tmp.0              ;; at the end: free the frame
global.set $__stack_pointer
```

The size, 16, is known at compile time: the sum of the function's records.
A function without records reserves 0 (demo 11.1). Freeing a frame is just
putting the old value back, so it costs one instruction and frees
everything in the frame at once.

**In class:** `main` calls `outer`, and `outer` calls `inner`. What values
does `__stack_pointer` take while the program runs? Write them down. Step 4
prints them.

WASM has a stack of its own, the operand stack that `local.get` pushes
onto. A program can't take the address of anything on that stack. A record
needs an address (`p` is an `i32`), so it goes into this second stack in
linear memory, which the program manages itself. In Rust or C compiled to
WASM it is called the *shadow stack*, and it works the same way.

## Step 3: 03_malloc — the allocator reports

```
a : bool[] = {true}          // 8 + 1 × 4 = 12 bytes
b : int[] = {1, 2}           // 8 + 2 × 8 = 24 bytes
c : bool[] = {true, false}   // 8 + 2 × 4 = 16 bytes
```

```
cargo run -q --bin rwetac -- demos/11.2-memory/03_malloc.eta
cargo run -q --bin runtime -- demos/11.2-memory/03_malloc.wat
```

```
Start of heap: 66640
Result : [I64(5)]
Value of the heap pointer: 66696
```

The heap pointer moved by 56 bytes, but the three arrays together are
52. Where did the other 4 go?

`eta_malloc` is not part of the module. The module only imports it:

```wasm
(import "env" "eta_malloc" (func $eta_malloc (param i32) (result i32)))
```

The runtime provides it. Open `runtime/src/main.rs` and find `eta_malloc`:
the bump allocator of the slides, written in Rust. Make it report every
allocation, after the line that computes `new_ptr`:

```rust
let new_ptr = ptr.checked_add(size).expect("overflow");
println!("eta_malloc({size}) = {ptr}, bump now {new_ptr}");    // add this
```

Run the same `.wat` again. It does not need to be recompiled, because only
the runtime changed:

```
Start of heap: 66640
eta_malloc(12) = 66640, bump now 66652
eta_malloc(24) = 66656, bump now 66680
eta_malloc(16) = 66680, bump now 66696
Result : [I64(5)]
Value of the heap pointer: 66696
```

The second call does not start at 66652, where the first one ended, but at
66656: `(*heap + 7) & !7` rounds the bump pointer up to the next multiple of
8 before it is handed out. Those are the 4 missing bytes, the padding of
the slide "Bump Allocator, Aligned". The array of `int`s is aligned for its
`i64.load`s. `__heap_base` is 66636, so even the first call is rounded up,
to 66640.

**In class:** the same program on the slide's numbers: `eta_malloc(7)`,
`eta_malloc(5)`, `eta_malloc(1)` from a heap at 0. Which pointers come out?
(0, 8, 16.)

## Step 4: adding print_i32

Eta has no `print`, so a program can only show its result. But the runtime
can give the module any function we want, in the same way it gives
`eta_malloc`. In `runtime/src/main.rs`, add the function

```rust
/// Imported by a module as `(import "env" "print_i32" (func $print_i32 (param i32)))`.
pub fn print_i32(value: i32) {
    println!("print_i32: {value}");
}
```

and offer it to the module next to `eta_malloc`:

```rust
let import_object = imports! {
    "env" => {
        "eta_malloc" => eta_malloc_func,
        "print_i32" => Function::new_typed(&mut store, print_i32),   // add this
    }
};
```

rwetac doesn't know about `print_i32`, so the call goes into
`02_stack.wat` by hand. First the import, as the second line of the module:

```wasm
(import "env" "eta_malloc" (func $eta_malloc (param i32) (result i32)))
(import "env" "print_i32" (func $print_i32 (param i32)))
```

Then, in each of the three functions, print the stack pointer right after
the frame is reserved:

```wasm
      i32.sub
      global.set $__stack_pointer
      global.get $__stack_pointer      ;; add
      call $print_i32                  ;; add
```

and after it is restored at the end of the function (four spaces of
indentation, after the closing `)` of `func_exit`):

```wasm
    local.get $tmp.0
    global.set $__stack_pointer
    global.get $__stack_pointer        ;; add
    call $print_i32                    ;; add
```

Don't recompile `02_stack.eta` now, or rwetac overwrites the edited `.wat`.
Run it:

```
cargo run -q --bin runtime -- demos/11.2-memory/02_stack.wat
```

```
Start of heap: 66640
print_i32: 66620      main reserves 16
print_i32: 66604      outer reserves 16
print_i32: 66588      inner reserves 16
print_i32: 66604      inner returns
print_i32: 66620      outer returns
print_i32: 66636      main returns: back at the start
Result : [I64(7)]
Value of the heap pointer: 66636
```

The stack grows down by one frame per call and shrinks back in reverse
order. The heap pointer did not move: this program never allocates.

**In class:** is 66620 a good address for a `Point`? (No: 66636 is not a
multiple of 8, so every frame is 4 bytes off, and each `i64.store` into a
`Point` is misaligned. WASM allows misaligned accesses, but they can be
slower. `eta_malloc` rounds the heap up. Nothing rounds the stack, which
would need `__heap_base` to be a multiple of 8.)



### Good Exercise: print in Eta

To give Eta programs their own `print(x : int)`, three places change:

1. The resolver and the typechecker know `print : int -> ()`, as they know
   the built-in `length` (`rwetac/src/resolver.rs`,
   `rwetac/src/typecheck.rs`).
2. The emitter imports it:
   `(import "env" "print_i64" (func $print_i64 (param i64)))`. A call is an
   ordinary `call $print_i64`.
3. The runtime provides `print_i64`, as above.

## After the demo

To undo the two runtime edits, remove the `println!` in `eta_malloc`, the
function `print_i32` and its line in `imports!`. They do no harm if they
stay: a module that does not import `print_i32` never sees it.
