//! Recursive-descent parser. The tree mirrors `scene::named_parameters::Expr`
//! with point literals, unit-suffixed numbers and `ans` added, and every node
//! keeps the span of the text it came from so errors can point at it.

use std::ops::Range;

use super::lexer::{TokKind, Token};
use super::units::{self, Unit};
use super::CalcError;

/// Parser stack frames, not nesting levels: each parenthesis level descends
/// about five methods (sum→product→unary→power→primary), so 256 frames is
/// roughly 50 nested parentheses — far past anything typed by hand, while
/// keeping the stack bounded.
const MAX_RECURSION: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Expr {
    pub kind: ExprKind,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ExprKind {
    Number(f64),
    /// A number with a unit suffix, converted when evaluated.
    Quantity(f64, Unit),
    /// A built-in name: `pi`, `e`.
    Ref(String),
    /// `$name`, lowercased.
    Var(String),
    Ans,
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    /// `a mod b`: the remainder with the sign of `b`.
    Mod(Box<Expr>, Box<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Point(Vec<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Statement {
    Expr(Expr),
    Assign {
        name: String,
        name_span: Range<usize>,
        value: Expr,
    },
}

pub(super) fn parse(tokens: &[Token], input_len: usize) -> Result<Statement, CalcError> {
    if tokens.is_empty() {
        return Err(CalcError::new(0..input_len, crate::t!("empty expression")));
    }
    let mut p = Parser {
        tokens,
        pos: 0,
        input_len,
    };
    let statement = match (tokens.first(), tokens.get(1)) {
        (
            Some(Token {
                kind: TokKind::Var(name),
                span,
            }),
            Some(Token {
                kind: TokKind::Eq, ..
            }),
        ) => {
            p.pos = 2;
            Statement::Assign {
                name: name.clone(),
                name_span: span.clone(),
                value: p.expr(0)?,
            }
        }
        (
            Some(Token {
                kind: TokKind::Ident(name),
                span,
            }),
            Some(Token {
                kind: TokKind::Eq, ..
            }),
        ) => {
            return Err(CalcError::new(
                span.clone(),
                crate::tf!(
                    "Variable names start with $: ${} = …",
                    name.to_ascii_lowercase()
                ),
            ));
        }
        _ => Statement::Expr(p.expr(0)?),
    };
    if let Some(t) = tokens.get(p.pos) {
        return Err(p.unexpected(t));
    }
    Ok(statement)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    input_len: usize,
}

fn node(kind: ExprKind, span: Range<usize>) -> Expr {
    Expr { kind, span }
}

fn binary(op: fn(Box<Expr>, Box<Expr>) -> ExprKind, lhs: Expr, rhs: Expr) -> Expr {
    let span = lhs.span.start.min(rhs.span.start)..rhs.span.end;
    node(op(Box::new(lhs), Box::new(rhs)), span)
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a TokKind> {
        self.tokens.get(self.pos).map(|t| &t.kind)
    }

    fn peek_token(&self) -> Option<&'a Token> {
        self.tokens.get(self.pos)
    }

    fn end_span(&self) -> Range<usize> {
        self.input_len..self.input_len
    }

    fn unexpected(&self, t: &Token) -> CalcError {
        CalcError::new(
            t.span.clone(),
            crate::tf!("unexpected '{}'", describe(&t.kind)),
        )
    }

    fn deeper(&self, depth: usize) -> Result<usize, CalcError> {
        if depth >= MAX_RECURSION {
            let span = self
                .peek_token()
                .map_or_else(|| self.end_span(), |t| t.span.clone());
            return Err(CalcError::new(
                span,
                crate::t!("expression is too deeply nested"),
            ));
        }
        Ok(depth + 1)
    }

    fn expect_close(&mut self, close: TokKind, open: Range<usize>) -> Result<usize, CalcError> {
        match self.peek_token() {
            Some(t) if t.kind == close => {
                self.pos += 1;
                Ok(t.span.end)
            }
            found => {
                let message = if close == TokKind::RParen {
                    crate::t!("missing ')'")
                } else {
                    crate::t!("missing ']'")
                };
                let end = found.map_or(self.input_len, |t| t.span.start);
                Err(CalcError::new(open.start..end.max(open.end), message))
            }
        }
    }

    // sum → product (('+' | '-') product)*
    fn expr(&mut self, depth: usize) -> Result<Expr, CalcError> {
        let depth = self.deeper(depth)?;
        let mut lhs = self.product(depth)?;
        loop {
            let op: fn(_, _) -> _ = match self.peek() {
                Some(TokKind::Plus) => ExprKind::Add,
                Some(TokKind::Minus) => ExprKind::Sub,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.product(depth)?;
            lhs = binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    // product → unary (('*' | '/' | 'mod') unary)*
    fn product(&mut self, depth: usize) -> Result<Expr, CalcError> {
        let depth = self.deeper(depth)?;
        let mut lhs = self.unary(depth)?;
        loop {
            let op: fn(_, _) -> _ = match self.peek() {
                Some(TokKind::Star) => ExprKind::Mul,
                Some(TokKind::Slash) => ExprKind::Div,
                Some(TokKind::Ident(name)) if is_mod(name) => ExprKind::Mod,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.unary(depth)?;
            lhs = binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    // unary → ('-' | '+') unary | power
    fn unary(&mut self, depth: usize) -> Result<Expr, CalcError> {
        let depth = self.deeper(depth)?;
        match self.peek_token() {
            Some(t) if t.kind == TokKind::Minus => {
                let start = t.span.start;
                self.pos += 1;
                let inner = self.unary(depth)?;
                let span = start..inner.span.end;
                Ok(node(ExprKind::Neg(Box::new(inner)), span))
            }
            Some(t) if t.kind == TokKind::Plus => {
                self.pos += 1;
                self.unary(depth)
            }
            _ => self.power(depth),
        }
    }

    // power → primary ('^' unary)?  — right-associative, and `-2^2` is -4.
    fn power(&mut self, depth: usize) -> Result<Expr, CalcError> {
        let depth = self.deeper(depth)?;
        let base = self.primary(depth)?;
        if self.peek() == Some(&TokKind::Caret) {
            self.pos += 1;
            let exponent = self.unary(depth)?;
            return Ok(binary(ExprKind::Pow, base, exponent));
        }
        Ok(base)
    }

    fn primary(&mut self, depth: usize) -> Result<Expr, CalcError> {
        let depth = self.deeper(depth)?;
        let Some(t) = self.peek_token().cloned() else {
            return Err(CalcError::new(
                self.end_span(),
                crate::t!("unexpected end of expression"),
            ));
        };
        match &t.kind {
            // A leading `* / ^` continues the previous result: `*2` is `ans*2`.
            TokKind::Star | TokKind::Slash | TokKind::Caret if self.pos == 0 => {
                Ok(node(ExprKind::Ans, t.span.clone()))
            }
            TokKind::Number(n) => {
                self.pos += 1;
                self.quantity(*n, t.span.clone())
            }
            TokKind::Ident(name) => {
                self.pos += 1;
                if self.peek() == Some(&TokKind::LParen) {
                    let open = self.tokens[self.pos].span.clone();
                    self.pos += 1;
                    let args = self.list(TokKind::RParen, depth)?;
                    let end = self.expect_close(TokKind::RParen, open)?;
                    return Ok(node(ExprKind::Call(name.clone(), args), t.span.start..end));
                }
                let kind = if name.eq_ignore_ascii_case("ans") {
                    ExprKind::Ans
                } else {
                    ExprKind::Ref(name.clone())
                };
                Ok(node(kind, t.span.clone()))
            }
            TokKind::Var(name) => {
                self.pos += 1;
                Ok(node(ExprKind::Var(name.clone()), t.span.clone()))
            }
            TokKind::LParen => {
                self.pos += 1;
                let inner = self.expr(depth)?;
                let end = self.expect_close(TokKind::RParen, t.span.clone())?;
                Ok(node(inner.kind, t.span.start..end))
            }
            TokKind::LBracket => {
                self.pos += 1;
                let coords = self.list(TokKind::RBracket, depth)?;
                let end = self.expect_close(TokKind::RBracket, t.span.clone())?;
                let span = t.span.start..end;
                if !(2..=3).contains(&coords.len()) {
                    return Err(CalcError::new(
                        span,
                        crate::t!("a point needs 2 or 3 coordinates"),
                    ));
                }
                Ok(node(ExprKind::Point(coords), span))
            }
            _ => Err(self.unexpected(&t)),
        }
    }

    /// Comma-separated expressions up to (not including) `close`.
    fn list(&mut self, close: TokKind, depth: usize) -> Result<Vec<Expr>, CalcError> {
        let mut items = Vec::new();
        if self.peek() == Some(&close) {
            return Ok(items);
        }
        loop {
            items.push(self.expr(depth)?);
            if self.peek() == Some(&TokKind::Comma) {
                self.pos += 1;
            } else {
                return Ok(items);
            }
        }
    }

    /// A number and the unit suffix after it, if any: `3.6m`, `90 deg`,
    /// `2'`, `9"`, and feet-inch pairs like `2'9"` (2 ft + 9 in).
    fn quantity(&mut self, value: f64, span: Range<usize>) -> Result<Expr, CalcError> {
        let Some(t) = self.peek_token().cloned() else {
            return Ok(node(ExprKind::Number(value), span));
        };
        let unit = match &t.kind {
            TokKind::Feet => units::lookup("'"),
            TokKind::Inches => units::lookup("\""),
            // `5 mod 3`: the operator, not a unit.
            TokKind::Ident(name) if is_mod(name) => None,
            TokKind::Ident(name) => match units::lookup(name) {
                Some(unit) => Some(unit),
                // `2x` reads as a typo'd unit; `2 x` as a stray name.
                None if t.span.start == span.end => {
                    return Err(CalcError::new(
                        t.span.clone(),
                        crate::tf!("unknown unit '{}'", name),
                    ));
                }
                None => None,
            },
            _ => None,
        };
        let Some(unit) = unit else {
            return Ok(node(ExprKind::Number(value), span));
        };
        self.pos += 1;
        let feet = node(ExprKind::Quantity(value, unit), span.start..t.span.end);
        if t.kind != TokKind::Feet {
            return Ok(feet);
        }
        // `2'9"` or `2'-9"`: a number with an inch mark after the feet, the
        // dash between them a separator, not a subtraction.
        let dash = usize::from(matches!(
            self.tokens.get(self.pos),
            Some(Token {
                kind: TokKind::Minus,
                ..
            })
        ));
        if let (
            Some(Token {
                kind: TokKind::Number(inches),
                span: n_span,
            }),
            Some(Token {
                kind: TokKind::Inches,
                span: i_span,
            }),
        ) = (
            self.tokens.get(self.pos + dash),
            self.tokens.get(self.pos + dash + 1),
        ) {
            let inch = node(
                ExprKind::Quantity(*inches, units::lookup("\"").expect("inch unit")),
                n_span.start..i_span.end,
            );
            self.pos += dash + 2;
            return Ok(binary(ExprKind::Add, feet, inch));
        }
        Ok(feet)
    }
}

fn is_mod(name: &str) -> bool {
    name.eq_ignore_ascii_case("mod")
}

/// A token as written, for error messages.
fn describe(kind: &TokKind) -> String {
    match kind {
        TokKind::Number(n) => n.to_string(),
        TokKind::Ident(name) => name.clone(),
        TokKind::Var(name) => format!("${name}"),
        TokKind::Plus => "+".into(),
        TokKind::Minus => "-".into(),
        TokKind::Star => "*".into(),
        TokKind::Slash => "/".into(),
        TokKind::Caret => "^".into(),
        TokKind::LParen => "(".into(),
        TokKind::RParen => ")".into(),
        TokKind::LBracket => "[".into(),
        TokKind::RBracket => "]".into(),
        TokKind::Comma => ",".into(),
        TokKind::Eq => "=".into(),
        TokKind::Feet => "'".into(),
        TokKind::Inches => "\"".into(),
    }
}
