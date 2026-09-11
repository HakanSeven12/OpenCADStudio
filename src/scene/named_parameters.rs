//! Named, formula-capable parameters stored per drawing.
//! Formulas are parsed locally so dependency cycles can be rejected before evaluation.

use super::Scene;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// A parsed formula. Ordinary arithmetic plus [`Expr::Ref`] (a reference to
/// another parameter by name) and [`Expr::Call`] (a function call). Only the
/// small builtin set in `call_builtin` is accepted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Number(f64),
    Ref(String),
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

impl Expr {
    /// Every parameter name this expression reads, deduplicated in
    /// first-seen order. This is the entire basis for cycle detection
    /// (`ParameterTable::set`): one dependency edge per name returned here.
    fn refs(&self, out: &mut Vec<String>) {
        match self {
            Expr::Number(_) => {}
            Expr::Ref(name) => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Expr::Neg(a) => a.refs(out),
            Expr::Add(a, b)
            | Expr::Sub(a, b)
            | Expr::Mul(a, b)
            | Expr::Div(a, b)
            | Expr::Pow(a, b) => {
                a.refs(out);
                b.refs(out);
            }
            Expr::Call(_, args) => {
                for arg in args {
                    arg.refs(out);
                }
            }
        }
    }
}

/// Everything that can go wrong parsing, defining, or resolving a
/// parameter. Every case is reported back to the caller (and, in later
/// stages, the UI) rather than panicking — matching this codebase's
/// established pattern of isolating a bad derived value instead of letting
/// it take down an unrelated computation (e.g. dangling-constraint handling
/// in `sketch_solve.rs`).
#[derive(Debug, Clone, PartialEq)]
pub enum ParamError {
    /// A formula's text doesn't parse. The `String` is a short, human-
    /// readable reason, not a machine-parseable code.
    Parse(String),
    /// Not a valid identifier, or collides with a builtin function name
    /// (`is_reserved_name`).
    InvalidName(String),
    /// Accepting this formula would close a circular reference right now.
    /// The path reads start-to-finish, e.g. `["a", "b", "a"]` for `a = b`,
    /// `b = a`.
    Cycle(Vec<String>),
    UnknownFunction(String),
    WrongArgCount {
        function: String,
        expected: usize,
        found: usize,
    },
    /// Raised by `resolve`/`resolve_all`, not `set` — a formula may
    /// legitimately reference a parameter that doesn't exist *yet* (order of
    /// definition shouldn't matter, mirroring a spreadsheet); this only
    /// surfaces once something actually tries to read the missing value.
    UndefinedReference(String),
    DivisionByZero,
}

impl fmt::Display for ParamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamError::Parse(msg) => write!(f, "formula error: {msg}"),
            ParamError::InvalidName(name) => write!(f, "'{name}' is not a valid parameter name"),
            ParamError::Cycle(path) => write!(f, "circular reference: {}", path.join(" -> ")),
            ParamError::UnknownFunction(name) => write!(f, "unknown function '{name}'"),
            ParamError::WrongArgCount {
                function,
                expected,
                found,
            } => {
                write!(
                    f,
                    "{function}() expects {expected} argument(s), found {found}"
                )
            }
            ParamError::UndefinedReference(name) => write!(f, "'{name}' is not defined"),
            ParamError::DivisionByZero => write!(f, "division by zero"),
        }
    }
}

impl std::error::Error for ParamError {}

/// The builtin function set. It also supplies the reserved-word list that
/// `is_reserved_name` checks a parameter name against, so a formula
/// like `sqrt(x)` is never ambiguous between "call the builtin" and
/// "multiply by a parameter named sqrt".
const BUILTIN_FUNCTIONS: &[&str] = &["sqrt", "abs", "pow", "min", "max"];

pub fn is_reserved_name(name: &str) -> bool {
    BUILTIN_FUNCTIONS.contains(&name)
}

/// A valid parameter name: starts with a letter or underscore, followed by
/// letters, digits, or underscores.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// A dimensional constraint's literal or named driving value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DrivingValue {
    Literal(f64),
    Named(String),
}

impl DrivingValue {
    /// Resolves to a number: a literal resolves to itself (never fails); a
    /// named reference resolves through `table`, which can fail the same
    /// ways any [`ParameterTable::resolve`] call can (undefined reference,
    /// division by zero — a cycle should be unreachable here since `set`
    /// already refuses to create one).
    pub fn resolve(&self, table: &ParameterTable) -> Result<f64, ParamError> {
        match self {
            DrivingValue::Literal(value) => Ok(*value),
            DrivingValue::Named(name) => table.resolve(name),
        }
    }
}

impl From<f64> for DrivingValue {
    fn from(value: f64) -> Self {
        DrivingValue::Literal(value)
    }
}

fn call_builtin(name: &str, args: &[f64]) -> Result<f64, ParamError> {
    fn check_arity(name: &str, args: &[f64], expected: usize) -> Result<(), ParamError> {
        if args.len() != expected {
            return Err(ParamError::WrongArgCount {
                function: name.to_string(),
                expected,
                found: args.len(),
            });
        }
        Ok(())
    }
    match name {
        "sqrt" => {
            check_arity(name, args, 1)?;
            Ok(args[0].sqrt())
        }
        "abs" => {
            check_arity(name, args, 1)?;
            Ok(args[0].abs())
        }
        "pow" => {
            check_arity(name, args, 2)?;
            Ok(args[0].powf(args[1]))
        }
        "min" => {
            check_arity(name, args, 2)?;
            Ok(args[0].min(args[1]))
        }
        "max" => {
            check_arity(name, args, 2)?;
            Ok(args[0].max(args[1]))
        }
        _ => Err(ParamError::UnknownFunction(name.to_string())),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
    Comma,
}

fn tokenize(src: &str) -> Result<Vec<Token>, ParamError> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '^' => {
                tokens.push(Token::Caret);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            _ if c.is_ascii_digit() || c == '.' => {
                let start = i;
                let mut seen_dot = c == '.';
                i += 1;
                while i < chars.len() {
                    let d = chars[i];
                    if d.is_ascii_digit() {
                        i += 1;
                    } else if d == '.' && !seen_dot {
                        seen_dot = true;
                        i += 1;
                    } else {
                        break;
                    }
                }
                // Scientific notation: 1e-3, 2.5E10. Only consumed when a
                // digit (optionally signed) actually follows the e/E, so a
                // bare trailing "e" is left for the identifier lexer instead
                // (not reachable today since no builtin/constant is named
                // "e", but keeps this branch from eating an identifier that
                // happens to start with it, e.g. a formula like `2 * ecc`).
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1;
                    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    if j < chars.len() && chars[j].is_ascii_digit() {
                        while j < chars.len() && chars[j].is_ascii_digit() {
                            j += 1;
                        }
                        i = j;
                    }
                }
                let text: String = chars[start..i].iter().collect();
                let value: f64 = text
                    .parse()
                    .map_err(|_| ParamError::Parse(format!("invalid number '{text}'")))?;
                tokens.push(Token::Number(value));
            }
            _ if c.is_alphabetic() || c == '_' => {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                tokens.push(Token::Ident(chars[start..i].iter().collect()));
            }
            other => return Err(ParamError::Parse(format!("unexpected character '{other}'"))),
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, expected: Token) -> Result<(), ParamError> {
        match self.bump() {
            Some(t) if t == expected => Ok(()),
            Some(t) => Err(ParamError::Parse(format!(
                "expected {expected:?}, found {t:?}"
            ))),
            None => Err(ParamError::Parse(format!(
                "expected {expected:?}, found end of formula"
            ))),
        }
    }

    // Precedence, low to high: expr (+ -) > term (* /) > unary (-) > power (^) > primary.
    // `^` binds tighter than unary minus on its left operand but is
    // right-associative on its own exponent (`parse_unary` recursing back
    // into `parse_power` covers `2^-2` == `2^(-2)`, and `2^3^2` ==
    // `2^(3^2)`).
    fn parse_expr(&mut self) -> Result<Expr, ParamError> {
        let mut lhs = self.parse_term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.pos += 1;
                    lhs = Expr::Add(Box::new(lhs), Box::new(self.parse_term()?));
                }
                Some(Token::Minus) => {
                    self.pos += 1;
                    lhs = Expr::Sub(Box::new(lhs), Box::new(self.parse_term()?));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_term(&mut self) -> Result<Expr, ParamError> {
        let mut lhs = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.pos += 1;
                    lhs = Expr::Mul(Box::new(lhs), Box::new(self.parse_unary()?));
                }
                Some(Token::Slash) => {
                    self.pos += 1;
                    lhs = Expr::Div(Box::new(lhs), Box::new(self.parse_unary()?));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParamError> {
        match self.peek() {
            Some(Token::Minus) => {
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.parse_unary()?)))
            }
            Some(Token::Plus) => {
                self.pos += 1;
                self.parse_unary()
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<Expr, ParamError> {
        let base = self.parse_primary()?;
        if let Some(Token::Caret) = self.peek() {
            self.pos += 1;
            let exponent = self.parse_unary()?;
            return Ok(Expr::Pow(Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParamError> {
        match self.bump() {
            Some(Token::Number(n)) => Ok(Expr::Number(n)),
            Some(Token::Ident(name)) => {
                if matches!(self.peek(), Some(Token::LParen)) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Token::RParen)) {
                        loop {
                            args.push(self.parse_expr()?);
                            if matches!(self.peek(), Some(Token::Comma)) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Token::RParen)?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Ref(name))
                }
            }
            Some(Token::LParen) => {
                let inner = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(inner)
            }
            Some(other) => Err(ParamError::Parse(format!("unexpected token {other:?}"))),
            None => Err(ParamError::Parse("unexpected end of formula".to_string())),
        }
    }
}

/// Parses a formula's text into an [`Expr`], without evaluating or
/// resolving any reference. `ParameterTable::set` is the usual entry point;
/// exposed standalone so a UI stage can validate-as-you-type without
/// touching the table.
pub fn parse(source: &str) -> Result<Expr, ParamError> {
    let tokens = tokenize(source)?;
    if tokens.is_empty() {
        return Err(ParamError::Parse("empty formula".to_string()));
    }
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
    };
    let expr = parser.parse_expr()?;
    if parser.pos != tokens.len() {
        return Err(ParamError::Parse(format!(
            "unexpected trailing token {:?}",
            tokens[parser.pos]
        )));
    }
    Ok(expr)
}

/// One named parameter: a name plus its formula, both the original typed
/// text (`source`, for redisplay/re-editing) and the parsed [`Expr`]
/// (`expr`, for evaluation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub source: String,
    pub expr: Expr,
}

/// Every named parameter for one document.
///
/// `parameters` is intentionally not `pub` (unlike the sibling
/// `SketchConstraintSet::constraints`): this table's entire value is that
/// every stored formula is already known to be parse-valid and cycle-free,
/// an invariant only `set`/`remove` maintain. A direct external push could
/// silently violate it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParameterTable {
    parameters: Vec<Parameter>,
}

impl ParameterTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.parameters.is_empty()
    }

    pub fn len(&self) -> usize {
        self.parameters.len()
    }

    /// Insertion order — the order parameters were first defined in, stable
    /// across edits to existing ones. The natural listing order for a
    /// future parameters panel.
    pub fn iter(&self) -> impl Iterator<Item = &Parameter> {
        self.parameters.iter()
    }

    pub fn get(&self, name: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.name == name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        self.parameters.iter().position(|p| p.name == name)
    }

    /// Defines a new parameter or redefines an existing one's formula (one
    /// upsert operation — there is no separate "must not already exist"
    /// entry point; a future UI stage that wants add-vs-rename semantics can
    /// check `contains` itself first).
    ///
    /// Rejects an invalid or reserved identifier, malformed formula, or
    /// circular reference. A reference
    /// to a parameter that doesn't exist *yet* is not rejected (order of
    /// definition shouldn't matter, mirroring a spreadsheet); it only
    /// surfaces as [`ParamError::UndefinedReference`] when [`Self::resolve`]
    /// actually needs that value.
    pub fn set(&mut self, name: &str, source: &str) -> Result<(), ParamError> {
        if !is_valid_name(name) || is_reserved_name(name) {
            return Err(ParamError::InvalidName(name.to_string()));
        }
        let expr = parse(source)?;
        self.check_no_cycle(name, &expr)?;
        match self.index_of(name) {
            Some(i) => {
                self.parameters[i].source = source.to_string();
                self.parameters[i].expr = expr;
            }
            None => self.parameters.push(Parameter {
                name: name.to_string(),
                source: source.to_string(),
                expr,
            }),
        }
        Ok(())
    }

    /// Removes a parameter by name. Returns whether one was actually
    /// removed. Deliberately does not check whether another parameter's
    /// formula still references `name` — that becomes an
    /// [`ParamError::UndefinedReference`] for the referencing parameter the
    /// next time it's resolved, the same graceful-degradation treatment an
    /// unresolvable reference gets everywhere else in this module, rather
    /// than a special "can't delete, still in use" rule this stage doesn't
    /// need to invent (a future UI stage may still want to warn before
    /// deleting — that's a UI-layer decision, not a data-layer one).
    pub fn remove(&mut self, name: &str) -> bool {
        match self.index_of(name) {
            Some(i) => {
                self.parameters.remove(i);
                true
            }
            None => false,
        }
    }

    /// Would accepting `new_expr` as `name`'s formula close a circular
    /// reference right now? Walks the dependency graph from each of
    /// `new_expr`'s direct references, following each visited parameter's
    /// *existing* stored formula, looking for a path back to `name`. A
    /// reference to a name not currently in the table just terminates that
    /// branch — no cycle through something that doesn't exist yet.
    fn check_no_cycle(&self, name: &str, new_expr: &Expr) -> Result<(), ParamError> {
        let mut direct_refs = Vec::new();
        new_expr.refs(&mut direct_refs);
        for start in &direct_refs {
            let mut visiting = Vec::new();
            if let Some(mut path) = self.find_path(start, name, &mut visiting) {
                let mut full = vec![name.to_string()];
                full.append(&mut path);
                return Err(ParamError::Cycle(full));
            }
        }
        Ok(())
    }

    /// Depth-first search from `current` to `target`, following each
    /// visited parameter's existing formula's references. Returns the path
    /// (starting with `current`) if `target` is reachable.
    fn find_path(
        &self,
        current: &str,
        target: &str,
        visiting: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        if current == target {
            return Some(vec![current.to_string()]);
        }
        if visiting.iter().any(|v| v == current) {
            // A cycle that doesn't involve `target` shouldn't be reachable
            // (`set` refuses to create one), but bail out rather than loop
            // forever if that invariant is ever violated some other way
            // (e.g. a foreign/hand-edited file deserialized straight into a
            // table without going through `set`).
            return None;
        }
        let param = self.get(current)?;
        visiting.push(current.to_string());
        let mut refs = Vec::new();
        param.expr.refs(&mut refs);
        for r in &refs {
            if let Some(mut rest) = self.find_path(r, target, visiting) {
                let mut path = vec![current.to_string()];
                path.append(&mut rest);
                visiting.pop();
                return Some(path);
            }
        }
        visiting.pop();
        None
    }

    /// Resolves one parameter's numeric value and dependency chain using a
    /// fresh memo cache.
    pub fn resolve(&self, name: &str) -> Result<f64, ParamError> {
        let mut cache = HashMap::new();
        let mut stack = Vec::new();
        self.resolve_inner(name, &mut cache, &mut stack)
    }

    /// Resolves every parameter, isolating a failure to just the parameters
    /// it actually affects rather than aborting the whole table — the same
    /// don't-let-one-bad-item-take-down-everything-else treatment this
    /// codebase already gives dangling constraints and similar derived-data
    /// failures elsewhere.
    pub fn resolve_all(&self) -> HashMap<String, Result<f64, ParamError>> {
        let mut out = HashMap::with_capacity(self.parameters.len());
        for param in &self.parameters {
            out.insert(param.name.clone(), self.resolve(&param.name));
        }
        out
    }

    fn resolve_inner(
        &self,
        name: &str,
        cache: &mut HashMap<String, f64>,
        stack: &mut Vec<String>,
    ) -> Result<f64, ParamError> {
        if let Some(v) = cache.get(name) {
            return Ok(*v);
        }
        if stack.iter().any(|s| s == name) {
            // Defense in depth only — `set` already refuses to create a
            // cycle; see `find_path`'s doc comment for how one could still
            // end up here.
            let mut cyc = stack.clone();
            cyc.push(name.to_string());
            return Err(ParamError::Cycle(cyc));
        }
        let param = self
            .get(name)
            .ok_or_else(|| ParamError::UndefinedReference(name.to_string()))?;
        stack.push(name.to_string());
        let value = self.eval(&param.expr, cache, stack)?;
        stack.pop();
        cache.insert(name.to_string(), value);
        Ok(value)
    }

    fn eval(
        &self,
        expr: &Expr,
        cache: &mut HashMap<String, f64>,
        stack: &mut Vec<String>,
    ) -> Result<f64, ParamError> {
        Ok(match expr {
            Expr::Number(n) => *n,
            Expr::Ref(name) => self.resolve_inner(name, cache, stack)?,
            Expr::Neg(a) => -self.eval(a, cache, stack)?,
            Expr::Add(a, b) => self.eval(a, cache, stack)? + self.eval(b, cache, stack)?,
            Expr::Sub(a, b) => self.eval(a, cache, stack)? - self.eval(b, cache, stack)?,
            Expr::Mul(a, b) => self.eval(a, cache, stack)? * self.eval(b, cache, stack)?,
            Expr::Div(a, b) => {
                let denom = self.eval(b, cache, stack)?;
                if denom == 0.0 {
                    return Err(ParamError::DivisionByZero);
                }
                self.eval(a, cache, stack)? / denom
            }
            Expr::Pow(a, b) => self
                .eval(a, cache, stack)?
                .powf(self.eval(b, cache, stack)?),
            Expr::Call(fname, args) => {
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.eval(arg, cache, stack)?);
                }
                call_builtin(fname, &values)?
            }
        })
    }
}

impl Scene {
    /// The document's named-parameter table. Use
    /// [`Scene::named_parameters_mut`] to
    /// define/redefine/remove a parameter.
    pub fn named_parameters(&self) -> &ParameterTable {
        &self.named_parameters
    }

    pub fn named_parameters_mut(&mut self) -> &mut ParameterTable {
        &mut self.named_parameters
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arithmetic_with_ordinary_precedence() {
        // 2 + 3 * 4 == 14, not 20 -- proves * binds tighter than +.
        assert_eq!(eval_literal("2 + 3 * 4"), 14.0);
        // (2 + 3) * 4 == 20 -- parens override precedence.
        assert_eq!(eval_literal("(2 + 3) * 4"), 20.0);
        // 2 ^ 3 ^ 2 == 2 ^ (3 ^ 2) == 512 -- right-associative power.
        assert_eq!(eval_literal("2 ^ 3 ^ 2"), 512.0);
        // Unary minus binds tighter than power's left operand is *read*,
        // but -2^2 must still equal -(2^2) = -4 under this grammar (unary
        // parses before power), matching common calculator convention.
        assert_eq!(eval_literal("-2^2"), -4.0);
        assert_eq!(eval_literal("2^-2"), 0.25);
        assert_eq!(eval_literal("10 / 4"), 2.5);
        assert_eq!(eval_literal("-(3 + 4)"), -7.0);
    }

    fn eval_literal(src: &str) -> f64 {
        let table = ParameterTable::new();
        let expr = parse(src).unwrap_or_else(|e| panic!("parse '{src}': {e}"));
        table
            .eval(&expr, &mut HashMap::new(), &mut Vec::new())
            .unwrap_or_else(|e| panic!("eval '{src}': {e}"))
    }

    #[test]
    fn parses_scientific_notation() {
        assert_eq!(eval_literal("1e3"), 1000.0);
        assert_eq!(eval_literal("1.5e-2"), 0.015);
    }

    #[test]
    fn rejects_malformed_formulas() {
        assert!(matches!(parse(""), Err(ParamError::Parse(_))));
        assert!(matches!(parse("1 +"), Err(ParamError::Parse(_))));
        assert!(matches!(parse("(1 + 2"), Err(ParamError::Parse(_))));
        assert!(matches!(parse("1 2"), Err(ParamError::Parse(_))));
        assert!(matches!(parse("1 @ 2"), Err(ParamError::Parse(_))));
    }

    #[test]
    fn builtin_functions_evaluate_and_check_arity() {
        assert_eq!(eval_literal("sqrt(16)"), 4.0);
        assert_eq!(eval_literal("abs(-5)"), 5.0);
        assert_eq!(eval_literal("min(3, 7)"), 3.0);
        assert_eq!(eval_literal("max(3, 7)"), 7.0);
        assert_eq!(eval_literal("pow(2, 10)"), 1024.0);

        let table = ParameterTable::new();
        let expr = parse("sqrt(1, 2)").unwrap();
        let err = table
            .eval(&expr, &mut HashMap::new(), &mut Vec::new())
            .unwrap_err();
        assert_eq!(
            err,
            ParamError::WrongArgCount {
                function: "sqrt".to_string(),
                expected: 1,
                found: 2
            }
        );

        let expr = parse("bogus(1)").unwrap();
        let err = table
            .eval(&expr, &mut HashMap::new(), &mut Vec::new())
            .unwrap_err();
        assert_eq!(err, ParamError::UnknownFunction("bogus".to_string()));
    }

    #[test]
    fn division_by_zero_is_reported_not_panicked() {
        let table = ParameterTable::new();
        let expr = parse("1 / 0").unwrap();
        assert_eq!(
            table.eval(&expr, &mut HashMap::new(), &mut Vec::new()),
            Err(ParamError::DivisionByZero)
        );
    }

    #[test]
    fn set_rejects_invalid_and_reserved_names() {
        let mut table = ParameterTable::new();
        assert!(matches!(
            table.set("2bad", "1"),
            Err(ParamError::InvalidName(_))
        ));
        assert!(matches!(
            table.set("has space", "1"),
            Err(ParamError::InvalidName(_))
        ));
        assert!(matches!(
            table.set("", "1"),
            Err(ParamError::InvalidName(_))
        ));
        assert!(matches!(
            table.set("sqrt", "1"),
            Err(ParamError::InvalidName(_))
        ));
        assert!(table.set("_valid1", "1").is_ok());
    }

    #[test]
    fn a_literal_parameter_resolves_to_its_number() {
        let mut table = ParameterTable::new();
        table.set("plate_len", "42.5").unwrap();
        assert_eq!(table.resolve("plate_len"), Ok(42.5));
    }

    #[test]
    fn a_formula_resolves_through_a_reference_chain() {
        let mut table = ParameterTable::new();
        table.set("hole_dia", "5").unwrap();
        table.set("hole_spacing", "2 * hole_dia + 1.5").unwrap();
        table.set("row_span", "3 * hole_spacing").unwrap();
        assert_eq!(table.resolve("hole_spacing"), Ok(11.5));
        assert_eq!(table.resolve("row_span"), Ok(34.5));
    }

    #[test]
    fn resolving_an_undefined_reference_reports_which_name_is_missing() {
        let mut table = ParameterTable::new();
        table.set("a", "b + 1").unwrap();
        assert_eq!(
            table.resolve("a"),
            Err(ParamError::UndefinedReference("b".to_string()))
        );
    }

    #[test]
    fn set_rejects_a_direct_self_reference() {
        let mut table = ParameterTable::new();
        let err = table.set("a", "a + 1").unwrap_err();
        assert!(matches!(err, ParamError::Cycle(_)));
        assert!(
            !table.contains("a"),
            "a rejected formula must not be stored"
        );
    }

    #[test]
    fn set_rejects_an_indirect_cycle_at_the_definition_that_closes_it() {
        let mut table = ParameterTable::new();
        table.set("a", "b").unwrap();
        // b doesn't exist yet -- not a cycle, a is just unresolved for now.
        assert_eq!(
            table.resolve("a"),
            Err(ParamError::UndefinedReference("b".to_string()))
        );
        // Defining b = a closes the loop right here and must be rejected,
        // leaving b undefined rather than silently accepted.
        let err = table.set("b", "a").unwrap_err();
        assert!(matches!(err, ParamError::Cycle(_)));
        assert!(!table.contains("b"));
        // a's formula (still "b") is untouched -- it stays unresolved, not
        // corrupted by the rejected attempt.
        assert_eq!(
            table.resolve("a"),
            Err(ParamError::UndefinedReference("b".to_string()))
        );
    }

    #[test]
    fn set_allows_redefining_an_existing_parameter_to_break_a_would_be_cycle() {
        let mut table = ParameterTable::new();
        table.set("a", "1").unwrap();
        table.set("b", "a + 1").unwrap();
        // Redefining a in terms of b must be rejected (a <- b <- a)...
        assert!(table.set("a", "b + 1").is_err());
        // ...but redefining a to something unrelated is always fine.
        table.set("a", "5").unwrap();
        assert_eq!(table.resolve("a"), Ok(5.0));
        assert_eq!(table.resolve("b"), Ok(6.0));
    }

    #[test]
    fn resolve_all_isolates_a_failure_to_the_affected_parameters_only() {
        let mut table = ParameterTable::new();
        table.set("good", "10").unwrap();
        table.set("bad", "missing + 1").unwrap();
        table.set("depends_on_bad", "bad * 2").unwrap();

        let results = table.resolve_all();
        assert_eq!(results.get("good"), Some(&Ok(10.0)));
        assert_eq!(
            results.get("bad"),
            Some(&Err(ParamError::UndefinedReference("missing".to_string())))
        );
        assert_eq!(
            results.get("depends_on_bad"),
            Some(&Err(ParamError::UndefinedReference("missing".to_string())))
        );
    }

    #[test]
    fn remove_drops_a_parameter_and_leaves_dependents_undefined_not_panicking() {
        let mut table = ParameterTable::new();
        table.set("a", "1").unwrap();
        table.set("b", "a + 1").unwrap();
        assert!(table.remove("a"));
        assert!(
            !table.remove("a"),
            "removing an already-removed name reports false"
        );
        assert_eq!(
            table.resolve("b"),
            Err(ParamError::UndefinedReference("a".to_string()))
        );
    }

    #[test]
    fn set_upserts_an_existing_parameters_formula() {
        let mut table = ParameterTable::new();
        table.set("a", "1").unwrap();
        table.set("a", "2").unwrap();
        assert_eq!(table.len(), 1, "redefining must not create a second entry");
        assert_eq!(table.resolve("a"), Ok(2.0));
    }

    #[test]
    fn table_round_trips_through_bincode() {
        let mut table = ParameterTable::new();
        table.set("hole_dia", "5").unwrap();
        table.set("hole_spacing", "2 * hole_dia + 1.5").unwrap();

        let bytes = bincode::serialize(&table).expect("serialize");
        let restored: ParameterTable = bincode::deserialize(&bytes).expect("deserialize");

        assert_eq!(restored.len(), 2);
        assert_eq!(restored.resolve("hole_spacing"), Ok(11.5));
        assert_eq!(restored.get("hole_dia").unwrap().source, "5");
    }

    #[test]
    fn iter_preserves_insertion_order_across_redefinitions() {
        let mut table = ParameterTable::new();
        table.set("z", "1").unwrap();
        table.set("a", "2").unwrap();
        table.set("z", "3").unwrap(); // redefine, should not move to the end
        let names: Vec<&str> = table.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["z", "a"]);
    }
}
