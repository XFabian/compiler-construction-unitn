//! Statement lowering: control flow, local declarations and assignment.
//!
//! # Design decision: early returns
//!
//! Every `return` compiles to a copy into the synthetic `__ret_val` variable
//! followed by `br func_exit`, and the function body ends with a single
//! `return __ret_val`. However many `return` statements the source had, the
//! generated function has exactly one exit.
//!
//! This falls out of WASM having no arbitrary jump: a `return` from inside
//! nested blocks has to become a branch to an enclosing label, and funnelling
//! every one of them through the same label keeps stack-frame teardown in a
//! single place rather than duplicated at each exit.
//!
//! Multi-value returns copy through the return pointer instead of `__ret_val`;
//! see the module header in `mod.rs`.

use crate::wtac::wtac_gen::builder::InstrBuilder;

use super::*;

impl WtacGen {
    /// Generates WTAC instructions for a statement.
    ///
    /// One method per statement kind; the interesting decisions are documented
    /// on those.
    #[instrument(skip(self), level = "Debug")]
    pub(super) fn gen_statement(&mut self, stmt: &ast::Statement) -> Vec<W::Instruction> {
        match &stmt.kind {
            ast::StatementKind::Return(es) => self.gen_return(es),
            ast::StatementKind::LocalDecl(vd) if vd.dims.is_empty() => self.gen_local_decl(vd),
            ast::StatementKind::LocalDecl(vd) => self.gen_local_array_decl(vd),
            ast::StatementKind::Assign { lhs, rhs } => self.gen_assign(lhs, rhs),
            ast::StatementKind::Compound(block) => block
                .stmts
                .iter()
                .flat_map(|s| self.gen_statement(s))
                .collect(),
            ast::StatementKind::If {
                guard,
                then_br,
                else_br,
            } => self.gen_if(guard, then_br, else_br),
            ast::StatementKind::While { guard, body } => self.gen_while(guard, body),
            ast::StatementKind::Procedure { name, args } => self.gen_procedure_call(name, args),
        }
    }

    /// Lowers `return`.
    ///
    /// Copies the value into `__ret_val` and branches to `func_exit`. A
    /// multi-value return instead copies each value into the return pointer at
    /// successive offsets.
    fn gen_return(&mut self, es: &[ast::Expression]) -> Vec<W::Instruction> {
        let es_ty: Vec<_> = es
            .iter()
            .map(|e| self.tchecker.typecheck_expression(e))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        debug!("In return: {:?}", es_ty);
        let es_ty_unfolded = Typechecker::unfold_tuple_types(es_ty);
        // If return is a functoin call, it contains a tuple type. Evefn if only one value
        let is_simple_return =
            es_ty_unfolded.len() == 1 && !matches!(es_ty_unfolded[0], Type::Record { .. });

        if es_ty_unfolded.is_empty() {
            // A bare `return` in a procedure: nothing to hand back, just leave.
            vec![W::Instruction::Br("func_exit".to_string())]
        } else if is_simple_return {
            let (mut instrs, res) = self.gen_and_convert_expression(&es[0]);
            instrs.push(W::Instruction::Copy {
                src: res,
                dst: W::Value::Var(RET_VAL_NAME.to_string()),
                t: convert_type(&es_ty_unfolded[0]),
            });
            instrs.push(W::Instruction::Br("func_exit".to_string()));
            instrs
        } else {
            // Also for records!
            // For multi, return store result on RSP. This will then be the first arg of that function
            let mut instrs = Vec::new();
            let store_ptr = match &self.curr_f_data.ret_ptr {
                Some(r_p) => r_p.clone(),
                None => panic!(
                    "Internal Error: Function with multi return should have a return pointer argument!"
                ),
            };
            let mut offset = 0;
            for e in es {
                let (instr_e, res_e) = self.gen_and_convert_expression(e);
                let e_ty = self.tchecker.typecheck_expression(e).unwrap();
                // BEcause of function calls here we generalize
                // Functoin calls always return a tuple! SO we check what we copy
                // If its tuple with elems >= 2 or record its a pointer and the call site handles it later
                // Otherwise its simple so we just copy the value
                // Size we compute here
                // debug!("Type of expression in Return {:?}", e_ty);
                instrs.extend(instr_e);

                let mut handle_record = |tag: &String| {
                    // Record returned. We need to deep copy all fields of records into our return pointer

                    // res_e is the local src pointer to the record
                    // We deep copy that now
                    let src_ptr = res_e.clone();
                    // Cloning annoying
                    let rec = self.tchecker.symtab.get_record(tag).clone();
                    let members = &rec.members;
                    // The copy is field by field, so the destination has to
                    // use the same offsets as the source. Walking it with
                    // get_size instead would drop the padding.
                    let base = offset;

                    for (
                        _,
                        MemberEntry {
                            member_t,
                            offset: mem_offset,
                        },
                    ) in members
                    {
                        let wt = convert_type(member_t);
                        let cpy_instr = W::Instruction::Copy {
                            src: self.mem_of_wtac_var(&src_ptr, *mem_offset, &wt),
                            dst: self.mem_of_wtac_var(&store_ptr, base + *mem_offset, &wt),
                            t: wt,
                        };
                        instrs.push(cpy_instr);
                    }
                    offset = base + rec.size();
                };
                // Records are typechecked as functions. So return type is Type::Tuple(Type::Record).
                // We need to deep copy when tuple contains record or when the tuple has more than one element
                // If a value is used directly it is different
                // If the type is only record we deep copy. Otherwise we do not need to do anything
                match e_ty {
                    // like double() : int, int called in return double(). We need to deep copy the values here
                    Type::Tuple { elems } if elems.len() >= 2 => {
                        let src_ptr = res_e.clone(); // previous ret_ptr
                        let mut src_offset = 0; // 
                        for elem in elems {
                            let wt = convert_type(&elem);
                            let cpy_instr = W::Instruction::Copy {
                                src: self.mem_of_wtac_var(&src_ptr, src_offset, &wt),
                                dst: self.mem_of_wtac_var(&store_ptr, offset, &wt),
                                t: wt,
                            };
                            instrs.push(cpy_instr);
                            src_offset += types::get_size(&elem);
                            offset += types::get_size(&elem);
                        }
                    }
                    // Either return Point(1,2) or x = Point(1,2) return x;
                    Type::Record(tag) => handle_record(&tag),
                    Type::Tuple { elems }
                        if elems.len() == 1 && matches!(elems[0], Type::Record(_)) =>
                    {
                        if let Type::Record(tag) = &elems[0] {
                            handle_record(tag);
                        } else {
                            panic!("Unreachable")
                        }
                    }
                    // Base case. Simple types and arrays returned
                    _ => {
                        let conv_type = convert_type(&e_ty);
                        let dst_mem = self.mem_of_wtac_var(&store_ptr, offset, &conv_type);
                        let cpy_instr = W::Instruction::Copy {
                            src: res_e,
                            dst: dst_mem,
                            t: conv_type,
                        };
                        instrs.push(cpy_instr);
                        offset += types::get_size(&e_ty);
                    }
                }
            }
            instrs.push(W::Instruction::Copy {
                src: store_ptr,
                dst: W::Value::Var(RET_VAL_NAME.to_string()),
                t: W::WType::I32,
            });
            instrs.push(W::Instruction::Br("func_exit".to_string()));
            instrs
        }
    }

    /// Lowers a scalar local declaration: register the local, then copy the
    /// initializer into it if there is one.
    fn gen_local_decl(&mut self, vd: &ast::VarDeclaration) -> Vec<W::Instruction> {
        // warn!("Local Non-Array Declaration: {:?}", vd);
        self.update_locals(&vd.name, convert_type(&vd.var_type));
        match &vd.init {
            None => vec![],
            Some(e) => {
                let (mut e_instr, res) = self.gen_expression(e);
                e_instr.push(W::Instruction::Copy {
                    src: res,
                    dst: W::Value::Var(vd.name.clone()),
                    t: convert_type(&vd.var_type),
                });
                e_instr
            }
        }
    }

    /// Lowers an array local declaration. An array literal initializer only needs
    /// its pointer copied; sized dimensions are allocated by [`Self::array_init_helper`].
    fn gen_local_array_decl(&mut self, vd: &ast::VarDeclaration) -> Vec<W::Instruction> {
        self.update_locals(&vd.name, convert_type(&vd.var_type));
        // Check dimensions
        let dims_filter: Vec<&ast::Expression> = vd.dims.iter().flatten().collect();

        // warn!("Local Array Declaration: {:?}", vd);
        // The initailzer is an array Literal. So in Some case the expression creates the array and we just need to copy the pointer
        // In None case we need to create the correct array
        let (mut instr, res_ptr) = match &vd.init {
            Some(e) => self.gen_and_convert_expression(e),
            None => {
                // a : int[] is legal. But nothing is initialized
                if dims_filter.is_empty() {
                    return vec![];
                }
                let elem_t = match &vd.var_type {
                    Type::Array { elem_type } => *elem_type.clone(),
                    _ => panic!("Internal Error: Should be array Type"),
                };
                // a : int[3] is legal. And we create the array
                // a : int[n] legal as well
                // a: int[2][3]
                self.array_init_helper(&elem_t, &dims_filter)
            }
        };
        let cpy_instr = W::Instruction::Copy {
            src: res_ptr,
            dst: W::Value::Var(vd.name.clone()),
            t: W::WType::I32,
        };
        instr.push(cpy_instr);
        instr
    }

    /// Lowers assignment, including multi-assignment.
    ///
    /// The RHS is evaluated first, then the LHS — which may yield `Mem`
    /// destinations for subscripts and field accesses — and then the copies.
    fn gen_assign(&mut self, lhs: &[ast::LVal], rhs: &[ast::Expression]) -> Vec<W::Instruction> {
        let (intrs_rhs, rhs_res): (Vec<Vec<W::Instruction>>, Vec<W::Value>) = rhs
            .iter()
            .map(|e| self.gen_and_convert_expression(e))
            .unzip();
        let rhs_ty: Vec<Type> = rhs
            .iter()
            .map(|e| self.tchecker.typecheck_expression(e).unwrap())
            .collect();
        debug!("LHS: {:?}", lhs);
        debug!("RHS: {:?}", rhs);
        // This typecheck adds a declaration again in Symbol Table. But is not a problem
        let lhs_ty: Vec<Type> = lhs
            .iter()
            .map(|lval| self.tchecker.typecheck_lval(lval).unwrap())
            .collect();
        let (instr_lhs, res_lhs): (Vec<Vec<_>>, Vec<_>) =
            lhs.iter().map(|l| self.gen_lval(l)).unzip();
        // Now generate a copy from rhs to lhs.
        // x, y = add() So can be unbalanced!
        // println!("After generation: {:?}", lhs_ty);
        // Essentially unfolds tuple return types
        let mut dst_rhs = Vec::new();
        let mut rhs_b = InstrBuilder::new(self);
        for (rhs_v, rhs_t) in rhs_res.into_iter().zip(rhs_ty) {
            match rhs_t {
                // The first Tuple is correct for a single type. No load is happening which is correct for Pointer
                Type::Tuple { elems } if elems.len() == 1 => {
                    // let dst = self.make_wtac_var(elems[0].clone());
                    let dst = rhs_b.copy(rhs_v, convert_type(&elems[0]));
                    dst_rhs.push(dst);
                }
                // Sadly duplication right now
                t @ (Type::Bool | Type::Int | Type::Array { .. }) => {
                    let dst = rhs_b.copy(rhs_v, convert_type(&t));
                    dst_rhs.push(dst);
                }
                // Unfolding/ deep copy
                // Except for records since they are deep copied. The values of a record are inside
                // the ret_ptr so we need to just create a pointer to them
                Type::Tuple { elems } => {
                    let mut offset: usize = 0;

                    for t in elems {
                        let dst = rhs_b.gene.make_wtac_var(convert_type(&t));
                        dst_rhs.push(dst.clone());
                        if let Type::Record(tag) = t {
                            let rec_dst_ptr = rhs_b.gene.make_wtac_var(W::WType::I32); // since its record
                            // offset of ret_ptr is ptr to record
                            rhs_b.binary_to(
                                W::BinaryOp::Add,
                                rhs_v.clone(),
                                rhs_b.i32_const(offset as i64),
                                rec_dst_ptr.clone(),
                                W::WType::I32,
                            );
                            rhs_b.copy_to(rec_dst_ptr, dst.clone(), W::WType::I32);
                            // The offset is also different
                            let rec_size = rhs_b.gene.tchecker.symtab.get_record(&tag).size();
                            offset += rec_size;
                        } else {
                            // Then we just load the value from the ret ptr
                            let src_mem =
                                rhs_b
                                    .gene
                                    .mem_of_wtac_var(&rhs_v, offset, &convert_type(&t));
                            rhs_b.copy_to(src_mem, dst.clone(), convert_type(&t));

                            offset += types::get_size(&t);
                        };
                    }
                }
                _ => panic!("Internal Error: Assign Gen: Cannot be Function Type."),
            }
        }

        let rhs_instr_copies = rhs_b.finish();
        let mut lhs_b = InstrBuilder::new(self);
        // Copy rhs to lhs destination
        for ((lhs_dst, lhs_t), rhs_dst) in res_lhs.into_iter().zip(lhs_ty).zip(dst_rhs) {
            if matches!(lhs_t, types::Type::Unknown) {
                // "_" case. do not generate a copy
                continue;
            }
            lhs_b.copy_to(rhs_dst, lhs_dst, convert_type(&lhs_t));
        }
        let cpy_lhs = lhs_b.finish();

        intrs_rhs
            .into_iter()
            .flatten()
            .chain(instr_lhs.into_iter().flatten())
            .chain(rhs_instr_copies.into_iter())
            .chain(cpy_lhs.into_iter())
            .collect()
    }

    /// Lowers `if`/`else` into a `Block` whose branches jump to its end label.
    fn gen_if(
        &mut self,
        guard: &ast::Expression,
        then_br: &ast::Statement,
        else_br: &Option<Box<ast::Statement>>,
    ) -> Vec<W::Instruction> {
        let (g_instr, g_dst) = self.gen_and_convert_expression(guard);
        let then_instr = self.gen_statement(then_br);
        let else_instr = else_br.as_ref().map_or(vec![], |s| self.gen_statement(s));

        let (else_label, end_label) = self.fresh_label_pair("if_else", "if_end");
        // First else block. If condition is true we brea out of else block
        let mut else_block_instrs = Vec::new();
        else_block_instrs.extend(g_instr);
        else_block_instrs.push(W::Instruction::BrIf {
            cond: g_dst,
            l: else_label.clone(),
        });
        else_block_instrs.extend(else_instr);
        else_block_instrs.push(W::Instruction::Br(end_label.clone()));
        let else_block = W::Instruction::Block {
            label: else_label,
            body: else_block_instrs,
        };

        let mut if_instrs = Vec::new();
        if_instrs.push(else_block);
        if_instrs.extend(then_instr);
        if_instrs.push(W::Instruction::Br(end_label.clone()));
        let if_block = W::Instruction::Block {
            label: end_label,
            body: if_instrs,
        };
        vec![if_block]
    }

    /// Lowers `while` into a `Loop` nested in a `Block`: the negated guard breaks
    /// out to the block label, the body falls through to a back-edge branch.
    fn gen_while(&mut self, guard: &ast::Expression, body: &ast::Statement) -> Vec<W::Instruction> {
        let (g_instrs, dst) = self.gen_and_convert_expression(guard);
        let body_instr = self.gen_statement(body);
        let (block_label, loop_label) = self.fresh_label_pair("while_end", "while_loop");
        let mut b = InstrBuilder::new(self);
        b.extend(g_instrs);
        // Negate condition
        let neg_op = b.unary(W::UnaryOp::Not, dst, W::WType::I32);
        b.brif(&neg_op, &block_label);

        b.extend(body_instr);
        b.br(&loop_label);

        let body = b.finish();
        let loop_block = W::Instruction::Loop {
            label: loop_label,
            body,
        };
        let block = W::Instruction::Block {
            label: block_label.clone(),
            body: vec![loop_block],
        };
        vec![block]
    }

    /// Lowers a procedure call: evaluate the arguments, then call with no destination.
    fn gen_procedure_call(&mut self, name: &str, args: &[ast::Expression]) -> Vec<W::Instruction> {
        let (arg_instrs, arg_vals): (Vec<Vec<_>>, Vec<_>) = args
            .iter()
            .map(|e| self.gen_and_convert_expression(e))
            .unzip();
        let mut instrs = Vec::new();
        instrs.extend(arg_instrs.into_iter().flatten());
        instrs.push(W::Instruction::FCall {
            name: name.to_string(),
            args: arg_vals,
            dst: None,
        });
        instrs
    }

    /// Generates the left-hand side of an assignment.
    ///
    /// For declarations, returns a `Value::Var`. For subscripts and field accesses,
    /// returns a `Value::Mem` (so that the assignment becomes a store).
    /// The wildcard `_` produces a dummy variable that is never read.
    fn gen_lval(&mut self, l: &ast::LVal) -> (Vec<W::Instruction>, W::Value) {
        match l {
            ast::LVal::V(vd) => {
                self.update_locals(&vd.name, convert_type(&vd.var_type));
                (vec![], W::Value::Var(vd.name.clone()))
            }
            // Specilay case for _ and marker
            ast::LVal::E(ast::Expression {
                kind: ast::ExpressionKind::Var(n),
                ..
            }) if n == "_" => (vec![], W::Value::Var("_".to_string())),
            ast::LVal::E(e) => {
                // IMPORTANT not to convert! a[i] here will be a store!
                self.gen_expression(e)
            }
        }
    }
}
