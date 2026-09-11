//! Pretty-printing for WTAC IR.
//!
//! Implements [`Display`](std::fmt::Display) for all WTAC types so that the IR
//! can be printed to stdout (via `--wtac`) or written to debug files (via `--dump`).
//! The output uses a C-like syntax with indentation for nested `Block` and `Loop`
//! constructs.

use crate::wtac::wtac_ast::*;
use std::fmt::{self};
use std::fs;
use std::path::Path;

/// Writes the WTAC program to a `.debug.wtac` file next to the given path.
pub fn write_wtac_file(path: &Path, wtac: &Program) -> std::io::Result<()> {
    let new_path = path.with_extension("debug.wtac");
    fs::write(new_path, wtac.to_string())
}

impl fmt::Display for WType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WType::I32 => write!(f, "i32"),
            WType::I64 => write!(f, "i64"),
            WType::FunType {
                param_type,
                ret_type,
            } => {
                let params = match param_type.as_slice() {
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

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
            UnaryOp::Wrap => "(i32)",
        };
        write!(f, "{}", s)
    }
}

impl fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mult => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::And => "&",
            BinaryOp::Or => "|",
            BinaryOp::Lt => "<",
            BinaryOp::Leq => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Geq => ">=",
            BinaryOp::Eq => "==",
            BinaryOp::Neq => "!=",
        };
        write!(f, "{}", s)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Var(name) => write!(f, "{}", name),
            Value::Mem { name, offset, t } => write!(f, "[{} + {}]%{}", name, offset, t),
            Value::Imm(val, t) => write!(f, "{}:{}", val, t),
        }
    }
}

impl Instruction {
    fn fmt_with_indent(&self, f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
        let indent_str = "  ".repeat(indent);
        match self {
            Instruction::Return(opt) => match opt {
                Some(v) => writeln!(f, "{}return {}", indent_str, v),
                None => writeln!(f, "{}return", indent_str),
            },
            Instruction::Unary { op, src, dst, t } => {
                writeln!(f, "{}{}: {} = {} {}", indent_str, dst, t, op, src)
            }
            Instruction::Binary {
                op,
                src1,
                src2,
                dst,
                t,
            } => writeln!(
                f,
                "{}{} : {} = {} {} {}",
                indent_str, dst, t, src1, op, src2
            ),
            Instruction::Copy { src, dst, t } => {
                writeln!(f, "{}{} : {} = {}", indent_str, dst, t, src)
            }
            Instruction::FCall { name, args, dst } => {
                let args_str = args
                    .iter()
                    .map(|a| format!("{}", a))
                    .collect::<Vec<_>>()
                    .join(", ");
                if let Some(d) = dst {
                    writeln!(f, "{}{} = {}({})", indent_str, d, name, args_str)
                } else {
                    writeln!(f, "{}{}({})", indent_str, name, args_str)
                }
            }
            Instruction::Block { label, body } => {
                writeln!(f, "{}block {}: {{", indent_str, label)?;
                for instr in body {
                    instr.fmt_with_indent(f, indent + 4)?;
                }
                writeln!(f, "{}}}", indent_str)
            }
            Instruction::Loop { label, body } => {
                writeln!(f, "{}loop {}: {{", indent_str, label)?;
                for instr in body {
                    instr.fmt_with_indent(f, indent + 4)?;
                }
                writeln!(f, "{}}}", indent_str)
            }
            Instruction::Br(label) => writeln!(f, "{}br {}", indent_str, label),
            Instruction::BrIf { cond, l } => writeln!(f, "{}br {} {}", indent_str, cond, l),
        }
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_with_indent(f, 0)
    }
}

impl fmt::Display for TopLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TopLevel::Function {
                name,
                body,
                params,
                f_type,
                ..
            } => {
                if let WType::FunType {
                    param_type,
                    ret_type,
                } = f_type
                {
                    let params_str: Vec<String> = params
                        .iter()
                        .zip(param_type)
                        .map(|(p, p_t)| format!("{} : {}", p, p_t))
                        .collect();
                    match ret_type {
                        Some(t) => writeln!(f, "{}({}) -> {} {{", name, params_str.join(", "), t)?,
                        None => writeln!(f, "{}({}) {{", name, params_str.join(", "))?,
                    }

                    for instr in body {
                        instr.fmt_with_indent(f, 4)?;
                    }
                    write!(f, "}}")
                } else {
                    panic!("Internal Error: Function has non-function type")
                }
            }
            TopLevel::Data { name, t, v } => write!(f, "global {}: {} = {}", name, t, v),
        }
    }
}

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for decl in &self.decls {
            writeln!(f, "{}", decl)?;
        }
        Ok(())
    }
}
