# Demo 9.1 — rwetac's control-flow graphs

The lecture drew CFGs by hand. Here rwetac draws them: for every function it
builds a CFG out of the WTAC, before running any optimization, and with
`--dump` it writes that graph as a Graphviz `.dot` file. Three programs, from
a single block to the running example of the slides.

rwetac's graph has more blocks than the slides, because it keeps a block for
every `block`/`loop` label. `compact_cfg.py` draws the graph of the slides
from it. The questions run on the compact graph; the raw one comes at the end.

Run everything from the repository root.

## The commands

The easiest way is the script: it draws the raw and the compact graph of all
three programs and prints where the `.svg` files are.

```
bash demos/9.1-cfg/render.sh
```

By hand, it is these commands, per program:

```
cargo run -q --bin rwetac -- --opt --dump demos/9.1-cfg/01_straight.eta
D=demos/9.1-cfg/01_straight_debug
python3 demos/9.1-cfg/compact_cfg.py $D/Dot_files/01_straight_f_initial.dot \
    $D/01_straight_compact.dot
dot -Tsvg $D/Dot_files/01_straight_f_initial.dot -o $D/01_straight.svg
dot -Tsvg $D/01_straight_compact.dot -o $D/01_straight_compact.svg

cargo run -q --bin rwetac -- --opt --dump demos/9.1-cfg/02_max.eta
D=demos/9.1-cfg/02_max_debug
python3 demos/9.1-cfg/compact_cfg.py $D/Dot_files/02_max_max_initial.dot \
    $D/02_max_compact.dot
dot -Tsvg $D/Dot_files/02_max_max_initial.dot -o $D/02_max.svg
dot -Tsvg $D/02_max_compact.dot -o $D/02_max_compact.svg

cargo run -q --bin rwetac -- --opt --dump demos/9.1-cfg/03_func.eta
D=demos/9.1-cfg/03_func_debug
python3 demos/9.1-cfg/compact_cfg.py $D/Dot_files/03_func_func_initial.dot \
    $D/03_func_compact.dot
dot -Tsvg $D/Dot_files/03_func_func_initial.dot -o $D/03_func.svg
dot -Tsvg $D/03_func_compact.dot -o $D/03_func_compact.svg
```

The CFG is only built when the optimizer runs, so `--dump` needs `--opt` (or
`--passes`). The file is named `<program>_<function>_initial.dot`: the graph as
built, before the first pass. Every program also gets a CFG for rwetac's
built-in `length`; ignore it. Open the `.svg` files in a browser. Everything
in `*_debug/` is ignored by git.

`compact_cfg.py` drops blocks that are empty or hold only a `br`, and merges
a block into its successor when it is that successor's only predecessor.
That is what finding leaders gives. Each block keeps the number of the first
rwetac block in it, and `br` lines are left out because the edges show them.

## The compact graphs

**01_straight.eta.** Straight-line code, so one block. `return d` became
`__ret_val = d`, and the block restores the stack pointer and holds the one
real `return`. That is the "One CFG per Function" slide.

**02_max.eta.** Two `return`s, one exit. The condition block branches to
Block 2 (`__ret_val = a`) and Block 1 (`__ret_val = b`). Both go on to Block
0, which holds the only `return`. Note the inverted if: `br_if tmp.2
if_else.3` jumps when `a > b` is true, into the *then* branch.

**03_func.eta.** The running example. The compact graph is the slide's graph:

| slide | compact | holds |
|---|---|---|
| 1 entry | Block 5 | the stack pointer |
| 2 `if ctr > 0` | Block 2 | `br_if tmp.11 while_end.10` |
| 3 `if n % 2 == 0` | Block 10 | `br_if tmp.4 if_else.7` |
| 4 `n = n * n` | Block 4 | the then-branch |
| 5 `ctr = ctr - 1` | Block 3 | the end of the loop body |
| 6 `return n` | Block 1 | `__ret_val = n`, `return` |

Questions for the class, with this graph on the screen:

- **Back edge.** Which edge is it? Block 3 → Block 2: the end of the loop
  body jumps back to the condition.
- **Dominators.** What dominates Block 3? 5, 2, 10 and 3, but not 4: Block
  3 can be reached from Block 10 directly.
- **The loop body.** Walk backwards from Block 3 and stop at Block 2:
  {2, 10, 4, 3}.
- **Post-dominators.** Which block post-dominates Block 10? Block 3: the two
  branches of the `if` meet again there.

## The raw graph

Now open `03_func.svg`, rwetac's own graph: 12 blocks instead of 6.

| compact | raw | why the extra blocks |
|---|---|---|
| Block 5 | 5, 6 (`func_exit`), 7 (`while_end.10`) | empty entry, frame, `block` label |
| Block 2 | 2 (`while_loop.10`) | |
| Block 10 | 8, 9 (`if_end.7`), 10 (`if_else.7`) | two `block` labels of the `if` |
| Block 4 | 4 (`merge_if_else.7`) | |
| Block 3 | 11, 3 (`merge_if_end.7`) | 11 holds only `br if_end.7` |
| Block 1 | 1 (`merge_while_end.10`), 0 (`merge_func_exit`) | the exit after `func_exit` |

- **Are these maximal basic blocks?** No. 5 → 6 → 7 is a chain without
  branches, and so is 8 → 9 → 10. A leaders-based builder would merge each
  chain into one block, which is what `compact_cfg.py` does.
- **Why does rwetac keep them?** Every `block` and `loop` label in the WTAC
  gets a block of its own: a merge block at the end of a `block`, a header
  block at the start of a `loop` (the "CFGs in rwetac" slide). The
  reconstruction at the end uses these labels to rebuild the `block`/`loop`
  nesting. Post-dominators find the end of the `if`: Block 3 post-dominates
  Block 10.
