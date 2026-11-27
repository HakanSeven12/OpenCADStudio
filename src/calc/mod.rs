//! The calculator engine behind CAL and QuickCalc: arithmetic, points,
//! variables, `ans`, unit suffixes and a small function library.
//!
//! Self-contained on purpose — no UI and no scene access, so every surface
//! that evaluates typed input can share it.

mod eval;
mod functions;
mod lexer;
mod parser;
mod units;
mod value;

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

pub use value::{AngleMode, CalcContext, SysVar, SysVars, Value};

/// Longer input is refused outright. It keeps the syntax tree (whose depth
/// a long `1+1+…` chain grows) far from the stack limit; nobody types an
/// expression this long.
const MAX_INPUT_LEN: usize = 1024;

/// What went wrong, and where: `span` is the byte range of the offending
/// text in the input, for underlining.
#[derive(Debug, Clone, PartialEq)]
pub struct CalcError {
    pub span: Range<usize>,
    pub message: String,
}

impl CalcError {
    fn new(span: Range<usize>, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CalcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn parse(input: &str) -> Result<parser::Statement, CalcError> {
    if input.len() > MAX_INPUT_LEN {
        return Err(CalcError::new(
            0..input.len(),
            crate::t!("expression is too long"),
        ));
    }
    let input = decimal_comma(input);
    let tokens = lexer::tokenize(&input)?;
    parser::parse(&tokens, input.len())
}

/// Evaluate one line: an expression, or `$name = expression`. On success
/// the result becomes `ans`, and an assignment stores the variable.
pub fn evaluate(input: &str, ctx: &mut CalcContext) -> Result<Value, CalcError> {
    let (target, expr) = match parse(input)? {
        parser::Statement::Expr(expr) => (None, expr),
        parser::Statement::Assign { name, value, .. } => (Some(name), value),
    };
    let value = eval::eval(&expr, ctx)?;
    if let Some(name) = target {
        ctx.vars.insert(name, value);
    }
    ctx.ans = Some(value);
    Ok(value)
}

/// What [`evaluate`] would return for `input`, changing nothing: an
/// assignment's value without storing it, and no `ans`.
pub fn result_of(input: &str, ctx: &CalcContext) -> Result<Value, CalcError> {
    match parse(input)? {
        parser::Statement::Expr(expr) | parser::Statement::Assign { value: expr, .. } => {
            eval::eval(&expr, ctx)
        }
    }
}

/// The value of an expression, changing nothing: no `ans`, and an
/// assignment is refused (a variable's value or function is an expression,
/// not a `$name = …` line).
pub fn value_of(input: &str, ctx: &CalcContext) -> Result<Value, CalcError> {
    match parse(input)? {
        parser::Statement::Expr(expr) => eval::eval(&expr, ctx),
        parser::Statement::Assign { name_span, .. } => Err(CalcError::new(
            name_span,
            crate::t!("Leave out the $name = part"),
        )),
    }
}

/// The variable `input` assigns (`$name = …`), lowercased, and its span.
pub fn assigned_variable(input: &str) -> Option<(String, Range<usize>)> {
    let tokens = lexer::tokenize(input).ok()?;
    match (tokens.first(), tokens.get(1)) {
        (Some(first), Some(second)) if second.kind == lexer::TokKind::Eq => match &first.kind {
            lexer::TokKind::Var(name) => Some((name.clone(), first.span.clone())),
            _ => None,
        },
        _ => None,
    }
}

/// Whether `input` reads `$name`.
pub fn uses_variable(input: &str, name: &str) -> bool {
    lexer::tokenize(input).is_ok_and(|tokens| {
        tokens
            .iter()
            .any(|t| matches!(&t.kind, lexer::TokKind::Var(v) if v.eq_ignore_ascii_case(name)))
    })
}

/// A variable name, without its `$`: a letter or `_`, then letters, digits
/// or `_`.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// How a variable gets its value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Definition<'a> {
    /// Calculated once and stored.
    Constant(Value),
    /// An expression, recalculated whenever the variable is used.
    Function(&'a str),
}

/// Put `variables` into `ctx`: constants first, then functions in list
/// order, each calculated against what is already there, so a function may
/// use constants and earlier functions. A function that fails gets no value;
/// its error comes back by name.
pub fn define_variables<'a>(
    ctx: &mut CalcContext,
    variables: impl IntoIterator<Item = (&'a str, Definition<'a>)> + Clone,
) -> HashMap<String, CalcError> {
    for (name, definition) in variables.clone() {
        if let Definition::Constant(value) = definition {
            ctx.vars.insert(name.to_ascii_lowercase(), value);
        }
    }
    let mut errors = HashMap::new();
    for (name, definition) in variables {
        if let Definition::Function(expr) = definition {
            match value_of(expr, ctx) {
                Ok(value) => {
                    ctx.vars.insert(name.to_ascii_lowercase(), value);
                }
                Err(error) => {
                    errors.insert(name.to_ascii_lowercase(), error);
                }
            }
        }
    }
    errors
}

/// A lone `3,5` is 3.5, as at any value prompt — commas otherwise separate
/// coordinates and arguments. Same length, so error spans still line up.
fn decimal_comma(input: &str) -> Cow<'_, str> {
    let t = input.trim();
    let digits = t.strip_prefix(['-', '+']).unwrap_or(t);
    match digits.split_once(',') {
        Some((a, b))
            if !a.is_empty()
                && !b.is_empty()
                && a.bytes().all(|c| c.is_ascii_digit())
                && b.bytes().all(|c| c.is_ascii_digit()) =>
        {
            Cow::Owned(input.replacen(',', ".", 1))
        }
        _ => Cow::Borrowed(input),
    }
}
