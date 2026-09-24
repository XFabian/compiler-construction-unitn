#!/usr/bin/env python3
"""Turns a RUST_LOG parser trace into something that fits on a slide.

`parse_expression` is the one instrumented function in the parser, so every
`tracing` span in the log is one call of it. The span prefix nests, and that
nesting is the recursion — this script turns it back into indentation.

    RUST_LOG=rwetac::parser=debug cargo run -q --bin rwetac \
        -- --parse demos/3.4-parsing/01_precedence.eta 2>&1 \
      | python3 demos/3.4-parsing/trim_trace.py

Log lines become the trace; anything else (the `--parse` dump, or an error)
is passed through with spans stripped, the same way trim_ast.py does it.
Pass --indent to break that dump across lines.
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "2.5-ast"))
from trim_ast import indent, trim  # noqa: E402

ANSI = re.compile(r"\x1b\[[0-9;]*m")
LEVEL = re.compile(r"^(TRACE|DEBUG|INFO|WARN|ERROR)\s+(.*)$")
# Each `parse_expression{min_prec=N}` in the prefix is one live call.
FRAME = re.compile(r"parse_expression\{min_prec=(\d+)\}")
TOKEN = re.compile(r"Current Token : (.*)$")


def main():
    trace, rest = [], []
    for raw in sys.stdin:
        # tracing right-aligns the level in five columns, so INFO and WARN
        # arrive with a leading space.
        line = ANSI.sub("", raw).strip()
        if not line:
            continue
        level = LEVEL.match(line)
        if not level:
            rest.append(line)
            continue
        body = level.group(2)
        tok = TOKEN.search(body)
        if not tok:
            # `enter`/`exit`/`close` and the per-function info! lines.
            continue
        frames = FRAME.findall(body)
        depth = len(frames) - 1
        trace.append(f"{'  ' * depth}parse_expression(min_prec={frames[-1]})"
                     f"   tok={tok.group(1)}")

    print("\n".join(trace))
    if rest:
        print()
        body = "\n".join(trim(line) for line in rest)
        print(indent(body) if "--indent" in sys.argv else body)


if __name__ == "__main__":
    main()
