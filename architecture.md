# Architecture

This document describes the high-level architecture of rwetac, a compiler from the ETA language to WebAssembly (WASM).
It walks through the phases in the order the compiler runs them.

Where a choice was made between a clear implementation and a fast one, this
compiler takes the clear one, and every phase can be stopped at and printed.

## Bird's Eye View



On the highest level, rwetac reads a single `.eta` source file and produces a `.wat` (WebAssembly Text format) file.
The compilation is driven by `compile.rs`, which chains each phase together in sequence and allows stopping at any intermediate stage via command-line flags (`--parse`, `--validate`, `--wtac`, `--codegen`).

## Code Map

Each module and its responsibility, in pipeline order.
The **Design Decision** notes record choices that could reasonably have gone the other way, and why this one was taken.

### `main.rs`

The entry point. Uses `clap` to parse command-line arguments and delegates to `compile::compile()`. 
Configures `tracing` for structured logging of each compilation phase.

### `token.rs`

Defines the `Token` enum representing all lexical tokens in the ETA language (keywords, operators, literals, identifiers, etc.). 
Tokens are derived with the `logos` crate for lexer generation.

### `lexer.rs`

Wraps the `logos`-generated lexer into a peekable iterator. The parser consumes tokens through this interface.

The lexer supports `peek()` (look at the next token without consuming it) and `peek_nth()` for bounded lookahead, which the parser needs for predictive parsing decisions.

**Design Decision:** The lexer automatically inserts semicolons. This simplifies the grammar and parsing logic.

### `ast.rs`

Defines the Abstract Syntax Tree node types: `Program`, `Declaration`, `Function`, `Statement`, `Expression`, `Block`, `VarDeclaration`, `Record`, etc.
Every AST node carries a `Span` for error reporting.

### `parser.rs`

A hand-written recursive descent parser that consumes the token stream and produces the AST.
It uses Pratt parsing (precedence climbing) for expressions via `parse_expression(min_bp)`.

The parser provides helpful error messages by reporting the unexpected token together with its source location. 
`expect()` consumes the next token and verifies it matches a given kind, returning its `Span` on success or a `ParserError` on failure.

**Design Decision:** Parsing never panics. All errors are returned as `ParserError` variants and surfaced to the user with line and column information via the `SourceMap`.

### `resolver.rs`

Performs variable resolution: it walks the AST and renames all variables to unique names so that later phases do not need to worry about scoping. 
It also checks for duplicate declarations within the same scope.

After the resolver runs, every variable reference in the AST points to a unique name. This is a precondition for the typechecker and all subsequent phases.

**Design Decision:** Resolution is a separate pass from typechecking. This keeps both passes simpler and easier to understand independently, even though a single-pass approach would be possible.

### `typecheck.rs`

The typechecker walks the resolved AST and verifies type correctness. It checks that operations receive operands of the correct type, that function calls match their signatures, that assignments are well-typed, and so on.

The typechecker uses a `SymbolTable` to track variable types and function signatures. It produces `TypeError` variants on failure, which are mapped to source locations.

### `types.rs`

Defines the `Type` enum for the ETA type system: `Int`, `Bool`, `Array`, `Tuple`, `Record`, `FunType`, and `Unknown`. 
Also provides utility functions like `get_size()` for computing the byte size of a type (used during WTAC lowering and code generation).

### `symbols.rs`

The `SymbolTable` and related types (`Entry`, `Attr`, `RecordEntry`, `MemberEntry`). 
It maps identifier names to their types and attributes (local, global, function, record). The symbol table is populated during typechecking and consumed by later phases.

### `source_map.rs`

Maps byte offsets (from `Span`s) back to `(line, column)` positions in the original source file.
Used for all error messages throughout the compiler. It precomputes line start offsets at construction time and uses binary search for lookups.

### `setting.rs`

Defines the `Stage` enum (`Parse`, `Validate`, `Wtac`, `Codegen`) that controls how far the compiler runs, and the `Pipeline` of optimization `Pass`es to apply.
Each stage includes the ones before it, so stopping at `Wtac` still lexes, parses, resolves and typechecks first.

### `compile.rs`

The main compilation driver. It chains all phases together:
1. Read source file
2. Lex → Parse → Resolve → Typecheck → WTAC Generation → (Optimization) → Emit WASM

Each phase is wrapped in a `tracing` span for observability.
The `--dump` flag writes intermediate representations (WTAC, CFG graphs) to a `<file>_debug/` directory for inspection.

**Design Decision:** The compiler can stop at any stage and print the intermediate result.

### `wtac/`

The WTAC (WASM Three Address Code) intermediate representation and its generation.

- `wtac_ast.rs` — Defines the WTAC IR: `Instruction` (Copy, Binary, Unary, FCall, Block, Loop, Br, BrIf, Return, ...), `Value` (Var, Imm, Mem), `WType`, `TopLevel`, `Program`.
- `wtac_gen/` — Lowers the typed AST into WTAC. Creates temporaries for intermediate values, manages stack frames for records/tuples, and handles multi-return functions via a return pointer. Split into `mod.rs` (functions), `stmt.rs`, `expr.rs`, `array.rs` and `builder.rs`; each file's header documents one design decision.
- `wtac_print.rs` — Pretty-prints WTAC to files (used for `--dump`).

**Design Decision:** Our WTAC differs from a textbook TAC in an important way: it retains high-level control flow structures (`Block`, `Loop`, `Br`, `BrIf`) instead of lowering everything to flat jumps. 
This is because WASM itself has structured control flow — it does not have arbitrary `goto`. 
Flattening control flow in the IR would discard exactly the structure WASM needs, leaving it to be recovered later — which is the hardest part of the optimizer as it stands (see `reconstruct_cfg.rs`). 
Similarly, WASM has typed functions with parameters, so we keep function calls as a first-class instruction.

### `opt/`

The optimization framework, operating on WTAC via a Control Flow Graph (CFG).

- `cfg.rs` — Builds the CFG from WTAC instructions. Each `BasicBlock` has predecessors, successors, and labeled instructions. Supports `to_dot()` for Graphviz visualization.
- `optimizer.rs` — The optimization driver that chains analyses and transformations.
- `solver.rs` — A generic dataflow analysis solver (iterative fixed-point).
- `liveness.rs` — Live variable analysis (backward dataflow).
- `reaching.rs` — Reaching definitions analysis (forward dataflow).
- `dominators.rs` — Dominator tree computation.
- `memory.rs` — Memory-related optimizations.
- `reconstruct_cfg.rs` — Reconstructs WTAC from the optimized CFG, recovering the structured control flow that WASM needs.

**Design Decision:** The CFG can be dumped as Graphviz `.dot` files via the `--dump` flag, one per pass, so the effect of a transformation can be read off the graph rather than inferred from the IR text.

### `emitter.rs`

Translates WTAC into WebAssembly Text format (`.wat`). It uses the `pretty` crate for indented output.
The emitter maintains a `Bindings` table to distinguish local from global variables (WASM uses `local.get`/`local.set` vs `global.get`/`global.set`).

It handles the WASM memory layout: stack, data section, and heap, and emits the necessary memory management globals (`__stack_pointer`, `__data_end`, `__heap_base`).

**Design Decision:** We emit `.wat` (text format) rather than `.wasm` (binary).
The output is then readable, and diffing two compilations shows what a pass actually changed.
The `.wat` file can be converted to `.wasm` using standard tooling like `wat2wasm`.

### `util.rs`

Small helper functions used across the compiler (e.g., `extend_file_name` for debug output paths).


## Cross-Cutting Concerns

### Error Handling

Errors never panic the compiler (except for internal invariant violations, which use `panic!`). 
Each phase returns a `Result`, and errors are mapped to source locations using the `SourceMap` before being displayed to the user.

### Testing

Tests are organized in `tests/`:
- `tests/inputs/` — ETA source files for valid programs.
- `tests/inputs/invalid/` — Source files that should fail during parsing.
- `tests/inputs/invalid_semantic/` — Source files that should fail during typechecking.
- `tests/file_tests.rs` — Runs every input file through the compiler. Valid programs are compiled both with and without optimization and the emitted `.wat` is validated; the two invalid directories are snapshotted with `insta`, since there the expected output is an error message.
- `tests/wtac_wasm.rs` — Unit tests for specific WTAC-to-WASM translations (e.g., memory loads/stores, record operations), validating the output with the `wat` crate.

The `runtime/tests/integration_tests/` programs go further and are executed. Each carries an `// EXPECT: <int>` directive checked against both the optimized and unoptimized run, so a wrong compiler, a wrong optimizer and a wrong result width fail separately.

**Design Decision:** Tests are data-driven — adding a case is adding a `.eta` file to the right directory. Success is asserted rather than snapshotted, because a snapshot of a fixed success string lets `cargo insta accept` bless a real failure as the expectation.

### Tracing and Observability

The compiler uses the `tracing` crate to log each phase. Set the `RUST_LOG` environment variable to control verbosity.
The `--dump` flag writes intermediate files (WTAC representation, CFG graphs) to a debug directory for visual inspection.

### The `--dump` Flag

When passed, the compiler writes:
- The WTAC IR (before and after optimization) as text files.
- CFG graphs as `.dot` files, viewable with Graphviz.

