//! Numeric Lookup cell expressions use the drawing's existing formula engine.
use super::*;
use crate::scene::named_parameters::{self, ParameterTable};

#[derive(Clone, Debug)]
pub(super) enum Cell {
    Scalar(String),
    Interval {
        low: f64,
        high: f64,
        low_closed: bool,
        high_closed: bool,
    },
}

fn formula(source: &str) -> &str {
    let source = source.trim();
    source.strip_prefix('=').unwrap_or(source).trim()
}

fn finite(table: &ParameterTable, source: &str) -> Result<f64, &'static str> {
    table
        .evaluate_expression(formula(source))
        .ok()
        .filter(|v| v.is_finite())
        .ok_or("The selected Lookup row has an invalid parameter value.")
}

impl Cell {
    pub(super) fn parse(source: &str) -> Result<Self, &'static str> {
        let source = source.trim();
        if source.len() > 4096 {
            return Err("The Lookup table is invalid.");
        }
        let mut depth = 0i32;
        let mut operations = 0;
        for (_, ch) in syntax_chars(source) {
            match ch {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                '+' | '-' | '*' | '/' | '^' | ',' => operations += 1,
                _ => {}
            }
            if source.len() > 4096 || depth > 64 || operations > 256 {
                return Err("The Lookup table is invalid.");
            }
        }
        // Parenthesized arithmetic remains a scalar unless a top-level comma
        // separates interval endpoints. Function argument commas are nested.
        if source.len() >= 3
            && matches!(source.as_bytes()[0], b'[' | b'(')
            && matches!(source.as_bytes()[source.len() - 1], b']' | b')')
        {
            let inner = &source[1..source.len() - 1];
            let mut depth = 0i32;
            let mut comma = None;
            for (index, ch) in syntax_chars(inner) {
                match ch {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    ',' if depth == 0 => {
                        if comma.replace(index).is_some() {
                            return Err("The Lookup table is invalid.");
                        }
                    }
                    _ => {}
                }
            }
            if let Some(comma) = comma {
                let empty = ParameterTable::new();
                let low = finite(&empty, &inner[..comma])?;
                let high = finite(&empty, &inner[comma + 1..])?;
                let low_closed = source.starts_with('[');
                let high_closed = source.ends_with(']');
                let cell = Self::Interval {
                    low,
                    high,
                    low_closed,
                    high_closed,
                };
                if low > high || !cell.contains(cell.clamp(low)) {
                    return Err("The Lookup table is invalid.");
                }
                return Ok(cell);
            }
        }
        named_parameters::parse(formula(source)).map_err(|_| "The Lookup table is invalid.")?;
        Ok(Self::Scalar(source.into()))
    }

    pub(super) fn contains(&self, value: f64) -> bool {
        match self {
            Self::Interval {
                low,
                high,
                low_closed,
                high_closed,
            } => {
                value.is_finite()
                    && (value > *low || *low_closed && value == *low)
                    && (value < *high || *high_closed && value == *high)
            }
            _ => false,
        }
    }

    pub(super) fn clamp(&self, value: f64) -> f64 {
        match self {
            Self::Interval {
                low,
                high,
                low_closed,
                high_closed,
            } => {
                let a = if *low_closed {
                    *low
                } else {
                    adjacent(*low, true)
                };
                let b = if *high_closed {
                    *high
                } else {
                    adjacent(*high, false)
                };
                value.clamp(a.min(b), a.max(b))
            }
            _ => value,
        }
    }
}

// Move one representable number into an open interval, including signed zero.
fn adjacent(value: f64, up: bool) -> f64 {
    if value == 0. {
        return if up {
            f64::from_bits(1)
        } else {
            -f64::from_bits(1)
        };
    }
    let bits = value.to_bits();
    f64::from_bits(if up == (value > 0.) {
        bits + 1
    } else {
        bits - 1
    })
}

/// One row's resolved numeric inputs. Range variables are seeded from the
/// current frame; reverse selection clamps them before dependent formulas run.
pub(super) fn row(
    document: &Document,
    definition: Handle,
    table: &BlockLookupAction,
    row: usize,
    actual: &[f64],
    reverse: bool,
) -> Result<Vec<Option<(Cell, f64)>>, &'static str> {
    let mut selected = actual.to_vec();
    if reverse {
        for (index, column) in table.columns.iter().enumerate() {
            if column.lookup_property {
                continue;
            }
            let cell = Cell::parse(&table.expressions[row * table.columns.len() + index])?;
            if matches!(cell, Cell::Interval { .. }) {
                selected[index] = range_value(document, definition, column, &cell, actual[index])?;
            }
        }
    }
    resolve_row(
        &properties(document, definition),
        table,
        row,
        &selected,
        reverse,
    )
}

fn resolve_row(
    properties: &[Property],
    table: &BlockLookupAction,
    row: usize,
    actual: &[f64],
    reverse: bool,
) -> Result<Vec<Option<(Cell, f64)>>, &'static str> {
    let mut namespace = ParameterTable::new();
    let mut cells = Vec::new();
    for (index, column) in table.columns.iter().enumerate() {
        if column.lookup_property {
            cells.push(None);
            continue;
        }
        if column.value_type != 40 {
            return Err("This Lookup input property is unsupported.");
        }
        let source = table
            .expressions
            .get(row * table.columns.len() + index)
            .ok_or("The selected Lookup row is incomplete.")?;
        let cell = Cell::parse(source)?;
        let property = properties
            .iter()
            .find(|p| {
                p.column.node_id == column.node_id
                    && p.column
                        .connection_name
                        .eq_ignore_ascii_case(&column.connection_name)
            })
            .ok_or("A Lookup input property is unavailable.")?;
        if !property.label.is_empty() && !property.label.chars().any(char::is_control) {
            if namespace.contains(&property.label) {
                return Err("The Lookup table is invalid.");
            }
            let expression = match &cell {
                Cell::Scalar(source) => formula(source).to_owned(),
                Cell::Interval { .. } => (if reverse {
                    cell.clamp(actual[index])
                } else {
                    actual[index]
                })
                .to_string(),
            };
            namespace
                .bind_expression(&property.label, &expression)
                .map_err(|_| "The Lookup table is invalid.")?;
        }
        cells.push(Some(cell));
    }
    cells
        .into_iter()
        .enumerate()
        .map(|(index, cell)| {
            cell.map(|cell| {
                let value = match &cell {
                    Cell::Scalar(source) => finite(&namespace, source)?,
                    Cell::Interval { .. } => {
                        if reverse {
                            cell.clamp(actual[index])
                        } else {
                            actual[index]
                        }
                    }
                };
                Ok((cell, value))
            })
            .transpose()
        })
        .collect()
}

pub(super) fn actual_inputs(
    document: &Document,
    baseline: Option<&Document>,
    definition: Handle,
    insert: Handle,
    table: &BlockLookupAction,
) -> Result<Vec<f64>, &'static str> {
    let params =
        crate::scene::block_actions::all_parameters(baseline.unwrap_or(document), definition);
    table
        .columns
        .iter()
        .map(|property| {
            if property.lookup_property {
                return Ok(0.);
            }
            let p = params
                .iter()
                .find(|p| p.id == property.node_id)
                .ok_or("A Lookup input property is unavailable.")?;
            let frame = if baseline.is_some() {
                crate::scene::block_actions::instance::test_frame(document, insert, p)
            } else {
                crate::scene::block_actions::instance::frame(document, insert, p)
            }?;
            let delta = frame.end - frame.base;
            if property
                .connection_name
                .eq_ignore_ascii_case("UpdatedDistance")
                && (p.linear || p.polar)
            {
                Ok(delta.length())
            } else if property
                .connection_name
                .eq_ignore_ascii_case("UpdatedAngle")
                && p.polar
            {
                Ok(delta.y.atan2(delta.x).rem_euclid(std::f64::consts::TAU))
            } else {
                Err("This Lookup input property is unsupported.")
            }
        })
        .collect()
}

/// Validate formula syntax and constant domains without requiring row names.
pub(super) fn valid_cell(source: &str) -> Result<(), &'static str> {
    if source
        .trim()
        .parse::<f64>()
        .is_ok_and(|value| !value.is_finite())
    {
        return Err("The Lookup table is invalid.");
    }
    let cell = Cell::parse(source)?;
    if let Cell::Scalar(source) = cell {
        match ParameterTable::new().evaluate_expression(formula(&source)) {
            Ok(value) if value.is_finite() => {}
            Err(named_parameters::ParamError::UndefinedReference(_)) => {}
            _ => return Err("The Lookup table is invalid."),
        }
    }
    Ok(())
}

pub(super) fn validate_context(
    document: &Document,
    definition: Handle,
    table: &BlockLookupAction,
    original: Option<&BlockLookupAction>,
) -> Result<(), String> {
    validate_with_properties(table, original, &properties(document, definition))
}

pub(crate) fn validate_with_properties(
    table: &BlockLookupAction,
    original: Option<&BlockLookupAction>,
    properties: &[Property],
) -> Result<(), String> {
    let actual = table
        .columns
        .iter()
        .map(|column| {
            properties
                .iter()
                .find(|p| {
                    p.column.node_id == column.node_id
                        && p.column
                            .connection_name
                            .eq_ignore_ascii_case(&column.connection_name)
                })
                .and_then(|p| p.value.parse::<f64>().ok())
                .unwrap_or(0.)
        })
        .collect::<Vec<_>>();
    for index in 0..table.row_count as usize {
        if let Err(error) = resolve_row(properties, table, index, &actual, true) {
            // Retained inputs may use an opaque imported expression dialect.
            let retained = original.is_some_and(|old| {
                table.columns.iter().enumerate().all(|(column, property)| {
                    property.lookup_property
                        || old.columns.iter().enumerate().any(|(i, p)| {
                            p == property
                                && old
                                    .expressions
                                    .iter()
                                    .skip(i)
                                    .step_by(old.columns.len())
                                    .any(|cell| {
                                        cell == &table.expressions
                                            [index * table.columns.len() + column]
                                    })
                        })
                })
            });
            if !retained {
                return Err(error.into());
            }
        }
    }
    Ok(())
}

pub(super) fn constant_value(source: &str) -> Option<f64> {
    if !matches!(Cell::parse(source).ok()?, Cell::Scalar(_)) {
        return None;
    }
    finite(&ParameterTable::new(), source).ok()
}

/// Intersect an interval with native bounds/list/increment constraints before
/// dependent formulas read the chosen range value.
fn range_value(
    document: &Document,
    definition: Handle,
    column: &BlockLookupColumn,
    cell: &Cell,
    current: f64,
) -> Result<f64, &'static str> {
    let parameter = crate::scene::block_actions::all_parameters(document, definition)
        .into_iter()
        .find(|p| p.id == column.node_id)
        .ok_or("A Lookup input property is unavailable.")?;
    let Some(ObjectType::DynamicBlock(object)) = document.objects.get(&parameter.handle) else {
        return Err("A Lookup input property is unavailable.");
    };
    let angle = column.connection_name.eq_ignore_ascii_case("UpdatedAngle");
    let set = match &object.data {
        DynamicBlockData::LinearParameter(p) => &p.value_set,
        DynamicBlockData::PolarParameter(p) if angle => &p.angle_value_set,
        DynamicBlockData::PolarParameter(p) => &p.distance_value_set,
        _ => return Err("This Lookup input property is unsupported."),
    };
    let Cell::Interval { low, high, .. } = cell else {
        unreachable!()
    };
    let mut candidates = vec![cell.clamp(current), cell.clamp(*low), cell.clamp(*high)];
    candidates.extend(set.values.iter().copied().filter(|_| set.flags & 8 != 0));
    if set.flags & 1 != 0 {
        candidates.push(set.minimum);
    }
    if set.flags & 2 != 0 {
        candidates.push(set.maximum);
    }
    if set.flags & 4 != 0 && set.increment.is_finite() && set.increment > 0. {
        let origin = if set.flags & 1 != 0 { set.minimum } else { 0. };
        for value in candidates.clone() {
            let step = ((value - origin) / set.increment).round();
            for delta in [-1., 0., 1.] {
                candidates.push(origin + (step + delta) * set.increment);
            }
        }
    }
    candidates
        .into_iter()
        .filter(|v| v.is_finite())
        .map(|value| {
            if angle {
                crate::scene::block_actions::constrain_angle(value, set)
            } else {
                crate::scene::block_actions::constrain_value_set(value, set)
            }
        })
        .filter(|value| cell.contains(*value) && (angle || *value > 1e-9))
        .min_by(|a, b| {
            (a - current)
                .abs()
                .total_cmp(&(b - current).abs())
                .then(a.total_cmp(b))
        })
        .ok_or("The Lookup row values do not satisfy the parameter value sets.")
}

fn syntax_chars(source: &str) -> Vec<(usize, char)> {
    let mut chars = source.char_indices().peekable();
    let mut quoted = false;
    let mut result = Vec::new();
    while let Some((index, ch)) = chars.next() {
        if ch == '`' {
            if quoted && chars.peek().is_some_and(|(_, ch)| *ch == '`') {
                chars.next();
            } else {
                quoted = !quoted;
            }
        } else if !quoted {
            result.push((index, ch));
        }
    }
    result
}

pub(super) fn rename_cell(source: &str, old: &str, new: &str) -> Result<String, &'static str> {
    if !matches!(Cell::parse(source), Ok(Cell::Scalar(_))) {
        return Ok(source.into());
    }
    let body = formula(source);
    let renamed = match named_parameters::rename_reference(body, old, new) {
        Ok(value) => value,
        // Retained opaque syntax is preserved without claiming to understand it.
        Err(_) => return Err("The Lookup table is invalid."),
    };
    let offset = source.find(body).ok_or("The Lookup table is invalid.")?;
    let result = format!(
        "{}{}{}",
        &source[..offset],
        renamed,
        &source[offset + body.len()..]
    );
    Cell::parse(&result)?;
    Ok(result)
}

/// Identify the implemented expression dialect without requiring a particular
/// current numeric value (a temporary division by zero is not a rename).
pub(super) fn rewritable_row(
    properties: &[Property],
    table: &BlockLookupAction,
    row: usize,
) -> bool {
    use named_parameters::Expr;
    fn known(expr: &Expr, names: &std::collections::HashSet<&str>) -> bool {
        match expr {
            Expr::Number(_) => true,
            Expr::Ref(name) => names.contains(name.as_str()),
            Expr::Neg(a) => known(a, names),
            Expr::Add(a, b)
            | Expr::Sub(a, b)
            | Expr::Mul(a, b)
            | Expr::Div(a, b)
            | Expr::Pow(a, b) => known(a, names) && known(b, names),
            Expr::Call(name, args) => {
                named_parameters::is_reserved_name(name) && args.iter().all(|arg| known(arg, names))
            }
        }
    }
    let inputs = table
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.lookup_property)
        .map(|(i, c)| {
            properties
                .iter()
                .find(|p| {
                    p.column.node_id == c.node_id
                        && p.column
                            .connection_name
                            .eq_ignore_ascii_case(&c.connection_name)
                })
                .map(|p| (i, c, p))
        })
        .collect::<Option<Vec<_>>>();
    let Some(inputs) = inputs else {
        return false;
    };
    let names = inputs.iter().map(|(_, _, p)| p.label.as_str()).collect();
    let mut namespace = ParameterTable::new();
    inputs.iter().all(|(i, c, p)| {
        if c.value_type != 40 || namespace.contains(&p.label) {
            return false;
        }
        let source = &table.expressions[row * table.columns.len() + i];
        match Cell::parse(source) {
            Ok(Cell::Scalar(_)) => named_parameters::parse(formula(source)).is_ok_and(|expr| {
                known(&expr, &names) && namespace.bind_expression(&p.label, formula(source)).is_ok()
            }),
            Ok(Cell::Interval { .. }) => namespace.bind_expression(&p.label, "1").is_ok(),
            _ => false,
        }
    })
}
