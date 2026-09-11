//! Expression lowering: literals, operators, calls, subscripts and field access.
//!
//! # Design decision: record values
//!
//! A record is never a value in the WASM sense — it is an address into the
//! current stack frame. Construction bumps the frame offset and copies each
//! member into place; `Dot` resolves a field to a `Mem` at that member's offset.
//!
//! Because a `Mem` is a place and not a value, `gen_and_convert_expression`
//! loads it into a temporary for every context except the left-hand side of an
//! assignment, where the store needs the address itself. That is the whole
//! reason two entry points exist: `gen_expression` yields the place,
//! `gen_and_convert_expression` yields something you can compute with.
//!
//! Returning a record deep-copies it into the caller's frame; see the module
//! header in `mod.rs`.

use super::builder::InstrBuilder;
use super::*;
impl WtacGen {
    /// Generates an expression and, if the result is a `Mem`, inserts a load
    /// to materialize the value into a temporary.
    ///
    /// Use this for most contexts. Use [`gen_expression`](Self::gen_expression)
    /// directly only on the LHS of an assignment, where a `Mem` should remain
    /// as a store destination.
    pub(super) fn gen_and_convert_expression(
        &mut self,
        e: &ast::Expression,
    ) -> (Vec<W::Instruction>, W::Value) {
        let (instrs, res) = self.gen_expression(e);
        let e_ty = self.tchecker.typecheck_expression(e).unwrap();

        let t = match e_ty {
            Type::Tuple { .. } => Type::Bool, // For Pointer Load
            Type::FunType { .. } => panic!("Internal Error: Expression with Function type!"),
            e_t => e_t,
        };
        match res {
            W::Value::Mem { .. } => {
                let mut b = InstrBuilder::new(self);
                b.extend(instrs);
                let dst = b.copy(res, convert_type(&t));

                (b.finish(), dst)
            }
            _ => (instrs, res),
        }
    }

    /// Generates an expression, returning instructions and a result value.
    ///
    /// Literals produce immediates, variables produce `Value::Var`, subscripts
    /// and field accesses produce `Value::Mem` (which is a *load* if used as
    /// a source, or a *store* if used as a destination).
    ///
    /// One method per non-trivial kind; the interesting decisions are documented
    /// on those.
    pub(super) fn gen_expression(
        &mut self,
        e: &ast::Expression,
    ) -> (Vec<W::Instruction>, W::Value) {
        match &e.kind {
            ast::ExpressionKind::Int(i) => (vec![], W::Value::Imm(*i, W::WType::I64)),
            ast::ExpressionKind::Bool(true) => (vec![], W::Value::Imm(1, W::WType::I32)),
            ast::ExpressionKind::Bool(false) => (vec![], W::Value::Imm(0, W::WType::I32)),
            ast::ExpressionKind::Var(name) => (vec![], W::Value::Var(name.clone())),
            ast::ExpressionKind::ArrayLit(es) => self.gen_array_lit(es),
            ast::ExpressionKind::Binary(bin_op, e1, e2) => self.gen_binary(bin_op, e1, e2),
            ast::ExpressionKind::Unary(unary_op, e) => self.gen_unary(unary_op, e),
            ast::ExpressionKind::Call { name, args }
                if matches!(self.tchecker.symtab.get(name).t, Type::FunType { .. }) =>
            {
                self.gen_call(name, args)
            }
            // For Record initilaiztion like Point(1,2)
            ast::ExpressionKind::Call { name, args } => self.gen_record_lit(name, args),
            ast::ExpressionKind::Subscript(e, e_idx) => self.gen_subscript(e, e_idx),
            ast::ExpressionKind::Dot(e, field) => self.gen_dot(e, field),
        }
    }

    /// Lowers an array literal: allocate an array of the literal's length, then
    /// copy each element into its slot.
    fn gen_array_lit(&mut self, es: &[ast::Expression]) -> (Vec<W::Instruction>, W::Value) {
        let es_ty: Vec<Type> = es
            .iter()
            .map(|e| self.tchecker.typecheck_expression(e))
            .collect::<Result<_, _>>()
            .unwrap();

        let elem_ty = match es_ty.as_slice() {
            [] => Type::Int, // Happens for {}. The Unknown Type
            [first, ..] => first.clone(),
        };
        let arr_len = es.len();
        let (arr_instr, arr_ptr) = self.create_simple_array(arr_len, &elem_ty);

        let mut b = InstrBuilder::new(self);
        b.extend(arr_instr);
        for (i, e) in es.iter().enumerate() {
            let (e_instr, e_res) = b.gene.gen_and_convert_expression(e);

            b.extend(e_instr);
            let offset = i * types::get_size(&elem_ty);
            // add offset to memory arr_ptr
            let mem_offset_addr = b
                .gene
                .mem_of_wtac_var(&arr_ptr, offset, &convert_type(&elem_ty));
            b.copy_to(e_res, mem_offset_addr, convert_type(&elem_ty));
        }
        (b.finish(), arr_ptr)
    }

    /// Lowers a call to a function. A multi-value or record return gets a return
    /// pointer into the current frame as an extra first argument.
    fn gen_call(
        &mut self,
        name: &str,
        args: &[ast::Expression],
    ) -> (Vec<W::Instruction>, W::Value) {
        let (arg_instrs, mut arg_vals): (Vec<_>, Vec<_>) = args
            .iter()
            .map(|e| self.gen_and_convert_expression(e))
            .unzip();
        let mut arg_instrs = arg_instrs.concat(); //flatten
        let f_ret_type = match &self.tchecker.symtab.get(name).t {
            Type::FunType { ret_type, .. } => ret_type.clone(),
            _ => panic!("Internal Error: Function has non Function type"),
        };

        // Records need to be handled like Tuples since they live on the stack. So when callee wants to return a tuple we need to deep copy
        // it on caller site
        // Helper to decide if the return value fits in a register.
        // It's a single return value AND it's not a record.
        let is_simple_return =
            f_ret_type.len() == 1 && !matches!(f_ret_type[0], Type::Record { .. });

        let (call_instrs, res_v) = if is_simple_return {
            let ret_type = f_ret_type[0].clone();
            let dst = self.make_wtac_var(convert_type(&ret_type));
            (
                vec![W::Instruction::FCall {
                    name: name.to_string(),
                    args: arg_vals,
                    dst: Some(dst.clone()),
                }],
                dst,
            )
        } else {
            // Unified Logic for Tuples and Record returns. Records are deep copied
            // This is called return value optimization (RVO)
            // Calculate where to put the return value on our (the caller's) stack.
            let FunData {
                offset: stack_offset,
                rsp,
                ..
            } = self.curr_f_data.clone();
            let offset_ptr = self.make_wtac_ptr();

            let flatten_ret_type = Type::Tuple {
                elems: f_ret_type.clone(),
            };
            debug!("Return type of Dst Pointer in Call: {}", flatten_ret_type);
            let dst_ptr = self.make_wtac_var(convert_type(&flatten_ret_type));
            let rsp_add = W::Instruction::Binary {
                op: W::BinaryOp::Add,
                src1: rsp,
                src2: W::Value::Imm(stack_offset as i64, W::WType::I32),
                dst: offset_ptr.clone(),
                t: W::WType::I32,
            };
            let copy_rsp = W::Instruction::Copy {
                src: offset_ptr,
                dst: dst_ptr.clone(),
                t: W::WType::I32,
            };

            let ret_size = f_ret_type.iter().fold(0, |acc, t| {
                // A record deeep copy. So we need all values
                if let Type::Record(rec) = t {
                    let rec_size = self.tchecker.symtab.get_record(rec).size();
                    acc + rec_size
                } else {
                    acc + types::get_size(t)
                }
            });
            self.curr_f_data.offset += util::round_to_n(ret_size as isize, 16) as usize;

            arg_vals.insert(0, dst_ptr.clone());
            let call_instr = W::Instruction::FCall {
                name: name.to_string(),
                args: arg_vals,
                dst: None,
            };
            // Unpacking of result is handled by assignment
            (vec![rsp_add, copy_rsp, call_instr], dst_ptr)
        };
        arg_instrs.extend(call_instrs);
        (arg_instrs, res_v)
    }

    /// Lowers record construction such as `Point(1, 2)`. Syntactically a call, but
    /// it reserves frame space and copies each argument to its member offset.
    fn gen_record_lit(
        &mut self,
        name: &str,
        args: &[ast::Expression],
    ) -> (Vec<W::Instruction>, W::Value) {
        let (arg_instrs, arg_vals): (Vec<_>, Vec<_>) = args
            .iter()
            .map(|e| self.gen_and_convert_expression(e))
            .unzip();
        let arg_instrs = arg_instrs.concat(); //flatten
        let members = &self.tchecker.symtab.get_record(name).members.clone();

        let f_ret_type = self.tchecker.symtab.get(name).t.clone();
        // When a record is returned. We need to deep copy it. Since it is stored on the stack.
        // We do this manually.
        // This also means the size of return is different because in a return the record type has the size of all of its members!
        // Then a new pointer is created at call site.

        // Compute location for storing the record
        let FunData {
            offset: stack_offset,
            rsp,
            ..
        } = self.curr_f_data.clone();

        let mut b = InstrBuilder::new(self);
        b.extend(arg_instrs);

        let offset_ptr = b.binary(
            W::BinaryOp::Add,
            rsp,
            b.i32_const(stack_offset as i64),
            convert_type(&f_ret_type),
        );
        let dst_ptr = b.copy(offset_ptr, convert_type(&f_ret_type));

        // Create a store for each member using the arg values Sort in increasing offset order!
        for ((_, MemberEntry { member_t, offset }), arg_val) in members.iter().zip(arg_vals) {
            let mem_dst = b
                .gene
                .mem_of_wtac_var(&dst_ptr, *offset, &convert_type(member_t));
            b.copy_to(arg_val, mem_dst, convert_type(member_t));
        }
        let instrs = b.finish();
        // Get space needed for members
        // Size is the offset of last entry + size of type of last entry
        let rec_size = self.tchecker.symtab.get_record(name).size();

        self.curr_f_data.offset += util::round_to_n(rec_size as isize, 16) as usize;
        (instrs, dst_ptr)
    }

    /// Lowers indexing to a `Mem` at `base + index * elem_size`, left as a place
    /// so it can serve as either a load or a store.
    fn gen_subscript(
        &mut self,
        e: &ast::Expression,
        e_idx: &ast::Expression,
    ) -> (Vec<W::Instruction>, W::Value) {
        // This returns a Memory location
        let (e_instr, e_v) = self.gen_and_convert_expression(e);
        let (e_idx_instr, e_idx_v) = self.gen_and_convert_expression(e_idx);

        let elem_t =
            if let Type::Array { elem_type } = self.tchecker.typecheck_expression(e).unwrap() {
                elem_type
            } else {
                panic!("Internal Error: This should have array type")
            };

        let mut b = InstrBuilder::new(self);
        b.extend(e_idx_instr);
        b.extend(e_instr);
        // Helper for base + (index * elem_size)
        let fin_arr_ptr = b.ptr_offset(&e_v, &e_idx_v, &elem_t);

        let mem_dst = b
            .gene
            .mem_of_wtac_var(&fin_arr_ptr, 0, &convert_type(&elem_t));
        (b.finish(), mem_dst)
    }

    /// Lowers field access to a `Mem` at the member's offset. Uses
    /// [`Self::gen_expression`] so a nested `Dot` stays a place.
    fn gen_dot(&mut self, e: &ast::Expression, field: &str) -> (Vec<W::Instruction>, W::Value) {
        let (e_instr, e_v) = self.gen_expression(e);

        let member = match self.tchecker.typecheck_expression(e).unwrap() {
            Type::Record(rec) => {
                let rec_entry = self.tchecker.symtab.get_record(&rec);
                rec_entry.find(field).unwrap()
            }
            _ => panic!("Internal Error: SHould be Record"),
        };

        // Specify size of load here
        let mem_dst = self.mem_of_wtac_var(&e_v, member.offset, &convert_type(&member.member_t));
        (e_instr, mem_dst)
    }

    /// Generates a binary expression.
    ///
    /// Array concatenation (`+` on arrays) is handled as a special case:
    /// a new array is allocated with the combined length, then two loops
    /// copy elements from the left and right operands.
    /// All other binary operations produce a single `Binary` instruction.
    fn gen_binary(
        &mut self,
        op: &ast::BinOp,
        e1: &ast::Expression,
        e2: &ast::Expression,
    ) -> (Vec<W::Instruction>, W::Value) {
        let e_ty = self.type_and_unfold(e1);
        if op == &ast::BinOp::Add && typecheck::is_arr(&e_ty) {
            return self.gen_array_concat(e1, e2, &e_ty);
        }
        let (e1_instr, v_e1) = self.gen_and_convert_expression(e1);
        let (e2_instr, v_e2) = self.gen_and_convert_expression(e2);
        let w_op = self.convert_binop(op);
        let res_ty = match op {
            ast::BinOp::Eq
            | ast::BinOp::Neq
            | ast::BinOp::Lt
            | ast::BinOp::Leq
            | ast::BinOp::Gt
            | ast::BinOp::Geq => Type::Bool,
            _ => e_ty.clone(),
        };
        let dst = self.make_wtac_var(convert_type(&res_ty));
        let w_bin_instr = W::Instruction::Binary {
            op: w_op,
            src1: v_e1,
            src2: v_e2,
            dst: dst.clone(),
            t: convert_type(&res_ty),
        };
        let mut instrs = Vec::new();
        instrs.extend(e1_instr);
        instrs.extend(e2_instr);
        instrs.push(w_bin_instr);
        (instrs, dst)
    }

    /// Generates a unary expression (`-x`, `!cond`).
    fn gen_unary(
        &mut self,
        op: &ast::UnaryOp,
        e: &ast::Expression,
    ) -> (Vec<W::Instruction>, W::Value) {
        let (e_instr, v_e) = self.gen_and_convert_expression(e);
        let e_ty = self.tchecker.typecheck_expression(e).unwrap();

        let dst = self.make_wtac_var(convert_type(&e_ty));
        let w_op = self.convert_unop(op);
        let w_un_instr = W::Instruction::Unary {
            op: w_op,
            src: v_e,
            dst: dst.clone(),
            t: convert_type(&e_ty),
        };
        let mut instrs = Vec::new();
        instrs.extend(e_instr);
        instrs.push(w_un_instr);
        (instrs, dst)
    }

    /// Converts an AST unary operator to its WTAC equivalent.
    fn convert_unop(&self, op: &ast::UnaryOp) -> W::UnaryOp {
        match op {
            ast::UnaryOp::Not => W::UnaryOp::Not,
            ast::UnaryOp::Neg => W::UnaryOp::Neg,
        }
    }

    /// Converts an AST binary operator to its WTAC equivalent.
    fn convert_binop(&self, op: &ast::BinOp) -> W::BinaryOp {
        match op {
            ast::BinOp::Add => W::BinaryOp::Add,
            ast::BinOp::Sub => W::BinaryOp::Sub,
            ast::BinOp::Mult => W::BinaryOp::Mult,
            ast::BinOp::Div => W::BinaryOp::Div,
            ast::BinOp::Mod => W::BinaryOp::Mod,
            ast::BinOp::And => W::BinaryOp::And,
            ast::BinOp::Or => W::BinaryOp::Or,
            ast::BinOp::Lt => W::BinaryOp::Lt,
            ast::BinOp::Leq => W::BinaryOp::Leq,
            ast::BinOp::Gt => W::BinaryOp::Gt,
            ast::BinOp::Geq => W::BinaryOp::Geq,
            ast::BinOp::Eq => W::BinaryOp::Eq,
            ast::BinOp::Neq => W::BinaryOp::Neq,
        }
    }
}
