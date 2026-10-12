# Demo 7.3 — Memory in WTAC: stack, heap, and returning values

Four programs, in the order of the lecture: a record on the stack, arrays on
the heap, a function with two return values, and the difference between
returning a record and returning an array.

Run everything from the repository root. For each program:

```
cargo run -q --bin rwetac -- --wtac demos/7.3-memory/01_record.eta
cargo run -q --bin rwetac -- --wtac demos/7.3-memory/02_arrays.eta
cargo run -q --bin rwetac -- --wtac demos/7.3-memory/03_multi_return.eta
cargo run -q --bin rwetac -- --wtac demos/7.3-memory/04_returning.eta
```

Add `--dump` to write the WTAC to `demos/7.3-memory/<name>_debug/<name>.debug.wtac`.
Ignore the function `length` at the end until `02_arrays`.

## 01_record — a record on the stack

```
record Point { x : int  y : int }
p : Point = Point(1, 2)
p.x = p.y + 3
return p.x
```

```
tmp.0 : i32 = __stack_pointer                       // save the old stack pointer
__stack_pointer : i32 = __stack_pointer - 16:i32    // reserve the frame: one Point
tmp.1 : i32 = __stack_pointer                       // start of the frame
tmp.2 : i32 = tmp.1 + 0:i32                         // p is at offset 0 in the frame
tmp.3 : i32 = tmp.2
[tmp.3 + 0]%i64 : i64 = 1:i64                       // p.x = 1
[tmp.3 + 8]%i64 : i64 = 2:i64                       // p.y = 2
p.0 : i32 = tmp.3
tmp.4 : i64 = [p.0 + 8]%i64                         // read p.y
tmp.5 : i64 = tmp.4 + 3:i64
tmp.6 : i64 = tmp.5
[p.0 + 0]%i64 : i64 = tmp.6                         // write p.x
tmp.7 : i64 = [p.0 + 0]%i64                         // read p.x
...
__stack_pointer : i32 = tmp.0                       // free the frame
```

These are the three `__stack_pointer` lines every function has: the frame is
reserved on entry and freed on exit. The size is known at compile time (here
16 bytes); a function without records reserves 0. `p` is only an `i32`
address. Every field access is a load or store at a fixed offset from it:
`x` at 0, `y` at 8. No call to an allocator.

## 02_arrays — arrays on the heap

```
A : int[n]              // size known at run time
A[0] = 7
B : int[] = {1, 2, 3}   // literal
F : bool[] = {true, false}
ok : bool = F[1]
M : int[2][3]           // two dimensions
M[1][2] = 5
return length(A) + A[0] + B[1] + M[1][2]
```

**Allocation** — `A : int[n]`

```
tmp.2 : i64 = n.0 * 8:i64
tmp.3 : i64 = tmp.2 + 8:i64              // + 8 for the length
tmp.4 : i32 = (i32) tmp.3
tmp.5 : i32 = eta_malloc(tmp.4)
[tmp.5 + 0]%i64 : i64 = n.0              // store the length first
tmp.6 : i32 = tmp.5 + 8:i32              // A points behind the length
A.1 : i32 = tmp.6
```

The array is on the heap (`eta_malloc`), its length in the 8 bytes before
element 0. That is why `length` at the end of every program reads
`[A - 8]`.

**Reading memory** — `[address + offset]%type`

```
tmp.7 : i64 = 0:i64 * 8:i64
tmp.8 : i32 = (i32) tmp.7
tmp.9 : i32 = A.1 + tmp.8
tmp.10 : i64 = 7:i64
[tmp.9 + 0]%i64 : i64 = tmp.10           // A[0] = 7
```

A memory operand is an `i32` address plus a constant offset; after `%` comes
how many bytes are read or written: `%i64` is 8 bytes, `%i32` is 4. On the
left of `=` it is a store, on the right a load.

**Literal** — `B : int[] = {1, 2, 3}`

```
tmp.11 : i32 = eta_malloc(32:i32)
[tmp.11 + 0]%i64 : i64 = 3:i64
tmp.12 : i32 = tmp.11 + 8:i32
[tmp.12 + 0]%i64 : i64 = 1:i64
[tmp.12 + 8]%i64 : i64 = 2:i64
[tmp.12 + 16]%i64 : i64 = 3:i64
B.2 : i32 = tmp.12
```

Here the size is known: 3 × 8 + 8 = 32, and every element is a store at a
constant offset.

**A bool array** — `F : bool[] = {true, false}` and `ok : bool = F[1]`

```
tmp.13 : i32 = eta_malloc(16:i32)        // 2 × 4 + 8
[tmp.13 + 0]%i64 : i64 = 2:i64           // the length is still an i64
tmp.14 : i32 = tmp.13 + 8:i32
[tmp.14 + 0]%i32 : i32 = 1:i32
[tmp.14 + 4]%i32 : i32 = 0:i32
F.3 : i32 = tmp.14
tmp.15 : i64 = 1:i64 * 4:i64             // index × 4, not × 8
tmp.16 : i32 = (i32) tmp.15
tmp.17 : i32 = F.3 + tmp.16
ok.4 : i32 = [tmp.17 + 0]%i32            // load 4 bytes
```

Compare with `B`: a `bool` is an `i32`, so elements are 4 bytes apart and
every access is `%i32`. Only the length in front stays `%i64`.

**Two dimensions** — `M : int[2][3]`

`M` is an array of 2 pointers. Each one points to its own array of 3 `int`s.
This is the code of the Multi-Dimensional Arrays slide:

```
tmp.18 : i64 = 2:i64 * 4:i64                    // outer array: 2 pointers × 4 bytes
tmp.19 : i64 = tmp.18 + 8:i64                   // + 8 for the length
tmp.20 : i32 = (i32) tmp.19
tmp.21 : i32 = eta_malloc(tmp.20)
[tmp.21 + 0]%i64 : i64 = 2:i64                  // outer length = 2
tmp.22 : i32 = tmp.21 + 8:i32                   // tmp.22 = the outer array
tmp.28 : i64 = 0:i64                            // i = 0
block while_end.33: {
        loop while_loop.33: {
                tmp.29 : i32 = tmp.28 < 2:i64   // while (i < 2)
                tmp.34 : i32 = ! tmp.29
                br_if tmp.34 while_end.33
                tmp.23 : i64 = 3:i64 * 8:i64    // inner array: 3 ints × 8 bytes
                tmp.24 : i64 = tmp.23 + 8:i64
                tmp.25 : i32 = (i32) tmp.24
                tmp.26 : i32 = eta_malloc(tmp.25)
                [tmp.26 + 0]%i64 : i64 = 3:i64  // inner length = 3
                tmp.27 : i32 = tmp.26 + 8:i32   // tmp.27 = the inner array
                tmp.30 : i64 = tmp.28 * 4:i64   // address of outer[i]
                tmp.31 : i32 = (i32) tmp.30
                tmp.32 : i32 = tmp.22 + tmp.31
                [tmp.32 + 0]%i32 : i32 = tmp.27 // outer[i] = inner, a 4-byte pointer
                tmp.28 : i64 = tmp.28 + 1:i64   // i = i + 1
                br while_loop.33
        }
}
M.5 : i32 = tmp.22                              // only now M is assigned
```

The outer array is allocated once, before the loop. The inner arrays are
allocated inside the loop, once per element, because each row needs its own
memory. The rows are not next to each other: `M` is not one block of 2 × 3
ints. The loop counter is a fresh temporary (`tmp.28`), not a variable from
the source.

With an unsized last dimension only the outer array is allocated:
`N : int[2][]` is the first six lines and no loop. Its two pointers stay
uninitialized until rows are assigned.

Reading `M[1][2]` is two loads:

```
tmp.54 : i64 = 1:i64 * 4:i64
tmp.55 : i32 = (i32) tmp.54
tmp.56 : i32 = M.5 + tmp.55
tmp.57 : i32 = [tmp.56 + 0]%i32          // M[1]: a pointer, 4 bytes
tmp.58 : i64 = 2:i64 * 8:i64
tmp.59 : i32 = (i32) tmp.58
tmp.60 : i32 = tmp.57 + tmp.59
tmp.61 : i64 = [tmp.60 + 0]%i64          // M[1][2]: an int, 8 bytes
```

The first load uses `* 4` and `%i32` (a pointer), the second `* 8` and
`%i64` (an `int`).

## 03_multi_return — two return values

```
g(n : int) : int, int { return n, n + 1 }
h() : int { a : int, b : int = g(3) ... }
```

The callee:

```
g(ret_ptr.2 : i32, n.0 : i64) {
        ...
        [ret_ptr.2 + 0]%i64 : i64 = n.0
        tmp.3 : i64 = n.0 + 1:i64
        [ret_ptr.2 + 8]%i64 : i64 = tmp.3
        br func_exit
        ...
        return
```

The type was rewritten: `g` got a first parameter `ret_ptr` and returns
nothing (no `->`, a bare `return`). It writes the two values to `ret_ptr`.

The caller:

```
__stack_pointer : i32 = __stack_pointer - 16:i32   // room for two ints in h's frame
tmp.5 : i32 = __stack_pointer
tmp.6 : i32 = tmp.5 + 0:i32
tmp.7 : i32 = tmp.6
g(tmp.7, 3:i64)
tmp.8 : i64 = [tmp.7 + 0]%i64
tmp.9 : i64 = [tmp.7 + 8]%i64
a.1 : i64 = tmp.8
b.2 : i64 = tmp.9
```

The memory for the results is in the *caller's* frame, reserved in its entry
like a record. After the call, the caller reads both values.

## 04_returning — returning a record vs returning an array

```
mkPoint(a : int) : Point { p : Point = Point(a, a + 1)  return p }
mkArray(a : int) : int[] { A : int[] = {a, a + 1}  return A }
```

**The record** is returned like two return values:

```
mkPoint(ret_ptr.2 : i32, a.0 : i64) {
        ...
        __stack_pointer : i32 = __stack_pointer - 16:i32    // p in mkPoint's frame
        ...
        [ret_ptr.2 + 0]%i64 : i64 = [p.1 + 0]%i64           // copy p.x
        [ret_ptr.2 + 8]%i64 : i64 = [p.1 + 8]%i64           // copy p.y
        ...
        return
```

`p` lives in `mkPoint`'s frame, and that frame is freed on return. So
returning `p`'s address would leave the caller with a dangling pointer.
Instead `p` is copied field by field into memory the caller passes in
`ret_ptr`:

```
main:
        __stack_pointer : i32 = __stack_pointer - 16:i32
        ...
        mkPoint(tmp.14, 3:i64)
        p.4 : i32 = tmp.14
```

**The array** is returned as one `i32`:

```
mkArray(a.2 : i64) -> i32 {
        tmp.8 : i32 = eta_malloc(24:i32)
        ...
        __ret_val : i32 = A.3

main:
        tmp.15 : i32 = mkArray(3:i64)
        A.5 : i32 = tmp.15
```

No copy and no `ret_ptr`: the array is on the heap and outlives the call,
so returning its address is safe. This is the Stack vs Heap slide in
practice.

## Running them

`04_returning` has a `main`. Compile without `--wtac` and run it:

```
cargo run -q --bin rwetac -- demos/7.3-memory/04_returning.eta
cargo run -q --bin runtime -- demos/7.3-memory/04_returning.wat    // Result : [I64(8)]
```

For the others, rename the function to `main` (`01_record`: 5,
`03_multi_return`: 7) or add `main() : int { return f(4) }` to `02_arrays`
(18).
