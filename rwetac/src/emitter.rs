//! WASM code emission.
//!
//! The [`Emitter`] translates the WTAC IR into WebAssembly Text format (`.wat`).
//! It uses the [`pretty`] crate to produce indented, human-readable output.
//!
//! The emitter handles the mapping from WTAC constructs to WASM instructions,
//! including the distinction between local and global variables
//! (`local.get`/`local.set` vs `global.get`/`global.set`) and the WASM linear
//! memory layout (stack, data section, heap).

use pretty::RcDoc;
use std::{
    collections::HashMap,
    io::{self, Write},
};
use tracing::{debug, info, instrument, warn};

use crate::wtac::wtac_ast::*;

/// Linear memory layout, following the one `wasm-ld` gives Rust:
///
/// ```text
///   [0 .. DATA_END)             static data
///   [DATA_END .. __heap_base)   shadow stack, __stack_pointer grows down
///   [__heap_base .. )           heap, grows up
/// ```
///
/// Nothing is placed in static data -- globals are WASM globals and arrays are
/// heap-allocated -- so `DATA_END` only marks where the stack bottoms out.
const DATA_END: usize = 100;
const STACK_SIZE: usize = 64 * 1024;

// NOTE: Invariant is that all names are unique. Which is done by resolver.

/// Tracks variable bindings and whether they are local or global in WASM.
///
/// The emitter needs this distinction because WASM uses separate instruction
/// families for local vs global access. Locals are cleared per function.
pub struct Bindings {
    pub locals: HashMap<String, WType>,
    globals: HashMap<String, WType>,
}

/// Whether a binding is local or global.
pub enum Attr {
    Local,
    Global,
}
impl Default for Bindings {
    fn default() -> Self {
        Self::new()
    }
}

impl Bindings {
    pub fn new() -> Self {
        Bindings {
            locals: HashMap::new(),
            globals: HashMap::new(),
        }
    }

    pub fn add_global(&mut self, k: &str, t: &WType) {
        self.globals.insert(k.to_string(), t.clone());
    }

    /// Replaces all local bindings with the given set.
    /// Called at the start of each function.
    pub fn add_locals<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (String, WType)>,
    {
        // Flushed first.
        self.locals.clear();
        self.locals.extend(iter);
    }

    pub fn get_attr(&self, name: &str) -> Attr {
        match self.globals.get(name) {
            Some(_) => Attr::Global,
            None => match self.locals.get(name) {
                Some(_) => Attr::Local,
                None => panic!(
                    "Internal Error: All bindings need to exist. Binding {} not found",
                    name
                ),
            },
        }
    }

    /// Returns the [`WType`] of a binding.
    ///
    /// Globals are checked first, then locals.
    ///
    /// # Panics
    ///
    /// Panics if the name is not found.
    pub fn get(&self, k: &str) -> WType {
        match self.globals.get(k) {
            Some(wt) => wt.clone(),
            None => match self.locals.get(k) {
                Some(wt) => wt.clone(),
                None => panic!(
                    "Internal Error: All bindings need to exist. Binding {} not found",
                    k
                ),
            },
        }
    }
}

/// Emits WASM Text format (`.wat`) from a WTAC [`Program`].
///
/// Writes to any [`io::Write`] destination (typically a buffered file writer).
///
/// # Usage
///
/// ```ignore
/// let file = fs::File::create("output.wat")?;
/// let mut emitter = Emitter::new(BufWriter::new(file));
/// emitter.emit(wtac_program)?;
/// ```
pub struct Emitter<W: io::Write> {
    out: W,
    pub bindings: Bindings,
}

impl<W: Write> Emitter<W> {
    pub fn new(out: W) -> Self {
        Emitter {
            out,
            bindings: Bindings::new(),
        }
    }

    /// Emits a complete WTAC program as a WASM module.
    ///
    /// Data declarations are emitted first (so globals are registered in
    /// [`Bindings`] before any function references them), followed by
    /// function definitions and exports.
    pub fn emit(&mut self, mut p: Program) -> io::Result<()> {
        // Handle all data first by sorting the declarations
        // Very important because we need all globals in bindings
        p.decls.sort_by_key(|k| match k {
            TopLevel::Function { .. } => 1,
            TopLevel::Data { .. } => 0,
        });

        // adds bindings for the compiler globals
        let compiler_globals = self.emit_compiler_globals(1000);
        let decl_docs: Vec<RcDoc> = p
            .decls
            .iter()
            .map(|decl| self.emit_top_level(decl))
            .collect();

        let mut export_docs = Vec::new();
        for decl in p.decls {
            if let TopLevel::Function { name, .. } = decl {
                export_docs.push(RcDoc::text(format!(
                    "(export \"{}\" (func ${}))",
                    name, name
                )))
            }
        }
        // Add our malloc that we later define with Wasmer
        let memory = RcDoc::text("(memory (export \"memory\") 17)");
        let import_malloc = RcDoc::text(
            "(import \"env\" \"eta_malloc\" (func $eta_malloc (param i32) (result i32)))",
        );
        let w_prog = RcDoc::text("(module")
            .append(RcDoc::hardline())
            .append(import_malloc)
            .append(RcDoc::hardline())
            .append(memory)
            .append(RcDoc::hardline())
            .append(RcDoc::intersperse(decl_docs, RcDoc::hardline()))
            .append(RcDoc::hardline())
            .append(compiler_globals)
            .append(RcDoc::hardline())
            .append(RcDoc::intersperse(export_docs, RcDoc::hardline()))
            .append(RcDoc::hardline())
            .append(")");

        w_prog.render(80, &mut self.out)
    }

    /// Emits the compiler-internal WASM globals: `__stack_pointer`, `__data_end`,
    /// and `__heap_base`.
    ///
    /// These define the WASM linear memory layout. `stk_offset` is added
    /// to a base value to determine where the stack begins.
    fn emit_compiler_globals(&mut self, stk_offset: usize) -> RcDoc<'static, ()> {
        // `__stack_pointer` starts *at* `__heap_base`: the stack grows down and
        // the heap grows up from one address, so neither can run into the other.
        // Basing the heap below the stack instead let a program that allocated
        // past the stack pointer silently overwrite its own frames.
        let heap_base = DATA_END + stk_offset + STACK_SIZE;
        self.bindings.add_global("__stack_pointer", &WType::I32);
        self.bindings.add_global("__data_end", &WType::I32);
        self.bindings.add_global("__heap_base", &WType::I32);
        let parts = [
            RcDoc::text(format!(
                "(global $__stack_pointer (mut i32) (i32.const {heap_base}) )"
            )),
            RcDoc::text(format!("(global $__data_end i32 (i32.const {DATA_END}) )")),
            RcDoc::text(format!(
                "(global $__heap_base (export \"__heap_base\") (mut i32) (i32.const {heap_base}) )"
            )),
        ];
        RcDoc::intersperse(parts, RcDoc::hardline())
    }

    /// Emits a single top-level declaration (function or global data).
    ///
    /// For functions, emits the signature (params + result), local declarations,
    /// and the body. Registers all locals and parameters in [`Bindings`] before
    /// emitting the body so that `emit_src`/`emit_dst` can resolve them.
    ///
    /// For data declarations, emits a WASM `(global ...)`.
    #[instrument(skip(self, tl))]
    fn emit_top_level(&mut self, tl: &TopLevel) -> RcDoc<'static, ()> {
        match tl {
            TopLevel::Function {
                name,
                body,
                params,
                f_type:
                    WType::FunType {
                        param_type,
                        ret_type,
                    },
                locals,
            } => {
                info!(Function = name, "Emitting");
                let mut header = RcDoc::text(format!("(func ${}", name));

                for (p, p_t) in params.iter().zip(param_type) {
                    header = header
                        .append(RcDoc::space())
                        .append(RcDoc::text(format!("(param ${}", p)))
                        .append(RcDoc::space())
                        .append(Self::emit_wtype(p_t))
                        .append(")");
                }
                // adds new line
                if let Some(r_t) = ret_type {
                    header = header
                        .append(RcDoc::space())
                        .append(RcDoc::text("(result"))
                        .append(RcDoc::space())
                        .append(Self::emit_wtype(r_t))
                        .append(")")
                        .append(RcDoc::hardline())
                };

                let local_docs: Vec<RcDoc> = locals
                    .iter()
                    .map(|(name, wt)| {
                        RcDoc::text(format!("(local ${}", name))
                            .append(RcDoc::space())
                            .append(Self::emit_wtype(wt))
                            .append(")")
                    })
                    .collect();
                // Update locals with local and function args
                let local_iter = locals
                    .clone()
                    .into_iter()
                    .chain(params.iter().cloned().zip(param_type.iter().cloned()));
                self.bindings.add_locals(local_iter);

                let body_docs: Vec<RcDoc> = body
                    .iter()
                    .map(|instr| self.emit_instruction(instr))
                    .collect();
                header
                    .append(RcDoc::intersperse(local_docs, RcDoc::hardline()))
                    .append(RcDoc::hardline())
                    .append(RcDoc::intersperse(body_docs, RcDoc::hardline()))
                    .nest(4)
                    .append(RcDoc::hardline())
                    .append(")")
                    .append(RcDoc::hardline())
            }
            TopLevel::Data { name, t, v } => {
                self.bindings.add_global(name, t);
                RcDoc::text("(global $")
                    .append(name.clone())
                    .append(RcDoc::space())
                    .append("(mut")
                    .append(RcDoc::space())
                    .append(Self::emit_wtype(t))
                    .append(")")
                    .append(RcDoc::space())
                    .append("(")
                    .append(self.emit_src(v))
                    .append("))")
            }
            _ => panic!("Internal Error: Function with non- function type"),
        }
    }

    /// Emits a single WTAC instruction as one or more WASM instructions.
    ///
    /// This is the core of the emitter. Each WTAC instruction maps to a
    /// sequence of WASM stack operations. The [`Copy`](Instruction::Copy)
    /// instruction is the most involved because it handles three cases:
    /// store (dst is `Mem`), load (src is `Mem`), and simple move.
    // #[instrument(skip(self, instr))]
    pub fn emit_instruction(&mut self, instr: &Instruction) -> RcDoc<'static, ()> {
        debug!(instr=%instr, "Processing Instruction");
        match instr {
            Instruction::Return(val) => match val {
                Some(v) => self.emit_src(v).append(RcDoc::hardline()).append("return"),
                None => RcDoc::text("return"),
            },
            Instruction::Copy {
                src,
                dst,
                t: copy_t,
            } => {
                // store expects the store address first and then the value. For load it is different
                // So we flip the order
                match dst {
                    // The type info is helpful here
                    Value::Mem { name, offset, t } => {
                        if t != copy_t {
                            panic!("Copy type {} and type in mem {} different.", copy_t, t)
                        }
                        let addr_doc = self.emit_addr_for_mem(name, *offset);
                        // Need to do that manually and then emit store after src
                        let parts = [
                            addr_doc,
                            self.emit_src(src),
                            Self::emit_wtype(t).append(".store"),
                        ];
                        RcDoc::intersperse(parts, RcDoc::hardline())
                    }
                    // Load and others are simpler
                    _ => self
                        .emit_src(src)
                        .append(RcDoc::hardline())
                        .append(self.emit_dst(dst)),
                }
            }
            // We used wrap to cast offset computations and to make that clear. Only place where this is used
            Instruction::Unary { op, src, dst, .. } => {
                let ops = match op {
                    UnaryOp::Neg => vec![
                        RcDoc::text("i64.const 0"),
                        self.emit_src(src),
                        self.emit_unary(op),
                        self.emit_dst(dst),
                    ],

                    UnaryOp::Not | UnaryOp::Wrap => {
                        vec![self.emit_src(src), self.emit_unary(op), self.emit_dst(dst)]
                    }
                };
                RcDoc::intersperse(ops, RcDoc::hardline())
            }
            Instruction::Binary {
                op,
                src1,
                src2,
                dst,
                ..
            } => {
                // Type t is the type of the dst value
                // Not the type of the operation
                // a :i32 = 8:i64 < 7:i64. Here < should be i64!
                // I am not a super fan of that lookup and that conversion but its fine
                let op_t = match src1 {
                    Value::Var(n) => &self.bindings.get(n),

                    Value::Imm(_, wt) => wt,
                    Value::Mem { .. } => panic!(
                        "Internal Error: Binary Operation should not have a Mem component! Did you 
                    use gen and convert expressoin correctly in generatipn?"
                    ),
                };
                let ops = vec![
                    self.emit_src(src1),
                    self.emit_src(src2),
                    self.emit_binary(op, op_t),
                    self.emit_dst(dst),
                ];
                RcDoc::intersperse(ops, RcDoc::hardline())
            }
            Instruction::FCall { name, args, dst } => {
                let args_doc =
                    RcDoc::intersperse(args.iter().map(|a| self.emit_src(a)), RcDoc::hardline());
                let call_doc = args_doc
                    .append(RcDoc::hardline())
                    .append(RcDoc::text(format!("call ${}", name)));
                match dst {
                    Some(d) => call_doc.append(RcDoc::hardline()).append(self.emit_dst(d)),
                    None => call_doc,
                }
            }
            Instruction::Block { label, body } => {
                let body_docs: Vec<RcDoc<()>> =
                    body.iter().map(|i| self.emit_instruction(i)).collect();
                RcDoc::text(format!("(block ${}", label))
                    .append(RcDoc::hardline())
                    .append(RcDoc::intersperse(body_docs, RcDoc::hardline()))
                    .nest(2)
                    .append(RcDoc::hardline())
                    .append(RcDoc::text(")"))
            }
            Instruction::Loop { label, body } => {
                let body_docs: Vec<RcDoc<()>> =
                    body.iter().map(|i| self.emit_instruction(i)).collect();
                RcDoc::text(format!("(loop ${}", label))
                    .append(RcDoc::hardline())
                    .append(RcDoc::intersperse(body_docs, RcDoc::hardline()))
                    .nest(2)
                    .append(RcDoc::hardline())
                    .append(RcDoc::text(")"))
            }
            Instruction::Br(lbl) => RcDoc::text("br")
                .append(RcDoc::space())
                .append(format!("${}", lbl)),
            Instruction::BrIf { cond, l } => self
                .emit_src(cond)
                .append(RcDoc::hardline())
                .append("br_if")
                .append(RcDoc::space())
                .append(format!("${}", l)),
        }
    }

    /// Emits the WASM instructions to compute a memory address.
    ///
    /// Loads the base pointer (local or global), pushes the byte offset,
    /// and adds them. The result is left on the WASM stack, ready for
    /// a subsequent `.load` or `.store`.
    fn emit_addr_for_mem(&self, name: &str, offset: usize) -> RcDoc<'static, ()> {
        let sel = match self.bindings.get_attr(name) {
            Attr::Local => "local",
            Attr::Global => "global", // if not in local set. Than it has to be global variable
        };
        let parts = [
            RcDoc::text(format!("{}.get ${}", sel, name)),
            RcDoc::text("i32.const ").append(RcDoc::as_string(offset)),
            RcDoc::text("i32.add"),
        ];
        RcDoc::intersperse(parts, RcDoc::hardline())
    }

    /// Emits a value as a destination (store target).
    ///
    /// For variables, emits `local.set` or `global.set`.
    /// `Mem` destinations are handled directly in [`emit_instruction`](Self::emit_instruction)
    /// because WASM stores require the address to be pushed *before* the value.
    ///
    /// # Panics
    ///
    /// Panics on `Imm` (immediates cannot be destinations) and `Mem`
    /// (must be handled in the `Copy` instruction).
    fn emit_dst(&mut self, v: &Value) -> RcDoc<'static, ()> {
        match v {
            Value::Var(name) => {
                let sel = match self.bindings.get_attr(name) {
                    Attr::Local => RcDoc::text("local"),
                    Attr::Global => RcDoc::text("global"), // if not in local set. Than it has to be global variable
                };
                sel.append(".set ").append(format!("${}", name.clone()))
            }
            Value::Imm(_, _) => panic!("Internal Error: Immediate cannot be used as Destination"),
            Value::Mem { .. } => panic!("Memory should be handled in Copy because of Ordering"),
        }
    }

    /// Emits a value as a source (pushes it onto the WASM stack).
    ///
    /// For immediates, emits `<type>.const <value>`.
    /// For variables, emits `local.get` or `global.get`.
    /// For memory values, emits an address computation followed by a `.load`.
    fn emit_src(&mut self, v: &Value) -> RcDoc<'static, ()> {
        match v {
            Value::Imm(i, wt) => Self::emit_wtype(wt)
                .append(".const ")
                .append(RcDoc::as_string(i)),
            Value::Var(name) => {
                let sel = match self.bindings.get_attr(name) {
                    Attr::Local => RcDoc::text("local"),
                    Attr::Global => RcDoc::text("global"), // if not in local set. Than it has to be global variable
                };
                sel.append(".get ").append(format!("${}", name.clone()))
            }
            Value::Mem { name, offset, t } => {
                debug!("Mem case of emit_src");
                let addr_emit = self.emit_addr_for_mem(name, *offset);
                // Offset Calculation
                let parts = [addr_emit, Self::emit_wtype(t).append(".load")];
                RcDoc::intersperse(parts, RcDoc::hardline())
            }
        }
    }

    /// Emits a [`WType`] as its WASM text representation (`i32` or `i64`).
    ///
    /// # Panics
    ///
    /// Panics on [`WType::FunType`], which has no direct WASM type representation.
    fn emit_wtype(wt: &WType) -> RcDoc<'static, ()> {
        match wt {
            WType::I32 => RcDoc::text("i32"),
            WType::I64 => RcDoc::text("i64"),
            WType::FunType { .. } => {
                panic!("Internal Error: Other types do not have a representation")
            }
        }
    }

    /// Emits a binary operator as a WASM instruction (e.g. `i64.add`, `i32.lt_s`).
    ///
    /// The type `t` determines the prefix (`i32` or `i64`). Signed variants
    /// are used for division, modulo, and comparisons (`div_s`, `rem_s`, `lt_s`, etc.).
    fn emit_binary(&mut self, op: &BinaryOp, t: &WType) -> RcDoc<'static, ()> {
        let op_str = match op {
            BinaryOp::Add => "add",
            BinaryOp::Sub => "sub",
            BinaryOp::Mult => "mul",
            BinaryOp::Div => "div_s",
            BinaryOp::Mod => "rem_s",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
            BinaryOp::Lt => "lt_s",
            BinaryOp::Leq => "le_s",
            BinaryOp::Gt => "gt_s",
            BinaryOp::Geq => "ge_s",
            BinaryOp::Eq => "eq",
            BinaryOp::Neq => "ne",
        };
        Self::emit_wtype(t).append(".").append(op_str)
    }

    /// Emits a unary operator as a WASM instruction.
    ///
    /// Negation is emitted as `i64.const 0` followed by `i64.sub` (since WASM
    /// has no dedicated negate instruction). Not is `i32.eqz`, and wrap is
    /// `i32.wrap_i64` for narrowing 64-bit values to 32-bit.
    pub fn emit_unary(&mut self, op: &UnaryOp) -> RcDoc<'static, ()> {
        match op {
            UnaryOp::Neg => RcDoc::text("i64.sub"),
            UnaryOp::Not => RcDoc::text("i32.eqz"),
            UnaryOp::Wrap => RcDoc::text("i32.wrap_i64"),
        }
    }

    //     if let types::Type::Record(tag) = t {
    //         self.symtab
    //             .get_record(&tag)
    //             .find_at_offset(offset)
    //             .map(|entry| entry.member_t.clone())
    //     } else {
    //         panic!("Internal Error: SHould only be called on Tuple Types!")
    //     }
    // }
}
