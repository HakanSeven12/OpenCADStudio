//! Generic data nodes: formulas with conditions, list reshaping, text
//! formatting and CSV files. None of them touches the drawing, so they work
//! for any kind of project; [`ops::LIBRARY`](super::ops::LIBRARY) lists them
//! under the graph's own Math, Logic, List and Text categories.

use super::expr::{self, Val};
use graph::value::{boolean, list, num, number, point, text};
use serde_json::Value;

/// Upper bound on generated list lengths, as in the graph's own library.
const MAX_ITEMS: usize = 100_000;

/// Columns a Read CSV node offers as outputs.
pub const CSV_COLUMNS: usize = 8;

fn out(value: Value) -> Vec<Value> {
    vec![value]
}

fn numbers(value: &Value) -> Vec<f64> {
    list(value).iter().filter_map(num).collect()
}

/// Display text, with whole numbers written without a fraction (`4`, not
/// `4.0`), so numbers read naturally in labels and files.
fn plain(value: &Value) -> String {
    match num(value).filter(|_| value.is_number()) {
        Some(x) if x.fract() == 0.0 && x.abs() < 1e15 => format!("{}", x as i64),
        _ => text(value),
    }
}

/// The variables an Expression node gives its formula, in input order.
const EXPRESSION_VARIABLES: &[&str] = &["a", "b", "c", "d", "x", "y"];

/// Expression: a formula over `a`..`d`, `x` and `y` that may also compare
/// and decide (`a > b ? a : b`, `x >= 0 && x <= 10`). A comparison gives a
/// boolean; anything that does not read gives `null`.
pub fn expression(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    let var = |name: &str| {
        EXPRESSION_VARIABLES
            .iter()
            .position(|v| *v == name)
            .and_then(|k| num(&a[1 + k]))
    };
    out(match expr::eval(&text(&a[0]), &var) {
        Ok(Val::Bool(b)) => Value::Bool(b),
        Ok(Val::Num(x)) if x.is_finite() => number(x),
        _ => Value::Null,
    })
}

/// Whether `item`, the `k`-th of `count`, passes a Filter condition. The
/// condition reads the item as `x`, a point's coordinates as `px`, `py` and
/// `pz`, the item's index as `i` and the list's size as `n`. `None` when the
/// condition does not read.
fn item_test(condition: &str, item: &Value, k: usize, count: usize) -> Option<bool> {
    let p = point(item);
    let var = |name: &str| match name {
        "x" => num(item).or(Some(f64::NAN)),
        "i" => Some(k as f64),
        "n" => Some(count as f64),
        "px" => p.map(|p| p[0]),
        "py" => p.map(|p| p[1]),
        "pz" => p.map(|p| p[2]),
        _ => None,
    };
    expr::eval(condition, &var).ok().map(Val::truth)
}

/// Filter: splits a list by a condition into the items that pass, those
/// that don't, and the indices of the passing ones.
pub fn filter(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    let items = list(&a[0]);
    let condition = text(&a[1]);
    let (mut kept, mut dropped, mut index) = (Vec::new(), Vec::new(), Vec::new());
    for (k, item) in items.iter().enumerate() {
        match item_test(&condition, item, k, items.len()) {
            Some(true) => {
                kept.push(item.clone());
                index.push(number(k as f64));
            }
            Some(false) => dropped.push(item.clone()),
            None => return vec![Value::Null; 3],
        }
    }
    vec![Value::Array(kept), Value::Array(dropped), Value::Array(index)]
}

/// Choose: the option at `index`, whole (a list stays a list).
pub fn choose(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    out(num(&a[0])
        .filter(|i| *i >= 0.0)
        .and_then(|i| a.get(1 + i as usize))
        .cloned()
        .unwrap_or(Value::Null))
}

/// Interpolate Values: linear interpolation of `y` at `x` over the points
/// `(x list, y list)`; beyond the ends the end values hold.
pub fn interpolate_values(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    out(num(&a[2])
        .and_then(|x| interpolate(&numbers(&a[0]), &numbers(&a[1]), x))
        .map_or(Value::Null, number))
}

fn interpolate(xs: &[f64], ys: &[f64], x: f64) -> Option<f64> {
    let mut points: Vec<(f64, f64)> = xs.iter().copied().zip(ys.iter().copied()).collect();
    points.sort_by(|p, q| p.0.total_cmp(&q.0));
    let (first, last) = (points.first()?, points.last()?);
    if x <= first.0 {
        return Some(first.1);
    }
    if x >= last.0 {
        return Some(last.1);
    }
    let k = points.partition_point(|p| p.0 <= x);
    let ((x0, y0), (x1, y1)) = (points[k - 1], points[k]);
    Some(if x1 > x0 { y0 + (y1 - y0) * (x - x0) / (x1 - x0) } else { y1 })
}

/// Zip: the lists given as columns, read across into rows.
pub fn zip(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    let columns: Vec<Vec<Value>> = a.iter().filter(|value| !value.is_null()).map(list).collect();
    let rows = columns.iter().map(Vec::len).min().unwrap_or(0);
    out(Value::Array(
        (0..rows)
            .map(|k| Value::Array(columns.iter().map(|column| column[k].clone()).collect()))
            .collect(),
    ))
}

/// Repeat List: the list over and over, `times` times or up to `length`
/// items when that is set.
pub fn repeat_list(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    let items = list(&a[0]);
    let length = num(&a[2]).unwrap_or(0.0).max(0.0) as usize;
    let total = if length > 0 {
        length
    } else {
        items.len().saturating_mul(num(&a[1]).unwrap_or(1.0).max(0.0) as usize)
    };
    out(Value::Array(if items.is_empty() {
        Vec::new()
    } else {
        items.iter().cycle().take(total.min(MAX_ITEMS)).cloned().collect()
    }))
}

/// Cull Pattern: keeps the items where a repeating true/false pattern is
/// true.
pub fn cull_pattern(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    let pattern: Vec<bool> = list(&a[1]).iter().map(|keep| boolean(keep).unwrap_or(false)).collect();
    out(if pattern.is_empty() {
        a[0].clone()
    } else {
        Value::Array(
            list(&a[0])
                .into_iter()
                .enumerate()
                .filter(|(k, _)| pattern[k % pattern.len()])
                .map(|(_, item)| item)
                .collect(),
        )
    })
}

/// Weave: the items of two lists taken in turn.
pub fn weave(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    let (xs, ys) = (list(&a[0]), list(&a[1]));
    let mut woven = Vec::with_capacity(xs.len() + ys.len());
    for k in 0..xs.len().max(ys.len()) {
        woven.extend(xs.get(k).cloned());
        woven.extend(ys.get(k).cloned());
    }
    out(Value::Array(woven))
}

/// Cross Reference: every pairing of the items of two lists, as two lists
/// read in step.
pub fn cross_reference(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    let (xs, ys) = (list(&a[0]), list(&a[1]));
    let pairs = xs.len().saturating_mul(ys.len()).min(MAX_ITEMS);
    let (mut left, mut right) = (Vec::with_capacity(pairs), Vec::with_capacity(pairs));
    'all: for x in &xs {
        for y in &ys {
            if left.len() == pairs {
                break 'all;
            }
            left.push(x.clone());
            right.push(y.clone());
        }
    }
    vec![Value::Array(left), Value::Array(right)]
}

/// Graft: each item in a list of its own.
pub fn graft(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    out(Value::Array(list(&a[0]).into_iter().map(|item| Value::Array(vec![item])).collect()))
}

/// Flatten Once: removes one level of nesting.
pub fn flatten_once(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    out(Value::Array(list(&a[0]).into_iter().flat_map(|item| list(&item)).collect()))
}

/// List Depth: how deeply the list nests (a plain list is 1).
pub fn list_depth(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    fn depth(value: &Value) -> usize {
        match value {
            Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
            _ => 0,
        }
    }
    out(number(depth(&a[0]) as f64))
}

/// `x` with `decimals` decimals, at least `digits` integer digits (zero
/// padded), the integer digits in groups of `group` from the right joined by
/// `separator` (none when 0): 1234.5 as `1,234.50`, or 30 as the station
/// `0+030.00` with 4 digits in groups of 3 joined by `+`.
fn format_number(x: f64, decimals: usize, digits: usize, group: usize, separator: &str, mark: &str) -> String {
    let fixed = format!("{:.*}", decimals, x.abs());
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let whole = format!("{whole:0>digits$}");
    let mut grouped = String::new();
    for (k, digit) in whole.chars().enumerate() {
        if group > 0 && k > 0 && (whole.len() - k) % group == 0 {
            grouped.push_str(separator);
        }
        grouped.push(digit);
    }
    let negative = x < 0.0 && fixed.chars().any(|c| c.is_ascii_digit() && c != '0');
    let mut result = if negative { format!("-{grouped}") } else { grouped };
    if !fraction.is_empty() {
        result.push_str(mark);
        result.push_str(fraction);
    }
    result
}

/// Format Number: decimals, zero padding and digit grouping.
pub fn format_number_node(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    out(match num(&a[0]) {
        Some(x) => Value::String(format_number(
            x,
            num(&a[1]).unwrap_or(0.0).clamp(0.0, 12.0) as usize,
            num(&a[2]).unwrap_or(1.0).clamp(1.0, 32.0) as usize,
            num(&a[3]).unwrap_or(0.0).clamp(0.0, 32.0) as usize,
            &text(&a[4]),
            &text(&a[5]),
        )),
        None => Value::Null,
    })
}

/// Fills `{a}`..`{d}` (or `{0}`..`{3}`) in `template` with `values`. A
/// format after a colon pads and rounds numbers: `{a:000}` gives three
/// integer digits, `{a:.2}` two decimals, `{a:0.00}` both. `{{` and `}}`
/// write braces.
fn format_text(template: &str, values: &[Value]) -> String {
    let mut result = String::new();
    let mut rest = template;
    while let Some(start) = rest.find(['{', '}']) {
        result.push_str(&rest[..start]);
        let tail = &rest[start..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            result.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        let Some(end) = tail.find('}').filter(|_| tail.starts_with('{')) else {
            result.push_str(&tail[..1]);
            rest = &tail[1..];
            continue;
        };
        let field = &tail[1..end];
        let (name, format) = field.split_once(':').unwrap_or((field, ""));
        let slot = match name.trim() {
            "a" | "0" => Some(0),
            "b" | "1" => Some(1),
            "c" | "2" => Some(2),
            "d" | "3" => Some(3),
            _ => None,
        };
        match slot.and_then(|k| values.get(k)) {
            Some(value) => {
                let (whole, fraction) = format.split_once('.').unwrap_or((format, ""));
                let padded =
                    whole.chars().all(|c| c == '0') && fraction.chars().all(|c| c.is_ascii_digit());
                match num(value).filter(|_| !format.is_empty() && padded) {
                    Some(x) => {
                        let decimals = fraction
                            .parse::<usize>()
                            .ok()
                            .filter(|_| !fraction.starts_with('0'))
                            .unwrap_or(fraction.len())
                            .min(12);
                        result.push_str(&format_number(x, decimals, whole.len().clamp(1, 32), 0, "", "."));
                    }
                    None => result.push_str(&plain(value)),
                }
            }
            None => result.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    result.push_str(rest);
    result
}

/// Format Text: a template with `{a}`..`{d}` placeholders.
pub fn format_text_node(a: &[Value], _: &dyn graph::Host) -> Vec<Value> {
    out(Value::String(format_text(&text(&a[0]), &a[1..])))
}

/// The rows of CSV `text`: the separator is the first of tab, `;` and `,`
/// found outside quotes; quoted cells may hold separators, doubled quotes
/// and line breaks. Cells that read as numbers become numbers.
pub fn csv_rows(source: &str) -> Vec<Vec<Value>> {
    let separator = {
        let mut quoted = false;
        let mut found = None;
        for ch in source.chars() {
            match ch {
                '"' => quoted = !quoted,
                '\t' | ';' | ',' if !quoted => {
                    found = Some(ch);
                    if ch != ',' {
                        break;
                    }
                }
                '\n' if !quoted && found.is_some() => break,
                _ => {}
            }
        }
        found.unwrap_or(',')
    };
    let cell = |raw: String, was_quoted: bool| match raw.trim().parse::<f64>() {
        Ok(x) if !was_quoted => number(x),
        _ => Value::String(if was_quoted { raw } else { raw.trim().to_owned() }),
    };
    let mut rows = Vec::new();
    let (mut row, mut field, mut quoted, mut was_quoted) = (Vec::new(), String::new(), false, false);
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' if quoted => quoted = false,
            '"' if field.trim().is_empty() => {
                field.clear();
                quoted = true;
                was_quoted = true;
            }
            c if c == separator && !quoted => {
                row.push(cell(std::mem::take(&mut field), was_quoted));
                was_quoted = false;
            }
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(cell(std::mem::take(&mut field), was_quoted));
                was_quoted = false;
                let line = std::mem::take(&mut row);
                if line.len() > 1 || !text(&line[0]).is_empty() {
                    rows.push(line);
                }
            }
            c => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() || was_quoted {
        row.push(cell(field, was_quoted));
        rows.push(row);
    }
    rows
}

/// Read CSV's outputs from the file's text: its first columns, its rows and
/// the row count, the first row left out when `header` is on.
pub fn csv_outputs(source: &str, header: bool) -> Vec<Value> {
    let mut rows = csv_rows(source);
    if header && !rows.is_empty() {
        rows.remove(0);
    }
    let mut outputs: Vec<Value> = (0..CSV_COLUMNS)
        .map(|k| Value::Array(rows.iter().map(|row| row.get(k).cloned().unwrap_or(Value::Null)).collect()))
        .collect();
    outputs.push(number(rows.len() as f64));
    outputs.insert(CSV_COLUMNS, Value::Array(rows.into_iter().map(Value::Array).collect()));
    outputs
}

/// Rows as CSV text, cells joined by `separator` and quoted when they hold
/// it, a quote or a line break.
pub fn csv_text(rows: &[Value], separator: &str) -> String {
    let cell = |value: &Value| {
        let text = plain(value);
        if text.contains(separator) || text.contains(['"', '\n', '\r']) {
            format!("\"{}\"", text.replace('"', "\"\""))
        } else {
            text
        }
    };
    rows.iter()
        .map(|row| match row {
            Value::Array(cells) => cells.iter().map(cell).collect::<Vec<_>>().join(separator),
            other => cell(other),
        })
        .map(|line| line + "\n")
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(f: fn(&[Value], &dyn graph::Host) -> Vec<Value>, args: Value) -> Vec<Value> {
        f(args.as_array().unwrap(), &())
    }

    #[test]
    fn expressions_compare_and_decide() {
        let args = |e: &str| json!([e, 2, 3, 0, 0, 0, 0]);
        assert_eq!(run(expression, args("a * 2 + b")), [json!(7.0)]);
        assert_eq!(run(expression, args("a > b ? a : b")), [json!(3.0)]);
        assert_eq!(run(expression, args("a < b && b <= 3")), [json!(true)]);
        assert_eq!(run(expression, args("a +")), [Value::Null]);
    }

    #[test]
    fn filters_split_by_condition() {
        let kept = run(filter, json!([[1, -2, 3, 4], "x > 0 && i < 3"]));
        assert_eq!(kept, [json!([1, 3]), json!([-2, 4]), json!([0.0, 2.0])]);
        let points = json!([[{"x": 1, "y": 5}, {"x": 2, "y": -1}], "py > 0"]);
        assert_eq!(run(filter, points)[0], json!([{"x": 1, "y": 5}]));
        assert_eq!(run(filter, json!([[1], "x >"])), vec![Value::Null; 3]);
    }

    #[test]
    fn lists_reshape() {
        assert_eq!(run(choose, json!([1, [1, 2], [3, 4], null, null, null])), [json!([3, 4])]);
        assert_eq!(run(zip, json!([[1, 2, 3], ["a", "b"], null, null])), [json!([[1, "a"], [2, "b"]])]);
        assert_eq!(run(repeat_list, json!([[1, 2], 2, 0])), [json!([1, 2, 1, 2])]);
        assert_eq!(run(repeat_list, json!([[1, 2], 0, 3])), [json!([1, 2, 1])]);
        assert_eq!(run(cull_pattern, json!([[1, 2, 3, 4, 5], [true, false]])), [json!([1, 3, 5])]);
        assert_eq!(run(weave, json!([[1, 3, 5], [2, 4]])), [json!([1, 2, 3, 4, 5])]);
        assert_eq!(run(cross_reference, json!([[1, 2], ["a", "b"]])), [json!([1, 1, 2, 2]), json!(["a", "b", "a", "b"])]);
        assert_eq!(run(graft, json!([[1, 2]])), [json!([[1], [2]])]);
        assert_eq!(run(flatten_once, json!([[[1, [2]], [3]]])), [json!([1, [2], 3])]);
        assert_eq!(run(list_depth, json!([[[1], [[2]]]])), [json!(3.0)]);
        assert_eq!(run(interpolate_values, json!([[0, 10], [100, 200], 2.5])), [json!(125.0)]);
        assert_eq!(run(interpolate_values, json!([[0, 10], [100, 200], 20])), [json!(200.0)]);
    }

    #[test]
    fn numbers_and_text_format() {
        let station = run(format_number_node, json!([1030, 2, 4, 3, "+", "."]));
        assert_eq!(station, [json!("1+030.00")]);
        assert_eq!(run(format_number_node, json!([-1234.5, 1, 1, 3, ",", "."])), [json!("-1,234.5")]);
        let label = run(format_text_node, json!(["P-{a:000} {b} {c:.2} {{x}}", 7, "left", 1.005, null]));
        assert_eq!(label, [json!("P-007 left 1.00 {x}")]);
        assert_eq!(format_text("{a}", &[json!(4.0)]), "4");
    }

    #[test]
    fn csv_reads_and_writes() {
        let source = "name;length\n\"A; one\";4\n\"say \"\"hi\"\"\";2.5\n";
        let rows = csv_rows(source);
        assert_eq!(rows[1], [json!("A; one"), json!(4.0)]);
        assert_eq!(rows[2], [json!("say \"hi\""), json!(2.5)]);
        let outputs = csv_outputs(source, true);
        assert_eq!(outputs[1], json!([4.0, 2.5]));
        assert_eq!(outputs[CSV_COLUMNS + 1], json!(2.0));
        let written = csv_text(&[json!(["A, one", 4.0]), json!(["b", 2.5])], ",");
        assert_eq!(written, "\"A, one\",4\nb,2.5\n");
        assert_eq!(csv_rows(&written)[0], [json!("A, one"), json!(4.0)]);
    }
}
