//! Compacting a page's content stream after it is built.
//!
//! The export draws item by item and states every item's colours, pen and
//! dash in full, and each glyph of embedded-font text as a text object of its
//! own; its numbers carry whatever an `f32` holds. A plot sheet came out at
//! around three times the size of the same sheet from other CAD programs,
//! most of it text objects and digits nobody can see. Three passes, each
//! leaving the page drawn the same:
//!
//! - a colour, line width, cap, join or dash that sets what is already set
//!   goes (the graphics state is tracked through `q` / `Q`);
//! - text objects that follow one another with only colour or general
//!   graphics state between them become one, a glyph at the same size and
//!   angle placed by `Td` relative to the one before instead of its own
//!   `Tm`, and a font set again to the same one goes;
//! - coordinates are rounded to 0.01 pt on the sheet (7200 per inch), sizes
//!   and angles to five significant digits; the step follows the current
//!   transformation, so a scaled plot keeps its precision on paper.

use lopdf::content::Operation;
use lopdf::Object;

/// The sheet-space step coordinates are rounded to, in points.
const STEP_PT: f64 = 0.01;

fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(*value as f64),
        _ => None,
    }
}

/// `value` rounded to a multiple of `step`, as the shortest number.
fn rounded(value: f64, step: f64) -> Object {
    let v = (value / step).round() * step;
    if v == v.trunc() && v.abs() < 1e15 {
        Object::Integer(v as i64)
    } else {
        Object::Real(v as f32)
    }
}

/// `value` with five significant digits.
fn significant(value: f64) -> Object {
    if value == 0.0 || !value.is_finite() {
        return Object::Integer(0);
    }
    let magnitude = value.abs().log10().floor();
    rounded(value, 10f64.powf(magnitude - 4.0))
}

/// The power of ten nearest below `STEP_PT / scale`: a user-space step that
/// is at most 0.01 pt on the sheet.
fn user_step(ctm: &[f64; 6]) -> f64 {
    let scale = (ctm[0] * ctm[3] - ctm[1] * ctm[2]).abs().sqrt();
    if !(scale.is_finite() && scale > 0.0) {
        return 1e-6;
    }
    10f64.powf((STEP_PT / scale).log10().floor())
}

fn multiply(m: &[f64; 6], n: &[f64; 6]) -> [f64; 6] {
    // m × n, PDF row-vector convention: the result maps through m, then n.
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

/// The state the dropped operators set.
#[derive(Clone, Default, PartialEq)]
struct State {
    fill: Option<(String, Vec<Object>)>,
    stroke: Option<(String, Vec<Object>)>,
    width: Option<Vec<Object>>,
    dash: Option<Vec<Object>>,
    cap: Option<Vec<Object>>,
    join: Option<Vec<Object>>,
}

fn same(a: &[Object], b: &[Object]) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

/// Operators allowed between text objects that a merged text object can
/// hold: colour and general graphics state.
fn allowed_in_text(operator: &str) -> bool {
    matches!(
        operator,
        "rg" | "RG"
            | "g"
            | "G"
            | "k"
            | "K"
            | "cs"
            | "CS"
            | "sc"
            | "SC"
            | "scn"
            | "SCN"
            | "w"
            | "J"
            | "j"
            | "M"
            | "d"
            | "ri"
            | "i"
            | "gs"
    )
}

/// Compact one page's operations.
pub(crate) fn compact(operations: Vec<Operation>) -> Vec<Operation> {
    let operations = drop_redundant_state(round_numbers(operations));
    merge_text_objects(operations)
}

/// Round coordinates to the sheet step and sizes to five digits.
fn round_numbers(mut operations: Vec<Operation>) -> Vec<Operation> {
    let mut ctm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut stack: Vec<[f64; 6]> = Vec::new();
    let mut step = user_step(&ctm);
    for op in &mut operations {
        match op.operator.as_str() {
            "q" => stack.push(ctm),
            "Q" => {
                ctm = stack.pop().unwrap_or(ctm);
                step = user_step(&ctm);
            }
            "cm" => {
                let values: Vec<f64> = op.operands.iter().filter_map(number).collect();
                if values.len() == 6 {
                    let m = [
                        values[0], values[1], values[2], values[3], values[4], values[5],
                    ];
                    ctm = multiply(&m, &ctm);
                    step = user_step(&ctm);
                }
            }
            "m" | "l" | "c" | "v" | "y" | "re" => {
                for operand in &mut op.operands {
                    if let Some(value) = number(operand) {
                        *operand = rounded(value, step);
                    }
                }
            }
            "Tm" if op.operands.len() == 6 => {
                for (index, operand) in op.operands.iter_mut().enumerate() {
                    if let Some(value) = number(operand) {
                        *operand = if index < 4 {
                            significant(value)
                        } else {
                            rounded(value, step)
                        };
                    }
                }
            }
            "w" => {
                for operand in &mut op.operands {
                    if let Some(value) = number(operand) {
                        *operand = significant(value);
                    }
                }
            }
            _ => {}
        }
    }
    operations
}

/// Leave out colour, pen, cap, join and dash operators that set the value
/// already in force.
fn drop_redundant_state(operations: Vec<Operation>) -> Vec<Operation> {
    let mut state = State::default();
    let mut stack: Vec<State> = Vec::new();
    let mut out = Vec::with_capacity(operations.len());
    for op in operations {
        let keep = match op.operator.as_str() {
            "q" => {
                stack.push(state.clone());
                true
            }
            "Q" => {
                state = stack.pop().unwrap_or_default();
                true
            }
            // A colour space resets the colour; patterns and shadings are
            // left to themselves.
            "cs" => {
                state.fill = None;
                true
            }
            "CS" => {
                state.stroke = None;
                true
            }
            "rg" | "g" | "k" => {
                let value = (op.operator.clone(), op.operands.clone());
                let repeat = state
                    .fill
                    .as_ref()
                    .is_some_and(|(o, v)| *o == value.0 && same(v, &value.1));
                state.fill = Some(value);
                !repeat
            }
            "sc" | "scn" => {
                state.fill = None;
                true
            }
            "RG" | "G" | "K" => {
                let value = (op.operator.clone(), op.operands.clone());
                let repeat = state
                    .stroke
                    .as_ref()
                    .is_some_and(|(o, v)| *o == value.0 && same(v, &value.1));
                state.stroke = Some(value);
                !repeat
            }
            "SC" | "SCN" => {
                state.stroke = None;
                true
            }
            "w" | "d" | "J" | "j" => {
                let slot = match op.operator.as_str() {
                    "w" => &mut state.width,
                    "d" => &mut state.dash,
                    "J" => &mut state.cap,
                    _ => &mut state.join,
                };
                let repeat = slot.as_ref().is_some_and(|v| same(v, &op.operands));
                *slot = Some(op.operands.clone());
                !repeat
            }
            // An extended graphics state can set any of them.
            "gs" => {
                state = State {
                    fill: state.fill,
                    stroke: state.stroke,
                    ..State::default()
                };
                true
            }
            _ => true,
        };
        if keep {
            out.push(op);
        }
    }
    out
}

/// A text object just closed that the next one may continue: the index of
/// its ET in the output, its font and its line matrix.
type OpenText = (usize, Option<Vec<Object>>, Option<[f64; 6]>);

/// Join text objects that follow one another, placing each next glyph run
/// relative to the one before when it has the same size and angle.
fn merge_text_objects(operations: Vec<Operation>) -> Vec<Operation> {
    let mut out: Vec<Operation> = Vec::with_capacity(operations.len());
    // The text object just closed, while the next operators may still join it.
    let mut open: Option<OpenText> = None;
    let iter = operations.into_iter();
    let mut in_text = false;
    let mut font: Option<Vec<Object>> = None;
    let mut line: Option<[f64; 6]> = None;
    let mut pending: Vec<Operation> = Vec::new();
    for op in iter {
        match op.operator.as_str() {
            "BT" if !in_text => {
                if let Some((et, last_font, last_line)) = open.take() {
                    // Reopen the previous text object: drop its ET, move the
                    // state operators between into it.
                    out.remove(et);
                    out.append(&mut pending);
                    font = last_font;
                    line = last_line;
                } else {
                    out.append(&mut pending);
                    out.push(op);
                    font = None;
                    line = None;
                }
                in_text = true;
            }
            "ET" if in_text => {
                in_text = false;
                out.push(op);
                open = Some((out.len() - 1, font.clone(), line));
            }
            "Tf" if in_text => {
                if font.as_ref().is_some_and(|f| same(f, &op.operands)) {
                    continue;
                }
                font = Some(op.operands.clone());
                out.push(op);
            }
            "Tm" if in_text => {
                let values: Vec<f64> = op.operands.iter().filter_map(number).collect();
                if values.len() != 6 {
                    line = None;
                    out.push(op);
                    continue;
                }
                let m = [
                    values[0], values[1], values[2], values[3], values[4], values[5],
                ];
                if let Some(l) = line.filter(|l| l[..4] == m[..4]) {
                    // Td moves the line matrix by (tx, ty) in text space.
                    let det = l[0] * l[3] - l[1] * l[2];
                    if det.abs() > 1e-12 {
                        let (ex, ey) = (m[4] - l[4], m[5] - l[5]);
                        let tx = (l[3] * ex - l[2] * ey) / det;
                        let ty = (-l[1] * ex + l[0] * ey) / det;
                        // The text-space step that is at most the sheet step.
                        let unit = det.abs().sqrt();
                        let step = 10f64.powf((STEP_PT / unit).log10().floor()).min(1e-3);
                        let (tx_o, ty_o) = (rounded(tx, step), rounded(ty, step));
                        // Where the rounded move lands, so the next glyph is
                        // placed from there.
                        let (txr, tyr) = (number(&tx_o).unwrap(), number(&ty_o).unwrap());
                        line = Some([
                            l[0],
                            l[1],
                            l[2],
                            l[3],
                            l[4] + txr * l[0] + tyr * l[2],
                            l[5] + txr * l[1] + tyr * l[3],
                        ]);
                        out.push(Operation::new("Td", vec![tx_o, ty_o]));
                        continue;
                    }
                }
                line = Some(m);
                out.push(op);
            }
            operator if !in_text && open.is_some() && allowed_in_text(operator) => {
                pending.push(op);
            }
            _ => {
                if !in_text {
                    open = None;
                    out.append(&mut pending);
                } else if matches!(op.operator.as_str(), "Td" | "TD" | "T*" | "'" | "\"") {
                    // The line matrix moves in ways not followed here.
                    line = None;
                }
                out.push(op);
            }
        }
    }
    out.append(&mut pending);
    out
}

/// Compact every page's content stream of `doc`.
pub(crate) fn compact_document(doc: &mut lopdf::Document) {
    let pages: Vec<lopdf::ObjectId> = doc.get_pages().into_values().collect();
    for page in pages {
        let Ok(content) = doc.get_and_decode_page_content(page) else {
            continue;
        };
        let compacted = lopdf::content::Content {
            operations: compact(content.operations),
        };
        if let Ok(bytes) = compacted.encode() {
            let _ = doc.change_page_content(page, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(text: &str) -> Vec<Operation> {
        lopdf::content::Content::decode(text.as_bytes())
            .unwrap()
            .operations
    }

    fn text(operations: Vec<Operation>) -> String {
        String::from_utf8(lopdf::content::Content { operations }.encode().unwrap()).unwrap()
    }

    #[test]
    fn repeated_state_goes_and_restored_state_counts_again() {
        let out = text(compact(ops(
            "0 0 0 RG 0 0 0 rg 1 w 0 0 0 rg 1 w [] 0 d [] 0 d \
             q 1 0 0 rg 0 0 m 1 1 l S Q 0 0 0 rg 1 0 0 rg 0 0 0 rg",
        )));
        assert_eq!(
            text_lines(&out),
            vec![
                "0 0 0 RG", "0 0 0 rg", "1 w", "[] 0 d", "q", "1 0 0 rg", "0 0 m", "1 1 l", "S",
                "Q", "1 0 0 rg", "0 0 0 rg"
            ]
        );
    }

    fn text_lines(s: &str) -> Vec<&str> {
        s.lines().collect()
    }

    #[test]
    fn coordinates_round_to_a_hundredth_of_a_point_on_the_sheet() {
        let out = text(compact(ops(
            "1683.77917 0.004 m 12.3456789 2 l S q 0.01 0 0 0.01 0 0 cm 123.456789 0 m Q 9.1079834 0 0 9.10799 87.396294 833.004332 cm",
        )));
        let lines = text_lines(&out);
        assert_eq!(lines[0], "1683.78 0 m");
        assert_eq!(lines[1], "12.35 2 l");
        // Under a CTM of 1:100 a user unit is 0.01 pt: one unit is the step.
        assert_eq!(lines[5], "123 0 m");
    }

    /// Glyphs drawn as text objects of their own become one text object; the
    /// next glyph at the same size is moved there by Td, a glyph at another
    /// size keeps its Tm, and a colour change between them moves inside.
    #[test]
    fn glyph_text_objects_merge_into_one() {
        let source = "0 0 0 rg BT /F1 1 Tf 9.9999999 0 0 10.0000001 10 20 Tm [<000B>] TJ ET \
                      0 0 0 rg BT /F1 1 Tf 10.0000002 0 0 9.9999998 16 20 Tm [<000C>] TJ ET \
                      1 0 0 rg BT /F1 1 Tf 10 0 0 10 22 20 Tm [<000D>] TJ ET \
                      BT /F1 1 Tf 4 0 0 4 30 20 Tm [<000E>] TJ ET \
                      0 0 m 1 1 l S \
                      BT /F1 1 Tf 4 0 0 4 30 30 Tm [<000F>] TJ ET";
        let out = text(compact(ops(source)));
        assert_eq!(
            text_lines(&out),
            vec![
                "0 0 0 rg",
                "BT",
                "/F1 1 Tf",
                "10 0 0 10 10 20 Tm",
                "[<000B>] TJ",
                "0.6 0 Td",
                "[<000C>] TJ",
                "1 0 0 rg",
                "0.6 0 Td",
                "[<000D>] TJ",
                "4 0 0 4 30 20 Tm",
                "[<000E>] TJ",
                "ET",
                "0 0 m",
                "1 1 l",
                "S",
                "BT",
                "/F1 1 Tf",
                "4 0 0 4 30 30 Tm",
                "[<000F>] TJ",
                "ET",
            ]
        );
    }

    /// Glyph positions after merging are those of the source, to the step.
    #[test]
    fn merged_glyphs_stay_where_they_were() {
        let mut source = String::from("BT /F1 1 Tf ");
        let mut expected = Vec::new();
        let mut x = 87.39629f64;
        for (k, advance) in [5.46, 3.0377, 7.9, 0.0001, 12.345678].iter().enumerate() {
            source.push_str(&format!(
                "ET BT /F1 1 Tf 2.07 0.5 -0.5 2.07 {x} {} Tm [<00{k:02X}>] TJ ",
                833.0 + k as f64 * 0.3
            ));
            expected.push((x, 833.0 + k as f64 * 0.3));
            x += advance;
        }
        source.push_str("ET");
        let out = compact(ops(&source));
        let mut line = [0.0f64; 6];
        let mut placed = Vec::new();
        for op in &out {
            let v: Vec<f64> = op.operands.iter().filter_map(number).collect();
            match op.operator.as_str() {
                "Tm" => line = [v[0], v[1], v[2], v[3], v[4], v[5]],
                "Td" => {
                    line[4] += v[0] * line[0] + v[1] * line[2];
                    line[5] += v[0] * line[1] + v[1] * line[3];
                }
                "TJ" => placed.push((line[4], line[5])),
                _ => {}
            }
        }
        assert_eq!(placed.len(), expected.len());
        for (got, want) in placed.iter().zip(&expected) {
            assert!(
                (got.0 - want.0).abs() < 0.006 && (got.1 - want.1).abs() < 0.006,
                "{got:?} vs {want:?}"
            );
        }
        assert_eq!(out.iter().filter(|op| op.operator == "BT").count(), 1);
    }

    /// Marked content between text objects keeps them apart.
    #[test]
    fn text_objects_in_different_marked_content_stay_apart() {
        let source = "/OC /L1 BDC BT /F1 1 Tf 1 0 0 1 0 0 Tm [<01>] TJ ET EMC \
                      /OC /L2 BDC BT /F1 1 Tf 1 0 0 1 5 0 Tm [<02>] TJ ET EMC";
        let out = compact(ops(source));
        assert_eq!(out.iter().filter(|op| op.operator == "BT").count(), 2);
    }
}
