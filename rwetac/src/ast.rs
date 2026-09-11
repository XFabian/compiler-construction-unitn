//! Abstract Syntax Tree (AST) for the ETA language.
//!
//! This module defines the tree structure produced by the [`Parser`](crate::parser::Parser).
//! Every node carries a [`Span`] that records its byte range in the source file,
//! which is used for error reporting in later phases.
//!
//! The AST closely mirrors the surface syntax of ETA. It is consumed by the
//! [`Resolver`](crate::resolver::Resolver) and [`Typechecker`](crate::typecheck::Typechecker)
//! before being lowered to the [`WTAC IR`](crate::wtac).

use logos::Span;

use crate::types::Type;

/// Binary operators.
#[derive(Debug, PartialEq)]
pub enum BinOp {
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

/// Unary operators.
#[derive(Debug)]
pub enum UnaryOp {
    /// Logical negation (`!x`).
    Not,
    /// Arithmetic negation (`-x`).
    Neg,
}

/// The kind of an expression, without span information.
///
/// Separated from [`Expression`] so that pattern matching on the kind
/// does not require destructuring the span every time.
#[derive(Debug)]
pub enum ExpressionKind {
    Var(String),
    Int(i64),
    Bool(bool),
    /// An array literal (e.g. `{1, 2, 3}`).
    /// String literals are desugared into this form as well,
    /// where each character becomes its integer code.
    ArrayLit(Vec<Expression>),
    Binary(BinOp, Box<Expression>, Box<Expression>),
    Unary(UnaryOp, Box<Expression>),
    Call {
        name: String,
        args: Vec<Expression>,
    },
    /// Array subscript (e.g. `a[i]`). Nested subscripts like `a[i][j]`
    /// are represented as `Subscript(Subscript(a, i), j)`.
    Subscript(Box<Expression>, Box<Expression>),
    /// Field access on a record (e.g. `p.x`).
    Dot(Box<Expression>, String),
}

/// An expression node with source span information.
#[derive(Debug)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub span: logos::Span,
}

/// A variable declaration, used for both local and global variables.
///
/// The `dims` field distinguishes scalar from array declarations:
/// `x: int` has an empty `dims`, while `a: int[3][]` has `dims = [Some(3), None]`.
#[derive(Debug)]
pub struct VarDeclaration {
    pub name: String,
    /// Optional initializer expression (e.g. the `5` in `x: int = 5`).
    pub init: Option<Expression>,
    /// The declared type. For arrays, this is the full nested
    /// [`Type::Array`] constructed from the base type and `dims`.
    pub var_type: Type,
    /// Array dimension expressions, empty for scalar variables.
    /// `Some` means a sized dimension (`int[3]`), `None` means unsized (`int[]`).
    pub dims: Vec<Option<Expression>>,
    pub span: Span,
}

/// The left-hand side of an assignment.
///
/// An assignment can declare new variables and assign to existing ones
/// in the same statement (multi-assignment), so the LHS is either a
/// new variable declaration or an assignable expression.
#[derive(Debug)]
pub enum LVal {
    /// A new variable declaration on the left-hand side (e.g. `x: int` in `x: int = 5`).
    /// Uses [`VarDeclaration`] because the LHS carries type and dimension info.
    /// Note: the `init` field is always `None` here — the initializer is the RHS of the assignment.
    V(VarDeclaration),
    /// An assignable expression (variable, subscript, or field access).
    E(Expression),
}

impl LVal {
    pub fn span(&self) -> logos::Span {
        match self {
            LVal::V(vd) => vd.span.clone(),
            LVal::E(e) => e.span.clone(),
        }
    }
}
/// The kind of a statement, without span information.
#[derive(Debug)]
pub enum StatementKind {
    /// A local variable or array declaration (e.g. `x: int = 5`).
    // (* We need to keep track of the dimension like c : int[2][] will have Some 2: None *)
    LocalDecl(VarDeclaration),

    /// A return statement with zero or more values.
    /// Multiple return values are supported (e.g. `return a, b`).
    Return(Vec<Expression>),

    /// An assignment can declare new variables and assign to existing ones
    /// in the same statement (multi-assignment), so the LHS is either a
    /// new variable declaration or an assignable expression.
    Assign {
        lhs: Vec<LVal>,
        rhs: Vec<Expression>,
    },

    /// A block of statements enclosed in braces.
    Compound(Block),

    /// An `if` statement with an optional `else` branch.
    If {
        guard: Expression,
        then_br: Box<Statement>,
        else_br: Option<Box<Statement>>,
    },

    /// A `while` loop.
    While {
        guard: Expression,
        body: Box<Statement>,
    },

    /// A procedure call (function call used as a statement, result discarded).
    Procedure { name: String, args: Vec<Expression> },
}

/// A statement node with source span information.
#[derive(Debug)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: logos::Span,
}

/// A sequence of statements, representing the body of a function or a compound statement.
#[derive(Debug)]
pub struct Block {
    pub stmts: Vec<Statement>,
}

/// A function definition.
#[derive(Debug)]
pub struct Function {
    pub name: String,
    /// Parameter names. Their types are encoded in `f_type`.
    pub params: Vec<String>,
    /// The full function type, including parameter and return types.
    pub f_type: Type,
    pub body: Block,
    pub span: Span,
}

/// A member within a record declaration (e.g. `x: int`).
#[derive(Debug)]
pub struct MemberDecl {
    pub name: String,
    pub t: Type,
    pub span: Span,
}

/// A record type declaration. Members are separated by newlines, not commas,
/// because the lexer inserts the semicolons:
///
/// ```text
/// record Point {
///     x : int
///     y : int
/// }
/// ```
#[derive(Debug)]
pub struct Record {
    /// The name of the record type.
    pub tag: String,
    pub members: Vec<MemberDecl>,
    pub span: Span,
}

/// A top-level declaration.
#[derive(Debug)]
pub enum Declaration {
    /// A global variable declaration.
    Global(VarDeclaration),
    /// A function definition.
    FunDecl(Function),
    /// A record type definition.
    RecordDecl(Record),
}

/// The root node of the AST, representing an entire source file.
#[derive(Debug)]
pub struct Program {
    pub decls: Vec<Declaration>,
}
