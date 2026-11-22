//! Native projection of plugin panel scenes (api v29): the plugin owns the
//! `PanelScene` tree, the host flattens it into paint primitives
//! (`project_scene`, without iced) and paints them on its canvas, resolving
//! theme colors only in `draw`.

use crate::app::Message;
use iced::widget::canvas;
use iced::{Color, Element, Length, Point, Rectangle, Size, Theme, mouse};
use ocs_plugin_api::scene::{PanelScene, SceneColor, SceneElement, ThemeColor};
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    /// Retained projection cache (api v29, S03): the projected prim list
    /// for the LAST rendered (owner, panel, scene revision). Keyed by the
    /// actual render inputs — a revision identifies scene content (the
    /// store only advances revisions through validated publishes/patches),
    /// and the theme applies at paint time so it never invalidates the
    /// cache. Bounded by design: one entry per thread; a newer revision of
    /// any panel replaces it (patched-away revisions are dead state), so
    /// the cache never grows with panel count or history.
    static PROJECTION_CACHE: RefCell<Option<((String, String, u64), Arc<Vec<ScenePrim>>)>> =
        const { RefCell::new(None) };
}

/// Number of times the cache re-projected a scene (test hook: proves
/// repaints are free and a patch costs exactly one re-projection).
#[cfg(test)]
thread_local! {
    static PROJECT_COUNT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Drop the retained projection: lifecycle callers (panel
/// re-registration, the dead-plugin reap) keep a dead scene's prims from
/// ever serving a re-registered panel that reuses the revision. The
/// cache is one bounded entry, so clearing is O(1) and correct.
pub(crate) fn forget_all_projections() {
    PROJECTION_CACHE.with(|cache| {
        *cache.borrow_mut() = None;
    });
}

fn cached_prims(owner: &str, panel: &str, scene: &PanelScene) -> Arc<Vec<ScenePrim>> {
    PROJECTION_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((key, prims)) = cache.as_ref() {
            if key.0 == owner && key.1 == panel && key.2 == scene.revision.0 {
                return Arc::clone(prims);
            }
        }
        #[cfg(test)]
        PROJECT_COUNT.with(|count| count.set(count.get() + 1));
        let prims = Arc::new(project_scene(scene));
        *cache = Some((
            (owner.to_owned(), panel.to_owned(), scene.revision.0),
            Arc::clone(&prims),
        ));
        prims
    })
}

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
    pub prims: Arc<Vec<ScenePrim>>,
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
        for prim in self.prims.iter() {
            paint_prim(&mut frame, theme, prim);
        }
        vec![frame.into_geometry()]
    }
}

fn paint_prim(frame: &mut canvas::Frame, theme: &Theme, prim: &ScenePrim) {
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

/// The panel body seam: one canvas element sized to the panel body,
/// painting the retained projection for (owner, panel, revision).
pub(crate) fn scene_view<'a>(owner: &str, panel: &str, scene: &PanelScene) -> Element<'a, Message> {
    let prims = cached_prims(owner, panel, scene);
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

    /// S03 retained-cache proof: repaints of an unchanged scene are FREE
    /// (zero re-projections), and a patch — even one touching a single
    /// subtree — costs exactly ONE re-projection at the new revision,
    /// never one per frame. The single bounded entry is replaced by the
    /// next panel that renders (patched-away revisions are dead state).
    #[test]
    fn retained_cache_reprojects_once_per_revision_not_per_frame() {
        let small = PanelScene {
            revision: ocs_plugin_api::scene::SceneRevision(1),
            elements: vec![rect(1, 0.0, 0.0, 10.0, 10.0, None)],
        };
        let patched = PanelScene {
            revision: ocs_plugin_api::scene::SceneRevision(2),
            elements: vec![
                rect(1, 0.0, 0.0, 10.0, 10.0, None),
                rect(
                    2,
                    20.0,
                    0.0,
                    5.0,
                    5.0,
                    Some(SceneColor::Theme(ThemeColor::Accent)),
                ),
            ],
        };
        PROJECT_COUNT.with(|count| count.set(0));

        let _ = cached_prims("acme", "acme:graph", &small);
        for _ in 0..5 {
            let _ = cached_prims("acme", "acme:graph", &small);
        }
        PROJECT_COUNT.with(|count| assert_eq!(count.get(), 1, "repaints must be cache hits"));

        let _ = cached_prims("acme", "acme:graph", &patched);
        for _ in 0..5 {
            let _ = cached_prims("acme", "acme:graph", &patched);
        }
        PROJECT_COUNT
            .with(|count| assert_eq!(count.get(), 2, "a patch costs exactly one re-projection"));

        let _ = cached_prims("other", "other:map", &small);
        PROJECT_COUNT.with(|count| assert_eq!(count.get(), 3, "panel switch replaces the entry"));
        let _ = cached_prims("acme", "acme:graph", &patched);
        PROJECT_COUNT.with(|count| assert_eq!(count.get(), 4, "evicted entry re-projects once"));
    }
}
