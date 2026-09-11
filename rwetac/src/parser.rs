//! Recursive descent parser for the ETA language.
//!
//! Consumes a token stream from the [`Lexer`] and produces an
//! [`AST`](crate::ast). The parser is hand-written rather than generated, which gives
//! full control over error messages and recovery.
//!
//! Expressions are parsed using precedence climbing (Pratt parsing) via
//! [`Parser::parse_expression`]. Statements and declarations use straightforward
//! recursive descent. The parser uses [`peekmore`] for bounded lookahead — a second
//! token of lookahead is needed to distinguish function declarations from globals
//! at the top level (e.g. `foo(` vs `foo:`).

use core::panic;
use std::fmt;

use crate::lexer::Lexer;
use crate::types::Type;
use crate::util::Merge;
use crate::{ast::*, token::Token};
use logos::Span;
use peekmore::PeekMore;
use peekmore::PeekMoreIterator;
use tracing::{debug, info, instrument, warn};

/// An error produced during parsing.
#[derive(Debug)]
pub enum ParserError {
    /// The parser encountered a token it did not expect in the current context.
    /// Contains a descriptive message, the offending token, and its span.
    UnexpectedToken(String, Token, logos::Span),

    /// An expression was found in a position where it is not valid
    /// (e.g. a non-identifier before a function call).
    InvalidExpression(String, ExpressionKind, logos::Span),
}

impl ParserError {
    pub fn span(&self) -> &logos::Span {
        match self {
            ParserError::UnexpectedToken(_, _, span) => span,
            ParserError::InvalidExpression(_, _, span) => span,
        }
    }
}
impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParserError::UnexpectedToken(msg, token, _) => {
                writeln!(f, "Unexpected Token {}! Message: {}", token, msg)?
            }
            ParserError::InvalidExpression(msg, e, _) => {
                writeln!(f, "Invalid Expression: {:?} Msg: {}", e, msg)?
            }
        }
        Ok(())
    }
}

/// Convenience alias for parser results.
type PResult<T> = Result<T, ParserError>;

/// The lexer turns newlines into semicolons, so a [`Token::Semicolon`] found
/// where something else was expected nearly always means a line was broken in
/// the wrong place rather than that the user typed a `;`.
fn asi_hint(found: &Token) -> &'static str {
    match found {
        Token::Semicolon => {
            ". Note the line break here was turned into a statement separator, so this `;` is \
             not in the source. If this line was meant to continue the previous one, end the \
             previous line with the operator or comma"
        }
        _ => "",
    }
}

/// A recursive descent parser for the ETA language.
///
/// Wraps a [`Lexer`] in a [`PeekMoreIterator`] to allow bounded lookahead.
/// The parser is the second phase of compilation, invoked by [`compile::compile`](crate::compile::compile).
///
/// # Usage
///
/// ```ignore
/// let lexer = Lexer::new(source);
/// let mut parser = Parser::new(lexer);
/// let ast = parser.parse()?;
/// ```
pub struct Parser<'a> {
    lexer: PeekMoreIterator<Lexer<'a>>,
    input_length: usize,
}

impl<'a> Parser<'a> {
    /// Creates a new parser from a [`Lexer`].
    pub fn new(lexer: Lexer<'a>) -> Self {
        let len = lexer.input_length;
        Parser {
            lexer: lexer.peekmore(),
            input_length: len,
        }
    }

    /// Returns `true` if the token stream is exhausted.
    pub fn is_eof(&mut self) -> bool {
        self.lexer.peek().is_none()
    }

    /// Consumes and returns the next token and its span.
    ///
    /// # Panics
    ///
    /// Panics if called at EOF.
    fn consume(&mut self) -> (Token, Span) {
        match self.lexer.next() {
            Some(s_tok) => s_tok.unwrap(),
            None => panic!("Consume called on EOF"),
        }
    }

    /// Peek at next token **without** advancing the Iterator.
    ///
    /// Returns [`Token::Eof`] if the stream is exhausted.
    fn peek(&mut self) -> &Token {
        match self.lexer.peek() {
            Some(Ok(tok)) => &tok.0,
            _ => &Token::Eof,
        }
    }

    /// Returns the span of the next token without consuming it.
    ///
    /// Useful for constructing error messages when the token itself is not needed.
    fn peek_span(&mut self) -> logos::Span {
        match self.lexer.peek() {
            Some(Ok(tok)) => tok.1.clone(),
            _ => logos::Span {
                start: self.input_length,
                end: self.input_length,
            },
        }
    }

    /// Peeks `n` tokens ahead without consuming anything.
    ///
    /// Used for the two-token lookahead needed to distinguish declarations
    /// at the top level (e.g. `foo(` for functions vs `foo:` for globals).
    fn peek_n(&mut self, n: usize) -> Token {
        match self.lexer.peek_nth(n) {
            Some(Ok(tok)) => tok.0.clone(),
            _ => Token::Eof,
        }
    }

    /// Consumes the next token of the iterator and compares it with `kind`.
    ///
    /// If the token matches the `kind` return its [`Span`].
    ///
    /// # Errors
    ///
    /// Returns [`ParserError::UnexpectedToken`] upon failure.
    fn expect(&mut self, kind: Token) -> PResult<Span> {
        let (tok, span) = match self.lexer.next() {
            Some(s_tok) => s_tok.unwrap(),
            None => (
                Token::Eof,
                Span {
                    start: self.input_length,
                    end: self.input_length,
                },
            ),
        };
        if kind == tok {
            return Ok(span);
        }
        let error_msg = format!("Expected {:?}{}", kind, asi_hint(&tok));
        Err(ParserError::UnexpectedToken(error_msg, tok, span))
    }

    /// Similar to [`Self::expect`] just specialized for identifiers.
    fn expect_id(&mut self) -> PResult<(String, Span)> {
        let (tok, span) = self.consume();
        match tok {
            Token::Var(id) => Ok((id, span)),
            _ => {
                let error_msg = "Expected Identifer".to_string();
                Err(ParserError::UnexpectedToken(error_msg, tok, span))
            }
        }
    }

    /// Entry point: parses the full token stream into a [`Program`].
    // #[instrument(name="Parsing",
    // skip(self))]
    pub fn parse(&mut self) -> PResult<Program> {
        self.parse_program()
    }

    pub fn parse_program(&mut self) -> PResult<Program> {
        let mut decls = Vec::new();
        while !self.is_eof() {
            let decl = self.parse_declaration()?;
            decls.push(decl);
            // Semcolons automatically added by Lexer
            self.expect(Token::Semicolon)?;
        }

        Ok(Program { decls })
    }

    fn parse_declaration(&mut self) -> PResult<Declaration> {
        match self.peek() {
            Token::Var(_) => {
                let decl = match self.peek_n(1) {
                    Token::LParen => Declaration::FunDecl(self.parse_function()?),
                    _ => Declaration::Global(self.parse_variable_declaration()?),
                };
                Ok(decl)
            }
            Token::Record => Ok(Declaration::RecordDecl(self.parse_record()?)),
            _ => Err(ParserError::UnexpectedToken(
                "Not a valid Declaration".to_string(),
                self.peek().clone(),
                self.peek_span(),
            )),
        }
    }

    /// Parses a record type declaration. Members are newline-separated:
    /// `record Point {` / `x : int` / `y : bool` / `}`.
    fn parse_record(&mut self) -> PResult<Record> {
        let span_keyword = self.expect(Token::Record)?;
        let (tag, span_name) = self.expect_id()?;
        self.expect(Token::LBrace)?;

        let members = self.parse_members()?;

        self.expect(Token::RBrace)?;
        Ok(Record {
            tag,
            members,
            span: span_keyword.merge(&span_name),
        })
    }

    fn parse_members(&mut self) -> PResult<Vec<MemberDecl>> {
        let mut members = Vec::new();
        // x, y : int allowed!
        while *self.peek() != Token::RBrace && !self.is_eof() {
            let names_spans = self.parse_sep_list_comma(Token::Colon, |p| p.expect_id())?;
            self.expect(Token::Colon)?;
            let (ty, span_ty) = self.parse_type()?;
            // match names with type
            for (name, span) in names_spans {
                members.push(MemberDecl {
                    name,
                    t: ty.clone(),
                    span: span.merge(&span_ty),
                });
            }

            self.expect(Token::Semicolon)?;
        }
        Ok(members)
    }
    // sort(a: int[]) { } ratadd(p1:int, q1:int, p2:int, q2:int) : int, int
    // #[instrument(skip(self))]
    fn parse_function(&mut self) -> PResult<Function> {
        let (name, span_name) = self.expect_id()?;
        info!("Parsing function {}", name);
        self.expect(Token::LParen)?;
        // Parse params
        let (params, types) = self.parse_params()?.into_iter().unzip();
        self.expect(Token::RParen)?;
        // Check return type

        let ret_t: Vec<Type> = match self.peek() {
            Token::Colon => {
                self.consume();
                // Throw away span information
                let vec_span: Vec<(Type, std::ops::Range<usize>)> =
                    self.parse_sep_list_comma(Token::LBrace, |p| p.parse_type())?;
                vec_span.into_iter().map(|(ty, _)| ty).collect()
            }
            _ => vec![],
        };
        let f_type = Type::FunType {
            param_types: types,
            ret_type: ret_t,
        };
        self.expect(Token::LBrace)?;
        // Parse Body
        let body = Block {
            stmts: self.parse_block()?,
        };
        self.expect(Token::RBrace)?;

        // Construct Type

        Ok(Function {
            name,
            params,
            f_type,
            body,
            span: span_name,
        })
    }

    fn parse_params(&mut self) -> PResult<Vec<(String, Type)>> {
        // let mut params = Vec::new();
        let ps = |p: &mut Self| {
            let (name, _span_name) = p.expect_id()?;
            p.expect(Token::Colon)?;
            let (t, _) = p.parse_type()?;
            Ok((name, t))
        };
        self.parse_sep_list_comma(Token::RParen, ps)
    }

    // Only primary types (Used for array declarations)
    fn parse_prim_type(&mut self) -> PResult<(Type, Span)> {
        // base type like int
        let (t, span) = self.consume();
        let ty = match t {
            Token::TInt => Type::Int,
            Token::TBool => Type::Bool,
            Token::Var(name) => Type::Record(name),
            _ => {
                return Err(ParserError::UnexpectedToken(
                    "Should be type".to_string(),
                    t,
                    span,
                ));
            }
        };
        Ok((ty, span))
    }

    fn parse_type(&mut self) -> PResult<(Type, Span)> {
        let (mut ty, span_ty) = self.parse_prim_type()?;
        // Parse array type
        let mut end_span = span_ty.clone();
        while let Token::LBrack = self.peek() {
            self.consume();
            end_span = self.expect(Token::RBrack)?;
            ty = Type::Array {
                elem_type: Box::new(ty),
            }
        }
        Ok((ty, span_ty.merge(&end_span)))
    }

    fn parse_block(&mut self) -> PResult<Vec<Statement>> {
        let mut stmts = Vec::new();
        while *self.peek() != Token::RBrace && !self.is_eof() {
            let stmt = self.parse_statement()?;
            stmts.push(stmt);
        }
        Ok(stmts)
    }
    // len: int = 100
    // 2 n’: int = -1
    // 3 debug: bool = false
    // and all the array stuff
    fn parse_variable_declaration(&mut self) -> PResult<VarDeclaration> {
        let (name, span_name) = self.expect_id()?;
        self.expect(Token::Colon)?;
        let (base_ty, _span_ty) = self.parse_prim_type()?;

        // Check for array dimensions
        let mut dims = Vec::new();
        while *self.peek() == Token::LBrack {
            self.consume(); // [
            let dim_expr = if *self.peek() != Token::RBrack {
                Some(self.parse_expression(0)?) // parse e.g. 3, n, etc.
            } else {
                None
            };
            self.expect(Token::RBrack)?;
            dims.push(dim_expr);
        }

        // Base_ty + Number of dims = array type
        let mut var_type = base_ty;
        for _ in 0..dims.len() {
            var_type = Type::Array {
                elem_type: Box::new(var_type),
            };
        }

        // Check initializer
        let init = match self.peek() {
            Token::Equal => {
                self.consume();
                Some(self.parse_expression(0)?)
            }
            _ => None,
        };
        Ok(VarDeclaration {
            name,
            init,
            var_type,
            dims,
            span: span_name,
        })
    }

    /// Parses a single statement.
    ///
    /// Statement parsing is the most involved part of the parser because
    /// assignments, variable declarations, and procedure calls all start with
    /// an identifier, so the parser must look ahead to distinguish them.
    pub fn parse_statement(&mut self) -> PResult<Statement> {
        match self.peek() {
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            Token::Return => self.parse_return(),
            Token::Var(_) | Token::Int(_) => {
                // could be assignment or declaration or prcedure call.
                match self.peek_n(1) {
                    Token::LParen => self.parse_procedure(),
                    // If it is multi assign then it can contain variable declartions!
                    // a :int, b = 2, 4 is a multi assign!
                    // a.field = 2 a = 4 b, c[1] = 5 a, a[1] = 3, 4 a : int, b = 2, 3
                    //
                    _ => {
                        // Assign or declaration
                        let mut lvals = self.parse_lvals()?;
                        // If only one element and its declaratoin then we have a normal declaration and try to parse an initialzier
                        if lvals.len() == 1 && matches!(&lvals[0], LVal::V(_)) {
                            // parse initialzer
                            if let LVal::V(mut vd) = lvals.pop().unwrap() {
                                let (init, init_span) = match self.peek() {
                                    Token::Equal => {
                                        self.consume();
                                        let expr = self.parse_expression(0)?;
                                        let expr_span = expr.span.clone();
                                        (Some(expr), expr_span)
                                    }
                                    _ => (None, vd.span.clone()),
                                };
                                self.expect(Token::Semicolon)?;

                                vd.init = init;
                                // println!("Full Local : {:?}", vd);
                                let vd_span = vd.span.clone();
                                let kind = StatementKind::LocalDecl(vd);
                                Ok(Statement {
                                    kind,
                                    span: vd_span.merge(&init_span),
                                })
                            } else {
                                unreachable!("Should not be possible")
                            }
                        } else {
                            // Need to parse equal and rhs
                            self.expect(Token::Equal)?;
                            // Relies on Semicolon
                            let rhs = self.parse_sep_list_comma(Token::Semicolon, |p| {
                                p.parse_expression(0)
                            })?;
                            self.expect(Token::Semicolon)?;
                            let span_start = lvals.first().unwrap().span();
                            let span_last = rhs.last().unwrap().span.clone();
                            let kind = StatementKind::Assign { lhs: lvals, rhs };
                            Ok(Statement {
                                kind,
                                span: span_start.merge(&span_last),
                            })
                        }
                    }
                }
            }
            // { block }
            Token::LBrace => {
                let span_start = self.expect(Token::LBrace)?;
                let block = self.parse_block()?;
                let span_end = self.expect(Token::RBrace)?;
                // The lexer only inserts the terminating semicolon when a newline
                // follows `}`, so in `} else {` there is none.
                if *self.peek() != Token::Else {
                    self.expect(Token::Semicolon)?;
                }
                let kind = StatementKind::Compound(Block { stmts: block });
                Ok(Statement {
                    kind,
                    span: span_start.merge(&span_end),
                })
            }
            _ => {
                // `.`, `(`, `[` and the binary operators can only continue an
                // expression, so seeing one here means the previous line was
                // ended by an inserted semicolon.
                let tok = self.peek().clone();
                let continues_expr = self.peek_binop().is_some()
                    || matches!(tok, Token::Dot | Token::LParen | Token::LBrack);
                let msg = if continues_expr {
                    "A statement cannot start with this token. The line break above ended the \
                     previous statement. To continue that expression instead, move this token \
                     to the end of the previous line"
                        .to_string()
                } else {
                    "Unexpected Token".to_string()
                };
                Err(ParserError::UnexpectedToken(msg, tok, self.peek_span()))
            }
        }
    }

    fn parse_return(&mut self) -> PResult<Statement> {
        let span_key = self.expect(Token::Return)?;
        // Relies on Semicolon
        let es = self.parse_sep_list_comma(Token::Semicolon, |p| p.parse_expression(0))?;
        self.expect(Token::Semicolon)?;
        let kind = StatementKind::Return(es);
        Ok(Statement {
            kind,
            span: span_key,
        })
    }

    fn parse_lvals(&mut self) -> PResult<Vec<LVal>> {
        let ps = |p: &mut Self| {
            if p.peek_n(1) == Token::Colon {
                // Found a var declaration
                let decl = p.parse_local_decl()?;
                Ok(decl)
            } else {
                // Hopefully assignable expression like a var or a[1]
                let e = p.parse_expression(0)?;
                Ok(LVal::E(e))
            }
        };

        self.parse_sep_list_comma(Token::Equal, ps)
    }
    // a : int b : int[2][] Local and array Declaration b : int[2] = "ab" allowed!
    // However, here we only parse b:int[2] since this can be part of an multi assignment or normal declaration
    fn parse_local_decl(&mut self) -> PResult<LVal> {
        let (name, span_name) = self.expect_id()?;
        self.expect(Token::Colon)?;
        let (base_ty, base_span) = self.parse_prim_type()?;

        // Check for array dimensions
        let mut dims = Vec::new();
        let mut span_end = base_span;
        while *self.peek() == Token::LBrack {
            self.consume(); // [
            let dim_expr = if *self.peek() != Token::RBrack {
                Some(self.parse_expression(0)?) // parse e.g. 3, n, etc.
            } else {
                None
            };
            span_end = self.expect(Token::RBrack)?;
            dims.push(dim_expr);
        }

        // Base_ty + Number of dims = array type
        let mut var_type = base_ty;
        for _ in 0..dims.len() {
            var_type = Type::Array {
                elem_type: Box::new(var_type),
            };
        }

        Ok(LVal::V(VarDeclaration {
            name,
            init: None,
            var_type,
            dims,
            span: span_name.merge(&span_end),
        }))
    }

    fn parse_procedure(&mut self) -> PResult<Statement> {
        debug!("Parsing procedure");
        let (name, span_name) = self.expect_id()?;
        self.expect(Token::LParen)?;
        let args = self.parse_sep_list_comma(Token::RParen, |p| p.parse_expression(0))?;
        let span_end = self.expect(Token::RParen)?;
        self.expect(Token::Semicolon)?;
        let kind = StatementKind::Procedure { name, args };
        Ok(Statement {
            kind,
            span: span_name.merge(&span_end),
        })
    }

    fn parse_while(&mut self) -> PResult<Statement> {
        let span_key = self.expect(Token::While)?;
        let guard = self.parse_expression(0)?;
        // Consume optional semicolon that lexer inserted after single-line condition
        if *self.peek() == Token::Semicolon {
            self.consume();
        }
        let body = Box::new(self.parse_statement()?);
        let kind = StatementKind::While { guard, body };
        Ok(Statement {
            kind,
            span: span_key,
        })
    }

    fn parse_if(&mut self) -> PResult<Statement> {
        let span_key = self.expect(Token::If)?;
        let guard = self.parse_expression(0)?;
        // Consume optional semicolon that lexer inserted after single-line condition
        if *self.peek() == Token::Semicolon {
            self.consume();
        }
        let then_br = Box::new(self.parse_statement()?);
        let else_br = if *self.peek() == Token::Else {
            self.consume();
            Some(Box::new(self.parse_statement()?))
        } else {
            None
        };
        let kind = StatementKind::If {
            guard,
            then_br,
            else_br,
        };
        Ok(Statement {
            kind,
            span: span_key,
        })
    }

    /// Parses an expression using precedence climbing (Pratt parsing).
    ///
    /// `min_prec` is the minimum precedence level that the parsed expression must have.
    /// Callers start with `0` for a full expression; recursive calls pass `prec + 1`
    /// to enforce left-to-right associativity.
    #[instrument(skip(self))]
    pub fn parse_expression(&mut self, min_prec: usize) -> PResult<Expression> {
        debug!("Current Token : {}", self.peek());
        let mut lhs = self.parse_unary()?;

        while let Some(op) = self.peek_binop() {
            let prec = self.binop_precedence(&op);
            if prec < min_prec {
                break;
            }
            self.consume(); // consume operator
            let rhs = self.parse_expression(prec + 1)?;
            let rhs_span = rhs.span.clone();
            let lhs_span = lhs.span.clone();
            let kind = ExpressionKind::Binary(op, Box::new(lhs), Box::new(rhs));
            lhs = Expression {
                kind,
                span: lhs_span.merge(&rhs_span),
            }
        }
        Ok(lhs)
    }
    /// Parses a unary expression (`-x`, `!cond`).
    ///
    /// Unary operators bind tighter than any binary operator (precedence 10).
    /// If no unary operator is found, falls through to [`Self::parse_postfix`].
    fn parse_unary(&mut self) -> PResult<Expression> {
        match self.peek() {
            Token::Minus => {
                let (_, span_key) = self.consume();
                let expr = self.parse_expression(10)?;
                let expr_span = expr.span.clone();
                let kind = ExpressionKind::Unary(UnaryOp::Neg, Box::new(expr));
                Ok(Expression {
                    kind,
                    span: span_key.merge(&expr_span),
                })
            }
            Token::Bang => {
                let (_, span_key) = self.consume();
                let expr = self.parse_expression(10)?;
                let expr_span = expr.span.clone();
                let kind = ExpressionKind::Unary(UnaryOp::Not, Box::new(expr));
                Ok(Expression {
                    kind,
                    span: span_key.merge(&expr_span),
                })
            }
            _ => self.parse_postfix(),
        }
    }

    /// Parses postfix operations: subscript (`a[i]`), field access (`a.x`),
    /// and function calls (`f(args)`).
    ///
    /// These can be chained arbitrarily, e.g. `a[1].field` or `matrix[i][j]`.
    fn parse_postfix(&mut self) -> PResult<Expression> {
        let mut expr = self.parse_primary()?;
        // loop for a[1].field etc.
        loop {
            match self.peek() {
                //a[]
                Token::LBrack => {
                    self.consume();
                    let idx = self.parse_expression(0)?;
                    let span_end = self.expect(Token::RBrack)?;
                    let expr_span = expr.span.clone();
                    let kind = ExpressionKind::Subscript(Box::new(expr), Box::new(idx));
                    expr = Expression {
                        kind,
                        span: expr_span.merge(&span_end),
                    }
                }
                // a.field
                Token::Dot => {
                    self.consume();
                    let (field, span_end) = self.expect_id()?;
                    let expr_span = expr.span.clone();
                    let kind = ExpressionKind::Dot(Box::new(expr), field);
                    expr = Expression {
                        kind,
                        span: expr_span.merge(&span_end),
                    }
                }
                // function call
                Token::LParen => {
                    self.consume();
                    let args =
                        self.parse_sep_list_comma(Token::RParen, |p| p.parse_expression(0))?;
                    let span_end = self.expect(Token::RParen)?;
                    if let ExpressionKind::Var(name) = expr.kind {
                        let kind = ExpressionKind::Call { name, args };
                        expr = Expression {
                            kind,
                            span: expr.span.merge(&span_end),
                        }
                    } else {
                        return Err(ParserError::InvalidExpression(
                            "Identifer Expected before Function Call".to_string(),
                            expr.kind,
                            expr.span,
                        ));
                    }
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// Tries to interpret the current token as a binary operator.
    ///
    /// Returns `None` if the current token is not a binary operator,
    /// which signals [`parse_expression`](Self::parse_expression) to stop climbing.
    fn peek_binop(&mut self) -> Option<BinOp> {
        match self.peek() {
            Token::Plus => Some(BinOp::Add),
            Token::Minus => Some(BinOp::Sub),
            Token::Asterisk => Some(BinOp::Mult),
            Token::Div => Some(BinOp::Div),
            Token::Mod => Some(BinOp::Mod),
            Token::And => Some(BinOp::And),
            Token::Bar => Some(BinOp::Or),
            Token::LessThan => Some(BinOp::Lt),
            Token::LessEqual => Some(BinOp::Leq),
            Token::GreaterThan => Some(BinOp::Gt),
            Token::GreaterEqual => Some(BinOp::Geq),
            Token::DoubleEquals => Some(BinOp::Eq),
            Token::NotEquals => Some(BinOp::Neq),
            _ => None,
        }
    }
    /// Returns the precedence level of a binary operator.
    /// Higher values mean tighter binding.
    fn binop_precedence(&self, op: &BinOp) -> usize {
        match op {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Neq => 3,
            BinOp::Lt | BinOp::Leq | BinOp::Gt | BinOp::Geq => 4,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Mult | BinOp::Div | BinOp::Mod => 6,
        }
    }

    /// Parses a comma-separated list of items, stopping before `end`.
    ///
    /// Used for function arguments, return values, and other comma-delimited sequences.
    /// The `end` token is **not** consumed.
    fn parse_sep_list_comma<F, T>(&mut self, end: Token, mut f: F) -> PResult<Vec<T>>
    where
        F: FnMut(&mut Self) -> PResult<T>,
    {
        let mut elems = Vec::new();
        if *self.peek() == end {
            return Ok(elems);
        }

        elems.push(f(self)?);
        while *self.peek() == Token::Comma && !self.is_eof() {
            self.consume();
            let e = f(self)?;
            elems.push(e);
            if *self.peek() == end {
                break;
            }
        }
        Ok(elems)
    }

    /// Like [`Self::parse_sep_list_comma`] but allows a trailing comma before `end`.
    fn parse_sep_list_comma_trailing<F, T>(&mut self, end: Token, mut f: F) -> PResult<Vec<T>>
    where
        F: FnMut(&mut Self) -> PResult<T>,
    {
        let mut elems = Vec::new();

        if *self.peek() == end {
            return Ok(elems);
        }

        while !self.is_eof() {
            elems.push(f(self)?);

            if *self.peek() == Token::Comma {
                self.consume();
                if *self.peek() == end {
                    break; // trailing comma before end is fine
                }
            } else {
                break;
            }
        }

        Ok(elems)
    }

    /// Parses a primary (atomic) expression: literals, identifiers, parenthesized
    /// expressions, array literals (`{1, 2, 3}`), and string literals.
    fn parse_primary(&mut self) -> PResult<Expression> {
        let (tok, span_tok) = self.consume();
        let (kind, span) = match tok {
            Token::Int(num) => (ExpressionKind::Int(num), span_tok),
            Token::Var(name) => (ExpressionKind::Var(name), span_tok),
            Token::True => (ExpressionKind::Bool(true), span_tok),
            Token::False => (ExpressionKind::Bool(false), span_tok),
            Token::LParen => {
                let expr = self.parse_expression(0)?;
                let span_rparen = self.expect(Token::RParen)?;
                (expr.kind, span_tok.merge(&span_rparen))
            }
            Token::LBrace => {
                let expr = self.parse_array_literal()?;
                (expr.kind, expr.span.merge(&span_tok))
            }
            Token::StringLiteral(lit) => (self.parse_string_literal(&lit), span_tok),
            tok => {
                return Err(ParserError::UnexpectedToken(
                    "Primary Expression not possible!".to_string(),
                    tok,
                    span_tok,
                ));
            }
        };
        Ok(Expression { kind, span })
    }

    fn parse_array_literal(&mut self) -> PResult<Expression> {
        let elems = self.parse_sep_list_comma_trailing(Token::RBrace, |p| p.parse_expression(0))?;
        let span_end = self.expect(Token::RBrace)?;
        Ok(Expression {
            kind: ExpressionKind::ArrayLit(elems),
            span: span_end,
        })
    }

    /// Parses a string literal into an array of integer character codes.
    ///
    /// ETA represents strings as `int[]`, so `"Hello"` becomes
    /// `{72, 101, 108, 108, 111}`.
    fn parse_string_literal(&mut self, lit: &str) -> ExpressionKind {
        // Dummy span because makes no sense to have that internal
        let elems = lit
            .chars()
            .map(|ch| Expression {
                kind: ExpressionKind::Int(ch as i64),
                span: logos::Span { start: 0, end: 0 },
            })
            .collect();
        ExpressionKind::ArrayLit(elems)
    }
}
