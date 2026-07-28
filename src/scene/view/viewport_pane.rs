//! Unified GPU shader widget for both model and paper layouts.
//!
//! There is one `shader::Program` for the whole canvas. `Scene::build_viewports`
//! decides what to render — model layout produces one viewport per tile
//! (`active_viewports` iterates `model_tiles`), paper layout produces one per
//! content viewport entity. Each one is drawn into its own scissor rect by a
//! dedicated inner `Pipeline` (kept in the `MultiPipeline` outer).

use super::render::{CameraState, Primitive};
use crate::scene::Scene;
use iced::widget::shader;
use iced::{mouse, Event, Rectangle};

// ── Widget struct ─────────────────────────────────────────────────────────

pub struct ViewportPane<'a> {
    pub scene: &'a Scene,
    pub show_viewcube: bool,
    /// Render mode applied to the Model layout's tiles. Paper-space content
    /// viewports use the render mode stored on their own viewport entity.
    pub render_mode: acadrust::entities::ViewportRenderMode,
    /// `Some(tile_idx)` → this widget renders a single Model pane (one shader
    /// per `pane_grid` pane, filling its own bounds). `None` → the unified
    /// full-canvas path (paper layout, or the whole-canvas Model fallback).
    pub pane: Option<usize>,
    /// Active Iced theme text colour, forwarded to the GPU-rendered ViewCube
    /// face and compass labels.
    pub viewcube_text_color: [f32; 4],
}

impl<'a> ViewportPane<'a> {
    pub fn model(
        scene: &'a Scene,
        show_viewcube: bool,
        render_mode: acadrust::entities::ViewportRenderMode,
        viewcube_text_color: [f32; 4],
    ) -> Self {
        Self {
            scene,
            show_viewcube,
            render_mode,
            pane: None,
            viewcube_text_color,
        }
    }

    /// One Model `pane_grid` pane: renders just `tile_idx` into this widget's
    /// own bounds (the pane rectangle).
    pub fn for_pane(
        scene: &'a Scene,
        show_viewcube: bool,
        render_mode: acadrust::entities::ViewportRenderMode,
        tile_idx: usize,
        viewcube_text_color: [f32; 4],
    ) -> Self {
        Self {
            scene,
            show_viewcube,
            render_mode,
            pane: Some(tile_idx),
            viewcube_text_color,
        }
    }
}

// ── shader::Program impl ──────────────────────────────────────────────────

impl<'a, Msg: std::fmt::Debug + Clone> shader::Program<Msg> for ViewportPane<'a> {
    type State = CameraState;
    type Primitive = Primitive;

    fn draw(
        &self,
        state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        match self.pane {
            Some(idx) => self.scene.build_viewport_for_pane(
                bounds,
                idx,
                self.render_mode,
                self.show_viewcube,
                self.viewcube_text_color,
            ),
            None => self.scene.build_viewports(
                bounds,
                self.render_mode,
                state.hover_region,
                self.show_viewcube,
                self.viewcube_text_color,
            ),
        }
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<Msg>> {
        // ViewCube hover is also driven from the app-level CursorMoved /
        // ViewportMove handlers (the cube hit-area overlay shadows this
        // widget for those events). Keeping the call here gives a fallback
        // path while the cursor is over the bare shader.
        if self.show_viewcube {
            self.scene.update_viewcube_state(state, bounds, cursor);
        } else {
            state.hover_region = None;
        }
        let _ = event;
        None
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        _b: Rectangle,
        _c: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.show_viewcube {
            self.scene.viewcube_mouse_interaction(state)
        } else {
            mouse::Interaction::default()
        }
    }
}
