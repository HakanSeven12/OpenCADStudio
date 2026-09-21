use super::OpenCADStudio;
use crate::ui::window::drafting_settings::{DraftingSettingsState, DraftingSettingsTab};

impl DraftingSettingsState {
    pub(crate) fn from_app(app: &OpenCADStudio) -> Self {
        let snap_x = app.snapper.snap_spacing_x;
        let snap_y = app.snapper.snap_spacing_y;
        // The dialog has no stored "equal" flag, so infer it: equal when
        // X and Y match. This keeps the checkbox checked for the common
        // case (including fresh defaults of 10/10).
        let snap_equal = (snap_x - snap_y).abs() < 1e-6;
        Self {
            active_tab: DraftingSettingsTab::SnapAndGrid,
            snap_on: app.snapper.grid_snap(),
            grid_on: app.show_grid,
            snap_x_input: crate::ui::window::drafting_settings::format_snap_spacing(snap_x),
            snap_y_input: crate::ui::window::drafting_settings::format_snap_spacing(snap_y),
            snap_equal,
            grid_x_input: crate::ui::window::drafting_settings::format_snap_spacing(
                app.grid_spacing_x,
            ),
            grid_y_input: crate::ui::window::drafting_settings::format_snap_spacing(
                app.grid_spacing_y,
            ),
            grid_major_input: format!("{}", app.grid_major_every),
            grid_adaptive: app.grid_adaptive,
            grid_beyond_limits: app.grid_beyond_limits,
            isometric: app.isometric_drafting,
            iso_plane: app.iso_plane,
            snap_angle_deg: app.snap_angle_deg,
            polar_on: app.polar_mode,
            ortho_on: app.ortho_mode,
            polar_increment_deg: app.polar_increment_deg,
            osnap_on: app.snapper.snap_enabled,
            otrack_on: app.snapper.otrack_enabled,
            snap_modes: app.snapper.enabled.clone(),
            osnap3d_on: app.snapper.snap3d_enabled,
            snap3d_modes: app.snapper.enabled3d.clone(),
            dyn_input_on: app.dyn_input,
            quick_props_on: app.quick_properties,
            selection_cycling_on: app.selection_cycling,
        }
    }
}

impl OpenCADStudio {
    pub(super) fn drafting_settings_dirty(&self) -> bool {
        match (&self.drafting_settings_state, &self.drafting_settings_saved) {
            (Some(curr), Some(saved)) => curr.is_dirty(saved),
            (Some(_), None) => true,
            _ => false,
        }
    }

    pub(super) fn apply_drafting_settings(&mut self) -> bool {
        let Some(state) = &self.drafting_settings_state.clone() else {
            return true;
        };
        let x = crate::ui::window::drafting_settings::parse_snap_spacing(&state.snap_x_input);
        let y = crate::ui::window::drafting_settings::parse_snap_spacing(&state.snap_y_input);
        let (Some(sx), Some(sy)) = (x, y) else {
            self.command_line
                .push_error(crate::t!("Snap X and Y spacings must be positive numbers.").as_ref());
            return false;
        };
        // When locked, Y follows X so the two can never diverge.
        let sy = if state.snap_equal { sx } else { sy };
        if let Some(live) = &mut self.drafting_settings_state {
            if live.snap_equal {
                live.snap_y_input = live.snap_x_input.clone();
            }
        }
        let gx = crate::ui::window::drafting_settings::parse_snap_spacing(&state.grid_x_input);
        let gy = crate::ui::window::drafting_settings::parse_snap_spacing(&state.grid_y_input);
        let gm = crate::ui::window::drafting_settings::parse_grid_major(&state.grid_major_input);
        let (Some(gx), Some(gy), Some(gm)) = (gx, gy, gm) else {
            self.command_line.push_error(
                crate::t!("Grid X/Y spacings must be positive numbers and Major every 2-100.")
                    .as_ref(),
            );
            return false;
        };
        self.show_grid = state.grid_on;
        self.grid_spacing_x = gx;
        self.grid_spacing_y = gy;
        self.grid_major_every = gm;
        self.grid_adaptive = state.grid_adaptive;
        self.grid_beyond_limits = state.grid_beyond_limits;
        self.snapper.grid_snap_on = state.snap_on;
        self.snapper.snap_spacing_x = sx;
        self.snapper.snap_spacing_y = sy;
        self.isometric_drafting = state.isometric;
        self.iso_plane = state.iso_plane;
        self.snap_angle_deg = state.snap_angle_deg;
        self.polar_mode = state.polar_on;
        self.ortho_mode = state.ortho_on;
        self.polar_increment_deg = state.polar_increment_deg;
        self.snapper.snap_enabled = state.osnap_on;
        self.snapper.otrack_enabled = state.otrack_on;
        self.snapper.enabled = state.snap_modes.clone();
        self.snapper.snap3d_enabled = state.osnap3d_on;
        self.snapper.enabled3d = state.snap3d_modes.clone();
        self.dyn_input = state.dyn_input_on;
        self.quick_properties = state.quick_props_on;
        self.selection_cycling = state.selection_cycling_on;
        self.sync_vport_display(self.active_tab);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drafting_settings_tab_switching_not_dirty() {
        let state1 = DraftingSettingsState {
            active_tab: DraftingSettingsTab::SnapAndGrid,
            snap_on: false,
            grid_on: true,
            snap_x_input: "10".to_string(),
            snap_y_input: "10".to_string(),
            snap_equal: true,
            grid_x_input: "10".to_string(),
            grid_y_input: "10".to_string(),
            grid_major_input: "5".to_string(),
            grid_adaptive: true,
            grid_beyond_limits: true,
            isometric: false,
            iso_plane: crate::app::settings::IsoPlane::Left,
            snap_angle_deg: 0.0,
            polar_on: false,
            ortho_on: false,
            polar_increment_deg: 15.0,
            osnap_on: true,
            otrack_on: false,
            snap_modes: rustc_hash::FxHashSet::default(),
            osnap3d_on: false,
            snap3d_modes: rustc_hash::FxHashSet::default(),
            dyn_input_on: false,
            quick_props_on: false,
            selection_cycling_on: false,
        };
        let mut state2 = state1.clone();
        assert!(!state2.is_dirty(&state1));

        // Switching tabs should NOT mark it dirty
        state2.active_tab = DraftingSettingsTab::ObjectSnap;
        assert!(!state2.is_dirty(&state1));

        // Toggling a setting should mark it dirty
        state2.grid_on = false;
        assert!(state2.is_dirty(&state1));
    }

    #[test]
    fn test_drafting_settings_dirty_toggles() {
        let base = DraftingSettingsState {
            active_tab: DraftingSettingsTab::SnapAndGrid,
            snap_on: false,
            grid_on: true,
            snap_x_input: "10".to_string(),
            snap_y_input: "10".to_string(),
            snap_equal: true,
            grid_x_input: "10".to_string(),
            grid_y_input: "10".to_string(),
            grid_major_input: "5".to_string(),
            grid_adaptive: true,
            grid_beyond_limits: true,
            isometric: false,
            iso_plane: crate::app::settings::IsoPlane::Left,
            snap_angle_deg: 0.0,
            polar_on: false,
            ortho_on: false,
            polar_increment_deg: 15.0,
            osnap_on: true,
            otrack_on: false,
            snap_modes: rustc_hash::FxHashSet::default(),
            osnap3d_on: false,
            snap3d_modes: rustc_hash::FxHashSet::default(),
            dyn_input_on: false,
            quick_props_on: false,
            selection_cycling_on: false,
        };

        let mut modded = base.clone();
        modded.snap_on = true;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.isometric = true;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.iso_plane = crate::app::settings::IsoPlane::Top;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.snap_angle_deg = 45.0;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.polar_on = true;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.ortho_on = true;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.polar_increment_deg = 30.0;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.osnap_on = false;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.otrack_on = true;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.snap_modes.insert(crate::snap::SnapType::Endpoint);
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.osnap3d_on = true;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.snap3d_modes.insert(crate::snap::SnapType::Vertex);
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.dyn_input_on = true;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.quick_props_on = true;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.selection_cycling_on = true;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.snap_x_input = "5".to_string();
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.snap_y_input = "5".to_string();
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.snap_equal = false;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.grid_x_input = "5".to_string();
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.grid_y_input = "5".to_string();
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.grid_major_input = "10".to_string();
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.grid_adaptive = false;
        assert!(modded.is_dirty(&base));

        let mut modded = base.clone();
        modded.grid_beyond_limits = false;
        assert!(modded.is_dirty(&base));
    }

    #[test]
    fn test_parse_snap_spacing() {
        use crate::ui::window::drafting_settings::{
            format_snap_spacing, parse_grid_major, parse_snap_spacing,
        };
        assert_eq!(parse_snap_spacing("10"), Some(10.0));
        assert_eq!(parse_snap_spacing(" 2.5 "), Some(2.5));
        assert_eq!(parse_snap_spacing("0"), None);
        assert_eq!(parse_snap_spacing("-1"), None);
        assert_eq!(parse_snap_spacing("abc"), None);
        assert_eq!(parse_snap_spacing(""), None);
        assert_eq!(format_snap_spacing(10.0), "10");
        assert_eq!(parse_grid_major("5"), Some(5));
        assert_eq!(parse_grid_major("1"), None);
        assert_eq!(parse_grid_major("101"), None);
        assert_eq!(parse_grid_major("abc"), None);
    }
}
