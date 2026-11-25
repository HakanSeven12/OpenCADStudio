//! What a point group of the Point Group Properties window holds: the
//! Include and Exclude rules ("With numbers matching" and the rest), the
//! groups picked on Point Groups and the rows of Query Builder, read the way
//! the reference reads them (measured, PG01):
//!
//! - numbers: a comma list of numbers and `a-b` ranges; anything else ("4-",
//!   "3 5", "x") is "Point list is not valid!";
//! - elevations: a comma list of `v`, `<v`, `>v`, `<=v`, `>=v` and `a-b`
//!   (ends included); anything else is a type mismatch; a point without an
//!   elevation matches none;
//! - names and descriptions: a comma list of patterns, `*` any run and `?`
//!   one character, case ignored unless the query is case-sensitive;
//! - the Include rules are joined by OR, the Exclude rules taken away
//!   ("( A OR B ) AND NOT ( C )").
//!
//! The query written to the drawing selects the same points; the codec
//! stores a single rule as the reference does and the rest as a custom
//! query.

use document::classes::civil::{CogoPoint, Comparison, PointAttribute, PointQuery, QueryConstant, QueryExpression, NO_ELEVATION};

/// A point attribute a rule or a Query Builder row compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attr {
    Number,
    Elevation,
    Name,
    Raw,
    Full,
}

impl Attr {
    pub const ALL: [Attr; 5] = [Attr::Number, Attr::Elevation, Attr::Name, Attr::Raw, Attr::Full];

    /// The attribute as the query statement names it.
    pub fn label(self) -> &'static str {
        match self {
            Attr::Number => "Point Number",
            Attr::Elevation => "Point Elevation",
            Attr::Name => "Name",
            Attr::Raw => "Raw Description",
            Attr::Full => "Full Description",
        }
    }

    fn codec(self) -> PointAttribute {
        match self {
            Attr::Number => PointAttribute::Number,
            Attr::Elevation => PointAttribute::Elevation,
            Attr::Name => PointAttribute::Name,
            Attr::Raw => PointAttribute::RawDescription,
            Attr::Full => PointAttribute::FullDescription,
        }
    }
}

/// A comparison of a Query Builder row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Gt,
    Lt,
    Ge,
    Le,
}

impl Op {
    pub const ALL: [Op; 5] = [Op::Eq, Op::Gt, Op::Lt, Op::Ge, Op::Le];

    pub fn label(self) -> &'static str {
        match self {
            Op::Eq => "=",
            Op::Gt => ">",
            Op::Lt => "<",
            Op::Ge => ">=",
            Op::Le => "<=",
        }
    }

    fn codec(self) -> Comparison {
        match self {
            Op::Eq => Comparison::Equal,
            Op::Gt => Comparison::Greater,
            Op::Lt => Comparison::Less,
            Op::Ge => Comparison::GreaterOrEqual,
            Op::Le => Comparison::LessOrEqual,
        }
    }

    fn holds(self, a: f64, b: f64) -> bool {
        match self {
            Op::Eq => a == b,
            Op::Gt => a > b,
            Op::Lt => a < b,
            Op::Ge => a >= b,
            Op::Le => a <= b,
        }
    }
}

/// The set operator opening a Query Builder row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joint {
    None,
    And,
    Or,
    Not,
}

impl Joint {
    pub const ALL: [Joint; 4] = [Joint::None, Joint::And, Joint::Or, Joint::Not];

    pub fn label(self) -> &'static str {
        match self {
            Joint::None => "",
            Joint::And => "AND",
            Joint::Or => "OR",
            Joint::Not => "NOT",
        }
    }
}

/// One row of Query Builder: a new row compares the name with `*`
/// (measured).
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub joint: Joint,
    pub open: bool,
    pub attr: Attr,
    pub op: Op,
    pub value: String,
    pub close: bool,
}

impl Default for Row {
    fn default() -> Self {
        Row { joint: Joint::None, open: false, attr: Attr::Name, op: Op::Eq, value: "*".into(), close: false }
    }
}

/// One "With … matching" rule: on, and its text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rule {
    pub on: bool,
    pub text: String,
}

/// The five rules of the Include or the Exclude tab, in the tab's order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rules(pub [Rule; 5]);

impl Rules {
    /// The tab's rows: numbers, elevations, names, raw and full descriptions.
    pub const ORDER: [Attr; 5] = [Attr::Number, Attr::Elevation, Attr::Name, Attr::Raw, Attr::Full];

    pub fn get(&self, attr: Attr) -> &Rule {
        &self.0[Self::ORDER.iter().position(|a| *a == attr).unwrap_or(0)]
    }

    pub fn get_mut(&mut self, attr: Attr) -> &mut Rule {
        &mut self.0[Self::ORDER.iter().position(|a| *a == attr).unwrap_or(0)]
    }
}

/// A rule that does not read.
#[derive(Debug, Clone, PartialEq)]
pub enum Bad {
    /// "Point list is not valid!"
    Numbers,
    /// A type mismatch: an elevation, or a Query Builder value, that is not
    /// a number.
    Mismatch,
}

/// One comparison a rule or a row makes.
#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    Compare(Attr, Op, Value),
    /// Both ends included.
    Range(Attr, f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Pattern(String),
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `a-b` with both ends numbers (an end may be negative: `-10--5`).
fn range(text: &str) -> Option<(f64, f64)> {
    let t = text.trim();
    (1..t.len()).filter(|&i| t.as_bytes()[i] == b'-').find_map(|i| Some((number(&t[..i])?, number(&t[i + 1..])?)))
}

/// The terms of a rule's text.
pub fn rule_terms(attr: Attr, text: &str) -> Result<Vec<Term>, Bad> {
    let items = text.split(',').map(str::trim).filter(|t| !t.is_empty());
    match attr {
        Attr::Number => items
            .map(|item| {
                let whole = |v: f64| v >= 1.0 && v.fract() == 0.0 && v <= i32::MAX as f64;
                if let Some(v) = number(item).filter(|v| whole(*v)) {
                    return Ok(Term::Compare(Attr::Number, Op::Eq, Value::Number(v)));
                }
                match range(item) {
                    Some((a, b)) if whole(a) && whole(b) => Ok(Term::Range(Attr::Number, a.min(b), a.max(b))),
                    _ => Err(Bad::Numbers),
                }
            })
            .collect(),
        Attr::Elevation => items
            .map(|item| {
                for (prefix, op) in [("<=", Op::Le), (">=", Op::Ge), ("<", Op::Lt), (">", Op::Gt), ("=", Op::Eq)] {
                    if let Some(rest) = item.strip_prefix(prefix) {
                        return number(rest).map(|v| Term::Compare(Attr::Elevation, op, Value::Number(v))).ok_or(Bad::Mismatch);
                    }
                }
                if let Some(v) = number(item) {
                    return Ok(Term::Compare(Attr::Elevation, Op::Eq, Value::Number(v)));
                }
                range(item).map(|(a, b)| Term::Range(Attr::Elevation, a.min(b), a.max(b))).ok_or(Bad::Mismatch)
            })
            .collect(),
        _ => Ok(items.map(|p| Term::Compare(attr, Op::Eq, Value::Pattern(p.to_string()))).collect()),
    }
}

/// The terms of the rules that are on, in the tab's order.
pub fn rules_terms(rules: &Rules) -> Result<Vec<Term>, (Attr, Bad)> {
    let mut out = Vec::new();
    for (attr, rule) in Rules::ORDER.iter().zip(&rules.0) {
        if rule.on {
            out.extend(rule_terms(*attr, &rule.text).map_err(|b| (*attr, b))?);
        }
    }
    Ok(out)
}

/// A point as the window lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub number: u32,
    pub position: [f64; 3],
    pub name: Option<String>,
    pub raw: Option<String>,
}

impl Point {
    pub fn of(p: &CogoPoint) -> Point {
        Point { number: p.number, position: [p.position.x, p.position.y, p.position.z], name: p.name.clone(), raw: p.description.clone() }
    }

    pub fn has_elevation(&self) -> bool {
        self.position[2] > NO_ELEVATION * 0.5
    }
}

/// `*` / `?` over the whole text, case ignored unless `case`.
pub fn wildcard(pattern: &str, text: &str, case: bool) -> bool {
    let fold = |s: &str| if case { s.to_string() } else { s.to_lowercase() };
    let t: Vec<char> = fold(text).chars().collect();
    let mut ok = vec![false; t.len() + 1];
    ok[0] = true;
    for c in fold(pattern).chars() {
        let mut next = vec![false; t.len() + 1];
        for j in 0..=t.len() {
            next[j] = match c {
                '*' => ok[j] || (j > 0 && next[j - 1]),
                '?' => j > 0 && ok[j - 1],
                _ => j > 0 && ok[j - 1] && t[j - 1] == c,
            };
        }
        ok = next;
    }
    ok[t.len()]
}

impl Term {
    fn selects(&self, p: &Point, case: bool) -> bool {
        let value = |attr: Attr| -> Option<f64> {
            match attr {
                Attr::Number => Some(p.number as f64),
                Attr::Elevation => p.has_elevation().then_some(p.position[2]),
                _ => None,
            }
        };
        match self {
            Term::Compare(attr, op, Value::Number(v)) => value(*attr).is_some_and(|x| op.holds(x, *v)),
            Term::Range(attr, a, b) => value(*attr).is_some_and(|x| *a <= x && x <= *b),
            Term::Compare(attr, _, Value::Pattern(pattern)) => {
                let text = match attr {
                    Attr::Name => p.name.as_deref(),
                    _ => p.raw.as_deref(),
                };
                match (attr, text) {
                    // A point without a name matches no name pattern.
                    (Attr::Name, None) => false,
                    (_, text) => wildcard(pattern, text.unwrap_or(""), case && *attr != Attr::Name),
                }
            }
        }
    }

    fn statement(&self) -> String {
        match self {
            Term::Compare(attr, op, Value::Number(v)) => format!("{} {} {}", attr.label(), op.label(), short(*v)),
            Term::Compare(attr, op, Value::Pattern(p)) => format!("{} {} {}", attr.label(), op.label(), p),
            Term::Range(attr, a, b) => format!("{} >= {} AND {} <= {}", attr.label(), short(*a), attr.label(), short(*b)),
        }
    }

    fn expression(&self) -> QueryExpression {
        let constant = |attr: Attr, v: f64| match attr {
            Attr::Number => QueryConstant::Number(v as u32),
            _ => QueryConstant::Elevation(v),
        };
        match self {
            Term::Compare(attr, op, Value::Number(v)) => QueryExpression::Compare(attr.codec(), op.codec(), constant(*attr, *v)),
            Term::Compare(attr, op, Value::Pattern(p)) => QueryExpression::Compare(attr.codec(), op.codec(), QueryConstant::Pattern(p.clone())),
            Term::Range(attr, a, b) => QueryExpression::And(
                Box::new(QueryExpression::Compare(attr.codec(), Comparison::GreaterOrEqual, constant(*attr, *a))),
                Box::new(QueryExpression::Compare(attr.codec(), Comparison::LessOrEqual, constant(*attr, *b))),
            ),
        }
    }
}

/// A number as the statement shows it: no trailing zeros.
fn short(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// What the window's group selects.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Query {
    pub include_all: bool,
    pub include: Vec<Term>,
    pub exclude: Vec<Term>,
    /// Query Builder rows in place of the rules, when modified.
    pub custom: Option<Vec<Row>>,
    pub case_sensitive: bool,
}

/// A parsed Query Builder expression.
#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Term(Term),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Parens(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    And,
    Or,
    Not,
    Open,
    Close,
    Term(Term),
}

/// The term of a row: a number or an elevation compares with a number, a
/// name or a description with a pattern (equal only).
pub fn row_term(row: &Row) -> Result<Term, Bad> {
    match row.attr {
        Attr::Number | Attr::Elevation => {
            let v = number(&row.value).ok_or(Bad::Mismatch)?;
            if row.attr == Attr::Number && (v < 0.0 || v.fract() != 0.0) {
                return Err(Bad::Mismatch);
            }
            Ok(Term::Compare(row.attr, row.op, Value::Number(v)))
        }
        _ => Ok(Term::Compare(row.attr, row.op, Value::Pattern(row.value.trim().to_string()))),
    }
}

fn tokens(rows: &[Row]) -> Result<Vec<Token>, Bad> {
    let mut out = Vec::new();
    for row in rows {
        match row.joint {
            Joint::None => {}
            Joint::And => out.push(Token::And),
            Joint::Or => out.push(Token::Or),
            Joint::Not => out.push(Token::Not),
        }
        if row.open {
            out.push(Token::Open);
        }
        out.push(Token::Term(row_term(row)?));
        if row.close {
            out.push(Token::Close);
        }
    }
    Ok(out)
}

/// expr := and (OR and)*, and := unary (AND unary)*, unary := NOT unary | primary,
/// primary := ( expr ) | term. Two rows without a set operator between them
/// are joined by AND.
fn parse(tokens: &[Token]) -> Option<Expr> {
    fn expr(t: &[Token], i: &mut usize) -> Option<Expr> {
        let mut left = and(t, i)?;
        while t.get(*i) == Some(&Token::Or) {
            *i += 1;
            left = Expr::Or(Box::new(left), Box::new(and(t, i)?));
        }
        Some(left)
    }
    fn and(t: &[Token], i: &mut usize) -> Option<Expr> {
        let mut left = unary(t, i)?;
        loop {
            match t.get(*i) {
                Some(Token::And) => *i += 1,
                Some(Token::Open | Token::Term(_) | Token::Not) => {}
                _ => return Some(left),
            }
            left = Expr::And(Box::new(left), Box::new(unary(t, i)?));
        }
    }
    fn unary(t: &[Token], i: &mut usize) -> Option<Expr> {
        match t.get(*i)? {
            Token::Not => {
                *i += 1;
                Some(Expr::Not(Box::new(unary(t, i)?)))
            }
            Token::Open => {
                *i += 1;
                let inner = expr(t, i)?;
                (t.get(*i) == Some(&Token::Close)).then_some(())?;
                *i += 1;
                Some(Expr::Parens(Box::new(inner)))
            }
            Token::Term(term) => {
                *i += 1;
                Some(Expr::Term(term.clone()))
            }
            _ => None,
        }
    }
    let mut i = 0;
    let e = expr(tokens, &mut i)?;
    (i == tokens.len()).then_some(e)
}

impl Expr {
    fn selects(&self, p: &Point, case: bool) -> bool {
        match self {
            Expr::Term(t) => t.selects(p, case),
            Expr::And(a, b) => a.selects(p, case) && b.selects(p, case),
            Expr::Or(a, b) => a.selects(p, case) || b.selects(p, case),
            Expr::Not(a) => !a.selects(p, case),
            Expr::Parens(a) => a.selects(p, case),
        }
    }

    fn codec(&self) -> QueryExpression {
        match self {
            Expr::Term(t) => t.expression(),
            Expr::And(a, b) => QueryExpression::And(Box::new(a.codec()), Box::new(b.codec())),
            Expr::Or(a, b) => QueryExpression::Or(Box::new(a.codec()), Box::new(b.codec())),
            Expr::Not(a) => QueryExpression::Not(Box::new(a.codec())),
            Expr::Parens(a) => QueryExpression::Parens(Box::new(a.codec())),
        }
    }
}

/// The rules as Query Builder rows: the Exclude terms under NOT first (a
/// NOT may only open a query the codec writes), then AND and the Include
/// terms, each group in parentheses with its terms joined by OR. They
/// select what the rules select.
pub fn rows_of(include: &[Term], exclude: &[Term]) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut group = |terms: &[Term], first: Joint| {
        let start = rows.len();
        for (k, term) in terms.iter().enumerate() {
            let joint = if k == 0 { first } else { Joint::Or };
            match term {
                Term::Compare(attr, op, value) => {
                    let value = match value {
                        Value::Number(v) => short(*v),
                        Value::Pattern(p) => p.clone(),
                    };
                    rows.push(Row { joint, open: false, attr: *attr, op: *op, value, close: false });
                }
                Term::Range(attr, a, b) => {
                    rows.push(Row { joint, open: false, attr: *attr, op: Op::Ge, value: short(*a), close: false });
                    rows.push(Row { joint: Joint::And, open: false, attr: *attr, op: Op::Le, value: short(*b), close: false });
                }
            }
        }
        if rows.len() > start {
            rows[start].open = true;
            rows.last_mut().unwrap().close = true;
        }
    };
    if !exclude.is_empty() {
        group(exclude, Joint::Not);
    }
    group(include, if exclude.is_empty() { Joint::None } else { Joint::And });
    rows
}

impl Query {
    fn expression(&self) -> Result<Option<Expr>, Bad> {
        if let Some(rows) = &self.custom {
            if rows.is_empty() {
                return Ok(None);
            }
            return parse(&tokens(rows)?).map(Some).ok_or(Bad::Mismatch);
        }
        let chain = |terms: &[Term]| terms.iter().cloned().map(Expr::Term).reduce(|a, b| Expr::Or(Box::new(a), Box::new(b)));
        let include = if self.include_all { None } else { chain(&self.include) };
        Ok(match (include, chain(&self.exclude)) {
            (Some(i), None) => Some(i),
            (Some(i), Some(e)) => Some(Expr::And(Box::new(Expr::Not(Box::new(Expr::Parens(Box::new(e))))), Box::new(Expr::Parens(Box::new(i))))),
            (None, Some(e)) => Some(Expr::Not(Box::new(Expr::Parens(Box::new(e))))),
            (None, None) => None,
        })
    }

    /// Whether the query selects nothing at all: no rule on, nothing picked.
    pub fn is_empty(&self) -> bool {
        match &self.custom {
            Some(rows) => rows.is_empty(),
            None => !self.include_all && self.include.is_empty(),
        }
    }

    /// The numbers of the points the group holds, ascending.
    pub fn members(&self, points: &[Point]) -> Result<Vec<u32>, Bad> {
        let mut out: Vec<u32> = match self.expression()? {
            Some(e) => points.iter().filter(|p| e.selects(p, self.case_sensitive)).map(|p| p.number).collect(),
            None if self.include_all && self.custom.is_none() => points.iter().map(|p| p.number).collect(),
            None => Vec::new(),
        };
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    /// The query statement the Summary tab shows.
    pub fn statement(&self) -> String {
        if let Some(rows) = &self.custom {
            return rows
                .iter()
                .map(|r| {
                    let joint = if r.joint == Joint::None { String::new() } else { format!("{} ", r.joint.label()) };
                    let term = row_term(r).map(|t| t.statement()).unwrap_or_else(|_| format!("{} {} {}", r.attr.label(), r.op.label(), r.value));
                    format!("{joint}{}{term}{}", if r.open { "( " } else { "" }, if r.close { " )" } else { "" })
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
        let group = |terms: &[Term]| {
            let lines: Vec<String> = terms.iter().enumerate().map(|(k, t)| format!("{}{}", if k == 0 { "( " } else { "OR " }, t.statement())).collect();
            if lines.is_empty() { None } else { Some(format!("{} )", lines.join("\n"))) }
        };
        let include = if self.include_all { None } else { group(&self.include) };
        match (include, group(&self.exclude)) {
            (Some(i), Some(e)) => format!("{i}\nAND\nNOT {e}"),
            (Some(i), None) => i,
            (None, Some(e)) => format!("NOT {e}"),
            (None, None) => String::new(),
        }
    }

    /// The query as the codec writes it: a single rule natively, the rest
    /// as a custom query; a group that holds nothing (or everything) is a
    /// comparison of the point number that no (every) point fails.
    pub fn to_codec(&self) -> Result<PointQuery, Bad> {
        let native = |terms: &[Term]| -> Option<PointQuery> {
            let attr = match terms.first()? {
                Term::Compare(a, ..) | Term::Range(a, ..) => *a,
            };
            if terms.iter().any(|t| !matches!(t, Term::Compare(a, ..) | Term::Range(a, ..) if *a == attr)) {
                return None;
            }
            match attr {
                Attr::Number => {
                    let mut numbers = Vec::new();
                    for t in terms {
                        match t {
                            Term::Compare(_, Op::Eq, Value::Number(v)) => numbers.push(*v as u32),
                            Term::Range(_, a, b) if b - a <= 10_000.0 => numbers.extend((*a as u32)..=(*b as u32)),
                            _ => return None,
                        }
                    }
                    Some(PointQuery::Numbers(numbers))
                }
                Attr::Elevation => match terms {
                    [Term::Compare(_, op, Value::Number(v))] => Some(PointQuery::Elevation(op.codec(), *v)),
                    [Term::Range(_, a, b)] => Some(PointQuery::ElevationRange(*a, *b)),
                    _ => None,
                },
                _ => {
                    let patterns = terms.iter().map(|t| match t {
                        Term::Compare(_, _, Value::Pattern(p)) => p.clone(),
                        _ => String::new(),
                    });
                    let patterns = patterns.collect();
                    Some(match attr {
                        Attr::Name => PointQuery::Names(patterns),
                        Attr::Raw => PointQuery::RawDescription(patterns),
                        _ => PointQuery::FullDescription(patterns),
                    })
                }
            }
        };
        let case = |q: PointQuery| if self.case_sensitive { PointQuery::CaseSensitive(Box::new(q)) } else { q };
        if self.custom.is_none() {
            let include = if self.include_all { Some(None) } else { native(&self.include).map(Some) };
            match (include, self.exclude.is_empty()) {
                (Some(Some(q)), true) => return Ok(case(q)),
                (Some(include), false) if !self.case_sensitive => {
                    if let Some(e) = native(&self.exclude) {
                        return Ok(PointQuery::Excluding(include.map(Box::new), Box::new(e)));
                    }
                }
                _ => {}
            }
        }
        let nobody = QueryExpression::Compare(PointAttribute::Number, Comparison::Less, QueryConstant::Number(1));
        let everybody = QueryExpression::Compare(PointAttribute::Number, Comparison::GreaterOrEqual, QueryConstant::Number(1));
        let expression = match self.expression()? {
            Some(e) => e.codec(),
            None if self.include_all && self.custom.is_none() => everybody,
            None => nobody,
        };
        Ok(case(PointQuery::Custom(expression)))
    }
}

/// The numbers as the "With numbers matching" field writes them: runs of
/// three or more as `a-b`.
pub fn number_list(numbers: &[u32]) -> String {
    let mut n = numbers.to_vec();
    n.sort_unstable();
    n.dedup();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < n.len() {
        let mut j = i;
        while j + 1 < n.len() && n[j + 1] == n[j] + 1 {
            j += 1;
        }
        if j >= i + 2 {
            out.push(format!("{}-{}", n[i], n[j]));
        } else {
            out.extend(n[i..=j].iter().map(u32::to_string));
        }
        i = j + 1;
    }
    out.join(",")
}

/// A group name with the characters the reference refuses in names
/// replaced by `_` (measured: "A;B" became "A_B").
pub fn clean_name(name: &str) -> String {
    name.chars().map(|c| if "<>/\\:;=|,*?'\"`".contains(c) { '_' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts() -> Vec<Point> {
        let p = |n, z: f64, d: &str| Point { number: n, position: [0.0, 0.0, z], name: None, raw: Some(d.into()) };
        vec![p(1, 10.0, "TREE"), p(2, 20.0, "TREE2"), p(3, 30.0, "POLE"), p(4, NO_ELEVATION, "MH"), p(5, 15.0, "tree")]
    }

    fn rules(on: &[(Attr, &str)]) -> Vec<Term> {
        on.iter().flat_map(|(a, t)| rule_terms(*a, t).unwrap()).collect()
    }

    #[test]
    fn rules_select_what_the_reference_counted() {
        let q = |include: Vec<Term>, exclude: Vec<Term>| Query { include, exclude, ..Default::default() }.members(&pts()).unwrap();
        // PG01 counts: TREE* 3; TREE* or >25 without 5: 3; 10-20: 3; <15,>25: 2; <=15: 2; T?EE 2; "TREE POLE" 0.
        assert_eq!(q(rules(&[(Attr::Raw, "TREE*")]), vec![]), vec![1, 2, 5]);
        assert_eq!(q(rules(&[(Attr::Raw, "TREE*"), (Attr::Elevation, ">25")]), rules(&[(Attr::Number, "5")])), vec![1, 2, 3]);
        assert_eq!(q(rules(&[(Attr::Elevation, "10-20")]), vec![]).len(), 3);
        assert_eq!(q(rules(&[(Attr::Elevation, "<15,>25")]), vec![]), vec![1, 3]);
        assert_eq!(q(rules(&[(Attr::Elevation, "<=15")]), vec![]), vec![1, 5]);
        assert_eq!(q(rules(&[(Attr::Raw, "T?EE")]), vec![]), vec![1, 5]);
        assert!(q(rules(&[(Attr::Raw, "TREE POLE")]), vec![]).is_empty());
        assert_eq!(q(rules(&[(Attr::Number, "1-3")]), vec![]), vec![1, 2, 3]);
        for bad in ["4-", "3 5", "x"] {
            assert_eq!(rule_terms(Attr::Number, bad), Err(Bad::Numbers));
        }
        for bad in ["abc", ">=20 <30"] {
            assert_eq!(rule_terms(Attr::Elevation, bad), Err(Bad::Mismatch));
        }
    }

    #[test]
    fn rows_select_as_the_rules_did() {
        let include = rules(&[(Attr::Raw, "TREE*"), (Attr::Elevation, ">25")]);
        let exclude = rules(&[(Attr::Number, "5")]);
        let rows = rows_of(&include, &exclude);
        let q = Query { custom: Some(rows), ..Default::default() };
        assert_eq!(q.members(&pts()).unwrap(), vec![1, 2, 3]);
        assert!(matches!(q.to_codec(), Ok(PointQuery::Custom(_))));
        assert_eq!(number_list(&[5, 1, 2, 3, 7, 8]), "1-3,5,7,8");
        assert_eq!(clean_name("A;B"), "A_B");
    }
}
