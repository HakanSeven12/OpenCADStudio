use acadrust::entities::{BoundaryEdge, Hatch};
use cadkernel::geom2d::{
    Arc as KernelArc, Curve as KernelCurve, Ellipse as KernelEllipse,
    EllipseArc as KernelEllipseArc, Line as KernelLine, NurbsCurve as KernelNurbs,
    Parameterization, Polyline as KernelPolyline, PolylineVertex as KernelVertex,
};
use glam::Vec3;
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{center_grip, circle_grip, edit_angle_prop as edit_angle, edit_prop as edit, parse_f64, ro_prop as ro};
use crate::entities::traits::{FallbackTess, Grippable, PropertyEditable, Transformable};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::convert::tess_util::FallbackGeometry;
use crate::scene::model::wire_model::SnapHint;

/// The area enclosed by the hatch boundary paths.
pub(crate) fn boundary_area(h: &Hatch) -> f64 {
    let mut path_areas = Vec::new();
    let mut rings = Vec::new();
    for path in &h.paths {
        let mut path_area = 0.0;
        let directions = crate::scene::hatch_path_directions(path);
        let curves = path.edges.iter().filter_map(edge_curve);
        for (curve, direction) in curves.zip(directions) {
            path_area += direction * curve.enclosed_area();
        }
        path_areas.push(path_area.abs());
        rings.push(crate::scene::hatch_path_ring(path).unwrap_or_default());
    }
    let depths = cadkernel::geom2d::ring_nesting_depths(&rings);
    path_areas
        .into_iter()
        .zip(depths)
        .filter_map(|(area, depth)| match h.style {
            acadrust::entities::HatchStyleType::Normal => {
                Some(if depth % 2 == 0 { area } else { -area })
            }
            acadrust::entities::HatchStyleType::Outer if depth <= 1 => {
                Some(if depth == 0 { area } else { -area })
            }
            acadrust::entities::HatchStyleType::Outer => None,
            acadrust::entities::HatchStyleType::Ignore if depth == 0 => Some(area),
            acadrust::entities::HatchStyleType::Ignore => None,
        })
        .sum::<f64>()
        .abs()
}

/// A hatch boundary edge as a kernel curve in the hatch OCS.
pub(crate) fn edge_curve(edge: &BoundaryEdge) -> Option<KernelCurve> {
    Some(match edge {
        BoundaryEdge::Line(l) => KernelCurve::Line(KernelLine {
            start: [l.start.x, l.start.y],
            end: [l.end.x, l.end.y],
        }),
        BoundaryEdge::CircularArc(a) => {
            let (start, end) = if a.counter_clockwise {
                (a.start_angle, a.end_angle)
            } else {
                (
                    std::f64::consts::TAU - a.end_angle,
                    std::f64::consts::TAU - a.start_angle,
                )
            };
            KernelCurve::Arc(KernelArc {
                centre: [a.center.x, a.center.y],
                radius: a.radius,
                start_angle: start,
                end_angle: end,
            })
        }
        BoundaryEdge::EllipticArc(e) => {
            let major = (e.major_axis_endpoint.x, e.major_axis_endpoint.y);
            let radius = major.0.hypot(major.1);
            if radius < 1e-12 {
                return None;
            }
            let (start, end) = if e.counter_clockwise {
                (e.start_angle, e.end_angle)
            } else {
                (
                    std::f64::consts::TAU - e.end_angle,
                    std::f64::consts::TAU - e.start_angle,
                )
            };
            KernelCurve::Ellipse(KernelEllipseArc {
                ellipse: KernelEllipse {
                    centre: [e.center.x, e.center.y],
                    major_radius: radius,
                    minor_radius: radius * e.minor_axis_ratio,
                    major_axis: [major.0 / radius, major.1 / radius],
                },
                start_parameter: start,
                end_parameter: end,
            })
        }
        BoundaryEdge::Polyline(poly) => {
            let vertices: Vec<KernelVertex> = poly
                .vertices
                .iter()
                .map(|v| KernelVertex {
                    position: [v.x, v.y],
                    // A boundary polyline stores its bulge in the vertex's
                    // third component.
                    bulge: v.z,
                })
                .collect();
            if vertices.len() < 2 {
                return None;
            }
            KernelCurve::Polyline(KernelPolyline {
                vertices,
                closed: poly.is_closed,
            })
        }
        BoundaryEdge::Spline(s) => {
            let control: Vec<[f64; 2]> = s.control_points.iter().map(|p| [p.x, p.y]).collect();
            // A boundary spline's control points carry their weight in the
            // third component; for a polynomial one it is unset and the
            // kernel's own default of all-ones applies.
            let weights = s
                .rational
                .then(|| s.control_points.iter().map(|p| p.z).collect::<Vec<f64>>());
            let curve = KernelNurbs::new(
                s.degree.max(1) as usize,
                control,
                s.knots.clone(),
                weights,
            )
            .or_else(|| {
                // A fit-point boundary spline, interpolated the same way a
                // SPLINE entity's is.
                let fit: Vec<[f64; 2]> = s.fit_points.iter().map(|p| [p.x, p.y]).collect();
                KernelNurbs::interpolate(&fit, None, None, Parameterization::Chord)
            })?;
            KernelCurve::Nurbs(curve)
        }
    })
}

/// Mean of every boundary edge point (OCS) — the centroid used to place the
/// pattern-origin grip on the hatch body. `None` when there are no boundary
/// points.
fn boundary_centroid(h: &Hatch) -> Option<(f64, f64)> {
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut n = 0.0;
    let mut add = |x: f64, y: f64| {
        sx += x;
        sy += y;
        n += 1.0;
    };
    for path in &h.paths {
        for edge in &path.edges {
            match edge {
                BoundaryEdge::Polyline(p) => {
                    for v in &p.vertices {
                        add(v.x, v.y);
                    }
                }
                BoundaryEdge::Line(l) => {
                    add(l.start.x, l.start.y);
                    add(l.end.x, l.end.y);
                }
                BoundaryEdge::CircularArc(a) => add(a.center.x, a.center.y),
                BoundaryEdge::EllipticArc(e) => add(e.center.x, e.center.y),
                BoundaryEdge::Spline(s) => {
                    if !s.fit_points.is_empty() {
                        for p in &s.fit_points {
                            add(p.x, p.y);
                        }
                    } else {
                        for p in &s.control_points {
                            add(p.x, p.y);
                        }
                    }
                }
            }
        }
    }
    (n > 0.0).then(|| (sx / n, sy / n))
}

/// Scale the stored, final hatch-pattern geometry around the pattern origin.
/// Pattern lines loaded from DXF/DWG are rendered prebaked, so their metadata
/// scale is not applied again by the renderer.
pub(crate) fn scale_pattern_geometry(
    pattern: &mut acadrust::entities::HatchPattern,
    factor: f64,
) {
    let (origin_x, origin_y) = pattern
        .lines
        .first()
        .map(|line| (line.base_point.x, line.base_point.y))
        .unwrap_or((0.0, 0.0));
    for line in pattern.lines.iter_mut() {
        line.base_point.x = origin_x + (line.base_point.x - origin_x) * factor;
        line.base_point.y = origin_y + (line.base_point.y - origin_y) * factor;
        line.offset.x *= factor;
        line.offset.y *= factor;
        for dash in line.dash_lengths.iter_mut() {
            *dash *= factor;
        }
    }
}

/// Rotate the stored, final hatch-pattern geometry around the pattern origin.
/// This keeps the line angle, base point and world-space offset in sync.
pub(crate) fn rotate_pattern_geometry(
    pattern: &mut acadrust::entities::HatchPattern,
    angle: f64,
) {
    let (sin, cos) = angle.sin_cos();
    let (origin_x, origin_y) = pattern
        .lines
        .first()
        .map(|line| (line.base_point.x, line.base_point.y))
        .unwrap_or((0.0, 0.0));
    for line in pattern.lines.iter_mut() {
        line.angle += angle;
        let (x, y) = (
            line.base_point.x - origin_x,
            line.base_point.y - origin_y,
        );
        line.base_point.x = origin_x + x * cos - y * sin;
        line.base_point.y = origin_y + x * sin + y * cos;
        let (x, y) = (line.offset.x, line.offset.y);
        line.offset.x = x * cos - y * sin;
        line.offset.y = x * sin + y * cos;
    }
}

/// Move every stored line by the same amount, preserving the pattern phase.
pub(crate) fn translate_pattern_geometry(
    pattern: &mut acadrust::entities::HatchPattern,
    dx: f64,
    dy: f64,
) {
    for line in pattern.lines.iter_mut() {
        line.base_point.x += dx;
        line.base_point.y += dy;
    }
}

/// Read the hatch background colour from its `HATCHBACKGROUNDCOLOR` extended
/// data (group 1071, packed true colour: high byte = colour method, low three =
/// RGB). `None` when unset or not a true-colour value.
pub fn background_color(h: &Hatch) -> Option<acadrust::types::Color> {
    let rec = h.common.extended_data.get_record("HATCHBACKGROUNDCOLOR")?;
    let raw = rec.values.iter().find_map(|v| match v {
        acadrust::xdata::XDataValue::Integer32(n) => Some(*n as u32),
        _ => None,
    })?;
    match (raw >> 24) & 0xFF {
        // Colour-method byte: C0 = ByLayer, C1 = ByBlock, C2 = true colour,
        // C3 = ACI index.
        0xC0 => Some(acadrust::types::Color::ByLayer),
        0xC1 => Some(acadrust::types::Color::ByBlock),
        0xC2 => Some(acadrust::types::Color::Rgb {
            r: ((raw >> 16) & 0xFF) as u8,
            g: ((raw >> 8) & 0xFF) as u8,
            b: (raw & 0xFF) as u8,
        }),
        0xC3 => Some(acadrust::types::Color::Index((raw & 0xFF) as u8)),
        _ => None,
    }
}

/// Pack an RGB colour into a `HATCHBACKGROUNDCOLOR` group-1071 true-colour word.
pub fn pack_background_color(c: &acadrust::types::Color) -> i32 {
    use acadrust::types::Color;
    (match c {
        Color::ByLayer => 0xC0000000u32,
        Color::ByBlock => 0xC1000000u32,
        Color::Index(i) => 0xC3000000u32 | (*i as u8 as u32),
        _ => {
            let (r, g, b) = c.rgb().unwrap_or((255, 255, 255));
            0xC2000000u32 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
        }
    }) as i32
}

/// Replace (or insert) the hatch's `HATCHBACKGROUNDCOLOR` extended-data record,
/// preserving every other application's records.
pub fn set_background_color(h: &mut Hatch, c: &acadrust::types::Color) {
    use acadrust::xdata::{ExtendedDataRecord, XDataValue};
    const APP: &str = "HATCHBACKGROUNDCOLOR";
    let kept: Vec<ExtendedDataRecord> = h
        .common
        .extended_data
        .records()
        .iter()
        .filter(|r| r.application_name != APP)
        .cloned()
        .collect();
    h.common.extended_data.clear();
    for r in kept {
        h.common.extended_data.add_record(r);
    }
    let mut rec = ExtendedDataRecord::new(APP);
    rec.add_value(XDataValue::Integer32(pack_background_color(c)));
    h.common.extended_data.add_record(rec);
}

/// Remove the hatch's `HATCHBACKGROUNDCOLOR` extended-data record (background
/// off), preserving every other application's records.
pub fn clear_background_color(h: &mut Hatch) {
    use acadrust::xdata::ExtendedDataRecord;
    const APP: &str = "HATCHBACKGROUNDCOLOR";
    let kept: Vec<ExtendedDataRecord> = h
        .common
        .extended_data
        .records()
        .iter()
        .filter(|r| r.application_name != APP)
        .cloned()
        .collect();
    h.common.extended_data.clear();
    for r in kept {
        h.common.extended_data.add_record(r);
    }
}

fn associative_property(h: &Hatch) -> Property {
    if h
        .paths
        .iter()
        .any(|path| !path.boundary_handles.is_empty())
    {
        Property {
            label: t!("Associative").into_owned(),
            field: "associative",
            value: PropValue::BoolToggle {
                field: "associative",
                value: h.is_associative,
            },
        }
    } else {
        ro(
            t!("Associative").as_ref(),
            "associative",
            if h.is_associative {
                t!("Yes").into_owned()
            } else {
                t!("No").into_owned()
            },
        )
    }
}

fn properties(h: &Hatch) -> Vec<PropSection> {
    let pattern_type = match h.pattern_type {
        acadrust::entities::HatchPatternType::Predefined => "Predefined",
        acadrust::entities::HatchPatternType::UserDefined => "User Defined",
        acadrust::entities::HatchPatternType::Custom => "Custom",
    };
    let style = match h.style {
        acadrust::entities::HatchStyleType::Normal => "Normal",
        acadrust::entities::HatchStyleType::Outer => "Outer",
        acadrust::entities::HatchStyleType::Ignore => "Ignore",
    };
    let area = boundary_area(h);
    let g = &h.gradient_color;
    let default_col = acadrust::types::Color::Index(7);
    let grad_c1 = g.colors.first().map(|e| e.color).unwrap_or(default_col);
    let grad_c2 = g.colors.get(1).map(|e| e.color).unwrap_or(default_col);
    let bg_on = background_color(h).is_some();
    let bg_col = background_color(h).unwrap_or(default_col);

    if g.enabled {
        // ── Gradient fill ──────────────────────────────────────────────────
        let grad_type = if g.is_single_color { "One color" } else { "Two color" };
        let (kind, inverted) =
            crate::scene::model::hatch_model::GradientKind::from_name(&g.name);
        let mut pattern_props = vec![
            ro(t!("Type").as_ref(), "fill_kind", t!("Gradient").into_owned()),
            Property {
                label: t!("Color mode").into_owned(),
                field: "fill_type",
                value: PropValue::Choice {
                    selected: grad_type.to_string(),
                    options: vec!["One color".into(), "Two color".into()],
                },
            },
            Property {
                label: t!("Gradient type").into_owned(),
                field: "gradient_type",
                value: PropValue::Choice {
                    selected: kind.choice_label(inverted).to_string(),
                    options: crate::scene::model::hatch_model::GradientKind::CHOICES
                        .iter()
                        .map(|(kind, inverted)| kind.choice_label(*inverted).to_string())
                        .collect(),
                },
            },
            Property {
                label: t!("Color 1").into_owned(),
                field: "gradient_color_1",
                value: PropValue::ColorChoice(grad_c1),
            },
        ];
        if g.is_single_color {
            pattern_props.push(edit(
                t!("Tint/Shade").as_ref(),
                "gradient_tint",
                g.color_tint.clamp(0.0, 1.0),
            ));
        } else {
            pattern_props.push(Property {
                label: t!("Color 2").into_owned(),
                field: "gradient_color_2",
                value: PropValue::ColorChoice(grad_c2),
            });
        }
        pattern_props.push(edit_angle(
            t!("Angle").as_ref(),
            "pattern_angle",
            g.angle.to_degrees(),
        ));
        pattern_props.push(Property {
            label: t!("Centered").into_owned(),
            field: "gradient_centered",
            value: PropValue::BoolToggle {
                field: "gradient_centered",
                value: g.shift < 0.5,
            },
        });

        return vec![
            PropSection {
                title: t!("Pattern").into_owned(),
                props: pattern_props,
            },
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    edit(t!("Elevation").as_ref(), "elevation", h.elevation),
                    ro(t!("Area").as_ref(), "area", format!("{:.4}", area)),
                    ro(t!("Cumulative area").as_ref(), "cumulative_area", format!("{:.4}", area)),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    associative_property(h),
                    ro(t!("Annotative").as_ref(), "annotative", String::new()),
                    Property {
                        label: t!("Island detection style").into_owned(),
                        field: "style",
                        value: PropValue::Choice {
                            selected: style.to_string(),
                            options: vec!["Normal".into(), "Outer".into(), "Ignore".into()],
                        },
                    },
                ],
            },
        ];
    }


    // ── Hatch (pattern / solid) ────────────────────────────────────────────
    // Show only controls used by the selected fill type.
    let type_row = if h.is_solid {
        ro(t!("Type").as_ref(), "fill_kind", t!("Solid").into_owned())
    } else {
        Property {
            label: t!("Type").into_owned(),
            field: "pattern_type_label",
            value: PropValue::Choice {
                selected: pattern_type.to_string(),
                options: vec!["Predefined".into(), "User Defined".into(), "Custom".into()],
            },
        }
    };
    let pattern_name_row = Property {
        label: t!("Pattern name").into_owned(),
        field: "pattern_name",
        value: PropValue::HatchPatternChoice(h.pattern.name.clone()),
    };
    let associative_row = associative_property(h);
    let double_row = Property {
        label: t!("Double").into_owned(),
        field: "double",
        value: PropValue::BoolToggle {
            field: "double",
            value: h.is_double,
        },
    };
    let island_row = Property {
        label: t!("Island detection style").into_owned(),
        field: "style",
        value: PropValue::Choice {
            selected: style.to_string(),
            options: vec!["Normal".into(), "Outer".into(), "Ignore".into()],
        },
    };
    let spacing_row = edit(t!("Spacing").as_ref(), "spacing", h.pattern_scale);

    let mut pattern_props = vec![type_row, pattern_name_row];
    pattern_props.push(ro(t!("Annotative").as_ref(), "annotative", String::new()));
    if !h.is_solid {
        pattern_props.push(edit_angle(
            t!("Angle").as_ref(),
            "pattern_angle",
            h.pattern_angle.to_degrees(),
        ));
        if matches!(
            h.pattern_type,
            acadrust::entities::HatchPatternType::UserDefined
        ) {
            pattern_props.push(spacing_row);
            pattern_props.push(double_row);
        } else {
            pattern_props.push(edit(t!("Scale").as_ref(), "pattern_scale", h.pattern_scale));
            // Origin edits are relative offsets.
            pattern_props.push(edit(t!("Origin X").as_ref(), "origin_x", 0.0));
            pattern_props.push(edit(t!("Origin Y").as_ref(), "origin_y", 0.0));
            if h.pattern.name.to_ascii_uppercase().starts_with("ISO") {
                pattern_props.push(edit(
                    t!("ISO pen width").as_ref(),
                    "iso_pen_width",
                    h.pattern_scale,
                ));
            }
        }
    }
    pattern_props.push(associative_row);
    pattern_props.push(island_row);
    pattern_props.push(Property {
        label: t!("Background").into_owned(),
        field: "bg_enabled",
        value: PropValue::BoolToggle {
            field: "bg_enabled",
            value: bg_on,
        },
    });

    let mut sections = vec![
        PropSection {
            title: t!("Pattern").into_owned(),
            props: pattern_props,
        },
        PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                edit(t!("Elevation").as_ref(), "elevation", h.elevation),
                ro(t!("Area").as_ref(), "area", format!("{:.4}", area)),
                ro(t!("Cumulative area").as_ref(), "cumulative_area", format!("{:.4}", area)),
            ],
        },
    ];
    if bg_on {
        if let Some(sec) = sections.first_mut() {
            sec.props.push(Property {
                label: t!("Background color").into_owned(),
                field: "background_color",
                value: PropValue::ColorChoice(bg_col),
            });
        }
    }
    sections
}

fn apply_geom_prop(h: &mut Hatch, field: &str, value: &str) {
    use acadrust::entities::{HatchPatternType, HatchStyleType};
    // Non-numeric fields (bool toggles / enum choices) — handled before the
    // f64 parse below, which would otherwise reject their string values.
    match field {
        // Background on/off (#415): off removes the HATCHBACKGROUNDCOLOR
        // record entirely (no background), on seeds a white one for the
        // colour picker to change.
        "bg_enabled" => {
            if background_color(h).is_some() {
                clear_background_color(h);
            } else {
                set_background_color(h, &acadrust::types::Color::ByLayer);
            }
            return;
        }
        "double" => {
            h.is_double = if value == "toggle" {
                !h.is_double
            } else {
                value == "true"
            };
            return;
        }
        "associative" => {
            let requested = if value == "toggle" {
                !h.is_associative
            } else {
                value == "true"
            };
            // An associative flag without source handles is inert and cannot
            // update from boundary edits. Only enable it when a real
            // relationship is available; disabling retains the handles so the
            // user can turn it back on later.
            if !requested
                || h.paths
                    .iter()
                    .any(|path| !path.boundary_handles.is_empty())
            {
                h.is_associative = requested;
            }
            return;
        }
        "pattern_type_label" => {
            let requested = match value {
                "Predefined" => HatchPatternType::Predefined,
                "User Defined" => HatchPatternType::UserDefined,
                "Custom" => HatchPatternType::Custom,
                _ => h.pattern_type,
            };
            if requested != h.pattern_type {
                let old_origin = h.pattern.lines.first().map(|line| line.base_point);
                h.pattern_type = requested;
                h.is_solid = false;
                match requested {
                    HatchPatternType::UserDefined => {
                        // Rebuild user-defined geometry from its parameters.
                        h.pattern = acadrust::entities::HatchPattern::new("_USER");
                    }
                    HatchPatternType::Predefined => {
                        if let Some(entry) =
                            crate::scene::model::hatch_patterns::find("ANSI31")
                        {
                            let mut pattern =
                                crate::scene::model::hatch_patterns::build_dxf_pattern(entry);
                            scale_pattern_geometry(&mut pattern, h.pattern_scale);
                            rotate_pattern_geometry(&mut pattern, h.pattern_angle);
                            if let (Some(old), Some(new)) =
                                (old_origin, pattern.lines.first().map(|line| line.base_point))
                            {
                                translate_pattern_geometry(
                                    &mut pattern,
                                    old.x - new.x,
                                    old.y - new.y,
                                );
                            }
                            h.pattern = pattern;
                        }
                    }
                    HatchPatternType::Custom => {}
                }
            }
            return;
        }
        "style" => {
            h.style = match value {
                "Normal" => HatchStyleType::Normal,
                "Outer" => HatchStyleType::Outer,
                "Ignore" => HatchStyleType::Ignore,
                _ => h.style,
            };
            return;
        }
        "fill_type" => {
            let one_color = value == "One color";
            if one_color && !h.gradient_color.is_single_color {
                h.gradient_color.color_tint = 1.0;
            }
            h.gradient_color.is_single_color = one_color;
            return;
        }
        "gradient_type" => {
            use crate::scene::model::hatch_model::GradientKind;
            if let Some((kind, invert)) = GradientKind::from_choice_label(value) {
                h.gradient_color.name = kind.dxf_name(invert).to_string();
            }
            return;
        }
        "gradient_centered" => {
            let centered = if value == "toggle" {
                h.gradient_color.shift >= 0.5
            } else {
                value == "true"
            };
            h.gradient_color.shift = if centered { 0.0 } else { 1.0 };
            return;
        }
        _ => {}
    }
    let Some(v) = parse_f64(value) else {
        return;
    };
    match field {
        "pattern_angle" if h.gradient_color.enabled => {
            h.gradient_color.angle = v.to_radians();
            h.pattern_angle = v.to_radians();
        }
        "gradient_tint" if h.gradient_color.enabled => {
            h.gradient_color.color_tint = v.clamp(0.0, 1.0);
        }
        "pattern_angle" => {
            let angle = v.to_radians();
            let delta = angle - h.pattern_angle;
            rotate_pattern_geometry(&mut h.pattern, delta);
            h.pattern_angle = angle;
        }
        // The stored pattern lines are the FINAL rendered geometry (offsets,
        // base points and dashes in world units — see the prebaked path in
        // hatch_model_from_dxf), so changing the scale must rescale them by
        // the ratio or the edit has no visible effect (#415).
        "pattern_scale" if v > 0.0 => {
            let old = h.pattern_scale;
            if old > 1e-12 {
                let k = v / old;
                scale_pattern_geometry(&mut h.pattern, k);
            }
            h.pattern_scale = v;
        }
        // Scale every pattern line's offset so the first line's spacing = v,
        // preserving the relative spacing between lines.
        "spacing" if v > 0.0 => {
            h.pattern_scale = v;
            if matches!(
                h.pattern_type,
                acadrust::entities::HatchPatternType::UserDefined
            ) {
                h.pattern.lines.clear();
                h.pattern.name = "_USER".to_string();
            }
        }
        // Origin rows are relative offsets and return to zero after commit.
        "origin_x" => {
            for line in h.pattern.lines.iter_mut() {
                line.base_point.x += v;
            }
        }
        "origin_y" => {
            for line in h.pattern.lines.iter_mut() {
                line.base_point.y += v;
            }
        }
        "iso_pen_width" if v > 0.0 => {
            let old = h.pattern_scale;
            if old > 1e-12 {
                scale_pattern_geometry(&mut h.pattern, v / old);
            }
            h.pattern_scale = v;
        }
        "elevation" => h.elevation = v,
        _ => {}
    }
}

fn apply_transform(h: &mut Hatch, t: &EntityTransform) {
    crate::scene::view::transform::apply_standard_entity_transform(h, t, |entity, p1, p2| {
        // Keep boundary directions, angles, and sweeps consistent.
        let t = crate::scene::view::transform::reflection_about_xy_line(p1, p2);
        acadrust::entities::Entity::apply_transform(entity, &t);
    });
}

impl PropertyEditable for Hatch {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        properties(self)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for Hatch {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}

// ── Grip editing ───────────────────────────────────────────────────────────

/// Assign sequential grip IDs across all boundary paths and edges.
/// Exposed control points per edge type:
///   Polyline       → each vertex (x, y)
///   Line           → start, end
///   CircularArc    → center
///   EllipticArc    → center
///   Spline         → fit points if present, else control points (x, y)
impl Grippable for Hatch {
    fn grips(&self) -> Vec<GripDef> {
        let elev = self.elevation;
        let mut out = Vec::new();
        let mut id = 0usize;
        // Grip 0 = pattern-origin handle, shown at the hatch centroid (only when a
        // pattern exists). Dragging it moves the pattern tiling origin; boundary
        // grips follow from the next id.
        if let Some(l0) = self.pattern.lines.first() {
            let (gx, gy) =
                boundary_centroid(self).unwrap_or((l0.base_point.x, l0.base_point.y));
            out.push(circle_grip(id, glam::DVec3::new(gx, gy, elev)));
            id += 1;
        } else if self.is_associative {
            if let Some((gx, gy)) = boundary_centroid(self) {
                out.push(circle_grip(id, glam::DVec3::new(gx, gy, elev)));
                id += 1;
            }
        }
        // Edit associative boundaries through their source objects.
        if self.is_associative {
            return out;
        }
        for path in &self.paths {
            for edge in &path.edges {
                match edge {
                    BoundaryEdge::Polyline(p) => {
                        for v in &p.vertices {
                            out.push(center_grip(id, glam::DVec3::new(v.x, v.y, elev)));
                            id += 1;
                        }
                    }
                    BoundaryEdge::Line(l) => {
                        out.push(center_grip(
                            id,
                            glam::DVec3::new(l.start.x, l.start.y, elev),
                        ));
                        id += 1;
                        out.push(center_grip(id, glam::DVec3::new(l.end.x, l.end.y, elev)));
                        id += 1;
                    }
                    BoundaryEdge::CircularArc(a) => {
                        out.push(center_grip(
                            id,
                            glam::DVec3::new(a.center.x, a.center.y, elev),
                        ));
                        id += 1;
                    }
                    BoundaryEdge::EllipticArc(e) => {
                        out.push(center_grip(
                            id,
                            glam::DVec3::new(e.center.x, e.center.y, elev),
                        ));
                        id += 1;
                    }
                    BoundaryEdge::Spline(s) => {
                        let pts: Vec<[f64; 2]> = if !s.fit_points.is_empty() {
                            s.fit_points.iter().map(|p| [p.x, p.y]).collect()
                        } else {
                            s.control_points.iter().map(|p| [p.x, p.y]).collect()
                        };
                        for [x, y] in pts {
                            out.push(center_grip(id, glam::DVec3::new(x, y, elev)));
                            id += 1;
                        }
                    }
                }
            }
        }
        out
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        let elev = self.elevation as f32;

        fn resolve(apply: &GripApply, cur: Vec3) -> (f64, f64) {
            let p = match apply {
                GripApply::Absolute(p) => *p,
                GripApply::Translate(d) => cur.as_dvec3() + *d,
            };
            (p.x, p.y)
        }

        let mut id = 0usize;
        // Grip 0 = pattern origin: shift every line's base point by the delta,
        // preserving relative offsets (mirrors the origin_x / origin_y edits).
        if let Some((ox, oy)) = self
            .pattern
            .lines
            .first()
            .map(|l| (l.base_point.x, l.base_point.y))
        {
            if grip_id == id {
                let (gx, gy) = boundary_centroid(self).unwrap_or((ox, oy));
                let (dx, dy) = match apply {
                    GripApply::Absolute(point) => (point.x - gx, point.y - gy),
                    GripApply::Translate(delta) => (delta.x, delta.y),
                };
                for line in self.pattern.lines.iter_mut() {
                    line.base_point.x += dx;
                    line.base_point.y += dy;
                }
                return;
            }
            id += 1;
        }

        'outer: for path in &mut self.paths {
            for edge in &mut path.edges {
                match edge {
                    BoundaryEdge::Polyline(p) => {
                        for v in &mut p.vertices {
                            if id == grip_id {
                                let (nx, ny) =
                                    resolve(&apply, Vec3::new(v.x as f32, v.y as f32, elev));
                                v.x = nx;
                                v.y = ny;
                                break 'outer;
                            }
                            id += 1;
                        }
                    }
                    BoundaryEdge::Line(l) => {
                        if id == grip_id {
                            let (nx, ny) = resolve(
                                &apply,
                                Vec3::new(l.start.x as f32, l.start.y as f32, elev),
                            );
                            l.start.x = nx;
                            l.start.y = ny;
                            break 'outer;
                        }
                        id += 1;
                        if id == grip_id {
                            let (nx, ny) =
                                resolve(&apply, Vec3::new(l.end.x as f32, l.end.y as f32, elev));
                            l.end.x = nx;
                            l.end.y = ny;
                            break 'outer;
                        }
                        id += 1;
                    }
                    BoundaryEdge::CircularArc(a) => {
                        if id == grip_id {
                            let (nx, ny) = resolve(
                                &apply,
                                Vec3::new(a.center.x as f32, a.center.y as f32, elev),
                            );
                            a.center.x = nx;
                            a.center.y = ny;
                            break 'outer;
                        }
                        id += 1;
                    }
                    BoundaryEdge::EllipticArc(e) => {
                        if id == grip_id {
                            let (nx, ny) = resolve(
                                &apply,
                                Vec3::new(e.center.x as f32, e.center.y as f32, elev),
                            );
                            e.center.x = nx;
                            e.center.y = ny;
                            break 'outer;
                        }
                        id += 1;
                    }
                    BoundaryEdge::Spline(s) => {
                        if !s.fit_points.is_empty() {
                            for fp in &mut s.fit_points {
                                if id == grip_id {
                                    let (nx, ny) =
                                        resolve(&apply, Vec3::new(fp.x as f32, fp.y as f32, elev));
                                    fp.x = nx;
                                    fp.y = ny;
                                    break 'outer;
                                }
                                id += 1;
                            }
                        } else {
                            for cp in &mut s.control_points {
                                if id == grip_id {
                                    let (nx, ny) =
                                        resolve(&apply, Vec3::new(cp.x as f32, cp.y as f32, elev));
                                    cp.x = nx;
                                    cp.y = ny;
                                    break 'outer;
                                }
                                id += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    fn grip_menu(&self, grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        if self.pattern.lines.is_empty() || grip_id != 0 {
            return vec![GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            }];
        }
        vec![
            GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            },
            GripMenuItem {
                label: "Origin Point",
                action: GripMenuAction::OriginPoint,
            },
            GripMenuItem {
                label: "Hatch Angle",
                action: GripMenuAction::HatchAngle,
            },
            GripMenuItem {
                label: "Hatch Scale",
                action: GripMenuAction::HatchScale,
            },
        ]
    }

    fn apply_grip_menu(&mut self, grip_id: usize, action: crate::scene::model::object::GripMenuAction) {
        use crate::scene::model::object::GripMenuAction as A;
        if grip_id == 0 && matches!(action, A::OriginPoint) {
            if let (Some((gx, gy)), Some((ox, oy))) = (
                boundary_centroid(self),
                self.pattern.lines.first().map(|line| (line.base_point.x, line.base_point.y)),
            ) {
                translate_pattern_geometry(&mut self.pattern, gx - ox, gy - oy);
            }
        }
    }

    fn grip_menu_value_prompt(
        &self,
        _grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
    ) -> Option<&'static str> {
        use crate::scene::model::object::GripMenuAction as A;
        match action {
            A::HatchAngle => Some("Angle (deg)"),
            A::HatchScale => Some("Scale"),
            _ => None,
        }
    }

    fn apply_grip_menu_value(
        &mut self,
        _grip_id: usize,
        action: crate::scene::model::object::GripMenuAction,
        value: f64,
    ) {
        use crate::scene::model::object::GripMenuAction as A;
        match action {
            A::HatchAngle => {
                let angle = value.to_radians();
                let delta = angle - self.pattern_angle;
                rotate_pattern_geometry(&mut self.pattern, delta);
                self.pattern_angle = angle;
            }
            A::HatchScale => {
                if value > 0.0 {
                    if self.pattern_scale > 1e-12 {
                        let factor = value / self.pattern_scale;
                        scale_pattern_geometry(&mut self.pattern, factor);
                    }
                    self.pattern_scale = value;
                }
            }
            _ => {}
        }
    }
}

impl FallbackTess for Hatch {
    fn fallback_geometry(&self) -> FallbackGeometry {
        let normal = (self.normal.x, self.normal.y, self.normal.z);
        // Convert a 2D OCS hatch boundary point to absolute WCS.
        let to_wcs = |x: f64, y: f64| -> [f64; 3] {
            let (wx, wy, wz) =
                crate::scene::view::transform::ocs_point_to_wcs((x, y, self.elevation), normal);
            [wx, wy, wz]
        };
        // Snap point at a world (f64) location, cast to the f32 snap buffer.
        let snap_at = |w: [f64; 3]| Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32);
        let mut pts: Vec<[f64; 3]> = Vec::new();
        let mut key_verts: Vec<[f64; 3]> = Vec::new();
        let mut snap_pts: Vec<(Vec3, SnapHint)> = Vec::new();
        for path in &self.paths {
            for edge in &path.edges {
                let Some(curve) = edge_curve(edge) else {
                    continue;
                };
                let local = curve
                    .tessellate_angle(cadkernel::tessellation::DEFAULT_ANGLE);
                if local.len() < 2 {
                    continue;
                }
                if !pts.is_empty() {
                    pts.push([f64::NAN; 3]);
                }
                let world: Vec<[f64; 3]> = local
                    .into_iter()
                    .map(|point| to_wcs(point[0], point[1]))
                    .collect();
                match edge {
                    BoundaryEdge::Polyline(poly) => key_verts.extend(
                        poly.vertices
                            .iter()
                            .map(|point| to_wcs(point.x, point.y)),
                    ),
                    _ => key_verts.extend([world[0], *world.last().unwrap()]),
                }
                match edge {
                    BoundaryEdge::CircularArc(arc) => snap_pts.push((
                        snap_at(to_wcs(arc.center.x, arc.center.y)),
                        SnapHint::Center,
                    )),
                    BoundaryEdge::EllipticArc(ellipse) => snap_pts.push((
                        snap_at(to_wcs(ellipse.center.x, ellipse.center.y)),
                        SnapHint::Center,
                    )),
                    _ => {}
                }
                pts.extend(world);
            }
        }
        if pts.is_empty() {
            pts = vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        }
        (pts, snap_pts, vec![], key_verts)
    }
}
