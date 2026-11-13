// DGNIMPORT / -DGNIMPORT — a DGN model (V7 or V8) as drawing objects.
//
//   Enter filename for DGN import:
//   Enter name of model or [?] <Default>:
//   Specify conversion units [Master/Sub] <Master>:
//   Specify mapping setup or [?] <Standard>:
//
// The Standard mapping setup: each level becomes the layer of its name (its
// number when unnamed), colours become the colour index of the same RGB
// (else a true colour), weights the lineweights below, V7 line styles
// Continuous. Lines, line strings and shapes, arcs, circles and ellipses,
// B-spline curves, complex chains and shapes (polylines with arcs), text,
// text nodes and shared cells (blocks) are converted.

use std::collections::{HashMap, HashSet};

use document::entities::{
    Arc, AttachmentPoint, Circle, DrawingDirection, Ellipse, Insert, Line, LineSpacingStyle,
    LwPolyline, LwVertex, MText, Point, Polyline3D, Spline, Text, TextHorizontalAlignment,
    TextVerticalAlignment,
};
use document::tables::DimStyle;
use document::types::{Color, LineWeight, Vector2, Vector3};
use document::EntityType;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::underlay_vector::{
    By, DgnDimLook, DgnDimStyle, DgnElement, DgnGeom, DgnImport, P3,
};

/// What one import reads and where it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct DgnImportRequest {
    pub path: String,
    pub model: String,
    /// Sub units (else master units) become drawing units.
    pub sub_units: bool,
    /// Text nodes become one TEXT per line (else one MTEXT).
    pub explode_text_nodes: bool,
    /// Into the current drawing (else a new one).
    pub current_drawing: bool,
    /// In the current drawing: every imported layer, block and text style
    /// name gets "<file> - <model>$0$" as prefix (else names the drawing
    /// already has keep the existing definitions).
    pub prefix_duplicates: bool,
    /// What becomes of the model's reference attachments.
    pub references: ReferenceMode,
    /// What an earlier translation of a reference already on disk gets.
    pub existing_reference: ExistingReference,
    /// The text code page of a V7 file (e.g. 1250 for Czech/Slovak files).
    /// V7 names no page; `None` keeps the historical 1252 decoding.
    pub code_page: Option<u16>,
}

/// The External DGN references choice: translated into drawings attached
/// as external references, left out, or attached as DGN underlays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReferenceMode {
    Translate,
    #[default]
    Ignore,
    Underlay,
}

/// A translated reference drawing that already exists: asked about,
/// overwritten, or kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExistingReference {
    #[default]
    Prompt,
    Overwrite,
    Keep,
}

/// The objects of an import: layers (name, colour), text styles (name, font
/// file), blocks (name, objects) and the model's objects, in file order.
#[derive(Default)]
pub struct Converted {
    pub layers: Vec<(String, Color)>,
    /// Layers that come in turned off.
    pub layers_off: Vec<String>,
    pub styles: Vec<(String, String)>,
    /// Dimension styles, by name, in the file's table order, with the
    /// values the file gives them (none: the drawing's style of the name).
    pub dim_styles: Vec<(String, Option<DimStyle>)>,
    pub blocks: Vec<(String, Vec<EntityType>)>,
    pub entities: Vec<EntityType>,
}

/// A DGN weight (0-31) as a lineweight (1/100 mm), as the Standard mapping
/// converts it.
pub fn lineweight_of(weight: u32) -> i16 {
    const TABLE: [i16; 15] = [
        0, 13, 30, 40, 53, 70, 80, 100, 106, 120, 140, 158, 158, 158, 200,
    ];
    TABLE.get(weight as usize).copied().unwrap_or(211)
}

/// An RGB colour as the colour index of the same RGB, else a true colour.
pub fn color_of(rgb: [u8; 3]) -> Color {
    let index = document::types::aci_table::nearest_aci(rgb[0], rgb[1], rgb[2]);
    if document::types::aci_table::aci_to_rgb(index) == Some((rgb[0], rgb[1], rgb[2])) {
        Color::Index(index)
    } else {
        Color::from_rgb(rgb[0], rgb[1], rgb[2])
    }
}

/// A unit's short name as words ("m" → Meters).
pub fn unit_display(short: &str) -> String {
    match short.trim().to_ascii_lowercase().as_str() {
        "m" => "Meters",
        "mm" => "Millimeters",
        "cm" => "Centimeters",
        "km" => "Kilometers",
        "in" | "\"" => "Inches",
        "ft" | "'" => "Feet",
        "yd" => "Yards",
        "mi" => "Miles",
        other => return other.to_string(),
    }
    .to_string()
}

/// A name read from a DGN file as a symbol (layer, block, text style) or
/// file name: the characters those cannot hold become `_`.
pub fn symbol_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_control() || "<>/\\\":;?*|=`".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

fn layer_name(import: &DgnImport, level: u32) -> String {
    import
        .levels
        .get(&level)
        .map(|l| symbol_name(&l.0))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| level.to_string())
}

/// A cell's name as a block name: a leading `*` (an anonymous block
/// written out) is dropped.
pub fn block_name(cell: &str) -> String {
    symbol_name(cell.trim_start_matches('*'))
}

fn v3(p: P3) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}

/// True when the axes `u` and `v` are of one length and at right angles.
fn circular(u: P3, v: P3) -> bool {
    let (lu, lv) = (norm(u), norm(v));
    (lu - lv).abs() <= 1e-9 * lu.max(1.0) && dot(u, v).abs() <= 1e-9 * lu * lv
}

/// The arc's points at `n` steps.
fn arc_points(center: P3, u: P3, v: P3, start: f64, sweep: f64) -> Vec<P3> {
    let n = ((sweep.abs() / (std::f64::consts::PI / 32.0)).ceil() as usize).clamp(2, 4096);
    (0..=n)
        .map(|k| {
            let t = start + sweep * k as f64 / n as f64;
            std::array::from_fn(|c| center[c] + t.cos() * u[c] + t.sin() * v[c])
        })
        .collect()
}

/// The plane of an arc: its normal (+z for an arc in the drawing plane,
/// whichever way it turns) and whether its parameter turns clockwise about
/// that normal.
fn arc_plane(u: P3, v: P3) -> (P3, bool) {
    let n = cross(u, v);
    let len = norm(n);
    if len < 1e-12 {
        return ([0.0, 0.0, 1.0], false);
    }
    let n = n.map(|c| c / len);
    if n[2] < -1.0 + 1e-9 {
        ([0.0, 0.0, 1.0], true)
    } else {
        (n, false)
    }
}

/// The angle of `d` in the entity coordinate system of `normal`.
fn ocs_angle(normal: P3, d: P3) -> f64 {
    let m = document::types::Matrix3::arbitrary_axis(v3(normal));
    let (x, y) = (
        m * Vector3::new(1.0, 0.0, 0.0),
        m * Vector3::new(0.0, 1.0, 0.0),
    );
    (d[0] * y.x + d[1] * y.y + d[2] * y.z).atan2(d[0] * x.x + d[1] * x.y + d[2] * x.z)
}

/// A point in the entity coordinate system of `normal`.
fn to_ocs(normal: P3, p: P3) -> Vector3 {
    document::types::Matrix3::arbitrary_axis(v3(normal)).transpose() * v3(p)
}

/// The unit normal of the plane the points lie in (Newell's, turned to
/// +z), `None` when they leave it by more than a millionth of their span.
/// Points on one line lie in the XY plane's parallel.
fn plane_normal(points: &[P3]) -> Option<P3> {
    let mut n = [0.0; 3];
    for (k, p) in points.iter().enumerate() {
        let q = points[(k + 1) % points.len()];
        n[0] += (p[1] - q[1]) * (p[2] + q[2]);
        n[1] += (p[2] - q[2]) * (p[0] + q[0]);
        n[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    let len = norm(n);
    let span = points
        .iter()
        .map(|p| norm(sub(*p, points[0])))
        .fold(0.0, f64::max);
    if !(len > 1e-12 * span * span) {
        return flat(points).then_some([0.0, 0.0, 1.0]);
    }
    let mut n = [n[0] / len, n[1] / len, n[2] / len];
    if n[2] < 0.0 || (n[2] == 0.0 && (n[1] < 0.0 || (n[1] == 0.0 && n[0] < 0.0))) {
        n = [-n[0], -n[1], -n[2]];
    }
    let off = |p: &P3| dot(sub(*p, points[0]), n).abs();
    points
        .iter()
        .all(|p| off(p) <= 1e-6 * span.max(1e-12))
        .then_some(n)
}

/// True when every point is at the first one's z.
fn flat(points: &[P3]) -> bool {
    let z = points.first().map_or(0.0, |p| p[2]);
    points
        .iter()
        .all(|p| (p[2] - z).abs() <= 1e-9 * z.abs().max(1.0))
}

/// The points as a polyline: at one z a 2D polyline at that elevation
/// (with the bulges), else a 3D polyline (no bulges).
fn polyline(points: &[P3], bulges: &[f64]) -> EntityType {
    if !flat(points) {
        let points: Vec<Vector3> = points.iter().map(|p| v3(*p)).collect();
        return EntityType::Polyline3D(Polyline3D::from_points(points));
    }
    let mut pl = LwPolyline::new();
    pl.plinegen = true;
    pl.elevation = points.first().map_or(0.0, |p| p[2]);
    pl.vertices = points
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let mut v = LwVertex::new(Vector2::new(p[0], p[1]));
            v.bulge = bulges.get(k).copied().unwrap_or(0.0);
            v
        })
        .collect();
    EntityType::LwPolyline(pl)
}

/// A flat outline that winds clockwise seen from +z (by its vertices, the
/// closing edge included) in its entity coordinates about -z: x and z
/// negated, the bulges negated. `None` when it winds the other way.
fn turned_about_minus_z(points: &[P3], bulges: &[f64]) -> Option<(Vec<P3>, Vec<f64>)> {
    let n = points.len();
    let area: f64 = (0..n)
        .map(|k| {
            let (a, b) = (points[k], points[(k + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    (n >= 2 && flat(points) && area < 0.0).then(|| {
        (
            points.iter().map(|p| [-p[0], p[1], -p[2]]).collect(),
            bulges.iter().map(|b| -b).collect(),
        )
    })
}

/// [`polyline`] and its filling [`solid_hatch`], about -z when `by_winding`
/// and the outline winds clockwise.
fn outline(points: &[P3], bulges: &[f64], filled: bool, by_winding: bool) -> Vec<EntityType> {
    let turned = by_winding
        .then(|| turned_about_minus_z(points, bulges))
        .flatten();
    let (points, bulges) = match &turned {
        Some((p, b)) => (p.as_slice(), b.as_slice()),
        None => (points, bulges),
    };
    let mut out = vec![polyline(points, bulges)];
    if filled {
        out.extend(solid_hatch(points, bulges));
    }
    if turned.is_some() {
        for entity in &mut out {
            match entity {
                EntityType::LwPolyline(pl) => pl.normal = Vector3::new(0.0, 0.0, -1.0),
                EntityType::Hatch(h) => h.normal = Vector3::new(0.0, 0.0, -1.0),
                _ => {}
            }
        }
    }
    out
}

/// A solid hatch filling a closed outline (with its bulges) at the
/// outline's elevation; `None` when the outline is not at one z. It is
/// marked associative with no source yet: the outline converted just before
/// it becomes its source once both are in the drawing.
fn solid_hatch(points: &[P3], bulges: &[f64]) -> Option<EntityType> {
    use document::entities::{BoundaryEdge, BoundaryPath, Hatch, PolylineEdge};
    if points.len() < 3 || !flat(points) {
        return None;
    }
    let mut n = points.len();
    if points[0][0] == points[n - 1][0] && points[0][1] == points[n - 1][1] {
        n -= 1;
    }
    let mut edge = PolylineEdge::new(Vec::new(), true);
    for (k, p) in points[..n].iter().enumerate() {
        let bulge = bulges.get(k).copied().unwrap_or(0.0);
        edge.add_vertex(Vector2::new(p[0], p[1]), bulge);
    }
    let mut hatch = Hatch::solid();
    hatch.is_associative = true;
    let mut boundary = BoundaryPath::external();
    boundary.flags.set_polyline(true);
    boundary.edges.push(BoundaryEdge::Polyline(edge));
    hatch.add_path(boundary);
    hatch.elevation = points[0][2];
    Some(EntityType::Hatch(Box::new(hatch)))
}

/// A complex chain's vertices and bulges: lines and line strings add their
/// points, circular arcs in the drawing plane a bulge, other arcs their
/// points. A chain whose vertices are not at one z comes out as points
/// only (a 3D polyline has no bulges).
fn chain(parts: &[DgnGeom]) -> (Vec<P3>, Vec<f64>) {
    let (points, bulges) = chain_points(parts, true);
    if flat(&points) {
        (points, bulges)
    } else {
        chain_points(parts, false)
    }
}

fn chain_points(parts: &[DgnGeom], use_bulges: bool) -> (Vec<P3>, Vec<f64>) {
    let mut points: Vec<P3> = Vec::new();
    let mut bulges: Vec<f64> = Vec::new();
    let mut add = |p: P3, bulge_before: Option<f64>| {
        let same = points
            .last()
            .is_some_and(|q| (0..3).all(|c| (q[c] - p[c]).abs() < 1e-9));
        if let (Some(b), Some(last)) = (bulge_before, bulges.last_mut()) {
            *last = b;
        }
        if !same {
            points.push(p);
            bulges.push(0.0);
        }
    };
    for part in parts {
        match part {
            DgnGeom::Line(a, b) => {
                add(*a, None);
                add(*b, None);
            }
            DgnGeom::Points(pts) => pts.iter().for_each(|p| add(*p, None)),
            DgnGeom::Curve(pts) => curve_fit_points(pts).iter().for_each(|p| add(*p, None)),
            DgnGeom::Arc {
                center,
                u,
                v,
                start,
                sweep,
            } if use_bulges && circular(*u, *v) && arc_plane(*u, *v).0[2] > 1.0 - 1e-9 => {
                let at = |t: f64| -> P3 {
                    std::array::from_fn(|c| center[c] + t.cos() * u[c] + t.sin() * v[c])
                };
                let side = if arc_plane(*u, *v).1 { -1.0 } else { 1.0 };
                add(at(*start), None);
                add(at(start + sweep), Some((side * sweep / 4.0).tan()));
            }
            DgnGeom::Arc {
                center,
                u,
                v,
                start,
                sweep,
            } => arc_points(*center, *u, *v, *start, *sweep)
                .into_iter()
                .for_each(|p| add(p, None)),
            _ => {}
        }
    }
    (points, bulges)
}

/// Horizontal (0 left, 1 centre, 2 right) and vertical (0 top, 1 middle,
/// 2 baseline, 3 descender) parts of a DGN justification code.
fn just_parts(just: u8) -> (u8, u8) {
    match just {
        0..=2 => (0, just),
        3..=5 => (0, just - 3),
        6..=8 => (1, just - 6),
        9..=11 => (2, just - 9),
        12..=14 => (2, just - 12),
        16 => (0, 3),
        20 => (1, 3),
        24 => (2, 3),
        _ => (0, 2),
    }
}

/// The justification point of a text: its lower left moved along the
/// baseline by the part of its length, up by the part of its height (a
/// descender justification 0.3 heights below the baseline).
fn just_point(origin: P3, rotation: f64, length: f64, height: f64, just: u8) -> P3 {
    let (h, v) = just_parts(just);
    let along = length * h as f64 / 2.0;
    let up = match v {
        // Centre top is placed half a height up.
        0 if just == 6 => height / 2.0,
        0 => height,
        1 => height / 2.0,
        2 => 0.0,
        _ => -0.3 * height,
    };
    let (c, s) = (rotation.cos(), rotation.sin());
    [
        origin[0] + along * c - up * s,
        origin[1] + along * s + up * c,
        origin[2],
    ]
}

fn attachment(just: u8) -> AttachmentPoint {
    use AttachmentPoint as A;
    match just_parts(just) {
        (0, 0) => A::TopLeft,
        (1, 0) => A::TopCenter,
        (2, 0) => A::TopRight,
        (0, 1) => A::MiddleLeft,
        (1, 1) => A::MiddleCenter,
        (2, 1) => A::MiddleRight,
        (1, _) => A::BottomCenter,
        (2, _) => A::BottomRight,
        _ => A::BottomLeft,
    }
}

/// Text as MTEXT contents: backslashes and braces stay characters instead
/// of becoming formatting codes.
fn mtext_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
}

/// A number with six significant digits, trailing zeros dropped.
fn sig6(x: f64) -> String {
    let digits = if x == 0.0 {
        0
    } else {
        (5 - x.abs().log10().floor() as i32).max(0) as usize
    };
    let s = format!("{x:.digits$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// The colour code a V8 text carries in its contents: 256 for the level's
/// colour, 0 for the cell's, else the colour index nearest its table entry.
fn color_code(color: By<crate::scene::model::underlay_vector::DgnColor>) -> u16 {
    match color {
        By::Level => 256,
        By::Cell => 0,
        By::Own(c) => {
            document::types::aci_table::nearest_aci(c.table[0], c.table[1], c.table[2]) as u16
        }
    }
}

/// The text style of a line of text (V7: one per font number, "dgnNNN";
/// V8: the style the text names), added to `styles` with its font file
/// the first time.
fn text_style(geom: &DgnGeom, import: &DgnImport, styles: &mut Vec<(String, String)>) -> String {
    match geom {
        DgnGeom::Text {
            style: Some(id), ..
        } => match import.text_styles.iter().find(|(i, ..)| i == id) {
            Some((_, name, font)) if !name.eq_ignore_ascii_case("Standard") => {
                let name = symbol_name(name);
                if !styles.iter().any(|s| s.0 == name) {
                    styles.push((name.clone(), font.clone()));
                }
                name
            }
            _ => "Standard".to_string(),
        },
        DgnGeom::Text {
            font: Some(font), ..
        } => {
            let name = format!("dgn{font:03}");
            if !styles.iter().any(|s| s.0 == name) {
                styles.push((name.clone(), format!("{name}.shx")));
            }
            name
        }
        _ => "Standard".to_string(),
    }
}

/// A V7 text as TEXT, justified at its justification point.
fn text_entity(geom: &DgnGeom, style: &str) -> Option<EntityType> {
    let DgnGeom::Text {
        text,
        origin,
        height,
        width_factor,
        length,
        rotation,
        just,
        ..
    } = geom
    else {
        return None;
    };
    let mut t = Text::with_value(text.clone(), v3(*origin))
        .with_height(*height)
        .with_rotation(rotation.rem_euclid(std::f64::consts::TAU));
    t.width_factor = if *width_factor > 0.0 {
        *width_factor
    } else {
        1.0
    };
    t.style = style.to_string();
    let (h, v) = just_parts(*just);
    if (h, v) != (0, 2) {
        t.horizontal_alignment = match h {
            1 => TextHorizontalAlignment::Center,
            2 => TextHorizontalAlignment::Right,
            _ => TextHorizontalAlignment::Left,
        };
        t.vertical_alignment = match v {
            0 => TextVerticalAlignment::Top,
            1 => TextVerticalAlignment::Middle,
            3 => TextVerticalAlignment::Bottom,
            _ => TextVerticalAlignment::Baseline,
        };
        t.alignment_point = Some(v3(just_point(*origin, *rotation, *length, *height, *just)));
    }
    Some(EntityType::Text(t))
}

fn text_line(g: &DgnGeom) -> Option<(&str, P3, f64, f64, f64)> {
    match g {
        DgnGeom::Text {
            text,
            origin,
            height,
            rotation,
            width_factor,
            ..
        } => Some((text.as_str(), *origin, *height, *rotation, *width_factor)),
        _ => None,
    }
}

/// A V8 text as MTEXT: a bottom left text
/// attached at its bottom left a third of the height below the baseline
/// (exact line spacing 0.8), any other at its justification point; its
/// contents carry its colour (and width factor).
fn mtext_of_text(geom: &DgnGeom, color: u16, style: &str) -> Option<EntityType> {
    let DgnGeom::Text {
        text,
        origin,
        height,
        width_factor,
        length,
        rotation,
        just,
        ..
    } = geom
    else {
        return None;
    };
    let mut m = MText::new();
    let width = if (width_factor - 1.0).abs() > 1e-6 {
        format!("\\W{};", sig6(*width_factor))
    } else {
        String::new()
    };
    m.value = format!("{{{width}\\C{color};{}}}", mtext_escape(text));
    if *just == 2 {
        let up = -0.33 * height;
        m.insertion_point = Vector3::new(
            origin[0] - rotation.sin() * up,
            origin[1] + rotation.cos() * up,
            origin[2],
        );
        m.attachment_point = AttachmentPoint::BottomLeft;
        m.line_spacing_style = LineSpacingStyle::Exactly;
        m.line_spacing_factor = 0.8;
    } else {
        m.insertion_point = v3(just_point(*origin, *rotation, *length, *height, *just));
        m.attachment_point = attachment(*just);
        m.line_spacing_style = LineSpacingStyle::AtLeast;
        m.line_spacing_factor = 1.0;
    }
    m.height = *height;
    m.rectangle_height = Some(*height);
    m.rotation = rotation.rem_euclid(std::f64::consts::TAU);
    m.rectangle_width = 0.0;
    m.drawing_direction = DrawingDirection::ByStyle;
    m.style = style.to_string();
    Some(EntityType::MText(Box::new(m)))
}

/// A text node as one MTEXT, a paragraph a line, attached at the node's
/// origin by its justification. V8: its contents carry its colour, its
/// defined height spans its lines; V7: exact spacing of the node's line
/// spacing.
fn mtext_of_node(
    lines: &[DgnGeom],
    origin: Option<P3>,
    just: u8,
    line_spacing: Option<f64>,
    style: String,
    color: Option<u16>,
) -> Option<EntityType> {
    let (_, first, height, rotation, _) = lines.iter().find_map(text_line)?;
    // A component on the line of the one before it (its origin less than
    // half a text height away across the line) continues that line.
    let up_dir = [-rotation.sin(), rotation.cos(), 0.0];
    let mut joined = String::new();
    let mut previous: Option<P3> = None;
    for (text, origin, h, _, _) in lines.iter().filter_map(text_line) {
        if let Some(p) = previous {
            if dot(sub(origin, p), up_dir).abs() >= 0.5 * h.max(height) {
                joined.push_str("\\P");
            }
        }
        joined.push_str(&mtext_escape(text));
        previous = Some(origin);
    }
    let mut m = MText::new();
    m.value = match color {
        Some(c) => format!("{{\\C{c};{joined}}}"),
        None => joined,
    };
    let up = [-rotation.sin(), rotation.cos(), 0.0];
    let (just, insertion) = match origin {
        Some(o) => (just, o),
        None => (0, std::array::from_fn(|c| first[c] + up[c] * height)),
    };
    m.insertion_point = v3(insertion);
    m.attachment_point = attachment(just);
    match line_spacing {
        // V7: the gap between lines over the default pitch (5/3 heights).
        Some(gap) => {
            m.line_spacing_style = LineSpacingStyle::Exactly;
            m.line_spacing_factor = if height > 0.0 {
                (height + gap) / (height * 5.0 / 3.0)
            } else {
                1.0
            };
            m.rectangle_height = None;
        }
        None => {
            let last = lines
                .iter()
                .filter_map(text_line)
                .next_back()
                .map_or(first, |l| l.1);
            m.line_spacing_style = LineSpacingStyle::AtLeast;
            m.line_spacing_factor = 1.0;
            m.rectangle_height = Some(height + dot(sub(first, last), up).max(0.0));
        }
    }
    m.height = height;
    m.rotation = rotation.rem_euclid(std::f64::consts::TAU);
    m.rectangle_width = 0.0;
    m.drawing_direction = DrawingDirection::ByStyle;
    m.style = style;
    Some(EntityType::MText(Box::new(m)))
}

/// An arc, circle or ellipse in its own plane.
fn arc_entity(center: P3, u: P3, v: P3, start: f64, sweep: f64) -> Vec<EntityType> {
    let full = sweep.abs() >= std::f64::consts::TAU - 1e-9;
    let (normal, flipped) = arc_plane(u, v);
    let at = |t: f64| -> P3 { std::array::from_fn(|c| t.cos() * u[c] + t.sin() * v[c]) };
    if circular(u, v) {
        let radius = norm(u);
        let c = to_ocs(normal, center);
        if full {
            let mut circle = Circle::from_center_radius(c, radius);
            circle.normal = v3(normal);
            return vec![EntityType::Circle(circle)];
        }
        let (a0, a1) = (
            ocs_angle(normal, at(start)),
            ocs_angle(normal, at(start + sweep)),
        );
        // The arc runs counter-clockwise about its normal.
        let ccw = (sweep > 0.0) != flipped;
        let (from, to) = if ccw { (a0, a1) } else { (a1, a0) };
        let mut arc = Arc::from_center_radius_angles(
            c,
            radius,
            from.rem_euclid(std::f64::consts::TAU),
            to.rem_euclid(std::f64::consts::TAU),
        );
        arc.normal = v3(normal);
        return vec![EntityType::Arc(arc)];
    }
    let (lu, lv) = (norm(u), norm(v));
    if dot(u, v).abs() > 1e-9 * lu * lv || lu < 1e-12 || lv < 1e-12 {
        // ponytail: axes not at right angles cannot come out of one plane's
        // ellipse; kept as points.
        return vec![polyline(&arc_points(center, u, v, start, sweep), &[])];
    }
    // P(t) = c + cos t·u + sin t·v as an ellipse on its major axis with the
    // minor axis turned counter-clockwise from it about the normal. A longer
    // second axis makes the major axis the first one turned back a quarter
    // (-v), as the reference does.
    let ccw = !flipped;
    let (major, ratio, map): (P3, f64, Box<dyn Fn(f64) -> f64>) = if lu >= lv {
        if ccw {
            (u, lv / lu, Box::new(|t| t))
        } else {
            (u, lv / lu, Box::new(|t: f64| -t))
        }
    } else if ccw {
        (
            v.map(|c| -c),
            lu / lv,
            Box::new(|t: f64| t + std::f64::consts::FRAC_PI_2),
        )
    } else {
        (
            v.map(|c| -c),
            lu / lv,
            Box::new(|t: f64| -t - std::f64::consts::FRAC_PI_2),
        )
    };
    let mut e = Ellipse::from_center_axes(v3(center), v3(major), ratio);
    e.normal = v3(normal);
    if !full {
        let (p0, p1) = (map(start), map(start + sweep));
        let (from, to) = if p1 >= p0 { (p0, p1) } else { (p1, p0) };
        e.start_parameter = from.rem_euclid(std::f64::consts::TAU);
        e.end_parameter = e.start_parameter + (to - from);
    }
    vec![EntityType::Ellipse(e)]
}

/// The name of the dimension style with this id (Standard when the
/// table has none).
fn dim_style_name(import: &DgnImport, id: u32) -> String {
    import
        .dim_styles
        .iter()
        .find(|s| s.id == id)
        .map(|s| symbol_name(&s.name))
        .unwrap_or_else(|| "Standard".to_string())
}

/// A V8 font (number, kind) as its name and the font file it imports with.
fn dgn_font(import: &DgnImport, font: (u8, u8)) -> Option<(&str, &str)> {
    import
        .fonts
        .iter()
        .find(|f| (f.0, f.1) == font)
        .map(|f| (f.2.as_str(), f.3.as_str()))
}

/// The text style a V8 dimension style's text takes: "Style" and its
/// font's name, with that font's file.
fn dim_text_style(import: &DgnImport, ds: &DgnDimStyle) -> (String, String) {
    match dgn_font(import, ds.font) {
        Some((name, file)) => (format!("Style{name}"), file.to_string()),
        None => ("Standard".to_string(), "txt.shx".to_string()),
    }
}

/// A V8 dimension style as a dimension style: its values over the
/// defaults (lengths times `k`), its text in [`dim_text_style`], closed
/// arrows and text beside the first extension line, as the reference
/// import gives every such style. Decimal accuracy reads as engineering
/// units in a feet and inches file, a fractional one as architectural.
fn dim_style_values(import: &DgnImport, ds: &DgnDimStyle, k: f64, name: &str) -> DimStyle {
    let mut s = DimStyle::dxf_defaults(name);
    s.dimtxt = ds.text_height * k;
    s.dimasz = ds.arrow * k;
    s.dimexo = ds.ext_offset * k;
    s.dimexe = ds.ext_extension * k;
    s.dimgap = ds.gap * k;
    s.dimdli = ds.stack_offset * k;
    s.dimcen = ds.center * k;
    s.dimscale = ds.scale;
    let bits = ds.accuracy & 0x7f;
    let places = if bits == 0 {
        0
    } else {
        bits.trailing_zeros() as i16 + 1
    };
    s.dimdec = places;
    s.dimtdec = places;
    s.dimadec = i16::from(ds.angle_places);
    s.dimlunit = if ds.accuracy & 0x80 == 0 && bits != 0 {
        4
    } else if ds.two_units {
        3
    } else {
        2
    };
    s.dimzin = if ds.trailing_zeros { 3 } else { 11 };
    s.dimtad = if ds.inline_text { 0 } else { 1 };
    s.dimtih = ds.horizontal_text;
    s.dimtoh = ds.horizontal_text;
    s.dimtofl = ds.line_inside;
    s.dimsah = true;
    s.dimblk1_name = "_Closed".to_string();
    s.dimblk2_name = "_Closed".to_string();
    // ponytail: text beside the first extension line on every imported style,
    // as every exported sample read; the record bit for it is not known.
    s.dimjust = 1;
    s.dimaltd = 3;
    s.dimalttd = 3;
    s.dimaltf = 1.0 / 25.4 / if ds.two_units { 12.0 } else { 1.0 };
    s.dimtxsty = dim_text_style(import, ds).0;
    s
}

/// A V8 dimension as a dimension object, in the dimension's own style:
/// linear (1) as a rotated dimension along its x axis, the dimension line
/// `offset` from its last point along its y axis; angle size (5) as a
/// three-point angle (vertex, first, second point) with its arc through the
/// first point; radius (11) and diameter (12) from the centre and a point
/// on the circle, the text at the third point.
/// It takes `text` and the variable `overrides` the reference import gives.
/// An angle's arc point follows from its text `layout`.
fn dimension_entity(
    kind: u8,
    points: &[P3],
    offset: f64,
    axes: &[[f64; 2]; 2],
    style: &str,
    text: String,
    overrides: &[(i16, document::xdata::XDataValue)],
    layout: &DimTextLayout,
) -> Option<EntityType> {
    use document::entities::{Dimension, DimensionDiameter, DimensionLinear, DimensionRadius};
    let at = |k: usize| points.get(k).copied();
    let along = |p: P3, d: [f64; 2], t: f64| [p[0] + d[0] * t, p[1] + d[1] * t, p[2]];
    let mut entity = match kind {
        1 => {
            let (p0, p1) = (at(0)?, at(1)?);
            let [x, y] = *axes;
            // The direction the dimension line reads along: x or -x, whichever
            // turns less than a half turn from the world x axis.
            let x = if x[0] < -1e-12 || (x[0].abs() <= 1e-12 && x[1] < 0.0) {
                [-x[0], -x[1]]
            } else {
                x
            };
            let length = x[0].hypot(x[1]);
            if !(length > 1e-12) {
                return None;
            }
            let x = [x[0] / length, x[1] / length];
            let line = along(p1, y, offset);
            let mut d = DimensionLinear::rotated(v3(p0), v3(p1), x[1].atan2(x[0]));
            d.definition_point = v3(line);
            d.base.actual_measurement = d.measurement();
            EntityType::Dimension(Box::new(Dimension::Linear(d)))
        }
        5 => {
            let (vertex, first, second) = (at(0)?, at(1)?, at(2)?);
            let a0 = (first[1] - vertex[1]).atan2(first[0] - vertex[0]);
            let mut sweep = (second[1] - vertex[1]).atan2(second[0] - vertex[0]) - a0;
            if sweep < 0.0 {
                sweep += std::f64::consts::TAU;
            }
            let radius = norm(sub(first, vertex));
            // The arc's point a third of the way round from the first line;
            // two thirds when the text, laid beside the second extension line
            // (a terminator, three gaps when it breaks the arc, one when it
            // does not, and half its width along the arc), fits between the
            // lines and reaches past the middle.
            let label = format!("{:.*}\u{b0}", layout.angle_places, sweep.to_degrees());
            let width = crate::entities::text_support::text_local_bounds(
                &layout.font,
                &label,
                layout.height as f32,
                1.0,
                0.0,
            )
            .map(|b| f64::from(b.ink_max[0] - b.ink_min[0]))
            .unwrap_or(0.0);
            let length = radius * sweep;
            let gaps = if layout.in_line { 3.0 } else { 1.0 };
            let beside = layout.arrow + gaps * layout.gap + width / 2.0;
            let fits = length >= layout.arrow + gaps * layout.gap + width;
            let third = if fits && beside > length / 2.0 {
                2.0
            } else {
                1.0
            };
            let mid = a0 + sweep * third / 3.0;
            let arc = [
                vertex[0] + radius * mid.cos(),
                vertex[1] + radius * mid.sin(),
                vertex[2],
            ];
            let dvec = |p: P3| glam::DVec3::from_array(p);
            crate::modules::annotate::angular_dim::angular_three_point_entity(
                dvec(vertex),
                dvec(first),
                dvec(second),
                dvec(arc),
                None,
            )?
        }
        11 | 12 => {
            let (center, point) = (at(0)?, at(1)?);
            let text = at(2).unwrap_or(point);
            let mut d = if kind == 11 {
                let mut d = DimensionRadius::new(v3(center), v3(point));
                d.base.definition_point = v3(center);
                d.leader_length = norm(sub(text, point));
                Dimension::Radius(d)
            } else {
                let far = sub(center, sub(point, center));
                let mut d = DimensionDiameter::new(v3(point), v3(far));
                d.leader_length = norm(sub(text, point));
                Dimension::Diameter(d)
            };
            // A diameter's text stays where the file has it; a radius's text
            // is laid out by its style along its arc point's direction.
            if kind == 12 {
                let base = d.base_mut();
                base.text_middle_point = v3(text);
                base.insertion_point = v3(text);
                base.text_user_positioned = true;
            }
            EntityType::Dimension(Box::new(d))
        }
        _ => return None,
    };
    if let EntityType::Dimension(d) = &mut entity {
        d.base_mut().style_name = style.to_string();
        crate::entities::dimension::set_dimension_text_override(d.base_mut(), Some(text));
    }
    for (code, value) in overrides {
        crate::entities::dim_override::set_on_entity(&mut entity, *code, Some(value.clone()));
    }
    Some(entity)
}

/// What lays out an imported dimension's text: its height, terminator,
/// gap and font, and whether it sits in the dimension line.
struct DimTextLayout {
    height: f64,
    arrow: f64,
    gap: f64,
    font: String,
    in_line: bool,
    angle_places: usize,
}

/// The text, the variable overrides and the text layout an imported
/// dimension takes: its text carries its colour and, when its font is not
/// its style's text font, that font; it draws no centre mark, fits text
/// and arrows best, and takes its own text height, terminator, extension
/// line offset and extension (each over its style's scale squared, as the
/// reference reads them), horizontal text as its style and, linear or
/// angular, text beside the first extension line. Text in its line takes
/// the margin as its gap; text off the line sits outside it at its own gap.
fn dimension_settings(
    import: &DgnImport,
    kind: u8,
    look: &DgnDimLook,
    style: Option<&DgnDimStyle>,
    color: u16,
) -> (
    String,
    Vec<(i16, document::xdata::XDataValue)>,
    DimTextLayout,
) {
    use crate::entities::dim_override as ov;
    use document::xdata::XDataValue::{Integer16, Real};
    let own = dgn_font(import, look.font);
    let style_font = style.and_then(|s| dgn_font(import, s.font));
    let differs = |file: &str| style_font.is_some_and(|(_, f)| !f.eq_ignore_ascii_case(file));
    let font = match own {
        Some((name, file)) if differs(file) && look.font.1 == 4 => {
            format!(r"\F{name}|b0|i0|c0;")
        }
        Some((name, file)) if differs(file) => format!(r"\F{name}|c0;"),
        _ => String::new(),
    };
    let text = format!(r"{{{font}\C{color};<>}}");
    let scale = style.map(|s| s.scale).filter(|s| *s > 0.0).unwrap_or(1.0);
    let length = |v: f64| v / (scale * scale);
    let gap = length(if look.text_outside {
        look.gap
    } else {
        look.margin
    });
    let horizontal = i16::from(style.is_some_and(|s| s.horizontal_text));
    let mut overrides = vec![
        (ov::DIMCEN, Real(0.0)),
        (ov::DIMATFIT, Integer16(3)),
        (ov::DIMSCALE, Real(scale)),
        (ov::DIMTXT, Real(length(look.text_height))),
        (ov::DIMASZ, Real(length(look.arrow))),
        (ov::DIMEXO, Real(length(look.ext_offset))),
        (ov::DIMEXE, Real(length(look.ext_extension))),
        (ov::DIMGAP, Real(gap)),
        (ov::DIMTAD, Integer16(if look.text_outside { 2 } else { 0 })),
        (ov::DIMTIH, Integer16(horizontal)),
        (ov::DIMTOH, Integer16(horizontal)),
    ];
    if matches!(kind, 1 | 5) {
        overrides.push((ov::DIMJUST, Integer16(1)));
    }
    let layout = DimTextLayout {
        height: length(look.text_height),
        arrow: length(look.arrow),
        gap,
        font: own.or(style_font).map_or("txt", |f| f.0).to_string(),
        in_line: !look.text_outside,
        angle_places: style.map_or(0, |s| usize::from(s.angle_places)),
    };
    (text, overrides, layout)
}

/// A mesh's face loops (1-based point numbers, a negative one hiding the
/// edge from it) as polyface meshes: a loop past four corners is fanned into
/// triangles whose inner edges are hidden. A polyface numbers its vertices
/// in 16 bits, so faces that need further vertices go on further meshes.
fn polyfaces(points: &[P3], loops: &[Vec<i32>]) -> Vec<EntityType> {
    use document::entities::{PolyfaceFace, PolyfaceMesh, PolyfaceVertex};
    const MAX_VERTICES: usize = i16::MAX as usize;
    let mut out = Vec::new();
    let mut mesh = PolyfaceMesh::new();
    let mut ids: HashMap<usize, i16> = HashMap::new();
    for face in loops {
        // Each corner: its point and whether the edge from it shows.
        let corners: Vec<(usize, bool)> = face
            .iter()
            .filter_map(|&k| {
                let p = (k.unsigned_abs() as usize).checked_sub(1)?;
                (p < points.len()).then_some((p, k > 0))
            })
            .collect();
        let n = corners.len();
        if n < 3 || n < face.len() {
            continue;
        }
        let parts: Vec<Vec<(usize, bool)>> = if n <= 4 {
            vec![corners]
        } else {
            (1..n - 1)
                .map(|k| {
                    let (first, last) = (corners[0], corners[k + 1]);
                    vec![
                        (first.0, first.1 && k == 1),
                        corners[k],
                        (last.0, last.1 && k == n - 2),
                    ]
                })
                .collect()
        };
        for part in parts {
            let new = part.iter().filter(|c| !ids.contains_key(&c.0)).count();
            if mesh.vertices.len() + new > MAX_VERTICES {
                out.push(std::mem::replace(&mut mesh, PolyfaceMesh::new()));
                ids.clear();
            }
            let mut index = Vec::with_capacity(4);
            for (p, shown) in part {
                let id = match ids.get(&p) {
                    Some(id) => *id,
                    None => {
                        let id = mesh.add_vertex(PolyfaceVertex::new(v3(points[p])));
                        ids.insert(p, id);
                        id
                    }
                };
                index.push(if shown { id } else { -id });
            }
            mesh.add_face(match index[..] {
                [a, b, c] => PolyfaceFace::triangle(a, b, c),
                [a, b, c, d] => PolyfaceFace::quad(a, b, c, d),
                _ => continue,
            });
        }
    }
    out.push(mesh);
    out.into_iter()
        .filter(|m| !m.faces.is_empty())
        .map(EntityType::PolyfaceMesh)
        .collect()
}

/// A shared cell instance as an INSERT: a mirrored placement looks along
/// -z (x negated in its entity coordinates, z scale negated).
fn insert_entity(block: String, m: [f64; 4], origin: P3, z_scale: f64) -> EntityType {
    let det = m[0] * m[3] - m[1] * m[2];
    let mirrored = det < 0.0;
    let m = if mirrored {
        [-m[0], -m[1], m[2], m[3]]
    } else {
        m
    };
    let sx = m[0].hypot(m[2]);
    let sy = if sx > 0.0 {
        (m[0] * m[3] - m[1] * m[2]) / sx
    } else {
        1.0
    };
    let (point, z) = if mirrored {
        (Vector3::new(-origin[0], origin[1], -origin[2]), -z_scale)
    } else {
        (v3(origin), z_scale)
    };
    let mut insert = Insert::new(block, point)
        .with_scale(sx, sy, z)
        .with_rotation(m[2].atan2(m[0]));
    if mirrored {
        insert.normal = Vector3::new(0.0, 0.0, -1.0);
    }
    EntityType::Insert(insert)
}

/// How a run of elements converts: a V7 cell copy turns every weight (and
/// a cell of one colour its colour) over to the inserting block.
#[derive(Clone, Copy, Default)]
struct CellCopy {
    active: bool,
    color_by_block: bool,
}

/// Conversion state: the output, the V7 cell copies made so far and the
/// definitions they copy.
struct Converter<'a> {
    import: &'a DgnImport,
    explode_text_nodes: bool,
    out: Converted,
    defs: HashMap<String, &'a [DgnElement]>,
    /// (cell, made inside a copy) → copy block name.
    copies: HashMap<(String, bool), String>,
    /// Block names given so far, upper-cased.
    names: std::collections::HashSet<String>,
    /// For each name used as a base, the first number not yet tried.
    next_number: HashMap<String, usize>,
    /// Converting a shared cell definition: the cells inside it are not
    /// blocks of their own, their objects are part of the definition.
    in_definition: bool,
    /// Self-contained cell headers whose geometry matches an already
    /// defined one, per (base name, copy kind) → the cell's
    /// origin-normalized parts and the block they defined. A GIS export
    /// can place the same symbol cell thousands of times as separate
    /// headers; those become one block and many inserts.
    shares: HashMap<(String, bool, bool), Vec<(Vec<DgnElement>, String)>>,
    /// The construction layer of each level, once one is made.
    construction: HashMap<u32, String>,
}

impl<'a> Converter<'a> {
    /// `base`, or `base` and the first number that makes it a new name.
    fn unique(&mut self, base: &str) -> String {
        let upper = base.to_ascii_uppercase();
        let name = if self.names.contains(&upper) {
            // Numbers below the last one given are all taken: carry on from
            // there (a file can place one cell tens of thousands of times).
            let mut k = self.next_number.get(&upper).copied().unwrap_or(1);
            while self.names.contains(&format!("{upper}{k}")) {
                k += 1;
            }
            self.next_number.insert(upper, k + 1);
            format!("{base}{k}")
        } else {
            base.to_string()
        };
        self.names.insert(name.to_ascii_uppercase());
        name
    }

    /// Every colour index a cell draws with, through the cells it places.
    fn cell_colors(&self, name: &str, depth: usize, out: &mut Vec<u32>) {
        let Some(def) = self.defs.get(&name.to_ascii_uppercase()) else {
            return;
        };
        for e in def.iter() {
            if let By::Own(c) = e.color {
                out.push(c.index);
            }
            if let (DgnGeom::Cell { name, .. }, true) = (&e.geom, depth < 8) {
                self.cell_colors(name, depth + 1, out);
            }
        }
    }

    /// The block a V7 instance of `cell` inserts: a copy of the definition
    /// whose weights (and, when the whole cell is one colour, colour) come
    /// from the INSERT, one per definition placed at the top or inside
    /// definitions and another for those inside copies.
    fn v7_copy(&mut self, cell: &str, in_copy: bool, depth: usize) -> String {
        let key = (cell.to_ascii_uppercase(), in_copy);
        if let Some(name) = self.copies.get(&key) {
            return name.clone();
        }
        let name = self.unique(&block_name(cell));
        self.copies.insert(key, name.clone());
        let Some(def) = self.defs.get(&cell.to_ascii_uppercase()).copied() else {
            return name;
        };
        let mut colors = Vec::new();
        self.cell_colors(cell, 0, &mut colors);
        let copy = CellCopy {
            active: true,
            color_by_block: colors.windows(2).all(|w| w[0] == w[1]),
        };
        let entities = self.elements(def, true, copy, depth + 1);
        self.out.blocks.push((name.clone(), entities));
        name
    }

    fn layer(&mut self, element: &DgnElement, in_cell: bool) -> String {
        // A cell's objects are on layer 0 of its block.
        let layer = if in_cell {
            "0".to_string()
        } else if element.construction && self.import.construction_layers {
            // A construction element of a file whose view hides them is
            // on "<level>(Construction)", turned off; a name another level
            // took gets a number.
            if let Some(name) = self.construction.get(&element.level) {
                return name.clone();
            }
            let base = format!("{}(Construction)", layer_name(self.import, element.level));
            let mut name = base.clone();
            let mut k = 1;
            while self.construction.values().any(|n| *n == name) {
                name = format!("{base}{k}");
                k += 1;
            }
            self.construction.insert(element.level, name.clone());
            self.out.layers_off.push(name.clone());
            name
        } else {
            layer_name(self.import, element.level)
        };
        if !self.out.layers.iter().any(|l| l.0 == layer) {
            let color = self
                .import
                .levels
                .get(&element.level)
                .and_then(|l| l.1)
                .filter(|_| !in_cell)
                .map(color_of)
                .unwrap_or(Color::Index(7));
            self.out.layers.push((layer.clone(), color));
        }
        layer
    }

    /// Whether a flat outline's normal follows its winding (-z when it
    /// winds clockwise): in V8 files and V7 3D files; a V7 2D file's are
    /// all +z.
    fn by_winding(&self) -> bool {
        !self.import.v7 || self.import.three_d
    }

    /// The object(s) one element converts to.
    fn geom(
        &mut self,
        element: &DgnElement,
        geom: &DgnGeom,
        copy: CellCopy,
        depth: usize,
    ) -> Vec<EntityType> {
        match geom {
            DgnGeom::Line(a, b) if dot(sub(*a, *b), sub(*a, *b)) < 1e-24 => {
                vec![EntityType::Point(Point::from_coords(a[0], a[1], a[2]))]
            }
            DgnGeom::Line(a, b) => vec![EntityType::Line(Line::from_points(v3(*a), v3(*b)))],
            // A filled shape is its outline and a solid hatch inside it.
            DgnGeom::Points(pts) if pts.len() >= 2 => {
                outline(pts, &[], element.filled, self.by_winding())
            }
            DgnGeom::Points(_) => Vec::new(),
            DgnGeom::Curve(pts) => {
                let fit = curve_fit_points(pts);
                if fit.len() < 2 {
                    return Vec::new();
                }
                let mut s = Spline::new();
                s.degree = 3;
                s.fit_points = fit.iter().map(|p| v3(*p)).collect();
                // A curve through points in one plane is planar about that
                // plane's normal; a 3D curve off any plane is not.
                match plane_normal(&fit) {
                    Some(n) => {
                        s.flags.planar = true;
                        s.normal = v3(n);
                    }
                    None => s.flags.planar = false,
                }
                vec![EntityType::Spline(s)]
            }
            DgnGeom::Dimension {
                kind,
                points,
                offset,
                axes,
                style,
                look,
            } => {
                let record = self.import.dim_styles.iter().find(|s| s.id == *style);
                let (text, overrides, layout) =
                    dimension_settings(self.import, *kind, look, record, color_code(element.color));
                let style = dim_style_name(self.import, *style);
                dimension_entity(
                    *kind, points, *offset, axes, &style, text, &overrides, &layout,
                )
                .into_iter()
                .collect()
            }
            DgnGeom::Mesh { points, loops } => polyfaces(points, loops),
            DgnGeom::Leader { points, style } => {
                let mut leader = document::entities::Leader::new();
                leader.vertices = points.iter().map(|p| v3(*p)).collect();
                leader.dimension_style = dim_style_name(self.import, *style);
                vec![EntityType::Leader(leader)]
            }
            DgnGeom::Arc {
                center,
                u,
                v,
                start,
                sweep,
            } if self.import.v7
                && *sweep < 0.0
                && *sweep > -std::f64::consts::TAU
                && circular(*u, *v)
                && u[2].abs() < 1e-12
                && v[2].abs() < 1e-12
                && cross(*u, *v)[2] > 0.0 =>
            {
                // A V7 circular arc drawn clockwise comes in as a two-point
                // polyline from its start to its end with a negative bulge.
                let at = |t: f64| {
                    [
                        center[0] + u[0] * t.cos() + v[0] * t.sin(),
                        center[1] + u[1] * t.cos() + v[1] * t.sin(),
                        center[2],
                    ]
                };
                vec![polyline(
                    &[at(*start), at(*start + *sweep)],
                    &[(*sweep / 4.0).tan()],
                )]
            }
            DgnGeom::Arc {
                center,
                u,
                v,
                start,
                sweep,
            } => arc_entity(*center, *u, *v, *start, *sweep),
            DgnGeom::Spline {
                degree,
                knots,
                poles,
                closed,
            } => {
                let mut s = Spline::new();
                s.degree = *degree as i32;
                s.knots = knots.clone();
                s.control_points = poles.iter().map(|p| v3(*p)).collect();
                s.flags.closed = *closed;
                // A V8 closed curve is periodic (its knots do not start at
                // 0); a V7 one is clamped.
                s.flags.periodic = *closed && knots.first().is_some_and(|k| *k > 0.0);
                s.flags.planar = true;
                vec![EntityType::Spline(s)]
            }
            DgnGeom::Chain(parts) => {
                let (points, bulges) = chain(parts);
                if points.len() < 2 {
                    return Vec::new();
                }
                outline(&points, &bulges, element.filled, self.by_winding())
            }
            // V7 text is TEXT, V8 text an MTEXT.
            DgnGeom::Text { font: Some(_), .. } => {
                let style = text_style(geom, self.import, &mut self.out.styles);
                text_entity(geom, &style).into_iter().collect()
            }
            DgnGeom::Text { .. } => {
                let style = text_style(geom, self.import, &mut self.out.styles);
                mtext_of_text(geom, color_code(element.color), &style)
                    .into_iter()
                    .collect()
            }
            // Exploded, each line converts as a text element of its own.
            DgnGeom::TextNode { lines, .. } if self.explode_text_nodes => lines
                .iter()
                .flat_map(|g| self.geom(element, g, copy, depth))
                .collect(),
            DgnGeom::TextNode {
                lines,
                origin,
                just,
                line_spacing,
            } => {
                let Some(first) = lines.first() else {
                    return Vec::new();
                };
                let style = text_style(first, self.import, &mut self.out.styles);
                let color = (!self.import.v7).then(|| color_code(element.color));
                mtext_of_node(lines, *origin, *just, *line_spacing, style, color)
                    .into_iter()
                    .collect()
            }
            // A cell that is not shared becomes a block of its own ("the
            // cell's name, else AnonymousCell" and a number) placed at the
            // cell's origin: its objects move by the origin only (not turned
            // or scaled back) and keep their layers.
            DgnGeom::CellGroup { parts, .. } if self.in_definition => {
                self.elements(parts, true, copy, depth + 1)
            }
            DgnGeom::CellGroup {
                name,
                origin,
                parts,
            } => {
                let base = if name.is_empty() {
                    "AnonymousCell"
                } else {
                    name.as_str()
                };
                // The definition's objects are relative to the cell's own
                // origin, which makes repeats of one symbol comparable and
                // placeable through the insert alone.
                let normalized = crate::scene::model::underlay_vector::scaled_elements(
                    parts,
                    1.0,
                    origin.map(|c| -c),
                );
                let key = (base.to_ascii_uppercase(), copy.active, copy.color_by_block);
                if let Some((_, shared)) = self
                    .shares
                    .get(&key)
                    .and_then(|shares| shares.iter().find(|(seen, _)| *seen == normalized).cloned())
                {
                    return vec![insert_entity(shared, [1.0, 0.0, 0.0, 1.0], *origin, 1.0)];
                }
                let block = self.unique(&block_name(base));
                let entities = self.elements(&normalized, false, copy, depth + 1);
                self.out.blocks.push((block.clone(), entities));
                self.shares
                    .entry(key)
                    .or_default()
                    .push((normalized, block.clone()));
                vec![insert_entity(block, [1.0, 0.0, 0.0, 1.0], *origin, 1.0)]
            }
            DgnGeom::Cell {
                name,
                m,
                origin,
                z_scale,
            } => {
                let block = if self.import.v7 && depth < 8 {
                    self.v7_copy(name, copy.active, depth)
                } else {
                    block_name(name)
                };
                vec![insert_entity(block, *m, *origin, *z_scale)]
            }
        }
    }

    fn elements(
        &mut self,
        elements: &[DgnElement],
        in_cell: bool,
        copy: CellCopy,
        depth: usize,
    ) -> Vec<EntityType> {
        let mut entities = Vec::new();
        for element in elements {
            let layer = self.layer(element, in_cell);
            for mut entity in self.geom(element, &element.geom, copy, depth) {
                let common = entity.common_mut();
                common.layer = layer.clone();
                common.color = match element.color {
                    By::Level => Color::ByLayer,
                    By::Cell => Color::ByBlock,
                    By::Own(c) => color_of(c.rgb),
                };
                common.line_weight = match element.weight {
                    By::Level => LineWeight::ByLayer,
                    By::Cell => LineWeight::ByBlock,
                    By::Own(w) => LineWeight::Value(lineweight_of(w)),
                };
                // The Standard mapping draws every line style Continuous.
                match element.style {
                    By::Own(_) => common.linetype = "Continuous".to_string(),
                    By::Cell => common.linetype = "ByBlock".to_string(),
                    By::Level => {}
                }
                if copy.active {
                    common.line_weight = LineWeight::ByBlock;
                    if copy.color_by_block {
                        common.color = Color::ByBlock;
                    }
                }
                entities.push(entity);
            }
        }
        entities
    }
}

/// The points a V7 curve's spline is fitted through. The curve runs from
/// its third point to its third last, each coordinate an Akima spline over
/// the points' chord length (a repeated point counts 0.0001 long and adds a
/// level stretch); the first two and last two points only set the end
/// slopes. It is sampled at 3n + 3 equal steps of chord length (n points),
/// and its own points between the ends are kept among the samples, a
/// sample on one of them (within 2.54e-8) giving way to it.
fn curve_fit_points(pts: &[P3]) -> Vec<P3> {
    let n = pts.len();
    if n < 6 {
        return Vec::new();
    }
    let mut u = vec![0.0];
    for k in 1..n {
        u.push(
            u[k - 1]
                + dot(sub(pts[k], pts[k - 1]), sub(pts[k], pts[k - 1]))
                    .sqrt()
                    .max(1e-4),
        );
    }
    // Each coordinate's slope at the points 2..n-3 (Akima).
    let slopes: Vec<Vec<f64>> = (0..3)
        .map(|c| {
            let m: Vec<f64> = (0..n - 1)
                .map(|k| (pts[k + 1][c] - pts[k][c]) / (u[k + 1] - u[k]))
                .collect();
            (0..n)
                .map(|i| {
                    if i < 2 || i + 2 >= n {
                        return 0.0;
                    }
                    let (w1, w2) = ((m[i + 1] - m[i]).abs(), (m[i - 1] - m[i - 2]).abs());
                    if w1 + w2 == 0.0 {
                        (m[i - 1] + m[i]) / 2.0
                    } else {
                        (w1 * m[i - 1] + w2 * m[i]) / (w1 + w2)
                    }
                })
                .collect()
        })
        .collect();
    let at = |at: f64| -> P3 {
        let mut i = 2;
        while i < n - 4 && at > u[i + 1] {
            i += 1;
        }
        let h = u[i + 1] - u[i];
        let t = (at - u[i]) / h;
        let (t2, t3) = (t * t, t * t * t);
        let (h00, h10) = (2.0 * t3 - 3.0 * t2 + 1.0, t3 - 2.0 * t2 + t);
        let (h01, h11) = (-2.0 * t3 + 3.0 * t2, t3 - t2);
        std::array::from_fn(|c| {
            h00 * pts[i][c]
                + h10 * slopes[c][i] * h
                + h01 * pts[i + 1][c]
                + h11 * slopes[c][i + 1] * h
        })
    };
    let (start, end) = (u[2], u[n - 3]);
    let steps = 3 * n + 3;
    let mut marks: Vec<(f64, Option<P3>)> = (0..=steps)
        .map(|k| {
            let point = match k {
                0 => Some(pts[2]),
                k if k == steps => Some(pts[n - 3]),
                _ => None,
            };
            (start + (end - start) * k as f64 / steps as f64, point)
        })
        .collect();
    marks.extend((3..n - 3).map(|i| (u[i], Some(pts[i]))));
    marks.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<P3> = Vec::new();
    for (position, point) in marks {
        let q = point.unwrap_or_else(|| at(position));
        if let Some(last) = out.last_mut() {
            if dot(sub(q, *last), sub(q, *last)).sqrt() <= 2.54e-8 {
                if point.is_some() {
                    *last = q;
                }
                continue;
            }
        }
        out.push(q);
    }
    out
}

/// Sub units per drawing unit: a file with 12 sub units to its master
/// unit (feet and inches, V7 or V8) comes in in sub units whichever is
/// chosen.
pub fn unit_scale(import: &DgnImport, sub_units: bool) -> f64 {
    if sub_units || (import.sub_per_master - 12.0).abs() < 1e-9 {
        import.sub_per_master
    } else {
        1.0
    }
}

/// The import's objects, in sub units when `sub_units`. V7 cell
/// definitions become blocks as drawn; each is placed through a copy (the
/// definition's name and a number), made before the blocks that place it. V8 cells are blocks placed as they
/// are. A V8 file's levels all become layers, in its level table's order.
pub fn convert(import: &DgnImport, sub_units: bool, explode_text_nodes: bool) -> Converted {
    let k = unit_scale(import, sub_units);
    let scale = |elements: &[DgnElement]| {
        crate::scene::model::underlay_vector::scaled_elements(elements, k, [0.0; 3])
    };
    let cells: Vec<(String, Vec<DgnElement>)> = import
        .cells
        .iter()
        .map(|(name, elements)| (name.clone(), scale(elements)))
        .collect();
    let top = scale(&import.elements);
    let mut c = Converter {
        import,
        explode_text_nodes,
        out: Converted::default(),
        defs: cells
            .iter()
            .map(|(name, elements)| (name.to_ascii_uppercase(), elements.as_slice()))
            .collect(),
        copies: HashMap::new(),
        names: cells
            .iter()
            .map(|(name, _)| block_name(name).to_ascii_uppercase())
            .collect(),
        next_number: HashMap::new(),
        in_definition: false,
        shares: HashMap::new(),
        construction: HashMap::new(),
    };
    if !import.v7 {
        for level in &import.level_order {
            let element = DgnElement {
                level: *level,
                color: By::Level,
                weight: By::Level,
                style: By::Level,
                geom: DgnGeom::Points(Vec::new()),
                filled: false,
                construction: false,
            };
            c.layer(&element, false);
        }
    }
    // Every dimension style of the file's table comes in, used or not, with
    // its values and its text style.
    for ds in &import.dim_styles {
        let name = symbol_name(&ds.name);
        if !c.out.dim_styles.iter().any(|s| s.0 == name) {
            let (style, font) = dim_text_style(import, ds);
            if style != "Standard" && !c.out.styles.iter().any(|s| s.0 == style) {
                c.out.styles.push((style, font));
            }
            let values = dim_style_values(import, ds, k, &name);
            c.out.dim_styles.push((name, Some(values)));
        }
    }
    // Every text style of the file's table comes in, used or not.
    for (_, name, font) in &import.text_styles {
        let name = symbol_name(name);
        if !name.eq_ignore_ascii_case("Standard") && !c.out.styles.iter().any(|s| s.0 == name) {
            c.out.styles.push((name, font.clone()));
        }
    }
    c.in_definition = true;
    for (name, elements) in &cells {
        let entities = c.elements(elements, true, CellCopy::default(), 0);
        if !name.is_empty() && !entities.is_empty() {
            c.out.blocks.push((block_name(name), entities));
        }
    }
    c.in_definition = false;
    let entities = c.elements(&top, false, CellCopy::default(), 0);
    c.out.entities = entities;
    // A cell left without a block (the point display cell, an empty or
    // missing definition) places nothing: its instances are dropped.
    let blocks = &c.out.blocks;
    let defined: HashSet<String> = blocks.iter().map(|b| b.0.to_ascii_uppercase()).collect();
    let placed = |e: &EntityType| match e {
        EntityType::Insert(i) => defined.contains(&i.block_name.to_ascii_uppercase()),
        _ => true,
    };
    c.out.entities.retain(placed);
    for (_, entities) in &mut c.out.blocks {
        entities.retain(placed);
    }
    // An anonymous definition (a "*" name, as a dimension's drawing) that
    // nothing places is not kept as a block.
    let anonymous: HashSet<String> = cells
        .iter()
        .filter(|(name, _)| name.starts_with('*'))
        .map(|(name, _)| block_name(name).to_ascii_uppercase())
        .collect();
    let used: HashSet<String> = c
        .out
        .entities
        .iter()
        .chain(c.out.blocks.iter().flat_map(|b| b.1.iter()))
        .filter_map(|e| match e {
            EntityType::Insert(i) => Some(i.block_name.to_ascii_uppercase()),
            _ => None,
        })
        .collect();
    c.out.blocks.retain(|(name, _)| {
        let key = name.to_ascii_uppercase();
        !anonymous.contains(&key) || used.contains(&key)
    });
    c.out
}

/// The Standard mapping of what a file uses: level → layer, line style →
/// linetype, weight → lineweight, colour → colour, one row each.
pub fn mapping_rows(import: &DgnImport, tab: usize) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut add = |dgn: String, dwg: String| {
        if !rows.iter().any(|r| r.0 == dgn) {
            rows.push((dgn, dwg));
        }
    };
    let all = import
        .elements
        .iter()
        .chain(import.cells.iter().flat_map(|c| c.1.iter()));
    for e in all {
        match tab {
            0 => {
                let name = layer_name(import, e.level);
                add(name.clone(), name);
            }
            1 => match e.style {
                By::Own(s) => add(s.to_string(), "Continuous".to_string()),
                By::Cell => add("ByCell".to_string(), "ByBlock".to_string()),
                By::Level => add("ByLevel".to_string(), "ByLayer".to_string()),
            },
            2 => match e.weight {
                By::Own(w) => add(
                    w.to_string(),
                    format!("{:.2} mm", lineweight_of(w) as f64 / 100.0),
                ),
                By::Cell => add("ByCell".to_string(), "ByBlock".to_string()),
                By::Level => add("ByLevel".to_string(), "ByLayer".to_string()),
            },
            _ => match e.color {
                By::Own(c) => {
                    let rgb = c.rgb;
                    add(
                        format!("RGB {},{},{}", rgb[0], rgb[1], rgb[2]),
                        match color_of(rgb) {
                            Color::Index(i) => i.to_string(),
                            _ => format!("{},{},{}", rgb[0], rgb[1], rgb[2]),
                        },
                    )
                }
                By::Cell => add("ByCell".to_string(), "ByBlock".to_string()),
                By::Level => add("ByLevel".to_string(), "ByLayer".to_string()),
            },
        }
    }
    rows
}

// ── Settings ───────────────────────────────────────────────────────────────

/// The import settings kept in the profile: DGNIMPORTMODE (0 a new
/// drawing, 1 the current drawing keeping its own definitions, 2 the
/// current drawing with prefixed names) and the DGN Import window's last
/// choices.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DgnImportSettings {
    pub mode: u8,
    pub explode_text_nodes: bool,
    pub references: ReferenceMode,
    pub existing_reference: ExistingReference,
    /// The V7 text code page the imports keep using (None: 1252).
    pub code_page: Option<u16>,
}

static SETTINGS: std::sync::Mutex<Option<DgnImportSettings>> = std::sync::Mutex::new(None);

pub fn settings() -> DgnImportSettings {
    SETTINGS.lock().ok().and_then(|s| *s).unwrap_or_default()
}

pub fn set_settings(mut settings: DgnImportSettings) {
    settings.mode = settings.mode.min(2);
    // A test that reads the settings back owns them; other tests' apps
    // (which apply their settings on start) wait until it is done.
    #[cfg(test)]
    let _owner = loop {
        let owner = SETTINGS_OWNER.lock().unwrap_or_else(|e| e.into_inner());
        if owner.is_none_or(|id| id == std::thread::current().id()) {
            break owner;
        }
        drop(owner);
        std::thread::yield_now();
    };
    if let Ok(mut s) = SETTINGS.lock() {
        *s = Some(settings);
    }
}

#[cfg(test)]
static SETTINGS_OWNER: std::sync::Mutex<Option<std::thread::ThreadId>> = std::sync::Mutex::new(None);

/// Keeps other threads from changing the settings until dropped.
#[cfg(test)]
pub(crate) struct OwnSettings;

#[cfg(test)]
impl OwnSettings {
    pub(crate) fn take() -> Self {
        loop {
            let mut owner = SETTINGS_OWNER.lock().unwrap_or_else(|e| e.into_inner());
            if owner.is_none() {
                *owner = Some(std::thread::current().id());
                return Self;
            }
            drop(owner);
            std::thread::yield_now();
        }
    }
}

#[cfg(test)]
impl Drop for OwnSettings {
    fn drop(&mut self) {
        *SETTINGS_OWNER.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// The code pages a V7 file's text can be read with, as picker labels (the
/// encodings' own names, which conventionally stay untranslated) paired with
/// the page the parser takes. 1252 is the default.
/// The picker rows: the label and the page the parser takes. The auto
/// sentinel (0) detects from the file's own text runs; 1252 is the plain
/// default.
const CODE_PAGE_TABLE: [(&str, u16); 14] = [
    ("Windows-1252 (Western, default)", 1252),
    ("Windows-1250 (Central European)", 1250),
    ("Windows-1251 (Cyrillic)", 1251),
    ("Windows-1253 (Greek)", 1253),
    ("Windows-1254 (Turkish)", 1254),
    ("Windows-1255 (Hebrew)", 1255),
    ("Windows-1256 (Arabic)", 1256),
    ("Windows-1257 (Baltic)", 1257),
    ("Windows-1258 (Vietnamese)", 1258),
    ("Windows-874 (Thai)", 874),
    ("Shift-JIS (Japanese, 932)", 932),
    ("GBK (Simplified Chinese, 936)", 936),
    ("Korean Wansung (949)", 949),
    ("Big5 (Traditional Chinese, 950)", 950),
];

/// The picker's labels in order.
pub fn code_page_choices() -> Vec<String> {
    CODE_PAGE_TABLE
        .iter()
        .map(|(label, _)| label.to_string())
        .collect()
}

/// The picker label's page; 1252 decodes as the default.
pub fn code_page_from_label(label: &str) -> Option<Option<u16>> {
    CODE_PAGE_TABLE
        .iter()
        .find(|(choice, _)| *choice == label)
        .map(|&(_, page)| (page != 1252).then_some(page))
}

pub fn code_page_label(code_page: Option<u16>) -> String {
    let page = code_page.unwrap_or(1252);
    CODE_PAGE_TABLE
        .iter()
        .find(|(_, known)| *known == page)
        .map_or_else(|| format!("Windows-{page}"), |(label, _)| label.to_string())
}

/// The mapping setup token, optionally suffixed with the V7 text code page
/// the import keeps: `Standard;codepage=1250`. The keyword is
/// case-insensitive, like the setup name itself.
pub fn parse_mapping_token(text: &str) -> Result<(String, Option<u16>), String> {
    let (mapping, page) = text
        .split_once(';')
        .map_or((text, ""), |(mapping, page)| (mapping, page));
    let page = match page.trim().to_ascii_lowercase().as_str() {
        "" => None,
        rest => match rest.strip_prefix("codepage=").unwrap_or(rest) {
            "1252" | "" => None,
            page => match page
                .parse::<u16>()
                .ok()
                .filter(|p| crate::scene::model::underlay_vector::code_page_supported(*p))
            {
                Some(page) => Some(page),
                None => {
                    return Err(format!(
                        "Unknown code page {page}; use another Windows page or 65001."
                    ));
                }
            },
        },
    };
    Ok((mapping.trim().to_string(), page))
}

// ── Command line ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Path,
    Model,
    ListModels,
    Conversion,
    Mapping,
    ListMappings,
}

pub struct DgnImportCommand {
    step: Step,
    path: String,
    models: Vec<String>,
    model: String,
    sub_units: bool,
    /// The V7 text code page (None: 1252), from the mapping token.
    code_page: Option<u16>,
}

impl Default for DgnImportCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl DgnImportCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Path,
            path: String::new(),
            models: Vec::new(),
            model: String::new(),
            sub_units: false,
            code_page: None,
        }
    }

    fn accept_path(&mut self, text: &str) -> CmdResult {
        let path = text.trim().trim_matches('"').to_string();
        if path.is_empty() {
            return CmdResult::NeedPoint;
        }
        let models = crate::scene::model::underlay_vector::dgn_import_models(&path).filter(|m| {
            !m.is_empty()
                && crate::scene::model::underlay_vector::dgn_import(&path, &m[0], None).is_some()
        });
        let Some(models) = models else {
            return CmdResult::ReportError("Invalid or unsupported DGN file.".to_string());
        };
        self.path = path;
        self.model = models[0].clone();
        self.models = models;
        self.step = Step::Model;
        CmdResult::NeedPoint
    }

    fn accept_model(&mut self, text: &str) -> CmdResult {
        let text = text.trim();
        if text == "?" {
            self.step = Step::ListModels;
            return CmdResult::NeedPoint;
        }
        let name = if text.is_empty() {
            self.models[0].as_str()
        } else {
            text
        };
        match self.models.iter().find(|m| m.eq_ignore_ascii_case(name)) {
            Some(found) => {
                self.model = found.clone();
                self.step = Step::Conversion;
                CmdResult::NeedPoint
            }
            None => CmdResult::ReportError(format!("There is no {name} in {}.", self.path)),
        }
    }

    fn list(&mut self, pattern: &str, names: &[&str], back: Step) -> CmdResult {
        let pattern = match pattern.trim() {
            "" => "*",
            p => p,
        };
        let lines: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| crate::io::xref_model::wildcard_match(n, pattern))
            .collect();
        self.step = back;
        CmdResult::ReportMeasurement(lines.join("\n"))
    }

    fn accept_conversion(&mut self, text: &str) -> CmdResult {
        let t = text.trim().to_ascii_uppercase();
        self.sub_units = match t.as_str() {
            "" => false,
            _ if "MASTER".starts_with(&t) => false,
            _ if "SUB".starts_with(&t) => true,
            _ => return CmdResult::ReportError("Invalid option keyword.".to_string()),
        };
        self.step = Step::Mapping;
        CmdResult::NeedPoint
    }

    fn accept_mapping(&mut self, text: &str) -> CmdResult {
        if text.trim() == "?" {
            self.step = Step::ListMappings;
            return CmdResult::NeedPoint;
        }
        let (mapping, code_page) = match parse_mapping_token(text) {
            Ok(parsed) => parsed,
            Err(error) => return CmdResult::ReportError(error),
        };
        if !mapping.is_empty() && !mapping.eq_ignore_ascii_case("Standard") {
            return CmdResult::ReportError(format!("{mapping} not found."));
        }
        // The rest comes from DGNIMPORTMODE and the import settings the
        // window last kept.
        self.code_page = code_page;
        let s = settings();
        let code_page = code_page.or(s.code_page);
        CmdResult::DgnImport(DgnImportRequest {
            path: self.path.clone(),
            model: self.model.clone(),
            sub_units: self.sub_units,
            explode_text_nodes: s.explode_text_nodes,
            current_drawing: s.mode != 0,
            prefix_duplicates: s.mode == 2,
            references: s.references,
            existing_reference: s.existing_reference,
            code_page,
        })
    }
}

impl CadCommand for DgnImportCommand {
    fn name(&self) -> &'static str {
        "DGNIMPORT"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Path => "Enter filename for DGN import:".to_string(),
            Step::Model => format!("Enter name of model or [?] <{}>:", self.models[0]),
            Step::ListModels => "Enter model(s) to list <*>:".to_string(),
            Step::Conversion => "Specify conversion units [Master/Sub] <Master>:".to_string(),
            Step::Mapping => "Specify mapping setup or [?] <Standard>:".to_string(),
            Step::ListMappings => "Enter mapping setup(s) to list <*>:".to_string(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Model | Step::Mapping => vec![CmdOption::new("?", "?")],
            Step::Conversion => vec![CmdOption::new("Master", "M"), CmdOption::new("Sub", "S")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Path | Step::Model => InputKind::FreeText,
            _ => InputKind::SingleToken,
        }
    }

    fn on_point(&mut self, _pt: glam::DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match self.step {
            Step::Path => self.accept_path(text),
            Step::Model => self.accept_model(text),
            Step::ListModels => {
                let models = self.models.clone();
                let names: Vec<&str> = models.iter().map(String::as_str).collect();
                self.list(text, &names, Step::Model)
            }
            Step::Conversion => self.accept_conversion(text),
            Step::Mapping => self.accept_mapping(text),
            Step::ListMappings => match self.list(text, &["Standard"], Step::Mapping) {
                CmdResult::ReportMeasurement(lines) => {
                    CmdResult::ReportMeasurement(format!("Import mapping setups:\n{lines}"))
                }
                other => other,
            },
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Path => CmdResult::Cancel,
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }
}

#[cfg(test)]
mod cell_sharing_tests {
    use super::*;
    use crate::scene::model::underlay_vector::{By, DgnColor, DgnElement, DgnGeom, DgnImport};

    fn line(from: P3, to: P3) -> DgnElement {
        DgnElement {
            level: 1,
            color: By::Own(DgnColor {
                index: 3,
                table: [8, 8, 8],
                rgb: [8, 8, 8],
            }),
            weight: By::Own(1),
            style: By::Own(0),
            geom: DgnGeom::Line(from, to),
            filled: false,
            construction: false,
        }
    }

    fn cell(name: &str, origin: P3, dx: f64) -> DgnElement {
        let x = origin[0] + dx;
        DgnElement {
            level: 1,
            color: By::Own(DgnColor {
                index: 3,
                table: [8, 8, 8],
                rgb: [8, 8, 8],
            }),
            weight: By::Own(1),
            style: By::Own(0),
            geom: DgnGeom::CellGroup {
                name: name.to_string(),
                origin,
                parts: vec![line([x + 10.0, origin[1], 0.0], [x + 40.0, origin[1], 0.0])],
            },
            filled: false,
            construction: false,
        }
    }

    fn import_with(elements: Vec<DgnElement>) -> DgnImport {
        DgnImport {
            elements,
            cells: Vec::new(),
            levels: [(1, ("Level 1".into(), None))].into(),
            level_order: vec![1],
            sub_per_master: 1.0,
            unit_names: ("m".into(), "mm".into()),
            v7: true,
            references: Vec::new(),
            text_styles: Vec::new(),
            ..Default::default()
        }
    }

    /// Repeats of one symbol cell become one block and one insert per
    /// placement: a survey file that carries the same cell thousands of
    /// times must not flood the palette with renamed definitions.
    #[test]
    fn identical_cells_share_one_definition() {
        let import = import_with(vec![
            cell("4.02", [10_000.0, 0.0, 0.0], 0.0),
            cell("4.02", [20_000.0, 5_000.0, 0.0], 0.0),
        ]);
        let c = convert(&import, true, false);
        assert_eq!(c.blocks.len(), 1, "{}", c.blocks.len());
        assert_eq!(c.blocks[0].0, "4.02");
        // Two inserts at the two placements.
        assert_eq!(c.entities.len(), 2, "{}", c.entities.len());
    }

    /// Different objects under one name keep their own definitions (the
    /// second one renamed), exactly as before.
    #[test]
    fn differently_shaped_same_named_cells_stay_separate() {
        let import = import_with(vec![
            cell("4.02", [10_000.0, 0.0, 0.0], 0.0),
            cell("4.02", [10_000.0, 0.0, 0.0], 50.0),
        ]);
        let c = convert(&import, true, false);
        assert_eq!(c.blocks.len(), 2, "{}", c.blocks.len());
        assert_eq!(c.blocks[0].0, "4.02");
        assert_eq!(c.blocks[1].0, "4.021");
        assert_eq!(c.entities.len(), 2);
    }
}

#[cfg(test)]
mod mapping_token_tests {
    use super::parse_mapping_token;

    #[test]
    fn mapping_token_splits_the_code_page() {
        for (text, mapping, page) in [
            ("Standard", "Standard", None),
            ("", "", None),
            ("standard;codepage=1250", "standard", Some(1250)),
            ("Standard ; CODEPAGE=1252", "Standard", None),
            ("Standard;codepage=1251", "Standard", Some(1251)),
            ("Standard;codepage=1257", "Standard", Some(1257)),
            ("Standard;codepage=65001", "Standard", Some(65001)),
            ("Standard;codepage=932", "Standard", Some(932)),
        ] {
            assert_eq!(
                parse_mapping_token(text).ok(),
                Some((mapping.to_owned(), page)),
                "{text}"
            );
        }
        for bad in [
            "Standard;codepage=9999",
            "Standard;page=1250",
            "Standard;codepage=1259",
            "Standard;codepage=931",
            "Standard;codepage=auto",
        ] {
            assert!(parse_mapping_token(bad).is_err(), "{bad}");
        }
    }
}
