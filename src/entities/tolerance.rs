use acadrust::entities::Tolerance;
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TextStroke, TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection};
use crate::scene::wire_model::SnapHint;
use crate::scene::{lff, transform};

// ── GDT text parser ───────────────────────────────────────────────────────────

/// Parse a DXF tolerance text string into rows of cell strings.
///
/// DXF format:  `{\Fgdt;p}%%v0.5%%vA%%vB%%v%%v^J{\Fgdt;j}%%v0.1%%vA%%v%%v%%v`
///   - `^J`  → row separator
///   - `%%v` → cell separator within a row
///   - `{\Fgdt;X}` → GDT symbol X (mapped to a text label)
fn parse_gdt_rows(raw: &str) -> Vec<Vec<String>> {
    raw.split("^J")
        .filter(|row| !row.trim().is_empty())
        .map(|row| {
            row.split("%%v")
                .map(|cell| substitute_gdt_codes(cell.trim()))
                .collect()
        })
        .collect()
}

/// Replace `{\Fgdt;X}` sequences with a short ASCII label and strip other
/// MTEXT-style format codes `{\...}`.
fn substitute_gdt_codes(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            // Collect to closing '}'
            let mut inner = String::new();
            let mut depth = 1usize;
            for c in chars.by_ref() {
                match c {
                    '{' => {
                        depth += 1;
                        inner.push(c);
                    }
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        inner.push(c);
                    }
                    _ => {
                        inner.push(c);
                    }
                }
            }
            // Is it a GDT font switch?
            if let Some(rest) = inner.strip_prefix("\\Fgdt;") {
                // rest is the symbol letter(s)
                if let Some(sym_char) = rest.chars().next() {
                    out.push_str(gdt_char_to_ascii(sym_char));
                }
            }
            // other format codes: skip
        } else {
            out.push(ch);
        }
    }
    out
}

/// Map a GDT font character to a short ASCII approximation.
fn gdt_char_to_ascii(c: char) -> &'static str {
    match c {
        'a' => "SRT", // Straightness
        'b' => "FLT", // Flatness
        'c' => "FLT", // Flatness
        'd' => "PSF", // Profile of Surface
        'e' => "CYL", // Cylindricity
        'f' => "PRL", // Profile of Line
        'g' => "CIR", // Circularity
        'h' => "PAR", // Parallelism
        'i' => "SYM", // Symmetry
        'j' => "PRP", // Perpendicularity
        'k' => "PLN", // Profile of Line
        'l' => "(L)", // LMC
        'm' => "(M)", // MMC / Diameter
        'n' => "ANG", // Angularity
        'o' => "(o)", // at maximum material boundary
        'p' => "POS", // Position
        'q' => "(q)",
        'r' => "RUN", // Circular Runout
        's' => "(S)", // RFS / Regardless of Feature Size
        't' => "TRN", // Total Runout
        'u' => "CON", // Concentricity
        'v' => "(v)",
        'w' => "(w)",
        _ => "?",
    }
}

// ── Feature-control frame builder ─────────────────────────────────────────────

/// Tessellate a Tolerance entity into CXF-style polyline output.
///
/// Returns (`box_lines`, `text_strokes`) where:
///   - `box_lines` — 3-D line segments forming the outer border and dividers
///   - `text_strokes` — 2-D polylines from the CXF tessellator
///
/// Since TruckObject has either Lines (3-D) or Text (2-D CXF), we render the
/// box frame via a separate Lines path (added by the caller) and the text cells
/// via the Text path.  For simplicity this function packs everything into a
/// single Vec<Vec<[f32;2]>> by projecting the box lines to 2-D and using NaN
/// polylines as separators — exactly what the Text wire-builder does.
fn tessellate_tolerance(tol: &Tolerance) -> Vec<Vec<[f32; 2]>> {
    if tol.text.is_empty() {
        return vec![];
    }

    let rows = parse_gdt_rows(&tol.text);
    if rows.is_empty() {
        return vec![];
    }

    // ── Metrics ──────────────────────────────────────────────────────────
    let h = if tol.text_height > 1e-6 {
        tol.text_height as f32
    } else {
        2.5_f32
    };
    // DIMGAP — stored on the entity (resolved from the dim style at creation).
    // Fall back to AutoCAD's 0.35 × height convention only when missing.
    let gap = if tol.dimension_gap > 1e-6 {
        tol.dimension_gap as f32
    } else {
        (h * 0.35).max(0.1)
    };
    let cell_h = h + 2.0 * gap;
    let char_w = h * 0.65;
    let min_cell_w = h * 1.4;

    // Column widths: max across all rows
    let max_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut col_widths: Vec<f32> = vec![0.0_f32; max_cols];
    for row in &rows {
        for (ci, cell) in row.iter().enumerate() {
            let w = (cell.len() as f32 * char_w).max(min_cell_w);
            if ci < col_widths.len() {
                col_widths[ci] = col_widths[ci].max(w);
            }
        }
    }
    let total_w: f32 = col_widths.iter().sum();
    let total_h = cell_h * rows.len() as f32;

    // ── Transform helpers (local space — translation applied in tessellate.rs) ──
    let angle = (tol.direction.y as f32).atan2(tol.direction.x as f32);
    let (sa, ca) = angle.sin_cos();

    // Rotate only; origin is kept as f64 and applied later with full precision.
    let rot = |x: f32, y: f32| -> [f32; 2] { [x * ca - y * sa, x * sa + y * ca] };

    let mut out: Vec<Vec<[f32; 2]>> = Vec::new();

    // ── Outer border ──────────────────────────────────────────────────────
    out.push(vec![
        rot(0.0, 0.0),
        rot(total_w, 0.0),
        rot(total_w, total_h),
        rot(0.0, total_h),
        rot(0.0, 0.0),
    ]);

    // ── Row separators ─────────────────────────────────────────────────────
    for ri in 1..rows.len() {
        let y = cell_h * ri as f32;
        out.push(vec![rot(0.0, y), rot(total_w, y)]);
    }

    // ── Column dividers ────────────────────────────────────────────────────
    let mut x_cursor = 0.0_f32;
    for ci in 0..col_widths.len().saturating_sub(1) {
        x_cursor += col_widths[ci];
        for ri in 0..rows.len() {
            if ci + 1 < rows[ri].len() {
                let y0 = cell_h * ri as f32;
                let y1 = y0 + cell_h;
                out.push(vec![rot(x_cursor, y0), rot(x_cursor, y1)]);
            }
        }
    }

    // ── Text content per cell ─────────────────────────────────────────────
    for (ri, row) in rows.iter().enumerate() {
        let row_y = cell_h * ri as f32 + gap;
        let mut cell_x = 0.0_f32;
        for (ci, cell) in row.iter().enumerate() {
            let cw = if ci < col_widths.len() {
                col_widths[ci]
            } else {
                0.0
            };
            if !cell.is_empty() {
                let text_w = cell.len() as f32 * char_w;
                let tx = cell_x + (cw - text_w) * 0.5;
                // Tessellate text in local frame then transform
                let local_strokes =
                    lff::tessellate_text_ex([0.0, 0.0], h, 0.0, 1.0, 0.0, "txt", cell);
                for polyline in local_strokes {
                    let transformed: Vec<[f32; 2]> = polyline
                        .into_iter()
                        .map(|[px, py]| rot(px + tx, py + row_y))
                        .collect();
                    if !transformed.is_empty() {
                        out.push(transformed);
                    }
                }
            }
            cell_x += cw;
        }
    }

    out
}

// ── TruckConvertible ──────────────────────────────────────────────────────────

impl TruckConvertible for Tolerance {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        if self.text.is_empty() {
            return None;
        }

        let snap_pt = Vec3::new(
            self.insertion_point.x as f32,
            self.insertion_point.y as f32,
            self.insertion_point.z as f32,
        );

        // Build the feature-control frame in local space; origin stored as f64.
        let strokes = tessellate_tolerance(self);
        let origin = [self.insertion_point.x, self.insertion_point.y];

        Some(TruckEntity {
            object: TruckObject::Text(vec![TextStroke { strokes, origin, color: None }]),
            snap_pts: vec![(snap_pt, SnapHint::Insertion)],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
        })
    }
}

// ── Grippable ─────────────────────────────────────────────────────────────────

impl Grippable for Tolerance {
    fn grips(&self) -> Vec<GripDef> {
        vec![square_grip(
            0,
            Vec3::new(
                self.insertion_point.x as f32,
                self.insertion_point.y as f32,
                self.insertion_point.z as f32,
            ),
        )]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id == 0 {
            match apply {
                GripApply::Translate(d) => {
                    self.insertion_point.x += d.x as f64;
                    self.insertion_point.y += d.y as f64;
                    self.insertion_point.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    self.insertion_point.x = p.x as f64;
                    self.insertion_point.y = p.y as f64;
                    self.insertion_point.z = p.z as f64;
                }
            }
        }
    }
}

// ── PropertyEditable ──────────────────────────────────────────────────────────

impl PropertyEditable for Tolerance {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        PropSection {
            title: "Geometry".into(),
            props: vec![
                ro("Text", "tol_text", self.text.clone()),
                edit("Insert X", "tol_ix", self.insertion_point.x),
                edit("Insert Y", "tol_iy", self.insertion_point.y),
                edit("Insert Z", "tol_iz", self.insertion_point.z),
                ro(
                    "Dim Style",
                    "tol_dim_style",
                    if self.dimension_style_name.is_empty() {
                        "(default)".to_string()
                    } else {
                        self.dimension_style_name.clone()
                    },
                ),
                ro(
                    "Dim Style Handle",
                    "tol_dim_style_handle",
                    match self.dimension_style_handle {
                        Some(h) if !h.is_null() => format!("{:X}", h.value()),
                        _ => "(none)".to_string(),
                    },
                ),
                edit("Text Height", "tol_text_height", self.text_height),
                edit("Dim Gap", "tol_dim_gap", self.dimension_gap),
                ro(
                    "Direction",
                    "tol_direction",
                    format!(
                        "{:.3}, {:.3}, {:.3}",
                        self.direction.x, self.direction.y, self.direction.z
                    ),
                ),
                ro(
                    "Normal",
                    "tol_normal",
                    format!(
                        "{:.3}, {:.3}, {:.3}",
                        self.normal.x, self.normal.y, self.normal.z
                    ),
                ),
            ],
        }
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "tol_ix" => self.insertion_point.x = v,
            "tol_iy" => self.insertion_point.y = v,
            "tol_iz" => self.insertion_point.z = v,
            "tol_text_height" if v > 0.0 => self.text_height = v,
            "tol_dim_gap" => self.dimension_gap = v,
            _ => {}
        }
    }
}

// ── Transformable ─────────────────────────────────────────────────────────────

impl Transformable for Tolerance {
    fn apply_transform(&mut self, t: &EntityTransform) {
        transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            transform::reflect_xy_point(
                &mut entity.insertion_point.x,
                &mut entity.insertion_point.y,
                p1,
                p2,
            );
        });
    }
}
