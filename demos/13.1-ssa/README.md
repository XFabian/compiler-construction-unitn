# Demo 13.1 — SSA in Real Compilers

GCC and LLVM both put every function into SSA form. On Compiler
Explorer we can look at it directly:

1. `01_straight.c` — straight-line renaming: `a` becomes `a_1` and `a_3`.
2. `02_if.c` — a φ where the two branches meet.
3. `03_loop.c` — the sum loop from the renaming slides: φs in the loop
   header. Clang at `-O0` keeps every variable in memory (as in demo 12.1),
   and one LLVM pass builds SSA from that.
4. `04_swap.c` — trap 2: the swap lives in the φs, and the machine code
   needs a temporary.
5. `05_undefined.c` — a variable with no definition on one path.

**In class** marks the questions to ask before showing the next output.

## Setup

Compiler Explorer needs only a browser: <https://godbolt.org>. Set it up
once, then paste each file into the same editor; the panes update by
themselves.

- **GCC:** compiler `x86-64 gcc`, options `-O1`. In the compiler pane,
  click **Add new… → GCC Tree/RTL** and pick the pass `ssa` (a GIMPLE
  pass, early in the list). That is the function right after GCC built
  SSA, before any optimization.
- **Clang:** a second compiler pane, `x86-64 clang`, options
  `-O1 -fno-discard-value-names`. Click **Add new… → Opt Pipeline**: on
  the left is the list of LLVM passes, and a click on one shows the IR
  before and after it. Pick **SROAPass** (the first pass that changes
  anything). Without `-fno-discard-value-names` clang names its values
  `%0`, `%1`, …, not `%i.0`.

Pass numbers and value numbers (`_5`, `!5`) can differ between compiler
versions. The outputs below are from gcc 13 and clang 18.

## 01_straight — renaming

```c
int a = 1;
int b = a + 2;
a = 7;
return a + b;
```

**In class:** what names will GCC give the two `a`s?

GCC, pass `ssa`:

```
a_1 = 1;
b_2 = a_1 + 2;
a_3 = 7;
_4 = a_3 + b_2;
return _4;
```

The slide "Renaming Straight-Line Code", with an underscore. Every name
is written once. `_4` is a temporary: it has no variable in C, so it has
no name before the number. GCC renames but does not optimize yet:
`a_1 + 2` is still there.

Clang, after SROA:

```
%add = add nsw i32 1, 2
%add1 = add nsw i32 7, %add
ret i32 %add1
```

There are no `a` and `b` at all, and no copies: `a_1 = 1` is gone, and
the 1 is used directly. In LLVM an instruction *is* its value, and the
operand `%add` is an edge to the instruction that computes it: "The SSA
Philosophy". The names `%add`, `%add1` only come from the operator.

## 02_if — φ on an if

**In class:** where does the φ go, and what are its arguments?

GCC:

```
<bb 3> :
x_4 = 1;
goto <bb 5>;

<bb 4> :
x_3 = 2;

<bb 5> :
# x_1 = PHI <x_4(3), x_3(4)>
_5 = x_1;
return _5;
```

The φ is at the top of the join block, marked with `#`: GCC also does
not treat it as a real instruction. Each argument has its predecessor in
brackets: `x_4(3)` is the value on the edge from block 3. So GCC's φ
writes down the edges that the slides show with the orange arrows.

`p_2(D)`: the parameter `p`. `(D)` means *default definition*: the value
the variable has on entry to the function. For a parameter, that is the
argument.

Clang:

```
if.end:                                    ; preds = %if.else, %if.then
  %x.0 = phi i32 [ 1, %if.then ], [ 2, %if.else ]
  ret i32 %x.0
```

The same: `[value, predecessor block]` pairs. The arguments are constants
because there are no copies (see 01). `if.then` and `if.else` are now
empty: they only exist so that the φ can tell the two edges apart.

## 03_loop — from memory to SSA

First the input of the construction. Change the clang options to
`-O0 -emit-llvm -fno-discard-value-names` (the output pane now shows LLVM
IR, not x86).

**In class:** is this SSA?

```
entry:
  %i = alloca i32, align 4
  %s = alloca i32, align 4
  store i32 0, ptr %i, align 4
  store i32 0, ptr %s, align 4
  br label %while.cond

while.cond:
  %0 = load i32, ptr %i, align 4
  %cmp = icmp slt i32 %0, 3
  br i1 %cmp, label %while.body, label %while.end

while.body:
  %1 = load i32, ptr %s, align 4
  %2 = load i32, ptr %i, align 4
  %add = add nsw i32 %1, %2
  store i32 %add, ptr %s, align 4
  %3 = load i32, ptr %i, align 4
  %add1 = add nsw i32 %3, 1
  store i32 %add1, ptr %i, align 4
  br label %while.cond

while.end:
  %4 = load i32, ptr %s, align 4
  ret i32 %4
```

Yes and no. Every `%name` is written once, so the IR is in SSA. But `i`
and `s` are not values: they are stack slots (`alloca`), written with
`store` and read with `load`. "Skip It: Everything in Memory", in LLVM.
Memory is not in SSA: `%i` is stored three times. This is how clang
avoids the construction: its front end never has to place a φ.

Back to `-O1 -fno-discard-value-names` and the Opt Pipeline, pass
SROAPass on `sum`. Before it: the code above (plus `llvm.lifetime`
calls and `!tbaa` metadata, ignore them). After it:

```
while.cond:                                ; preds = %while.body, %entry
  %i.0 = phi i32 [ 0, %entry ], [ %add1, %while.body ]
  %s.0 = phi i32 [ 0, %entry ], [ %add, %while.body ]
  %cmp = icmp slt i32 %i.0, 3
  br i1 %cmp, label %while.body, label %while.end

while.body:                                ; preds = %while.cond
  %add = add nsw i32 %s.0, %i.0
  %add1 = add nsw i32 %i.0, 1
  br label %while.cond

while.end:                                 ; preds = %while.cond
  ret i32 %s.0
```

All loads, stores and allocas are gone, and the φs are in the loop
header, one for `i` and one for `s`: the result of "Placing φs" and
"Renaming", step 4. SROA ("scalar replacement of aggregates") includes
the classic `mem2reg` pass: it places φs with dominance frontiers and
renames by walking the dominator tree, as on the slides.

Things to try live: it only works for slots whose address does not
escape. Add `void use(int *p);` above the function and `use(&i);` after
`int s = 0;`. Now `i` stays an `alloca` with loads and stores (the
unknown function could keep the pointer), and only `s` gets a φ. GCC
does the same: `i = 0;` and `i.0_1 = i;` are memory accesses, and only
`s_5` has a PHI.

GCC, pass `ssa`: GCC builds SSA directly from its own IR, without the
detour through memory:

```
<bb 2> :
i_3 = 0;
s_4 = 0;
goto <bb 4>;

<bb 3> :
s_7 = s_2 + i_1;
i_8 = i_1 + 1;

<bb 4> :
# i_1 = PHI <i_3(2), i_8(3)>
# s_2 = PHI <s_4(2), s_7(3)>
if (i_1 <= 2)
  goto <bb 3>;
else
  goto <bb 5>;

<bb 5> :
_5 = s_2;
return _5;
```

The slides' `i2 = φ(i1, i3)`, with other numbers. GCC has already
turned `i < 3` into `i <= 2`, and puts the loop body before the header.

## 04_swap — trap 2

**In class:** what do the φs look like after SROA? There is a `t` in the
source.

Clang, after SROA:

```
while.cond:
  %a.0 = phi i32 [ 1, %entry ], [ %b.0, %while.body ]
  %b.0 = phi i32 [ 2, %entry ], [ %a.0, %while.body ]
  ...
while.body:
  %sub = sub nsw i32 %n.addr.0, 1
  br label %while.cond
```

`t` is gone, and the loop body only counts `n` down: the swap lives in
the two φs, which read each other. Exactly "Trap 2: The Swap". (clang may
list `%b.0` first; the order of φs does not matter, because they run all
at once.)

**In class:** how many registers does the machine code need for `a`
and `b`?

The x86 output of the GCC pane (`-O1`):

```
swap:
        test    edi, edi
        jle     .L4
        mov     eax, 2
        mov     edx, 1
        jmp     .L3
.L5:
        mov     ecx, edx        t = edx
        mov     edx, eax        edx = eax
        mov     eax, ecx        eax = t
.L3:
        sub     edi, 1
        jne     .L5
        ret
.L4:
        mov     eax, 1
        ret
```

Three registers: `ecx` is the temporary of "Trap 2: Copy in Parallel".
And `.L5` is a block that holds only the copies, on the back edge from
`.L3` to `.L3`: that edge is critical (`.L3` has two successors), so GCC
split it, as in "Trap 1: Split the Critical Edge". (GCC also rotated the
loop: it jumps to the test first and starts with `a` and `b` already
swapped once.)

The clang pane shows the same three `mov`s with `edx` as the temporary.

## 05_undefined — no definition on a path

**In class:** `x` has no value when `p` is 0. What will the φ be?

GCC:

```
<bb 3> :
x_4 = 1;

<bb 4> :
# x_1 = PHI <x_3(D)(2), x_4(3)>
```

`x_3(D)` is the default definition of `x` again, as for `p` in 02. For a
local, that is the uninitialized value: GCC gives the missing path a
definition at the function entry. That is "Undefined: Three Ways Out",
the `x0 = …` at the top, except that the value is "undefined".

Clang, after SROA:

```
if.end:
  ret i32 1
```

No φ at all. The φ would have been `phi [1, %if.then], [undef, %entry]`,
and LLVM may choose any value for `undef`. It chose 1, so both arguments
are 1 and the φ is just 1. The x86 code of both compilers is
`mov eax, 1; ret`: the `if` is gone. This is why reading an
uninitialized variable is undefined behaviour in C: the compiler does not
have to pick one value and stick to it.
