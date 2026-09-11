//! WTAC (WASM Three Address Code) intermediate representation.
//!
//! This is the IR that sits between the typed AST and WASM output.
//! Unlike a textbook TAC, WTAC retains structured control flow (`Block`, `Loop`,
//! `Br`, `BrIf`) because WASM requires it — there is no arbitrary `goto` in WASM.
//!
//! The IR is defined in [`wtac_ast`], generated from the AST by [`wtac_gen`],
//! and can be pretty-printed with [`wtac_print`] (used by `--dump` and `--wtac`).

pub mod wtac_ast;
pub mod wtac_gen;
pub mod wtac_print;
