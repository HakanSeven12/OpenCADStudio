use super::*;
use super::super::util::*;

impl OpenCADStudio {
    /// The colour a drawing command's new object will be drawn in, for its
    /// preview; `None` for any other command.
    pub(in crate::app) fn creation_preview_color(&self, i: usize) -> Option<[f32; 4]> {
        const DRAWING: &[&str] = &[
            "LINE",
            "PLINE",
            "RECT",
            "CIRCLE",
            "ARC",
            "ELLIPSE",
            "ELLIPSE_CENTER",
            "POLY",
            "SPLINE",
            "XLINE",
            "RAY",
            "POINT",
            "MULTIPOINT",
            "DONUT",
            "REVCLOUD",
            "3DPOLY",
            "HELIX",
            "MLINE",
            "SKETCH",
            "TRACE",
            "SOLID",
            "3DFACE",
        ];
        let tab = &self.tabs[i];
        let name = tab.active_cmd.as_ref()?.name();
        let base = name.split('_').next().unwrap_or(name);
        if !DRAWING.contains(&base) && !name.starts_with("SPLINE") {
            return None;
        }
        let document = &tab.scene.document;
        let color = match document.header.current_entity_color {
            document::types::Color::ByLayer => document.layers.get(&tab.active_layer)?.color,
            document::types::Color::ByBlock => document::types::Color::Index(7),
            color => color,
        };
        let rgba = crate::scene::convert::tess_util::aci_to_rgba(&color);
        Some(crate::scene::view::render::adapt_to_bg(
            rgba,
            tab.scene.bg_color,
        ))
    }

    /// A drawing command previews the new object in the colour it will get:
    /// the current colour, or the current layer's when ByLayer. Both passes
    /// that rebuild a command preview call this: the full pointer-move pass
    /// and `refresh_active_cmd_preview`, which the snap dwell tick runs on
    /// the frames between full passes while the pointer moves. Without it on
    /// either one, the preview would switch colour from frame to frame.
    pub(in crate::app) fn tint_creation_preview(&self, i: usize, previews: &mut [crate::scene::WireModel]) {
        let Some(color) = self.creation_preview_color(i) else {
            return;
        };
        for wire in previews {
            if wire.color == crate::scene::WireModel::CYAN {
                wire.color = color;
            }
        }
    }

    /// The navigation style's action for `held` and the modifiers now down.
    pub(in crate::app) fn nav_style_action(
        &self,
        held: crate::app::settings::NavButtons,
        dragging: bool,
    ) -> Option<crate::app::settings::NavAction> {
        self.navigation_style.action(
            held,
            self.shift_down,
            self.ctrl_down,
            self.alt_down,
            dragging,
        )
    }

    /// What the navigation drag under way does: an armed tool decides, then
    /// CATIA's zoom latch, then the style for the buttons held. A drag the
    /// style has no meaning for pans.
    pub(in crate::app) fn nav_drag_action(&self, i: usize) -> crate::app::settings::NavAction {
        use crate::app::settings::NavAction;
        let tab = &self.tabs[i];
        if tab.zoom_dynamic_mode {
            NavAction::Zoom
        } else if tab.orbit_mode {
            NavAction::Orbit
        } else if tab.pan_mode {
            NavAction::Pan
        } else if self.nav_zoom_latched {
            NavAction::Zoom
        } else {
            self.nav_style_action(self.nav_held, true)
                .unwrap_or(NavAction::Pan)
        }
    }

    /// Starts the drag a pending left/right press turns into once it moves,
    /// and the modifier-held drag of the Touchpad style (ended when its keys
    /// are up).
    pub(in crate::app) fn nav_pointer_moved(&mut self, i: usize, p: Point) {
        use crate::app::settings::{NavButtons, NavigationStyle};
        if let Some((_, from, _)) = &self.nav_pending {
            let (dx, dy) = (p.x - from.x, p.y - from.y);
            if dx * dx + dy * dy > 16.0 {
                let from = *from;
                self.nav_pending = None;
                self.nav_start_drag(i, from);
            }
            return;
        }
        if self.navigation_style != NavigationStyle::Touchpad
            || self.nav_held != NavButtons::default()
        {
            return;
        }
        let keyed = self
            .nav_style_action(NavButtons::default(), false)
            .is_some();
        let (dragging, previous) = {
            let sel = self.tabs[i].scene.selection.borrow();
            (sel.input.middle_down, sel.input.last_move_pos)
        };
        if keyed && !dragging {
            if let Some(previous) = previous {
                self.nav_start_drag(i, previous);
            }
        } else if !keyed && dragging && self.nav_drag_by_button {
            self.nav_end_drag(i);
        }
    }

    /// Run a navigation drag from `from` on the middle-button movement path,
    /// dropping whatever the left or right button had begun.
    pub(in crate::app) fn nav_start_drag(&mut self, i: usize, from: Point) {
        self.clear_navigation_hover(i);
        self.tabs[i].scene.remember_current_view();
        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.clear_left_selection_gesture();
        sel.input.left_down = false;
        sel.input.right_down = false;
        sel.input.middle_down = true;
        sel.input.middle_last_pos = Some(from);
        sel.input.middle_pan_from = None;
        drop(sel);
        self.nav_drag_by_button = true;
    }

    /// Drops every pointer navigation in flight. Its release can go missing
    /// when the window loses focus or the drawing it began in is switched
    /// away or closed, leaving the view following the pointer (#3195).
    pub(in crate::app) fn release_pointer_navigation(&mut self) {
        for tab in &self.tabs {
            let mut sel = tab.scene.selection.borrow_mut();
            sel.input.middle_down = false;
            sel.input.middle_last_pos = None;
            sel.orbit.pivot = None;
        }
        self.nav_held = Default::default();
        self.nav_claimed = Default::default();
        self.nav_pending = None;
        self.nav_middle_press = None;
        self.nav_drag_by_button = false;
        self.nav_zoom_latched = false;
    }

    pub(in crate::app) fn nav_end_drag(&mut self, i: usize) {
        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.input.middle_down = false;
        sel.input.middle_last_pos = None;
        sel.orbit.pivot = None;
        drop(sel);
        self.nav_drag_by_button = false;
        self.nav_zoom_latched = false;
        self.arm_hover_after_navigation(i);
    }

    /// A left (`left`) or right press: the navigation style takes it when
    /// the buttons now held drag the view. A press that may still be a
    /// click waits for the pointer to move. `None` lets the press run its
    /// normal path.
    pub(in crate::app) fn nav_press(
        &mut self,
        left: bool,
        press: &Message,
    ) -> Option<Task<Message>> {
        self.nav_resync_held();
        if left {
            self.nav_held.left = true;
        } else {
            self.nav_held.right = true;
        }
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.orbit_mode || tab.pan_mode || tab.zoom_dynamic_mode || tab.is_start {
            return None;
        }
        let (pos, dragging) = {
            let sel = tab.scene.selection.borrow();
            (sel.input.last_move_pos?, sel.input.middle_down)
        };
        let held = self.nav_held;
        if dragging {
            self.nav_style_action(held, true)?;
            if !left && held.middle {
                self.nav_right_press = Some(pos);
            }
        } else {
            self.nav_style_action(held, false)?;
            let other_held = if left { held.right } else { held.left };
            if other_held {
                // Both buttons together navigate at once; whatever the
                // other one had begun gives way.
                if left {
                    self.nav_claimed.right = true;
                } else {
                    self.nav_claimed.left = true;
                }
                self.nav_pending = None;
                self.nav_start_drag(i, pos);
            } else {
                self.nav_pending = Some((left, pos, press.clone()));
            }
        }
        if left {
            self.nav_claimed.left = true;
        } else {
            self.nav_claimed.right = true;
        }
        Some(Task::none())
    }

    /// A left (`left`) or right release. Navigation keeps the release of a
    /// press it took: a press that never moved is replayed as the click it
    /// was. `None` lets the release run its normal path.
    pub(in crate::app) fn nav_release(&mut self, left: bool) -> Option<Task<Message>> {
        use crate::app::settings::NavigationStyle;
        if left {
            self.nav_held.left = false;
        } else {
            self.nav_held.right = false;
        }
        let claimed = if left {
            &mut self.nav_claimed.left
        } else {
            &mut self.nav_claimed.right
        };
        if !std::mem::take(claimed) {
            return None;
        }
        let i = self.active_tab;
        if let Some((_, _, press)) = self.nav_pending.take_if(|(l, _, _)| *l == left) {
            let release = if left {
                Message::ViewportLeftRelease
            } else {
                Message::ViewportRightRelease
            };
            self.nav_replay = true;
            let tasks = [self.update(press), self.update(release)];
            self.nav_replay = false;
            return Some(Task::batch(tasks));
        }
        let (at, dragging) = {
            let sel = self.tabs[i].scene.selection.borrow();
            (sel.input.last_move_pos, sel.input.middle_down)
        };
        // CATIA: a right click while the middle button drags turns it into
        // a zoom.
        if let (false, Some(from), Some(at)) = (left, self.nav_right_press.take(), at) {
            let (dx, dy) = (at.x - from.x, at.y - from.y);
            if self.navigation_style == NavigationStyle::Catia
                && self.nav_held.middle
                && dx * dx + dy * dy < 9.0
            {
                self.nav_zoom_latched = true;
            }
        }
        if dragging
            && self.nav_drag_by_button
            && self.nav_style_action(self.nav_held, true).is_none()
        {
            self.nav_end_drag(i);
            return Some(self.refresh_pointer_after_navigation(i));
        }
        Some(Task::none())
    }

    /// Forget a left or right button whose release never arrived (let go
    /// outside the window): one is held only while its normal press is
    /// down or navigation holds its release.
    pub(in crate::app) fn nav_resync_held(&mut self) {
        let sel = self.tabs[self.active_tab].scene.selection.borrow();
        self.nav_held.left &= sel.input.left_down || self.nav_claimed.left;
        self.nav_held.right &= sel.input.right_down || self.nav_claimed.right;
    }

    /// Run a press or release the navigation style passed on through its
    /// normal handler.
    pub(in crate::app) fn update_navigation_passthrough(
        &mut self,
        message: Message,
    ) -> Task<Message> {
        self.nav_replay = true;
        let task = self.update(message);
        self.nav_replay = false;
        task
    }

    /// Bring the point under `p` to the centre of the model view.
    pub(in crate::app) fn nav_recenter_at(&mut self, i: usize, p: Point) {
        let scene = &self.tabs[i].scene;
        if scene.active_viewport.is_some() || scene.current_layout != "Model" {
            return;
        }
        let (width, height) = scene.selection.borrow().view.vp_size;
        let bounds = scene.active_model_tile_bounds(width, height);
        let local = Point::new(p.x - bounds.x, p.y - bounds.y);
        let local_bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: bounds.width,
            height: bounds.height,
        };
        {
            let mut camera = scene.camera.borrow_mut();
            let target = camera.pick_on_target_plane(local, local_bounds);
            if target.is_finite() {
                camera.target = target;
            }
        }
        self.tabs[i].scene.camera_generation += 1;
    }

    pub(crate) fn on_viewport_exit(&mut self) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].measurement.live_visible = false;
        self.hover_dwell = None;
        self.snap_dwell = None;
        if self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.entity_pick_deferred_hover())
        {
            self.tabs[i].scene.clear_preview_wire();
        }
        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.input.pointer_away = true;
        if crate::perf::enabled() {
            crate::perf_record_diagnostic!(
                "[perf] viewport-exit  left_down={} box_anchor={:?} dragging={}",
                sel.input.left_down,
                sel.gesture.box_anchor,
                sel.input.left_dragging,
            );
        }
        // A selection box or fence dragged over the command line, a panel or
        // past the window edge keeps going: the input layer reports its moves
        // and its release out there. A click-move-click box waiting for its
        // second corner keeps going too — its button is up, so `left_down`
        // alone would drop a selection the user has not finished or cancelled
        // (#3339). Only a pointer with no selection gesture in flight clears.
        let pending_box = sel.gesture.box_anchor.is_some() || sel.gesture.poly_active;
        let left_drag = sel.input.left_down || pending_box;
        if !sel.input.left_down {
            sel.input.left_press_pos = None;
            sel.input.left_press_time = None;
            sel.input.left_dragging = false;
        }
        sel.input.right_down = false;
        sel.input.right_press_pos = None;
        sel.input.right_press_time = None;
        sel.input.right_last_pos = None;
        sel.input.right_dragging = false;
        sel.input.right_click_entered = false;
        // A pan or orbit keeps following the pointer outside the viewport;
        // its input layer reports the moves and the release there.
        if !sel.input.middle_down {
            sel.input.middle_down = false;
            sel.input.middle_last_pos = None;
            sel.orbit.pivot = None;
        }
        if !left_drag {
            sel.gesture.box_anchor = None;
            sel.gesture.box_anchor_world = None;
            sel.gesture.box_current = None;
            sel.gesture.box_crossing = false;
            sel.gesture.box_crossing_locked = false;
            sel.gesture.poly_active = false;
            sel.gesture.poly_points.clear();
            sel.gesture.poly_crossing = false;
        }
        drop(sel);
        if !left_drag {
            self.box_preview_pending = None;
            self.box_preview_due = None;
            self.box_preview_cache = None;
            self.tabs[i].scene.set_box_hover_highlights(&[]);
        }
        // Clear the rollover highlight when the cursor leaves the
        // viewport so it doesn't stick while the mouse is over the
        // ribbon / panels.
        self.tabs[i].scene.set_hover_highlight(None);
        self.tabs[i].scene.set_constraint_hover_highlights(&[]);
        // Don't touch `context_menu` here. ViewportExit also fires
        // when an upper overlay (the right-click menu panel) takes
        // the cursor, so clearing the menu state on every exit
        // would close the menu the moment it opens. Outside-click
        // dismiss is handled in `ViewportLeftPress`.
        Task::none()
    }

}
