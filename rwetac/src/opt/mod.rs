//! Optimization framework operating on WTAC via a Control Flow Graph.
//!
//! This module contains the CFG construction, dataflow analysis infrastructure,
//! concrete analyses (liveness, reaching definitions, available copies,
//! dominators), and the transformations that use their results (dead code
//! elimination, constant propagation, copy propagation).
//!
//! The optimization pipeline is driven by [`optimizer::optmize`], which
//! iterates analyses and transformations until a fixed point is reached.

pub mod cfg;
pub mod dominators;
pub mod facts;
pub mod liveness;
pub mod memory;
pub mod optimizer;
pub mod reaching;
pub mod copies;
pub mod reconstruct_cfg;
pub mod solver;
