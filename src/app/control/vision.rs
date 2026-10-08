use crate::app::OpenCADStudio;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 5x7 bitmapped font glyphs for digits '0' through '9'
const DIGITS_5X7: [[u8; 7]; 10] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110], // 0
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110], // 1
    [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111], // 2
    [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110], // 3
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010], // 4
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110], // 5
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110], // 6
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000], // 7
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110], // 8
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100], // 9
];

/// Draw a Set-of-Marks badge at `(center_x, center_y)` onto `image`.
pub(crate) fn draw_som_badge(
    image: &mut image::RgbaImage,
    center_x: i32,
    center_y: i32,
    tag: usize,
    selected: bool,
) {
    let tag_str = tag.to_string();
    let num_digits = tag_str.len();
    if num_digits == 0 {
        return;
    }
    let text_w = num_digits * 5 + (num_digits - 1) * 1;
    let badge_w = (text_w + 8) as i32;
    let badge_h = 13i32;

    let img_w = image.width() as i32;
    let img_h = image.height() as i32;

    // Anchor badge slightly above the center anchor point if possible
    let x0 = (center_x - badge_w / 2).clamp(1, (img_w - badge_w - 1).max(1));
    let y0 = (center_y - badge_h - 2).clamp(1, (img_h - badge_h - 1).max(1));

    let (bg, border, text_col) = if selected {
        (
            image::Rgba([0, 230, 255, 245]), // Electric cyan
            image::Rgba([0, 60, 80, 255]),
            image::Rgba([0, 0, 0, 255]),
        )
    } else {
        (
            image::Rgba([255, 220, 0, 245]), // Vibrant yellow
            image::Rgba([40, 30, 0, 255]),
            image::Rgba([0, 0, 0, 255]),
        )
    };

    // Fill badge background and border
    for y in y0..(y0 + badge_h) {
        for x in x0..(x0 + badge_w) {
            if x < 0 || x >= img_w || y < 0 || y >= img_h {
                continue;
            }
            let is_border = x == x0 || x == x0 + badge_w - 1 || y == y0 || y == y0 + badge_h - 1;
            let col = if is_border { border } else { bg };
            image.put_pixel(x as u32, y as u32, col);
        }
    }

    // Draw digits
    let mut cur_x = x0 + 4;
    let cur_y = y0 + 3;
    for ch in tag_str.chars() {
        if let Some(digit) = ch.to_digit(10) {
            let d = digit as usize;
            if d < 10 {
                for r in 0..7 {
                    let row_bits = DIGITS_5X7[d][r];
                    for c in 0..5 {
                        if (row_bits & (1 << (4 - c))) != 0 {
                            let px = cur_x + c as i32;
                            let py = cur_y + r as i32;
                            if px >= 0 && px < img_w && py >= 0 && py < img_h {
                                image.put_pixel(px as u32, py as u32, text_col);
                            }
                        }
                    }
                }
            }
        }
        cur_x += 6;
    }

    // Small 3x3 indicator dot at the center anchor
    let dot_col = if selected {
        image::Rgba([0, 230, 255, 255])
    } else {
        image::Rgba([255, 60, 0, 255])
    };
    for dy in -1..=1 {
        for dx in -1..=1 {
            let px = center_x + dx;
            let py = center_y + dy;
            if px >= 0 && px < img_w && py >= 0 && py < img_h {
                image.put_pixel(px as u32, py as u32, dot_col);
            }
        }
    }
}

pub(crate) struct VisionGrounding {
    pub viewport_world_bounds: Value,
    pub camera: Value,
    pub target_plane: Value,
    pub pixel_to_world_matrix: [f64; 6],
    pub world_to_pixel_matrix: [f64; 6],
    pub visible_entities: Vec<Value>,
}

pub(crate) fn compute_grounding(
    app: &OpenCADStudio,
    vp_logical_width: f32,
    vp_logical_height: f32,
    image_width: u32,
    image_height: u32,
) -> VisionGrounding {
    let tab = &app.tabs[app.active_tab];
    let c = tab.scene.camera.borrow();
    let vp_rect = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: vp_logical_width.max(1.0),
        height: vp_logical_height.max(1.0),
    };

    let p_tl = c.pick_on_target_plane(iced::Point::new(0.0, 0.0), vp_rect);
    let p_tr = c.pick_on_target_plane(iced::Point::new(vp_rect.width, 0.0), vp_rect);
    let p_bl = c.pick_on_target_plane(iced::Point::new(0.0, vp_rect.height), vp_rect);
    let p_br = c.pick_on_target_plane(iced::Point::new(vp_rect.width, vp_rect.height), vp_rect);

    let min_x = p_tl.x.min(p_tr.x).min(p_bl.x).min(p_br.x);
    let max_x = p_tl.x.max(p_tr.x).max(p_bl.x).max(p_br.x);
    let min_y = p_tl.y.min(p_tr.y).min(p_bl.y).min(p_br.y);
    let max_y = p_tl.y.max(p_tr.y).max(p_bl.y).max(p_br.y);

    let viewport_world_bounds = json!({
        "min": [min_x, min_y],
        "max": [max_x, max_y]
    });

    let eye = c.eye();
    let target = c.target;

    let normal = (c.rotation * glam::Vec3::NEG_Z).normalize_or(glam::Vec3::Z);
    let target_plane = json!({
        "normal": [normal.x, normal.y, normal.z],
        "origin": [target.x, target.y, target.z],
    });

    let proj_name = match c.projection {
        crate::scene::view::camera::Projection::Orthographic => "Orthographic",
        crate::scene::view::camera::Projection::Perspective => "Perspective",
    };
    let camera_info = json!({
        "eye": [eye.x, eye.y, eye.z],
        "target": [target.x, target.y, target.z],
        "distance": c.distance,
        "ortho_size": c.ortho_size(),
        "projection": proj_name,
        "fov_y": c.fov_y,
    });

    let p_origin = p_tl;
    let p_right = p_tr;
    let p_bottom = p_bl;

    let d_x = (p_right - p_origin) / (image_width as f64).max(1.0);
    let d_y = (p_bottom - p_origin) / (image_height as f64).max(1.0);

    let m00 = d_x.x;
    let m01 = d_y.x;
    let tx = p_origin.x;
    let m10 = d_x.y;
    let m11 = d_y.y;
    let ty = p_origin.y;

    let det = m00 * m11 - m01 * m10;
    let (inv_m00, inv_m01, inv_m10, inv_m11, inv_tx, inv_ty) = if det.abs() > 1e-12 {
        let inv_m00 = m11 / det;
        let inv_m01 = -m01 / det;
        let inv_m10 = -m10 / det;
        let inv_m11 = m00 / det;
        let inv_tx = -(inv_m00 * tx + inv_m01 * ty);
        let inv_ty = -(inv_m10 * tx + inv_m11 * ty);
        (inv_m00, inv_m01, inv_m10, inv_m11, inv_tx, inv_ty)
    } else {
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
    };

    let pixel_to_world_matrix = [m00, m01, m10, m11, tx, ty];
    let world_to_pixel_matrix = [inv_m00, inv_m01, inv_m10, inv_m11, inv_tx, inv_ty];

    let img_w = image_width as f32;
    let img_h = image_height as f32;

    struct Candidate {
        handle: codec::Handle,
        entity_type: &'static str,
        layer: String,
        screen_pixel: [i32; 2],
        screen_bounds: [i32; 4],
        world_center: [f64; 3],
        selected: bool,
        area: f32,
    }

    let selected_set: std::collections::HashSet<codec::Handle> =
        tab.scene.selected_handles_in_order().into_iter().collect();

    let mut candidates = Vec::new();

    for entity in tab.scene.document.entities() {
        let handle = entity.common().handle;
        let (b_min, b_max) = crate::scene::convert::tess::entity_bounds_in(&tab.scene.document, entity);
        let min_dvec = glam::DVec3::from(b_min);
        let max_dvec = glam::DVec3::from(b_max);
        if !min_dvec.is_finite() || !max_dvec.is_finite() {
            continue;
        }
        let center = (min_dvec + max_dvec) * 0.5;
        if let Some(screen_pt) = c.project(center, vp_rect) {
            if screen_pt.x >= 0.0
                && screen_pt.x <= vp_rect.width
                && screen_pt.y >= 0.0
                && screen_pt.y <= vp_rect.height
            {
                let px = (screen_pt.x / vp_rect.width * img_w).round() as i32;
                let py = (screen_pt.y / vp_rect.height * img_h).round() as i32;

                let mut s_min_x = px;
                let mut s_max_x = px;
                let mut s_min_y = py;
                let mut s_max_y = py;

                if let (Some(p0), Some(p1)) =
                    (c.project(min_dvec, vp_rect), c.project(max_dvec, vp_rect))
                {
                    let x0 = (p0.x / vp_rect.width * img_w).round() as i32;
                    let y0 = (p0.y / vp_rect.height * img_h).round() as i32;
                    let x1 = (p1.x / vp_rect.width * img_w).round() as i32;
                    let y1 = (p1.y / vp_rect.height * img_h).round() as i32;
                    s_min_x = x0.min(x1);
                    s_max_x = x0.max(x1);
                    s_min_y = y0.min(y1);
                    s_max_y = y0.max(y1);
                }

                // An unbounded entity (XLINE, RAY) projects to saturated i32
                // corners, whose difference overflows i32.
                let width = (f64::from(s_max_x) - f64::from(s_min_x)).abs();
                let area = (width * (f64::from(s_max_y) - f64::from(s_min_y)).abs()) as f32;
                let selected = selected_set.contains(&handle);
                let layer = entity.common().layer.clone();

                candidates.push(Candidate {
                    handle,
                    entity_type: crate::entities::names::ui_name(entity),
                    layer,
                    screen_pixel: [px, py],
                    screen_bounds: [s_min_x, s_min_y, s_max_x, s_max_y],
                    world_center: [center.x, center.y, center.z],
                    selected,
                    area,
                });
            }
        }
    }

    // Selected entities prioritized, then larger visual area first
    candidates.sort_by(|a, b| {
        b.selected
            .cmp(&a.selected)
            .then_with(|| b.area.partial_cmp(&a.area).unwrap_or(std::cmp::Ordering::Equal))
    });

    candidates.truncate(64);

    let visible_entities = candidates
        .into_iter()
        .enumerate()
        .map(|(idx, cand)| {
            json!({
                "tag": idx + 1,
                "handle": format!("{:X}", cand.handle.value()),
                "type": cand.entity_type,
                "layer": cand.layer,
                "screen_pixel": cand.screen_pixel,
                "screen_bounds": cand.screen_bounds,
                "world_center": cand.world_center,
                "selected": cand.selected
            })
        })
        .collect();

    VisionGrounding {
        viewport_world_bounds,
        camera: camera_info,
        target_plane,
        pixel_to_world_matrix,
        world_to_pixel_matrix,
        visible_entities,
    }
}

#[derive(Clone, Debug)]
pub(crate) struct VisualDiffResult {
    pub changed: bool,
    pub change_percentage: f64,
    pub changed_pixels: usize,
    pub dirty_pixel_bounds: Option<[u32; 4]>, // [min_x, min_y, max_x, max_y]
    pub dirty_world_bounds: Option<[f64; 4]>, // [min_wx, min_wy, max_wx, max_wy]
    pub patch_resolution: Option<[u32; 2]>,
    pub cropped_patch: Option<image::RgbaImage>,
    pub diff_overlay: image::RgbaImage,
}

pub(crate) fn compute_visual_diff(
    baseline: &image::RgbaImage,
    current: &image::RgbaImage,
    pixel_to_world_matrix: &[f64; 6],
) -> VisualDiffResult {
    let (w, h) = (current.width(), current.height());
    let mut diff_overlay = image::RgbaImage::new(w, h);

    if baseline.width() != w || baseline.height() != h {
        return VisualDiffResult {
            changed: true,
            change_percentage: 100.0,
            changed_pixels: (w * h) as usize,
            dirty_pixel_bounds: Some([0, 0, w.saturating_sub(1), h.saturating_sub(1)]),
            dirty_world_bounds: None,
            patch_resolution: Some([w, h]),
            cropped_patch: Some(current.clone()),
            diff_overlay: current.clone(),
        };
    }

    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut changed_pixels = 0usize;

    for y in 0..h {
        for x in 0..w {
            let p1 = baseline.get_pixel(x, y);
            let p2 = current.get_pixel(x, y);

            let dr = (p1[0] as i32 - p2[0] as i32).abs();
            let dg = (p1[1] as i32 - p2[1] as i32).abs();
            let db = (p1[2] as i32 - p2[2] as i32).abs();
            let da = (p1[3] as i32 - p2[3] as i32).abs();

            let is_diff = (dr + dg + db + da) > 20;

            if is_diff {
                changed_pixels += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);

                // Highlight changed pixel in vibrant green
                diff_overlay.put_pixel(x, y, image::Rgba([0, 255, 128, 255]));
            } else {
                // Dim unchanged background: grayscale attenuated to 35%
                let gray = ((p2[0] as f32 * 0.299 + p2[1] as f32 * 0.587 + p2[2] as f32 * 0.114) * 0.35) as u8;
                diff_overlay.put_pixel(x, y, image::Rgba([gray, gray, gray, 200]));
            }
        }
    }

    let total_pixels = (w * h) as f64;
    let change_percentage = if total_pixels > 0.0 {
        (changed_pixels as f64 / total_pixels) * 100.0
    } else {
        0.0
    };

    if changed_pixels == 0 {
        return VisualDiffResult {
            changed: false,
            change_percentage: 0.0,
            changed_pixels: 0,
            dirty_pixel_bounds: None,
            dirty_world_bounds: None,
            patch_resolution: None,
            cropped_patch: None,
            diff_overlay,
        };
    }

    let pad = 16u32;
    let b_min_x = min_x.saturating_sub(pad);
    let b_min_y = min_y.saturating_sub(pad);
    let b_max_x = (max_x + pad).min(w.saturating_sub(1));
    let b_max_y = (max_y + pad).min(h.saturating_sub(1));

    let patch_w = (b_max_x - b_min_x + 1).max(1);
    let patch_h = (b_max_y - b_min_y + 1).max(1);

    let m = pixel_to_world_matrix;
    let px0 = b_min_x as f64;
    let py0 = b_min_y as f64;
    let px1 = b_max_x as f64;
    let py1 = b_max_y as f64;

    let p00_x = m[0] * px0 + m[1] * py0 + m[4];
    let p00_y = m[2] * px0 + m[3] * py0 + m[5];
    let p10_x = m[0] * px1 + m[1] * py0 + m[4];
    let p10_y = m[2] * px1 + m[3] * py0 + m[5];
    let p01_x = m[0] * px0 + m[1] * py1 + m[4];
    let p01_y = m[2] * px0 + m[3] * py1 + m[5];
    let p11_x = m[0] * px1 + m[1] * py1 + m[4];
    let p11_y = m[2] * px1 + m[3] * py1 + m[5];

    let w_min_x = p00_x.min(p10_x).min(p01_x).min(p11_x);
    let w_max_x = p00_x.max(p10_x).max(p01_x).max(p11_x);
    let w_min_y = p00_y.min(p10_y).min(p01_y).min(p11_y);
    let w_max_y = p00_y.max(p10_y).max(p01_y).max(p11_y);

    let cropped = image::imageops::crop_imm(current, b_min_x, b_min_y, patch_w, patch_h).to_image();

    VisualDiffResult {
        changed: true,
        change_percentage,
        changed_pixels,
        dirty_pixel_bounds: Some([b_min_x, b_min_y, b_max_x, b_max_y]),
        dirty_world_bounds: Some([w_min_x, w_min_y, w_max_x, w_max_y]),
        patch_resolution: Some([patch_w, patch_h]),
        cropped_patch: Some(cropped),
        diff_overlay,
    }
}

pub(crate) fn encode_image_png(img: &image::RgbaImage) -> Result<Vec<u8>, String> {
    use std::io::Cursor;
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode PNG: {e}"))?;
    Ok(buf.into_inner())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PyramidLevelInfo {
    pub level: u32,
    pub grid: [u32; 2],
    pub tile_world_span: [f64; 2],
    pub tiles_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PyramidManifest {
    pub crs: String,
    pub unit: String,
    pub world_bounds: [f64; 4],
    pub max_level: u32,
    pub tile_size: u32,
    pub tile_uri_template: String,
    pub levels: Vec<PyramidLevelInfo>,
}

pub fn compute_pyramid_manifest(
    session_id: &str,
    unit: &str,
    world_bounds: [f64; 4],
    max_level: u32,
    tile_size: u32,
) -> PyramidManifest {
    let min_x = world_bounds[0];
    let min_y = world_bounds[1];
    let max_x = world_bounds[2];
    let max_y = world_bounds[3];

    let w = (max_x - min_x).abs().max(1e-4);
    let h = (max_y - min_y).abs().max(1e-4);

    let max_lvl = max_level.clamp(1, 6);
    let mut levels = Vec::new();

    for level in 0..=max_lvl {
        let cols = 1u32 << level;
        let rows = 1u32 << level;
        let tile_w = w / cols as f64;
        let tile_h = h / rows as f64;
        levels.push(PyramidLevelInfo {
            level,
            grid: [cols, rows],
            tile_world_span: [tile_w, tile_h],
            tiles_count: cols * rows,
        });
    }

    PyramidManifest {
        crs: "CAD_WCS".to_string(),
        unit: unit.to_string(),
        world_bounds: [min_x, min_y, max_x, max_y],
        max_level: max_lvl,
        tile_size,
        tile_uri_template: format!("cad://session/{session_id}/tile/{{level}}/{{x}}/{{y}}.png"),
        levels,
    }
}

pub fn compute_tile_bounds(
    manifest_world_bounds: [f64; 4],
    level: u32,
    x: u32,
    y: u32,
) -> Result<[f64; 4], String> {
    let cols = 1u32 << level;
    let rows = 1u32 << level;

    if x >= cols || y >= rows {
        return Err(format!(
            "Tile coordinates ({x}, {y}) out of range for level {level} (grid is {cols}x{rows})"
        ));
    }

    let min_x = manifest_world_bounds[0];
    let min_y = manifest_world_bounds[1];
    let max_x = manifest_world_bounds[2];
    let max_y = manifest_world_bounds[3];

    let w = (max_x - min_x).abs().max(1e-4);
    let h = (max_y - min_y).abs().max(1e-4);

    let tile_w = w / cols as f64;
    let tile_h = h / rows as f64;

    let t_min_x = min_x + (x as f64) * tile_w;
    let t_max_x = t_min_x + tile_w;
    let t_max_y = max_y - (y as f64) * tile_h;
    let t_min_y = t_max_y - tile_h;

    Ok([t_min_x, t_min_y, t_max_x, t_max_y])
}

#[allow(dead_code)]
pub fn point_to_tile(
    manifest_world_bounds: [f64; 4],
    level: u32,
    point_x: f64,
    point_y: f64,
) -> (u32, u32) {
    let cols = 1u32 << level;
    let rows = 1u32 << level;

    let min_x = manifest_world_bounds[0];
    let min_y = manifest_world_bounds[1];
    let max_x = manifest_world_bounds[2];
    let max_y = manifest_world_bounds[3];

    let w = (max_x - min_x).abs().max(1e-4);
    let h = (max_y - min_y).abs().max(1e-4);

    let tile_w = w / cols as f64;
    let tile_h = h / rows as f64;

    let tx = (((point_x - min_x) / tile_w).floor() as i64).clamp(0, (cols - 1) as i64) as u32;
    let ty = (((max_y - point_y) / tile_h).floor() as i64).clamp(0, (rows - 1) as i64) as u32;

    (tx, ty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_draw_som_badge_renders_without_panics() {
        let mut img = image::RgbaImage::from_pixel(120, 100, image::Rgba([40, 40, 40, 255]));

        // Render normal badge
        draw_som_badge(&mut img, 50, 50, 1, false);

        // Center dot should be drawn
        let dot = img.get_pixel(50, 50);
        assert_eq!(*dot, image::Rgba([255, 60, 0, 255]));

        // Render selected badge with multi-digit tag near edge (clamped)
        draw_som_badge(&mut img, 2, 2, 42, true);
        let selected_dot = img.get_pixel(2, 2);
        assert_eq!(*selected_dot, image::Rgba([0, 230, 255, 255]));

        // Right edge clamp
        draw_som_badge(&mut img, 118, 98, 99, false);
    }

    #[test]
    fn test_compute_grounding_identifies_visible_entities() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 0,0 5"}"#);
        app.automation_op(r#"{"op":"run","cmd":"LINE -10,-10 10,10"}"#);

        // Set test canvas viewport size
        app.tabs[0].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        app.tabs[0].scene.fit_all();

        let grounding = compute_grounding(&app, 800.0, 600.0, 800, 600);

        // Viewport world bounds should be valid
        assert!(grounding.viewport_world_bounds["min"].is_array());
        assert!(grounding.viewport_world_bounds["max"].is_array());

        // Camera info should be populated with projection
        assert!(grounding.camera["eye"].is_array());
        assert!(grounding.camera["target"].is_array());
        assert_eq!(grounding.camera["projection"], "Orthographic");

        // Target plane normal and origin
        assert!(grounding.target_plane["normal"].is_array());
        assert!(grounding.target_plane["origin"].is_array());

        // Affine matrices: test forward and inverse round-trip
        let p2w = grounding.pixel_to_world_matrix;
        let w2p = grounding.world_to_pixel_matrix;

        // Center pixel (400, 300) should project to camera target (within 1e-4)
        let cx = 400.0;
        let cy = 300.0;
        let wx = p2w[0] * cx + p2w[1] * cy + p2w[4];
        let wy = p2w[2] * cx + p2w[3] * cy + p2w[5];
        let target_x = grounding.camera["target"][0].as_f64().unwrap();
        let target_y = grounding.camera["target"][1].as_f64().unwrap();
        assert!((wx - target_x).abs() < 1e-3, "wx={wx}, target_x={target_x}");
        assert!((wy - target_y).abs() < 1e-3, "wy={wy}, target_y={target_y}");

        // Inverse mapping of (wx, wy) should return (400, 300)
        let inv_px = w2p[0] * wx + w2p[1] * wy + w2p[4];
        let inv_py = w2p[2] * wx + w2p[3] * wy + w2p[5];
        assert!((inv_px - cx).abs() < 1e-3, "inv_px={inv_px}, cx={cx}");
        assert!((inv_py - cy).abs() < 1e-3, "inv_py={inv_py}, cy={cy}");

        // Both entities should be visible and tagged
        assert_eq!(grounding.visible_entities.len(), 2);

        let first = &grounding.visible_entities[0];
        assert_eq!(first["tag"], 1);
        assert!(first["handle"].is_string());
        assert!(first["type"].is_string());
        assert!(first["screen_pixel"].is_array());
        assert!(first["screen_bounds"].is_array());
        assert!(first["world_center"].is_array());

        let second = &grounding.visible_entities[1];
        assert_eq!(second["tag"], 2);
    }

    #[test]
    fn test_compute_visual_diff() {
        let w = 200;
        let h = 150;
        let base = image::RgbaImage::from_pixel(w, h, image::Rgba([30, 30, 30, 255]));
        let p2w = [0.1, 0.0, 0.0, -0.1, -10.0, 7.5];

        // 1. Identical images -> changed: false
        let diff_same = compute_visual_diff(&base, &base, &p2w);
        assert_eq!(diff_same.changed, false);
        assert_eq!(diff_same.change_percentage, 0.0);
        assert_eq!(diff_same.changed_pixels, 0);
        assert!(diff_same.dirty_pixel_bounds.is_none());

        // 2. Modify a 20x20 block from (50, 40) to (69, 59)
        let mut curr = base.clone();
        for y in 40..60 {
            for x in 50..70 {
                curr.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }

        let diff_mod = compute_visual_diff(&base, &curr, &p2w);
        assert_eq!(diff_mod.changed, true);
        assert_eq!(diff_mod.changed_pixels, 400);
        assert!(diff_mod.change_percentage > 1.0);

        let bounds = diff_mod.dirty_pixel_bounds.unwrap();
        // With 16px pad: min_x <= 50, max_x >= 69, min_y <= 40, max_y >= 59
        assert!(bounds[0] <= 50);
        assert!(bounds[1] <= 40);
        assert!(bounds[2] >= 69);
        assert!(bounds[3] >= 59);

        // World bounds should be populated
        assert!(diff_mod.dirty_world_bounds.is_some());
        let wb = diff_mod.dirty_world_bounds.unwrap();
        assert!(wb[0] < wb[2]); // min_x < max_x
        assert!(wb[1] < wb[3]); // min_y < max_y

        // Cropped patch and overlay
        assert!(diff_mod.cropped_patch.is_some());
        let patch = diff_mod.cropped_patch.unwrap();
        assert!(patch.width() >= 20);
        assert!(patch.height() >= 20);

        // PNG encoding roundtrip
        let png_bytes = encode_image_png(&patch).expect("png encode succeeds");
        assert!(!png_bytes.is_empty());
        assert_eq!(&png_bytes[1..4], b"PNG");
    }

    #[test]
    fn test_pyramid_tiling_manifest_and_bounds() {
        let wb = [0.0, 0.0, 200.0, 100.0];
        let manifest = compute_pyramid_manifest("sess_test", "Millimeters", wb, 3, 512);

        assert_eq!(manifest.crs, "CAD_WCS");
        assert_eq!(manifest.unit, "Millimeters");
        assert_eq!(manifest.world_bounds, wb);
        assert_eq!(manifest.max_level, 3);
        assert_eq!(manifest.tile_size, 512);
        assert_eq!(manifest.levels.len(), 4); // 0, 1, 2, 3

        // Level 0: 1x1 tile covering full world bounds
        assert_eq!(manifest.levels[0].grid, [1, 1]);
        assert_eq!(manifest.levels[0].tiles_count, 1);
        assert_eq!(manifest.levels[0].tile_world_span, [200.0, 100.0]);
        let t00 = compute_tile_bounds(wb, 0, 0, 0).unwrap();
        assert_eq!(t00, [0.0, 0.0, 200.0, 100.0]);

        // Level 1: 2x2 grid (4 tiles, 100x50 span each)
        assert_eq!(manifest.levels[1].grid, [2, 2]);
        assert_eq!(manifest.levels[1].tiles_count, 4);
        assert_eq!(manifest.levels[1].tile_world_span, [100.0, 50.0]);

        // (0, 0): top-left
        let t1_00 = compute_tile_bounds(wb, 1, 0, 0).unwrap();
        assert_eq!(t1_00, [0.0, 50.0, 100.0, 100.0]);

        // (1, 0): top-right
        let t1_10 = compute_tile_bounds(wb, 1, 1, 0).unwrap();
        assert_eq!(t1_10, [100.0, 50.0, 200.0, 100.0]);

        // (0, 1): bottom-left
        let t1_01 = compute_tile_bounds(wb, 1, 0, 1).unwrap();
        assert_eq!(t1_01, [0.0, 0.0, 100.0, 50.0]);

        // (1, 1): bottom-right
        let t1_11 = compute_tile_bounds(wb, 1, 1, 1).unwrap();
        assert_eq!(t1_11, [100.0, 0.0, 200.0, 50.0]);

        // Out-of-range checks
        assert!(compute_tile_bounds(wb, 1, 2, 0).is_err());
        assert!(compute_tile_bounds(wb, 1, 0, 2).is_err());

        // Point-to-tile mapping
        assert_eq!(point_to_tile(wb, 1, 25.0, 75.0), (0, 0));
        assert_eq!(point_to_tile(wb, 1, 175.0, 75.0), (1, 0));
        assert_eq!(point_to_tile(wb, 1, 25.0, 25.0), (0, 1));
        assert_eq!(point_to_tile(wb, 1, 175.0, 25.0), (1, 1));
    }
}
