//! WTAC IR generation from the typed AST.
//!
//! [`WtacGen`] lowers the type-checked AST into the WTAC intermediate
//! representation. This is the most involved compilation phase, so it is split
//! across four submodules. Each one owns a single design decision and documents
//! it in its own header:
//!
//! - `stmt.rs` — statements and control flow; *early returns*.
//! - `expr.rs` — literals, operators, calls, field access; *record values*.
//! - `array.rs` — allocation, initialization, concatenation; *array layout*.
//! - `builder.rs` — the `InstrBuilder` helper; *pointers*.
//!
//! This module keeps the [`WtacGen`] struct itself, the helpers that mint fresh
//! names and temporaries, the top-level walk (`generate`, `gen_top_level`,
//! `gen_function`), and the decision below.
//!
//! # Design decision: stack frames and multi-return
//!
//! Every function reserves a stack frame on entry and restores the stack pointer
//! from `rsp` on the way out. `FunData::offset` tracks how much of the frame is
//! spoken for: records and tuples live there, and the generator bumps the offset
//! by the value's byte size and hands out `Mem` values to reach into it.
//!
//! This is also how a function returns more than one value. When the return type
//! is a tuple or a record, the caller passes a *return pointer* as an extra first
//! argument and the callee writes its results through it; the WASM-level return
//! type becomes `void`. `make_ret_ptr` flattens record types to their base types
//! first, so the emitter deep-copies field by field instead of handing back a
//! pointer into a frame that is about to be torn down.

mod array;
mod builder;
mod expr;
mod stmt;

use tracing::{debug, instrument, warn};

use crate::symbols::MemberEntry;
use crate::typecheck::Typechecker;
use crate::types::Type;
use crate::wtac::wtac_gen::builder::InstrBuilder;
use crate::{ast, typecheck};
use crate::{types, util};

use crate::wtac::wtac_ast::{self as W};

/// Name of the synthetic return-value variable inserted into every function.
const RET_VAL_NAME: &str = "__ret_val";

/// Per-function state tracked during IR generation.
#[derive(Debug, Clone)]
struct FunData {
    /// Current byte offset into the stack frame (grows as locals are allocated).
    pub offset: usize,
    /// The restored stack pointer variable for this function (used to reset the stack on exit).
    pub rsp: W::Value,
    /// For multi-return functions, the pointer argument where results are written.
    pub ret_ptr: Option<W::Value>,
    /// Accumulated local variable declarations for the current function.
    pub locals: Vec<(String, W::WType)>,
}

impl FunData {
    pub fn new(rsp: W::Value, ret_ptr: Option<W::Value>) -> Self {
        FunData {
            offset: 0,
            rsp,
            ret_ptr,
            locals: Vec::new(),
        }
    }
}

/// Lowers the typed AST into WTAC IR.
///
/// Holds a reference to the [`Typechecker`] (and its [`SymbolTable`](crate::symbols::SymbolTable))
/// so that types can be queried during generation. Also tracks per-function state
/// in `FunData`.
pub struct WtacGen {
    // symtab: SymbolTable,
    pub tchecker: Typechecker,
    curr_f_data: FunData,
    // Local to this generator, so the same source always yields the same names.
    next_id: usize,
}

impl WtacGen {
    pub fn new(tchecker: Typechecker) -> Self {
        WtacGen {
            tchecker,
            curr_f_data: FunData::new(W::Value::Var("Dummy".to_string()), None),
            next_id: 0,
        }
    }

    /// Registers a local variable in the current function's data.
    fn update_locals(&mut self, name: &str, wt: W::WType) {
        self.curr_f_data.locals.push((name.to_string(), wt));
    }

    fn fresh(&mut self, prefix: &str) -> String {
        let n = self.next_id;
        self.next_id += 1;
        format!("{}.{}", prefix, n)
    }

    /// Paired labels of one construct share a number, which reconstruction
    /// relies on to tie a header to its container.
    fn fresh_label_pair(&mut self, name1: &str, name2: &str) -> (String, String) {
        let n = self.next_id;
        self.next_id += 1;
        (format!("{}.{}", name1, n), format!("{}.{}", name2, n))
    }

    /// Creates a fresh temporary variable, registers it as a local, and returns
    /// it as a [`Value::Var`](crate::wtac::wtac_ast::Value).
    fn make_wtac_var(&mut self, wt: W::WType) -> W::Value {
        let dst_name = self.fresh("tmp");
        self.update_locals(&dst_name, wt);
        W::Value::Var(dst_name)
    }

    /// Creates a fresh temporary holding an address.
    fn make_wtac_ptr(&mut self) -> W::Value {
        let dst_name = self.fresh("tmp");
        self.update_locals(&dst_name, W::WType::I32);
        W::Value::Var(dst_name)
    }

    /// Wraps a variable in a [`Value::Mem`](crate::wtac::wtac_ast::Value) with the given offset and type.
    /// This represents a memory access at `[name + offset]`.
    fn mem_of_wtac_var(&mut self, v: &W::Value, offset: usize, wt: &W::WType) -> W::Value {
        match v {
            W::Value::Var(name) => W::Value::Mem {
                name: name.clone(),
                offset,
                t: wt.clone(),
            },
            _ => panic!("Internal Error: Should only be used on variables"),
        }
    }

    /// Creates a return pointer variable for multi-return functions.
    /// Flattens record types into their base types so that the emitter
    /// can store each field individually.
    fn make_ret_ptr(&mut self, t: Type) -> String {
        let dst_name = self.fresh("ret_ptr");
        // To make it easier for emit we flatten Records to their base types recursively. This ensures when we store a point in the ret ptr we do this by deep copying.
        // This simplifies the logic

        let flattened_t = if let Type::Tuple { elems } = t {
            let mut new_t = Vec::new();
            for el in elems {
                if let Type::Record(tag) = el {
                    let members = &self.tchecker.symtab.get_record(&tag).members;
                    // We do not do this recursively. If it again contains a record it does not work. We only allow basic types for now!
                    // To allow self referential we could Box the value. If its just another record that contains only basic types we are fine!
                    // Record loops are handled by type system.
                    // This functoin is then recursive
                    for (_, MemberEntry { member_t, .. }) in members {
                        new_t.push(member_t.clone());
                    }
                } else {
                    new_t.push(el);
                }
            }
            new_t
        } else {
            panic!("Retun Pointer has to have Tuple Type")
        };
        self.tchecker
            .symtab
            .add_local_var(dst_name.clone(), Type::Tuple { elems: flattened_t });

        dst_name
    }

    /// Typechecks an expression and unwraps single-element tuples.
    fn type_and_unfold(&mut self, e: &ast::Expression) -> Type {
        let e_ty = self.tchecker.typecheck_expression(e).unwrap();
        match e_ty {
            Type::FunType { .. } => {
                panic!("Internal Error: Not possible that expression has function Type")
            }
            Type::Tuple { elems } if elems.len() != 1 => {
                panic!("Tuple Type only with 1 element allowed!")
            }
            Type::Tuple { elems } => elems[0].clone(),
            t => t,
        }
    }

    /// Entry point: generates WTAC for an entire program.
    ///
    /// Filters out record declarations (they are already in the symbol table)
    /// and appends the built-in `length` function.
    pub fn generate(&mut self, ast: ast::Program) -> W::Program {
        let mut wtac_decls: Vec<W::TopLevel> = ast
            .decls
            .iter()
            .filter(|dec| !matches!(dec, ast::Declaration::RecordDecl(_))) // not looking at records
            .map(|decl| self.gen_top_level(decl))
            .collect();

        let length_fun = self.add_length();
        wtac_decls.push(length_fun);
        W::Program { decls: wtac_decls }
    }

    /// Generates a top-level declaration (function or global data).
    fn gen_top_level(&mut self, decl: &ast::Declaration) -> W::TopLevel {
        match decl {
            ast::Declaration::FunDecl(fun) => self.gen_function(fun),
            ast::Declaration::RecordDecl(_) => {
                unreachable!("Record Declarations should be filtered out")
            } // in decl we dont need to do anything
            ast::Declaration::Global(ast::VarDeclaration {
                name,
                init,
                var_type,
                ..
            }) => {
                // Uninitialized is to 0
                let val = init
                    .as_ref()
                    .map_or(W::Value::Imm(0, convert_type(var_type)), |e| {
                        match &e.kind {
                            ast::ExpressionKind::Int(i) => W::Value::Imm(*i, W::WType::I64),
                            ast::ExpressionKind::Bool(true) => W::Value::Imm(1, W::WType::I32),
                            ast::ExpressionKind::Bool(false) => W::Value::Imm(0, W::WType::I32),
                            _ => {
                                panic!("Internal Error: A non-literal used to initialzie a global")
                            }
                        }
                    });
                let wt = match var_type {
                    Type::Int => W::WType::I64,
                    Type::Bool => W::WType::I32,
                    Type::Array { .. } => W::WType::I32,
                    Type::Record(_) => {
                        panic!("Intenral Error: Records are currently not allowed as Glibal Data")
                    }
                    _ => panic!("Internal Error: Functions and Unknown not allowed for Data!"),
                };
                W::TopLevel::Data {
                    name: name.clone(),
                    t: wt,
                    v: val,
                }
            }
        }
    }

    /// Generates the WTAC for a function definition.
    ///
    /// Sets up the stack frame (saves `__stack_pointer`, allocates space for
    /// records and local arrays), generates the body, and appends the exit
    /// block that restores the stack pointer and returns.
    ///
    /// For multi-return functions, the return type is rewritten: a return pointer
    /// is added as the first parameter and the function returns `void` instead.
    #[instrument(skip(self, fun), fields(name=format!("{} : {}", fun.name, fun.f_type)))]
    fn gen_function(&mut self, fun: &ast::Function) -> W::TopLevel {
        let rbp = self.make_wtac_ptr();
        let rsp = self.make_wtac_ptr();
        let stack_pointer = W::Value::Var("__stack_pointer".to_string());
        // If return type has multiple values, then we need to change the function type
        // Instead a Pointer is returned to the return values
        let (p_type, ret_type) = match &fun.f_type {
            Type::FunType {
                param_types,
                ret_type,
            } => (param_types, ret_type),
            _ => panic!("Internal Error: Function with non-function type"),
        };

        let mut w_p_t: Vec<W::WType> = p_type.iter().map(convert_type).collect();
        // Convert function if Tuple or Record is returned
        // Return a Pointer which is added as the first argument! Function then
        // returns nothing
        let mut ret_ptr_v = None;
        let (new_p_t, new_ret_t, new_params) = match &ret_type[..] {
            [] => (w_p_t, None, fun.params.clone()),
            [r_t] if !matches!(r_t, Type::Record(_)) => {
                (w_p_t, Some(convert_type(r_t)), fun.params.clone())
            }
            rs => {
                let ret_ptr = self.make_ret_ptr(Type::Tuple { elems: rs.to_vec() });
                ret_ptr_v = Some(W::Value::Var(ret_ptr.clone()));
                w_p_t.insert(0, W::WType::I32); // Pointer to front
                let mut new_params = fun.params.clone();
                new_params.insert(0, ret_ptr);
                (w_p_t, None, new_params)
            }
        };

        let w_f_type = W::WType::FunType {
            param_type: new_p_t,
            ret_type: new_ret_t.map(Box::new),
        };
        debug!("New Function Type: {}", w_f_type);
        // since f_data created here, we need to add rbp and rsp manually to locals
        let (rbp_name, rsp_name) = match (&rbp, &rsp) {
            (W::Value::Var(rn_name), W::Value::Var(rs_name)) => (rn_name.clone(), rs_name.clone()),
            _ => panic!("Not possible"),
        };
        self.curr_f_data = FunData {
            offset: 0,
            rsp: rsp.clone(),
            ret_ptr: ret_ptr_v,
            locals: vec![(rbp_name, W::WType::I32), (rsp_name, W::WType::I32)],
        };

        let body_instrs: Vec<W::Instruction> = fun
            .body
            .stmts
            .iter()
            .flat_map(|s| self.gen_statement(s))
            .collect();
        // Collect rsp and the offset needed for that function!
        // Decrement stack pointer by that offset
        let FunData {
            offset: stack_offset,
            ..
        } = self.curr_f_data;

        // Frame Setup
        let mut b = InstrBuilder::new(self);
        b.copy_to(stack_pointer.clone(), rbp.clone(), W::WType::I32);
        b.binary_to(
            W::BinaryOp::Sub,
            stack_pointer.clone(),
            b.i32_const(stack_offset as i64),
            stack_pointer.clone(),
            W::WType::I32,
        );
        b.copy_to(stack_pointer.clone(), rsp.clone(), W::WType::I32);

        let reset_stk_ptr = W::Instruction::Copy {
            src: rbp.clone(),
            dst: stack_pointer.clone(),
            t: W::WType::I32,
        };

        let frame_setup = b.finish();
        let frame_end = vec![reset_stk_ptr];

        // Add  return if there is not any in the end
        // Also add frame setup and end
        let mut new_body = Vec::new();

        for instr in body_instrs.into_iter() {
            match instr {
                r @ W::Instruction::Return(_) => {
                    new_body.extend(frame_end.clone());
                    new_body.push(r);
                }
                other => new_body.push(other),
            }
        }
        // Add a br instruction to this func exit body. This makes analysis easier and cleaner
        // We also add teh frame setupinto this block. Again to make analysis easier
        // Everything is in a block then except for frame_end whcih is fine
        // func_exit falls through the frame_end which may look weird in the analysis since it is two basic blocks in a straight line
        // Can later be optimized if no early return is possible
        new_body.push(W::Instruction::Br("func_exit".to_string()));
        new_body.splice(0..0, frame_setup);
        // Wrap all instructions in a block
        let new_fin_body = W::Instruction::Block {
            label: "func_exit".to_string(),
            body: new_body,
        };
        // add frame setup and frame end
        let mut fin_fun_body = Vec::new();
        // Add frame to end
        fin_fun_body.push(new_fin_body);
        fin_fun_body.extend(frame_end);

        // Check the original return type to know if we need to copy a return val
        let ret_val = W::Value::Var(RET_VAL_NAME.to_string()); // Name by convention!
        let ret_instruction = match &ret_type[..] {
            [] => W::Instruction::Return(None),
            [t] => {
                self.update_locals(RET_VAL_NAME, convert_type(t));
                W::Instruction::Return(Some(ret_val))
            }
            [_, _b @ ..] => {
                // A pointer to the return slot; Eta has no pointer type and it needs none.
                self.update_locals(RET_VAL_NAME, W::WType::I32);
                W::Instruction::Return(Some(ret_val))
            }
        };

        fin_fun_body.push(ret_instruction);

        W::TopLevel::Function {
            name: fun.name.clone(),
            body: fin_fun_body,
            params: new_params,
            f_type: w_f_type,
            locals: self.curr_f_data.locals.clone(),
        }
    }
}

/// Converts an ETA [`Type`] to a WASM-level [`WType`](crate::wtac::wtac_ast::WType).
///
/// Arrays and records become `I32` (pointers). Integers become `I64`.
/// Booleans become `I32`.
///
/// # Panics
///
/// Panics on `Type::FunType` and `Type::Unknown`, which should not appear
/// at this stage.
pub fn convert_type(t: &Type) -> W::WType {
    match t {
        Type::Int => W::WType::I64,
        Type::Bool | Type::Record(_) | Type::Array { .. } => W::WType::I32,
        Type::Unknown => panic!("Internal Error: Tried to convert Unknown Type"),
        Type::Tuple { .. } => W::WType::I32, // If tuple is encountered it was for ret_ptr or dst_ptr of function calls. To know how to destruct them. They act as pinters
        Type::FunType { .. } => panic!("Internal Error: Cannot convert Function Type"),
    }
}
