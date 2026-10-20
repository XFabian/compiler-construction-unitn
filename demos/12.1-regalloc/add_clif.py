#!/usr/bin/env python3
"""Put Cranelift's IR into a `wasmtime explore` page.

The explorer has a CLIF column, but Wasmtime 49 leaves it empty: it looks
for `wasm_func_N.clif`, while Cranelift writes `wasm[0]--function[N].clif`.
This script reads the files that `wasmtime compile --emit-clif DIR` writes
and fills the column the way the explorer would have.

    python3 add_clif.py PAGE.html CLIF_DIR
"""
import json
import re
import sys
from pathlib import Path

EMPTY = 'window.CLIF = {"functions":[]};'


def read_function(path):
    """The explorer's own parsing: `@offs` lines are linked to the WAT."""
    instructions = []
    for line in path.read_text().splitlines():
        if not line:
            continue
        offset = None
        if line.startswith("@"):
            offset = int(line[1:5], 16)
            line = line[28:]
        elif line.startswith("     "):
            line = line[28:]
        instructions.append({"wasm_offset": offset, "clif": line})
    return instructions


def main(page, clif_dir):
    html = Path(page).read_text()
    if EMPTY not in html:
        sys.exit(f"{page}: no empty CLIF column found")
    asm = json.loads(re.search(r"window\.ASM = (\{.*?\});\n", html, re.S).group(1))
    # wasm[0]--function[N].clif, or wasm[0]--function[N]--NAME.clif when the
    # module has names.
    files = {int(m.group(1)): f for f in Path(clif_dir).iterdir()
             if (m := re.match(r"wasm\[0\]--function\[(\d+)\]", f.name))}
    functions = []
    for func in asm["functions"]:
        path = files.get(func["func_index"])
        if path:
            functions.append({"func_index": func["func_index"],
                              "name": func["name"],
                              "demangled_name": func["demangled_name"],
                              "instructions": read_function(path)})
    clif = json.dumps({"functions": functions})
    Path(page).write_text(html.replace(EMPTY, f"window.CLIF = {clif};"))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
