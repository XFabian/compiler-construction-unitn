#!/usr/bin/env python3
"""Compact one of rwetac's CFG dumps into the textbook CFG of the slides.

rwetac keeps a block for every `block`/`loop` label, because the
reconstruction needs them. For the lecture, this script draws the graph a
leaders-based builder would give:

- a block that is empty or holds only a `br` is dropped, its edge goes on;
- a block with one successor is merged with that successor when the
  successor has no other predecessor.

The remaining blocks keep the number of their first rwetac block. `br`
lines are left out: the edges show them.

    python3 demos/9.1-cfg/compact_cfg.py IN.dot OUT.dot
"""
import re
import sys


def read(path):
    src = open(path).read()
    title = re.search(r'label="(Function: [^"]*)"', src)
    blocks = {}
    for m in re.finditer(r'"(Block \d+)" \[label="(.*?)"\];', src, re.S):
        lines = [l.strip() for l in m.group(2).replace("\n", "").split("\\l")]
        blocks[m.group(1)] = [re.sub(r"^\d+: ", "", l) for l in lines[1:]
                              if l and not l.startswith("Name:")]
    edges = re.findall(r'"(Block \d+)" -> "(Block \d+)"', src)
    return (title.group(1) if title else ""), blocks, edges


def compact(blocks, edges):
    succ = {b: [] for b in blocks}
    pred = {b: [] for b in blocks}
    for a, b in edges:
        succ[a].append(b)
        pred[b].append(a)

    def code(b):
        return [i for i in blocks[b] if not i.startswith("br ")]

    def drop(b):
        s = succ[b][0]
        pred[s].remove(b)
        for p in pred[b]:
            succ[p] = [s if x == b else x for x in succ[p]]
            pred[s].append(p)
        del blocks[b], succ[b], pred[b]

    def merge(a, b):
        blocks[a] = code(a) + blocks[b]
        succ[a] = succ[b]
        for s in succ[b]:
            pred[s] = [a if x == b else x for x in pred[s]]
        del blocks[b], succ[b], pred[b]

    changed = True
    while changed:
        changed = False
        for b in list(blocks):
            if not code(b) and len(succ[b]) == 1 and pred[b]:
                drop(b)
                changed = True
                break
        if changed:
            continue
        for a in list(blocks):
            if len(succ[a]) == 1:
                b = succ[a][0]
                if b != a and len(pred[b]) == 1:
                    merge(a, b)
                    changed = True
                    break
    return succ


def write(path, title, blocks, succ):
    out = [f'digraph CFG {{ label="{title} (compact)"; labelloc=t;',
           '  node [shape=box fontname="monospace"];']
    for b in sorted(blocks, key=lambda x: int(x.split()[1])):
        body = "".join(f"{i}\\l" for i in blocks[b] if not i.startswith("br "))
        out.append(f'  "{b}" [label="{b}:\\l{body}"];')
    for a in blocks:
        for b in succ[a]:
            out.append(f'  "{a}" -> "{b}";')
    out.append("}")
    open(path, "w").write("\n".join(out) + "\n")


if __name__ == "__main__":
    title, blocks, edges = read(sys.argv[1])
    succ = compact(blocks, edges)
    write(sys.argv[2], title, blocks, succ)
