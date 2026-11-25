//! The selection as SVG for the system clipboard, so other programs paste
//! it as editable paths: each object's outline as a stroked path in its
//! colour, one drawing unit to one millimetre, Y pointing up as drawn.
//!
//! The coordinates are written in CSS pixels, SVG's own user unit, with the
//! size in millimetres: drawing programs take the user unit as a pixel when
//! they paste, and read the size from `width` and `height` otherwise. Both
//! give one drawing unit as one millimetre.

use std::fmt::Write;

use crate::scene::{Scene, WireModel};
use document::Handle;

/// CSS pixels (1/96 in) in a millimetre: SVG user units per drawing unit.
const PX_PER_MM: f64 = 96.0 / 25.4;

fn point(wire: &WireModel, index: usize) -> [f64; 2] {
    let high = wire.points[index];
    let low = wire.points_low.get(index).unwrap_or([0.0; 3]);
    [
        f64::from(high[0]) + f64::from(low[0]),
        f64::from(high[1]) + f64::from(low[1]),
    ]
}

/// One object's outline for the clipboard: its pen-down runs in drawing
/// units, and its colour as drawn on paper.
pub struct Stroke {
    pub runs: Vec<Vec<[f64; 2]>>,
    pub rgb: [u8; 3],
}

/// The outlines of `handles`, empty when they draw nothing.
pub fn selection_strokes(scene: &Scene, handles: &[Handle]) -> Vec<Stroke> {
    let wanted: rustc_hash::FxHashSet<Handle> = handles.iter().copied().collect();
    let all = scene.entity_wires();
    let wires: Vec<&WireModel> = all
        .iter()
        .filter(|wire| {
            Scene::handle_from_wire_name(&wire.name).is_some_and(|h| wanted.contains(&h))
        })
        .collect();
    strokes(&wires)
}

/// The outlines of `handles` as an SVG document, `None` when they draw
/// nothing.
pub fn selection_svg(scene: &Scene, handles: &[Handle]) -> Option<String> {
    strokes_svg(&selection_strokes(scene, handles))
}

pub fn wires_svg(wires: &[&WireModel]) -> Option<String> {
    strokes_svg(&strokes(wires))
}

fn strokes(wires: &[&WireModel]) -> Vec<Stroke> {
    let mut out = Vec::new();
    for wire in wires {
        let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
        let mut pen_down = false;
        for index in 0..wire.points.len() {
            let [x, y] = point(wire, index);
            if !x.is_finite() || !y.is_finite() {
                pen_down = false;
                continue;
            }
            if !pen_down {
                runs.push(Vec::new());
            }
            if let Some(run) = runs.last_mut() {
                run.push([x, y]);
            }
            pen_down = true;
        }
        if runs.is_empty() {
            continue;
        }
        // Drawn light on the dark canvas, white reads as black on paper.
        let [r, g, b, _] = wire.color;
        let (r, g, b) = if r > 0.9 && g > 0.9 && b > 0.9 {
            (0.0, 0.0, 0.0)
        } else {
            (r, g, b)
        };
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        out.push(Stroke {
            runs,
            rgb: [byte(r), byte(g), byte(b)],
        });
    }
    out
}

/// The bounding box of `strokes`, `None` when they hold no point.
pub fn strokes_extent(strokes: &[Stroke]) -> Option<([f64; 2], [f64; 2])> {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for [x, y] in strokes.iter().flat_map(|s| s.runs.iter().flatten()) {
        min = [min[0].min(*x), min[1].min(*y)];
        max = [max[0].max(*x), max[1].max(*y)];
    }
    min[0].is_finite().then_some((min, max))
}

pub fn strokes_svg(strokes: &[Stroke]) -> Option<String> {
    let (min, max) = strokes_extent(strokes)?;
    let mut body = String::new();
    for stroke in strokes {
        let mut path = String::new();
        for run in &stroke.runs {
            for (k, [x, y]) in run.iter().enumerate() {
                let _ = write!(
                    path,
                    "{}{} {} ",
                    if k == 0 { "M" } else { "L" },
                    x * PX_PER_MM,
                    -y * PX_PER_MM
                );
            }
        }
        let [r, g, b] = stroke.rgb;
        let _ = writeln!(
            body,
            r##"<path d="{}" stroke="#{r:02x}{g:02x}{b:02x}"/>"##,
            path.trim_end(),
        );
    }
    if body.is_empty() {
        return None;
    }
    let (width, height) = ((max[0] - min[0]).max(1e-6), (max[1] - min[1]).max(1e-6));
    Some(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}mm\" height=\"{height}mm\" viewBox=\"{} {} {} {}\">\n<g fill=\"none\" stroke-width=\"1\" stroke-linecap=\"round\" stroke-linejoin=\"round\" vector-effect=\"non-scaling-stroke\" style=\"vector-effect:non-scaling-stroke\">\n{body}</g>\n</svg>\n",
        min[0] * PX_PER_MM,
        -max[1] * PX_PER_MM,
        width * PX_PER_MM,
        height * PX_PER_MM,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_becomes_a_path_in_millimetres_with_y_up() {
        let mut wire = WireModel::default();
        wire.points = vec![[0.0, 0.0, 0.0], [10.0, 5.0, 0.0]];
        wire.color = [1.0, 0.0, 0.0, 1.0];
        let svg = wires_svg(&[&wire]).unwrap();
        // 10 × 5 mm; the coordinates in CSS pixels, 96 to the inch.
        let px = |mm: f64| mm * (96.0 / 25.4);
        assert!(
            svg.contains(&format!(
                r#"width="10mm" height="5mm" viewBox="0 {} {} {}""#,
                -px(5.0),
                px(10.0),
                px(5.0)
            )),
            "{svg}"
        );
        assert!(
            svg.contains(&format!(
                r##"<path d="M0 -0 L{} {}" stroke="#ff0000"/>"##,
                px(10.0),
                -px(5.0)
            )),
            "{svg}"
        );
        // And reads back as geometry.
        assert!(crate::io::svg_import::read(svg.as_bytes(), [100.0, 100.0]).is_ok());
    }

    /// The size an SVG reader gets when it takes the user unit as a CSS
    /// pixel (1/96 in), as SVG does by default and as drawing programs do
    /// when they paste: `width` × `height` in millimetres.
    fn size_at_one_unit_per_px(svg: &str) -> [f64; 2] {
        let attribute = |name: &str| {
            let start = svg.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
            let end = start + svg[start..].find('"').unwrap();
            svg[start..end].to_owned()
        };
        let view_box: Vec<f64> = attribute("viewBox")
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        let mm_per_px = 25.4 / 96.0;
        [view_box[2] * mm_per_px, view_box[3] * mm_per_px]
    }

    #[test]
    fn a_reader_taking_svg_units_as_pixels_gets_the_size_in_millimetres() {
        let mut wire = WireModel::default();
        wire.points = vec![
            [0.0, 0.0, 0.0],
            [60.0, 0.0, 0.0],
            [60.0, 40.0, 0.0],
            [0.0, 40.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        wire.color = [0.0, 0.0, 1.0, 1.0];
        let svg = wires_svg(&[&wire]).unwrap();
        assert!(svg.contains(r#"width="60mm" height="40mm""#), "{svg}");
        let [width, height] = size_at_one_unit_per_px(&svg);
        assert!(
            (width - 60.0).abs() < 1e-6 && (height - 40.0).abs() < 1e-6,
            "a 60 × 40 mm drawing reads as {width:.3} × {height:.3} mm"
        );
    }
}
