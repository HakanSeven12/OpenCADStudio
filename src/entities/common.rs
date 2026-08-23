use std::cell::Cell;

use crate::scene::model::object::{GripDef, GripShape, PropValue, Property};

/// Linear / angular unit format pulled from the document header so the
/// per-thread properties pipeline can format values consistently without
/// passing the document through every callsite.
#[derive(Clone, Copy, Default)]
pub struct UnitContext {
    /// LUNITS — 1=Sci, 2=Decimal, 3=Engineering, 4=Architectural, 5=Fractional
    pub lunits: i16,
    /// LUPREC — decimal places (linear)
    pub luprec: i16,
    /// AUNITS — 0=Decimal degrees, 1=DMS, 2=Grad, 3=Rad, 4=Surveyor. Surfaced
    /// via `format_angle`, which is read on demand by code that already
    /// formats angular values via radians.
    #[allow(dead_code)]
    pub aunits: i16,
    /// AUPREC — decimal places (angular)
    #[allow(dead_code)]
    pub auprec: i16,
    /// ANGBASE — the world direction that a written angle of zero points in,
    /// in radians. Applies to directions, never to angular sizes.
    pub angbase: f64,
    /// ANGDIR — true when written angles grow clockwise.
    pub angdir_cw: bool,
}

impl UnitContext {
    /// The settings as this drawing holds them. Every place that formats or
    /// reads a number seeds the context from here, so none of them can be
    /// working from a different idea of the drawing's conventions.
    pub fn from_header(header: &acadrust::document::HeaderVariables) -> Self {
        Self {
            lunits: header.linear_unit_format,
            luprec: header.linear_unit_precision,
            aunits: header.angular_unit_format,
            auprec: header.angular_unit_precision,
            angbase: header.angle_base,
            angdir_cw: header.angle_direction != 0,
        }
    }
}

thread_local! {
    static UNIT_CTX: Cell<UnitContext> = const { Cell::new(UnitContext {
        lunits: 2,
        luprec: 4,
        aunits: 0,
        auprec: 0,
        angbase: 0.0,
        angdir_cw: false,
    }) };
}

thread_local! {
    /// Text styles that fix their own height, by lowercased name.
    ///
    /// A style with a non-zero height fixes the size of everything drawn in it
    /// — the CAD that writes the file skips the height prompt for such a style,
    /// and the properties palette shows the height without letting it be
    /// changed. The panel builders are handed an entity, not the document it
    /// came from, so the lookup rides here beside the unit context, seeded from
    /// the same place.
    static FIXED_TEXT_HEIGHTS: std::cell::RefCell<rustc_hash::FxHashMap<String, f64>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
}

/// Record which text styles fix their height, from the drawing's style table.
pub fn set_fixed_text_heights(document: &acadrust::CadDocument) {
    FIXED_TEXT_HEIGHTS.with(|cell| {
        let mut map = cell.borrow_mut();
        map.clear();
        for style in document.text_styles.iter() {
            if style.height > 0.0 {
                map.insert(style.name.to_ascii_lowercase(), style.height);
            }
        }
    });
}

/// The height `style` fixes, or `None` when it leaves the height to the entity.
pub fn style_fixed_height(style: &str) -> Option<f64> {
    let key = style.trim().to_ascii_lowercase();
    FIXED_TEXT_HEIGHTS.with(|cell| cell.borrow().get(&key).copied())
}

/// Set the per-thread unit context. Properties helpers consult it when
/// they format f64 values into display strings.
pub fn set_unit_context(ctx: UnitContext) {
    UNIT_CTX.with(|c| c.set(ctx));
}

pub fn unit_context() -> UnitContext {
    UNIT_CTX.with(|c| c.get())
}

/// Format a linear length using LUNITS / LUPREC. Architectural / fractional
/// produce "n'-d/D"" style strings (1 unit = 1 inch); decimal / scientific /
/// engineering / Windows-desktop fall back to plain decimal at LUPREC places.
pub fn format_length(value: f64) -> String {
    let ctx = unit_context();
    let prec = ctx.luprec.max(0) as usize;
    match ctx.lunits {
        1 => format!("{:.*e}", prec, value),
        3 => {
            // Round before splitting so twelve inches carries into feet.
            let sign = if value < 0.0 { "-" } else { "" };
            let abs = value.abs();
            let scale = 10f64.powi(prec.min(15) as i32);
            let total = (abs * scale).round();
            let per_foot = 12.0 * scale;
            let feet = (total / per_foot).trunc();
            let rem = (total - feet * per_foot) / scale;
            format!("{}{:.0}'-{:.*}\"", sign, feet, prec, rem)
        }
        4 | 5 => {
            // Architectural and fractional formats use 1/64-inch resolution.
            let sign = if value < 0.0 { "-" } else { "" };
            let abs = value.abs();
            let denom = 64.0;
            let ticks = (abs * denom).round();
            let (feet, rest) = if ctx.lunits == 4 {
                let per_foot = 12.0 * denom;
                (Some((ticks / per_foot).floor()), ticks.rem_euclid(per_foot))
            } else {
                (None, ticks)
            };
            let whole = (rest / denom).floor();
            let mut n = (rest - whole * denom).round() as u64;
            let mut d = denom as u64;
            while d > 1 && n % 2 == 0 && d % 2 == 0 {
                n /= 2;
                d /= 2;
            }
            let frac_str = if n == 0 || d == 1 {
                String::new()
            } else {
                format!(" {}/{}", n, d)
            };
            let unit_suffix = if ctx.lunits == 4 { "\"" } else { "" };
            match feet {
                Some(f) => format!("{}{:.0}'-{:.0}{}{}", sign, f, whole, frac_str, unit_suffix),
                None => format!("{}{:.0}{}", sign, whole, frac_str),
            }
        }
        _ => format!("{:.*}", prec, value),
    }
}

/// Format an area with the drawing's linear precision. Area stays a decimal
/// scalar even when linear distances use architectural or fractional notation.
pub fn format_area(value: f64) -> String {
    let ctx = unit_context();
    let precision = ctx.luprec.max(0).min(15) as usize;
    if ctx.lunits == 1 {
        format!("{:.*e}", precision, value)
    } else {
        format!("{:.*}", precision, value)
    }
}

/// Format an angle (input in radians) using AUNITS / AUPREC.
pub fn format_angle(value_rad: f64) -> String {
    let ctx = unit_context();
    let prec = ctx.auprec.max(0) as usize;
    match ctx.aunits {
        1 => dms(value_rad.to_degrees(), prec),
        2 => {
            let g = value_rad.to_degrees() / 0.9;
            format!("{:.*}g", prec, g)
        }
        3 => format!("{:.*}r", prec, value_rad),
        4 => surveyor(value_rad, prec),
        _ => format!("{:.*}°", prec, value_rad.to_degrees()),
    }
}

/// Degrees / minutes / seconds. Each part carries its own mark — `d`, `'`, `"`
/// — so the written angle says which convention it is in.
fn dms(degrees: f64, prec: usize) -> String {
    let sign = if degrees < 0.0 { "-" } else { "" };
    let a = degrees.abs();
    // Round before splitting so seconds carry into minutes and degrees.
    let scale = 10f64.powi(prec.min(15) as i32);
    let per_minute = 60.0 * scale;
    let per_degree = 60.0 * per_minute;
    let ticks = (a * per_degree).round();
    let d = (ticks / per_degree).trunc();
    let rest = ticks - d * per_degree;
    let m = (rest / per_minute).trunc();
    let s = (rest - m * per_minute) / scale;
    format!("{}{:.0}d{:.0}'{:.*}\"", sign, d, m, prec, s)
}

/// Surveyor's units: a bearing away from north or south, toward east or west,
/// so the angle is never more than a quarter turn — `N 45d0'0" E`.
///
/// Due north, south, east and west have no bearing to quote and are written as
/// the single letter, which is also what keeps a 90° angle from reading as the
/// contradictory `N 90d0'0" E`.
fn surveyor(value_rad: f64, prec: usize) -> String {
    // Quantize before choosing a cardinal or quadrant.
    let scale = 3600.0 * 10f64.powi(prec.min(15) as i32);
    let turn = 360.0 * scale;
    let ticks = (value_rad.to_degrees().rem_euclid(360.0) * scale)
        .round()
        .rem_euclid(turn);
    let at = |degrees: f64| (ticks - degrees * scale).abs() < 0.5;
    if at(0.0) {
        return "E".into();
    }
    if at(90.0) {
        return "N".into();
    }
    if at(180.0) {
        return "W".into();
    }
    if at(270.0) {
        return "S".into();
    }
    let deg = ticks / scale;
    // Measured from the nearer pole, toward the side the angle falls on.
    let (pole, bearing, side) = if deg < 90.0 {
        ("N", 90.0 - deg, "E")
    } else if deg < 180.0 {
        ("N", deg - 90.0, "W")
    } else if deg < 270.0 {
        ("S", 270.0 - deg, "W")
    } else {
        ("S", deg - 270.0, "E")
    };
    format!("{pole} {} {side}", dms(bearing, prec))
}

// ── Reading numbers back ───────────────────────────────────────────────────
//
// The inverses of the formatters above, kept beside them so the pair cannot
// drift: whatever the drawing writes, it can be handed back. Reading is the
// looser of the two — it takes the shorthands people type as well as the full
// forms — but it never accepts a form the drawing could not have produced.

/// Read a length. Accepts plain decimals, feet-and-inches, and fractions.
///
/// Feet and inches separate with a dash, a space, or nothing, and the closing
/// `"` is optional: `5'-9 1/2"`, `5' 9-1/2`, `5'9`, `9 1/2`, `1/2`, `60"` and
/// `5'` all read. As with the architectural and engineering formats, the
/// notation itself means one unit is one inch.
///
/// A leading `-` is a sign, but the `-` inside `5'-9"` is a separator — after
/// feet there is nothing left to subtract from.
pub fn parse_length(text: &str) -> Option<f64> {
    let text = text.trim();
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest.trim_start()),
        None => (1.0, text.strip_prefix('+').unwrap_or(text).trim_start()),
    };
    if rest.is_empty() {
        return None;
    }
    let magnitude = match rest.split_once('\'') {
        Some((feet, inches)) => {
            let feet: f64 = feet.trim().parse().ok()?;
            let inches = inches.trim().trim_end_matches('"').trim();
            feet * 12.0 + parse_inches(inches.trim_start_matches('-').trim())?
        }
        None => parse_inches(rest.trim_end_matches('"').trim())?,
    };
    Some(sign * magnitude)
}

/// A count of inches: `9`, `9.5`, `1/2`, `9 1/2`, `9-1/2`, or nothing at all.
fn parse_inches(text: &str) -> Option<f64> {
    if text.is_empty() {
        return Some(0.0);
    }
    let Some(slash) = text.find('/') else {
        // No fraction, so any `-` left in here belongs to an exponent
        // (`3.35E-01`) rather than to a separator.
        return text.parse().ok();
    };
    let denominator: f64 = text[slash + 1..].trim().parse().ok()?;
    if denominator == 0.0 {
        return None;
    }
    // The numerator is the number nearest the slash; anything before the
    // separator in front of it is a whole count of inches.
    let head = text[..slash].trim();
    let (whole, numerator) = match head.rfind([' ', '-']) {
        Some(cut) => (head[..cut].trim(), head[cut + 1..].trim()),
        None => ("", head),
    };
    let numerator: f64 = numerator.parse().ok()?;
    let whole: f64 = if whole.is_empty() {
        0.0
    } else {
        whole.parse().ok()?
    };
    Some(whole + numerator / denominator)
}

/// Read an angle, in radians.
///
/// A mark decides the convention — `g` grads, `r` radians, `d`/`'`/`"`
/// degrees-minutes-seconds, compass letters a surveyor's bearing. A bare
/// number is read in whatever convention the drawing is set to, so what the
/// readout shows can be typed straight back.
pub fn parse_angle(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let upper = text.to_ascii_uppercase();
    if upper.starts_with('N') || upper.starts_with('S') {
        return parse_surveyor(&upper);
    }
    if let Some(body) = upper.strip_suffix('G') {
        return Some((body.trim().parse::<f64>().ok()? * 0.9).to_radians());
    }
    if let Some(body) = upper.strip_suffix('R') {
        return body.trim().parse().ok();
    }
    // Bare east/west read as the directions they name.
    if upper == "E" {
        return Some(0.0);
    }
    if upper == "W" {
        return Some(std::f64::consts::PI);
    }
    if upper.contains('D') || upper.contains('°') || upper.contains('\'') {
        return Some(parse_dms(&upper)?.to_radians());
    }
    let value: f64 = upper.parse().ok()?;
    Some(match unit_context().aunits {
        2 => (value * 0.9).to_radians(),
        3 => value,
        _ => value.to_radians(),
    })
}

/// Degrees, minutes and seconds, each part optional after the first:
/// `45`, `45d`, `45d20'`, `45d20'6"`.
fn parse_dms(upper: &str) -> Option<f64> {
    let (sign, rest) = match upper.strip_prefix('-') {
        Some(rest) => (-1.0, rest.trim_start()),
        None => (1.0, upper),
    };
    let (degrees, rest) = match rest.find(['D', '°']) {
        Some(at) => (
            rest[..at].trim().parse::<f64>().ok()?,
            rest[at + rest[at..].chars().next()?.len_utf8()..].trim(),
        ),
        None => (rest.trim().parse::<f64>().ok()?, ""),
    };
    let (minutes, rest) = match rest.split_once('\'') {
        Some((minutes, rest)) if !minutes.trim().is_empty() => {
            (minutes.trim().parse::<f64>().ok()?, rest.trim())
        }
        Some((_, rest)) => (0.0, rest.trim()),
        None => (0.0, rest),
    };
    let seconds = match rest.trim_end_matches('"').trim() {
        "" => 0.0,
        seconds => seconds.parse::<f64>().ok()?,
    };
    Some(sign * (degrees + minutes / 60.0 + seconds / 3600.0))
}

/// A surveyor's bearing — `N45D20'6"E`, or a lone `N` / `S` for due north and
/// south. The inverse of [`surveyor`].
fn parse_surveyor(upper: &str) -> Option<f64> {
    let pole = upper.chars().next()?;
    let body = upper[1..].trim();
    if body.is_empty() {
        return Some(if pole == 'N' {
            std::f64::consts::FRAC_PI_2
        } else {
            3.0 * std::f64::consts::FRAC_PI_2
        });
    }
    let side = body.chars().last()?;
    if !matches!(side, 'E' | 'W') {
        return None;
    }
    let bearing = parse_dms(body[..body.len() - 1].trim())?;
    let degrees = match (pole, side) {
        ('N', 'E') => 90.0 - bearing,
        ('N', _) => 90.0 + bearing,
        (_, 'W') => 270.0 - bearing,
        _ => 270.0 + bearing,
    };
    Some(degrees.to_radians())
}

/// Read a length typed at a command prompt.
///
/// Same forms as [`parse_length`], plus the comma some keyboards put where a
/// decimal point belongs. A prompt asking for one value has no other use for a
/// comma, so taking it here is safe in a way it would not be inside a
/// coordinate, where the comma separates the axes.
pub fn parse_typed_length(text: &str) -> Option<f64> {
    parse_length(&text.replace(',', "."))
}

/// Read an angle typed at a command prompt, in radians. Same forms as
/// [`parse_angle`], with the same tolerance for a decimal comma.
pub fn parse_typed_angle(text: &str) -> Option<f64> {
    parse_angle(&text.replace(',', "."))
}

// ── Directions ─────────────────────────────────────────────────────────────
//
// A direction is not an angular size. Which way something points is measured
// from ANGBASE and runs the way ANGDIR says, while how wide an arc opens is
// the same number whatever zero the drawing counts from. Only directions go
// through the pair below; `format_angle` and `parse_angle` stay for sizes.

/// Write a world direction the way this drawing counts directions.
pub fn format_direction(world_rad: f64) -> String {
    let ctx = unit_context();
    let relative = world_rad - ctx.angbase;
    let shown = if ctx.angdir_cw { -relative } else { relative };
    format_angle(shown.rem_euclid(std::f64::consts::TAU))
}

/// Read a direction written the way this drawing counts them, as a world
/// angle.
pub fn parse_direction(text: &str) -> Option<f64> {
    let ctx = unit_context();
    let typed = parse_angle(text)?;
    let relative = if ctx.angdir_cw { -typed } else { typed };
    Some(relative + ctx.angbase)
}

/// Read a direction typed at a command prompt, accepting a decimal comma.
pub fn parse_typed_direction(text: &str) -> Option<f64> {
    parse_direction(&text.replace(',', "."))
}

/// Two interior triangles covering a quad (flat list, 6 vertices) — the
/// click-anywhere pick surface for frame-like entities (image, OLE frame,
/// underlay, wipeout). Corners in ring order.
pub fn quad_pick_tris(c: &[[f64; 3]; 4]) -> Vec<[f64; 3]> {
    vec![c[0], c[1], c[2], c[0], c[2], c[3]]
}

pub fn square_grip(id: usize, world: glam::DVec3) -> GripDef {
    GripDef {
        id,
        world,
        is_midpoint: false,
        shape: GripShape::Square,
        dir: None,
        axis: None,
    }
}

/// Centre / translate grip — same square marker as a vertex grip but
/// flagged as a "whole-object move" handle for the grip-edit code.
pub fn center_grip(id: usize, world: glam::DVec3) -> GripDef {
    GripDef {
        id,
        world,
        is_midpoint: true,
        shape: GripShape::Square,
        dir: None,
        axis: None,
    }
}

/// Circle grip — a round handle, flagged as a whole-object move. Used for
/// special anchors like a hatch's pattern origin.
pub fn circle_grip(id: usize, world: glam::DVec3) -> GripDef {
    GripDef {
        id,
        world,
        is_midpoint: true,
        shape: GripShape::Circle,
        dir: None,
        axis: None,
    }
}

/// Round grip that edits one vertex.
pub fn round_grip(id: usize, world: glam::DVec3) -> GripDef {
    GripDef {
        id,
        world,
        is_midpoint: false,
        shape: GripShape::Circle,
        dir: None,
        axis: None,
    }
}

/// Screen-offset down-arrow that opens a choice menu for an entity.
pub fn dropdown_grip(id: usize, world: glam::DVec3) -> GripDef {
    GripDef {
        id,
        world,
        is_midpoint: false,
        shape: GripShape::Dropdown,
        dir: None,
        axis: None,
    }
}

/// Mid-segment stretch grip oriented along `dir` (the segment's in-plane
/// world-XY direction). Drawn as a small rectangle elongated along the
/// segment so the affordance reads as "stretch perpendicular".
pub fn rectangle_grip(id: usize, world: glam::DVec3, dir: [f32; 2]) -> GripDef {
    GripDef {
        id,
        world,
        is_midpoint: true,
        shape: GripShape::Rectangle,
        dir: Some(glam::DVec3::new(dir[0] as f64, dir[1] as f64, 0.0)),
        axis: None,
    }
}

#[allow(dead_code)]
pub fn triangle_grip(id: usize, world: glam::DVec3) -> GripDef {
    GripDef {
        id,
        world,
        is_midpoint: false,
        shape: GripShape::Triangle,
        dir: None,
        axis: None,
    }
}

/// Triangle grip pointing along a world-space direction.
pub fn oriented_triangle_grip(id: usize, world: glam::DVec3, dir: glam::DVec3) -> GripDef {
    GripDef {
        id,
        world,
        is_midpoint: false,
        shape: GripShape::Triangle,
        dir: Some(dir),
        axis: None,
    }
}

/// Editable ANGLE row: displays via AUNITS/AUPREC. Angle rows used the
/// LINEAR formatter, so LUNITS=Architectural showed a block rotation as
/// feet-and-inches and the string wouldn't parse back (#297). Value in
/// DEGREES, matching every angle call site.
pub fn edit_angle_prop(label: &str, field: &'static str, value_deg: f64) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::EditText(format_angle(value_deg.to_radians())),
    }
}

pub fn edit_prop(label: &str, field: &'static str, value: f64) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::EditText(format_length(value)),
    }
}

/// Editable dimensionless value independent of drawing units.
pub fn edit_scalar_prop(label: &str, field: &'static str, value: f64) -> Property {
    let value = if value == 0.0 { 0.0 } else { value };
    Property {
        label: label.into(),
        field,
        value: PropValue::EditText(value.to_string()),
    }
}

pub fn ro_prop(label: &str, field: &'static str, value: impl Into<String>) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::ReadOnly(value.into()),
    }
}

/// A numeric row that is an editable box when `editable`, otherwise a grayed
/// read-only value using the same length formatting. Used where a field's
/// editability depends on entity state (e.g. a text point that is only live for
/// certain justifications, or an MText column dimension).
pub fn num_prop(label: &str, field: &'static str, value: f64, editable: bool) -> Property {
    if editable {
        edit_prop(label, field, value)
    } else {
        ro_prop(label, field, format_length(value))
    }
}

/// A ◀ / ▶ index navigator row (e.g. a polyline's Current Vertex). `display` is
/// the label shown between the arrows (e.g. "2 / 7").
pub fn stepper_prop(
    label: &str,
    field: &'static str,
    display: impl Into<String>,
) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::Stepper {
            field,
            display: display.into(),
        },
    }
}

pub fn parse_f64(value: &str) -> Option<f64> {
    let t = value.trim();
    // Length rows display via LUNITS and angle rows via AUNITS. Accept both
    // representations back so a value shown by Properties can always be
    // committed unchanged.
    t.parse::<f64>()
        .ok()
        .or_else(|| parse_length(t))
        .or_else(|| parse_angle_deg(t))
}

/// Parse an angle string the panel displayed via AUNITS back to DEGREES:
/// "30", "30°"/"30d", DMS "30°15'20.5\"", grads "33.33g", radians "0.52r".
pub fn parse_angle_deg(value: &str) -> Option<f64> {
    let s = value.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    let lower = s.to_ascii_lowercase();
    if let Some(num) = lower.strip_suffix('g') {
        return num.trim().parse::<f64>().ok().map(|g| g * 0.9);
    }
    if let Some(num) = lower.strip_suffix('r') {
        return num.trim().parse::<f64>().ok().map(f64::to_degrees);
    }
    // Degrees with optional DMS parts: [-]D(°|d)[M'[S"]]
    let mut rest = s;
    let neg = rest.starts_with('-');
    if neg {
        rest = &rest[1..];
    }
    let i = rest.find(['°', 'd', 'D'])?;
    let d: f64 = rest[..i].trim().parse().ok()?;
    let mut total = d;
    let tail = rest[i..]
        .trim_start_matches(['°', 'd', 'D'])
        .trim();
    if !tail.is_empty() {
        let (mpart, spart) = match tail.find('\'') {
            Some(j) => (&tail[..j], &tail[j + 1..]),
            None => (tail, ""),
        };
        if !mpart.trim().is_empty() {
            total += mpart.trim().parse::<f64>().ok()? / 60.0;
        }
        let sp = spart.trim().trim_end_matches('"').trim();
        if !sp.is_empty() {
            total += sp.parse::<f64>().ok()? / 3600.0;
        }
    }
    Some(if neg { -total } else { total })
}

/// Bulge → arc geometry for a polyline segment, from the kernel.
///
/// Re-exported rather than imported at each call site so the twelve modules
/// that already reach for `entities::common::BulgeArc` keep working, and so
/// there is one obvious place to see that the maths moved out.
pub use cadkernel::geom2d::BulgeArc;

/// Triangulate the solid bands a `wide_fills` returns into the flat WCS f64
/// triangle list `RenderEntity::pick_tris` carries, so a wide polyline is
/// selectable across the band it draws and not just along its centreline.
///
/// `origin` and `fills` are that function's own pair: 2-D offsets from the
/// first vertex, which is the exact frame the band's `HatchModel` renders in
/// (`world_origin` + boundary, no elevation). Building the pick geometry from
/// the same numbers keeps hit-testing on whatever the fill actually drew.
///
/// An arc band is an annular sector — concave on its inner edge — so this ear
/// clips rather than fans.
pub(crate) fn wide_band_tris(origin: [f64; 2], fills: &[Vec<[f32; 2]>]) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for poly in fills {
        let ring: Vec<[f64; 3]> = poly
            .iter()
            .map(|&[x, y]| [origin[0] + x as f64, origin[1] + y as f64, 0.0])
            .collect();
        out.extend(crate::entities::mesh::triangulate_planar(&ring));
    }
    out
}

/// Extrude a wide-polyline band (from `wide_fills`) into a solid tube for a DXF
/// thickness (code 39): a vertical wall between every band-boundary point and
/// its `thickness`-along-`normal` copy, plus triangulated bottom and top caps.
/// Returns `(fill_tris, edge_lines)` as flat WCS f64 lists — the caller wraps
/// them in a `RenderEntity` (object = `Lines(edge_lines)`, `fill_tris`, and
/// `pick_tris = fill_tris`). Shared by LwPolyline and Polyline2D so both wide
/// polyline kinds extrude the same solid instead of just their centre-line.
///
/// `polyline_segment_fill` emits each band loop as the outer boundary forward
/// then the inner boundary back, so its two transition edges (`half-1 → half`
/// and `n-1 → 0`) are radial cap ends inside the band — no wall is drawn there.
pub(crate) fn thick_band_tube(
    origin: [f64; 2],
    fills: &[Vec<[f32; 2]>],
    thickness: f64,
    normal: (f64, f64, f64),
    to_wcs: &dyn Fn(f64, f64) -> (f64, f64, f64),
) -> (Vec<[f64; 3]>, Vec<[f64; 3]>) {
    let (nx, ny, nz) = normal;
    let t = thickness;
    let off = |p: [f64; 3]| -> [f64; 3] { [p[0] + t * nx, p[1] + t * ny, p[2] + t * nz] };
    let push_seg = |lines: &mut Vec<[f64; 3]>, a: [f64; 3], b: [f64; 3]| {
        lines.push(a);
        lines.push(b);
        lines.push([f64::NAN; 3]);
    };
    let mut lines: Vec<[f64; 3]> = Vec::new();
    let mut fill_tris: Vec<[f64; 3]> = Vec::new();
    for poly in fills {
        let n = poly.len();
        if n < 4 {
            continue;
        }
        let half = n / 2;
        let bot: Vec<[f64; 3]> = poly
            .iter()
            .map(|&[x, y]| {
                let (wx, wy, wz) = to_wcs(origin[0] + x as f64, origin[1] + y as f64);
                [wx, wy, wz]
            })
            .collect();
        let top: Vec<[f64; 3]> = bot.iter().map(|&p| off(p)).collect();
        for k in 0..n {
            push_seg(&mut lines, bot[k], top[k]);
            if k == half - 1 || k == n - 1 {
                continue;
            }
            let kn = (k + 1) % n;
            push_seg(&mut lines, bot[k], bot[kn]);
            push_seg(&mut lines, top[k], top[kn]);
            fill_tris.extend_from_slice(&[bot[k], bot[kn], top[kn], bot[k], top[kn], top[k]]);
        }
        fill_tris.extend(crate::entities::mesh::triangulate_planar(&bot));
        fill_tris.extend(crate::entities::mesh::triangulate_planar(&top));
    }
    (fill_tris, lines)
}

/// Build a continuous WCS point list and full width for a tapered polyline.
pub(crate) fn tapered_band_points(
    verts: &[([f64; 2], f64, f64, f64)],
    is_closed: bool,
    to_wcs: &dyn Fn(f64, f64) -> (f64, f64, f64),
) -> (Vec<[f64; 3]>, Vec<f32>) {
    let n = verts.len();
    let seg_count = if is_closed { n } else { n.saturating_sub(1) };
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut widths: Vec<f32> = Vec::new();
    let mut push = |x: f64, y: f64, w: f32| {
        let (wx, wy, wz) = to_wcs(x, y);
        pts.push([wx, wy, wz]);
        widths.push(w);
    };
    for i in 0..seg_count {
        let (p0, bulge, sw0, ew0) = verts[i];
        let (p1, _, _, _) = verts[(i + 1) % n];
        if i == 0 {
            push(p0[0], p0[1], sw0 as f32);
        }
        if bulge.abs() < 1e-9 {
            push(p1[0], p1[1], ew0 as f32);
        } else if let Some(arc) = BulgeArc::from_bulge(p0, p1, bulge) {
            let samples = arc.tessellate_angle(cadkernel::tessellation::DEFAULT_ANGLE);
            let segments = samples.len().saturating_sub(1).max(1);
            for (index, s) in samples.into_iter().enumerate().skip(1) {
                let t = index as f64 / segments as f64;
                push(s[0], s[1], (sw0 + (ew0 - sw0) * t) as f32);
            }
        }
    }
    (pts, widths)
}

/// Compute the filled boundary polygon for one polyline segment.
/// For straight segments: a rectangle/trapezoid.
/// For arc segments: an arc band (outer arc + reversed inner arc).
/// Returns `None` if the segment is degenerate.
pub(crate) fn polyline_segment_fill(
    p0: [f32; 2],
    p1: [f32; 2],
    hw0: f32,
    hw1: f32,
    bulge: f32,
) -> Option<Vec<[f32; 2]>> {
    if bulge.abs() < 1e-9 {
        // Straight segment — rectangle or trapezoid
        let dx = p1[0] - p0[0];
        let dy = p1[1] - p0[1];
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-9 {
            return None;
        }
        let nx = -dy / len;
        let ny = dx / len;
        Some(vec![
            [p0[0] + hw0 * nx, p0[1] + hw0 * ny],
            [p1[0] + hw1 * nx, p1[1] + hw1 * ny],
            [p1[0] - hw1 * nx, p1[1] - hw1 * ny],
            [p0[0] - hw0 * nx, p0[1] - hw0 * ny],
        ])
    } else {
        // Arc segment — arc band polygon.
        // Center math matches `bulge_to_arc` in modules/home/modify/explode.rs:
        //   r = chord * (1 + b²) / (4·|b|)
        //   d = r * (1 - b²) / (1 + b²)   (signed: negative ⇒ major arc, center
        //                                  flips to the opposite side of chord)
        //   center = midpoint + sign(b) · d · left_perp(chord)
        let b = bulge as f64;
        let b2 = b * b;
        let dx = (p1[0] - p0[0]) as f64;
        let dy = (p1[1] - p0[1]) as f64;
        let chord_len = (dx * dx + dy * dy).sqrt();
        if chord_len < 1e-9 || b.abs() < 1e-12 {
            return None;
        }
        let r = chord_len * (1.0 + b2) / (4.0 * b.abs());
        let d_perp = r * (1.0 - b2) / (1.0 + b2);
        let mx = ((p0[0] + p1[0]) * 0.5) as f64;
        let my = ((p0[1] + p1[1]) * 0.5) as f64;
        let perp_x = -dy / chord_len;
        let perp_y = dx / chord_len;
        let sign = b.signum();
        let cx = (mx + sign * d_perp * perp_x) as f32;
        let cy = (my + sign * d_perp * perp_y) as f32;
        let a0 = ((p0[1] - cy) as f32).atan2((p0[0] - cx) as f32);
        let a1 = ((p1[1] - cy) as f32).atan2((p1[0] - cx) as f32);
        let (sa, mut ea) = if bulge > 0.0 { (a0, a1) } else { (a1, a0) };
        if ea < sa {
            ea += std::f32::consts::TAU;
        }
        let span = ea - sa;
        let segs = ((span.abs() / std::f32::consts::TAU) * 24.0)
            .ceil()
            .max(4.0) as u32;
        let r = r as f32;
        let r_outer = |t: f32| r + (hw0 + (hw1 - hw0) * t);
        let r_inner = |t: f32| (r - (hw0 + (hw1 - hw0) * t)).max(0.0);
        let mut boundary = Vec::with_capacity((segs as usize + 1) * 2);
        let inv = 1.0 / segs as f32;
        for j in 0..=segs {
            let t = j as f32 * inv;
            let ang = sa + span * t;
            let ro = r_outer(t);
            boundary.push([cx + ro * ang.cos(), cy + ro * ang.sin()]);
        }
        for j in (0..=segs).rev() {
            let t = j as f32 * inv;
            let ang = sa + span * t;
            let ri = r_inner(t);
            boundary.push([cx + ri * ang.cos(), cy + ri * ang.sin()]);
        }
        Some(boundary)
    }
}

#[cfg(test)]
mod length_format_tests {
    use super::*;

    fn with_units(lunits: i16, luprec: i16, value: f64) -> String {
        let mut ctx = unit_context();
        ctx.lunits = lunits;
        ctx.luprec = luprec;
        set_unit_context(ctx);
        format_length(value)
    }

    #[test]
    fn fractional_carries_a_rounded_up_fraction() {
        assert_eq!(with_units(5, 4, 5.99), "5 63/64");
        assert_eq!(with_units(5, 4, 5.995), "6");
        assert_eq!(with_units(5, 4, 0.995), "1");
        assert_eq!(with_units(5, 4, 11.999), "12");
        assert_eq!(with_units(5, 4, -5.995), "-6");
        assert_eq!(with_units(5, 4, 0.5), "0 1/2");
        assert_eq!(with_units(5, 4, 9.25), "9 1/4");
        assert_eq!(with_units(5, 4, 3.0e17), "300000000000000000");
    }

    #[test]
    fn architectural_carries_into_feet() {
        assert_eq!(with_units(4, 4, 11.99), "0'-11 63/64\"");
        assert_eq!(with_units(4, 4, 11.999), "1'-0\"");
        assert_eq!(with_units(4, 4, 23.999), "2'-0\"");
        assert_eq!(with_units(4, 4, 66.5), "5'-6 1/2\"");
        assert_eq!(with_units(4, 4, -11.999), "-1'-0\"");
    }

    #[test]
    fn engineering_carries_into_feet() {
        assert_eq!(with_units(3, 1, 11.94), "0'-11.9\"");
        assert_eq!(with_units(3, 1, 11.99), "1'-0.0\"");
        assert_eq!(with_units(3, 1, 23.99), "2'-0.0\"");
        assert_eq!(with_units(3, 2, 11.999), "1'-0.00\"");
        assert_eq!(with_units(3, 1, 66.5), "5'-6.5\"");
    }

    #[test]
    fn formatted_lengths_read_back() {
        for lunits in [3, 4, 5] {
            for value in [0.995f64, 5.995, 11.999, 23.999, 66.5, 9.25] {
                let shown = with_units(lunits, if lunits == 3 { 4 } else { 4 }, value);
                let read = parse_length(&shown)
                    .unwrap_or_else(|| panic!("lunits {lunits} wrote {shown:?}, which does not read back"));
                assert!(
                    (read - value).abs() < 0.02,
                    "lunits {lunits}: {value} wrote {shown:?}, read back as {read}"
                );
            }
        }
    }
}

#[cfg(test)]
mod angle_format_tests {
    use super::*;

    fn shown(aunits: i16, auprec: i16, degrees: f64) -> String {
        let mut ctx = unit_context();
        ctx.aunits = aunits;
        ctx.auprec = auprec;
        set_unit_context(ctx);
        format_angle(degrees.to_radians())
    }

    #[test]
    fn degrees_minutes_seconds_carry() {
        assert_eq!(shown(1, 0, 45.9999), "46d0'0\"");
        assert_eq!(shown(1, 0, 89.99999), "90d0'0\"");
        assert_eq!(shown(1, 0, 0.99999), "1d0'0\"");
        assert_eq!(shown(1, 0, 45.5), "45d30'0\"");
        assert_eq!(shown(1, 2, 45.9999), "45d59'59.64\"");
        assert_eq!(shown(1, 2, 45.5), "45d30'0.00\"");
    }

    #[test]
    fn surveyor_bearings_stay_inside_a_quadrant() {
        assert_eq!(shown(4, 0, 359.99999), "E");
        assert_eq!(shown(4, 0, 89.99999), "N");
        assert_eq!(shown(4, 0, 45.0), "N 45d0'0\" E");
        assert_eq!(shown(4, 0, 180.0), "W");
        assert_ne!(shown(4, 8, 0.0000000005), "E");
        for prec in [0, 2] {
            for deg in [0.99999f64, 45.9999, 89.99999, 179.99999, 269.99999, 359.99999] {
                let text = shown(4, prec, deg);
                assert!(
                    !text.contains("90d"),
                    "prec {prec}: {deg} wrote {text:?}, a quarter turn inside a quadrant"
                );
            }
        }
    }

    #[test]
    fn formatted_angles_read_back() {
        for aunits in [0, 1, 4] {
            for prec in [0, 2] {
                for deg in [0.99999f64, 45.5, 45.9999, 89.99999, 200.25, 359.99999] {
                    let text = shown(aunits, prec, deg);
                    let read = parse_angle(&text).unwrap_or_else(|| {
                        panic!("aunits {aunits} prec {prec} wrote {text:?}, which does not read back")
                    });
                    let delta = (read.to_degrees().rem_euclid(360.0) - deg.rem_euclid(360.0))
                        .rem_euclid(360.0);
                    let delta = delta.min(360.0 - delta);
                    assert!(
                        delta < 1.0,
                        "aunits {aunits} prec {prec}: {deg} wrote {text:?}, read back as {}",
                        read.to_degrees()
                    );
                }
            }
        }
    }
}
