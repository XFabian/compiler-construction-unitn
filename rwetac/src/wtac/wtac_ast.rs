//! WTAC IR node definitions.
//!
//! This module defines the intermediate representation used between the typed
//! AST and WASM emission. Each instruction operates on at most three addresses
//! (source operands + destination), following the three-address code pattern.
//!
//! Every instruction carries a [`WType`] so that the emitter and optimizer can
//! determine operand sizes without access to the symbol table.

/// WASM-level types used in the IR.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WType {
    /// 32-bit integer. Used for booleans, pointers, and array references.
    I32,
    /// 64-bit integer. Used for ETA `int` values.
    I64,
    FunType {
        param_type: Vec<WType>,
        ret_type: Option<Box<WType>>,
    },
}

/// Unary operators in the IR.
#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
    Not,
    /// Narrows an i64 to i32. Used for index calculations where the
    /// offset is computed in 64-bit but WASM memory addresses are 32-bit.
    Wrap,
}
/// Binary operators in the IR.
#[derive(Debug, Clone)]
pub enum BinaryOp {
    Add,
    Sub,
    Mult,
    Div,
    Mod,
    And,
    Or,
    Lt,
    Leq,
    Gt,
    Geq,
    Eq,
    Neq,
}

// Depending on Value Copy can act as a load or store

// A Mem value left hand side means store!
// A Mem value rhs means load (e.g array load)
// Mem values also store the offset to load from. This makes generating WTac more pleasant
// because we do not need to write so many offset calculatin instructions in the code
// let offset_ptr =
//               make_wtac_var [ Types.Array { elem_type = Types.Int } ]
//             in
//             let offset_instr =
//               W.Binary
//                 {
//                   op = Add;
//                   src1 = store_ptr;
//                   src2 = W.Imm (offset, W.I32);
//                   dst = offset_ptr;
//                   t = W.I32;
//                 }
//             in
// This can be shortened by specifying th eoffset in Mem

/// An operand in a WTAC instruction.
///
/// The `Mem` variant represents a memory access at a base pointer plus a
/// byte offset. Whether it acts as a load or a store depends on whether
/// it appears as `src` or `dst` in a [`Copy`](Instruction::Copy) instruction.
/// Mem is represented with \[\] in the textual representation.
/// \[a\] = 3 would be a store into memory!
/// x = \[a\] would load from a and then copies into x
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Value {
    /// A named variable (local or global).
    Var(String),
    /// A memory location: base pointer variable + byte offset.
    /// The [`WType`] indicates the size of the load/store.
    Mem {
        name: String,
        offset: usize,
        t: WType,
    },
    /// An immediate constant with its type.
    Imm(i64, WType),
}

/// A WTAC instruction.
#[derive(Debug, Clone)]
pub enum Instruction {
    /// Return from the current function, optionally with a value.
    Return(Option<Value>),
    /// Unary operation: `dst = op src`.
    Unary {
        op: UnaryOp,
        src: Value,
        dst: Value,
        t: WType,
    },
    /// Binary operation: `dst = src1 op src2`.
    Binary {
        op: BinaryOp,
        src1: Value,
        src2: Value,
        dst: Value,
        /// Type of the *destination*, which may differ from the operands
        /// (e.g. a comparison produces an i32 from two i64 operands).
        t: WType,
    },
    /// Copy (move) instruction: `dst = src`.
    ///
    /// This is the most versatile instruction. Depending on whether `src`
    /// or `dst` is a [`Mem`](Value::Mem) value, it acts as a load, store,
    /// or simple register-to-register move.
    Copy { src: Value, dst: Value, t: WType },
    /// Function call. `dst` is `None` for procedures (no return value).
    FCall {
        name: String,
        args: Vec<Value>,
        dst: Option<Value>,
    },
    /// A WASM `block` construct. A `Br` targeting this label jumps to
    /// *after* the block (forward branch).
    Block {
        label: String,
        body: Vec<Instruction>,
    },
    /// A WASM `loop` construct. A `Br` targeting this label jumps back to
    /// the *start* of the loop (backward branch).
    Loop {
        label: String,
        body: Vec<Instruction>,
    },
    /// Unconditional branch to a label.
    Br(String),
    /// Conditional branch: jumps to label `l` if `cond` is nonzero.
    BrIf { cond: Value, l: String },
}

/// A top-level declaration in the WTAC IR.
#[derive(Debug, Clone)]
pub enum TopLevel {
    /// A function with its name, body, parameters, type, and local variable declarations.
    Function {
        name: String,
        body: Vec<Instruction>,
        params: Vec<String>,
        f_type: WType,
        locals: Vec<(String, WType)>,
    },
    /// A global data declaration (e.g. a global variable with an initial value).
    Data { name: String, t: WType, v: Value },
}

/// The root of a WTAC program.
#[derive(Debug, Clone)]
pub struct Program {
    pub decls: Vec<TopLevel>,
}

/// Counts instructions in a body, descending into `Block` and `Loop`.
///
/// The `Block`/`Loop` wrapper itself counts as one, since it becomes a real
/// WASM instruction.
pub fn count_instructions(instrs: &[Instruction]) -> usize {
    instrs
        .iter()
        .map(|instr| match instr {
            Instruction::Block { body, .. } | Instruction::Loop { body, .. } => {
                1 + count_instructions(body)
            }
            _ => 1,
        })
        .sum()
}

/// Per-function WTAC instruction counts, in declaration order.
///
/// This is the measurement behind "did that optimization actually do anything".
pub fn function_counts(prog: &Program) -> Vec<(String, usize)> {
    prog.decls
        .iter()
        .filter_map(|decl| match decl {
            TopLevel::Function { name, body, .. } => Some((name.clone(), count_instructions(body))),
            TopLevel::Data { .. } => None,
        })
        .collect()
}
