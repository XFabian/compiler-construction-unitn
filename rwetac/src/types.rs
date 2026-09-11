//! Type definitions for the ETA language.
//!
//! The [`Type`] enum represents all types that can appear in an ETA program.
//! It is used throughout the compiler: the [`Parser`](crate::parser::Parser) constructs
//! types from annotations, the [`Typechecker`](crate::typecheck::Typechecker) verifies
//! them, and [`get_size`] is used during WTAC lowering and code generation to
//! determine memory layout.
use std::fmt::{self};

use core::panic;

/// A type in the ETA type system.
#[derive(Debug, PartialEq, Clone)]
pub enum Type {
    Int,
    Bool,
    /// A placeholder type used for wildcard patterns (`_`) and empty array
    /// literals (`{}`). Resolved during typechecking.
    Unknown,

    /// A product of multiple types, used to represent multiple return values
    /// from functions (e.g. `foo() : int, bool` returns a `Tuple { elems: [Int, Bool] }`)
    Tuple {
        elems: Vec<Type>,
    },
    /// A dynamically-sized array type. Nesting represents multi-dimensional arrays,
    /// e.g. `int[][]` is `Array { elem_type: Array { elem_type: Int } }`.
    Array {
        elem_type: Box<Type>,
    },
    /// A named record type (e.g. `Point`). The actual field layout is stored
    /// in the [`SymbolTable`](crate::symbols::SymbolTable) as a
    /// [`RecordEntry`](crate::symbols::RecordEntry).
    Record(String),
    /// A function signature with parameter types and return types.
    /// Multiple return types are possible (e.g. `(int, int) -> (int, bool)`).
    FunType {
        param_types: Vec<Type>,
        ret_type: Vec<Type>,
    },
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // write!(f, "{:?}", self)
        match self {
            Type::Int => write!(f, "int"),
            Type::Bool => write!(f, "bool"),
            Type::Unknown => write!(f, "unknown"),
            Type::Tuple { elems } => {
                let t = format!(
                    "({})",
                    elems
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                write!(f, "{}", t)
            }
            Type::Array { elem_type } => write!(f, "{}[]", elem_type),
            Type::Record(name) => write!(f, "{name}"),
            Type::FunType {
                param_types,
                ret_type,
            } => {
                let params = match param_types.as_slice() {
                    [] => "()".to_string(),
                    [single] => single.to_string(),
                    many => format!(
                        "({})",
                        many.iter()
                            .map(|t| t.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                };
                let ret = match ret_type.as_slice() {
                    [] => "()".to_string(),
                    [single] => single.to_string(),
                    ts => format!(
                        "({})",
                        ts.iter()
                            .map(|t| t.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                };

                write!(f, "{} -> {}", params, ret)
            }
        }
    }
}

/// Returns the byte size of a type as it will be laid out in WASM memory.
///
/// Arrays and records are pointer types at runtime (4 bytes / i32).
/// Integers are 8 bytes (i64), booleans are 4 bytes (i32).
///
/// # Panics
///
/// Panics on [`Type::FunType`], [`Type::Tuple`], and [`Type::Unknown`],
/// which do not have a meaningful runtime size.
pub fn get_size(t: &Type) -> usize {
    match t {
        // Both are pointers to heap/ stack respectively
        Type::Array { .. } | Type::Record(_) => 4,
        Type::Int => 8,
        Type::Bool => 4,
        Type::FunType { .. } | Type::Tuple { .. } | Type::Unknown => {
            panic!("Internal Error: Function, Tuple Types and Unknown have no size")
        }
    }
}

/// Natural alignment of a type in bytes: a value of this type must start at an
/// offset that is a multiple of it.
///
/// These are the usual C / System V rules, where a primitive's alignment equals
/// its size. Pointers are 4 because we target wasm32.
pub fn get_align(t: &Type) -> usize {
    match t {
        Type::Array { .. } | Type::Record(_) => 4,
        Type::Int => 8,
        Type::Bool => 4,
        Type::FunType { .. } | Type::Tuple { .. } | Type::Unknown => {
            panic!("Internal Error: Function, Tuple Types and Unknown have no alignment")
        }
    }
}
