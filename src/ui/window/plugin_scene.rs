//! Native projection of plugin panel scenes (api v29).
//!
//! Ownership split: the plugin owns the *semantic* scene — the
//! `ocs_plugin_api::scene::PanelScene` tree, what it means and holds; the
//! host owns the *projection*: flattening that validated tree into ordered
//! paint primitives and painting them on its own canvas. Plugin and host
//! share nothing but the serializable tree. This stage is deliberately
//! static: every frame re-projects the stored scene; the retained cache and
//! hit-testing are later stages.
//!
//! The projection is pure and iced-free (`project_scene` produces prims
//! that carry semantic colors); the theme resolves only inside the canvas
//! program's `draw`, the single place the host theme is consulted.

use crate::app::Message;
use iced::widget::canvas;
use iced::{Color, Element, Length, Point, Rectangle, Size, Theme, mouse};
use ocs_plugin_api::scene::{PanelScene, SceneColor, SceneElement, ThemeColor};

/// One flattened paint primitive, semantic colors unresolved. Paint order
/// is the projection order; groups flatten depth-first.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ScenePrim {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fill: Option<SceneColor>,
        stroke: Option<(SceneColor, f32)>,
    },
    Polyline {
        points: Vec<(f32, f32)>,
        closed: bool,
        stroke: Option<(SceneColor, f32)>,
    },
    Label {
        x: f32,
        y: f32,
        size: f32,
        content: String,
        color: SceneColor,
    },
}

/// Flatten a validated scene into paint-order prims. Groups carry no
/// transform (child coordinates stay scene-space, S01 invariant), so
/// flattening is a depth-first walk in paint order.
pub(crate) fn project_scene(scene: &PanelScene) -> Vec<ScenePrim> {
    let mut prims = Vec::new();
    project_elements(&scene.elements, &mut prims);
    prims
}

fn project_elements(elements: &[SceneElement], prims: &mut Vec<ScenePrim>) {
    for element in elements {
        match element {
            SceneElement::Group { children, .. } => project_elements(children, prims),
            SceneElement::Rect {
                pos,
                size,
                fill,
                stroke,
                ..
            } => prims.push(ScenePrim::Rect {
                x: pos.0[0] as f32,
                y: pos.0[1] as f32,
                w: size.0[0] as f32,
                h: size.0[1] as f32,
                fill: *fill,
                stroke: stroke.as_ref().map(|s| (s.color, s.width)),
            }),
            SceneElement::Path {
                points,
                closed,
                stroke,
                ..
            } => prims.push(ScenePrim::Polyline {
                points: points
                    .iter()
                    .map(|p| (p.0[0] as f32, p.0[1] as f32))
                    .collect(),
                closed: *closed,
                stroke: stroke.as_ref().map(|s| (s.color, s.width)),
            }),
            SceneElement::Text {
                pos,
                content,
                size,
                color,
                ..
            } => prims.push(ScenePrim::Label {
                x: pos.0[0] as f32,
                y: pos.0[1] as f32,
                size: *size,
                content: content.clone(),
                color: *color,
            }),
        }
    }
}

/// Semantic color -> host theme slot. Deterministic and pinned by tests so
/// iced palette drift breaks the pin instead of silently re-skinning
/// plugin panels.
pub(crate) fn scene_color(theme: &Theme, color: SceneColor) -> Color {
    let palette = theme.palette();
    match color {
        SceneColor::Theme(ThemeColor::Normal | ThemeColor::Strong) => palette.background.base.text,
        SceneColor::Theme(ThemeColor::Accent) => palette.primary.base.color,
        SceneColor::Theme(ThemeColor::Muted) => crate::ui::style::readable::muted_text(theme),
        SceneColor::Theme(ThemeColor::Success) => palette.success.base.color,
        SceneColor::Theme(ThemeColor::Warning) => palette.warning.base.color,
        SceneColor::Theme(ThemeColor::Danger) => palette.danger.base.color,
        SceneColor::Rgba(rgba) => Color::from_rgba8(
            rgba.0[0],
            rgba.0[1],
            rgba.0[2],
            f32::from(rgba.0[3]) / 255.0,
        ),
    }
}

/// A canvas that paints one projected scene. Static stage: `draw` repaints
/// the flat prim list each invocation; no cache (S03 adds the retained
/// projection keyed by render inputs).
pub(crate) struct SceneCanvas {
    pub prims: Vec<ScenePrim>,
}

impl canvas::Program<Message> for SceneCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for prim in &self.prims {
            paint_prim(&mut frame, theme, bounds, prim);
        }
        vec![frame.into_geometry()]
    }
}

fn paint_prim(frame: &mut canvas::Frame, theme: &Theme, _bounds: Rectangle, prim: &ScenePrim) {
    match prim {
        ScenePrim::Rect {
            x,
            y,
            w,
            h,
            fill,
            stroke,
        } => {
            let origin = Point::new(*x, *y);
            let size = Size::new(*w, *h);
            if let Some(color) = fill {
                frame.fill_rectangle(origin, size, scene_color(theme, *color));
            }
            if let Some((color, width)) = stroke {
                frame.stroke_rectangle(
                    origin,
                    size,
                    canvas::Stroke::default()
                        .with_color(scene_color(theme, *color))
                        .with_width(*width),
                );
            }
        }
        ScenePrim::Polyline {
            points,
            closed,
            stroke,
        } => {
            if points.len() < 2 {
                return;
            }
            if let Some((color, width)) = stroke {
                let path = canvas::Path::new(|builder| {
                    let mut pts = points.iter();
                    if let Some((x, y)) = pts.next() {
                        builder.move_to(Point::new(*x, *y));
                    }
                    for (x, y) in pts {
                        builder.line_to(Point::new(*x, *y));
                    }
                    if *closed {
                        builder.close();
                    }
                });
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(scene_color(theme, *color))
                        .with_width(*width),
                );
            }
        }
        ScenePrim::Label {
            x,
            y,
            size,
            content,
            color,
        } => {
            frame.fill_text(canvas::Text {
                content: content.clone().into(),
                position: Point::new(*x, *y),
                color: scene_color(theme, *color),
                size: (*size).into(),
                ..canvas::Text::default()
            });
        }
    }
}

/// The panel body seam: one canvas element sized to the panel body.
pub(crate) fn scene_view<'a>(scene: &PanelScene) -> Element<'a, Message> {
    let prims = project_scene(scene);
    iced::widget::canvas(SceneCanvas { prims })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_plugin_api::scene::{Rect2D, StableId, Stroke, Vec2};

    fn rect(id: u64, x: f64, y: f64, w: f64, h: f64, fill: Option<SceneColor>) -> SceneElement {
        SceneElement::Rect {
            id: StableId(id),
            name: None,
            pos: Vec2([x, y]),
            size: Vec2([w, h]),
            fill,
            stroke: None,
        }
    }

    /// Paint order: roots in Vec order, groups depth-first, children in
    /// order. Semantic colors ride the prim unresolved.
    #[test]
    fn projection_preserves_paint_order_and_semantic_colors() {
        let scene = PanelScene {
            revision: ocs_plugin_api::scene::SceneRevision(1),
            elements: vec![
                rect(
                    1,
                    0.0,
                    0.0,
                    10.0,
                    10.0,
                    Some(SceneColor::Theme(ThemeColor::Normal)),
                ),
                SceneElement::Group {
                    id: StableId(2),
                    name: None,
                    children: vec![rect(
                        3,
                        1.0,
                        1.0,
                        2.0,
                        2.0,
                        Some(SceneColor::Theme(ThemeColor::Accent)),
                    )],
                },
                SceneElement::Text {
                    id: StableId(4),
                    name: None,
                    content: "hi".to_owned(),
                    pos: Vec2([2.0, 2.0]),
                    size: 12.0,
                    color: SceneColor::Rgba(ocs_plugin_api::scene::Rgba8([255, 0, 0, 255])),
                },
            ],
        };
        let prims = project_scene(&scene);
        assert_eq!(prims.len(), 3, "groups flatten, not paint");
        assert!(matches!(
            &prims[0],
            ScenePrim::Rect {
                fill: Some(SceneColor::Theme(ThemeColor::Normal)),
                ..
            }
        ));
        assert!(matches!(
            &prims[1],
            ScenePrim::Rect {
                fill: Some(SceneColor::Theme(ThemeColor::Accent)),
                ..
            }
        ));
        assert!(
            matches!(&prims[2], ScenePrim::Label { content, color: SceneColor::Rgba(_), .. } if content == "hi")
        );
    }

    /// The theme mapping is deterministic per slot (pins iced palette
    /// drift; mirrors the dialog TextStyle mapping spirit).
    #[test]
    fn scene_color_maps_each_semantic_slot_deterministically() {
        let rgba = SceneColor::Rgba(ocs_plugin_api::scene::Rgba8([255, 0, 0, 255]));
        for theme in [Theme::Light, Theme::Dark] {
            let palette = theme.palette();
            assert_eq!(
                scene_color(&theme, SceneColor::Theme(ThemeColor::Normal)),
                palette.background.base.text
            );
            assert_eq!(
                scene_color(&theme, SceneColor::Theme(ThemeColor::Accent)),
                palette.primary.base.color
            );
            assert_eq!(
                scene_color(&theme, SceneColor::Theme(ThemeColor::Muted)),
                crate::ui::style::readable::muted_text(&theme)
            );
            assert_eq!(
                scene_color(&theme, SceneColor::Theme(ThemeColor::Warning)),
                palette.warning.base.color
            );
            assert_eq!(
                scene_color(&theme, SceneColor::Theme(ThemeColor::Danger)),
                palette.danger.base.color
            );
            assert_eq!(scene_color(&theme, rgba), Color::from_rgb8(255, 0, 0));
        }
    }
}
