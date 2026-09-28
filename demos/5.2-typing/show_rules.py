#!/usr/bin/env python3
"""Turns the type checker's log into the rules it applied.

`typecheck_statement` is instrumented, so the log has one span per statement,
nested the way the statements are. This script names the statement rule each
span stands for -- (VARINIT), (SEQ), (IFELSE), (RETURN), ... -- indented by
nesting, and prints Γ before and after, one binding per line.

    RUST_LOG=rwetac::typecheck=debug cargo run -q --bin rwetac \
        -- --validate demos/5.2-typing/02_gamma.eta 2>&1 \
      | python3 demos/5.2-typing/show_rules.py

Lines that are not from the type checker (an error, say) pass through.
"""
import re
import sys

ANSI = re.compile(r"\x1b\[[0-9;]*m")
GAMMA = re.compile(r"(Top-Level|Final) SymbolTable sym_tab=Γ = \{ (.*) \}")
FUNCTION = re.compile(r"Function\{n=([^}]*)\}: rwetac::typecheck: enter")
STMT = "stmt=Statement { kind: "


def top_level_field(text, field):
    """The value of `field` at the outermost level of a Debug dump, or None."""
    depth = 0
    i = 0
    while i < len(text):
        c = text[i]
        if c in "{[(":
            depth += 1
        elif c in "}])":
            depth -= 1
        elif depth == 1 and text.startswith(field, i):
            return text[i + len(field):]
        i += 1
    return None


def assign_target(stmt):
    m = re.search(r'lhs: \[E\(Expression \{ kind: Var\("([^"]*)"\)', stmt)
    return f"{m.group(1)} = ..." if m else ""


def rule(stmt):
    """(RULE)  detail, for the Debug dump of one Statement."""
    kind = re.match(r"(\w+)", stmt).group(1)
    if kind == "LocalDecl":
        name = re.search(r'name: "([^"]*)"', stmt).group(1)
        t = re.search(r"var_type: (\w+)", stmt).group(1).lower()
        init = re.search(r"init: (Some|None)", stmt).group(1)
        return ("(VARINIT)" if init == "Some" else "(VARDECL)"), f"{name} : {t}"
    if kind == "If":
        els = top_level_field(stmt[len("If"):], "else_br: ")
        has_else = els is not None and els.startswith("Some")
        return ("(IFELSE)" if has_else else "(IF)"), "lub of the branches"
    return {
        "Compound": ("(SEQ)", "a block"),
        "Assign": ("(ASSIGN)", assign_target(stmt)),
        "While": ("(WHILE)", ""),
        "Return": ("(RETURN)", "checked against ρ = ret_t"),
        "Procedure": ("(PRCALL)", ""),
    }.get(kind, (f"({kind})", ""))


def split_top(text):
    """Splits at ", " outside brackets: fn (int, int) -> int stays whole."""
    parts, depth, start = [], 0, 0
    for i, c in enumerate(text):
        depth += c in "([{"
        depth -= c in ")]}"
        if depth == 0 and text.startswith(", ", i):
            parts.append(text[start:i])
            start = i + 2
    parts.append(text[start:])
    return parts


def print_gamma(label, entries):
    print(f"Γ {label}")
    for entry in split_top(entries):
        print(f"    {entry}")


def main():
    returns = {}                       # function -> its return type, from Γ
    for raw in sys.stdin:
        line = ANSI.sub("", raw.rstrip("\n"))
        m = GAMMA.search(line)
        if m:
            label = "before the bodies" if m.group(1) == "Top-Level" else \
                "after everything"
            print_gamma(label, m.group(2))
            for entry in split_top(m.group(2)):
                f, _, t = entry.partition(" ↦ ")
                if t.startswith("fn "):
                    returns[f] = t.rsplit(" -> ", 1)[1]
            continue
        m = FUNCTION.search(line)
        if m:
            name = m.group(1)
            if returns.get(name) == "()":
                print(f"\n{name}: a procedure, so its body may be unit")
            else:
                print(f"\n{name}: returns {returns.get(name, '?')}, so its "
                      "body must be void")
            continue
        if "rwetac::typecheck" in line:
            if "typecheck_statement{" in line and line.endswith("enter"):
                depth = line.count("typecheck_statement{")
                stmt = line[line.rfind(STMT) + len(STMT):]
                name, detail = rule(stmt)
                print("    " * depth + f"{name:<11}{detail}")
            continue
        print(line)


if __name__ == "__main__":
    main()
