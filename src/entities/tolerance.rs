use acadrust::entities::Tolerance;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{GlyphRun, TextStroke, RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection};
use crate::scene::model::wire_model::SnapHint;
use crate::scene::text::lff;
use crate::scene::view::transform;
use crate::t;

// ── GDT text parser ───────────────────────────────────────────────────────────

/// The font a compartment's symbols come from.
///
/// The source names it in its own escapes, and it is registered under that name
/// like any other font — so nothing here special-cases it. A letter in this font
/// IS a symbol; it holds no Latin text.
const SYMBOL_FONT: &str = "gdt";
/// The font a compartment's ordinary text comes from.
const TEXT_FONT: &str = "txt";

/// One stretch of a compartment drawn in a single font.
#[derive(Debug, Clone, PartialEq)]
struct Run {
    text: String,
    font: &'static str,
}

/// A compartment: the runs it is built from, in order.
type Cell = Vec<Run>;

/// The characters of `cell`, whichever font each came from. For messages and
/// tests — never for measuring, since the runs are in different fonts.
#[cfg(test)]
fn cell_text(cell: &Cell) -> String {
    cell.iter().map(|r| r.text.as_str()).collect()
}

/// Parse a tolerance text string into rows of compartments.
///
/// Example: `{\Fgdt;p}%%v0.5%%vA%%vB%%v%%v` + newline + `{\Fgdt;j}%%v0.1%%vA`
///   - row separator → see below; `%%v` → compartment separator within a row
///   - `{\Fgdt;X}` → character X, drawn from the font that escape names
///
/// The row break reaches us in four encodings and all four are live:
/// a DWG carries a raw newline; the text-DXF reader rewrites the on-disk `^J`
/// to a newline before we ever see it; the binary-DXF reader does not, so `^J`
/// can still arrive literally; and our own DXF writer re-emits embedded
/// newlines as `\P`, because a line-based format cannot carry a raw newline
/// inside a string value. Splitting on `^J` alone — as this did — therefore
/// never fired on the two mainstream paths, and every row collapsed into one.
fn parse_gdt_rows(raw: &str) -> Vec<Vec<Cell>> {
    let norm = raw
        .replace("^J", "\n")
        .replace("\\P", "\n")
        .replace("\r\n", "\n")
        .replace('\r', "\n");

    norm.split('\n')
        .filter(|row| !row.trim().is_empty())
        .map(|row| {
            let mut cells: Vec<Cell> = row.split("%%v").map(|c| parse_cell(c.trim())).collect();
            // Trailing empties are unused compartments, not blank boxes: a row
            // written with spare datum slots ends in `%%v%%v` yet draws only the
            // compartments it filled. Interior empties stay — there the position
            // itself carries meaning (datum A, _, C).
            while cells.last().is_some_and(|c| c.is_empty()) {
                cells.pop();
            }
            cells
        })
        .filter(|cells: &Vec<Cell>| !cells.is_empty())
        .collect()
}

/// Split a compartment into font runs, resolving `{\Fgdt;X}` switches and
/// dropping every other inline format code.
///
/// The escape names its font and carries the character to draw in it; both are
/// kept as-is, so the symbol is looked up by the ordinary font machinery rather
/// than translated into some other character first.
fn parse_cell(s: &str) -> Cell {
    let mut runs: Cell = Vec::new();
    let push = |ch: char, font: &'static str, runs: &mut Cell| match runs.last_mut() {
        Some(r) if r.font == font => r.text.push(ch),
        _ => runs.push(Run {
            text: ch.to_string(),
            font,
        }),
    };

    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '{' {
            push(ch, TEXT_FONT, &mut runs);
            continue;
        }
        // Collect to the closing brace.
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
                _ => inner.push(c),
            }
        }
        if let Some(sym) = symbol_font_switch(&inner) {
            push(sym, SYMBOL_FONT, &mut runs);
        }
        // Any other format code contributes nothing.
    }
    runs
}

/// The character selected by a `\F<symbol font>;X` switch, or `None` when the
/// braces hold some other format code.
///
/// The font name may carry an extension and a run of `|`-separated parameters
/// (`\Fgdt.shx|b0|i0|c134|p6;j`), so match the name and skip to the `;` rather
/// than demanding the bare `\Fgdt;` form — the parameterised spelling is legal
/// and would otherwise drop the symbol silently.
fn symbol_font_switch(inner: &str) -> Option<char> {
    let rest = inner
        .strip_prefix("\\F")
        .or_else(|| inner.strip_prefix("\\f"))?;
    let (name, tail) = rest.split_once(';')?;
    let name = name.split('|').next()?.trim();
    let stem = name.strip_suffix(".shx").unwrap_or(name);
    if !stem.eq_ignore_ascii_case("gdt") {
        return None;
    }
    tail.chars().next()
}

/// Per-entity overrides of individual dimension-style variables, carried as
/// extended data under the `DSTYLE` application.
///
/// An entity may keep its style yet override single variables on itself, as
/// `(variable group code, value)` pairs. Reading the style but ignoring these
/// draws the frame at the style's size rather than its own — which is how this
/// one, overriding its text height to a fraction of the style's, came out far
/// too large.
///
/// Only the character height is read — it is the frame's single geometric
/// input (see `tessellate_tolerance`); every other variable is left to the style.
fn dstyle_overrides(tol: &Tolerance) -> Option<f64> {
    // The only variable the frame is built from.
    const DIMTXT: i16 = 140;

    use acadrust::xdata::XDataValue as V;

    // The record belongs to the shared "ACAD" application and names itself in
    // its FIRST STRING VALUE. There is no record called "DSTYLE" — asking for
    // one finds nothing and leaves every override silently unread.
    let rec = tol.common.extended_data.get_record("ACAD")?;
    let mut vals = rec.values.iter();
    match vals.next() {
        Some(V::String(s)) if s == "DSTYLE" => {}
        _ => return None,
    }

    let mut txt = None;
    let mut pending: Option<i16> = None;
    for v in vals {
        match v {
            V::Integer16(code) => pending = Some(*code),
            V::Real(value) | V::Distance(value) => {
                if pending.take() == Some(DIMTXT) {
                    txt = Some(*value);
                }
            }
            // Braces and anything else just delimit; a non-numeric value also
            // ends the pair we were waiting on.
            _ => pending = None,
        }
    }
    txt
}

/// The tolerance's dimension style — by handle first, then by name.
///
/// The order matters and is not interchangeable: a DWG records the style's
/// handle and leaves the name at its default, while a DXF records the name and
/// leaves the handle empty. Matching on the name alone — the shape used
/// elsewhere for entities that only ever carry one — would silently resolve
/// every DWG-read tolerance to "Standard" and pick the wrong metrics.
fn resolve_dim_style<'a>(
    tol: &Tolerance,
    doc: &'a acadrust::CadDocument,
) -> Option<&'a acadrust::tables::DimStyle> {
    if let Some(h) = tol.dimension_style_handle {
        if !h.is_null() {
            if let Some(s) = doc.dim_styles.iter().find(|s| s.handle == h) {
                return Some(s);
            }
        }
    }
    let name = tol.dimension_style_name.trim();
    if name.is_empty() {
        return None;
    }
    doc.dim_styles
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
}

// ── Feature-control frame builder ─────────────────────────────────────────────

/// The pen advance of `text` in `font` at `height`.
///
/// This is where the pen LANDS, so it carries the font's letter spacing past
/// the final glyph — which is what puts the gap between one run and the next.
fn run_advance(text: &str, font: &str, height: f32) -> f32 {
    crate::entities::text_support::text_local_bounds(font, text, height, 1.0, 0.0)
        .map(|b| b.advance)
        .unwrap_or(0.0)
}

/// The font's letter spacing at `height`, in world units.
///
/// Glyph geometry is authored against a 9-unit cap height, so a font's spacing
/// scales with the character height like everything else.
fn letter_spacing(font: &str, height: f32) -> f32 {
    crate::scene::text::font_face::Face::resolve(font).letter_spacing() * height / 9.0
}

/// How wide a compartment's content actually draws.
///
/// Not the pen advance: that stops one letter-space PAST the last glyph, since
/// its job is to place whatever comes next. Summing run advances therefore
/// over-measures by exactly one spacing — the gaps between runs are real, the
/// one hanging off the end is not — and every compartment came out that much
/// too wide.
fn content_width(cell: &Cell, height: f32) -> f32 {
    let Some(last) = cell.last() else {
        return 0.0;
    };
    let pen: f32 = cell
        .iter()
        .map(|r| run_advance(&r.text, r.font, height))
        .sum();
    (pen - letter_spacing(last.font, height)).max(0.0)
}

/// One text run of a feature-control frame, ready to become a `TextStroke`
/// with a `GlyphRun` so the run can render as SDF glyph quads (or, when
/// SDF is off, from `strokes`). `origin` is relative to the tolerance insertion
/// point (already rotated); `strokes` are the glyph polylines rotated about the
/// origin (no origin translation — the wire-builder adds the origin).
struct TolCell {
    text: String,
    font: &'static str,
    origin: [f32; 2],
    strokes: Vec<Vec<[f32; 2]>>,
    height: f32,
    rotation: f32,
}

/// Tessellate a Tolerance entity's feature-control frame.
///
/// Returns (`box_strokes`, `cells`):
///   - `box_strokes` — the outer border, row separators and column dividers as
///     2-D polylines (rotated; run-less, so they always render as strokes)
///   - `cells` — one [`TolCell`] per non-empty cell, carrying its text + a
///     `GlyphRun` so the cell renders as SDF text (frame stays geometry).
fn tessellate_tolerance(
    tol: &Tolerance,
    doc: &acadrust::CadDocument,
) -> (Vec<Vec<[f32; 2]>>, Vec<TolCell>) {
    if tol.text.is_empty() {
        return (vec![], vec![]);
    }

    let rows = parse_gdt_rows(&tol.text);
    if rows.is_empty() {
        return (vec![], vec![]);
    }

    // ── Metrics ──────────────────────────────────────────────────────────
    // Text height and gap come from the dimension style whenever one resolves.
    // The entity's own fields are not trustworthy: a DWG carries only the
    // style's handle and leaves these at their constructed defaults (0.18 /
    // 0.09), which are small but non-zero — so a "> 0" test happily uses them
    // and the frame draws about ten times too small. Falling back to them only
    // when no style resolves keeps DXF-read entities working.
    let style = resolve_dim_style(tol, doc);
    let scale = style
        .map(|s| if s.dimscale > 1e-6 { s.dimscale } else { 1.0 })
        .unwrap_or(1.0);
    // The entity's own overrides win over its style; the style wins over the
    // entity's constructed defaults.
    let h = dstyle_overrides(tol)
        .or(style.map(|s| s.dimtxt))
        .map(|v| v * scale)
        .unwrap_or(tol.text_height) as f32;
    let h = if h > 1e-6 { h } else { 2.5_f32 };

    // A compartment is a PROPORTION of the character height: it spans -h..+h
    // about the row's centreline, so it is exactly 2h tall and the h/2 margin
    // falls out of that rather than being an input.
    //
    // The gap variable is deliberately NOT read. Its scope is the dimension
    // line's break and the rectangle around a basic dimension — a different
    // entity's geometry, where it genuinely does apply twice per axis. Carrying
    // that shape over to this frame drew it at ~4x the character height. Even
    // fed a self-consistent style it can only ever produce 1.5h, never 2h, so
    // the formula was wrong in kind, not merely mis-fed.
    let cell_h = 2.0 * h;
    let pad = h * 0.5;
    let min_cell_w = 2.0 * h;

    // Cell widths per row, not a shared column grid: a row holding one
    // compartment is only as wide as that compartment, so a lone datum below a
    // four-compartment row draws a small box rather than inheriting the row
    // above it. Width comes from the real pen advance — a character count times
    // an average width mis-measures every symbol, and counting UTF-8 bytes
    // (`len()`) would make each one two or three cells wide.
    // A compartment's runs come from different fonts, so each is measured in its
    // own before they are summed.
    let cell_width = |cell: &Cell| -> f32 { (content_width(cell, h) + 2.0 * pad).max(min_cell_w) };
    let row_widths: Vec<Vec<f32>> = rows
        .iter()
        .map(|row| row.iter().map(|c| cell_width(c)).collect())
        .collect();

    // ── Transform helpers (local space — translation applied in tessellate.rs) ──
    let angle = (tol.direction.y as f32).atan2(tol.direction.x as f32);
    let (sa, ca) = angle.sin_cos();

    // Rotate only; origin is kept as f64 and applied later with full precision.
    let rot = |x: f32, y: f32| -> [f32; 2] { [x * ca - y * sa, x * sa + y * ca] };

    let mut box_out: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut cells: Vec<TolCell> = Vec::new();

    // The insertion point sits on the CENTRELINE OF THE FIRST ROW, and rows
    // stack downward from there: row `i` spans `-h - 2h*i` .. `+h - 2h*i`. So
    // row 0 straddles y = 0 and the frame grows downward only.
    //
    // Anchoring the frame's bottom-left at the insertion point instead — as
    // this did — offsets the whole frame upward by its own height, and by an
    // amount that changes with the number of rows, so a two-row frame and a
    // three-row one would sit at different heights from the same point.
    let row_bottom = |ri: usize| -h - cell_h * ri as f32;

    // ── Per-row border ────────────────────────────────────────────────────
    // One box per row, each left-aligned at x = 0 and only as wide as its own
    // compartments. Adjacent rows share the edge between them, so this draws
    // the row separators too.
    for (ri, widths) in row_widths.iter().enumerate() {
        let rw: f32 = widths.iter().sum();
        let y0 = row_bottom(ri);
        let y1 = y0 + cell_h;
        box_out.push(vec![
            rot(0.0, y0),
            rot(rw, y0),
            rot(rw, y1),
            rot(0.0, y1),
            rot(0.0, y0),
        ]);
    }

    // ── Compartment dividers, within each row ─────────────────────────────
    for (ri, widths) in row_widths.iter().enumerate() {
        let y0 = row_bottom(ri);
        let y1 = y0 + cell_h;
        let mut x_cursor = 0.0_f32;
        for w in widths.iter().take(widths.len().saturating_sub(1)) {
            x_cursor += w;
            box_out.push(vec![rot(x_cursor, y0), rot(x_cursor, y1)]);
        }
    }

    // ── Text content per cell ─────────────────────────────────────────────
    // Each cell becomes a TolCell: its origin is the (rotated) cell position
    // relative to the insertion point; its `strokes` are the glyph polylines
    // rotated about that origin (used only when SDF is off). The GlyphRun the
    // caller attaches lets the cell render as SDF text.
    for (ri, row) in rows.iter().enumerate() {
        // The compartment is 2h tall and the character h, so an h/2 margin
        // centres the text on the row's centreline.
        let row_y = row_bottom(ri) + pad;
        let mut cell_x = 0.0_f32;
        for (ci, cell) in row.iter().enumerate() {
            let cw = row_widths[ri][ci];
            if !cell.is_empty() {
                // Centre the compartment's whole content, then lay its runs out
                // left to right — each in its own font, each advancing the pen
                // by what that font actually measures.
                let mut run_x = cell_x + (cw - content_width(cell, h)) * 0.5;
                for run in cell {
                    let (text, font) = (run.text.clone(), run.font);
                    let (local_strokes, _) =
                        lff::tessellate_text_ex([0.0, 0.0], h, 0.0, 1.0, 0.0, font, &text);
                    // Glyph polylines rotated about the run's origin (no origin
                    // translation — the wire-builder adds `origin`).
                    let strokes: Vec<Vec<[f32; 2]>> = local_strokes
                        .into_iter()
                        .map(|pl| pl.into_iter().map(|[px, py]| rot(px, py)).collect())
                        .filter(|pl: &Vec<[f32; 2]>| !pl.is_empty())
                        .collect();
                    let advance = run_advance(&text, font, h);
                    cells.push(TolCell {
                        text,
                        font,
                        origin: rot(run_x, row_y),
                        strokes,
                        height: h,
                        rotation: angle,
                    });
                    run_x += advance;
                }
            }
            cell_x += cw;
        }
    }

    (box_out, cells)
}

// ── RenderConvertible ──────────────────────────────────────────────────────────

impl RenderConvertible for Tolerance {
    fn to_render(&self, document: &acadrust::CadDocument) -> Option<RenderEntity> {
        if self.text.is_empty() {
            return None;
        }

        let snap_pt = glam::DVec3::new(
            self.insertion_point.x,
            self.insertion_point.y,
            self.insertion_point.z,
        );

        // Build the feature-control frame in local space; origin stored as f64.
        let (box_strokes, cells) = tessellate_tolerance(self, document);
        let ins = [self.insertion_point.x, self.insertion_point.y];

        // Frame geometry first (run-less → always strokes; also the anchor
        // group so its origin = the insertion point), then one run-group per
        // text cell so the cell text renders as SDF glyphs (or strokes when
        // SDF is off).
        let mut groups: Vec<TextStroke> = Vec::with_capacity(1 + cells.len());
        if !box_strokes.is_empty() {
            groups.push(TextStroke {
                strokes: box_strokes,
                origin: ins,
                color: None,
                fill_tris: vec![],
                run: None,
            });
        }
        for cell in cells {
            groups.push(TextStroke {
                strokes: cell.strokes,
                origin: [
                    ins[0] + cell.origin[0] as f64,
                    ins[1] + cell.origin[1] as f64,
                ],
                color: None,
                fill_tris: vec![],
                run: Some(GlyphRun {
                    text: cell.text,
                    font: cell.font.to_string(),
                    height: cell.height,
                    rotation: cell.rotation,
                    width_factor: 1.0,
                    oblique: 0.0,
                    // `tracking` scales the font's own letter spacing, so 0
                    // collapses the gap between glyphs and the characters run
                    // together. Every other text-bearing entity passes 1.0, and
                    // so do both the stroke path (`tessellate_text_ex`) and the
                    // width measurement above — leaving this at 0 made the cells
                    // measure wider than the text they drew.
                    tracking: 1.0,
                    bold: false,
                }),
            });
        }
        if groups.is_empty() {
            return None;
        }

        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Text(groups),
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
            glam::DVec3::new(
                self.insertion_point.x,
                self.insertion_point.y,
                self.insertion_point.z,
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
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        vec![
            PropSection {
                title: t!("Text").into_owned(),
                props: vec![
                    ro(t!("Text style").as_ref(), "tol_text_style", String::new()),
                    edit(t!("Text height").as_ref(), "tol_text_height", self.text_height),
                ],
            },
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    edit(t!("Position X").as_ref(), "tol_ix", self.insertion_point.x),
                    edit(t!("Position Y").as_ref(), "tol_iy", self.insertion_point.y),
                    edit(t!("Position Z").as_ref(), "tol_iz", self.insertion_point.z),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    ro(
                        t!("Dimension style").as_ref(),
                        "tol_dim_style",
                        if self.dimension_style_name.is_empty() {
                            "(default)".to_string()
                        } else {
                            self.dimension_style_name.clone()
                        },
                    ),
                    edit(t!("Direction X").as_ref(), "tol_dir_x", self.direction.x),
                    edit(t!("Direction Y").as_ref(), "tol_dir_y", self.direction.y),
                    edit(t!("Direction Z").as_ref(), "tol_dir_z", self.direction.z),
                ],
            },
        ]
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
            "tol_dir_x" => self.direction.x = v,
            "tol_dir_y" => self.direction.y = v,
            "tol_dir_z" => self.direction.z = v,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The benchmark's own string, byte for byte, in the form a DWG carries it
    /// (a raw newline). This one assertion is the whole reported bug: it used to
    /// collapse to a single row of ASCII soup.
    const BENCH: &str =
        "{\\Fgdt;r}%%v{\\Fgdt;n}tol{\\Fgdt;m}%%v{\\Fgdt;n}tol{\\Fgdt;s}%%v1{\\Fgdt;m}%%v%%v\n2{\\Fgdt;p}\nA";

    #[test]
    fn the_benchmark_frame_parses_to_four_one_one() {
        let rows = parse_gdt_rows(BENCH);
        // Rendered as "<font>:<text>" per run so a wrong font fails loudly.
        let shown: Vec<Vec<String>> = rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        cell.iter()
                            .map(|r| format!("{}:{}", r.font, r.text))
                            .collect::<Vec<_>>()
                            .join("|")
                    })
                    .collect()
            })
            .collect();
        assert_eq!(
            shown,
            vec![
                vec![
                    "gdt:r".to_string(),
                    "gdt:n|txt:tol|gdt:m".to_string(),
                    "gdt:n|txt:tol|gdt:s".to_string(),
                    "txt:1|gdt:m".to_string(),
                ],
                vec!["txt:2|gdt:p".to_string()],
                vec!["txt:A".to_string()],
            ],
            "the reference draws [◎][⌀tolⓂ][⌀tolⓈ][1Ⓜ] / [2Ⓟ] / [A], each symbol \
             from the font its escape names"
        );
    }

    /// A row break reaches us in four encodings depending on the format and who
    /// wrote it. All four must land on the same frame.
    #[test]
    fn every_row_break_encoding_gives_the_same_rows() {
        let want = parse_gdt_rows(BENCH);
        assert_eq!(want.len(), 3, "guard: the baseline itself must be 3 rows");
        for (label, variant) in [
            ("^J", BENCH.replace('\n', "^J")),
            ("\\P", BENCH.replace('\n', "\\P")),
            ("CRLF", BENCH.replace('\n', "\r\n")),
            ("CR", BENCH.replace('\n', "\r")),
        ] {
            assert_eq!(parse_gdt_rows(&variant), want, "{label} parsed differently");
        }
    }

    /// Spare compartments at the end of a row are unused slots, not empty boxes
    /// — the benchmark's first row ends `%%v%%v` yet draws four.
    #[test]
    fn trailing_empty_compartments_are_dropped_interior_ones_kept() {
        let shown = |raw: &str| -> Vec<Vec<String>> {
            parse_gdt_rows(raw)
                .iter()
                .map(|row| row.iter().map(cell_text).collect())
                .collect()
        };
        assert_eq!(shown("a%%vb%%v%%v"), vec![vec!["a", "b"]]);
        assert_eq!(shown("a%%v%%vc"), vec![vec!["a", "", "c"]]);
    }

    /// The font switch legally carries an extension and parameters; the bare
    /// form is only the common spelling.
    #[test]
    fn a_parameterised_font_switch_still_yields_its_symbol() {
        assert_eq!(symbol_font_switch("\\Fgdt;j"), Some('j'));
        assert_eq!(symbol_font_switch("\\Fgdt.shx|b0|i0|c134|p6;j"), Some('j'));
        assert_eq!(symbol_font_switch("\\FGDT;j"), Some('j'));
        // Not the symbol font — leave it to the generic format-code stripper.
        assert_eq!(symbol_font_switch("\\Farial;j"), None);
        assert_eq!(symbol_font_switch("\\C1"), None);
    }

    /// The frame must take its size from the dimension style. A DWG carries
    /// only the style's handle and leaves the entity's own height at a small
    /// non-zero default, so trusting that field drew the frame ~10x too small.
    #[test]
    fn the_frame_is_sized_from_the_dimension_style_not_the_entity_default() {
        use acadrust::tables::DimStyle;
        let mut doc = acadrust::CadDocument::new();
        let handle = acadrust::Handle::from(0x27_u64);
        let mut style = DimStyle::new("ISO-25");
        style.handle = handle;
        style.dimtxt = 2.5;
        style.dimgap = 0.625;
        style.dimscale = 1.0;
        doc.dim_styles.add_or_replace(style);

        let mut tol = Tolerance::new();
        tol.text = "A".into();
        // What a DWG gives us: the handle set, the metrics left at defaults.
        tol.dimension_style_handle = Some(handle);
        assert!(
            tol.text_height < 1.0,
            "guard: the default height must stay small for this test to mean anything"
        );

        let (boxes, _) = tessellate_tolerance(&tol, &doc);
        // 2 x dimtxt. The entity's own default (0.18) would give 0.36.
        let height = frame_height(&boxes);
        assert!(
            (height - 5.0).abs() < 1e-3,
            "frame is {height} tall; 2 x the style's 2.5 character height is 5.0"
        );
    }

    /// An entity that overrides a style variable on itself wins over the style.
    /// The benchmark's tolerance keeps ISO-25 but overrides its text height to
    /// 0.42; reading the style alone drew the frame several times too large.
    #[test]
    fn an_entity_override_beats_its_dimension_style() {
        use acadrust::tables::DimStyle;
        use acadrust::xdata::XDataValue;

        let mut doc = acadrust::CadDocument::new();
        let handle = acadrust::Handle::from(0x27_u64);
        let mut style = DimStyle::new("ISO-25");
        style.handle = handle;
        style.dimtxt = 2.5;
        style.dimgap = 0.625;
        style.dimscale = 1.0;
        doc.dim_styles.add_or_replace(style);

        let mut tol = Tolerance::new();
        tol.text = "A".into();
        tol.dimension_style_handle = Some(handle);

        // Without the override: 2 x the style's 2.5 character height.
        let (boxes, _) = tessellate_tolerance(&tol, &doc);
        assert!(
            (frame_height(&boxes) - 5.0).abs() < 1e-3,
            "guard: style-sized, got {}",
            frame_height(&boxes)
        );

        // The entity's own overrides, shaped exactly as the file carries them:
        // application "ACAD", naming itself "DSTYLE" in its first string value,
        // then (variable code, value) pairs. Variable 140 = text height.
        let mut rec = acadrust::xdata::ExtendedDataRecord::new("ACAD");
        rec.add_value(XDataValue::String("DSTYLE".into()));
        rec.add_value(XDataValue::ControlString("{".into()));
        rec.add_value(XDataValue::Integer16(140));
        rec.add_value(XDataValue::Real(0.42));
        rec.add_value(XDataValue::ControlString("}".into()));
        tol.common.extended_data.add_record(rec);

        let (boxes, _) = tessellate_tolerance(&tol, &doc);
        // 2 x 0.42 — the frame follows the entity's own character height.
        assert!(
            (frame_height(&boxes) - 0.84).abs() < 1e-3,
            "override ignored: frame is {} tall, expected 0.84",
            frame_height(&boxes)
        );
    }

    fn frame_height(boxes: &[Vec<[f32; 2]>]) -> f32 {
        let ys: Vec<f32> = boxes.iter().flatten().map(|p| p[1]).collect();
        ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min)
    }

    /// Rows read downward and start at the left edge.
    #[test]
    fn rows_stack_downward_and_left_align() {
        let doc = acadrust::CadDocument::new();
        let mut tol = Tolerance::new();
        tol.text = BENCH.to_string();
        let (boxes, _) = tessellate_tolerance(&tol, &doc);

        // One closed rect per row (plus dividers); every rect starts at x = 0.
        let rects: Vec<&Vec<[f32; 2]>> = boxes.iter().filter(|b| b.len() == 5).collect();
        assert_eq!(rects.len(), 3, "one border per row");
        for r in &rects {
            let x0 = r.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
            assert!(x0.abs() < 1e-3, "row border not left-aligned: x0={x0}");
        }
        // The insertion point is the first row's centreline: row 0 straddles
        // y = 0 and the frame only ever grows downward from it, so the anchor
        // does not drift with the number of rows.
        let h = tol.text_height as f32;
        let top = rects
            .iter()
            .flat_map(|r| r.iter())
            .map(|p| p[1])
            .fold(f32::MIN, f32::max);
        let bottom = rects
            .iter()
            .flat_map(|r| r.iter())
            .map(|p| p[1])
            .fold(f32::MAX, f32::min);
        assert!(
            (top - h).abs() < 1e-4,
            "frame should reach +h ({h}) above the insertion point, reaches {top}"
        );
        // 3 rows of 2h, the first centred on 0 -> the last bottom is -5h.
        assert!(
            (bottom - (-5.0 * h)).abs() < 1e-4,
            "three rows should reach -5h ({}) below, reach {bottom}",
            -5.0 * h
        );

        // Row 0 (4 compartments) is the widest AND the topmost.
        let top = |r: &Vec<[f32; 2]>| r.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        let wide = |r: &Vec<[f32; 2]>| r.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
        assert!(
            top(rects[0]) > top(rects[1]) && top(rects[1]) > top(rects[2]),
            "rows must stack downward, row 0 on top"
        );
        assert!(
            wide(rects[0]) > wide(rects[1]),
            "the lone datum row must not inherit the four-compartment row's width"
        );
    }

    /// The cell is measured with the font's normal letter spacing, so it must be
    /// DRAWN with it too. They diverged once — the measurement assumed normal
    /// spacing while the glyph run asked for none — and the characters ran
    /// together inside correctly-sized boxes.
    #[test]
    fn cells_are_drawn_with_the_spacing_they_were_measured_with() {
        use acadrust::EntityType;
        let doc = acadrust::CadDocument::new();
        let mut tol = Tolerance::new();
        tol.text = "ABC".into();

        let Some(RenderEntity {
            object: RenderObject::Text(groups),
            ..
        }) = tol.to_render(&doc)
        else {
            panic!("tolerance produced no text");
        };
        // The frame is the run-less group; every cell carries a run.
        let runs: Vec<_> = groups.iter().filter_map(|g| g.run.as_ref()).collect();
        assert!(!runs.is_empty(), "no cell runs emitted");
        for r in &runs {
            assert_eq!(
                r.tracking, 1.0,
                "cell text must use the font's normal letter spacing, like every \
                 other entity and like the width measurement"
            );
        }
    }

    /// The symbol font is reached by NAME, through the same lookup every other
    /// font goes through — so a switch to it works wherever inline codes do, not
    /// only inside a tolerance. An MTEXT carrying `{\\Fgdt;j}`, or a text style
    /// literally named "gdt.shx", lands on it too, with no code of their own.
    #[test]
    fn the_symbol_font_is_reached_by_name_like_any_other() {
        use crate::scene::text::font_face::Face;
        for name in ["gdt", "GDT", "gdt.shx"] {
            assert!(
                crate::scene::text::lff::is_builtin(name),
                "{name:?} must resolve to the bundled symbol font"
            );
            let face = Face::resolve(name);
            let Face::Lff(font) = &face else {
                panic!("{name:?} resolved to a system outline, not the stroke font");
            };
            assert_eq!(
                font.name.to_ascii_lowercase(),
                "gdt",
                "{name:?} landed on {}",
                font.name
            );
            // Every character the escape can carry draws from it.
            for c in 'a'..='z' {
                assert!(font.glyph(c).is_some(), "{name:?} has no glyph for {c:?}");
            }
        }
    }

    /// It holds symbols, not letters: the glyph for 'm' is the circled M, and
    /// must not be the letter m the text fonts draw.
    #[test]
    fn a_letter_in_the_symbol_font_is_a_symbol() {
        use crate::scene::text::font_face::Face;
        let gdt = Face::resolve(SYMBOL_FONT);
        let txt = Face::resolve(TEXT_FONT);
        let mut differ = 0;
        for c in 'a'..='z' {
            let (g, x) = (gdt.glyph(c), txt.glyph(c));
            if let (Some(g), Some(x)) = (g, x) {
                if g.strokes.len() != x.strokes.len() || (g.advance - x.advance).abs() > 1e-3 {
                    differ += 1;
                }
            }
        }
        assert!(
            differ >= 20,
            "only {differ} of 26 differ from the text font — the symbol font is \
             drawing letters"
        );
    }

    /// A compartment is measured by what it draws, not by where the pen lands.
    /// The pen stops one letter-space past the last glyph — that gap belongs to
    /// whatever comes next, and counting it made every compartment that much
    /// too wide.
    #[test]
    fn compartments_are_measured_by_their_ink_not_the_trailing_pen_gap() {
        // Cap height, so glyph units and world units line up.
        let h = 9.0_f32;
        for src in ["{\\Fgdt;r}", "{\\Fgdt;n}tol{\\Fgdt;m}", "1{\\Fgdt;m}", "A"] {
            let cell = parse_cell(src);
            let pen: f32 = cell.iter().map(|r| run_advance(&r.text, r.font, h)).sum();
            let ink: f32 = cell
                .iter()
                .map(|r| {
                    crate::entities::text_support::text_local_bounds(r.font, &r.text, h, 1.0, 0.0)
                        .map(|b| b.ink_max[0] - b.ink_min[0])
                        .unwrap_or(0.0)
                })
                .sum();
            let got = content_width(&cell, h);

            // Exactly one trailing gap comes off — the gaps BETWEEN runs are
            // real spacing and must stay.
            let spacing = letter_spacing(cell.last().unwrap().font, h);
            assert!(
                (got - (pen - spacing)).abs() < 1e-3,
                "{src:?}: content {got} should be pen {pen} less one {spacing} gap"
            );
            let expected = ink + (cell.len() as f32 - 1.0) * spacing;
            assert!(
                (got - expected).abs() < 1e-3,
                "{src:?}: content {got} should be ink {ink} plus {} inter-run gaps",
                cell.len() - 1
            );
        }
    }
}
