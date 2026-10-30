//! The standard arrowheads offered by the dimension, multileader and
//! properties pickers, and the blocks that carry them in a drawing.
//!
//! An arrowhead is a block record: "Closed filled" is the null handle, every
//! other standard one is a block named like `_Open`. The renderer draws the
//! standard names itself; the block still needs its geometry so other
//! programs draw the same arrow. It is created the first time it is picked.

use codec::entities::{Circle, Line, LwPolyline, LwVertex, Solid};
use codec::types::{Color, LineWeight, Vector3};
use codec::{Document, EntityType, Handle};

/// Name shown, block name (empty: closed filled, the null handle).
pub(crate) const STANDARD: &[(&str, &str)] = &[
    ("Closed filled", ""),
    ("Closed blank", "_ClosedBlank"),
    ("Closed", "_Closed"),
    ("Dot", "_Dot"),
    ("Architectural tick", "_ArchTick"),
    ("Oblique", "_Oblique"),
    ("Open", "_Open"),
    ("Origin indicator", "_Origin"),
    ("Origin indicator 2", "_Origin2"),
    ("Right angle", "_Open90"),
    ("Open 30", "_Open30"),
    ("Dot small", "_DotSmall"),
    ("Dot blank", "_DotBlank"),
    ("Dot small blank", "_Small"),
    ("Box blank", "_BoxBlank"),
    ("Box filled", "_BoxFilled"),
    ("Datum triangle", "_DatumBlank"),
    ("Datum triangle filled", "_DatumFilled"),
    ("None", "_None"),
];

fn standard_by_block(block: &str) -> Option<&'static str> {
    STANDARD
        .iter()
        .find(|(_, name)| name.eq_ignore_ascii_case(block))
        .map(|(label, _)| *label)
}

fn standard_by_label(label: &str) -> Option<&'static str> {
    STANDARD
        .iter()
        .find(|(shown, _)| *shown == label)
        .map(|(_, name)| *name)
}

/// A standard arrowhead's block typed by name, in any case and with or
/// without its leading underscore (`_DOT`, `dot`); `.` and `ClosedFilled`
/// are the closed filled default (an empty name).
pub(crate) fn typed_standard_block(value: &str) -> Option<&'static str> {
    let value = value.trim();
    if value.is_empty() || value == "." || value.eq_ignore_ascii_case("ClosedFilled") {
        return Some("");
    }
    let bare = value.trim_start_matches('_');
    STANDARD
        .iter()
        .map(|(_, name)| *name)
        .find(|name| !name.is_empty() && name.trim_start_matches('_').eq_ignore_ascii_case(bare))
}

/// The block name a picker value stands for: a standard name's block, or
/// the value itself (a user arrow block).
pub(crate) fn block_name(value: &str) -> &str {
    standard_by_label(value).unwrap_or(value)
}

/// What a picker shows for the arrowhead block `name`.
pub(crate) fn label_for_block(name: &str) -> String {
    if name.is_empty() {
        return STANDARD[0].0.to_string();
    }
    standard_by_block(name).map(str::to_string).unwrap_or_else(|| name.to_string())
}

/// What a picker shows for the arrowhead `handle`.
pub(crate) fn label(doc: &Document, handle: Handle) -> String {
    if handle.is_null() {
        return STANDARD[0].0.to_string();
    }
    match doc.block_records.iter().find(|b| b.handle == handle) {
        Some(record) => standard_by_block(&record.name)
            .map(str::to_string)
            .unwrap_or_else(|| record.name.clone()),
        None => STANDARD[0].0.to_string(),
    }
}

/// The picker's choices: the standard arrowheads, then each user arrow
/// block in `current` (the values shown now), not every block of the
/// drawing.
pub(crate) fn options<'a>(current: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = STANDARD.iter().map(|(label, _)| label.to_string()).collect();
    for value in current {
        if !value.is_empty() && !out.iter().any(|known| known == value) {
            out.push(value.to_string());
        }
    }
    out
}

/// The handle a picker value selects, creating a standard arrowhead's block
/// when the drawing does not have it yet. `None` for an unknown name.
pub(crate) fn resolve(doc: &mut Document, value: &str) -> Option<Handle> {
    let value = value.trim();
    if value.is_empty() || value == "Default" {
        return Some(Handle::NULL);
    }
    let name = block_name(value);
    if name.is_empty() {
        return Some(Handle::NULL);
    }
    if let Some(record) = doc.block_records.get(name) {
        return Some(record.handle);
    }
    let entities = standard_geometry(name)?;
    crate::app::define_block_raw_doc(
        doc,
        name,
        Vector3::ZERO,
        &entities,
    );
    doc.block_records.get(name).map(|record| record.handle)
}

/// A standard arrowhead block's contents at unit size, tip at the origin
/// pointing along +X, as the reference program writes them.
fn standard_geometry(name: &str) -> Option<Vec<EntityType>> {
    let line = |x1: f64, y1: f64, x2: f64, y2: f64| {
        EntityType::Line(Line::from_points(
            Vector3::new(x1, y1, 0.0),
            Vector3::new(x2, y2, 0.0),
        ))
    };
    let circle = |r: f64| EntityType::Circle(Circle::from_center_radius(Vector3::ZERO, r));
    // A filled dot: a closed two-arc polyline of constant width.
    let dot = |half: f64| {
        let mut p = LwPolyline::new();
        for x in [-half, half] {
            let mut v = LwVertex::from_coords(x, 0.0);
            v.bulge = 1.0;
            p.vertices.push(v);
        }
        p.is_closed = true;
        p.constant_width = 0.5;
        EntityType::LwPolyline(p)
    };
    let solid = |c: [[f64; 2]; 4]| {
        let v = |i: usize| Vector3::new(c[i][0], c[i][1], 0.0);
        EntityType::Solid(Solid::new(v(0), v(1), v(2), v(3)))
    };
    let (w, s) = (1.0 / 6.0, 0.267949);
    let d = 0.57735;
    let mut entities = match name {
        "_ClosedBlank" => vec![
            line(-1.0, w, 0.0, 0.0),
            line(0.0, 0.0, -1.0, -w),
            line(-1.0, w, -1.0, -w),
        ],
        "_Closed" => vec![
            line(-1.0, w, 0.0, 0.0),
            line(0.0, 0.0, -1.0, -w),
            line(-1.0, w, -1.0, -w),
            line(0.0, 0.0, -1.0, 0.0),
        ],
        "_Dot" => vec![dot(0.25), line(-0.5, 0.0, -1.0, 0.0)],
        "_ArchTick" => {
            let mut p = LwPolyline::new();
            p.vertices.push(LwVertex::from_coords(-0.5, -0.5));
            p.vertices.push(LwVertex::from_coords(0.5, 0.5));
            p.constant_width = 0.15;
            vec![EntityType::LwPolyline(p)]
        }
        "_Oblique" => vec![line(-0.5, -0.5, 0.5, 0.5)],
        "_Open" => vec![
            line(-1.0, w, 0.0, 0.0),
            line(-1.0, -w, 0.0, 0.0),
            line(0.0, 0.0, -1.0, 0.0),
        ],
        "_Origin" => vec![circle(0.5), line(0.0, 0.0, -1.0, 0.0)],
        "_Origin2" => vec![circle(0.5), circle(0.25), line(-0.5, 0.0, -1.0, 0.0)],
        "_Open90" => vec![
            line(-0.5, 0.5, 0.0, 0.0),
            line(0.0, 0.0, -0.5, -0.5),
            line(0.0, 0.0, -1.0, 0.0),
        ],
        "_Open30" => vec![
            line(-1.0, s, 0.0, 0.0),
            line(0.0, 0.0, -1.0, -s),
            line(0.0, 0.0, -1.0, 0.0),
        ],
        "_DotSmall" => vec![dot(0.0625)],
        "_DotBlank" => vec![circle(0.5), line(-0.5, 0.0, -1.0, 0.0)],
        "_Small" => vec![circle(0.25)],
        "_BoxBlank" => vec![
            line(-0.5, -0.5, 0.5, -0.5),
            line(0.5, -0.5, 0.5, 0.5),
            line(0.5, 0.5, -0.5, 0.5),
            line(-0.5, 0.5, -0.5, -0.5),
            line(-0.5, 0.0, -1.0, 0.0),
        ],
        "_BoxFilled" => vec![
            solid([[-0.5, 0.5], [0.5, 0.5], [-0.5, -0.5], [0.5, -0.5]]),
            line(-0.5, 0.0, -1.0, 0.0),
        ],
        "_DatumBlank" => vec![
            line(0.0, d, -1.0, 0.0),
            line(-1.0, 0.0, 0.0, -d),
            line(0.0, -d, 0.0, d),
        ],
        "_DatumFilled" => vec![solid([[0.0, d], [-1.0, 0.0], [0.0, -d], [0.0, -d]])],
        "_None" => Vec::new(),
        _ => return None,
    };
    // Arrowheads take the dimension's colour and lineweight.
    for entity in &mut entities {
        let common = entity.common_mut();
        common.color = Color::ByBlock;
        common.line_weight = LineWeight::ByBlock;
    }
    Some(entities)
}
