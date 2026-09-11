#!/usr/bin/env python3
"""Strips the noise out of a --parse dump so it fits on a slide.

Removes the `Expression { kind: ..., span: N..M }` wrapper around every node and
every remaining span, leaving the tree itself. Reads stdin, writes stdout.

    cargo run --bin rwetac -- --parse demos/2.5-ast/02_precedence.eta \
        | python3 demos/2.5-ast/trim_ast.py

Pass --indent to break it across lines instead of keeping it on one.
"""
import re
import sys

WRAPPER = re.compile(r"Expression \{ kind: ((?:[^{}]|\{[^{}]*\})*?), span: \d+\.\.\d+ \}")


def trim(text):
    prev = None
    while prev != text:
        prev = text
        text = WRAPPER.sub(r"\1", text)
    return re.sub(r", span: \d+\.\.\d+", "", text)


def indent(text, width=2):
    out, depth, i = [], 0, 0
    while i < len(text):
        ch = text[i]
        if ch in "([{":
            depth += 1
            out.append(ch + "\n" + " " * width * depth)
        elif ch in ")]}":
            depth -= 1
            out.append("\n" + " " * width * depth + ch)
        elif ch == ",":
            out.append(",\n" + " " * width * depth)
        else:
            out.append(ch)
            i += 1
            continue
        # Skip the source's own spacing; the indentation replaces it.
        i += 1
        while i < len(text) and text[i] == " ":
            i += 1
    return "".join(out)


if __name__ == "__main__":
    result = trim(sys.stdin.read())
    print(indent(result) if "--indent" in sys.argv else result)
