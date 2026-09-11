//! Lexical analysis for the ETA language.
//!
//! This module wraps the [`logos`]-generated token stream into a [`Lexer`] that implements
//! [`Iterator`]. The key addition over a raw logos lexer is **automatic semicolon insertion**:
//! newlines are converted into [`Token::Semicolon`] when the preceding token could end
//! a statement (similar to how Go handles semicolons).
//!
//! The [`Lexer`] is consumed by the [`Parser`](crate::parser::Parser), which wraps it in
//! a [`PeekMoreIterator`](peekmore::PeekMoreIterator) for bounded lookahead.

use crate::token::{LexicalError, Token};
use logos::{Logos, Span, SpannedIter};

/// The item type yielded by the [`Lexer`] iterator.
///
/// Each item is either a `(Token, Span)` pair on success, or a [`LexicalError`].
pub type LexerItem = Result<(Token, Span), LexicalError>;

/// A lexer for the ETA language that wraps a [`logos::SpannedIter`].
pub struct Lexer<'input> {
    token_stream: SpannedIter<'input, Token>,
    last_tok: Token,
    pub input_length: usize,
    // If one last semicolon is needed
    eof_emitted: bool,
}

impl<'input> Lexer<'input> {
    pub fn new(input: &'input str) -> Self {
        // the Token::lexer() method is provided by the Logos trait
        Self {
            token_stream: Token::lexer(input).spanned(),
            input_length: input.len(),
            last_tok: Token::Eof,
            eof_emitted: false,
        }
    }
}

/// Returns `true` if `tok` can end a statement, meaning a newline after it
/// should be treated as a semicolon.
///
/// For example, `x` in `x = 5` or `)` in `foo()` are candidates,
/// while `+` or `(` are not.
fn is_semicolon_candidate(tok: &Token) -> bool {
    matches!(
        tok,
        Token::Var(_)
            | Token::Int(_)
            | Token::True
            | Token::False
            | Token::Return
            | Token::RParen
            | Token::RBrace
            | Token::TInt
            | Token::TBool
            | Token::RBrack
            | Token::StringLiteral(_)
    )
}

/// Convenience function that collects all tokens from the input into a `Vec`.
pub fn tokenize(input: &str) -> Vec<Result<(Token, Span), LexicalError>> {
    let lexer = Lexer::new(input);
    lexer.collect()
}

impl<'input> Iterator for Lexer<'input> {
    type Item = LexerItem;

    /// Returns the next token, performing semicolon insertion as needed.
    ///
    /// Newline tokens are never yielded directly. Instead, they are either
    /// converted to [`Token::Semicolon`] (if the previous token is a
    /// semicolon candidate) or silently skipped.
    fn next(&mut self) -> Option<Self::Item> {
        // Loop to find consecutive empty lines and remove them
        // Other empty lines are needed for semicolon placement
        loop {
            let next_item = self.token_stream.next(); // unwrap Result<(Token, Span), _>
            match next_item {
                Some((token_res, span)) => {
                    let token = token_res.ok()?;

                    // One last semicolon before eof if newline was omitted
                    if token == Token::Eof && self.last_tok != Token::Eof {
                        self.last_tok = Token::Eof;
                        return Some(Ok((Token::Semicolon, span)));
                    }
                    // Important because we only add semicolon if line was not completly empty!
                    if token == Token::Newline {
                        if is_semicolon_candidate(&self.last_tok) {
                            // Insert a semicolon
                            self.last_tok = Token::Newline;
                            return Some(Ok((Token::Semicolon, span)));
                        } else {
                            // If it is not a semicolon candidate then we skip over it
                            continue;
                        }
                    }
                    self.last_tok = token.clone();
                    return Some(Ok((token, span)));
                }
                _ => {
                    if !self.eof_emitted {
                        self.eof_emitted = true;
                        if is_semicolon_candidate(&self.last_tok) {
                            return Some(Ok((Token::Semicolon, Span { start: 0, end: 0 })));
                        }
                    } else {
                        return None;
                    }
                }
            }
        }
    }
}
