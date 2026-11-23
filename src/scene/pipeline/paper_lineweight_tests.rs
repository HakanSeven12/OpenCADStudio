//! Lineweights thinner than a screen pixel keep their own width on the GPU.
//!
//! In a layout the shaders multiply the baked half-width by the sheet's
//! pixels per millimetre, so a 0.05 mm line drawn on paper zoomed in has to
//! stay a fifth of a 0.25 mm one. The shaders already keep every line at least
//! half a pixel wide after that scale (and in model space they draw anything
//! under a pixel as one pixel), so the half-width handed to them is the
//! lineweight's own (#3470).

use crate::scene::model::wire_model::TangentGeom;
use crate::scene::pipeline::circle_gpu::extract_circle_instance;
use crate::scene::pipeline::ellipse_gpu::extract_ellipse_instance;
use crate::scene::pipeline::wire_gpu::{emit_wire_native, emit_wire_packed};
use crate::scene::WireModel;
use document::types::LineWeight;

/// The lineweights of #3470, in hundredths of a millimetre.
const WEIGHTS: [i16; 3] = [5, 15, 25];

fn px(hundredths: i16) -> f32 {
    crate::scene::view::render::lineweight_to_px(&LineWeight::Value(hundredths))
}

fn line(hundredths: i16) -> WireModel {
    let mut wire = WireModel::solid(
        "1".into(),
        vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
        [1.0; 4],
        false,
    );
    wire.line_weight_px = px(hundredths);
    wire
}

fn assert_own_half_widths(what: &str, half_widths: [f32; 3]) {
    for (hundredths, half_width) in WEIGHTS.into_iter().zip(half_widths) {
        let expected = px(hundredths) * 0.5;
        assert!(
            (half_width - expected).abs() < 1e-6,
            "{what}: {:.2} mm has half-width {half_width} px, expected {expected} px; \
             half-widths for 0.05 / 0.15 / 0.25 mm: {half_widths:?}",
            hundredths as f32 / 100.0,
        );
    }
}

#[test]
fn packed_line_segments_keep_lineweights_under_a_pixel() {
    let half_widths = WEIGHTS.map(|w| emit_wire_packed(&line(w), [1.0; 4], 0.0)[0].half_width);
    assert_own_half_widths("packed line", half_widths);
}

#[test]
fn storage_line_constants_keep_lineweights_under_a_pixel() {
    let half_widths = WEIGHTS.map(|w| emit_wire_native(&line(w), 0, [1.0; 4], 0.0).1.half_width);
    assert_own_half_widths("storage line", half_widths);
}

#[test]
fn circles_keep_lineweights_under_a_pixel() {
    let half_widths = WEIGHTS.map(|w| {
        let mut wire = WireModel::default();
        wire.tangent_geoms.push(TangentGeom::PlanarCircle {
            center: [0.0, 0.0, 0.0],
            axis_x: [1.0, 0.0, 0.0],
            axis_y: [0.0, 1.0, 0.0],
            radius: 5.0,
        });
        wire.line_weight_px = px(w);
        extract_circle_instance(&wire, 0.0).expect("circle instance").params[0]
    });
    assert_own_half_widths("circle", half_widths);
}

#[test]
fn ellipses_keep_lineweights_under_a_pixel() {
    let half_widths = WEIGHTS.map(|w| {
        let mut wire = WireModel::default();
        wire.tangent_geoms.push(TangentGeom::PlanarEllipse {
            center: [0.0, 0.0, 0.0],
            major_axis: [5.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            minor_axis_ratio: 0.5,
            start_param: 0.0,
            end_param: std::f64::consts::TAU,
        });
        wire.line_weight_px = px(w);
        extract_ellipse_instance(&wire, 0.0).expect("ellipse instance").params[0]
    });
    assert_own_half_widths("ellipse", half_widths);
}
