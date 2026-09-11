//! Variable resolution and duplicate detection.
//!
//! The [`Resolver`] walks the AST and renames all variables to globally unique names,
//! eliminating the need for later phases to reason about scoping. It also rejects
//! duplicate declarations within the same scope.
//!
//! After resolution, a program like:
//! ```text
//! x: int = 5
//! { x: int = 7 }
//! ```
//! becomes:
//! ```text
//! x.0: int = 5
//! { x.1: int = 7 }
//! ```
//!
//! This is a precondition for the [`Typechecker`](crate::typecheck::Typechecker) and
//! the [`SymbolTable`](crate::symbols::SymbolTable), which use a flat namespace.

use std::collections::HashMap;

use tracing::debug;

use crate::ast::*;

/// An error produced during resolution.
pub struct ResolveError {
    pub msg: String,
    pub span: logos::Span,
}

type ResolveResult = Result<(), ResolveError>;

/// An entry in the [`StackMap`], recording the unique name assigned to an identifier.
#[derive(Debug)]
#[allow(dead_code)] // global is not read right now
pub struct IdEntry {
    unique_name: String,
    global: bool,
}

/// A stack of hash maps that mirrors the lexical scope structure of the program.
///
/// Each map represents one scope level. Lookups search from the innermost
/// scope outward, so inner declarations shadow outer ones.
/// Pushing creates a new scope (e.g. entering a block), popping discards it.
#[derive(Debug)]
pub struct StackMap {
    stack: Vec<HashMap<String, IdEntry>>,
}

impl Default for StackMap {
    fn default() -> Self {
        Self::new()
    }
}

impl StackMap {
    /// Creates a new stack map with a single empty scope (the global scope).
    pub fn new() -> Self {
        StackMap {
            stack: vec![HashMap::new()],
        }
    }

    /// Pushes a new empty scope onto the stack (e.g. entering a block or function body).
    pub fn push(&mut self) {
        self.stack.push(HashMap::new());
    }

    /// Pops the innermost scope (e.g. leaving a block).
    ///
    /// # Panics
    ///
    /// Panics if the stack is empty.
    pub fn pop(&mut self) {
        match self.stack.pop() {
            Some(_) => (),
            None => panic!("Internal Error: Tried to Pop empty Stack Map"),
        }
    }

    /// Adds a binding to the current (innermost) scope.
    pub fn add(&mut self, k: String, v: IdEntry) {
        match self.stack.last_mut() {
            Some(map) => {
                map.insert(k, v);
            }
            None => panic!("Internal Error: Stack Map was empty"),
        }
    }

    /// Looks up a name starting from the innermost scope and searching outward.
    ///
    /// Returns `None` if the name is not found in any scope.
    pub fn find(&self, k: &str) -> Option<&IdEntry> {
        // In reverse order. Since on top is innermost Scope
        for map in self.stack.iter().rev() {
            if let Some(entry) = map.get(k) {
                return Some(entry);
            }
        }
        None
    }

    /// Looks up a name in the current scope only, without searching outer scopes.
    ///
    /// Used to detect duplicate declarations within the same scope.
    pub fn find_curr(&self, k: &str) -> Option<&IdEntry> {
        match self.stack.last() {
            Some(map) => map.get(k),
            None => None,
        }
    }
}

/// Resolves variable names and checks for duplicates.
///
/// Resolution happens in two passes over the top-level declarations:
/// first, all function and record names are registered (to allow mutual recursion
/// and forward references), then each declaration body is resolved.
pub struct Resolver {
    pub scope_map: StackMap,
    // Local to this resolver, so renaming is reproducible across runs.
    next_id: usize,
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Resolver {
    pub fn new() -> Self {
        Resolver {
            scope_map: StackMap::new(),
            next_id: 0,
        }
    }

    /// Resolves all names in a program, mutating the AST in place.
    ///
    /// Top-level functions and records are registered first (so they can
    /// reference each other), then each body is resolved.
    ///
    /// # Errors
    ///
    /// Returns a [`ResolveError`] if a variable is used before declaration,
    /// or if a name is declared twice in the same scope.
    pub fn resolve(&mut self, p: &mut Program) -> ResolveResult {
        // Add length function here
        self.scope_map.add(
            "length".to_string(),
            IdEntry {
                unique_name: "length".to_string(),
                global: true,
            },
        );
        p.decls
            .iter()
            .try_for_each(|d| self.resolve_only_top_level(d))?;
        debug!(scope_map = ?self.scope_map, "Resolved Top Level");
        p.decls.iter_mut().try_for_each(|d| self.resolve_decl(d))
    }

    /// Registers top-level function and record names without resolving their bodies.
    fn resolve_only_top_level(&mut self, decl: &Declaration) -> ResolveResult {
        match decl {
            Declaration::Global(_) => (),
            Declaration::FunDecl(fun) => match self.scope_map.find(&fun.name) {
                Some(_) => {
                    return Err(ResolveError {
                        msg: format!("Function {} was already defined", fun.name),
                        span: fun.span.clone(),
                    });
                }
                None => self.scope_map.add(
                    fun.name.clone(),
                    IdEntry {
                        unique_name: fun.name.clone(),
                        global: true,
                    },
                ),
            },
            Declaration::RecordDecl(record) => match self.scope_map.find(&record.tag) {
                Some(_) => {
                    return Err(ResolveError {
                        msg: format!("Record {} was already defined", record.tag),
                        span: record.span.clone(),
                    });
                }
                None => self.scope_map.add(
                    record.tag.clone(),
                    IdEntry {
                        unique_name: record.tag.clone(),
                        global: true,
                    },
                ),
            },
        }
        Ok(())
    }

    /// Resolves a single declaration (global, function, or record).
    pub fn resolve_decl(&mut self, decl: &mut Declaration) -> ResolveResult {
        match decl {
            Declaration::Global(vd) => match self.scope_map.find_curr(&vd.name) {
                Some(entry) => {
                    return Err(ResolveError {
                        msg: format!("Global {} already exists! Entry : {:?}", vd.name, entry),
                        span: vd.span.clone(),
                    });
                }
                None => self.scope_map.add(
                    vd.name.clone(),
                    IdEntry {
                        unique_name: vd.name.clone(),
                        global: true,
                    },
                ),
            },
            Declaration::FunDecl(fun) => self.resolve_function(fun)?,
            Declaration::RecordDecl(record) => self.resolve_record(record)?,
        }
        Ok(())
    }

    /// Resolves a function: opens a new scope, renames parameters, then resolves the body.
    pub fn resolve_function(&mut self, fun: &mut Function) -> ResolveResult {
        self.scope_map.push();
        for p in &mut fun.params {
            *p = self.add_local_var(p, &fun.span)?
        }
        self.scope_map.push(); // Scope for body
        fun.body
            .stmts
            .iter_mut()
            .try_for_each(|s| self.resolve_stmt(s))
    }

    /// Resolves a record declaration.
    pub fn resolve_record(&mut self, rec: &mut Record) -> ResolveResult {
        let resolve_member = |_m| {
            //  I do not think we need to do anything here
            Ok(())
        };
        rec.members.iter_mut().try_for_each(resolve_member)
    }

    /// Adds a local variable to the current scope with a fresh unique name.
    ///
    /// # Errors
    ///
    /// Returns a [`ResolveError`] if the name already exists in the current scope.
    fn add_local_var(&mut self, name: &str, span: &logos::Span) -> Result<String, ResolveError> {
        // Check if var is already defined
        if self.scope_map.find_curr(name).is_some() {
            return Err(ResolveError {
                msg: format!("Duplicate Variable {} found!", name),
                span: span.clone(),
            });
        }

        let unique_name = format!("{}.{}", name, self.next_id);
        self.next_id += 1;
        self.scope_map.add(
            name.to_string(),
            IdEntry {
                unique_name: unique_name.clone(),
                global: false,
            },
        );
        Ok(unique_name)
    }

    /// Resolves a local variable declaration, including its initializer if present.
    ///
    /// The initializer is resolved *before* the name is added to the scope,
    /// so `x: int = x` refers to an outer `x`, not itself.
    fn resolve_local_var_declaration(&mut self, vd: &mut VarDeclaration) -> ResolveResult {
        if let Some(e) = vd.init.as_mut() {
            self.resolve_expr(e)?
        }
        let unique_name = self.add_local_var(&vd.name, &vd.span)?;
        vd.name = unique_name;
        Ok(())
    }

    /// Resolves the left-hand side of an assignment.
    pub fn resolve_lval(&mut self, lval: &mut LVal) -> ResolveResult {
        match lval {
            LVal::V(vd) => {
                self.resolve_local_var_declaration(vd)?;
            }
            LVal::E(e) => self.resolve_expr(e)?,
        }
        Ok(())
    }

    /// Resolves a statement, opening new scopes for compound blocks.
    pub fn resolve_stmt(&mut self, s: &mut Statement) -> ResolveResult {
        match &mut s.kind {
            StatementKind::LocalDecl(vd) if vd.dims.is_empty() => {
                self.resolve_local_var_declaration(vd)?;
            }
            // Array decl
            StatementKind::LocalDecl(vd) => {
                self.resolve_local_var_declaration(vd)?;

                vd.dims.iter_mut().try_for_each(|e_opt| {
                    if let Some(e) = e_opt {
                        self.resolve_expr(e)
                    } else {
                        Ok(())
                    }
                })?;
            }
            StatementKind::Return(es) => es.iter_mut().try_for_each(|e| self.resolve_expr(e))?,
            StatementKind::Assign { lhs, rhs } => {
                rhs.iter_mut().try_for_each(|e| self.resolve_expr(e))?;
                lhs.iter_mut()
                    .try_for_each(|lval| self.resolve_lval(lval))?;
            }
            StatementKind::Compound(block) => {
                // New Scope
                self.scope_map.push();
                block
                    .stmts
                    .iter_mut()
                    .try_for_each(|s| self.resolve_stmt(s))?;
            }
            StatementKind::If {
                guard,
                then_br,
                else_br,
            } => {
                self.resolve_expr(guard)?;
                self.resolve_stmt(then_br)?;
                else_br.as_mut().map(|els| self.resolve_stmt(els.as_mut()));
            }
            StatementKind::While { guard, body } => {
                self.resolve_expr(guard)?;
                self.resolve_stmt(body)?;
            }
            StatementKind::Procedure { name, args } => {
                // Check name
                let pos_entry = self.scope_map.find(name);
                match pos_entry {
                    None => {
                        return Err(ResolveError {
                            msg: format!("Procedure {} not declared!", name),
                            span: s.span.clone(),
                        });
                    }
                    Some(entry) => *name = entry.unique_name.clone(),
                }
                args.iter_mut().try_for_each(|e| self.resolve_expr(e))?;
            }
        }
        Ok(())
    }

    /// Resolves an expression, replacing variable names with their unique names.
    ///
    /// The wildcard identifier `_` is left unchanged.
    pub fn resolve_expr(&mut self, e: &mut Expression) -> ResolveResult {
        match &mut e.kind {
            ExpressionKind::Var(n) if n == "_" => (),
            ExpressionKind::Var(x) => match self.scope_map.find(x) {
                Some(entry) => *x = entry.unique_name.clone(),
                None => {
                    return Err(ResolveError {
                        msg: format!("Undefined Variable {} found!", x),
                        span: e.span.clone(),
                    });
                }
            },
            ExpressionKind::ArrayLit(es) => es.iter_mut().try_for_each(|e| self.resolve_expr(e))?,
            ExpressionKind::Binary(_, e1, e2) => {
                self.resolve_expr(e1)?;
                self.resolve_expr(e2)?
            }
            ExpressionKind::Unary(_, e) => self.resolve_expr(e)?,
            ExpressionKind::Subscript(e, e1) => {
                self.resolve_expr(e)?;
                self.resolve_expr(e1)?;
            }
            ExpressionKind::Dot(e, _) => self.resolve_expr(e)?,
            ExpressionKind::Call { name, args } => {
                let pos_entry = self.scope_map.find(name);
                match pos_entry {
                    None => {
                        return Err(ResolveError {
                            msg: format!("Function {} was not declared!", name),
                            span: e.span.clone(),
                        });
                    }
                    Some(entry) => *name = entry.unique_name.clone(),
                }
                args.iter_mut().try_for_each(|e| self.resolve_expr(e))?;
            }
            _ => (),
        }
        Ok(())
    }
}
