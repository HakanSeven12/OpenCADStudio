//! The selection as SVG for the system clipboard, so other programs paste
//! it as editable paths: each object's outline as a stroked path in its
//! colour, one drawing unit to one millimetre, Y pointing up as drawn.

use std::fmt::Write;

use crate::scene::{Scene, WireModel};
use document::Handle;

fn point(wire: &WireModel, index: usize) -> [f64; 2] {
    let high = wire.points[index];
    let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
    [
        f64::from(high[0]) + f64::from(low[0]),
        f64::from(high[1]) + f64::from(low[1]),
    ]
}

/// The outlines of `handles` as an SVG document, `None` when they draw
/// nothing.
pub fn selection_svg(scene: &Scene, handles: &[Handle]) -> Option<String> {
    let wanted: rustc_hash::FxHashSet<Handle> = handles.iter().copied().collect();
    let all = scene.entity_wires();
    let wires: Vec<&WireModel> = all
        .iter()
        .filter(|wire| Scene::handle_from_wire_name(&wire.name).is_some_and(|h| wanted.contains(&h)))
        .collect();
    wires_svg(&wires)
}

pub fn wires_svg(wires: &[&WireModel]) -> Option<String> {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    let mut body = String::new();
    for wire in wires {
        let mut path = String::new();
        let mut pen_down = false;
        for index in 0..wire.points.len() {
            let [x, y] = point(wire, index);
            if !x.is_finite() || !y.is_finite() {
                pen_down = false;
                continue;
            }
            min = [min[0].min(x), min[1].min(y)];
            max = [max[0].max(x), max[1].max(y)];
            let _ = write!(path, "{}{} {} ", if pen_down { "L" } else { "M" }, x, -y);
            pen_down = true;
        }
        if path.is_empty() {
            continue;
        }
        // Drawn light on the dark canvas, white reads as black on paper.
        let [r, g, b, _] = wire.color;
        let (r, g, b) = if r > 0.9 && g > 0.9 && b > 0.9 { (0.0, 0.0, 0.0) } else { (r, g, b) };
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let _ = writeln!(
            body,
            r##"<path d="{}" stroke="#{:02x}{:02x}{:02x}"/>"##,
            path.trim_end(),
            byte(r),
            byte(g),
            byte(b)
        );
    }
    if body.is_empty() {
        return None;
    }
    let (width, height) = ((max[0] - min[0]).max(1e-6), (max[1] - min[1]).max(1e-6));
    Some(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}mm\" height=\"{height}mm\" viewBox=\"{} {} {width} {height}\">\n<g fill=\"none\" stroke-width=\"1\" stroke-linecap=\"round\" stroke-linejoin=\"round\" vector-effect=\"non-scaling-stroke\" style=\"vector-effect:non-scaling-stroke\">\n{body}</g>\n</svg>\n",
        min[0], -max[1],
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
        assert!(svg.contains(r#"width="10mm" height="5mm" viewBox="0 -5 10 5""#), "{svg}");
        assert!(svg.contains(r##"<path d="M0 -0 L10 -5" stroke="#ff0000"/>"##), "{svg}");
        // And reads back as geometry.
        assert!(crate::io::svg_import::read(svg.as_bytes(), [100.0, 100.0]).is_ok());
    }
}
