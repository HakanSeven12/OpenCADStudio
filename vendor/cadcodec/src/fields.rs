//! Dynamic text field (`AcDbField`) evaluation engine.
//!
//! The library owns the field **language** (the DIESEL macro language and the
//! `AcVar` named fields), the field **structure** (the container → child → host
//! linkage recovered from object owners), and all the pure date math. It does
//! **not** read the system clock, the OS user, or environment variables — those
//! come from a caller-supplied [`FieldContext`], so the engine stays
//! deterministic and platform-neutral (no `std::time`, `getenv`, or web deps in
//! the core library).
//!
//! A field-hosting entity (usually an MTEXT) stores only the *cached* evaluated
//! text, frozen at the last save. [`resolve`] recomputes it against the current
//! context. Anything it can't evaluate — an unknown `getvar`, an unsupported
//! evaluator (`AcExpr` table sums, `AcObjProp` object properties) — yields
//! `None`, and the caller keeps the cached text.

use crate::document::{CadDocument, FieldDef};
use crate::entities::table::{CellValueType, Table};
use crate::entities::EntityType;
use crate::objects::ObjectType;
use crate::types::Handle;

/// Environment values the field engine cannot derive from the document alone.
/// Implemented by the host application; every method has a "don't know" default
/// (`None` / epoch) so a minimal host only needs [`now_julian`](FieldContext::now_julian).
pub trait FieldContext {
    /// "Now" as an astronomical Julian date. Drives `$(getvar,date)` /
    /// `$(getvar,cdate)`, `$(edtime,...)` on the current time, `$(time)`, and
    /// the `Date` AcVar field.
    fn now_julian(&self) -> f64;
    /// Current user / login name (`\AcVar Login`, `$(getvar,loginname)`).
    fn login(&self) -> Option<String> {
        None
    }
    /// OS environment variable (`$(getenv,name)`).
    fn getenv(&self, _name: &str) -> Option<String> {
        None
    }
    /// Any other system variable the host can answer (`$(getvar,name)`) beyond
    /// the ones the engine resolves itself. `None` keeps the cached field text.
    fn getvar(&self, _name: &str) -> Option<String> {
        None
    }
}

/// Re-evaluate the field hosted by entity `host` (usually an MTEXT), returning
/// fresh display text, or `None` when the entity hosts no field or the field
/// can't be fully evaluated (the caller then keeps the cached text).
pub fn resolve(doc: &CadDocument, host: Handle, ctx: &dyn FieldContext) -> Option<String> {
    if doc.fields.is_empty() {
        return None;
    }
    let container = container_for_host(doc, host)?;
    eval_field(doc, container, ctx, host)
}

/// Re-evaluate one specific field object for a known host.
///
/// Table cells reference their FIELD directly rather than through the single
/// `_text` container used by MTEXT. Exposing this narrow entry point lets a
/// host render those cells without guessing which field belongs to them.
pub fn resolve_handle(
    doc: &CadDocument,
    field_handle: Handle,
    host: Handle,
    ctx: &dyn FieldContext,
) -> Option<String> {
    let field = doc.fields.get(&field_handle)?;
    eval_field(doc, field, ctx, host)
}

// ── linkage ────────────────────────────────────────────────────────────────

/// Owner of an *object* handle (field / dictionary / other object). `None` when
/// the handle is not an object — i.e. it is an entity (the field's host).
fn owner_of(doc: &CadDocument, h: Handle) -> Option<Handle> {
    if let Some(f) = doc.fields.get(&h) {
        return Some(f.owner);
    }
    match doc.objects.get(&h)? {
        ObjectType::Dictionary(d) => Some(d.owner),
        ObjectType::Unknown { owner, .. } => Some(*owner),
        _ => None,
    }
}

/// Walk the owner chain up from a container field until the owner is not an
/// object — that handle is the host entity.
fn host_of(doc: &CadDocument, container: &FieldDef) -> Option<Handle> {
    let mut h = container.handle;
    for _ in 0..12 {
        let o = owner_of(doc, h)?;
        if owner_of(doc, o).is_none() {
            return Some(o);
        }
        h = o;
    }
    None
}

fn container_for_host(doc: &CadDocument, host: Handle) -> Option<&FieldDef> {
    doc.fields
        .values()
        .find(|f| f.evaluator == "_text" && host_of(doc, f) == Some(host))
}

// ── evaluation ─────────────────────────────────────────────────────────────

fn eval_field(
    doc: &CadDocument,
    field: &FieldDef,
    ctx: &dyn FieldContext,
    host: Handle,
) -> Option<String> {
    match field.evaluator.as_str() {
        "_text" => eval_template(doc, field, ctx, host),
        "AcVar" => eval_acvar(doc, &field.code, ctx),
        "AcDiesel" => {
            let expr = field
                .code
                .strip_prefix("\\AcDiesel ")
                .unwrap_or(&field.code)
                .trim();
            diesel_eval(doc, expr, ctx)
        }
        // AcExpr — a table cell formula (Sum/Average/… over A1-style cell refs).
        "AcExpr" => eval_acexpr(doc, &field.code, host),
        // AcObjProp[.ver] — a property of a referenced object.
        e if e.starts_with("AcObjProp") => eval_acobjprop(doc, field),
        _ => None,
    }
}

/// Substitute each `%<\_FldIdx N>%` marker in a container's template with the
/// evaluation of its Nth child field, keeping the surrounding MTEXT codes.
fn eval_template(
    doc: &CadDocument,
    container: &FieldDef,
    ctx: &dyn FieldContext,
    host: Handle,
) -> Option<String> {
    let mut children: Vec<&FieldDef> = doc
        .fields
        .values()
        .filter(|f| f.owner == container.handle)
        .collect();
    children.sort_by_key(|f| u64::from(f.handle));

    let mut out = String::new();
    let mut rest = container.code.as_str();
    while let Some(p) = rest.find("%<") {
        out.push_str(&rest[..p]);
        let after = &rest[p..];
        let Some(end) = after.find(">%") else {
            out.push_str(after);
            return Some(out);
        };
        let marker = &after[2..end]; // e.g. "\_FldIdx 0"
        if !marker.contains("_FldIdx") {
            return None;
        }
        let idx: usize = marker.rsplit(' ').next()?.trim().parse().ok()?;
        let child = children.get(idx)?;
        out.push_str(&eval_field(doc, child, ctx, host)?);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Some(out)
}

// ── AcExpr: table-cell formulas ──────────────────────────────────────────────

/// Evaluate an `AcExpr` field — a table-cell formula such as `(Sum(A3:B3))` or
/// `(A3*2+B4)`. Cell references resolve against the ACAD_TABLE that owns the
/// host cell (found via the host's block-record). Returns `None` (→ keep the
/// cached text) when the table or any referenced cell can't be resolved to a
/// number — e.g. a range that includes another formula cell, whose value is not
/// stored on the table entity.
fn eval_acexpr(doc: &CadDocument, code: &str, host: Handle) -> Option<String> {
    let expr = code.strip_prefix("\\AcExpr ").unwrap_or(code).trim();
    // Strip the single wrapping parenthesis AutoCAD stores around the formula.
    let expr = expr
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(expr)
        .trim();
    let table = table_for_host(doc, host)?;
    let mut p = ExprParser {
        s: expr.as_bytes(),
        i: 0,
        table,
    };
    let v = p.parse_expr()?;
    p.skip_ws();
    if p.i != p.s.len() {
        return None; // unparsed trailing input — don't guess
    }
    Some(num_str(v))
}

/// The ACAD_TABLE whose rendered block owns the host cell (its MTEXT sits in the
/// table's block-record), falling back to the sole table when a drawing has
/// exactly one.
fn table_for_host(doc: &CadDocument, host: Handle) -> Option<&Table> {
    if let Some(EntityType::Table(table)) = doc.get_entity(host) {
        return Some(table);
    }
    let owner = doc
        .entities()
        .find(|e| e.common().handle == host)
        .map(|e| e.common().owner_handle);
    if let Some(owner) = owner {
        if let Some(t) = doc.entities().find_map(|e| match e {
            EntityType::Table(t) if t.block_record_handle == Some(owner) => Some(t),
            _ => None,
        }) {
            return Some(t);
        }
    }
    let mut tables = doc.entities().filter_map(|e| match e {
        EntityType::Table(t) => Some(t),
        _ => None,
    });
    let first = tables.next()?;
    tables.next().is_none().then_some(first)
}

/// Numeric value of a table cell (0-based row/col). `None` for a non-numeric or
/// empty cell (e.g. another formula cell, whose result is not on the entity).
fn cell_num(table: &Table, col: usize, row: usize) -> Option<f64> {
    let cv = &table.rows.get(row)?.cells.get(col)?.contents.first()?.value;
    match cv.value_type {
        CellValueType::Long | CellValueType::Double => Some(cv.numeric_value),
        _ => {
            let s = if !cv.formatted_value.is_empty() {
                &cv.formatted_value
            } else {
                &cv.text
            };
            s.trim().parse::<f64>().ok()
        }
    }
}

/// Parse an A1-style cell reference at `*i`, returning 0-based `(col, row)`.
fn parse_cellref(s: &[u8], i: &mut usize) -> Option<(usize, usize)> {
    let start = *i;
    let mut col = 0usize;
    let mut has_col = false;
    while let Some(&b) = s.get(*i) {
        if b.is_ascii_alphabetic() {
            col = col * 26 + (b.to_ascii_uppercase() - b'A' + 1) as usize;
            *i += 1;
            has_col = true;
        } else {
            break;
        }
    }
    let mut row = 0usize;
    let mut has_row = false;
    while let Some(&b) = s.get(*i) {
        if b.is_ascii_digit() {
            row = row * 10 + (b - b'0') as usize;
            *i += 1;
            has_row = true;
        } else {
            break;
        }
    }
    if has_col && has_row && col >= 1 && row >= 1 {
        Some((col - 1, row - 1))
    } else {
        *i = start;
        None
    }
}

/// A tiny recursive-descent evaluator for table formulas: `+ - * /`, parentheses,
/// cell references, ranges (`A3:B3`) and the `Sum/Average/Count/Min/Max/Product`
/// aggregate functions.
struct ExprParser<'a> {
    s: &'a [u8],
    i: usize,
    table: &'a Table,
}

impl ExprParser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn skip_ws(&mut self) {
        while self.peek() == Some(b' ') {
            self.i += 1;
        }
    }
    fn parse_expr(&mut self) -> Option<f64> {
        let mut v = self.parse_term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'+') => {
                    self.i += 1;
                    v += self.parse_term()?;
                }
                Some(b'-') => {
                    self.i += 1;
                    v -= self.parse_term()?;
                }
                _ => break,
            }
        }
        Some(v)
    }
    fn parse_term(&mut self) -> Option<f64> {
        let mut v = self.parse_factor()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'*') => {
                    self.i += 1;
                    v *= self.parse_factor()?;
                }
                Some(b'/') => {
                    self.i += 1;
                    let d = self.parse_factor()?;
                    if d != 0.0 {
                        v /= d;
                    }
                }
                _ => break,
            }
        }
        Some(v)
    }
    fn parse_factor(&mut self) -> Option<f64> {
        self.skip_ws();
        match self.peek()? {
            b'(' => {
                self.i += 1;
                let v = self.parse_expr()?;
                self.skip_ws();
                if self.peek() == Some(b')') {
                    self.i += 1;
                }
                Some(v)
            }
            b'-' => {
                self.i += 1;
                Some(-self.parse_factor()?)
            }
            c if c.is_ascii_digit() || c == b'.' => {
                let start = self.i;
                while matches!(self.peek(), Some(b) if b.is_ascii_digit() || b == b'.') {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[start..self.i])
                    .ok()?
                    .parse()
                    .ok()
            }
            c if c.is_ascii_alphabetic() => self.parse_name(),
            _ => None,
        }
    }
    /// A letter run is either a function call `Name(...)` or a cell reference.
    fn parse_name(&mut self) -> Option<f64> {
        let start = self.i;
        while matches!(self.peek(), Some(b) if b.is_ascii_alphabetic()) {
            self.i += 1;
        }
        if self.peek() == Some(b'(') {
            let name = std::str::from_utf8(&self.s[start..self.i])
                .ok()?
                .to_lowercase();
            self.i += 1;
            let vals = self.parse_args()?;
            self.skip_ws();
            if self.peek() == Some(b')') {
                self.i += 1;
            }
            apply_func(&name, &vals)
        } else {
            self.i = start;
            let (col, row) = parse_cellref(self.s, &mut self.i)?;
            cell_num(self.table, col, row)
        }
    }
    /// Function arguments: comma-separated ranges (`A3:B3`) and/or expressions.
    fn parse_args(&mut self) -> Option<Vec<f64>> {
        let mut vals = Vec::new();
        loop {
            self.skip_ws();
            let save = self.i;
            if let Some((c1, r1)) = parse_cellref(self.s, &mut self.i) {
                self.skip_ws();
                if self.peek() == Some(b':') {
                    self.i += 1;
                    self.skip_ws();
                    let (c2, r2) = parse_cellref(self.s, &mut self.i)?;
                    for r in r1.min(r2)..=r1.max(r2) {
                        for c in c1.min(c2)..=c1.max(c2) {
                            vals.push(cell_num(self.table, c, r)?);
                        }
                    }
                } else {
                    self.i = save;
                    vals.push(self.parse_expr()?);
                }
            } else {
                vals.push(self.parse_expr()?);
            }
            self.skip_ws();
            if self.peek() == Some(b',') {
                self.i += 1;
            } else {
                break;
            }
        }
        Some(vals)
    }
}

fn apply_func(name: &str, vals: &[f64]) -> Option<f64> {
    match name {
        "sum" => Some(vals.iter().sum()),
        "average" | "mean" => {
            (!vals.is_empty()).then(|| vals.iter().sum::<f64>() / vals.len() as f64)
        }
        "count" => Some(vals.len() as f64),
        "min" => vals.iter().copied().reduce(f64::min),
        "max" => vals.iter().copied().reduce(f64::max),
        "product" => Some(vals.iter().product()),
        _ => None,
    }
}

// ── AcObjProp: object properties ─────────────────────────────────────────────

/// A resolved object-property value, before formatting.
enum PropVal {
    Num(f64),
    Point([f64; 3]),
}

/// Evaluate an `AcObjProp` field — a property of a referenced object, e.g.
/// `Object(%<\_ObjIdx 0>%).Center \f "%lu2%pr8%ps[X=,]%zs8"`. The object is the
/// field's `objects[N]` handle. Geometry properties (Center / Area / Length /
/// Radius / …) are computed from the entity; view- and section-specific
/// properties, or references to objects the reader keeps only as raw bytes,
/// yield `None` (→ cached text).
fn eval_acobjprop(doc: &CadDocument, field: &FieldDef) -> Option<String> {
    let code = &field.code;
    let idx: usize = between(code, "_ObjIdx ", ">%")?.trim().parse().ok()?;
    let handle = field.objects.get(idx)?;
    let prop = code.split(").").nth(1)?.split([' ', '\\']).next()?.trim();
    if prop.is_empty() {
        return None;
    }
    let fmt = code
        .find("\\f ")
        .map(|p| code[p + 3..].trim().trim_matches('"'))
        .unwrap_or("");
    let entity = doc.entities().find(|e| &e.common().handle == handle)?;
    let val = object_property(entity, prop)?;
    Some(format_propval(val, fmt))
}

/// Substring strictly between `a` and the next `b` following it.
fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let start = s.find(a)? + a.len();
    let end = s[start..].find(b)? + start;
    Some(&s[start..end])
}

/// The substring immediately after `a` (to the end).
fn after<'a>(s: &'a str, a: &str) -> Option<&'a str> {
    s.find(a).map(|p| &s[p + a.len()..])
}

fn object_property(e: &EntityType, prop: &str) -> Option<PropVal> {
    use std::f64::consts::PI;
    match prop {
        "Center" => center(e).map(PropVal::Point),
        "StartPoint" => match e {
            EntityType::Line(l) => Some([l.start.x, l.start.y, l.start.z]),
            _ => None,
        }
        .map(PropVal::Point),
        "EndPoint" => match e {
            EntityType::Line(l) => Some([l.end.x, l.end.y, l.end.z]),
            _ => None,
        }
        .map(PropVal::Point),
        "Radius" => radius(e).map(PropVal::Num),
        "Diameter" => radius(e).map(|r| PropVal::Num(2.0 * r)),
        "Circumference" => radius(e).map(|r| PropVal::Num(2.0 * PI * r)),
        "Area" => area(e).map(PropVal::Num),
        "Length" | "Perimeter" => length(e).map(PropVal::Num),
        // View/section-specific properties (StartIdentifier, EndIdentifier,
        // StandardScaleViewLabel, …) live on objects the reader keeps as raw
        // bytes — not evaluable, so the cache is kept.
        _ => None,
    }
}

fn center(e: &EntityType) -> Option<[f64; 3]> {
    match e {
        EntityType::Circle(c) => Some([c.center.x, c.center.y, c.center.z]),
        EntityType::Arc(a) => Some([a.center.x, a.center.y, a.center.z]),
        EntityType::Ellipse(el) => Some([el.center.x, el.center.y, el.center.z]),
        EntityType::Line(l) => Some([
            (l.start.x + l.end.x) / 2.0,
            (l.start.y + l.end.y) / 2.0,
            (l.start.z + l.end.z) / 2.0,
        ]),
        _ => None,
    }
}

fn radius(e: &EntityType) -> Option<f64> {
    match e {
        EntityType::Circle(c) => Some(c.radius),
        EntityType::Arc(a) => Some(a.radius),
        _ => None,
    }
}

fn area(e: &EntityType) -> Option<f64> {
    use std::f64::consts::PI;
    match e {
        EntityType::Circle(c) => Some(PI * c.radius * c.radius),
        _ => None,
    }
}

fn length(e: &EntityType) -> Option<f64> {
    use std::f64::consts::PI;
    match e {
        EntityType::Line(l) => {
            let (dx, dy, dz) = (
                l.end.x - l.start.x,
                l.end.y - l.start.y,
                l.end.z - l.start.z,
            );
            Some((dx * dx + dy * dy + dz * dz).sqrt())
        }
        EntityType::Circle(c) => Some(2.0 * PI * c.radius),
        EntityType::Arc(a) => {
            let mut sweep = a.end_angle - a.start_angle;
            while sweep < 0.0 {
                sweep += 2.0 * PI;
            }
            Some(a.radius * sweep)
        }
        _ => None,
    }
}

/// Format a property value with an AutoCAD field unit picture. Understands the
/// common codes: `%pr<n>` precision, `%ps[pre,suf]` prefix/suffix, `%zs<n>`
/// trailing-zero suppression. A point renders one coordinate when the prefix
/// names it (`X=`, `Y=`, `Z=`), else `x,y`.
fn format_propval(val: PropVal, pic: &str) -> String {
    let prec = after(pic, "%pr")
        .and_then(|s| {
            s.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        })
        .unwrap_or(4);
    let (pre, suf) = between(pic, "%ps[", "]")
        .map(|ps| {
            let mut it = ps.splitn(2, ',');
            (
                it.next().unwrap_or("").to_string(),
                it.next().unwrap_or("").to_string(),
            )
        })
        .unwrap_or_default();
    let zs = pic.contains("%zs");
    let num = |v: f64| {
        let mut s = format!("{:.*}", prec, v);
        if zs && s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        s
    };
    match val {
        PropVal::Num(n) => format!("{}{}{}", pre, num(n), suf),
        PropVal::Point(p) => {
            let coord = match pre.chars().next() {
                Some('X') | Some('x') => Some(p[0]),
                Some('Y') | Some('y') => Some(p[1]),
                Some('Z') | Some('z') => Some(p[2]),
                _ => None,
            };
            match coord {
                Some(c) => format!("{}{}{}", pre, num(c), suf),
                None => format!("{}{},{}{}", pre, num(p[0]), num(p[1]), suf),
            }
        }
    }
}

fn eval_acvar(doc: &CadDocument, code: &str, ctx: &dyn FieldContext) -> Option<String> {
    let body = code.strip_prefix("\\AcVar ").unwrap_or(code).trim();
    let (name, fmt) = match body.find("\\f ") {
        Some(fp) => (body[..fp].trim(), body[fp + 3..].trim().trim_matches('"')),
        None => (body, "yyyy/MM/dd"),
    };
    let si = &doc.summary_info;
    match name {
        // System / clock / user.
        "Login" => ctx.login(),
        "CreateDate" => Some(format_dt(julian_parts(doc.header.create_date_julian), fmt)),
        "SaveDate" => Some(format_dt(julian_parts(doc.header.update_date_julian), fmt)),
        "PlotDate" => Some(format_dt(julian_parts(ctx.now_julian()), fmt)),
        "Date" => Some(format_dt(julian_parts(ctx.now_julian()), fmt)),
        // Document summary properties (DWGPROPS / SummaryInfo section).
        "Author" => nonempty(&si.author),
        "Title" => nonempty(&si.title),
        "Subject" => nonempty(&si.subject),
        "Keywords" => nonempty(&si.keywords),
        "Comments" => nonempty(&si.comments),
        "HyperlinkBase" => nonempty(&si.hyperlink_base),
        "RevisionNumber" => nonempty(&si.revision_number),
        "LastSavedBy" => {
            nonempty(&si.last_saved_by).or_else(|| nonempty(&doc.header.last_saved_by))
        }
        // File provenance.
        "Filename" | "FileName" => filename(doc),
        "FilePath" => filepath(doc),
        // Otherwise try a custom document property of this name.
        _ => si
            .custom_properties
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .and_then(|(_, v)| nonempty(v)),
    }
}

fn nonempty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// The drawing file name with extension (the common `Filename` display).
fn filename(doc: &CadDocument) -> Option<String> {
    let p = doc.source_path.as_deref()?;
    let base = p.rsplit(['/', '\\']).next().unwrap_or(p);
    nonempty(base)
}

/// The full path the drawing was read from (`FilePath`).
fn filepath(doc: &CadDocument) -> Option<String> {
    doc.source_path.as_deref().and_then(nonempty)
}

// ── DIESEL ─────────────────────────────────────────────────────────────────

/// Evaluate a DIESEL string (literals interspersed with `$(func,args)`).
fn diesel_eval(doc: &CadDocument, s: &str, ctx: &dyn FieldContext) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '(' {
            let mut depth = 0;
            let mut j = i + 1;
            while j < chars.len() {
                match chars[j] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let inner: String = chars[i + 2..j].iter().collect();
            out.push_str(&diesel_macro(doc, &inner, ctx)?);
            i = j + 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Some(out)
}

/// Evaluate one `func,args` DIESEL macro. Implements the full standard function
/// set; an unknown function concatenates its arguments (DIESEL's own fallback).
fn diesel_macro(doc: &CadDocument, inner: &str, ctx: &dyn FieldContext) -> Option<String> {
    let parts = split_top_commas(inner);
    let func = parts[0].trim().to_lowercase();
    let raw: Vec<&str> = parts[1..].iter().map(|s| s.trim()).collect();
    let ev = |i: usize| -> Option<String> {
        raw.get(i)
            .and_then(|a| diesel_eval(doc, a.trim_matches('"'), ctx))
    };

    // Control-flow functions evaluate their branches lazily.
    match func.as_str() {
        "if" => {
            let cond = ev(0)?;
            let take = cond
                .trim()
                .parse::<f64>()
                .map(|n| n != 0.0)
                .unwrap_or(!cond.trim().is_empty());
            return if take {
                ev(1)
            } else if raw.len() > 2 {
                ev(2)
            } else {
                Some(String::new())
            };
        }
        "nth" => {
            let w: usize = ev(0)?.trim().parse().ok()?;
            return ev(1 + w).or(Some(String::new()));
        }
        "index" => {
            let w: usize = ev(0)?.trim().parse().ok()?;
            return Some(ev(1)?.split(',').nth(w).unwrap_or("").to_string());
        }
        _ => {}
    }

    // Everything else evaluates all arguments first.
    let mut args = Vec::with_capacity(raw.len());
    for i in 0..raw.len() {
        args.push(ev(i)?);
    }
    let a = |i: usize| args.get(i).cloned().unwrap_or_default();
    let n = |i: usize| a(i).trim().parse::<f64>().unwrap_or(0.0);
    let ni = |i: usize| a(i).trim().parse::<i64>().unwrap_or(0);

    Some(match func.as_str() {
        "getvar" => return getvar(doc, &a(0), ctx),
        "getenv" => ctx.getenv(&a(0)).unwrap_or_default(),
        "eval" => return diesel_eval(doc, &a(0), ctx),
        "substr" => {
            let chars: Vec<char> = a(0).chars().collect();
            let s0 = args
                .get(1)
                .and_then(|x| x.parse::<usize>().ok())
                .unwrap_or(1)
                .saturating_sub(1);
            let end = match args.get(2).and_then(|x| x.parse::<usize>().ok()) {
                Some(l) => (s0 + l).min(chars.len()),
                None => chars.len(),
            };
            chars
                .get(s0..end)
                .map(|c| c.iter().collect())
                .unwrap_or_default()
        }
        "strlen" => a(0).chars().count().to_string(),
        "upper" => a(0).to_uppercase(),
        "strfill" => a(0).repeat(ni(1).max(0) as usize),
        "eq" => bool_str(a(0) == a(1)),
        "=" => bool_str(n(0) == n(1)),
        "!=" => bool_str(n(0) != n(1)),
        "<" => bool_str(n(0) < n(1)),
        ">" => bool_str(n(0) > n(1)),
        "<=" => bool_str(n(0) <= n(1)),
        ">=" => bool_str(n(0) >= n(1)),
        "and" => (0..args.len())
            .map(&ni)
            .fold(!0i64, |acc, x| acc & x)
            .to_string(),
        "or" => (0..args.len())
            .map(&ni)
            .fold(0i64, |acc, x| acc | x)
            .to_string(),
        "xor" => (0..args.len())
            .map(&ni)
            .fold(0i64, |acc, x| acc ^ x)
            .to_string(),
        "+" => num_str((0..args.len()).map(&n).sum()),
        "*" => num_str((0..args.len()).map(&n).product()),
        "-" => num_str((1..args.len()).map(&n).fold(n(0), |acc, x| acc - x)),
        "/" => num_str(
            (1..args.len())
                .map(&n)
                .fold(n(0), |acc, x| if x != 0.0 { acc / x } else { acc }),
        ),
        "fix" => (n(0).trunc() as i64).to_string(),
        "rtos" => rtos(n(0), args.get(2).and_then(|x| x.parse().ok())),
        "angtos" => angtos(n(0), ni(1), args.get(2).and_then(|x| x.parse().ok())),
        "edtime" => edtime(n(0), &a(1)),
        "time" => (((ctx.now_julian() - 2_440_587.5) * 86_400.0).round() as i64).to_string(),
        // Unknown function — DIESEL concatenates the evaluated arguments.
        _ => args.join(""),
    })
}

/// AutoCAD system variables the engine answers directly (from the document +
/// context clock); anything else is delegated to the host via
/// [`FieldContext::getvar`]. `None` keeps the cached field text.
fn getvar(doc: &CadDocument, name: &str, ctx: &dyn FieldContext) -> Option<String> {
    let key = name.trim().trim_start_matches('*').to_lowercase();
    match key.as_str() {
        "cdate" => {
            let (y, mo, d, h, mi, s) = julian_parts(ctx.now_julian());
            Some(format!(
                "{:04}{:02}{:02}.{:02}{:02}{:02}",
                y, mo, d, h, mi, s
            ))
        }
        "date" => Some(format!("{:.6}", ctx.now_julian())),
        "loginname" => ctx.login(),
        "tdcreate" | "tducreate" => Some(format!("{:.6}", doc.header.create_date_julian)),
        "tdupdate" | "tduupdate" => Some(format!("{:.6}", doc.header.update_date_julian)),
        _ => ctx.getvar(name),
    }
}

fn split_top_commas(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                parts.push(cur.clone());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    parts.push(cur);
    parts
}

fn bool_str(b: bool) -> String {
    if b { "1" } else { "0" }.to_string()
}

// ── pure numeric / date formatting ─────────────────────────────────────────

fn num_str(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{}", x)
    }
}

/// `$(rtos,value[,mode,prec])` — decimal with `prec` fractional digits (4).
fn rtos(val: f64, prec: Option<usize>) -> String {
    format!("{:.*}", prec.unwrap_or(4), val)
}

/// `$(angtos,value[,mode,prec])` — `value` in radians; mode 3 = radians, else degrees.
fn angtos(val: f64, mode: i64, prec: Option<usize>) -> String {
    let p = prec.unwrap_or(0);
    if mode == 3 {
        format!("{:.*}r", p, val)
    } else {
        format!("{:.*}", p, val.to_degrees())
    }
}

/// Gregorian (Y, M, D, h, m, s) for an astronomical Julian date.
pub fn julian_parts(jd: f64) -> (i64, u32, u32, u32, u32, u32) {
    if jd <= 0.0 {
        return (0, 1, 1, 0, 0, 0);
    }
    let secs = ((jd - 2_440_587.5) * 86_400.0).round() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // days → civil date (Howard Hinnant).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32, h as u32, mi as u32, s as u32)
}

/// Day of week for a Julian date: 0 = Sunday … 6 = Saturday.
fn weekday(jd: f64) -> u32 {
    let secs = ((jd - 2_440_587.5) * 86_400.0).round() as i64;
    let days = secs.div_euclid(86_400);
    (days + 4).rem_euclid(7) as u32
}

/// Format (Y, M, D, h, m, s) with a .NET-style picture (`yyyy`, `yy`, `MM`,
/// `dd`, `HH`, `mm`, `ss`) — used by `\AcVar … \f "…"`.
pub fn format_dt(dt: (i64, u32, u32, u32, u32, u32), fmt: &str) -> String {
    let (y, mo, d, h, mi, s) = dt;
    fmt.replace("yyyy", &format!("{:04}", y))
        .replace("MM", &format!("{:02}", mo))
        .replace("dd", &format!("{:02}", d))
        .replace("HH", &format!("{:02}", h))
        .replace("mm", &format!("{:02}", mi))
        .replace("ss", &format!("{:02}", s))
        .replace("yy", &format!("{:02}", (y % 100).unsigned_abs()))
}

/// `$(edtime,time,picture)` — format the Julian `time` per a DIESEL picture
/// (case-sensitive tokens; longest match first). `MM` = minutes, `MO` = month.
fn edtime(jd: f64, pic: &str) -> String {
    let (y, mo, d, h, mi, s) = julian_parts(jd);
    let wd = weekday(jd);
    let ampm = pic.contains("AM/PM")
        || pic.contains("am/pm")
        || pic.contains("A/P")
        || pic.contains("a/p");
    let hh = if ampm {
        let x = h % 12;
        if x == 0 {
            12
        } else {
            x
        }
    } else {
        h
    };
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const DAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let month = MONTHS[(mo as usize).clamp(1, 12) - 1];
    let day = DAYS[(wd as usize) % 7];
    let toks: [(&str, String); 17] = [
        ("MONTH", month.to_string()),
        ("MON", month[..3].to_string()),
        ("MO", format!("{:02}", mo)),
        ("DDDD", day.to_string()),
        ("DDD", day[..3].to_string()),
        ("DD", format!("{:02}", d)),
        ("YYYY", format!("{:04}", y)),
        ("YY", format!("{:02}", (y % 100).unsigned_abs())),
        ("HH", format!("{:02}", hh)),
        ("MM", format!("{:02}", mi)),
        ("SS", format!("{:02}", s)),
        ("AM/PM", if h < 12 { "AM" } else { "PM" }.to_string()),
        ("am/pm", if h < 12 { "am" } else { "pm" }.to_string()),
        ("A/P", if h < 12 { "A" } else { "P" }.to_string()),
        ("a/p", if h < 12 { "a" } else { "p" }.to_string()),
        ("M", format!("{}", mo)),
        ("D", format!("{}", d)),
    ];
    let chars: Vec<char> = pic.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    'scan: while i < chars.len() {
        for (tok, val) in &toks {
            let tl = tok.chars().count();
            if i + tl <= chars.len() && chars[i..i + tl].iter().collect::<String>() == *tok {
                out.push_str(val);
                i += tl;
                continue 'scan;
            }
        }
        if chars[i] == 'H' {
            out.push_str(&format!("{}", hh));
            i += 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn julian_and_edtime() {
        // 2452257.345 = 2001-12-13, a Thursday.
        assert_eq!(julian_parts(2452257.345).0, 2001);
        assert_eq!(julian_parts(2452257.345).1, 12);
        assert_eq!(julian_parts(2452257.345).2, 13);
        assert_eq!(weekday(2452257.345), 4);
        assert_eq!(edtime(2452257.345, "YYYY/MO/DD"), "2001/12/13");
        assert_eq!(
            edtime(2452257.345, "DDDD, MONTH D, YYYY"),
            "Thursday, December 13, 2001"
        );
        assert_eq!(edtime(2452257.345, "DD.MON.YY"), "13.Dec.01");
        assert_eq!(
            format_dt(julian_parts(2452257.345), "yyyy/MM/dd"),
            "2001/12/13"
        );
        assert_eq!(num_str(5.0), "5");
        assert_eq!(rtos(3.14159, Some(2)), "3.14");
    }
}
