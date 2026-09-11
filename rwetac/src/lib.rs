//! A compiler from ETA to WASM
//!
//! For a high-level overview of the codebase, see the [`architecture`] module.
//! For the WASM runtime, see the [`runtime`](../runtime/index.html) crate.
#[doc = include_str!("../../architecture.md")]
pub mod architecture {}
pub mod ast;
pub mod compile;
pub mod emitter;
pub mod lexer;
pub mod opt;
pub mod parser;
pub mod resolver;
pub mod setting;
pub mod source_map;
pub mod symbols;
pub mod token;
pub mod typecheck;
pub mod types;
pub mod util;
pub mod wtac;
