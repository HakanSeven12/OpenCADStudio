use iced::{widget::canvas, Color, Point, Size};

#[derive(Default)]
pub struct AuthoringFeedback {
    pub dependencies: Vec<[Point; 2]>,
    pub markers: Vec<Point>,
    pub clip: iced::Rectangle,
}

/// Display-only settings; no geometry or drawing history depends on these values.
#[derive(Clone, Debug)]
pub struct AuthoringVisualOptions {
    pub action: Color,
    pub parameter: Color,
    pub grip: Color,
    pub grip_size: f32,
    pub text_size: f32,
    pub font: String,
    pub horizontal: bool,
}

impl AuthoringVisualOptions {
    pub(crate) fn new(
        prefs: &crate::app::settings::BlockEditorPreferences,
        doc: &codec::Document,
        bg: [f32; 4],
    ) -> Self {
        let foreground = if bg[0] * 0.2126 + bg[1] * 0.7152 + bg[2] * 0.0722 < 0.5 {
            Color::WHITE
        } else {
            Color::BLACK
        };
        let by_layer = doc
            .layers
            .get(&doc.header.current_layer_name)
            .and_then(|l| match l.color {
                codec::types::Color::Index(7) => None,
                c => c.rgb(),
            })
            .map(|(r, g, b)| Color::from_rgb8(r, g, b))
            .unwrap_or(foreground);
        Self {
            action: display_color(&prefs.action_color, by_layer, foreground),
            parameter: display_color(&prefs.parameter_color, by_layer, foreground),
            grip: display_color(&prefs.grip_color, by_layer, foreground),
            grip_size: prefs.grip_size.max(1) as f32,
            text_size: prefs.parameter_size.max(1) as f32,
            font: prefs.parameter_font.clone(),
            horizontal: prefs.text_horizontal,
        }
    }
}

pub(crate) fn normalize_color(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_uppercase();
    match value.as_str() {
        "BYLAYER" | "BYBLOCK" => Some(value),
        _ if value.starts_with("RGB:") => {
            let channels = value[4..]
                .split(',')
                .map(|v| v.trim().parse::<u8>())
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            (channels.len() == 3)
                .then(|| format!("RGB:{},{},{}", channels[0], channels[1], channels[2]))
        }
        _ => value
            .parse::<u8>()
            .ok()
            .filter(|v| *v > 0)
            .map(|v| v.to_string()),
    }
}

pub(crate) fn display_color(value: &str, by_layer: Color, foreground: Color) -> Color {
    let Some(value) = normalize_color(value) else {
        return foreground;
    };
    match value.as_str() {
        "BYLAYER" => by_layer,
        "BYBLOCK" | "7" => foreground,
        _ if value.starts_with("RGB:") => {
            let channels: Vec<u8> = value[4..]
                .split(',')
                .filter_map(|s| s.parse().ok())
                .collect();
            Color::from_rgb8(channels[0], channels[1], channels[2])
        }
        _ => codec::types::aci_table::aci_to_rgb(value.parse().unwrap_or(7))
            .map(|(r, g, b)| Color::from_rgb8(r, g, b))
            .unwrap_or(foreground),
    }
}

/// Glyphs share the drawing text engine's stroke, SHX, TrueType and big-font
/// resolution. Transform both the label and its opaque backing together.
pub(super) fn draw_label(
    frame: &mut canvas::Frame,
    point: Point,
    text: &str,
    color: Color,
    background: Option<Color>,
    angle: f32,
    style: &AuthoringVisualOptions,
) {
    use crate::scene::text::font_face::Face;
    let face = Face::resolve(&font_name(&style.font));
    let scale = style.text_size / 9.0;
    let width: f32 = text
        .chars()
        .map(|ch| {
            if ch == ' ' {
                face.word_spacing()
            } else {
                face.glyph(ch).map_or(face.word_spacing(), |g| g.advance) + face.spacing_after(ch)
            }
        })
        .sum::<f32>()
        * scale;
    let (sin, cos) = angle.sin_cos();
    let project =
        |x: f32, y: f32| Point::new(point.x + x * cos - y * sin, point.y + x * sin + y * cos);
    if let Some(background) = background {
        let path = canvas::Path::new(|p| {
            p.move_to(project(-3.0, -style.text_size - 3.0));
            p.line_to(project(width + 3.0, -style.text_size - 3.0));
            p.line_to(project(width + 3.0, style.text_size * 0.4 + 3.0));
            p.line_to(project(-3.0, style.text_size * 0.4 + 3.0));
            p.close();
        });
        frame.fill(&path, background);
    }
    let mut x = 0.0;
    for ch in text.chars() {
        if ch == ' ' {
            x += face.word_spacing() * scale;
            continue;
        }
        if let Some(glyph) = face.glyph(ch) {
            for stroke in &glyph.strokes {
                let path = canvas::Path::new(|p| {
                    for (index, v) in stroke.iter().enumerate() {
                        let v = project(x + v[0] * scale, -v[1] * scale);
                        if index == 0 {
                            p.move_to(v);
                        } else {
                            p.line_to(v);
                        }
                    }
                });
                frame.stroke(&path, canvas::Stroke::default().with_color(color));
            }
            for triangle in glyph.fill_tris.chunks_exact(3) {
                let path = canvas::Path::new(|p| {
                    p.move_to(project(x + triangle[0][0] * scale, -triangle[0][1] * scale));
                    p.line_to(project(x + triangle[1][0] * scale, -triangle[1][1] * scale));
                    p.line_to(project(x + triangle[2][0] * scale, -triangle[2][1] * scale));
                    p.close();
                });
                frame.fill(&path, color);
            }
            x += (glyph.advance + face.spacing_after(ch)) * scale;
        } else {
            x += face.word_spacing() * scale;
        }
    }
}

fn font_name(font: &str) -> String {
    let (primary, big) = font
        .split_once(',')
        .map_or((font, None), |(primary, big)| (primary, Some(big.trim())));
    let primary = primary.trim();
    let resolved = if primary.to_ascii_lowercase().ends_with(".shx") {
        primary.to_owned()
    } else {
        crate::scene::text::sysfont::family_for_reference(primary)
            .unwrap_or_else(|| primary.to_owned())
    };
    big.map_or_else(
        || resolved.clone(),
        |big| crate::scene::text::font_face::Face::with_big_font(&resolved, big),
    )
}

pub(crate) fn label_corners(
    anchor: Point,
    text: &str,
    angle: f32,
    size: u8,
    font: &str,
) -> [Point; 4] {
    let face = crate::scene::text::font_face::Face::resolve(&font_name(font));
    let size = size.max(1) as f32;
    let width = text
        .chars()
        .map(|ch| {
            if ch == ' ' {
                face.word_spacing()
            } else {
                face.glyph(ch).map_or(face.word_spacing(), |g| g.advance) + face.spacing_after(ch)
            }
        })
        .sum::<f32>()
        * size
        / 9.0;
    let (sin, cos) = angle.sin_cos();
    let project = |x: f32, y: f32| {
        Point::new(
            anchor.x + 5.0 + x * cos - y * sin,
            anchor.y - 4.0 + x * sin + y * cos,
        )
    };
    [
        project(-3.0, -size - 3.0),
        project(width + 3.0, -size - 3.0),
        project(width + 3.0, size * 0.4 + 3.0),
        project(-3.0, size * 0.4 + 3.0),
    ]
}

pub(crate) fn label_contains(corners: [Point; 4], point: Point) -> bool {
    let mut sign = 0.0_f32;
    for (a, b) in corners.iter().zip(corners.iter().cycle().skip(1)).take(4) {
        let cross = (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x);
        if cross.abs() < 0.01 {
            continue;
        }
        if sign != 0.0 && cross.signum() != sign {
            return false;
        }
        sign = cross.signum();
    }
    true
}

pub(super) fn grip_rectangle(point: Point, size: f32) -> canvas::Path {
    canvas::Path::rectangle(
        Point::new(point.x - size / 2.0, point.y - size / 2.0),
        Size::new(size, size),
    )
}

pub(crate) fn readable_angle(mut angle: f32) -> f32 {
    if angle > std::f32::consts::FRAC_PI_2 {
        angle -= std::f32::consts::PI;
    }
    if angle < -std::f32::consts::FRAC_PI_2 {
        angle += std::f32::consts::PI;
    }
    angle
}

pub(crate) fn label_angle(dimensions: &[[Point; 2]], index: usize, horizontal: bool) -> f32 {
    if horizontal {
        return 0.0;
    }
    dimensions
        .get(index)
        .or_else(|| dimensions.first())
        .map_or(0.0, |[a, b]| readable_angle((b.y - a.y).atan2(b.x - a.x)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authoring_colors_validate_and_resolve_without_silent_coercion() {
        for invalid in [
            "0",
            "256",
            "-1",
            "RGB:1,2",
            "RGB:1,2,256",
            "RGB:1,2,3,4",
            "red",
        ] {
            assert!(normalize_color(invalid).is_none(), "{invalid}");
        }
        assert_eq!(
            normalize_color("rgb: 01,2,255").as_deref(),
            Some("RGB:1,2,255")
        );
        assert_eq!(
            display_color("BYLAYER", Color::BLACK, Color::WHITE),
            Color::BLACK
        );
        assert_eq!(
            display_color("BYBLOCK", Color::BLACK, Color::WHITE),
            Color::WHITE
        );
        assert_eq!(
            display_color("RGB:1,2,3", Color::BLACK, Color::WHITE),
            Color::from_rgb8(1, 2, 3)
        );
    }

    #[test]
    fn authoring_label_picking_tracks_font_size_and_rotation() {
        let anchor = Point::new(100.0, 100.0);
        let small = label_corners(anchor, "Distance", 0.0, 12, "Simplex.shx");
        let large = label_corners(anchor, "Distance", 0.0, 48, "Simplex.shx");
        assert!(large[1].x - small[1].x > 100.0);
        let angle = std::f32::consts::FRAC_PI_4;
        let rotated = label_corners(anchor, "Distance", angle, 48, "Simplex.shx");
        let center = Point::new(
            (rotated[0].x + rotated[2].x) / 2.0,
            (rotated[0].y + rotated[2].y) / 2.0,
        );
        assert!(label_contains(rotated, center));
        assert!(!label_contains(small, center));
        assert!(!label_contains(rotated, Point::ORIGIN));
        assert!((readable_angle(3.0).abs()) < std::f32::consts::FRAC_PI_2);
    }
}
