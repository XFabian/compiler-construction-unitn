# Demo 5.3 — Testing the compiler, errors, and logging

How rwetac is tested, how it reports errors, and how to see what it is doing.
Two small Eta programs and two live steps.

Run everything from the repository root.

```
cargo test --workspace          # everything, about 130 tests, a few seconds
```

## Code tour: the five layers

| Layer | Where | What to open |
|---|---|---|
| unit | `#[cfg(test)] mod tests` inside `src/` | [`rwetac/src/setting.rs`](../../rwetac/src/setting.rs), [`token.rs`](../../rwetac/src/token.rs), [`opt/reaching.rs`](../../rwetac/src/opt/reaching.rs) |
| phase | [`rwetac/tests/typecheck_test.rs`](../../rwetac/tests/typecheck_test.rs) | one expression against a hand-built Γ: `common::setup()` in [`tests/common/mod.rs`](../../rwetac/tests/common/mod.rs) |
| snapshot | [`rwetac/tests/file_tests.rs`](../../rwetac/tests/file_tests.rs) | every file in `tests/inputs/invalid/` and `invalid_semantic/` against its `.snap` in `tests/snapshots/` |
| end-to-end | [`runtime/tests/compiler_tests.rs`](../../runtime/tests/compiler_tests.rs) | every `.eta` in `runtime/tests/integration_tests/`, compiled, run in wasmer, compared to its `// EXPECT:` |
| regression | [`rwetac/tests/scope_resolution.rs`](../../rwetac/tests/scope_resolution.rs) | one test per bug that was fixed |

### Running one layer

`--lib` runs only the tests inside `src/`; `--test <name>` runs one file in
`tests/`. A word after that filters by test name, as a substring.

```
cargo test -p rwetac --lib                              # unit, 18 tests
cargo test -p rwetac --lib token                        #   only those in token.rs
cargo test -p rwetac --test typecheck_test              # phase, 28 tests
cargo test -p rwetac --test file_tests                  # snapshot, 3 tests over all input files
cargo test -p rwetac --test file_tests invalid_typecheck_cases   # only the typecheck inputs
cargo test -p runtime --test compiler_tests             # end-to-end, 58 programs
cargo test -p runtime --test compiler_tests arr_        # only the programs named arr_*.eta
cargo test -p rwetac --test scope_resolution            # regression, 4 tests
```

The end-to-end tests are generated, one per file, and named after it
(`eta_integration_tests::path_01_tests_integration_tests_arr_access_eta`), so
the file name works as a filter. `-- --list` shows the names without running
anything.

Points to make while walking it:

- **Phase tests need no program.** `common::setup()` puts `x`, `y`, `a`, a record
  `p` and a function `test` into the symbol table by hand, and
  `typecheck_expr("1 + true")` checks one expression against that Γ. That is
  `Γ ⊢ e : t` as a test: demo 5.2 from the other side.
- **Two kinds of snapshot.** `typecheck_test.rs` keeps the expected output
  inline, `assert_snapshot!(out, @"int[]")`. `file_tests.rs` keeps it in files,
  one `.snap` per input program, 30 of them.
- **Not everything should be a snapshot.** `valid_cases` compiles every valid
  input with and without the optimizer and checks the WebAssembly with an
  *assert*. The comment says why: a snapshot of "it worked" can be accepted
  when it did not.
- **End-to-end runs every program twice.** Unoptimized and optimized must both
  give the `EXPECT` value, so an optimizer that changes the meaning of a
  program is caught. `// EXPECT: trap` expects a runtime abort (division by
  zero).

## Live 1 — a new snapshot test

A program that must be rejected becomes a test by dropping it into
`tests/inputs/invalid_semantic/`. `invalid_typecheck_cases` in
[`file_tests.rs`](../../rwetac/tests/file_tests.rs) loops over that folder and
snapshots the error message of each file, named after it. There is no
test code to write.

[`03_int_guard.eta`](03_int_guard.eta) has an `int` as the guard of an `if`,
which C accepts and the (IF) rule does not. No test covers that yet.

```
cp demos/5.3-testing/03_int_guard.eta rwetac/tests/inputs/invalid_semantic/int_guard.eta
cargo test -p rwetac --test file_tests
```

The test fails, because there is nothing to compare against yet:

```
Snapshot file: rwetac/tests/snapshots/file_tests__invalid_typecheck_cases@int_guard.snap
Snapshot: invalid_typecheck_cases@int_guard
Source: rwetac/tests/file_tests.rs:69
────────────────────────────────────────────────────────────────────────────────
+new results
────────────┬───────────────────────────────────────────────────────────────────
          1 │+Failure: Guard of If has to have Type bool. Found int
────────────┴───────────────────────────────────────────────────────────────────
To update snapshots run `cargo insta review`
```

What to point at:

- **Only `+new results`, no `-old snapshot`.** A new snapshot is shown as a
  diff against nothing. For a changed message you would see both lines.
- **insta wrote the result next to the snapshots**, as
  `…@int_guard.snap.new`. Nothing is accepted yet: the `.snap.new` is a proposal.
- **The test does not know whether the message is right.** It only records
  what the compiler says today. Read it before you accept it.

The `cargo insta` commands:

```
cargo insta pending-snapshots   # list the .snap.new files waiting for a decision
cargo insta review              # step through them: a = accept, r = reject, s = skip
cargo insta accept              # accept all without asking (reject: the opposite)
cargo insta test                # like cargo test, but collects every changed snapshot
                                # instead of stopping at the first failure
```

After `cargo insta review` and `a`, the `.snap.new` becomes
`file_tests__invalid_typecheck_cases@int_guard.snap`:

```
---
source: rwetac/tests/file_tests.rs
expression: out
---
Failure: Guard of If has to have Type bool. Found int
```

Run `cargo test -p rwetac --test file_tests` again and it passes. The input and
the `.snap` go into the commit together. If someone later changes that message,
this test fails and shows `-old` against `+new`, and they have to accept the
new message on purpose.

Remove both files again afterwards:

```
rm rwetac/tests/inputs/invalid_semantic/int_guard.eta \
   rwetac/tests/snapshots/file_tests__invalid_typecheck_cases@int_guard.snap
```

## Live 2 — a new end-to-end test is one file

```
cp demos/5.3-testing/02_sum.eta runtime/tests/integration_tests/
cargo test -p runtime -- 02_sum
```

`rstest`'s `#[files("tests/integration_tests/*.eta")]` turns every file into a
test; `runtime/build.rs` tells cargo to rebuild when a file is added, or the
new test would silently not exist. Change the first line to `// EXPECT: 56`
and run it again:

```
assertion `left == right` failed: 02_sum: wrong result unoptimised
  left: 55
 right: 56
```

Delete the file again afterwards.


## Errors: one type per phase

| Phase | Error type | Where |
|---|---|---|
| lexer | `LexicalError` | [`token.rs:13`](../../rwetac/src/token.rs) |
| parser | `ParserError` (`UnexpectedToken`, `InvalidExpression`) | [`parser.rs:27`](../../rwetac/src/parser.rs) |
| resolver | `ResolveError { msg, span }` | [`resolver.rs:29`](../../rwetac/src/resolver.rs) |
| type checker | `TypeError` (`MismatchedTypes`, `Generic`) | [`typecheck.rs:61`](../../rwetac/src/typecheck.rs) |

Every phase returns `Result<_, ItsError>`, and every error carries a `span`:
byte offsets into the source, attached by the lexer. Inside a phase, `?`
hands an error up to the caller. At the top,
[`compile.rs`](../../rwetac/src/compile.rs) turns each into one `anyhow`
error with a line and column (lines 87, 103, 115):

```rust
resolver.resolve(&mut ast).map_err(|e| {
    let (line, col) = source_map.get_span_location(&e.span);
    anyhow::anyhow!("Resolving failed at {}:{}. {}", line, col, e.msg)
})?;
```

and `main` returns `anyhow::Result<()>`, so the message is printed and the
process exits with an error. Everything else, the `panic!("Internal Error:
…")` calls, is a broken promise inside the compiler: a bug, not a user error.

## Logging

rwetac logs with `tracing`, quiet by default. `RUST_LOG` picks what to see, by
level and by module:

```
RUST_LOG=info  cargo run -q --bin rwetac -- demos/5.2-typing/02_gamma.eta      # the phases
RUST_LOG=rwetac::resolver=trace  ...                                        # demo 5.1
RUST_LOG=rwetac::typecheck=debug ...                                        # demo 5.2
RUST_LOG=rwetac::parser=debug    ...                                        # demo 4.4
```

- **Spans.** `compile.rs` opens one span per phase (`Parsing`, `Resolving`,
  `Typecheck`, `IR`), and every log line inside it is prefixed with it.
  `#[instrument]` on a function opens a span per call; nested calls nest, which
  is what demo 4.4 turns into a recursion trace.
- **Careful with `#[instrument]`.** It records the arguments with `Debug`, so
  `typecheck_statement` logs every statement's whole AST. That is why demo 5.2
  needs a script to read it, and why the parser's `#[instrument]` skips `self`.
- **In tests.** The end-to-end tests set up the same subscriber, so
  `RUST_LOG=rwetac::typecheck=debug cargo test -p runtime -- fib` shows the
  log for one program.
- **`--dump`** writes the IR before and after optimization and the control
  flow graphs as `.dot` files into `<program>_debug/`: logging for the later
  phases, as files.

## The programs

| File | Used in |
|---|---|
| `02_sum.eta` | Live 2: copy into the runtime tests, `EXPECT: 55` |
| `03_int_guard.eta` | Live 1: copy into `invalid_semantic/`, a new snapshot |
