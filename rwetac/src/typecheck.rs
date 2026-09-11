//! Type checking for the ETA language.
//!
//! The [`Typechecker`] walks the resolved AST and verifies that all operations
//! are well-typed. It populates a [`SymbolTable`] with type information that is
//! consumed by WTAC generation.
//!
//! Typechecking uses two passes over top-level declarations: first, all function
//! signatures and record definitions are registered (to support mutual recursion
//! and forward references), then each declaration body is checked.
//!
//! Statements produce a [`StatementType`] (`Unit` or `Void`) rather than a value
//! type. This tracks whether control flow can continue past a statement, which is
//! needed to verify that all function branches return a value.

use std::collections::HashSet;

use tracing::{debug, instrument};

use crate::{
    ast::*,
    symbols::{Attr, Entry, MemberEntry, RecordEntry, SymbolTable},
    types::{self, Type},
};

//     my_func(b: bool): int {
//   if (b) {
//     return 1
//   } else {
//     // This branch falls through
//   }
// }
// This should fail since function body has void type!
/// The result type of statement typechecking.
///
/// This is not a data type — it describes control flow behavior.
/// A function body must have type `Void` to guarantee that every path returns.
#[derive(Debug, Clone, PartialEq)]
pub enum StatementType {
    /// The statement may complete normally (control passes to the next statement).
    Unit,
    /// The statement never passes control to the next one (e.g. it returns).
    Void,
}

impl StatementType {
    /// Computes the least upper bound of two statement types.
    ///
    /// If either branch might complete normally (`Unit`), the combination is `Unit`.
    /// Only if both branches return (`Void`) is the result `Void`.
    pub fn lub(s1: &StatementType, s2: &StatementType) -> StatementType {
        match (s1, s2) {
            (StatementType::Unit, _) => StatementType::Unit,
            (_, StatementType::Unit) => StatementType::Unit,
            _ => s1.clone(), // They are equal
        }
    }
}

/// An error produced during typechecking.
#[derive(Debug)]
pub enum TypeError {
    /// A type mismatch where the expected type is known.
    MismatchedTypes {
        msg: String,
        expected: Type,
        span: logos::Span,
    },
    /// A general type error without a single expected type.
    Generic { msg: String, span: logos::Span },
}

type TypeResult = Result<(), TypeError>;

/// The typechecker for the ETA language.
///
/// Maintains a [`SymbolTable`] that is populated during checking and later
/// consumed by the [`WtacGen`](crate::wtac::wtac_gen::WtacGen).
/// The built-in `length` function (takes any array, returns `int`) is
/// pre-registered.
pub struct Typechecker {
    pub symtab: SymbolTable,
}

impl Default for Typechecker {
    fn default() -> Self {
        Self::new()
    }
}

impl Typechecker {
    /// Creates a new typechecker with a fresh symbol table.
    ///
    /// Pre-registers the built-in `length` function.
    pub fn new() -> Self {
        let mut symtab = SymbolTable::new();
        let length_f_type = Type::FunType {
            param_types: vec![Type::Array {
                elem_type: Box::new(Type::Unknown),
            }],
            ret_type: vec![Type::Int],
        };
        symtab.add_fun("length".to_string(), length_f_type, true);
        Typechecker { symtab }
    }

    /// Entry point: typechecks an entire program.
    ///
    /// First registers all top-level names, then checks each declaration body.
    pub fn typecheck(&mut self, prog: &Program) -> TypeResult {
        for decl in &prog.decls {
            self.add_top_level(decl)?;
        }
        debug!(sym_tab=?self.symtab,"Top-Level SymbolTable");
        for decl in &prog.decls {
            self.typecheck_declarations(decl)?
        }
        Ok(())
    }

    /// Registers a top-level declaration (function signature or record type)
    /// without checking its body.
    fn add_top_level(&mut self, decl: &Declaration) -> TypeResult {
        match decl {
            Declaration::FunDecl(Function { name, f_type, .. }) => {
                self.symtab.add_fun(name.clone(), f_type.clone(), false);
                Ok(())
            }
            Declaration::RecordDecl(rec) => self.typecheck_record(rec),
            _ => Ok(()),
        }
    }

    /// Typechecks a single top-level declaration.
    fn typecheck_declarations(&mut self, decl: &Declaration) -> TypeResult {
        match decl {
            Declaration::Global(VarDeclaration {
                name,
                init,
                var_type,
                dims,
                span,
            }) if dims.is_empty() => {
                if let Type::Record(_) = var_type {
                    return Err(TypeError::Generic {
                        msg: "Global Record declarations are not allowed".to_string(),
                        span: span.clone(),
                    });
                }
                self.symtab.add_global_var(name.clone(), var_type.clone());
                match init
                    .as_ref()
                    .map(|e| self.typecheck_expression(e))
                    .transpose()?
                {
                    Some(t) => {
                        if t != *var_type {
                            return Err(TypeError::MismatchedTypes {
                                msg: "Type of global and its initializer mismatch".to_string(),
                                expected: var_type.clone(),
                                span: span.clone(),
                            });
                        }
                        Ok(())
                    }
                    None => Ok(()),
                }
            }
            // Array globals
            Declaration::Global(vd) => {
                if vd.init.is_some() {
                    return Err(TypeError::Generic {
                        msg: format!(
                            "Global Array declaration {} is not allowed to have initializer",
                            vd.name
                        ),
                        span: vd.span.clone(),
                    });
                }

                self.typecheck_arr_decl(vd)?;
                self.symtab
                    .add_global_var(vd.name.clone(), vd.var_type.clone());
                Ok(())
            }
            Declaration::FunDecl(function) => self.typecheck_fun(function),
            Declaration::RecordDecl(rec) => self.check_record_cycles(rec),
        }
    }

    /// Nothing to check while a record cannot contain a record: the field
    /// typecheck rejects that outright, so no cycle can be built. Adding nested
    /// records means implementing this — walk members recursively, keeping the
    /// records already seen, and report a cycle on revisiting one.
    fn check_record_cycles(&self, _cur_rec: &Record) -> TypeResult {
        Ok(())
    }

    /// Typechecks a record declaration: verifies that no two fields share a name
    /// and computes byte offsets for each field. Registers the record in the
    /// symbol table.
    fn typecheck_record(&mut self, rec: &Record) -> TypeResult {
        let mut seen = HashSet::new();
        let mut members = Vec::new();
        let mut offset = 0;
        // Natural alignment: a field starts at the next multiple of its own
        // alignment, and the record's alignment is the widest of its fields.
        let mut align = 1;

        for MemberDecl { name, t, span } in &rec.members {
            if seen.contains(name) {
                return Err(TypeError::Generic {
                    msg: format!("Duplicate member in Record {}", rec.tag),
                    span: span.clone(),
                });
            }
            seen.insert(name.clone());
            match t {
                Type::FunType { .. } => {
                    return Err(TypeError::Generic {
                        msg: format!(
                            "Function Type not allowed inside records. Record {}",
                            rec.tag
                        ),
                        span: span.clone(),
                    });
                }
                Type::Record(_) => {
                    return Err(TypeError::Generic {
                        msg: "Record Types currently not allowed inside records!.".to_string(),
                        span: span.clone(),
                    });
                }
                _ => (),
            }
            let member_align = types::get_align(t);
            align = align.max(member_align);
            offset = crate::util::round_to_n(offset as isize, member_align as isize) as usize;

            let mem_entry = MemberEntry {
                member_t: t.clone(),
                offset,
            };
            offset += types::get_size(t);
            members.push((name.clone(), mem_entry));
        }

        let rec_entry = RecordEntry { members, align };

        // This add fun is not used since it is overwritten by the add_record.
        // Record Construcotr is handled in function call seperatly. They are nto real functions here
        // let param_types = rec.members.iter().map(|md| md.t.clone()).collect();
        // Add constructor
        // let fn_type = Type::FunType { param_types, ret_type: vec![Type::Record(rec.tag.clone())] };
        // self.symtab.add_fun(rec.tag.clone(), fn_type, true);

        self.symtab.add_record(rec.tag.clone(), rec_entry);
        Ok(())
    }

    /// Typechecks a function: adds parameters to the symbol table, checks the body,
    /// and verifies that the body has [`StatementType::Void`] (i.e. all paths return).
    #[instrument(name="Function", skip(self, fun), fields(n = %fun.name))]
    fn typecheck_fun(&mut self, fun: &Function) -> TypeResult {
        let (params_t, ret_t) = match &fun.f_type {
            Type::FunType {
                param_types,
                ret_type,
            } => (param_types, ret_type),
            _ => panic!("Internal Error: Function has non-function type"),
        };

        let check_old = |old_entry: &Entry| {
            if old_entry.t != fun.f_type {
                return Err(TypeError::Generic {
                    msg: format!(
                        "Incompatible function declarations. Old {:?}, New {:?}",
                        old_entry.t, fun.f_type
                    ),
                    span: fun.span.clone(),
                });
            }
            match old_entry.attr {
                Attr::Fun { defined, .. } => {
                    if defined {
                        return Err(TypeError::Generic {
                            msg: format!("Function {} was already previously defined", fun.name),
                            span: fun.span.clone(),
                        });
                    }
                    Ok(())
                }
                _ => panic!("Internal Error: Symbols is function but has no function atrributes"),
            }
        };
        // Check with old entry
        if let Some(e) = self.symtab.get_opt(&fun.name) {
            check_old(e)?
        };

        self.symtab
            .add_fun(fun.name.clone(), fun.f_type.clone(), true);
        // Add parameters to Symbol Table and then check the body
        for (p, ty) in fun.params.iter().zip(params_t.iter()) {
            self.symtab.add_local_var(p.clone(), ty.clone());
        }
        // Add return type as tuple here!
        let tuple_ret_t = Type::Tuple {
            elems: ret_t.clone(),
        };

        // If functin has return type then the body has to have type void
        // otherwise just return it
        let block_t = self.typecheck_block(&fun.body, &tuple_ret_t)?;
        if !ret_t.is_empty() && block_t != StatementType::Void {
            return Err(TypeError::Generic {
                msg: "Function block does not have void as its type!".to_string(),
                span: fun.span.clone(),
            });
        }
        Ok(())
    }

    // In some cases we need to have the types without tuples. For example
    // x, y = add2()
    // add2() returns a tuple type. However, for assignment check we need the elements of the tuple
    // Second return Point(x,y)
    // Here Point returns a tuple which we need to comparew with the return_type. SO we need to wrap the type in tuple
    // which is wrong for function calls. So we first unfold the tuple type in those cases
    /// Flattens nested tuples into a single flat list of types.
    ///
    /// Multi-return functions produce `Tuple` types, and when multiple such
    /// calls appear on the RHS of an assignment, their types need to be
    /// flattened before comparing against the LHS.
    pub fn unfold_tuple_types(tys: Vec<Type>) -> Vec<Type> {
        let mut unfolded_tys = Vec::new();

        for ty in tys {
            match ty {
                // I do not think we need to do that recusrively here
                Type::Tuple { elems } => unfolded_tys.extend(elems),
                Type::FunType { .. } => panic!("Internal Error: Cant unfold Function Type"),
                _ => unfolded_tys.push(ty),
            }
        }
        unfolded_tys
    }

    /// Compares two types for equality, treating [`Type::Unknown`] as matching
    /// any type.
    ///
    /// This is needed for empty array literals (`{}`) whose element type
    /// is unknown until the context provides it.
    fn eq_mod_unknown(t1: &Type, t2: &Type) -> bool {
        match (t1, t2) {
            (Type::Unknown, _) => true,
            (_, Type::Unknown) => true,
            // pretty hacky I would say. Just used for length function. Does not cover all cases. Also used for Binary add on arrays
            (Type::Array { elem_type: et1 }, Type::Array { elem_type: et2 }) => {
                Self::eq_mod_unknown(et1, et2)
            }
            (t1_, t2_) => *t1_ == *t2_,
        }
    }

    /// Unifies two types, preferring the more specific one.
    ///
    /// When one side is [`Type::Unknown`] (e.g. from an empty array literal `{}`),
    /// the other side's type is returned. For arrays, unification recurses into
    /// the element type. Used after [`eq_mod_unknown`](Self::eq_mod_unknown)
    /// confirms the types are compatible.
    fn unify(t1: &Type, t2: &Type) -> Type {
        match (t1, t2) {
            (Type::Unknown, _) => t2.clone(),
            (_, Type::Unknown) => t1.clone(),
            // pretty hacky I would say. Just used for length function. Does not cover all cases. Also used for Binary add on arrays
            (Type::Array { elem_type: et1 }, Type::Array { elem_type: et2 }) => Type::Array {
                elem_type: Box::new(Self::unify(et1, et2)),
            },
            (t1_, t2_) if t1_ == t2_ => t1_.clone(),
            _ => panic!(
                "Internal Error: Unification only allowed for arrays and simple types right now!"
            ),
        }
    }

    fn typecheck_block(&mut self, block: &Block, ret_t: &Type) -> Result<StatementType, TypeError> {
        if block.stmts.is_empty() {
            return Ok(StatementType::Unit);
        }
        let (last_stmt, preceding_stmts) = block.stmts.split_last().unwrap();
        // All except last have to have type unit
        for s in preceding_stmts {
            let stmt_t = self.typecheck_statement(s, ret_t)?;
            if stmt_t != StatementType::Unit {
                return Err(TypeError::Generic {
                    msg: format!(
                        "Statment {:?} in Block had Statement Type Void but was not the last Stement in block",
                        s
                    ),
                    span: s.span.clone(),
                });
            }
        }
        // Result of Block is result of last stmt
        self.typecheck_statement(last_stmt, ret_t)
    }

    /// Typechecks a statement, returning its [`StatementType`].
    ///
    /// `ret_t` is the expected return type of the enclosing function, passed
    /// down so that `return` statements can be checked against it.
    #[instrument(skip(self, ret_t), level = "debug")]
    pub fn typecheck_statement(
        &mut self,
        stmt: &Statement,
        ret_t: &Type,
    ) -> Result<StatementType, TypeError> {
        match &stmt.kind {
            StatementKind::Return(es) => self.typecheck_return(es, &stmt.span, ret_t),
            StatementKind::LocalDecl(vd) => {
                self.symtab
                    .add_local_var(vd.name.clone(), vd.var_type.clone());
                if vd.dims.is_empty() {
                    self.typecheck_scalar_decl(vd)
                } else {
                    // Arrays
                    self.typecheck_arr_decl(vd)?;
                    Ok(StatementType::Unit)
                }
            }
            StatementKind::Assign { lhs, rhs } => self.typecheck_assign(lhs, rhs, &stmt.span),
            StatementKind::Compound(block) => {
                if block.stmts.is_empty() {
                    return Ok(StatementType::Unit);
                }
                let (last_stmt, preceding_stmts) = block.stmts.split_last().unwrap();
                // All except last have to have type unit
                for s in preceding_stmts {
                    let stmt_t = self.typecheck_statement(s, ret_t)?;
                    if stmt_t != StatementType::Unit {
                        return Err(TypeError::Generic {
                            msg: format!(
                                "Statment {:?} in Block had Statement Type Void but was not the last Statement in block",
                                s
                            ),
                            span: s.span.clone(),
                        });
                    }
                }
                // Result of Block is result of last stmt
                self.typecheck_statement(last_stmt, ret_t)
            }
            StatementKind::If {
                guard,
                then_br,
                else_br,
            } => {
                let e_t = self.typecheck_expression(guard)?;
                if e_t != Type::Bool {
                    return Err(TypeError::MismatchedTypes {
                        msg: format!("Guard of If has to have Type bool. Found {}", e_t),
                        expected: Type::Bool,
                        span: guard.span.clone(),
                    });
                }

                let then_t = self.typecheck_statement(then_br, ret_t)?;
                if let Some(s) = else_br.as_ref() {
                    let else_t = self.typecheck_statement(s, ret_t)?;
                    let lub = StatementType::lub(&then_t, &else_t);
                    Ok(lub)
                }
                // No else branch then if has to be unit since it can be skipped
                else {
                    Ok(StatementType::Unit)
                }
            }
            StatementKind::While { guard, body } => {
                let e_t = self.typecheck_expression(guard)?;
                if e_t != Type::Bool {
                    return Err(TypeError::MismatchedTypes {
                        msg: format!("Guard of While has to have Type bool. Found {}", e_t),
                        expected: Type::Bool,
                        span: guard.span.clone(),
                    });
                }
                self.typecheck_statement(body, ret_t)?;
                Ok(StatementType::Unit)
            }
            StatementKind::Procedure { name, args } => {
                match self.check_fun_args(name, args, &stmt.span)? {
                    Type::Tuple { elems } if elems.is_empty() => Ok(StatementType::Unit),
                    _ => Err(TypeError::Generic {
                        msg: "Procedure call needs empty return type".to_string(),
                        span: stmt.span.clone(),
                    }),
                }
            }
        }
    }

    /// Typechecks a `return` statement.
    ///
    /// Evaluates each return expression, flattens tuple types (from multi-return
    /// function calls), and verifies that the resulting types match the enclosing
    /// function's declared return type `ret_t` in both number and kind.
    fn typecheck_return(
        &mut self,
        es: &[Expression],
        span: &logos::Span,
        ret_t: &Type,
    ) -> Result<StatementType, TypeError> {
        let es_ty: Vec<Type> = es
            .iter()
            .map(|e| self.typecheck_expression(e))
            .collect::<Result<_, _>>()?;
        // Function calls return tuples
        let unfold_es_ty = Self::unfold_tuple_types(es_ty);

        let tuple_es_ty = Type::Tuple {
            elems: unfold_es_ty,
        };
        match (tuple_es_ty, ret_t) {
            (Type::Tuple { elems: elems1 }, Type::Tuple { elems: ret_elems }) => {
                if elems1.len() != ret_elems.len() {
                    Err(TypeError::MismatchedTypes {
                        msg: format!(
                            "Return has wrong type!. Number of types do not match. Found {:?}",
                            elems1
                        ),
                        expected: ret_t.clone(),
                        span: span.clone(),
                    })
                } else if elems1 != *ret_elems {
                    Err(TypeError::Generic {
                        msg: format!(
                            "One Return type does not match with the return statement. Found {:?}. Expected {:?}",
                            elems1, ret_elems
                        ),
                        span: span.clone(),
                    })
                } else {
                    Ok(StatementType::Void)
                }
            }
            _ => panic!("Internal Error: Provided Return Type was not a Tuple!"),
        }
    }

    /// Typechecks a scalar variable declaration (no array dimensions).
    ///
    /// Checks the initializer (if present) against the declared type.
    /// Single-element tuples returned by function calls are unwrapped before comparison.
    fn typecheck_scalar_decl(&mut self, vd: &VarDeclaration) -> Result<StatementType, TypeError> {
        match &vd.init {
            Some(e) => {
                let e_ty = self.typecheck_expression(e)?;
                let e_ty_unfold = if let Type::Tuple { elems } = &e_ty {
                    let es_unfold = Self::unfold_tuple_types(elems.clone());
                    if es_unfold.len() != 1 {
                        return Err(TypeError::MismatchedTypes {
                            msg: format!(
                                "Initializer has wrong Tuple type. Length does not works. Found {}",
                                Type::Tuple {
                                    elems: elems.clone()
                                }
                            ),
                            expected: vd.var_type.clone(),
                            span: e.span.clone(),
                        });
                    }
                    es_unfold[0].clone()
                } else {
                    e_ty.clone()
                };
                if e_ty_unfold != vd.var_type {
                    return Err(TypeError::MismatchedTypes {
                        msg: format!("Initializer has wrong type. Found {}", e_ty.clone()),
                        expected: vd.var_type.clone(),
                        span: e.span.clone(),
                    });
                }
                Ok(StatementType::Unit)
            }
            None => Ok(StatementType::Unit),
        }
    }

    /// Typechecks an assignment statement.
    ///
    /// Verifies that all LHS entries are valid l-values, typechecks both sides,
    /// flattens tuple types on the RHS (to handle multi-return function calls),
    /// and checks that the LHS and RHS match in length and types.
    /// Uses [`eq_mod_unknown`](Self::eq_mod_unknown) for comparison so that
    /// the wildcard `_` (typed as `Unknown`) is accepted on the LHS.
    fn typecheck_assign(
        &mut self,
        lhs: &[LVal],
        rhs: &[Expression],
        span: &logos::Span,
    ) -> Result<StatementType, TypeError> {
        if !lhs.iter().all(is_lval) {
            return Err(TypeError::Generic {
                msg: format!("Not all expression on LHS are Lvalues. LVals : {:?}", lhs),
                span: span.clone(),
            });
        }

        let lhs_t: Vec<Type> = lhs
            .iter()
            .map(|s| self.typecheck_lval(s))
            .collect::<Result<_, _>>()?;

        let rhs_t: Vec<Type> = rhs
            .iter()
            .map(|e| self.typecheck_expression(e))
            .collect::<Result<_, _>>()?;
        let rhs_t_unfolded = Self::unfold_tuple_types(rhs_t);
        if lhs_t.len() != rhs_t_unfolded.len() {
            return Err(TypeError::Generic {
                msg: format!(
                    "Amount of Lvalues and rhs value mismatch. Lvals: {:?}, RVals: {:?}",
                    lhs, rhs
                ),
                span: span.clone(),
            });
        }
        if !lhs_t
            .iter()
            .zip(rhs_t_unfolded.iter())
            .all(|(l, r)| Self::eq_mod_unknown(l, r))
        {
            return Err(TypeError::Generic {
                msg: format!(
                    "Types of LHS and RHS are mismatched. LHS: {:?} RHS: {:?}",
                    lhs_t, rhs_t_unfolded
                ),
                span: span.clone(),
            });
        }
        Ok(StatementType::Unit)
    }

    /// Validates that array dimensions are well-formed.
    ///
    /// A specified dimension (e.g. `[3]`) cannot follow an unspecified one (`[]`).
    /// For example, `int[3][]` is valid but `int[][3]` is not, because the
    /// inner dimension must be known if the outer one is.
    fn dim_check(dims_b: &[bool]) -> bool {
        let mut unspec_bool = false;
        for d_b in dims_b {
            if !*d_b {
                unspec_bool = true;
            }

            if *d_b && unspec_bool {
                return false;
            }
        }
        true
    }

    /// Typechecks an array declaration, verifying that dimension expressions
    /// are integers and that the initializer (if present) matches the declared type.
    fn typecheck_arr_decl(&mut self, vd: &VarDeclaration) -> TypeResult {
        //Check out dimensions Convert dims with value to true empty ones to false
        // [1][][2] -> true, false, true
        debug!("Isnide array initialzer");
        let dims_bool: Vec<bool> = vd.dims.iter().map(|d| d.is_some()).collect();
        if !Self::dim_check(&dims_bool) {
            return Err(TypeError::Generic {
                msg: format!(
                    "A specified dimension cannot follow after an unspecified one, Array {}",
                    vd.name
                ),
                span: vd.span.clone(),
            });
        }

        // Only Int indices allowed!
        let dims_e: Vec<Type> = vd
            .dims
            .iter()
            .flatten()
            .map(|e| self.typecheck_expression(e))
            .collect::<Result<_, _>>()?;
        if !dims_e.iter().all(|e_t| *e_t == Type::Int) {
            return Err(TypeError::Generic {
                msg: format!("Non-Int value used as Index into Array!, Array {}", vd.name),
                span: vd.span.clone(),
            });
        }
        // If dimension is specified an initializer is not allowed!
        let has_spec_dim = dims_bool.iter().any(|b| *b);
        match &vd.init {
            Some(_) if has_spec_dim => {
                return Err(TypeError::Generic {
                    msg: "Cannot specify dimensions and initialize array. Not allowed.".to_string(),
                    span: vd.span.clone(),
                });
            }
            Some(e) => {
                let e_t = self.typecheck_expression(e)?;
                match e_t {
                    Type::Array { elem_type } if *elem_type == Type::Unknown => (),
                    t if t == vd.var_type => (),
                    // Happens when array is returned by function call
                    Type::Tuple { elems } if elems[0] == vd.var_type => (),
                    t => {
                        return Err(TypeError::MismatchedTypes {
                            msg: format!("Initializer has wrong type for the array. Found {}", t),
                            expected: vd.var_type.clone(),
                            span: e.span.clone(),
                        });
                    }
                }
            }
            None => (),
        }

        Ok(())
    }

    /// Typechecks the left-hand side of an assignment.
    ///
    /// For [`LVal::V`], adds the variable to the symbol table.
    /// For [`LVal::E`], checks that the expression is a valid l-value.
    pub fn typecheck_lval(&mut self, l: &LVal) -> Result<Type, TypeError> {
        match l {
            LVal::E(Expression {
                kind: ExpressionKind::Var(n),
                ..
            }) if n == "_" => Ok(Type::Unknown),
            LVal::E(e) => self.typecheck_expression(e),
            LVal::V(vd) => {
                // add to symbol table
                self.symtab
                    .add_local_var(vd.name.clone(), vd.var_type.clone());
                Ok(vd.var_type.clone())
            }
        }
    }

    /// Typechecks an expression, returning its [`Type`].
    //#[instrument(skip(self), level = "debug")]
    pub fn typecheck_expression(&mut self, expr: &Expression) -> Result<Type, TypeError> {
        let e = match &expr.kind {
            ExpressionKind::Var(name) => self.symtab.get(name).t.clone(),
            ExpressionKind::Int(_) => Type::Int,
            ExpressionKind::Bool(_) => Type::Bool,
            ExpressionKind::ArrayLit(es) => {
                let es_t: Vec<Type> = es
                    .iter()
                    .map(|e| self.typecheck_expression(e))
                    .collect::<Result<_, _>>()?;
                match es_t.as_slice() {
                    [] => Type::Array {
                        elem_type: Box::new(Type::Unknown),
                    }, // {} case
                    [x, rest @ ..] => {
                        if !rest.iter().all(|e_t| *x == *e_t) {
                            return Err(TypeError::Generic {
                                msg: format!(
                                    "Not all elements in array literal have same type. Types: {:?}",
                                    es_t
                                ),
                                span: expr.span.clone(),
                            });
                        }
                        Type::Array {
                            elem_type: Box::new(x.clone()),
                        }
                    }
                }
            }
            ExpressionKind::Unary(UnaryOp::Neg, e) => {
                let e_ty = self.typecheck_expression(e)?;
                if e_ty != Type::Int {
                    return Err(TypeError::MismatchedTypes {
                        msg: format!("Expression has wrong type in Unary. Found {}", e_ty),
                        expected: Type::Int,
                        span: expr.span.clone(),
                    });
                }
                e_ty
            }
            ExpressionKind::Unary(UnaryOp::Not, e) => {
                let e_ty = self.typecheck_expression(e)?;
                if e_ty != Type::Bool {
                    return Err(TypeError::MismatchedTypes {
                        msg: format!("Expression has wrong type in Unary. Found {}", e_ty),
                        expected: Type::Bool,
                        span: expr.span.clone(),
                    });
                }
                e_ty
            }
            ExpressionKind::Binary(bin_op, e1, e2) => self.typecheck_binary(bin_op, e1, e2)?,
            ExpressionKind::Call { name, args } => {
                self.typecheck_fun_call(name, args, &expr.span)?
            }
            ExpressionKind::Subscript(e1, e2) => {
                let arr_t = self.typecheck_expression(e1)?;
                let idx_t = self.typecheck_expression(e2)?;
                match (&arr_t, &idx_t) {
                    (Type::Array { elem_type }, Type::Int) => *elem_type.clone(),
                    (_, Type::Int) => {
                        return Err(TypeError::Generic {
                            msg: format!("Trying to Index into non-array. Found {}", arr_t.clone()),
                            span: expr.span.clone(),
                        });
                    }
                    (_, _) => {
                        return Err(TypeError::Generic {
                            msg: format!("Array Index must be an integer. Found {}", idx_t.clone()),
                            span: expr.span.clone(),
                        });
                    }
                }
            }
            ExpressionKind::Dot(e, field) => {
                let e_ty = self.typecheck_expression(e)?;
                if let Type::Record(name) = e_ty {
                    match self.symtab.get_attr(&name) {
                        Attr::Record(rec_entry) => match rec_entry.find(field) {
                            Some(md) => md.member_t.clone(),
                            None => {
                                return Err(TypeError::Generic {
                                    msg: format!(
                                        "Field {} does not exist in Record {}",
                                        field, name
                                    ),
                                    span: expr.span.clone(),
                                });
                            }
                        },
                        _ => panic!("Internal Error: Record with non- record attribute"),
                    }
                } else {
                    return Err(TypeError::Generic {
                        msg: "Expression in Dot has to be Record Type!".to_string(),
                        span: expr.span.clone(),
                    });
                }
            }
        };
        Ok(e)
    }

    /// Typechecks a function call expression.
    ///
    /// Rejects calls to functions with no return value (those are procedure calls
    /// and must appear as statements).
    fn typecheck_fun_call(
        &mut self,
        name: &str,
        args: &[Expression],
        span: &logos::Span,
    ) -> Result<Type, TypeError> {
        match self.check_fun_args(name, args, span)? {
            Type::Tuple { elems } if elems.is_empty() => Err(TypeError::Generic {
                msg: format!(
                    "Function call needs non-empty return type. Function was {}",
                    name
                ),
                span: span.clone(),
            }),
            t @ Type::Tuple { .. } => Ok(t),
            // NEW CHANGE. FUNCTIONS WITH ONE RETURN VALUE RETURN THAT LIKE INT AND NOT (INT)
            t => Ok(t),
        }
    }

    /// Shared helper for function and procedure calls. Verifies that argument
    /// types match the parameter types in the function signature.
    fn check_fun_args(
        &mut self,
        name: &str,
        args: &[Expression],
        span: &logos::Span,
    ) -> Result<Type, TypeError> {
        let args_t: Vec<Type> = args
            .iter()
            .map(|arg| self.typecheck_expression(arg))
            .collect::<Result<_, _>>()?;
        let (param_t, ret_t) = match self.symtab.get(name) {
            Entry {
                t:
                    Type::FunType {
                        param_types,
                        ret_type,
                    },
                attr: Attr::Fun { .. },
            } => (param_types, ret_type),
            // Record Constructor!
            Entry {
                t,
                attr: Attr::Record(rec_entry),
            } => {
                let mut param_types = Vec::new();
                for (_, mem_entry) in &rec_entry.members {
                    param_types.push(mem_entry.member_t.clone());
                }
                // meh that clone is annoying
                (&param_types.clone(), &vec![t.clone()])
            }

            Entry {
                t: Type::FunType { .. },
                ..
            } => panic!("Internal Error: Function has non-function and non-record attribute!"),
            _ => panic!("Function has non-function Type!"),
        };
        if param_t.len() != args.len() {
            return Err(TypeError::Generic {
                msg: format!(
                    "Number of arguments provided does not fit with function type.\n Function {} Param Type : {:?} and Arguments {:?}",
                    name, param_t, args
                ),
                span: span.clone(),
            });
        }
        // This does not work. it is an array inside but eq mod unknown is not recursive
        if !param_t
            .iter()
            .zip(args_t)
            .all(|(t1, t2)| Self::eq_mod_unknown(t1, &t2))
        {
            return Err(TypeError::Generic {
                msg: format!(
                    "Some arguments do not have the the correct type.\n Function {}",
                    name
                ),
                span: span.clone(),
            });
        }
        match ret_t.len() {
            0 => Ok(Type::Tuple { elems: vec![] }), // Used for procedures!
            1 => Ok(ret_t[0].clone()),
            _ => Ok(Type::Tuple {
                elems: ret_t.clone(),
            }),
        }
    }

    /// Typechecks a binary operation. Arithmetic operators require `int` operands,
    /// comparison operators require matching types and return `bool`, and
    /// logical operators require `bool` operands.
    fn typecheck_binary(
        &mut self,
        op: &BinOp,
        e1: &Expression,
        e2: &Expression,
    ) -> Result<Type, TypeError> {
        let e1_ty = self.typecheck_expression(e1)?;
        let e2_ty = self.typecheck_expression(e2)?;

        let res_ty = match op {
            BinOp::Add if is_arr(&e1_ty) && is_arr(&e2_ty) => {
                if !Self::eq_mod_unknown(&e1_ty, &e2_ty) {
                    return Err(TypeError::Generic {
                        msg: format!("Array Types are not equal. Found {} and {}", e1_ty, e2_ty),
                        span: e1.span.clone(),
                    });
                }
                // Since types are equal we know that they have same dimension
                // If unknown was used we want the more specific type
                // Or is that handled in the statement Similar for function callsS
                Self::unify(&e1_ty, &e2_ty)
            }
            BinOp::Add | BinOp::Sub | BinOp::Mult | BinOp::Div | BinOp::Mod
                if e1_ty == Type::Int && e2_ty == Type::Int =>
            {
                e1_ty
            }
            BinOp::Lt | BinOp::Leq | BinOp::Gt | BinOp::Geq
                if e1_ty == Type::Int && e2_ty == Type::Int =>
            {
                Type::Bool
            }
            BinOp::And | BinOp::Or if e1_ty == Type::Bool && e2_ty == Type::Bool => Type::Bool,
            BinOp::Eq | BinOp::Neq if e1_ty == e2_ty => Type::Bool,
            _ => {
                return Err(TypeError::Generic {
                    msg: format!(
                        "Binary Operation with invalid Type! e1 : {} e2 : {}",
                        e1_ty, e2_ty
                    ),
                    span: e1.span.clone(),
                });
            }
        };
        Ok(res_ty)
    }
}

/// Returns `true` if an [`LVal`] is a valid assignment target.
///
/// Variables, subscripts, and field accesses are valid l-values.
/// Literals, binary operations, and function calls are not.
fn is_lval(l: &LVal) -> bool {
    match l {
        LVal::V(_) => true,
        LVal::E(e) => matches!(
            e.kind,
            ExpressionKind::Var(..) | ExpressionKind::Dot(..) | ExpressionKind::Subscript(..)
        ),
    }
}

/// Returns `true` if the type is an array type.
pub fn is_arr(t: &Type) -> bool {
    matches!(t, Type::Array { .. })
}
