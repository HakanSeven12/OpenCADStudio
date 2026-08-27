//! `file` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{
    parse_coord, polar_constrain_near, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis,
    CoordKind,
};
use crate::app::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
use crate::scene::pick::grip::{find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit};
use crate::scene::model::object::GripApply;
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::PropertiesPanel;
use acadrust::types::Color as AcadColor;
use acadrust::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};

pub(super) fn background_task<T, F, M>(work: F, map: M) -> Task<Message>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
    M: FnOnce(T) -> Message + Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        Task::perform(
            async move { rx.await.expect("background export worker dropped") },
            map,
        )
    }
    #[cfg(target_arch = "wasm32")]
    {
        Task::perform(async move { work() }, map)
    }
}

fn plot_dialog_scale_factor(d: &crate::ui::window::plot::PlotDialogState) -> f64 {
    d.scales
        .iter()
        .find(|(name, _)| name == &d.scale)
        .map(|(_, factor)| *factor)
        .or_else(|| {
            let (paper, drawing) = parse_plot_scale(&d.scale);
            (paper > 0.0 && drawing > 0.0).then_some(paper / drawing)
        })
        .unwrap_or(1.0)
        .max(1e-9)
}

fn scale_name_for_factor(scales: &[(String, f64)], factor: f64) -> Option<String> {
    scales
        .iter()
        .find(|(_, candidate)| {
            (*candidate - factor).abs() <= 1e-6 * factor.abs().max(1.0)
        })
        .map(|(name, _)| name.clone())
}

fn plot_render_mode_override(
    d: &crate::ui::window::plot::PlotDialogState,
) -> Option<acadrust::entities::ViewportRenderMode> {
    use acadrust::entities::ViewportRenderMode as Mode;
    match d.shade.as_str() {
        "2D Wireframe" => Some(Mode::Wireframe2D),
        "3D Wireframe" => Some(Mode::Wireframe3D),
        "Hidden Line" => Some(Mode::HiddenLine),
        "Flat Shaded" => Some(Mode::FlatShaded),
        "Gouraud Shaded" => Some(Mode::GouraudShaded),
        "Flat Shaded + Edges" => Some(Mode::FlatShadedWithEdges),
        "Gouraud Shaded + Edges" => Some(Mode::GouraudShadedWithEdges),
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_paths_match(left: &std::path::Path, right: &std::path::Path) -> bool {
    match (
        std::fs::canonicalize(left),
        std::fs::canonicalize(right),
    ) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

type LayoutPlotParams = (
    std::sync::Arc<Vec<crate::io::pdf_export::PlotWire>>,
    Vec<crate::scene::model::hatch_model::HatchModel>,
    Vec<crate::scene::model::hatch_model::HatchModel>,
    crate::io::pdf_export::PlotGroupSplits,
    f64,
    f64,
    f64,
    f64,
    i32,
    f32,
    Option<(f32, f32, f32, f32)>,
);

type ClippedPlotParams = (
    Vec<crate::io::pdf_export::PlotWire>,
    Vec<crate::scene::model::hatch_model::HatchModel>,
    Vec<crate::scene::model::hatch_model::HatchModel>,
    crate::io::pdf_export::PlotGroupSplits,
    f64,
    f64,
    f64,
    f64,
    i32,
    f32,
    Option<(f32, f32, f32, f32)>,
);

fn plot_dialog_sheet_mm(d: &crate::ui::window::plot::PlotDialogState) -> (f64, f64) {
    use crate::io::paper_sizes::{sheet_mm, Orientation, PaperSize};
    let orientation = if d.orientation == "Portrait" {
        Orientation::Portrait
    } else {
        Orientation::Landscape
    };
    let standard = match d.paper.as_str() {
        "A3" => Some(PaperSize::A3),
        "A2" => Some(PaperSize::A2),
        "A1" => Some(PaperSize::A1),
        "A0" => Some(PaperSize::A0),
        "A4" => Some(PaperSize::A4),
        _ => None,
    };
    if let Some(paper) = standard {
        return sheet_mm(paper, orientation);
    }
    let short = d.paper_width_mm.min(d.paper_height_mm).max(1.0);
    let long = d.paper_width_mm.max(d.paper_height_mm).max(1.0);
    match orientation {
        Orientation::Portrait => (short, long),
        Orientation::Landscape => (long, short),
    }
}

fn plot_content_extents(
    wires: &[crate::io::pdf_export::PlotWire],
    hatches: &[crate::scene::model::hatch_model::HatchModel],
    wipeouts: &[crate::scene::model::hatch_model::HatchModel],
) -> Option<(f64, f64, f64, f64)> {
    let mut bounds = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    let mut include = |x: f64, y: f64| {
        if x.is_finite() && y.is_finite() {
            bounds.0 = bounds.0.min(x);
            bounds.1 = bounds.1.min(y);
            bounds.2 = bounds.2.max(x);
            bounds.3 = bounds.3.max(y);
        }
    };
    for wire in wires {
        let [x0, y0, x1, y1] = wire.aabb;
        include(x0 as f64, y0 as f64);
        include(x1 as f64, y1 as f64);
    }
    for hatch in wipeouts.iter().chain(hatches.iter()) {
        for &[x, y] in hatch.boundary.iter() {
            include(hatch.world_origin[0] + x as f64, hatch.world_origin[1] + y as f64);
        }
    }
    (bounds.0.is_finite()
        && bounds.1.is_finite()
        && bounds.2.is_finite()
        && bounds.3.is_finite()
        && bounds.2 > bounds.0
        && bounds.3 > bounds.1)
        .then_some(bounds)
}

fn plot_scene_content(
    scene: &crate::scene::Scene,
    paper_space_last: bool,
    render_mode_override: Option<acadrust::entities::ViewportRenderMode>,
) -> (
    std::sync::Arc<Vec<crate::io::pdf_export::PlotWire>>,
    Vec<crate::scene::model::hatch_model::HatchModel>,
    Vec<crate::scene::model::hatch_model::HatchModel>,
    crate::io::pdf_export::PlotGroupSplits,
) {
    let (mut paper_wires, mut model_wires) = scene.plot_wire_groups(render_mode_override);
    let plot_viewport_borders = scene
        .effective_plot_settings()
        .is_none_or(|settings| settings.flags.plot_viewport_borders);
    paper_wires.retain(|wire| {
        wire.plot_visible
            && (plot_viewport_borders
                || !crate::scene::Scene::handle_from_wire_name(&wire.name)
                    .and_then(|handle| scene.document.get_entity(handle))
                    .is_some_and(|entity| {
                        matches!(
                            entity,
                            acadrust::EntityType::Viewport(viewport)
                                if !crate::scene::Scene::is_sheet_viewport(
                                    &scene.document,
                                    viewport,
                                )
                        )
                    }))
    });
    model_wires.retain(|wire| wire.plot_visible);
    let with_depth = |wires: Vec<crate::scene::WireModel>| {
        let depths = scene.plot_wire_depths(&wires);
        wires
            .into_iter()
            .zip(depths)
            .map(|(wire, draw_depth)| crate::io::pdf_export::PlotWire { wire, draw_depth })
            .collect::<Vec<_>>()
    };
    let paper_wires = with_depth(paper_wires);
    let model_wires = with_depth(model_wires);
    let paper_hatches = scene.paper_plot_hatches().as_ref().clone();
    let paper_wipeouts = scene.paper_plot_wipeouts().as_ref().clone();
    if scene.current_layout == "Model" {
        let splits = crate::io::pdf_export::PlotGroupSplits {
            wires: paper_wires.len(),
            hatches: paper_hatches.len(),
            wipeouts: paper_wipeouts.len(),
        };
        return (
            std::sync::Arc::new(paper_wires),
            paper_hatches,
            paper_wipeouts,
            splits,
        );
    }
    let (mut model_pattern_wires, model_hatches, model_wipeouts) =
        scene.viewport_plot_fills();
    model_pattern_wires.retain(|(wire, _)| wire.plot_visible);
    let model_pattern_wires = model_pattern_wires
        .into_iter()
        .map(|(wire, draw_depth)| crate::io::pdf_export::PlotWire { wire, draw_depth })
        .collect::<Vec<_>>();

    let (wires, hatches, wipeouts, splits) = if paper_space_last {
        let splits = crate::io::pdf_export::PlotGroupSplits {
            wires: model_wires.len() + model_pattern_wires.len(),
            hatches: model_hatches.len(),
            wipeouts: model_wipeouts.len(),
        };
        let mut wires = model_wires;
        let mut hatches = model_hatches;
        let mut wipeouts = model_wipeouts;
        wires.extend(model_pattern_wires);
        wires.extend(paper_wires);
        hatches.extend(paper_hatches);
        wipeouts.extend(paper_wipeouts);
        (wires, hatches, wipeouts, splits)
    } else {
        let splits = crate::io::pdf_export::PlotGroupSplits {
            wires: paper_wires.len(),
            hatches: paper_hatches.len(),
            wipeouts: paper_wipeouts.len(),
        };
        let mut wires = paper_wires;
        let mut hatches = paper_hatches;
        let mut wipeouts = paper_wipeouts;
        wires.extend(model_wires);
        wires.extend(model_pattern_wires);
        hatches.extend(model_hatches);
        wipeouts.extend(model_wipeouts);
        (wires, hatches, wipeouts, splits)
    };
    (std::sync::Arc::new(wires), hatches, wipeouts, splits)
}

impl OpenCADStudio {
    /// Persist exact ACIS bodies and kernel-derived edge caches before saving.
    fn sync_solid_models_for_save(&mut self, i: usize) {
        use acadrust::EntityType;
        let scene = &mut self.tabs[i].scene;
        let targets: Vec<(acadrust::Handle, bool, bool)> = scene
            .document
            .entities()
            .filter_map(|entity| {
                let EntityType::Solid3D(solid) = entity else {
                    return None;
                };
                let h = solid.common.handle;
                let needs_acis = !solid.acis_data.has_data();
                let needs_wires = solid.wires.is_empty();
                (needs_acis || needs_wires).then_some((h, needs_acis, needs_wires))
            })
            .collect();
        for (h, needs_acis, needs_wires) in targets {
            let body = scene.solid_models.get(&h);
            let sat = needs_acis
                .then(|| {
                    body.and_then(crate::scene::convert::acis_export::solid_to_sat)
                })
                .flatten();
            let wires = needs_wires.then(|| {
                if let Some(body) = body {
                    return crate::scene::model::solid_model::edge_wires(body);
                }
                let Some(mesh) = scene.meshes.get(&h).or_else(|| scene.block_meshes.get(&h)) else {
                    return Vec::new();
                };
                mesh.edge_verts
                    .chunks_exact(2)
                    .enumerate()
                    .map(|(index, points)| {
                        let first_low = mesh
                            .edge_verts_low
                            .get(index * 2)
                            .copied()
                            .unwrap_or([0.0; 3]);
                        let second_low = mesh
                            .edge_verts_low
                            .get(index * 2 + 1)
                            .copied()
                            .unwrap_or([0.0; 3]);
                        acadrust::entities::Wire::from_points(vec![
                            acadrust::types::Vector3::new(
                                points[0][0] as f64 + first_low[0] as f64,
                                points[0][1] as f64 + first_low[1] as f64,
                                points[0][2] as f64 + first_low[2] as f64,
                            ),
                            acadrust::types::Vector3::new(
                                points[1][0] as f64 + second_low[0] as f64,
                                points[1][1] as f64 + second_low[1] as f64,
                                points[1][2] as f64 + second_low[2] as f64,
                            ),
                        ])
                    })
                    .collect()
            });
            if let Some(EntityType::Solid3D(solid)) = scene.document.get_entity_mut(h) {
                if let Some(sat) = sat {
                    solid.set_sat_document(&sat);
                }
                if let Some(wires) = wires.filter(|wires| !wires.is_empty()) {
                    solid.wires = wires;
                }
            }
        }
    }

    /// Snapshot the persisted UI preferences from live state.
    pub(in crate::app) fn current_settings(&self) -> crate::app::settings::UserSettings {
        crate::app::settings::UserSettings {
            dyn_input: self.dyn_input,
            polar: self.polar_mode,
            polar_increment_deg: self.polar_increment_deg,
            zoom_wheel_reversed: self.zoom_wheel_reversed,
            zoom_factor: self.zoom_factor,
            cursor_size: self.cursor_size,
            pick_box: self.pick_box,
            cursor_type: self.cursor_type,
            crosshair_color: self.crosshair_color,
            isometric_drafting: self.isometric_drafting,
            iso_plane: self.iso_plane,
            snap_angle_deg: self.snap_angle_deg,
            otrack: self.snapper.otrack_enabled,
            default_assoc_prompted: self.default_assoc_prompted,
            disabled_plugins: {
                let mut v: Vec<String> = self.disabled_plugins.iter().cloned().collect();
                v.sort();
                v
            },
            plugin_repos: self.plugin_repos.clone(),
            literal_spaces: self.command_line.literal_spaces,
            command_history_height: self.command_line.history_height,
            osmode: crate::app::settings::osmode_from_snaps(
                self.snapper.enabled.iter(),
                self.snapper.snap_enabled,
            ),
            texteditmode: self.texteditmode,
            dimension_continue_mode: self.dimension_continue_mode,
            textfill: crate::scene::text::sdf_atlas::textfill(),
            backup_on_save: self.backup_on_save,
            file_assoc_enabled: self.file_assoc_enabled,
            savetime_min: self.savetime_min,
            default_save_format: self.default_save_format.clone(),
            pick_add: self.pick_add,
            pick_drag_rect: self.pick_drag_rect,
            quick_properties: self.quick_properties,
            bg_color: self.default_bg_color.map(f4_to_u3),
            paper_bg_color: self.default_paper_bg_color.map(f4_to_u3),
            language: self.language,
            cliprompt_lines: crate::app::settings::clamp_clipromptlines(self.cliprompt_lines),
            block_mru: self.block_mru.clone(),
            block_freq: self.block_freq.clone(),
        }
    }

    /// Apply restored preferences to live state.
    pub(in crate::app) fn apply_settings(&mut self, s: &crate::app::settings::UserSettings) {
        self.dyn_input = s.dyn_input;
        self.polar_mode = s.polar;
        self.polar_increment_deg = s.polar_increment_deg;
        self.zoom_wheel_reversed = s.zoom_wheel_reversed;
        self.zoom_factor = s.zoom_factor.clamp(3, 100);
        self.cursor_size = s.cursor_size.clamp(1, 100);
        self.pick_box = s.pick_box.clamp(0, 50);
        self.cursor_type = s.cursor_type;
        self.crosshair_color = s.crosshair_color;
        self.crosshair_color_input = s
            .crosshair_color
            .map(crate::app::config::rgb_to_hex)
            .unwrap_or_default();
        self.isometric_drafting = s.isometric_drafting;
        self.iso_plane = s.iso_plane;
        self.snap_angle_deg = if s.snap_angle_deg.is_finite() {
            s.snap_angle_deg.rem_euclid(360.0)
        } else {
            0.0
        };
        // Ortho + running OSNAP are per-drawing (adopted from the header on
        // open / tab switch), not app-global, so they are not applied here.
        self.snapper.otrack_enabled = s.otrack;
        self.default_assoc_prompted = s.default_assoc_prompted;
        self.disabled_plugins = s.disabled_plugins.iter().cloned().collect();
        self.plugin_repos = s.plugin_repos.clone();
        self.command_line.literal_spaces = s.literal_spaces;
        self.command_line.history_height = if s.command_history_height.is_finite() {
            s.command_history_height.clamp(
                crate::ui::command_line::HISTORY_HEIGHT_MIN,
                crate::ui::command_line::HISTORY_HEIGHT_MAX,
            )
        } else {
            crate::ui::command_line::HISTORY_HEIGHT_DEFAULT
        };
        let (modes, snap_enabled) = crate::app::settings::snaps_from_osmode(s.osmode);
        self.snapper.enabled = modes.into_iter().collect();
        self.snapper.snap_enabled = snap_enabled;
        self.texteditmode = s.texteditmode;
        self.dimension_continue_mode = s.dimension_continue_mode.clamp(0, 1);
        crate::scene::text::sdf_atlas::set_textfill(s.textfill);
        self.backup_on_save = s.backup_on_save;
        self.file_assoc_enabled = s.file_assoc_enabled;
        self.savetime_min = s.savetime_min;
        self.default_save_format =
            crate::io::canonical_save_format(&s.default_save_format).to_string();
        self.pick_add = s.pick_add;
        self.pick_drag_rect = s.pick_drag_rect;
        self.quick_properties = s.quick_properties;
        self.default_bg_color = s.bg_color.map(u3_to_f4);
        self.default_paper_bg_color = s.paper_bg_color.map(u3_to_f4);
        if crate::i18n::set_language(s.language).is_ok() {
            self.language = s.language;
        }
        self.cliprompt_lines = crate::app::settings::clamp_clipromptlines(s.cliprompt_lines);
        self.command_line
            .set_cliprompt_lines(self.cliprompt_lines.clamp(0, 50) as u8);
        // Block usage: clone but cap to sane sizes (MRU 20, freq map 200)
        self.block_mru = s.block_mru.iter().take(20).cloned().collect();
        self.block_freq = s
            .block_freq
            .iter()
            .take(200)
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        // Push the restored background onto every drawing tab that exists now
        // (the start tab and any initial drawing). Tabs created later pick it
        // up via `apply_bg_default` at their construction site.
        for idx in 0..self.tabs.len() {
            self.apply_bg_default(idx);
        }
        self.rebuild_ribbon_modules();
    }

    pub(crate) fn record_block_insert(&mut self, name: &str) {
        let key = name.to_ascii_uppercase();
        *self.block_freq.entry(key).or_insert(0) += 1;
        self.block_mru.retain(|n| !n.eq_ignore_ascii_case(name));
        self.block_mru.insert(0, name.to_string());
        if self.block_mru.len() > 20 {
            self.block_mru.truncate(20);
        }
        // Cap freq map to 200 most frequent to bound persistence size.
        if self.block_freq.len() > 200 {
            let mut items: Vec<(String, u32)> = self
                .block_freq
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            items.sort_by(|a, b| b.1.cmp(&a.1));
            items.truncate(200);
            self.block_freq = items.into_iter().collect();
        }
        // Debounce disk writes: at most once per second (2.4) to avoid thrash
        // during rapid scripting/batch inserts. Immediate first write ensures
        // crash recovery. Wasm has no `std::time::Instant` guarantee, so persist
        // immediately there.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = std::time::Instant::now();
            let should_persist = match self.block_usage_last_persist {
                None => true,
                Some(t) => now.duration_since(t).as_secs_f32() >= 1.0,
            };
            if should_persist {
                self.block_usage_last_persist = Some(now);
                self.persist_settings_if_changed();
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.persist_settings_if_changed();
        }
    }

    pub(crate) fn block_usage_snapshot(&self) -> rustc_hash::FxHashMap<String, (u32, usize)> {
        let mut out = rustc_hash::FxHashMap::default();
        for (idx, name) in self.block_mru.iter().enumerate() {
            let up = name.to_ascii_uppercase();
            let freq = self.block_freq.get(&up).copied().unwrap_or(0);
            out.insert(up, (freq, idx));
        }
        // Ensure every freq entry has at least an entry (for blocks not in MRU)
        for (up, freq) in &self.block_freq {
            out.entry(up.clone()).or_insert((*freq, usize::MAX));
        }
        out
    }

    pub(crate) fn ranked_block_names(&self, names: &[String]) -> Vec<String> {
        let mut scored: Vec<(&String, u32, usize)> = names
            .iter()
            .map(|n| {
                let up = n.to_ascii_uppercase();
                let f = self.block_freq.get(&up).copied().unwrap_or(0);
                let m = self
                    .block_mru
                    .iter()
                    .position(|x| x.eq_ignore_ascii_case(n))
                    .unwrap_or(usize::MAX);
                (n, f, m)
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)).then_with(|| a.0.cmp(b.0)));
        scored.into_iter().map(|(n, _, _)| n.clone()).collect()
    }

    /// Adopt the per-drawing sysvars stored in tab `i`'s document header —
    /// Ortho (`$ORTHOMODE`) and the running object-snap set (`$OSMODE`) — into
    /// the live app state, so each drawing keeps its own. Called when a drawing
    /// becomes active (open, tab switch). The app-settings values seed the boot
    /// default; the per-file header overrides it here.
    pub(in crate::app) fn adopt_header_sysvars(&mut self, i: usize) {
        if self.tabs[i].is_start {
            return;
        }
        let (ortho, osmode) = {
            let h = &self.tabs[i].scene.document.header;
            (h.ortho_mode, h.object_snap_mode)
        };
        self.ortho_mode = ortho;
        // Ortho and Polar are mutually exclusive (ToggleOrtho clears Polar).
        if ortho {
            self.polar_mode = false;
        }
        // OSMODE has no file slot in modern DWG (R2000+ moved it to the
        // registry), so a header value of 0 just means "absent" — keep the
        // user's app-level set instead of wiping it. Only a legacy R13/R14 or
        // DXF file that really carries a nonzero mask overrides.
        if osmode != 0 {
            let (modes, snap_enabled) = crate::app::settings::snaps_from_osmode(osmode);
            self.snapper.enabled = modes.into_iter().collect();
            self.snapper.snap_enabled = snap_enabled;
        }
    }

    /// Stamp the live per-drawing sysvars (Ortho, running OSNAP) onto tab `i`'s
    /// document header just before it is written to disk (or before switching
    /// away from it), so they persist with the drawing.
    pub(in crate::app) fn stamp_header_sysvars(&mut self, i: usize) {
        if self.tabs[i].is_start {
            return;
        }
        let osmode = crate::app::settings::osmode_from_snaps(
            self.snapper.enabled.iter(),
            self.snapper.snap_enabled,
        );
        let h = &mut self.tabs[i].scene.document.header;
        h.ortho_mode = self.ortho_mode;
        h.object_snap_mode = osmode;
    }

    /// Apply the persisted default background(s) to tab `idx`. No-op for the
    /// start tab or when no default is set. Refreshes the tab's cached wires
    /// and meshes so background-adaptive colours pick up the change.
    pub(in crate::app) fn apply_bg_default(&mut self, idx: usize) {
        let bg = self.default_bg_color;
        let paper_bg = self.default_paper_bg_color;
        if bg.is_none() && paper_bg.is_none() {
            return;
        }
        let tab = &mut self.tabs[idx];
        if tab.is_start {
            return;
        }
        if let Some(c) = bg {
            tab.bg_color = Some(c);
            tab.scene.bg_color = c;
        }
        if let Some(c) = paper_bg {
            tab.paper_bg_color = Some(c);
            tab.scene.paper_bg_color = c;
        }
        tab.scene.recolor_meshes();
        tab.scene.bump_geometry();
    }

    /// Check if a suspended command exists on the active tab and resume it
    /// with the outcome of the text editor.
    pub(in crate::app) fn post_editor_closed(&mut self, committed: bool) -> Task<Message> {
        self.reset_modal_geometry();
        let i = self.active_tab;
        if let Some(mut cmd) = self.tabs[i].suspended_cmd.take() {
            if committed {
                if let Some(value) = self.pending_command_editor_text.take() {
                    cmd.on_editor_text(value);
                }
            } else {
                self.pending_command_editor_text = None;
            }
            let res = cmd.on_editor_closed(committed);
            self.tabs[i].active_cmd = Some(cmd);
            self.apply_cmd_result(res)
        } else {
            self.focus_cmd_input()
        }
    }

    /// Rebuild the ribbon's tab list from the registry, dropping the tabs of any
    /// disabled plugins. Call after `disabled_plugins` changes.
    pub(in crate::app) fn rebuild_ribbon_modules(&mut self) {
        let modules =
            crate::plugin::ribbon_modules_enabled(&self.disabled_plugins);
        self.ribbon.set_modules(modules);
        // Refresh the command-line autocomplete pool so a newly enabled plugin's
        // commands become typeable (and a disabled one's drop out). This runs on
        // startup load, settings reload, and every enable/disable toggle (#272).
        self.command_line.dynamic_commands =
            crate::plugin::plugin_command_names(&self.disabled_plugins);
    }

    /// Snapshot of disabled plugin ids — lets the registry skip them while it
    /// holds a `&mut` borrow of the app via `HostSession`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn disabled_plugin_ids(&self) -> rustc_hash::FxHashSet<String> {
        self.disabled_plugins.clone()
    }

    /// Background task: fetch the curated plugin registry.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn fetch_registry_task(&self) -> Task<Message> {
        Task::perform(
            async { crate::plugin::marketplace::fetch_registry() },
            Message::PluginRegistryFetched,
        )
    }

    /// Background task: fetch `owner/repo`'s installable releases and their
    /// manifest API versions. The fetch runs on its own OS thread because the
    /// several sequential HTTP requests inside `fetch_release_info` would
    /// otherwise block the async executor and serialise all repo fetches.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn fetch_releases_task(&self, repo: String) -> Task<Message> {
        let label = repo.clone();
        Task::perform(
            async move {
                let (tx, rx) = iced::futures::channel::oneshot::channel();
                std::thread::spawn(move || {
                    let result = crate::plugin::marketplace::fetch_release_info(&repo);
                    let _ = tx.send(result);
                });
                rx.await
                    .unwrap_or_else(|_| Err("release fetch thread died".into()))
            },
            move |res| Message::PluginReleasesFetched(label, res),
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn fetch_releases_task(&self, repo: String) -> Task<Message> {
        Task::done(Message::PluginReleasesFetched(
            repo,
            Err("External plugins are available in the desktop app.".to_string()),
        ))
    }

    /// Background task: fetch a repository README from its default branch.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn fetch_plugin_readme_task(&self, repo: String) -> Task<Message> {
        let label = repo.clone();
        Task::perform(
            async move { crate::plugin::marketplace::fetch_readme(&repo) },
            move |result| Message::PluginReadmeFetched(label, result),
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn fetch_plugin_readme_task(&self, repo: String) -> Task<Message> {
        Task::done(Message::PluginReadmeFetched(
            repo,
            Err("Plugin details are available in the desktop app.".to_string()),
        ))
    }

    /// Background task: download and install the `tag` release of `owner/repo`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn install_task(&self, repo: String, tag: String) -> Task<Message> {
        Task::perform(
            async move {
                let releases = crate::plugin::marketplace::fetch_releases(&repo)?;
                let rel = releases
                    .into_iter()
                    .find(|r| r.tag == tag)
                    .ok_or_else(|| format!("release {tag} not found"))?;
                crate::plugin::marketplace::install(&rel, &repo)
            },
            Message::PluginInstalled,
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn install_task(&self, _repo: String, _tag: String) -> Task<Message> {
        Task::done(Message::PluginInstalled(Err(
            "External plugins are available in the desktop app.".to_string(),
        )))
    }

    /// Gather the full persisted config (all sections) from live app state.
    pub(in crate::app) fn current_config(&self) -> crate::app::config::AppConfig {
        crate::app::config::AppConfig {
            settings: self.current_settings(),
            theme: self.ui_theme.clone(),
            recent: crate::app::config::RecentConfig {
                files: self
                    .recent_files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
                limit: self.recent_limit,
            },
            start: crate::app::config::StartConfig {
                section: self.start_section,
            },
            statusbar: self.statusbar_config.clone(),
            dock: {
                let mut dock = self.dock.clone();
                dock.ensure_settings();
                dock
            },
            annotation_auto_scale: self.annotation_auto_scale,
            ribbon: crate::app::config::RibbonConfig {
                collapse: self.ribbon.collapse_mode(),
            },
            plot: self.plot_dialog.clone(),
            shortcuts: crate::app::config::ShortcutConfig {
                bindings: self
                    .shortcut_bindings
                    .iter()
                    .map(|(key, command)| (key.clone(), command.clone()))
                    .collect(),
            },
        }
    }

    /// Distribute a loaded config into live app state (called once at startup).
    pub(in crate::app) fn apply_config(&mut self, cfg: crate::app::config::AppConfig) {
        self.apply_settings(&cfg.settings);
        self.ui_theme = cfg.theme.clone();
        self.active_theme = self.ui_theme.to_iced();
        self.theme_color_inputs = self.ui_theme.palette.hex_values();
        self.recent_files = cfg
            .recent
            .files
            .iter()
            .map(std::path::PathBuf::from)
            .collect();
        self.recent_limit = cfg
            .recent
            .limit
            .clamp(crate::app::recent::RECENT_MIN, crate::app::recent::RECENT_MAX);
        self.recent_files.truncate(self.recent_limit);
        self.recent_limit_input = self.recent_limit.to_string();
        // Thumbnails are decoded by a background task queued at boot
        // (`refresh_recent_thumbs`) — never here on the boot path.
        self.start_section = cfg.start.section;
        self.statusbar_config = cfg.statusbar;
        let mut dock = cfg.dock;
        dock.ensure_settings();
        self.dock = dock;
        self.annotation_auto_scale = cfg.annotation_auto_scale.clamp(-4, 4);
        self.ribbon.set_collapse_mode(cfg.ribbon.collapse);
        self.plot_dialog = cfg.plot;
        self.shortcut_bindings = cfg.shortcuts.bindings.into_iter().collect();
        self.shortcut_bindings
            .entry("F5".to_string())
            .or_insert_with(|| "ISOPLANE".to_string());
    }

    /// Write the config only when it changed since the last write, so a toggle
    /// persists immediately without thrashing native or browser storage.
    pub(in crate::app) fn save_config(&mut self) {
        let cur = self.current_config();
        if self.last_saved_config.as_ref() != Some(&cur) {
            cur.save();
            self.last_saved_config = Some(cur);
        }
    }

    /// Back-compat name for the many "a preference changed, persist it" sites.
    pub(in crate::app) fn persist_settings_if_changed(&mut self) {
        // Keep resize feedback live without writing settings on every pointer
        // move. The release message saves the final height once.
        if !self.command_history_resizing {
            self.save_config();
        }
    }

    /// Record that the one-time default-association prompt has been answered and
    /// flush it to disk, so the dialog never reappears on later launches.
    pub(in crate::app) fn mark_assoc_prompted(&mut self) {
        self.default_assoc_prompted = true;
        self.persist_settings_if_changed();
    }

pub(super) fn on_open_file(&mut self) -> Task<Message> {
                // Native: pick a path, then load on a worker thread. Web: the
                // browser hands back bytes, so pick + parse in one step and feed
                // the shared `FileOpened` handler directly.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Task::perform(crate::io::pick_open_path(), Message::OpenPathPicked)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    // `FileOpened` only installs the result when an open is in
                    // progress, so mark one. The browser picker + parse happen
                    // inside `pick_and_load_web`; the real name is unknown until
                    // then, so show a generic label meanwhile.
            let state = std::sync::Arc::new(crate::io::OpenProgressState::new(
                crate::app::OPEN_PHASE_READING,
            ));
                    let open_id = self.next_open_id();
                    self.opening = Some(crate::app::OpenProgress {
                        id: open_id,
                        name: "Opening…".into(),
                        source_path: None,
                        size_bytes: 0,
                        state: state.clone(),
                        started: Instant::now(),
                        recovery_error: None,
                        recovery_read_stats: None,
                        recovery_bytes: None,
                    });
                    Task::perform(crate::io::pick_and_load_web(state), move |outcome| {
                        Message::WebFileOpened(open_id, outcome)
                    })
                }
    }

    pub(in crate::app) fn next_open_id(&mut self) -> u64 {
        self.open_job_serial = self.open_job_serial.wrapping_add(1).max(1);
        self.open_job_serial
    }

    /// Index of a tab already showing `path`, or `None`.
    ///
    /// Compares resolved paths, so the same drawing reached through a symlink,
    /// a `..` segment or a different relative spelling is recognised as the one
    /// already open rather than loaded a second time. A path that cannot be
    /// resolved (deleted since) matches nothing and falls through to the normal
    /// open, which reports the miss.
    pub(in crate::app) fn tab_showing(&self, path: &std::path::Path) -> Option<usize> {
        #[cfg(target_arch = "wasm32")]
        {
            return self
                .tabs
                .iter()
                .position(|tab| tab.current_path.as_deref() == Some(path));
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let want = std::fs::canonicalize(path).ok()?;
            self.tabs.iter().position(|t| {
                t.current_path
                    .as_deref()
                    .and_then(|p| std::fs::canonicalize(p).ok())
                    .is_some_and(|p| p == want)
            })
        }
    }

    /// Read through this session's lease when it covers `path`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn read_drawing(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};

        let lease = self
            .tab_showing(path)
            .and_then(|i| self.tabs[i].edit_lease.as_ref());
        let leased = match lease {
            Some(lease) => lease.reader()?,
            None => None,
        };
        let Some(mut reader) = leased else {
            return std::fs::read(path);
        };
        reader.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// Start the next drawing a second launch handed us, if any.
    ///
    /// Must be called from EVERY path that clears `opening` — completion, error
    /// and cancel alike. Draining only the success path would strand the queue
    /// forever the first time a file fails to parse.
    pub(in crate::app) fn drain_pending_open(&mut self) -> Task<Message> {
        match self.pending_opens.pop_front() {
            Some(p) => Task::done(Message::OpenExternal(p)),
            None => Task::none(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn install_native_edit_guard(
        &mut self,
        i: usize,
        path: &std::path::Path,
        loaded_fingerprint: Option<crate::io::edit_lock::FileFingerprint>,
    ) {
        let current_fingerprint =
            crate::io::edit_lock::FileFingerprint::capture(path).ok();
        if loaded_fingerprint
            .as_ref()
            .zip(current_fingerprint.as_ref())
            .is_some_and(|(loaded, current)| loaded != current)
        {
            self.command_line.push_error_once(
                crate::t!(
                    "Drawing changed on disk while it was opening; Save will require conflict resolution."
                )
                .as_ref(),
            );
        }
        self.tabs[i].disk_fingerprint =
            loaded_fingerprint.or(current_fingerprint);
        match crate::io::edit_lock::EditLease::acquire(path) {
            Ok(lease) => {
                if let Some(warning) = lease.platform_warning() {
                    self.command_line.push_info(crate::tf!(
                        "Edit lease active; {warning}. External-change checks remain active."
                    ).as_ref());
                }
                self.tabs[i].edit_lease = Some(lease);
                self.tabs[i].edit_lock_conflict = false;
            }
            Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                self.tabs[i].edit_lease = None;
                self.tabs[i].edit_lock_conflict = true;
                self.command_line.push_error_once(crate::tf!(
                    "Opened read-only against other editors: {error}"
                ).as_ref());
            }
            Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                self.tabs[i].edit_lease = None;
                self.tabs[i].edit_lock_conflict = false;
                self.command_line.push_info(crate::tf!(
                    "{error}. External-change checks remain active."
                ).as_ref());
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn refresh_native_edit_guard_after_save(
        &mut self,
        i: usize,
        path: &std::path::Path,
        path_changed: bool,
        destination_lease: Option<crate::io::edit_lock::EditLease>,
    ) {
        if path_changed {
            self.tabs[i].edit_lease = None;
            self.tabs[i].edit_lease = destination_lease;
            self.tabs[i].edit_lock_conflict = false;
            if self.tabs[i].edit_lease.is_none() {
                self.install_native_edit_guard(i, path, None);
                return;
            }
        }

        let refresh = self.tabs[i].edit_lease.as_mut().map(|lease| {
            lease
                .refresh_drawing_lock(path)
                .map(|_| lease.platform_warning().map(str::to_owned))
        });
        match refresh {
            Some(Ok(warning)) => {
                self.tabs[i].edit_lock_conflict = false;
                if let Some(warning) = warning {
                    self.command_line.push_info(crate::tf!(
                        "{warning}. External-change checks remain active."
                    ).as_ref());
                }
            }
            Some(Err(crate::io::edit_lock::EditLeaseError::Locked(error))) => {
                self.tabs[i].edit_lock_conflict = true;
                self.command_line.push_error_once(crate::tf!(
                    "Saved, but the refreshed drawing is locked by another editor: {error}"
                ).as_ref());
            }
            Some(Err(crate::io::edit_lock::EditLeaseError::Unavailable(error))) => {
                self.tabs[i].edit_lock_conflict = false;
                self.command_line.push_info(crate::tf!(
                    "{error}. External-change checks remain active."
                ).as_ref());
            }
            None => self.install_native_edit_guard(i, path, None),
        }
        self.tabs[i].disk_fingerprint =
            crate::io::edit_lock::FileFingerprint::capture(path).ok();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn save_tab_synchronously_protected(
        &mut self,
        i: usize,
        path: std::path::PathBuf,
        set_current_path: bool,
    ) -> Result<(), crate::io::SaveFailure> {
        let previous_autosave = self.autosave_target(i);
        if self.tabs[i].recovery_save_as_required
            && self.tabs[i]
                .current_path
                .as_deref()
                .is_some_and(|source| native_paths_match(source, &path))
        {
            return Err(crate::io::SaveFailure::other(
                "repaired drawing must be saved to a new file",
            ));
        }
        let path_changed = self.tabs[i]
            .current_path
            .as_deref()
            .is_none_or(|current| !native_paths_match(current, &path));
        if !path_changed && self.tabs[i].edit_lock_conflict {
            return Err(crate::io::SaveFailure::file_in_use(
                "drawing edit lock is held by another editor",
            ));
        }

        let mut destination_lease = if path_changed {
            match crate::io::edit_lock::EditLease::acquire(&path) {
                Ok(lease) => Some(lease),
                Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                    return Err(crate::io::SaveFailure::file_in_use(error));
                }
                Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                    self.command_line.push_info(crate::tf!(
                        "{error}. External-change checks remain active."
                    ).as_ref());
                    None
                }
            }
        } else {
            None
        };

        let expected_fingerprint = if path_changed {
            None
        } else {
            self.tabs[i].disk_fingerprint.clone()
        };
        let lease = if path_changed {
            destination_lease.as_mut()
        } else {
            self.tabs[i].edit_lease.as_mut()
        };
        let (expected_fingerprint, verify_reader) =
            Self::native_save_verification(&path, lease, expected_fingerprint)?;

        self.prepare_native_save(i);
        let version = self.tabs[i].scene.document.version;
        let snapshot = self.tabs[i].scene.document.clone();
        crate::io::save_owned_as_version_atomic(
            snapshot,
            &path,
            version,
            self.backup_on_save,
            expected_fingerprint,
            verify_reader,
        )?;

        if set_current_path {
            self.tabs[i].current_path = Some(path.clone());
        }
        self.refresh_native_edit_guard_after_save(
            i,
            &path,
            path_changed,
            destination_lease,
        );
        self.tabs[i].dirty = false;
        if set_current_path {
            self.tabs[i].recovery_save_as_required = false;
        }
        let _ = std::fs::remove_file(previous_autosave);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn retry_native_edit_guard(
        &mut self,
        i: usize,
        path: &std::path::Path,
    ) -> Result<(), String> {
        if let Some(lease) = self.tabs[i].edit_lease.as_mut() {
            match lease.refresh_drawing_lock(path) {
                Ok(()) => {
                    let warning = lease.platform_warning().map(str::to_owned);
                    self.tabs[i].edit_lock_conflict = false;
                    if let Some(warning) = warning {
                        self.command_line.push_info(crate::tf!(
                            "{warning}. External-change checks remain active."
                        ).as_ref());
                    }
                    return Ok(());
                }
                Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                    return Err(error);
                }
                Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                    self.tabs[i].edit_lock_conflict = false;
                    self.command_line.push_info(crate::tf!(
                        "{error}. External-change checks remain active."
                    ).as_ref());
                    return Ok(());
                }
            }
        }

        match crate::io::edit_lock::EditLease::acquire(path) {
            Ok(lease) => {
                let warning = lease.platform_warning().map(str::to_owned);
                self.tabs[i].edit_lease = Some(lease);
                self.tabs[i].edit_lock_conflict = false;
                if let Some(warning) = warning {
                    self.command_line.push_info(crate::tf!(
                        "{warning}. External-change checks remain active."
                    ).as_ref());
                }
                Ok(())
            }
            Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => Err(error),
            Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                self.tabs[i].edit_lock_conflict = false;
                self.command_line.push_info(crate::tf!(
                    "{error}. External-change checks remain active."
                ).as_ref());
                Ok(())
            }
        }
    }

    pub(super) fn on_file_opened(&mut self, name: String, path: std::path::PathBuf, doc: acadrust::CadDocument,
        mut caches: crate::scene::DerivedCaches,
    ) -> Task<Message> {
                // If the user clicked Cancel while the parser was running, the
                // overlay state was cleared and we silently drop the result.
                if self.opening.is_none() {
                    return Task::none();
                }
                let open_started = self.opening.as_ref().map(|p| p.started);
                let size_bytes = self
                    .opening
                    .as_ref()
                    .map(|progress| progress.size_bytes)
                    .unwrap_or(0);
                let timings = caches.timings;
                let entity_count = doc.entities().count();
                let parser_errors_recovered = caches.read_stats.as_ref().is_some_and(|stats| {
                    stats.recovered()
                        || stats.skipped_source_records > 0
                        || !stats.stream_completed
                }) || doc.notifications.iter().any(|item| {
                    item.notification_type == acadrust::notification::NotificationType::Error
                });
                let reference_recovered = caches
                    .xrefs
                    .iter()
                    .any(|item| item.status == crate::io::xref::XrefStatus::Recovered);
                let reference_failed = caches
                    .xrefs
                    .iter()
                    .any(|item| item.status == crate::io::xref::XrefStatus::Failed);
                let document_repaired = parser_errors_recovered
                    || reference_recovered
                    || caches.corrupt_dropped > 0
                    || caches.xref_dropped > 0;
                let recovery_needed = document_repaired || reference_failed;
                let total_ms = open_started
                    .map(|started| started.elapsed().as_millis() as u32)
                    .unwrap_or(0);
                self.command_line
                    .push_output(crate::tf!("Opened \"{name}\" — {entity_count} entities").as_ref());
                if caches.corrupt_dropped > 0 {
                    self.command_line.push_error(crate::tf!(
                        "Warning: {} corrupt entities dropped (parser junk — bad normals / counts)",
                        caches.corrupt_dropped
                    ).as_ref());
                }
        if caches.xref_dropped > 0 {
            self.command_line.push_error(crate::tf!(
                "Warning: {} corrupt xref entities dropped",
                caches.xref_dropped
            ).as_ref());
        }
        for info in &caches.xrefs {
            match info.status {
                crate::io::xref::XrefStatus::Loaded => {
                    self.command_line
                        .push_output(crate::tf!("XREF  Loaded \"{}\"", info.name).as_ref());
                }
                crate::io::xref::XrefStatus::Recovered => {
                    self.command_line.push_error(crate::tf!(
                        "XREF  Recovered with warnings: \"{}\"",
                        info.name
                    ).as_ref());
                }
                crate::io::xref::XrefStatus::NotFound => {
                    self.command_line.push_error(crate::tf!(
                        "XREF  Not found: \"{}\" ({})",
                        info.name, info.path
                    ).as_ref());
                }
                crate::io::xref::XrefStatus::Failed => {
                    self.command_line.push_error(crate::tf!(
                        "XREF  Recovery failed: \"{}\" ({})",
                        info.name, info.path
                    ).as_ref());
                }
                crate::io::xref::XrefStatus::Unloaded => {
                    self.command_line
                        .push_info(crate::tf!("XREF  Unloaded (skipped): \"{}\"", info.name).as_ref());
                }
            }
        }
                #[cfg(not(target_arch = "wasm32"))]
                let thumbs_task = self.push_recent(path.clone());
                #[cfg(target_arch = "wasm32")]
                let thumbs_task = Task::none();

                let current_is_empty = {
                    let t = &self.tabs[self.active_tab];
                    !t.is_start
                        && t.current_path.is_none()
                        && !t.dirty
                        && self.tabs[self.active_tab].scene.document.entities().count() == 0
                };
                let i = if current_is_empty {
                    self.active_tab
                } else {
                    self.tab_counter += 1;
                    let new_tab = crate::app::document::DocumentTab::new_drawing(self.tab_counter);
                    self.tabs.push(new_tab);
                    let idx = self.tabs.len() - 1;
                    self.active_tab = idx;
                    self.apply_bg_default(idx);
                    idx
                };

                let mut recovery_report = recovery_needed.then(|| {
                    crate::io::recovery::RecoveryReport::recovered(
                        self.tabs[i].id,
                        &path,
                        size_bytes,
                        caches.source_sha256.clone(),
                        caches.read_stats.clone(),
                        entity_count.saturating_add(caches.corrupt_dropped),
                        caches.corrupt_dropped,
                        caches.xref_dropped,
                        &caches.xrefs,
                        &doc.notifications,
                        document_repaired,
                        timings,
                        total_ms,
                    )
                });
                if let Some(report) = recovery_report.as_mut() {
                    report.persist();
                }

                #[cfg(not(target_arch = "wasm32"))]
                let opened_fingerprint = self
                    .opening
                    .as_ref()
                    .and_then(|opening| opening.fingerprint.clone());
                self.tabs[i].current_path = Some(path.clone());
                #[cfg(not(target_arch = "wasm32"))]
                self.install_native_edit_guard(i, &path, opened_fingerprint);
                self.tabs[i].scene.material_base_dir =
                    path.parent().map(std::path::Path::to_path_buf);
                self.tabs[i].scene.document = doc;
                self.tabs[i].active_layer = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_layer_name
                    .clone();
                // A file saved without the built-in Standard styles (foreign
                // or damaged) gets them re-seeded so nothing dangles (#366).
                crate::app::style_ops::ensure_standard_styles(
                    &mut self.tabs[i].scene.document,
                );
                // Follow the file's saved current UCS from the moment it opens.
                self.tabs[i].adopt_active_ucs_from_header();
                // Adopt the drawing's own Ortho ($ORTHOMODE) and running OSNAP
                // ($OSMODE) so each file keeps its own state.
                self.adopt_header_sysvars(i);
                // Route shared CJK ideographs to the language matching this
                // drawing's code page (web per-language font split). Drop the
                // glyph cache if it changed so Han re-resolves to the new
                // language's font; geometry is (re)built below regardless. (#141)
                if crate::scene::text::web_font::set_cjk_lang_from_codepage(
                    &self.tabs[i].scene.document.header.code_page,
                ) {
                    crate::scene::text::ttf_glyph::clear_fallback_cache();
                }
                // Current model-space annotation scale comes from the drawing's
                // CANNOSCALEVALUE (paper/drawing factor). Convert its inverse into
                // drawing units as well: metric annotation sizes are paper millimetres
                // and imperial annotation sizes are paper inches.
                // Current model-space annotation scale comes from the drawing's
                // CANNOSCALEVALUE (paper/drawing factor). Convert its paper unit into
                // the drawing's INSUNITS as well, so e.g. a metre drawing uses
                // 0.001 model units for 1 mm of paper at 1:1.
                let cannoscale_value = self.tabs[i].scene.document.header.annotation_scale_value;
                let unit_factor = self.tabs[i].scene.annotation_scale_unit_factor();

                self.tabs[i].scene.annotation_scale = if cannoscale_value > 1e-9 {
                    ((1.0 / cannoscale_value) / unit_factor) as f32
                } else {
                    (1.0 / unit_factor) as f32
                };

                // Open-time breakdown so regressions are visible immediately.
                // `total` is wall time from the Open click to here (post-xref,
                // pre-first-frame); the phase figures are the background-thread
                // parse/purge/cache spans plus the UI-thread xref resolve.
                self.command_line.push_info(crate::tf!(
                    "  parse {}ms · purge {}ms · caches {}ms · xref {}ms · total {}ms",
                    timings.parse_ms, timings.purge_ms, timings.caches_ms, timings.xref_ms, total_ms
                ).as_ref());

                // Caches were built on the background thread inside open_path().
                self.tabs[i].scene.local_extent_max = caches.local_extent_max;
                self.tabs[i].scene.local_center = caches.local_center;
                self.tabs[i].scene.hatches = caches.hatches;
                self.tabs[i].scene.images = caches.images;
                self.tabs[i].scene.meshes = caches.meshes;
                self.tabs[i].scene.block_meshes = caches.block_meshes;
                self.tabs[i].scene.object_data_cache = caches.object_data;
        let prepared_geometry = caches.prepared_geometry.take();
                // Invalidate the wire cache so the new document is tessellated.
                self.tabs[i].scene.bump_geometry();
                if let Some(prepared) = prepared_geometry {
                    self.tabs[i].scene.install_prepared_open_geometry(prepared);
                }
                self.tabs[i]
                    .scene
                    .replace_selection(rustc_hash::FxHashSet::default());
                self.tabs[i].scene.preview_wires = vec![];
                // Reopen in whichever space the file was saved in — the CTAB
                // tab name when recorded, else the $TILEMODE model/paper flag —
                // instead of always landing in Model.
                {
                    let names = self.tabs[i].scene.layout_names();
                    let saved = crate::io::saved_active_layout(&self.tabs[i].scene.document)
                        .and_then(|n| {
                            names.iter().find(|x| x.eq_ignore_ascii_case(&n)).cloned()
                        });
                    self.tabs[i].scene.current_layout = match saved {
                        Some(n) => n,
                        None if self.tabs[i].scene.document.header.show_model_space => {
                            "Model".to_string()
                        }
                        // Paper was active but no CTAB — fall back to the first
                        // paper layout (names[0] is always "Model").
                        None => names
                            .into_iter()
                            .nth(1)
                            .unwrap_or_else(|| "Model".to_string()),
                    };
                    self.tabs[i].scene.load_current_layout_state();
                    self.tabs[i].refresh_active_ucs();
                }
                // Object isolation is session-only. A newly opened drawing must
                // not inherit the previous tab's filter, and persisted entity
                // visibility remains independent (not an isolation session).
                self.tabs[i].scene.reset_transient_visibility();
                crate::io::linetypes::populate_document(&mut self.tabs[i].scene.document);
                self.tabs[i].properties = PropertiesPanel::empty();
                // Seed the current table / multileader style from the file's
                // header so the ✓ marks the right one (text/dim/mline come from
                // the document header directly). DXF provides these via
                // $CTABLESTYLE / $CMLEADERSTYLE; DWG leaves them at "Standard".
                self.ribbon.active_table_style = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_table_style_name
                    .clone();
                self.tabs[i].active_mleader_style = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_mleader_style_name
                    .clone();
                let doc_layers = self.tabs[i].scene.document.layers.clone();
                let vp_info = self.tabs[i].scene.viewport_list();
                self.tabs[i]
                    .layers
                    .sync_with_viewports(&doc_layers, vp_info);
                self.sync_ribbon_layers();
                // Load the Annotate-ribbon style dropdowns (text / dimension /
                // multileader / table) from the opened document instead of
                // leaving them on the hard-coded "Standard" default.
                self.sync_ribbon_styles();
                // Reset the Home-ribbon Color / Linetype / Lineweight chips
                // to the newly opened document's CECOLOR / CELTYPE / CELWEIGHT
                // defaults (or to ByLayer when the file leaves them empty).
                // Without this they stick to whatever the prior tab had
                // selected — see #21.
                self.sync_ribbon_from_selection();
                self.tabs[i].scene.restore_saved_camera();
                // Grid/snap are per-drawing view settings — adopt the opened
                // file's active viewport state rather than a global preference.
                self.adopt_view_display(i);
                self.sync_render_mode_to_active_tile(i);
                self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
                self.tabs[i].dirty = document_repaired;
                self.tabs[i].recovery_save_as_required = document_repaired;
                self.tabs[i].history = crate::app::document::HistoryState::default();
                self.refresh_properties();
                #[cfg(not(target_arch = "wasm32"))]
                let interaction_task = {
                    let wires = self.tabs[i].scene.hit_test_wires();
                    let screen_height = self.tabs[i].scene.selection.borrow().vp_size.1;
                    self.prepare_interaction_index_task(i, wires, screen_height)
                        .unwrap_or_else(Task::none)
                };
                #[cfg(target_arch = "wasm32")]
                let interaction_task = Task::none();
        if let Some(opening) = &self.opening {
            opening
                .state
                .set(crate::app::OPEN_PHASE_FINALIZING, 10000, 1, 1);
        }
        self.opening.take();
                let pending_open_task = if let Some(report) = recovery_report {
                    self.recovery_report = Some(report);
                    self.active_modal = Some(crate::app::ModalKind::Recovery);
                    Task::none()
                } else {
                    self.drain_pending_open()
                };
                Task::batch([thumbs_task, pending_open_task, interaction_task])
    }

    pub(super) fn on_wblock_save_result_some(&mut self, block_name: String, path: std::path::PathBuf,
    ) -> Task<Message> {
                let i = self.active_tab;
                let document = self.tabs[i].scene.document.clone();
                    let handles: Vec<_> = self.tabs[i].scene.selected.iter().copied().collect();
        let worker_name = block_name.clone();
        let worker_path = path.clone();
        background_task(
            move || {
                let document = if worker_name == "*" {
                    crate::modules::insert::wblock::extract_entities_to_doc(
                        &document,
                        &handles,
                    )
                } else {
                    crate::modules::insert::wblock::extract_block_to_doc(
                        &document,
                        &worker_name)
                }
                .map_err(|e| e.to_string())?; crate::io::save(&document, &worker_path).map_err(|e| e.to_string())
                    },
            move |result| Message::WblockWriteFinished(block_name, path, result),)
    }

    pub(super) fn on_stl_export_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
                // Re-build STL bytes (we can't easily pass them through the message).
                let i = self.active_tab;
                // STL gets the highest-resolution LOD (slot 0) so the
                // exported geometry isn't downgraded by the view-dependent
                // mesh LOD ladder used for rendering.
                let meshes: Vec<crate::scene::model::mesh_model::MeshModel> = self.tabs[i]
                    .scene
                    .meshes
                    .values()
                    .filter_map(|s| s.lods.first().cloned())
                    .collect();
                let worker_path = path.clone();
        background_task(
            move || {
                let mesh_refs: Vec<_> = meshes.iter().collect();
                let bytes = crate::io::stl::build_stl(&mesh_refs)
                    .ok_or_else(|| "no mesh data to export".to_string())?;
                std::fs::write(&worker_path, bytes).map_err(|e| e.to_string())
                    },
            move |result| Message::StlExportFinished(path, result),)
    }

    pub(super) fn on_step_export_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
                let i = self.active_tab;
                // Export uses LOD 0 (full resolution); see StlExportPath above.
                let meshes: Vec<crate::scene::model::mesh_model::MeshModel> = self.tabs[i]
                    .scene
                    .meshes
                    .values()
                    .filter_map(|s| s.lods.first().cloned())
                    .collect();
                let worker_path = path.clone();
        background_task(
            move || {
                let mesh_refs: Vec<_> = meshes.iter().collect();
                let text = crate::io::step::build_step(&mesh_refs)
                    .ok_or_else(|| "no mesh data to export".to_string())?;
                std::fs::write(&worker_path, text.as_bytes()).map_err(|e| e.to_string())
                    },
            move |result| Message::StepExportFinished(path, result),)
    }

    pub(super) fn on_obj_import_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
                let tab_id = self.tabs[self.active_tab].id;
        let worker_path = path.clone();
        background_task(
            move || {
                let src = std::fs::read_to_string(&worker_path).map_err(|e| e.to_string())?; crate::io::obj::parse_obj(&src, [0.7, 0.7, 0.85, 1.0])
                            .ok_or_else(|| "no usable geometry in file".to_string())
            },
            move |result| Message::ObjImportFinished(tab_id, path, result),)
    }

    fn sync_view_state_for_save(&mut self, i: usize) {
        self.sync_vport_display(i);
        if self.tabs[i].active_block_edit.is_none() {
            self.tabs[i].scene.sync_camera_to_document();
            self.tabs[i].last_synced_camera_gen =
                self.tabs[i].scene.camera_generation;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn prepare_native_save(&mut self, i: usize) {
        self.sync_view_state_for_save(i);
        sync_annotation_scale_header(&mut self.tabs[i].scene);
        self.stamp_header_sysvars(i);
        self.tabs[i].scene.document.header.user_real1 =
            self.tabs[i].scene.annotation_scale as f64;
        self.sync_solid_models_for_save(i);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn native_save_verification(
        path: &std::path::Path,
        mut lease: Option<&mut crate::io::edit_lock::EditLease>,
        expected: Option<crate::io::edit_lock::FileFingerprint>,
    ) -> Result<
        (
            Option<crate::io::edit_lock::FileFingerprint>,
            Option<std::fs::File>,
        ),
        crate::io::SaveFailure,
    > {
        let expected = match expected {
            Some(expected) => Some(expected),
            None => {
                let captured = match lease.as_deref_mut() {
                    Some(lease) => match lease.fingerprint() {
                        Ok(Some(fingerprint)) => Ok(fingerprint),
                        Ok(None) => crate::io::edit_lock::FileFingerprint::capture(path),
                        Err(error) => Err(error),
                    },
                    None => crate::io::edit_lock::FileFingerprint::capture(path),
                };
                match captured {
                    Ok(fingerprint) => Some(fingerprint),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => {
                        return Err(crate::io::SaveFailure::other(format!(
                            "could not verify {} before saving: {error}",
                            path.display()
                        )));
                    }
                }
            }
        };
        let reader = if expected.is_some() {
            match lease.as_deref() {
                Some(lease) => lease.reader().map_err(|error| {
                    crate::io::SaveFailure::other(format!(
                        "could not verify {} before saving: {error}",
                        path.display()
                    ))
                })?,
                None => None,
            }
        } else {
            None
        };
        Ok((expected, reader))
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn on_thumbnail_capture_frame(&mut self) -> Task<Message> {
        let Some(pending) = self.pending_web_thumbnail_save.take() else {
            return Task::none();
        };
        iced::window::latest()
            .then(|window| match window {
                Some(window) => iced::window::screenshot(window).map(Some),
                None => Task::done(None),
            })
            .map(move |screenshot| Message::WebSaveScreenshot {
                tab_id: pending.tab_id,
                filename: pending.filename.clone(),
                ext: pending.ext.clone(),
                version: pending.version,
                bounds: Some(pending.bounds),
                screenshot,
            })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_thumbnail_capture_frame(&mut self) -> Task<Message> {
        let Some(pending) = self.pending_native_thumbnail_save.take() else {
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == pending.tab_id) else {
            self.thumbnail_capture_clean = false;
            return Task::none();
        };
        self.queue_native_save(
            i,
            pending.path,
            pending.version,
            pending.purpose,
            pending.continuation,
            pending.set_current_path,
            pending.check_external_change,
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn on_web_save_screenshot(
        &mut self,
        tab_id: u64,
        filename: String,
        ext: String,
        version: acadrust::DxfVersion,
        bounds: Option<iced::Rectangle>,
        screenshot: Option<iced::window::Screenshot>,
    ) -> Task<Message> {
        self.thumbnail_capture_clean = false;
        let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return Task::none();
        };
        let preview = screenshot.as_ref().and_then(|screenshot| {
            bounds.and_then(|bounds| {
                crate::io::thumbnail::from_screenshot(
                    screenshot,
                    bounds,
                    version >= acadrust::DxfVersion::AC1027,
                )
            })
        });
        if let Some(preview) = preview {
            self.tabs[i].scene.document.preview = Some(preview);
        }

        let mut recent_task = Task::none();
        let saved = match crate::io::save_to_bytes(
            &self.tabs[i].scene.document,
            &ext,
            version,
        ) {
            Ok(bytes) => {
                crate::sys::download_bytes(&filename, &bytes);
                let cache_name = std::path::Path::new(&filename)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| filename.clone());
                let path = std::path::PathBuf::from(cache_name);
                self.tabs[i].current_path = Some(path.clone());
                self.tabs[i].scene.document.version = version;
                self.tabs[i].dirty = false;
                self.tabs[i].recovery_save_as_required = false;
                recent_task = Task::perform(
                    async move {
                        crate::io::web_recent::store(&path.to_string_lossy(), &bytes)
                            .await
                            .map(|_| path)
                    },
                    Message::WebRecentStored,
                );
                self.command_line
                    .push_output(crate::tf!("Saved: {filename}").as_ref());
                true
            }
            Err(error) => {
                self.command_line
                    .push_error(crate::tf!("Save failed: {error}").as_ref());
                false
            }
        };
        if self.save_dialog_for_unsaved {
            if saved {
                if let Some(crate::app::PendingClose::Tab(index)) = self.pending_close.take() {
                    let continuation = self.update(Message::TabClose(index));
                    let rest = self.continue_tab_close_queue();
                    return Task::batch([recent_task, continuation, rest]);
                }
            } else if self.pending_close.is_some() {
                let retry = self.open_unsaved_dialog_window();
                return Task::batch([recent_task, retry]);
            }
        }
        recent_task
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn queue_native_save(
        &mut self,
        i: usize,
        path: std::path::PathBuf,
        version: acadrust::DxfVersion,
        purpose: crate::app::SavePurpose,
        continuation: crate::app::SaveContinuation,
        set_current_path: bool,
        check_external_change: bool,
    ) -> Task<Message> {
        let tab_id = self.tabs[i].id;
        if self
            .pending_native_thumbnail_save
            .as_ref()
            .is_some_and(|pending| pending.tab_id == tab_id)
        {
            if purpose != crate::app::SavePurpose::Autosave {
                self.command_line
                    .push_info(crate::t!("Save already running for this drawing.").as_ref());
            }
            return Task::none();
        }
        let capture_ready =
            self.thumbnail_capture_clean && self.pending_native_thumbnail_save.is_none();
        if self.active_save_jobs.contains_key(&tab_id) {
            if purpose != crate::app::SavePurpose::Autosave {
                self.command_line
                    .push_info(crate::t!("Save already running for this drawing.").as_ref());
            }
            if capture_ready {
                self.thumbnail_capture_clean = false;
            }
            return Task::none();
        }
        let destination_is_current = self.tabs[i]
            .current_path
            .as_deref()
            .is_some_and(|current| native_paths_match(current, &path));
        if purpose != crate::app::SavePurpose::Autosave
            && self.tabs[i].recovery_save_as_required
            && destination_is_current
        {
            self.command_line.push_error_once(
                crate::tr!("recovery", "save-new-file-required").as_ref(),
            );
            self.restore_failed_save_continuation(continuation, i);
            self.active_tab = i;
            self.save_dialog_for_unsaved =
                continuation != crate::app::SaveContinuation::None;
            if capture_ready {
                self.thumbnail_capture_clean = false;
            }
            return self.open_save_dialog_window(i);
        }
        if purpose != crate::app::SavePurpose::Autosave
            && !set_current_path
            && self.tabs[i].edit_lock_conflict
        {
            let error = "Drawing edit lock is held by another editor.".to_string();
            self.command_line.push_error_once(crate::tf!(
                "Unable to save \"{}\": {error}",
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string())
            ).as_ref());
            self.pending_save_failure = Some(crate::app::PendingSaveFailure {
                tab_id,
                path,
                version,
                purpose,
                continuation,
                set_current_path,
                error,
            });
            self.restore_failed_save_continuation(continuation, i);
            self.active_modal = Some(crate::app::ModalKind::FileInUse);
            if capture_ready {
                self.thumbnail_capture_clean = false;
            }
            return Task::none();
        }

        if purpose != crate::app::SavePurpose::Autosave
            && i == self.active_tab
            && !self.thumbnail_capture_clean
            && self.main_window.is_some()
            && crate::ui::wrap_bar::dropdown_bounds(
                crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID,
            )
            .is_some()
        {
            self.pending_native_thumbnail_save = Some(
                crate::app::PendingNativeThumbnailSave {
                    tab_id,
                    path,
                    version,
                    purpose,
                    continuation,
                    set_current_path,
                    check_external_change,
                },
            );
            self.thumbnail_capture_clean = true;
            return Task::none();
        }

        if set_current_path && !destination_is_current {
            match crate::io::edit_lock::EditLease::acquire(&path) {
                Ok(lease) => {
                    self.pending_save_leases.insert(tab_id, lease);
                }
                Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                    self.command_line.push_error_once(crate::tf!(
                        "Unable to save \"{}\": {error}",
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string())
                    ).as_ref());
                    self.pending_save_failure = Some(crate::app::PendingSaveFailure {
                        tab_id,
                        path,
                        version,
                        purpose,
                        continuation,
                        set_current_path,
                        error,
                    });
                    self.restore_failed_save_continuation(continuation, i);
                    self.active_modal = Some(crate::app::ModalKind::FileInUse);
                    if capture_ready {
                        self.thumbnail_capture_clean = false;
                    }
                    return Task::none();
                }
                Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                    self.command_line.push_info(crate::tf!(
                        "{error}. External-change checks remain active."
                    ).as_ref());
                }
            }
        }

        let epoch = self.tabs[i].scene.geometry_epoch;
        let revision = self.tabs[i].edit_revision;
        let camera_generation = self.tabs[i].scene.camera_generation;
        let thumbnail = (purpose != crate::app::SavePurpose::Autosave && i == self.active_tab)
            .then_some(version >= acadrust::DxfVersion::AC1027);
        let capture_bounds = thumbnail.and_then(|_| {
            crate::ui::wrap_bar::dropdown_bounds(
                crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID,
            )
        });
        let clone_started = iced::time::Instant::now();
        let snapshot = self.tabs[i].scene.document.clone();
        let clone_ms = clone_started.elapsed().as_secs_f64() * 1000.0;
        if crate::perf::enabled() {
            crate::perf_record!(
                "[perf] save-snapshot {:.1}ms entities={} objects={} purpose={purpose:?}",
                clone_ms,
                snapshot.entities().count(),
                snapshot.objects.len(),
            );
        }

        self.save_job_serial = self.save_job_serial.wrapping_add(1);
        let job_id = self.save_job_serial;
        self.active_save_jobs.insert(tab_id, job_id);
        let previous_autosave =
            (purpose != crate::app::SavePurpose::Autosave).then(|| self.autosave_target(i));
        let backup = purpose != crate::app::SavePurpose::Autosave && self.backup_on_save;
        let verification = if check_external_change
            && purpose != crate::app::SavePurpose::Autosave
        {
            let expected = if set_current_path {
                None
            } else {
                self.tabs[i].disk_fingerprint.clone()
            };
            if self.pending_save_leases.contains_key(&tab_id) {
                Self::native_save_verification(
                    &path,
                    self.pending_save_leases.get_mut(&tab_id),
                    expected,
                )
            } else if destination_is_current {
                Self::native_save_verification(
                    &path,
                    self.tabs[i].edit_lease.as_mut(),
                    expected,
                )
            } else {
                Self::native_save_verification(&path, None, expected)
            }
        } else {
            Ok((None, None))
        };
        let (expected_fingerprint, verify_reader) = match verification {
            Ok(verification) => verification,
            Err(error) => {
                if capture_ready {
                    self.thumbnail_capture_clean = false;
                }
                return Task::perform(
                    async move {
                        crate::app::SaveOutcome {
                            job_id,
                            tab_id,
                            epoch,
                            revision,
                            camera_generation,
                            path,
                            version,
                            previous_autosave,
                            set_current_path,
                            purpose,
                            continuation,
                            refreshed_preview: None,
                            result: Err(error),
                        }
                    },
                    Message::SaveFinished,
                );
            }
        };
        let worker_path = path.clone();
        let capture_window = thumbnail.and(capture_bounds).and(self.main_window);
        let mut work = Some((
            snapshot,
            thumbnail,
            capture_bounds,
            worker_path,
            expected_fingerprint,
            verify_reader,
            path,
            previous_autosave,
        ));
        let mut run_save = move |screenshot: Option<iced::window::Screenshot>| {
            let (
                mut snapshot,
                thumbnail,
                capture_bounds,
                worker_path,
                expected_fingerprint,
                verify_reader,
                path,
                previous_autosave,
            ) = work.take().expect("save capture produced more than one result");
            Task::perform(
                async move {
                    let (result, refreshed_preview) = std::thread::spawn(move || {
                        let mut refreshed_preview = None;
                        if let Some(png) = thumbnail {
                            let started = iced::time::Instant::now();
                            if let Some(preview) = screenshot.as_ref().and_then(|screenshot| {
                                capture_bounds.and_then(|bounds| {
                                    crate::io::thumbnail::from_screenshot(screenshot, bounds, png)
                                })
                            }) {
                                snapshot.preview = Some(preview);
                                refreshed_preview = Some(snapshot.preview.clone());
                            }
                            if crate::perf::enabled() {
                                crate::perf_record!(
                                    "[perf] save-thumbnail {:.1}ms",
                                    started.elapsed().as_secs_f64() * 1000.0,
                                );
                            }
                        }
                        let result = crate::io::save_owned_as_version_atomic(
                            snapshot,
                            &worker_path,
                            version,
                            backup,
                            expected_fingerprint,
                            verify_reader,
                        );
                        (result, refreshed_preview)
                    })
                    .join()
                    .unwrap_or_else(|_| {
                        (
                            Err(crate::io::SaveFailure::other("save worker panicked")),
                            None,
                        )
                    });
                    crate::app::SaveOutcome {
                        job_id,
                        tab_id,
                        epoch,
                        revision,
                        camera_generation,
                        path,
                        version,
                        previous_autosave,
                        set_current_path,
                        purpose,
                        continuation,
                        refreshed_preview,
                        result,
                    }
                },
                Message::SaveFinished,
            )
        };
        match capture_window {
            Some(window) => iced::window::screenshot(window).map(Some).then(move |screenshot| {
                Task::batch([
                    Task::done(Message::ThumbnailCaptureFinished),
                    run_save(screenshot),
                ])
            }),
            None => {
                self.thumbnail_capture_clean = false;
                run_save(None)
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_save_finished(
        &mut self,
        outcome: crate::app::SaveOutcome,
    ) -> Task<Message> {
        let latest = self.active_save_jobs.get(&outcome.tab_id).copied()
            == Some(outcome.job_id);
        if latest {
            self.active_save_jobs.remove(&outcome.tab_id);
        }
        let destination_lease = self.pending_save_leases.remove(&outcome.tab_id);

        let Some(i) = self.tabs.iter().position(|tab| tab.id == outcome.tab_id) else {
            if outcome.purpose == crate::app::SavePurpose::Autosave {
                let _ = std::fs::remove_file(&outcome.path);
            }
            return Task::none();
        };
        if !latest {
            if outcome.purpose == crate::app::SavePurpose::Autosave {
                let _ = std::fs::remove_file(&outcome.path);
            }
            return Task::none();
        }

        if let Err(error) = &outcome.result {
            if error.externally_modified
                && outcome.purpose != crate::app::SavePurpose::Autosave
            {
                let file_name = outcome
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| outcome.path.display().to_string());
                self.command_line.push_error_once(crate::tf!(
                    "Save stopped: \"{file_name}\" changed outside Open CAD Studio."
                ).as_ref());
                self.pending_external_change = Some(crate::app::PendingExternalChange {
                    tab_id: outcome.tab_id,
                    path: outcome.path.clone(),
                    version: outcome.version,
                    purpose: outcome.purpose,
                    continuation: outcome.continuation,
                    set_current_path: outcome.set_current_path,
                });
                self.restore_failed_save_continuation(outcome.continuation, i);
                self.active_modal = Some(crate::app::ModalKind::ExternalChange);
                return Task::none();
            }
            if error.file_in_use && outcome.purpose != crate::app::SavePurpose::Autosave {
                let file_name = outcome
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| outcome.path.display().to_string());
                self.command_line.push_error_once(crate::tf!(
                    "Unable to save \"{file_name}\": file is in use by another application."
                ).as_ref());
                self.pending_save_failure = Some(crate::app::PendingSaveFailure {
                    tab_id: outcome.tab_id,
                    path: outcome.path.clone(),
                    version: outcome.version,
                    purpose: outcome.purpose,
                    continuation: outcome.continuation,
                    set_current_path: outcome.set_current_path,
                    error: error.to_string(),
                });
                match outcome.continuation {
                    crate::app::SaveContinuation::CloseTab => {
                        self.pending_close = Some(crate::app::PendingClose::Tab(i));
                    }
                    crate::app::SaveContinuation::Quit => {
                        self.pending_close = Some(crate::app::PendingClose::Quit);
                    }
                    crate::app::SaveContinuation::None => {}
                }
                self.active_modal = Some(crate::app::ModalKind::FileInUse);
                return Task::none();
            }
            self.command_line
                .push_error(crate::tf!("Save failed: {error}").as_ref());
            return match outcome.continuation {
                crate::app::SaveContinuation::CloseTab => {
                    self.pending_close = Some(crate::app::PendingClose::Tab(i));
                    self.open_unsaved_dialog_window()
                }
                crate::app::SaveContinuation::Quit => {
                    self.pending_close = Some(crate::app::PendingClose::Quit);
                    self.open_unsaved_dialog_window()
                }
                crate::app::SaveContinuation::None => Task::none(),
            };
        }

        let snapshot_is_current = self.tabs[i].scene.geometry_epoch == outcome.epoch
            && self.tabs[i].edit_revision == outcome.revision
            && self.tabs[i].scene.camera_generation == outcome.camera_generation;
        if snapshot_is_current && outcome.purpose != crate::app::SavePurpose::Autosave {
            if let Some(preview) = outcome.refreshed_preview {
                self.tabs[i].scene.document.preview = preview;
            }
        }
        let mut tasks = Vec::new();
        match outcome.purpose {
            crate::app::SavePurpose::Autosave => {
                self.command_line.push_output(crate::t!("Autosaved 1 drawing").as_ref());
            }
            crate::app::SavePurpose::Manual | crate::app::SavePurpose::SaveAs => {
                let path_changed = outcome.set_current_path
                    && self.tabs[i]
                        .current_path
                        .as_deref()
                        .is_none_or(|current| {
                            !native_paths_match(current, &outcome.path)
                });
                self.command_line
                    .push_output(crate::tf!("Saved: {}", outcome.path.display()).as_ref());
                if let Some(previous) = outcome.previous_autosave {
                    if previous != outcome.path {
                        let _ = std::fs::remove_file(previous);
                    }
                }
                if outcome.set_current_path {
                    self.tabs[i].current_path = Some(outcome.path.clone());
                    self.tabs[i].scene.document.version = outcome.version;
                    if outcome.purpose == crate::app::SavePurpose::SaveAs {
                        self.tabs[i].recovery_save_as_required = false;
                    }
                }
                tasks.push(self.push_recent(outcome.path.clone()));
                self.refresh_native_edit_guard_after_save(
                    i,
                    &outcome.path,
                    path_changed,
                    destination_lease,
                );
                if snapshot_is_current {
                    self.tabs[i].dirty = false;
                }
            }
        }

        match outcome.continuation {
            crate::app::SaveContinuation::None => {}
            crate::app::SaveContinuation::CloseTab if snapshot_is_current => {
                self.pending_close = None;
                tasks.push(self.close_unsaved_dialog_window());
                tasks.push(self.update(Message::TabClose(i)));
                tasks.push(self.continue_tab_close_queue());
            }
            crate::app::SaveContinuation::Quit if snapshot_is_current => {
                self.pending_close = None;
                if self.tabs.iter().any(|tab| tab.dirty) {
                    self.pending_close = Some(crate::app::PendingClose::Quit);
                    tasks.push(self.open_unsaved_dialog_window());
                } else {
                    tasks.push(self.close_unsaved_dialog_window());
                    tasks.push(self.exit_app());
                }
            }
            crate::app::SaveContinuation::CloseTab => {
                self.pending_close = Some(crate::app::PendingClose::Tab(i));
                tasks.push(self.open_unsaved_dialog_window());
            }
            crate::app::SaveContinuation::Quit => {
                self.pending_close = Some(crate::app::PendingClose::Quit);
                tasks.push(self.open_unsaved_dialog_window());
            }
        }
        Task::batch(tasks)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn restore_failed_save_continuation(
        &mut self,
        continuation: crate::app::SaveContinuation,
        tab_idx: usize,
    ) {
        self.pending_close = match continuation {
            crate::app::SaveContinuation::None => None,
            crate::app::SaveContinuation::CloseTab => {
                Some(crate::app::PendingClose::Tab(tab_idx))
            }
            crate::app::SaveContinuation::Quit => Some(crate::app::PendingClose::Quit),
        };
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_save_file_in_use_retry(&mut self) -> Task<Message> {
        let Some(mut failure) = self.pending_save_failure.take() else {
            self.close_active_modal();
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == failure.tab_id) else {
            self.close_active_modal();
            return Task::none();
        };
        if self.tabs[i].edit_lock_conflict {
            if let Err(error) = self.retry_native_edit_guard(i, &failure.path) {
                let continuation = failure.continuation;
                failure.error = error.clone();
                self.pending_save_failure = Some(failure);
                self.restore_failed_save_continuation(continuation, i);
                self.command_line
                    .push_error_once(crate::tf!("Unable to acquire edit lock: {error}").as_ref());
                self.active_modal = Some(crate::app::ModalKind::FileInUse);
                return Task::none();
            }
        }
        self.close_active_modal();
        self.restore_failed_save_continuation(failure.continuation, i);
        self.active_tab = i;
        self.prepare_native_save(i);
        self.queue_native_save(
            i,
            failure.path,
            failure.version,
            failure.purpose,
            failure.continuation,
            failure.set_current_path,
            true,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_save_file_in_use_save_as(&mut self) -> Task<Message> {
        let Some(failure) = self.pending_save_failure.take() else {
            self.close_active_modal();
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == failure.tab_id) else {
            self.close_active_modal();
            return Task::none();
        };
        self.close_active_modal();
        self.restore_failed_save_continuation(failure.continuation, i);
        self.active_tab = i;
        self.save_dialog_for_unsaved =
            failure.continuation != crate::app::SaveContinuation::None;
        self.open_save_dialog_window(i)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_external_change_overwrite(&mut self) -> Task<Message> {
        let Some(conflict) = self.pending_external_change.take() else {
            self.close_active_modal();
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == conflict.tab_id) else {
            self.close_active_modal();
            return Task::none();
        };
        self.close_active_modal();
        self.restore_failed_save_continuation(conflict.continuation, i);
        self.active_tab = i;
        self.prepare_native_save(i);
        self.queue_native_save(
            i,
            conflict.path,
            conflict.version,
            conflict.purpose,
            conflict.continuation,
            conflict.set_current_path,
            false,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_external_change_save_as(&mut self) -> Task<Message> {
        let Some(conflict) = self.pending_external_change.take() else {
            self.close_active_modal();
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == conflict.tab_id) else {
            self.close_active_modal();
            return Task::none();
        };
        self.close_active_modal();
        self.restore_failed_save_continuation(conflict.continuation, i);
        self.active_tab = i;
        self.save_dialog_for_unsaved =
            conflict.continuation != crate::app::SaveContinuation::None;
        self.open_save_dialog_window(i)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_external_change_reload(&mut self) -> Task<Message> {
        let Some(conflict) = self.pending_external_change.take() else {
            self.close_active_modal();
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == conflict.tab_id) else {
            self.close_active_modal();
            return Task::none();
        };
        let metadata = match std::fs::metadata(&conflict.path) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.close_active_modal();
                self.command_line
                    .push_error(crate::tf!("Reload failed: {error}").as_ref());
                return Task::none();
            }
        };
        self.close_active_modal();
        self.pending_close = None;
        self.tab_counter += 1;
        self.tabs[i] = crate::app::document::DocumentTab::new_drawing(self.tab_counter);
        self.active_tab = i;
        self.apply_bg_default(i);
        self.update(Message::OpenPathPicked(Some((
            conflict.path,
            metadata.len(),
        ))))
    }

    pub(super) fn on_save_file(&mut self) -> Task<Message> {
                if self.read_only {
                    self.command_line
                        .push_error(crate::t!("Read-only session (--read-only): saving is disabled.").as_ref());
                    return Task::none();
                }
                let i = self.active_tab;
                // Web serializes immediately below. Native preparation happens
                // once after the destination/version is known.
                #[cfg(target_arch = "wasm32")]
                {
                    self.sync_view_state_for_save(i);
                    self.stamp_header_sysvars(i);
                }
                // Native: save straight to the known path. Web has no path
                // (downloads instead), so always go through the Save dialog.
                #[cfg(not(target_arch = "wasm32"))]
                if !self.tabs[i].recovery_save_as_required {
                    if let Some(path) = self.tabs[i].current_path.clone() {
                        // A direct Save preserves the document's current version.
                        let ver = self.tabs[i].scene.document.version;
                        self.prepare_native_save(i);
                        return self.queue_native_save(
                            i,
                            path,
                            ver,
                            crate::app::SavePurpose::Manual,
                            crate::app::SaveContinuation::None,
                            false,
                            true,
                        );
                    }
                }
                self.save_dialog_for_unsaved = false;
                self.save_with_default_format(i)
    }

    /// Save without the version picker: use the configured default and go straight to
    /// the native destination dialog (native) or the browser download (web).
    /// Used by plain Save (QSAVE) on an as-yet-unsaved drawing and by the
    /// save-before-close flow — the version picker is reserved for Save As.
    pub(in crate::app) fn save_with_default_format(&mut self, tab_idx: usize) -> Task<Message> {
        self.active_tab = tab_idx;
        self.save_dialog_format = if self.tabs[tab_idx].recovery_save_as_required {
            let document = &self.tabs[tab_idx].scene.document;
            let is_dxf = crate::io::source_is_dxf(
                self.tabs[tab_idx].current_path.as_deref(),
                document,
            );
            let version = if is_dxf {
                document.version
            } else {
                document.dwg_source_version.unwrap_or(document.version)
            };
            crate::io::format_for_version(version, is_dxf)
        } else {
            self.default_save_format.clone()
        };
        let (ext, _) = crate::io::parse_save_format(&self.save_dialog_format);
        self.save_dialog_filename = self.tabs[tab_idx]
            .current_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.{ext}", self.tabs[tab_idx].tab_display_name()));
        if self.tabs[tab_idx].recovery_save_as_required {
            let path = std::path::Path::new(&self.save_dialog_filename);
            let stem = path
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| "drawing".to_string());
            self.save_dialog_filename = format!("{stem}_recovered.{ext}");
        }
        self.aec_drop_acknowledged = false;
        self.on_save_dialog_confirm()
    }

    pub(super) fn on_save_dialog_confirm(&mut self) -> Task<Message> {
                let (ext, version) = crate::io::parse_save_format(&self.save_dialog_format);
                // Warn before a lossy Save-As that would drop unsupported
                // (AEC / application) objects kept only as verbatim
                // source-version bytes — let the user keep them by saving in the
                // source version, or proceed and drop them.
                if !self.aec_drop_acknowledged {
                    let is_dxf = ext.eq_ignore_ascii_case("dxf");
                    let n = crate::io::dropped_on_save_count(
                        &self.tabs[self.active_tab].scene.document,
                        version,
                        is_dxf,
                    );
                    if n > 0 {
                        self.aec_drop_count = n;
                        self.active_modal = Some(crate::app::ModalKind::AecDropWarning);
                        return Task::none();
                    }
                }
                // The user need not type an extension: append the selected
                // format's one when the entered name carries none.
                let name = self.save_dialog_filename.trim();
                let filename = if name.is_empty() {
                    format!("drawing.{ext}")
                } else if std::path::Path::new(name).extension().is_none() {
                    format!("{name}.{ext}")
                } else {
                    name.to_string()
                };
                self.save_dialog_filename = filename.clone();
                let i = self.active_tab;

                // Native: hand the destination choice to the OS save dialog —
                // it provides folder browsing and overwrite confirmation. The
                // chosen version rides in `save_dialog_format` and the write
                // happens in `on_save_dialog_path_picked` once a path returns.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let seed_dir = self.tabs[i]
                        .current_path
                        .as_ref()
                        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
                    let default_name = filename;
                    let (filter_label, filter_ext): (&str, &str) =
                        if ext.eq_ignore_ascii_case("dxf") {
                            ("DXF Files", "dxf")
                        } else {
                            ("DWG Files", "dwg")
                        };
                    let close = self.close_save_dialog_window();
                    let pick = Task::perform(
                        async move {
                            let mut dlg = crate::sys::file_dialog()
                                .set_title("Save Drawing As")
                                .set_file_name(default_name)
                                .add_filter(filter_label, &[filter_ext]);
                            if let Some(dir) = seed_dir {
                                dlg = dlg.set_directory(dir);
                            }
                            dlg.save_file().await.map(|h| crate::sys::handle_path(&h))
                        },
                        Message::SaveDialogPathPicked,
                    );
                    Task::batch([close, pick])
                }
                // Web: no filesystem — serialize and hand the browser a download.
                #[cfg(target_arch = "wasm32")]
                {
                    let close = self.close_save_dialog_window();
                    self.sync_view_state_for_save(i);
                    sync_annotation_scale_header(&mut self.tabs[i].scene);
                    self.stamp_header_sysvars(i);
                    self.sync_solid_models_for_save(i);
                    let tab_id = self.tabs[i].id;
                    let bounds = crate::ui::wrap_bar::dropdown_bounds(
                        crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID,
                    );
                    let Some(bounds) = bounds else {
                        return Task::batch([
                            close,
                            Task::done(Message::WebSaveScreenshot {
                                tab_id,
                                filename,
                                ext: ext.to_string(),
                                version,
                                bounds: None,
                                screenshot: None,
                            }),
                        ]);
                    };
                    self.pending_web_thumbnail_save = Some(
                        crate::app::PendingWebThumbnailSave {
                            tab_id,
                            filename,
                            ext: ext.to_string(),
                            version,
                            bounds,
                        },
                    );
                    self.thumbnail_capture_clean = true;
                    close
                }
    }

    /// Native: a destination path came back from the OS save dialog — write the
    /// file there in the chosen version. `None` means the user cancelled.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_save_dialog_path_picked(
        &mut self,
        picked: Option<std::path::PathBuf>,
    ) -> Task<Message> {
        let Some(path) = picked else {
            // Cancelled the OS dialog — if this came from the unsaved-changes →
            // save → close flow, re-show the confirmation so the tab isn't lost.
            if self.save_dialog_for_unsaved && self.pending_close.is_some() {
                return self.open_unsaved_dialog_window();
            }
            return Task::none();
        };
        let (_ext, version) = crate::io::parse_save_format(&self.save_dialog_format);
        let i = self.active_tab;
        self.prepare_native_save(i);
        let continuation = if self.save_dialog_for_unsaved {
            match self.pending_close {
                Some(crate::app::PendingClose::Tab(_)) => crate::app::SaveContinuation::CloseTab,
                Some(crate::app::PendingClose::Quit) => crate::app::SaveContinuation::Quit,
                None => crate::app::SaveContinuation::None,
            }
        } else {
            crate::app::SaveContinuation::None
        };
        self.queue_native_save(
            i,
            path,
            version,
            crate::app::SavePurpose::SaveAs,
            continuation,
            true,
            true,
        )
    }

    /// AEC-drop warning → "Save anyway": accept the loss and proceed with the
    /// format the user already chose.
    pub(super) fn on_aec_drop_proceed(&mut self) -> Task<Message> {
        self.aec_drop_acknowledged = true;
        self.active_modal = Some(crate::app::ModalKind::SaveDialog);
        self.on_save_dialog_confirm()
    }

    /// AEC-drop warning → "Save in source version": switch the target to the
    /// document's source type and version, then save.
    pub(super) fn on_aec_drop_same_version(&mut self) -> Task<Message> {
        let tab = &self.tabs[self.active_tab];
        let document = &tab.scene.document;
        let is_dxf = crate::io::source_is_dxf(tab.current_path.as_deref(), document);
        let src = if is_dxf {
            document.version
        } else {
            document.dwg_source_version.unwrap_or(document.version)
        };
        self.save_dialog_format = crate::io::format_for_version(src, is_dxf);
        // Strip the old extension so the confirm path appends the source type.
        let stem = std::path::Path::new(&self.save_dialog_filename)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.save_dialog_filename.clone());
        self.save_dialog_filename = stem;
        self.aec_drop_acknowledged = true;
        self.active_modal = Some(crate::app::ModalKind::SaveDialog);
        self.on_save_dialog_confirm()
    }

    /// Where the autosave recovery copy for tab `i` lives.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn autosave_target(&self, i: usize) -> std::path::PathBuf {
        match &self.tabs[i].current_path {
            Some(p) => {
                let name = p
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "drawing".to_string());
                p.with_file_name(format!("{name}.ocs-autosave.sv$"))
            }
            None => {
                let safe: String = self.tabs[i]
                    .tab_display_name()
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                    .collect();
                std::env::temp_dir().join(format!(
                    "OpenCADStudio_{safe}_{}.sv$",
                    self.tabs[i].id
                ))
            }
        }
    }

    /// Periodic autosave (SAVETIME): write a `.sv$` recovery copy for every
    /// dirty tab — beside the file if it's saved, else under the temp dir — at
    /// the document's own DWG version. Best-effort and non-destructive: it never
    /// touches the original file or the dirty flag.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_autosave(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        for i in 0..self.tabs.len() {
            if !self.tabs[i].dirty
                || self.active_save_jobs.contains_key(&self.tabs[i].id)
            {
                continue;
            }
            self.prepare_native_save(i);
            let version = self.tabs[i].scene.document.version;
            let target = self.autosave_target(i);
            tasks.push(self.queue_native_save(
                i,
                target,
                version,
                crate::app::SavePurpose::Autosave,
                crate::app::SaveContinuation::None,
                false,
                false,
            ));
        }
        Task::batch(tasks)
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn on_autosave(&mut self) -> Task<Message> {
        Task::none()
    }

    /// Delete the `.sv$` autosave recovery files for all open drawings. They
    /// exist only to survive a crash, so a clean save or exit removes them.
    pub(in crate::app) fn cleanup_autosaves(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        for i in 0..self.tabs.len() {
            let _ = std::fs::remove_file(self.autosave_target(i));
        }
    }

    /// Remove the autosave recovery files, then quit the application.
    pub(in crate::app) fn exit_app(&self) -> Task<Message> {
        self.cleanup_autosaves();
        iced::exit()
    }

    /// Write the given plot page settings into the active layout.
    /// No-op on the Model tab (which has no paper layout). Marks the tab dirty
    /// and re-tessellates the sheet. Called by the Plot dialog's Set current action.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_plot_page_settings(
        &mut self,
        w: f64,
        h: f64,
        plot_area: &str,
        center: bool,
        offset_x: f64,
        offset_y: f64,
        rotation: i16,
    ) {
        let i = self.active_tab;
        let dialog = self.plot_dialog.clone();
        let layout_name = self.tabs[i].scene.current_layout.clone();
        let plot_window = self.plot_window;
        if layout_name != "Model" {
            let w: f64 = w.max(1.0);
            let h: f64 = h.max(1.0);
            use acadrust::objects::{
                PlotRotation, PlotSettings, PlotType, ScaledType, ShadePlotMode,
                ShadePlotResolutionLevel,
            };
            let mut ps = self
                .plot_setup_template
                .clone()
                .or_else(|| self.tabs[i].scene.plot_settings_for(&layout_name))
                .unwrap_or_else(|| PlotSettings::new(""));
            ps.paper_width = w;
            ps.paper_height = h;
            ps.paper_size = dialog.paper.clone();
            ps.plot_type = match plot_area {
                "Window" => PlotType::Window,
                "Display" => PlotType::LastScreenDisplay,
                "Extents" => PlotType::Extents,
                "Limits" => PlotType::Limits,
                area if area.starts_with("View: ") => PlotType::View,
                _ => PlotType::Layout,
            };
            ps.plot_view_name = plot_area
                .strip_prefix("View: ")
                .unwrap_or("")
                .to_string();
            if plot_area == "Window" {
                if let Some((x0, y0, x1, y1)) = plot_window {
                    ps.set_plot_window(x0, y0, x1, y1);
                }
            }
            ps.flags.plot_centered = center && plot_area != "Layout";
            ps.origin_x = if plot_area == "Layout" { 0.0 } else { offset_x };
            ps.origin_y = if plot_area == "Layout" { 0.0 } else { offset_y };
            ps.rotation = match rotation {
                90 => PlotRotation::Degrees90,
                180 => PlotRotation::Degrees180,
                270 => PlotRotation::Degrees270,
                _ => PlotRotation::None,
            };
            if dialog.fit_to_paper && plot_area != "Layout" {
                ps.set_scale_to_fit();
            } else if plot_area == "Layout" {
                ps.set_standard_scale(ScaledType::OneToOne);
                ps.standard_scale_factor = 1.0;
            } else {
                let factor = plot_dialog_scale_factor(&dialog);
                ps.scale_type = ScaledType::CustomScale;
                ps.scale_numerator = factor;
                ps.scale_denominator = 1.0;
                ps.standard_scale_factor = factor;
                ps.flags.use_standard_scale = false;
            }
            ps.printer_name = if dialog.to_file {
                crate::ui::window::plot::OUT_PDF.into()
            } else {
                dialog.printer.clone().unwrap_or_default()
            };
            ps.current_style_sheet = dialog.style_name.clone();
            ps.flags.scale_lineweights = dialog.scale_lw;
            ps.flags.print_lineweights = dialog.lineweights;
            ps.flags.plot_plot_styles = dialog.apply_plot_styles && !dialog.style_name.is_empty();
            ps.flags.show_plot_styles = dialog.show_plot_styles && !dialog.style_name.is_empty();
            ps.flags.draw_viewports_first = dialog.paperspace_last;
            ps.flags.plot_hidden = dialog.shade == "Hidden Line";
            ps.shade_plot_mode = match dialog.shade.as_str() {
                "2D Wireframe" | "3D Wireframe" => ShadePlotMode::Wireframe,
                "Hidden Line" => ShadePlotMode::Hidden,
                "As displayed" => ShadePlotMode::AsDisplayed,
                _ => ShadePlotMode::Rendered,
            };
            ps.shade_plot_resolution = match dialog.quality.as_str() {
                "Low" => ShadePlotResolutionLevel::Draft,
                "High" => ShadePlotResolutionLevel::Presentation,
                _ => ShadePlotResolutionLevel::Normal,
            };
            ps.shade_plot_dpi = 300;

            self.tabs[i]
                .scene
                .set_layout_plot_settings(&layout_name, &ps);
            for obj in self.tabs[i].scene.document.objects.values_mut() {
                if let acadrust::objects::ObjectType::Layout(layout) = obj {
                    if layout.name == layout_name {
                        layout.min_limits = (0.0, 0.0);
                        layout.max_limits = (w, h);
                        layout.min_extents = (0.0, 0.0, 0.0);
                        layout.max_extents = (w, h, 0.0);
                        break;
                    }
                }
            }

            self.tabs[i].dirty = true;
            self.tabs[i].scene.bump_geometry_no_blocks();
            self.command_line.push_info(crate::tf!(
                "Page setup: {w:.1}×{h:.1} mm  area={plot_area}  \
                 center={center}  rot={rotation}°"
            ).as_ref());
        }
    }

    pub(in crate::app) fn on_plot_export_path_some(
        &mut self,
        path: std::path::PathBuf,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].scene.current_layout != "Model" {
            self.plot_dialog.paper_space = true;
            self.plot_dialog.scales = self.tabs[i]
                .scene
                .scale_list()
                .into_iter()
                .map(|(name, _, factor)| (name, factor))
                .collect();
            if let Some(settings) = self.tabs[i].scene.effective_plot_settings() {
                self.load_plotsettings_into_dialog(&settings);
            }
        }
        let Some((wires, hatches, wipeouts, group_splits, page_w, page_h, ox, oy, rotation, scale, clip)) =
            self.direct_plot_params()
        else {
            self.command_line
                .push_error(crate::t!("Nothing to plot: model space contains no printable geometry.").as_ref());
            return Task::none();
        };
        let plot_style = self.dialog_plot_style(&self.plot_dialog);
        let render_options = Self::pdf_plot_options(&self.plot_dialog, group_splits);
        let worker_path = path.clone();
        let work = move || {
            crate::io::pdf_export::export_pdf(
                    &wires,
                    &hatches,
                    &wipeouts,
                    page_w,
                    page_h,
                    ox,
                    oy,
                    rotation,
                    scale,
                    clip,
                    &worker_path,
                    plot_style.as_ref(),
                    render_options,
                )
                .map(|_| format!("Exported: {}", worker_path.display()))
                .map_err(|e| format!("Export failed: {e}"))
        };
        self.run_plot_work(self.plot_dialog.background, false, work)
    }

    /// Export the pending Extents/Window/Display area using the same clipped
    /// render path in model space and paper space.
    pub(super) fn on_plot_window_export_path_some(
        &mut self,
        path: std::path::PathBuf,
    ) -> Task<Message> {
        let job = match self.plot_dialog.area.as_str() {
            "Display" => self.display_plot_job(),
            "Extents" => self.extents_plot_job(),
            _ => self.window_plot_job(),
        };
        let Some((wires, hatches, wipeouts, group_splits, page_w, page_h, ox, oy, rotation, scale, clip)) =
            job
        else {
            self.command_line
                .push_error(crate::t!("Plot area is empty. Pick a larger window.").as_ref());
            return Task::none();
        };
        let plot_style = self.dialog_plot_style(&self.plot_dialog);
        let render_options = Self::pdf_plot_options(&self.plot_dialog, group_splits);
        let worker_path = path.clone();
        self.close_active_modal();
        let work = move || {
            crate::io::pdf_export::export_pdf(
                    &wires,
                    &hatches,
                    &wipeouts,
                    page_w,
                    page_h,
                    ox,
                    oy,
                    rotation,
                    scale,
                    clip,
                    &worker_path,
                    plot_style.as_ref(),
                    render_options,
                )
                .map(|_| {format!(
                            "Plotted window to {}",
                        worker_path
                            .file_name().unwrap_or_default().to_string_lossy()
                        )
                }).map_err(|e| format!("Plot failed: {e}"))
        };
        self.run_plot_work(self.plot_dialog.background, false, work)
    }

    pub(super) fn report_plot_result(
        &mut self,
        result: Result<String, String>,
        reopen_plot: bool,
    ) {
        match result {
            Ok(message) => self.command_line.push_info(&message),
            Err(error) => self.command_line.push_error(&error),
        }
        if reopen_plot {
            self.active_modal = Some(crate::app::ModalKind::Plot);
        }
    }

    pub(super) fn on_print_all_open(&mut self) -> Task<Message> {
        self.print_all_layouts = self.tabs[self.active_tab]
            .scene
            .layout_names()
            .into_iter()
            .filter(|name| name != "Model")
            .map(|name| (name, true))
            .collect();
        self.print_all_settings_override = false;
        self.active_modal = Some(crate::app::ModalKind::PrintAll);
        self.reset_modal_geometry();
        Task::none()
    }

    pub(super) fn on_print_all_options(&mut self) -> Task<Message> {
        let previous = self.plot_dialog.clone();
        let previous_style = self.active_plot_style.clone();
        let previous_window = self.plot_window;
        let previous_setup = self.plot_setup_template.clone();
        let task = self.on_plot_dialog_open();
        self.print_all_options_prev = Some(previous);
        self.print_all_plot_style_prev = Some(previous_style);
        self.print_all_plot_window_prev = Some(previous_window);
        self.print_all_plot_setup_prev = Some(previous_setup);
        self.print_all_options = true;
        self.plot_dialog.paper_space = true;
        self.plot_dialog.area = "Layout".into();
        task
    }

    fn print_all_pages(&mut self) -> Result<Vec<crate::io::pdf_export::PdfPageInput>, String> {
        let available = self.tabs[self.active_tab].scene.layout_names();
        let selected: Vec<String> = self
            .print_all_layouts
            .iter()
            .filter(|(name, checked)| *checked && available.contains(name))
            .map(|(name, _)| name.clone())
            .collect();
        if selected.is_empty() {
            return Err(crate::t!("Select at least one layout.").into_owned());
        }

        let i = self.active_tab;
        let original_layout = self.tabs[i].scene.current_layout.clone();
        let original_viewport = self.tabs[i].scene.active_viewport;
        let original_psltscale = self.tabs[i]
            .scene
            .document
            .header
            .paper_space_linetype_scaling;
        let original_plimcheck = self.tabs[i]
            .scene
            .document
            .header
            .paper_space_limit_check;
        let original_dialog = self.plot_dialog.clone();
        let original_style = self.active_plot_style.clone();
        let original_window = self.plot_window;
        let original_setup = self.plot_setup_template.clone();
        let original_camera = self.tabs[i].scene.camera.borrow().clone();
        let original_camera_generation = self.tabs[i].scene.camera_generation;
        let override_dialog = self.print_all_settings_override.then(|| original_dialog.clone());
        let override_style = self.print_all_settings_override.then(|| original_style.clone());
        let result = (|| {
            let mut pages = Vec::with_capacity(selected.len());
            for name in selected {
                let page_setup = self.tabs[i]
                    .scene
                    .plot_settings_for(&name)
                    .ok_or_else(|| format!("Layout '{name}' has no page setup."))?;
                {
                    let scene = &mut self.tabs[i].scene;
                    scene.current_layout = name.clone();
                    scene.active_viewport = None;
                    scene.load_current_layout_state();
                }
                if let Some(dialog) = &override_dialog {
                    self.plot_dialog = dialog.clone();
                    self.active_plot_style = override_style.clone().flatten();
                    self.plot_window = original_window;
                } else {
                    self.plot_dialog.paper_space = true;
                    self.plot_window = None;
                    self.load_plotsettings_into_dialog(&page_setup);
                }
                let dialog = self.plot_dialog.clone();
                if dialog.area == "Display" {
                    self.tabs[i].scene.restore_saved_camera();
                }
                if dialog.style_missing && dialog.apply_plot_styles {
                    return Err(format!(
                        "Layout '{name}' plot style table '{}' is not loaded.",
                        dialog.style_name
                    ));
                }
                let plot_style = self.dialog_plot_style(&dialog);
                let params = match dialog.area.as_str() {
                    "Display" => self.display_plot_job(),
                    "Extents" => self.extents_plot_job(),
                    "Limits" => self.limits_plot_job(),
                    "Window" => self.window_plot_job(),
                    area if area.starts_with("View: ") => {
                        self.named_view_plot_job(area.trim_start_matches("View: "))
                    }
                    _ => None,
                };
                let (
                    wires,
                    hatches,
                    wipeouts,
                    group_splits,
                    paper_w,
                    paper_h,
                    offset_x,
                    offset_y,
                    rotation_deg,
                    scale,
                    clip,
                ) = if dialog.area == "Layout" {
                    self.layout_plot_params_for("Layout")
                } else {
                    let (
                        wires,
                        hatches,
                        wipeouts,
                        group_splits,
                        paper_w,
                        paper_h,
                        offset_x,
                        offset_y,
                        rotation_deg,
                        scale,
                        clip,
                    ) = params.ok_or_else(|| format!("Layout '{name}' plot area is empty."))?;
                    (
                        std::sync::Arc::new(wires),
                        hatches,
                        wipeouts,
                        group_splits,
                        paper_w,
                        paper_h,
                        offset_x,
                        offset_y,
                        rotation_deg,
                        scale,
                        clip,
                    )
                };
                pages.push(crate::io::pdf_export::PdfPageInput {
                    wires,
                    hatches,
                    wipeouts,
                    paper_w,
                    paper_h,
                    offset_x,
                    offset_y,
                    rotation_deg,
                    scale,
                    clip,
                    options: Self::pdf_plot_options(&dialog, group_splits),
                    plot_style,
                });
            }
            Ok(pages)
        })();
        self.tabs[i].scene.current_layout = original_layout;
        self.tabs[i].scene.active_viewport = original_viewport;
        self.tabs[i]
            .scene
            .document
            .header
            .paper_space_linetype_scaling = original_psltscale;
        self.tabs[i]
            .scene
            .document
            .header
            .paper_space_limit_check = original_plimcheck;
        self.plot_dialog = original_dialog;
        self.active_plot_style = original_style;
        self.plot_window = original_window;
        self.plot_setup_template = original_setup;
        *self.tabs[i].scene.camera.borrow_mut() = original_camera;
        self.tabs[i].scene.camera_generation = original_camera_generation;
        result
    }

    pub(super) fn on_print_all_pdf_path_some(
        &mut self,
        path: std::path::PathBuf,
    ) -> Task<Message> {
        let dialog = self.plot_dialog.clone();
        if self.print_all_settings_override
            && dialog.style_missing
            && dialog.apply_plot_styles
        {
            self.command_line.push_error(crate::tf!(
                "Plot style table '{}' is not loaded.",
                dialog.style_name
            ).as_ref());
            return Task::none();
        }
        let pages = match self.print_all_pages() {
            Ok(pages) => pages,
            Err(error) => {
                self.command_line.push_error(&error);
                return Task::none();
            }
        };
        let worker_path = path.clone();
        self.save_config();
        self.close_active_modal();
        let work = move || {
            crate::io::pdf_export::export_pdf_pages(
                &pages,
                &worker_path,
                None,
            )
            .map(|_| format!("Exported {} layouts to {}", pages.len(), worker_path.display()))
            .map_err(|error| format!("Export failed: {error}"))
        };
        self.run_print_all_work(dialog.background, work)
    }

    pub(super) fn on_print_all_print(&mut self) -> Task<Message> {
        #[cfg(target_arch = "wasm32")]
        {
            self.command_line.push_error(
                crate::t!("Printing is not available in the web version.").as_ref(),
            );
            Task::none()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dialog = self.plot_dialog.clone();
            if self.print_all_settings_override
                && dialog.style_missing
                && dialog.apply_plot_styles
            {
                self.command_line.push_error(crate::tf!(
                    "Plot style table '{}' is not loaded.",
                    dialog.style_name
                ).as_ref());
                return Task::none();
            }
            let pages = match self.print_all_pages() {
                Ok(pages) => pages,
                Err(error) => {
                    self.command_line.push_error(&error);
                    return Task::none();
                }
            };
            let options = self.plot_print_options(&dialog, Default::default());
            let temp_path = crate::io::print_to_printer::temp_pdf_path("print_all");
            self.save_config();
            self.close_active_modal();
            self.command_line.push_info(
                crate::t!("Sending selected layouts to the system printer…").as_ref(),
            );
            let work = move || {
                crate::io::pdf_export::export_pdf_pages(
                    &pages,
                    &temp_path,
                    None,
                )
                .and_then(|_| {
                    crate::io::print_to_printer::print_existing_pdf(&temp_path, &options)
                })
                .map(|printer| format!("Sent {} layouts to printer: {printer}", pages.len()))
                .map_err(|error| format!("Print failed: {error}"))
            };
            self.run_print_all_work(true, work)
        }
    }

    fn run_print_all_work<F>(&mut self, background: bool, work: F) -> Task<Message>
    where
        F: FnOnce() -> Result<String, String> + Send + 'static,
    {
        if background {
            background_task(work, Message::PrintAllFinished)
        } else {
            Task::done(Message::PrintAllFinished(work()))
        }
    }

    fn run_plot_work<F>(
        &mut self,
        background: bool,
        reopen_plot: bool,
        work: F,
    ) -> Task<Message>
    where
        F: FnOnce() -> Result<String, String> + Send + 'static,
    {
        if background {
            background_task(work, move |result| {
                Message::BackgroundIoFinished(result, reopen_plot)
            })
        } else {
            let result = work();
            self.report_plot_result(result, reopen_plot);
            Task::none()
        }
    }

    /// Build the render inputs and page geometry for a full-layout plot: wires
    /// plus hatch / wipeout fills, the effective (rotation-swapped) sheet size,
    /// draw-origin offset, rotation, unit scale, and optional clip. Shared by
    /// PDF export, preview, and printer output so all three render identically.
    pub(super) fn layout_plot_params(&self) -> LayoutPlotParams {
        self.layout_plot_params_for(&self.plot_dialog.area)
    }

    fn layout_plot_params_for(&self, plot_area: &str) -> LayoutPlotParams {
        let i = self.active_tab;
        let scene = &self.tabs[i].scene;
        let paper_space = scene.current_layout != "Model";
        let selected_sheet = plot_dialog_sheet_mm(&self.plot_dialog);
        let (source_wires, hatches, wipeouts, mut group_splits) =
            plot_scene_content(
                scene,
                self.plot_dialog.paperspace_last,
                plot_render_mode_override(&self.plot_dialog),
            );
        // The printable-area rectangle is an on-screen guide, not drawing
        // content. It used to leak into every paper-space PDF/preview/print.
        let wires = if paper_space {
            group_splits.wires = source_wires[..group_splits.wires.min(source_wires.len())]
                .iter()
                .filter(|wire| wire.name != "paper_printable_area")
                .count();
            std::sync::Arc::new(
                source_wires
                    .iter()
                    .filter(|wire| wire.name != "paper_printable_area")
                    .cloned()
                    .collect(),
            )
        } else {
            source_wires
        };
        if let Some(((x0, y0), (x1, y1))) = scene.paper_limits() {
            // Paper geometry is stored in layout units; PDF pages use mm.
            // Scale both geometry and offsets by the same unit conversion.
            // The old path converted only page width/height, so inch/scaled
            // layouts placed tiny or 25.4×-shifted content on the page.
            let units_per_mm = scene.paper_space_unit_factor().max(1e-9);
            let mm_per_unit = 1.0 / units_per_mm;
            let paper_w = ((x1 - x0) * mm_per_unit).max(1.0);
            let paper_h = ((y1 - y0) * mm_per_unit).max(1.0);
            // Positioning choices are transient until explicitly saved. Layout
            // area keeps the sheet's current physical bounds; the other area
            // modes use the dialog paper size through `area_plot_job`.
            let dialog_rotation = if self.plot_dialog.upside_down { 180 } else { 0 };
            let plot_extents = plot_area == "Extents";
            let plot_layout = plot_area == "Layout";
            // Page orientation is already represented by the sheet bounds.
            // Only the explicit upside-down choice rotates layout content here.
            let rotation = dialog_rotation;
            let centered = self.plot_dialog.center;
            let origin_x = self
                .plot_dialog
                .offset_x
                .parse::<f64>()
                .unwrap_or(0.0);
            let origin_y = self
                .plot_dialog
                .offset_y
                .parse::<f64>()
                .unwrap_or(0.0);

            let (scale, offset_x, offset_y) = if plot_extents {
                let (min_x, min_y, max_x, max_y) =
                    plot_content_extents(&wires, &hatches, &wipeouts)
                        .unwrap_or((x0, y0, x1, y1));
                let content_w = (max_x - min_x).max(1e-9);
                let content_h = (max_y - min_y).max(1e-9);
                let scale = if self.plot_dialog.fit_to_paper {
                    const MARGIN: f64 = 1.05;
                    ((paper_w / MARGIN) / content_w)
                        .min((paper_h / MARGIN) / content_h)
                        .max(1e-9)
                } else {
                    (plot_dialog_scale_factor(&self.plot_dialog) * mm_per_unit).max(1e-9)
                };
                let target_x = if centered {
                    (paper_w - content_w * scale) * 0.5
                } else {
                    origin_x
                };
                let target_y = if centered {
                    (paper_h - content_h * scale) * 0.5
                } else {
                    origin_y
                };
                (
                    scale,
                    target_x / scale - min_x,
                    target_y / scale - min_y,
                )
            } else if plot_layout {
                // Map the complete paper-space sheet to the physical paper
                // selected in the dialog. Layout bounds can be stored in mm,
                // inches, or carry incomplete legacy metadata; deriving the
                // scale from the visible sheet avoids applying a stale unit
                // factor twice while preserving loaded layouts exactly when
                // their metadata is valid.
                let bounds_w = (x1 - x0).max(1e-9);
                let bounds_h = (y1 - y0).max(1e-9);
                let scale = (selected_sheet.0 / bounds_w)
                    .min(selected_sheet.1 / bounds_h)
                    .max(1e-9);
                let target_x = (selected_sheet.0 - bounds_w * scale) * 0.5;
                let target_y = (selected_sheet.1 - bounds_h * scale) * 0.5;
                (
                    scale,
                    target_x / scale - x0,
                    target_y / scale - y0,
                )
            } else {
                // Legacy direct callers still get dialog positioning. Normal
                // Display/Window paths use `area_plot_job` instead.
                let scale = mm_per_unit;
                let target_x = if centered {
                    (paper_w - (x1 - x0) * scale) * 0.5
                } else {
                    origin_x
                };
                let target_y = if centered {
                    (paper_h - (y1 - y0) * scale) * 0.5
                } else {
                    origin_y
                };
                (scale, target_x / scale - x0, target_y / scale - y0)
            };
            let (base_page_w, base_page_h) = if plot_layout {
                selected_sheet
            } else {
                (paper_w, paper_h)
            };
            let (page_w, page_h) = match rotation {
                90 | 270 => (base_page_h, base_page_w),
                _ => (base_page_w, base_page_h),
            };
            // Layout plots keep the full physical sheet as the PDF page, but
            // ink is restricted to the device's printable rectangle. Extents
            // has its own fitted area and must not inherit this paper clip.
            let clip = if plot_layout {
                scene.printable_area_limits().map(|((px0, py0), (px1, py1))| {
                    (
                        (px0 + offset_x) as f32,
                        (py0 + offset_y) as f32,
                        (px1 - px0) as f32,
                        (py1 - py0) as f32,
                    )
                })
            } else {
                None
            };
            return (
                wires,
                hatches,
                wipeouts,
                group_splits,
                page_w,
                page_h,
                offset_x,
                offset_y,
                rotation,
                scale as f32,
                clip,
            );
        }

        // Model space keeps its established extents + 5% margin behaviour.
        let (page_w, page_h, offset_x, offset_y) =
            if let Some((min, max)) = scene.model_space_extents() {
                let margin = 1.05_f64;
                let width = ((max.x - min.x) as f64 * margin).max(1.0);
                let height = ((max.y - min.y) as f64 * margin).max(1.0);
                let pad_x = (width - (max.x - min.x) as f64) * 0.5;
                let pad_y = (height - (max.y - min.y) as f64) * 0.5;
                (
                    width,
                    height,
                    -(min.x as f64) + pad_x,
                    -(min.y as f64) + pad_y,
                )
            } else {
                (297.0, 210.0, 0.0, 0.0)
            };
        (
            wires,
            hatches,
            wipeouts,
            group_splits,
            page_w,
            page_h,
            offset_x,
            offset_y,
            0,
            1.0,
            None,
        )
    }

    /// Build direct PDF/printer output using the physical layout sheet in
    /// paper space and the selected ISO sheet/scale around Model extents.
    fn direct_plot_params(&self) -> Option<LayoutPlotParams> {
        if self.tabs[self.active_tab].scene.current_layout != "Model" {
            return Some(self.layout_plot_params_for("Layout"));
        }
        let (wires, hatches, wipeouts, group_splits, page_w, page_h, ox, oy, rotation, scale, clip) =
            self.extents_plot_job()?;
        if wires.is_empty() && hatches.is_empty() && wipeouts.is_empty() {
            return None;
        }
        Some((
            std::sync::Arc::new(wires),
            hatches,
            wipeouts,
            group_splits,
            page_w,
            page_h,
            ox,
            oy,
            rotation,
            scale,
            clip,
        ))
    }

    pub(super) fn on_print_to_printer(&mut self) -> Task<Message> {
        let Some((wires, hatches, wipeouts, group_splits, page_w, page_h, ox, oy, rotation, scale, clip)) =
            self.direct_plot_params()
        else {
            self.command_line
                .push_error(crate::t!("Nothing to plot: model space contains no printable geometry.").as_ref());
            return Task::none();
        };
        let plot_style = self.dialog_plot_style(&self.plot_dialog);
        let options = self.plot_print_options(&self.plot_dialog, group_splits);
        self.command_line.push_info(crate::t!("Sending to system printer…").as_ref());
        background_task(
            move || {
                iced::futures::executor::block_on(
                crate::io::print_to_printer::print_wires_with(
                    wires, hatches, wipeouts, page_w, page_h, ox, oy, rotation, scale, clip,
                    plot_style, options,
                ))
            },
            Message::PrintResult,
        )
    }

    /// QUICKPRINT / QP — use the current selection's bounding box as the plot
    /// window and export a PDF (drawing folder + name + timestamp) with the
    /// active page setup, no dialog. Model space only. (#325)
    pub(crate) fn on_quick_print_handles(
        &mut self,
        handles: Vec<acadrust::Handle>,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].scene.current_layout != "Model" {
            self.command_line
                .push_error(crate::t!("Quick print works in model space.").as_ref());
            return Task::none();
        }
        let set: std::collections::HashSet<acadrust::Handle> = handles.into_iter().collect();
        // Union the AABBs of the picked entities' wires (world XY), matched by
        // each wire's handle.
        let (x0, y0, x1, y1, any) = {
            let scene = &self.tabs[i].scene;
            let mut x0 = f32::INFINITY;
            let mut y0 = f32::INFINITY;
            let mut x1 = f32::NEG_INFINITY;
            let mut y1 = f32::NEG_INFINITY;
            let mut any = false;
            for w in scene.entity_wires().iter() {
                let picked = crate::scene::Scene::handle_from_wire_name(&w.name)
                    .is_some_and(|h| set.contains(&h));
                if !picked {
                    continue;
                }
                let [ax0, ay0, ax1, ay1] = w.aabb;
                if ax0.is_finite() && ay0.is_finite() && ax1.is_finite() && ay1.is_finite() {
                    x0 = x0.min(ax0);
                    y0 = y0.min(ay0);
                    x1 = x1.max(ax1);
                    y1 = y1.max(ay1);
                    any = true;
                }
            }
            (x0, y0, x1, y1, any)
        };
        if !any {
            self.command_line
                .push_error(crate::t!("Selection has no printable geometry.").as_ref());
            return Task::none();
        }
        if !(x1 > x0 && y1 > y0) {
            self.command_line
                .push_error(crate::t!("Selection has no printable area.").as_ref());
            return Task::none();
        }
        // Small margin so the outermost strokes aren't clipped flush to the edge.
        let mx = ((x1 - x0) * 0.02).max(0.0);
        let my = ((y1 - y0) * 0.02).max(0.0);
        self.plot_window = Some((
            (x0 - mx) as f64,
            (y0 - my) as f64,
            (x1 + mx) as f64,
            (y1 + my) as f64,
        ));
        let path = self.quick_print_path();
        self.on_plot_window_export_path_some(path)
    }

    /// Auto output path for quick print: the drawing's folder + name + a
    /// timestamp, falling back to the temp dir / "drawing" when unsaved.
    fn quick_print_path(&self) -> std::path::PathBuf {
        let i = self.active_tab;
        let cur = self.tabs[i].current_path.as_deref();
        let dir = cur
            .and_then(|p| p.parent())
            .map(|d| d.to_path_buf())
            .unwrap_or_else(std::env::temp_dir);
        let stem = cur
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "drawing".into());
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        dir.join(format!("{stem}_{ts}.pdf"))
    }

    /// Open the full Plot / Print dialog, seeding its state from the active
    /// layout's plot settings and the printers found on the system.
    pub(super) fn on_plot_dialog_open(&mut self) -> Task<Message> {
        use crate::io::paper_sizes::Orientation;
        let previous = self.plot_dialog.clone();
        let scales: Vec<(String, f64)> = self.tabs[self.active_tab]
            .scene
            .scale_list()
            .into_iter()
            .map(|(name, _, factor)| (name, factor))
            .collect();
        let one_to_one = scale_name_for_factor(&scales, 1.0)
            .or_else(|| scales.first().map(|(name, _)| name.clone()))
            .unwrap_or_else(|| "1:1".into());
        let paper_space = self.tabs[self.active_tab].scene.current_layout != "Model";
        let plot_views = self.tabs[self.active_tab]
            .scene
            .document
            .views
            .iter()
            .filter(|view| view.paper_space == paper_space)
            .map(|view| view.name.clone())
            .collect();
        // Keep session-only choices while loading drawing fields from the layout.
        let d = &mut self.plot_dialog;
        d.printers = crate::io::print_to_printer::list_printers();
        d.plot_styles = crate::io::plot_style::available_ctb_names();
        d.scales = scales;
        d.plot_views = plot_views;
        if d.scale.eq_ignore_ascii_case("fit") {
            d.fit_to_paper = true;
            d.scale_lw = false;
            d.scale = one_to_one.clone();
        } else if !d.scales.iter().any(|(name, _)| name == &d.scale) {
            d.scale = one_to_one.clone();
        }
        d.paper_space = paper_space;
        d.paper = self.plot_format.label().to_string();
        d.orientation = match self.plot_orientation {
            Orientation::Portrait => "Portrait",
            Orientation::Landscape => "Landscape",
        }
        .to_string();
        d.quality = match d.quality.as_str() {
            "Low" | "Draft" => "Low",
            "High" | "Presentation" | "Maximum" | "Custom" => "High",
            _ => "Normal",
        }
        .into();
        d.shade = match d.shade.as_str() {
            "Wireframe" => "2D Wireframe",
            value if value.starts_with("As displayed") => "As displayed",
            "2D Wireframe"
            | "3D Wireframe"
            | "Hidden Line"
            | "Flat Shaded"
            | "Gouraud Shaded"
            | "Flat Shaded + Edges"
            | "Gouraud Shaded + Edges" => d.shade.as_str(),
            _ => "As displayed",
        }
        .into();
        d.style_name = self
            .active_plot_style
            .as_ref()
            .map(|t| t.name.clone())
            .unwrap_or_default();
        d.style_missing = false;
        self.plot_dialog.name_input = None;
        self.plot_dialog.name_rename = false;
        if self.plot_dialog.fit_to_paper {
            self.plot_dialog.scale_lw = false;
        }
        self.refresh_page_setups();
        self.plot_prev = Some(previous);
        let cur = self.tabs[self.active_tab].scene.current_layout.clone();
        let layout_entry = format!("*{cur}*");
        if self.tabs[self.active_tab]
            .scene
            .plot_settings_for(&cur)
            .is_some()
        {
            self.select_page_setup(&layout_entry);
        } else {
            self.select_page_setup(crate::ui::window::plot::SETUP_PREV);
        }
        if self.plot_dialog.scale.eq_ignore_ascii_case("fit") {
            self.plot_dialog.fit_to_paper = true;
            self.plot_dialog.scale = one_to_one.clone();
        } else if !self
            .plot_dialog
            .scales
            .iter()
            .any(|(name, _)| name == &self.plot_dialog.scale)
        {
            self.plot_dialog.scale = one_to_one;
        }
        if self.plot_dialog.fit_to_paper {
            self.plot_dialog.scale_lw = false;
        }
        // A model-space plot cannot use the paper-only Layout area.
        if cur == "Model" {
            if self.plot_dialog.area == "Layout" {
                self.plot_dialog.area = "Window".into();
                self.normalize_common_plot_dialog();
            }
        }
        self.active_modal = Some(crate::app::ModalKind::Plot);
        Task::none()
    }

    fn normalize_common_plot_dialog(&mut self) {
        self.plot_dialog.center = true;
        self.plot_dialog.offset_x = "0.0".into();
        self.plot_dialog.offset_y = "0.0".into();
        self.plot_dialog.fit_to_paper = true;
        self.plot_dialog.scale_lw = false;
    }

    /// Handle one edit / action from the Plot dialog.
    pub(super) fn on_plot_dlg(
        &mut self,
        msg: crate::ui::window::plot::PlotDlgMsg,
    ) -> Task<Message> {
        use crate::ui::window::plot::{
            PlotDlgMsg as M, PlotFlag, OUT_DEFAULT, OUT_PDF, STYLE_NONE,
        };
        match msg {
            M::Close => {
                self.close_active_modal();
                Task::none()
            }
            M::Printer(s) => {
                if s == OUT_PDF {
                    self.plot_dialog.to_file = true;
                } else if s == OUT_DEFAULT {
                    self.plot_dialog.to_file = false;
                    self.plot_dialog.printer = None;
                } else {
                    self.plot_dialog.to_file = false;
                    self.plot_dialog.printer = Some(s);
                }
                Task::none()
            }
            M::PrinterProperties => {
                match crate::io::print_to_printer::open_printer_properties(
                    self.plot_dialog.printer.as_deref(),
                ) {
                    Ok(()) => self.command_line.push_info(crate::t!("Opened printer properties.").as_ref()),
                    Err(error) => self.command_line.push_error(&error),
                }
                Task::none()
            }
            M::Paper(s) => {
                self.plot_dialog.paper = s;
                let (w, h) = plot_dialog_sheet_mm(&self.plot_dialog);
                self.plot_dialog.paper_width_mm = w;
                self.plot_dialog.paper_height_mm = h;
                Task::none()
            }
            M::Orientation(s) => {
                self.plot_dialog.orientation = s;
                let (w, h) = plot_dialog_sheet_mm(&self.plot_dialog);
                self.plot_dialog.paper_width_mm = w;
                self.plot_dialog.paper_height_mm = h;
                Task::none()
            }
            M::Area(s) => {
                if self.print_all_options && s != "Layout" {
                    return Task::none();
                }
                // A paper Layout page setup commonly carries physical-sheet
                // origin and 1:1 scale. Those values are correct only for
                // Layout; carrying them into Window/Extents/Display moves a
                // large selected area off-page. Enter the common area modes
                // with centered, fit-to-paper defaults.
                if self.plot_dialog.area == "Layout" && s != "Layout" {
                    self.normalize_common_plot_dialog();
                }
                if s == "Layout" {
                    self.plot_dialog.center = false;
                    self.plot_dialog.offset_x = "0.0".into();
                    self.plot_dialog.offset_y = "0.0".into();
                    self.plot_dialog.fit_to_paper = false;
                    self.plot_dialog.scale = scale_name_for_factor(
                        &self.plot_dialog.scales,
                        1.0,
                    )
                    .unwrap_or_else(|| "1:1".into());
                }
                self.plot_dialog.area = s;
                Task::none()
            }
            M::Scale(s) => {
                if self.plot_dialog.area != "Layout" {
                    self.plot_dialog.scale = s;
                }
                Task::none()
            }
            M::Quality(s) => {
                self.plot_dialog.quality = s;
                Task::none()
            }
            M::Shade(s) => {
                self.plot_dialog.shade = s;
                Task::none()
            }
            M::Copies(s) => {
                self.plot_dialog.copies = s;
                Task::none()
            }
            M::OffsetX(s) => {
                self.plot_dialog.offset_x = s;
                Task::none()
            }
            M::OffsetY(s) => {
                self.plot_dialog.offset_y = s;
                Task::none()
            }
            M::Flag(f) => {
                let d = &mut self.plot_dialog;
                match f {
                    PlotFlag::Background => d.background = !d.background,
                    PlotFlag::MergeLines => d.merge_lines = !d.merge_lines,
                    PlotFlag::FitToPaper if d.area != "Layout" => {
                        d.fit_to_paper = !d.fit_to_paper;
                        if d.fit_to_paper {
                            d.scale_lw = false;
                        }
                    }
                    PlotFlag::Center if d.area != "Layout" => d.center = !d.center,
                    PlotFlag::ScaleLw if !d.fit_to_paper => {
                        d.scale_lw = !d.scale_lw
                    }
                    PlotFlag::PlotStyles if !d.style_name.is_empty() => {
                        d.apply_plot_styles = !d.apply_plot_styles
                    }
                    PlotFlag::DisplayStyles if d.paper_space && !d.style_name.is_empty() => {
                        d.show_plot_styles = !d.show_plot_styles
                    }
                    PlotFlag::UpsideDown => {
                        d.upside_down = !d.upside_down;
                    }
                    PlotFlag::Lineweights => d.lineweights = !d.lineweights,
                    PlotFlag::Transparency => d.transparency = !d.transparency,
                    PlotFlag::PaperspaceLast if d.paper_space => {
                        d.paperspace_last = !d.paperspace_last
                    }
                    PlotFlag::Stamp => d.stamp = !d.stamp,
                    _ => {}
                }
                Task::none()
            }
            M::LoadStyle => Task::done(Message::PlotStyleLoad),
            M::SaveStyle => Task::done(Message::PlotStylePanelSave),
            M::Style(name) => {
                if name == STYLE_NONE {
                    self.active_plot_style = None;
                    self.plot_dialog.style_name.clear();
                    self.plot_dialog.apply_plot_styles = false;
                    self.plot_dialog.show_plot_styles = false;
                    self.plot_dialog.style_missing = false;
                } else {
                    match crate::io::plot_style::PlotStyleTable::load_named(&name) {
                        Ok(table) => {
                            self.plot_dialog.style_name = table.name.clone();
                            self.plot_dialog.apply_plot_styles = true;
                            self.plot_dialog.style_missing = false;
                            self.active_plot_style = Some(table);
                        }
                        Err(error) => {
                            self.plot_dialog.style_name = name;
                            self.plot_dialog.style_missing = true;
                            self.command_line.push_error(&error);
                        }
                    }
                }
                Task::none()
            }
            M::PickWindow => {
                if self.print_all_options {
                    return Task::none();
                }
                let i = self.active_tab;
                if self.tabs[i].scene.current_layout != "Model"
                    && self.tabs[i].scene.active_viewport.is_some()
                {
                    // A layout Window is selected in paper coordinates and may
                    // extend anywhere on the canvas, including outside the sheet.
                    // Leave MSPACE so the floating viewport cannot constrain the
                    // two picks to its model view.
                    self.tabs[i].scene.deselect_all();
                    self.tabs[i].scene.active_viewport = None;
                    self.adopt_view_display(i);
                    self.tabs[i].refresh_active_ucs();
                    self.refresh_properties();
                    self.sync_dyn_fields();
                }
                self.close_active_modal();
                Task::done(Message::Command("PLOTWINDOW".into()))
            }
            M::SelectSetup(name) => {
                self.select_page_setup(&name);
                if self.print_all_options {
                    self.plot_dialog.paper_space = true;
                    self.plot_dialog.area = "Layout".into();
                }
                Task::none()
            }
            M::SetCurrent => { 
                self.apply_dialog_to_layout();
                self.command_line.push_info(crate::t!("Page setup applied to the layout.").as_ref());
                Task::none()
            }
            M::NewSetup => {
                if self.print_all_options {
                    return Task::none();
                }
                // Create a setup from the current editor values, then start an
                // inline rename so the user can name it.
                let name = self.next_page_setup_name("Setup");
                let ps = self.dialog_to_plotsettings();
                self.tabs[self.active_tab].scene.page_setup_save(&name, ps);
                self.tabs[self.active_tab].dirty = true;
                self.plot_dialog.selected_setup = name.clone();
                self.refresh_page_setups();
                self.plot_dialog.name_input = Some(name);
                self.plot_dialog.name_rename = true;
                Task::none()
            }
            M::CopySetup => {
                if self.print_all_options {
                    return Task::none();
                }
                // Duplicate the selected entry — a layout OR a named setup —
                // into a new standalone named page setup.
                let sel = self.plot_dialog.selected_setup.clone();
                let scene = &self.tabs[self.active_tab].scene;
                let (src_ps, base) = if is_layout_entry(&sel) {
                    let ln = layout_entry_name(&sel);
                    (scene.plot_settings_for(ln), format!("{ln} copy"))
                } else {
                    (scene.page_setup_get(&sel), format!("{sel} copy"))
                };
                if let Some(ps) = src_ps {
                    let name = self.next_page_setup_name(&base);
                    self.tabs[self.active_tab].scene.page_setup_save(&name, ps);
                    self.tabs[self.active_tab].dirty = true;
                    self.plot_dialog.selected_setup = name.clone();
                    self.refresh_page_setups();
                    self.plot_dialog.name_input = Some(name);
                    self.plot_dialog.name_rename = true;
                }
                Task::none()
            }
            M::RenameStart(name) => {
                if self.print_all_options {
                    return Task::none();
                }
                // Only standalone named setups can be renamed.
                if is_layout_entry(&name) || is_special_entry(&name) {
                    return Task::none();
                }
                self.plot_dialog.selected_setup = name.clone();
                if let Some(ps) = self.tabs[self.active_tab].scene.page_setup_get(&name) {
                    self.load_plotsettings_into_dialog(&ps);
                }
                self.plot_dialog.name_input = Some(name);
                self.plot_dialog.name_rename = true;
                Task::none()
            }
            M::DeleteSetup => {
                if self.print_all_options {
                    return Task::none();
                }
                let sel = self.plot_dialog.selected_setup.clone();
                if !sel.is_empty() && !is_layout_entry(&sel) {
                    self.tabs[self.active_tab].scene.page_setup_delete(&sel);
                    self.tabs[self.active_tab].dirty = true;
                    self.plot_dialog.selected_setup.clear();
                    self.refresh_page_setups();
                }
                Task::none()
            }
            M::NameInput(s) => {
                self.plot_dialog.name_input = Some(s);
                Task::none()
            }
            M::NameCommit => {
                if let Some(name) = self.plot_dialog.name_input.take() {
                    let name = name.trim().to_string();
                    if !name.is_empty() {
                        if self.plot_dialog.name_rename {
                            let old = self.plot_dialog.selected_setup.clone();
                            self.tabs[self.active_tab].scene.page_setup_rename(&old, &name);
                        } else {
                            let ps = self.dialog_to_plotsettings();
                            self.tabs[self.active_tab].scene.page_setup_save(&name, ps);
                        }
                        self.tabs[self.active_tab].dirty = true;
                        self.plot_dialog.selected_setup = name;
                        self.refresh_page_setups();
                    }
                }
                self.plot_dialog.name_rename = false;
                Task::none()
            }
            M::NameCancel => {
                self.plot_dialog.name_input = None;
                self.plot_dialog.name_rename = false;
                Task::none()
            }
            M::Preview => self.on_plot_dlg_commit(true),
            M::Commit if self.print_all_options => {
                if self.plot_dialog.style_missing && self.plot_dialog.apply_plot_styles {
                    self.command_line.push_error(crate::tf!(
                        "Plot style table '{}' is not loaded.",
                        self.plot_dialog.style_name
                    ).as_ref());
                    return Task::none();
                }
                self.plot_dialog.paper_space = true;
                self.plot_dialog.area = "Layout".into();
                self.sync_dialog_plot_runtime();
                self.save_config();
                self.print_all_settings_override = true;
                self.print_all_options = false;
                self.print_all_options_prev = None;
                self.print_all_plot_style_prev = None;
                self.print_all_plot_setup_prev = None;
                if let Some(previous) = self.print_all_plot_window_prev.take() {
                    self.plot_window = previous;
                }
                self.active_modal = Some(crate::app::ModalKind::PrintAll);
                self.reset_modal_geometry();
                Task::none()
            }
            M::Commit => self.on_plot_dlg_commit(false),
        }
    }

    fn refresh_page_setups(&mut self) {
        use crate::ui::window::plot::{SETUP_NONE, SETUP_PREV};
        let scene = &self.tabs[self.active_tab].scene;
        // <none> / <previous>, then layouts (`*name*`), then named setups.
        let mut list = vec![SETUP_NONE.to_string(), SETUP_PREV.to_string()];
        list.extend(scene.layout_names().into_iter().map(|n| format!("*{n}*")));
        list.extend(scene.page_setup_names());
        self.plot_dialog.page_setups = list;
    }

    /// Apply a page-setup list selection to the editor. Handles the pseudo
    /// entries (`<none>` / `<previous>`), layout rows (`*name*`) and named
    /// setups.
    fn select_page_setup(&mut self, name: &str) {
        use crate::ui::window::plot::{SETUP_NONE, SETUP_PREV};
        self.plot_dialog.selected_setup = name.to_string();
        if name == SETUP_NONE {
            self.plot_setup_template = None;
            // No page setup: default geometry + PDF output.
            let is_model = self.tabs[self.active_tab].scene.current_layout == "Model";
            let d = &mut self.plot_dialog;
            d.to_file = true;
            d.paper = "A4".into();
            d.orientation = "Landscape".into();
            d.paper_width_mm = 297.0;
            d.paper_height_mm = 210.0;
            d.area = if is_model { "Window".into() } else { "Layout".into() };
            d.center = true;
            d.offset_x = "0.0".into();
            d.offset_y = "0.0".into();
            d.upside_down = false;
            d.fit_to_paper = is_model;
            d.scale_lw = false;
            d.scale = scale_name_for_factor(&d.scales, 1.0)
                .or_else(|| d.scales.first().map(|(name, _)| name.clone()))
                .unwrap_or_else(|| "1:1".into());
        } else if name == SETUP_PREV {
            if let Some(prev) = self.plot_prev.clone() {
                self.plot_dialog.copy_settings_from(&prev);
            }
        } else if is_layout_entry(name) {
            if let Some(ps) = self
                .tabs[self.active_tab]
                .scene
                .plot_settings_for(layout_entry_name(name))
            {
                self.load_plotsettings_into_dialog(&ps);
            }
        } else if let Some(ps) = self.tabs[self.active_tab].scene.page_setup_get(name) {
            self.load_plotsettings_into_dialog(&ps);
        }
    }

    /// A page-setup name based on `base` that isn't already taken (`base`,
    /// `base 2`, `base 3`, …).
    fn next_page_setup_name(&self, base: &str) -> String {
        let existing = self.tabs[self.active_tab].scene.page_setup_names();
        if !existing.iter().any(|n| n == base) {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|c| !existing.iter().any(|n| n == c))
            .unwrap_or_else(|| base.to_string())
    }

    /// Build a `PlotSettings` from the current dialog fields (for saving a named
    /// page setup).
    fn dialog_to_plotsettings(&self) -> acadrust::objects::PlotSettings {
        use acadrust::objects::{
            PlotPaperUnits, PlotRotation, PlotSettings, PlotType, ScaledType, ShadePlotMode,
            ShadePlotResolutionLevel,
        };
        let d = &self.plot_dialog;
        let (w, h) = plot_dialog_sheet_mm(d);
        let mut ps = match self.plot_setup_template.clone() {
            Some(settings) => settings,
            None => {
                let mut settings = PlotSettings::new("");
                settings.paper_units = PlotPaperUnits::Millimeters;
                settings
            }
        };
        ps.paper_width = w;
        ps.paper_height = h;
        ps.paper_size = d.paper.clone();
        ps.plot_type = match d.area.as_str() {
            "Window" => PlotType::Window,
            "Layout" => PlotType::Layout,
            "Display" => PlotType::LastScreenDisplay,
            "Limits" => PlotType::Limits,
            area if area.starts_with("View: ") => PlotType::View,
            _ => PlotType::Extents,
        };
        ps.plot_view_name = d
            .area
            .strip_prefix("View: ")
            .unwrap_or("")
            .to_string();
        if d.area == "Window" {
            if let Some((x0, y0, x1, y1)) = self.plot_window {
                ps.set_plot_window(x0, y0, x1, y1);
            }
        }
        ps.flags.plot_centered = d.center && d.area != "Layout";
        ps.origin_x = if d.area == "Layout" {
            0.0
        } else {
            d.offset_x.parse::<f64>().unwrap_or(0.0)
        };
        ps.origin_y = if d.area == "Layout" {
            0.0
        } else {
            d.offset_y.parse::<f64>().unwrap_or(0.0)
        };
        let rot: i16 = if d.upside_down { 180 } else { 0 };
        ps.rotation = match rot {
            90 => PlotRotation::Degrees90,
            180 => PlotRotation::Degrees180,
            270 => PlotRotation::Degrees270,
            _ => PlotRotation::None,
        };
        if d.area == "Layout" {
            ps.set_standard_scale(ScaledType::OneToOne);
            ps.standard_scale_factor = 1.0;
        } else if d.fit_to_paper {
            ps.set_scale_to_fit();
        } else {
            let factor = plot_dialog_scale_factor(d);
            ps.scale_type = ScaledType::CustomScale;
            ps.scale_numerator = factor;
            ps.scale_denominator = 1.0;
            ps.standard_scale_factor = factor;
            ps.flags.use_standard_scale = false;
        }
        ps.printer_name = if d.to_file {
            crate::ui::window::plot::OUT_PDF.into()
        } else {
            d.printer.clone().unwrap_or_default()
        };
        ps.current_style_sheet = d.style_name.clone();
        ps.flags.scale_lineweights = d.scale_lw;
        ps.flags.print_lineweights = d.lineweights;
        ps.flags.plot_plot_styles = d.apply_plot_styles && !d.style_name.is_empty();
        ps.flags.show_plot_styles = d.show_plot_styles && !d.style_name.is_empty();
        ps.flags.draw_viewports_first = d.paperspace_last;
        ps.flags.plot_hidden = d.shade == "Hidden Line";
        ps.shade_plot_mode = match d.shade.as_str() {
            "2D Wireframe" | "3D Wireframe" => ShadePlotMode::Wireframe,
            "Hidden Line" => ShadePlotMode::Hidden,
            "As displayed" => ShadePlotMode::AsDisplayed,
            _ => ShadePlotMode::Rendered,
        };
        ps.shade_plot_resolution = match d.quality.as_str() {
            "Low" => ShadePlotResolutionLevel::Draft,
            "High" => ShadePlotResolutionLevel::Presentation,
            _ => ShadePlotResolutionLevel::Normal,
        };
        ps.shade_plot_dpi = 300;
        ps
    }

    /// Load a `PlotSettings` into the dialog editor fields.
    fn load_plotsettings_into_dialog(&mut self, ps: &acadrust::objects::PlotSettings) {
        use acadrust::objects::{
            PlotType, ShadePlotMode, ShadePlotResolutionLevel,
        };
        self.plot_setup_template = Some(ps.clone());
        if matches!(ps.plot_type, PlotType::Window) && !ps.plot_window.is_empty() {
            self.plot_window = Some((
                ps.plot_window.lower_left_x,
                ps.plot_window.lower_left_y,
                ps.plot_window.upper_right_x,
                ps.plot_window.upper_right_y,
            ));
        }
        if !ps.current_style_sheet.is_empty()
            && self
            .active_plot_style
            .as_ref()
            .is_none_or(|table| !table.name.eq_ignore_ascii_case(&ps.current_style_sheet))
        {
            if let Ok(table) = crate::io::plot_style::PlotStyleTable::load_named(
                &ps.current_style_sheet,
            ) {
                self.active_plot_style = Some(table);
            }
        }
        let active_style_name = self.active_plot_style.as_ref().map(|table| table.name.clone());
        let style_name = if ps.current_style_sheet.is_empty() {
            String::new()
        } else if active_style_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(&ps.current_style_sheet))
        {
            active_style_name.clone().unwrap_or_default()
        } else {
            ps.current_style_sheet.clone()
        };
        let style_loaded = !style_name.is_empty()
            && active_style_name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(&style_name));
        let (mut paper, orient) = paper_label_from_dims(ps.paper_width, ps.paper_height);
        if !matches!(paper.as_str(), "A4" | "A3" | "A2" | "A1" | "A0")
            && !ps.paper_size.is_empty()
        {
            paper = ps.paper_size.clone();
        }
        let d = &mut self.plot_dialog;
        d.paper = paper;
        d.paper_width_mm = ps.paper_width.max(1.0);
        d.paper_height_mm = ps.paper_height.max(1.0);
        d.orientation = orient;
        d.area = match ps.plot_type {
            PlotType::Window => "Window".to_string(),
            PlotType::Layout => "Layout".to_string(),
            PlotType::LastScreenDisplay => "Display".to_string(),
            PlotType::Limits => "Limits".to_string(),
            PlotType::View if !ps.plot_view_name.is_empty() => {
                d.plot_views.push(ps.plot_view_name.clone());
                d.plot_views.sort();
                d.plot_views.dedup();
                format!("View: {}", ps.plot_view_name)
            }
            _ => "Extents".to_string(),
        };
        if !d.paper_space && d.area == "Layout" {
            d.area = "Extents".into();
        }
        d.center = ps.flags.plot_centered;
        d.offset_x = format!("{:.2}", ps.origin_x);
        d.offset_y = format!("{:.2}", ps.origin_y);
        let deg = ps.rotation.to_degrees() as i32;
        if matches!(deg, 90 | 270) {
            d.orientation = if d.orientation == "Portrait" {
                "Landscape"
            } else {
                "Portrait"
            }
            .into();
        }
        d.upside_down = matches!(deg, 180 | 270);
        d.fit_to_paper = d.area != "Layout" && ps.is_scale_to_fit();
        let target_factor = if d.area == "Layout" {
            1.0
        } else if ps.flags.use_standard_scale {
            if ps.standard_scale_factor.is_finite() && ps.standard_scale_factor > 0.0 {
                ps.standard_scale_factor
            } else {
                ps.scale_type.scale_factor()
            }
        } else if ps.scale_denominator.abs() > 1e-9 {
            ps.scale_numerator / ps.scale_denominator
        } else {
            1.0
        };
        if !d.fit_to_paper || !d.scales.iter().any(|(name, _)| name == &d.scale) {
            d.scale = scale_name_for_factor(&d.scales, target_factor)
                .or_else(|| scale_name_for_factor(&d.scales, 1.0))
                .or_else(|| d.scales.first().map(|(name, _)| name.clone()))
                .unwrap_or_else(|| "1:1".into());
        }
        d.scale_lw = ps.flags.scale_lineweights && !d.fit_to_paper;
        d.lineweights = ps.flags.print_lineweights;
        d.paperspace_last = ps.flags.draw_viewports_first;
        d.shade = if ps.flags.plot_hidden {
            "Hidden Line"
        } else {
            match ps.shade_plot_mode {
                ShadePlotMode::Wireframe => "2D Wireframe",
                ShadePlotMode::Hidden => "Hidden Line",
                ShadePlotMode::Rendered => "Gouraud Shaded",
                ShadePlotMode::AsDisplayed => "As displayed",
            }
        }
        .into();
        d.quality = match ps.shade_plot_resolution {
            ShadePlotResolutionLevel::Draft => "Low",
            ShadePlotResolutionLevel::Presentation
            | ShadePlotResolutionLevel::Maximum
            | ShadePlotResolutionLevel::Custom => "High",
            _ => "Normal",
        }
        .into();
        if ps.printer_name.to_ascii_lowercase().contains("pdf") {
            d.to_file = true;
            d.printer = None;
        } else {
            d.to_file = false;
            d.printer = (!ps.printer_name.is_empty()).then(|| ps.printer_name.clone());
        }
        d.style_name = style_name;
        d.apply_plot_styles = ps.flags.plot_plot_styles;
        d.show_plot_styles = ps.flags.show_plot_styles;
        d.style_missing = !d.style_name.is_empty() && !style_loaded;
    }

    /// Copy transient paper/scale choices out of the dialog without changing
    /// the active layout.
    fn sync_dialog_plot_runtime(&mut self) {
        use crate::io::paper_sizes::{Orientation, PaperSize};
        let d = self.plot_dialog.clone();
        let paper = match d.paper.as_str() {
            "A3" => PaperSize::A3,
            "A2" => PaperSize::A2,
            "A1" => PaperSize::A1,
            "A0" => PaperSize::A0,
            _ => PaperSize::A4,
        };
        let orient = if d.orientation == "Portrait" {
            Orientation::Portrait
        } else {
            Orientation::Landscape
        };
        self.plot_format = paper;
        self.plot_orientation = orient;
    }

    fn apply_dialog_to_layout(&mut self) {
        self.sync_dialog_plot_runtime();
        let d = self.plot_dialog.clone();
        let (sheet_w, sheet_h) = plot_dialog_sheet_mm(&d);
        let rotation: i16 = if d.upside_down { 180 } else { 0 };
        let off_x = d.offset_x.parse::<f64>().unwrap_or(0.0);
        let off_y = d.offset_y.parse::<f64>().unwrap_or(0.0);
        self.apply_plot_page_settings(
            sheet_w,
            sheet_h,
            &d.area,
            d.center,
            off_x,
            off_y,
            rotation,
        );
    }

    /// Open a preview PDF, export a PDF, or send the job to the chosen printer.
    fn on_plot_dlg_commit(&mut self, preview: bool) -> Task<Message> {
        let d = self.plot_dialog.clone();
        if d.style_missing && d.apply_plot_styles {
            self.command_line.push_error(crate::tf!(
                "Plot style table '{}' is not loaded.",
                d.style_name
            ).as_ref());
            return Task::none();
        }
        // Remember the user's print preferences across sessions.
        self.save_config();
        // Preview and normal printing must not resize/mutate the live layout;
        // the runtime paper/scale choices still drive this one plot operation.
        self.sync_dialog_plot_runtime();
        self.active_modal = None;
        self.reset_modal_geometry();

        let plot_style = self.dialog_plot_style(&d);
        // Extents, Window and Display use one plot path in both spaces. Only
        // Paper-space Layout is special: it uses the physical sheet bounds.
        if d.area != "Layout" {
            let job = match d.area.as_str() {
                "Display" => self.display_plot_job(),
                "Extents" => self.extents_plot_job(),
                "Limits" => self.limits_plot_job(),
                "Window" => self.window_plot_job(),
                area if area.starts_with("View: ") => {
                    self.named_view_plot_job(area.trim_start_matches("View: "))
                }
                _ => None,
            };
            let Some((
                w_wires,
                w_hatches,
                w_wipeouts,
                wgroup_splits,
                sw,
                sh,
                wox,
                woy,
                wrotation,
                wscale,
                wclip,
            )) = job
            else {
                self.command_line
                    .push_error(crate::t!("Plot area is empty. Pick a larger window.").as_ref());
                self.active_modal = Some(crate::app::ModalKind::Plot);
                return Task::none();
            };
            // Preview renders to a temp PDF and opens it — never a save dialog,
            // whatever the output target is.
            if preview {
                let tmp = crate::io::print_to_printer::temp_pdf_path("preview");
                let render_options = Self::pdf_plot_options(&d, wgroup_splits);
                let work = move || {
                    crate::io::pdf_export::export_pdf(
                    &w_wires, &w_hatches, &w_wipeouts, sw, sh, wox, woy, wrotation, wscale, wclip, &tmp,
                    plot_style.as_ref(),
                    render_options,
                ).and_then(|_| crate::io::print_to_printer::open_in_viewer(&tmp))
                        .map(|_| "Opened plot preview.".to_string()).map_err(|e| format!("Preview failed: {e}"))
                };
                return self.run_plot_work(d.background, true, work);
            }
            if d.to_file {
                // Tested clipped export (opens a save dialog).
                return Task::done(Message::PlotWindowExport);
            }
            let tmp = crate::io::print_to_printer::temp_pdf_path("print");
            let render_options = Self::pdf_plot_options(&d, wgroup_splits);
            let opts = self.plot_print_options(&d, wgroup_splits);
            let work = move || {
                crate::io::pdf_export::export_pdf(
                &w_wires, &w_hatches, &w_wipeouts, sw, sh, wox, woy, wrotation, wscale, wclip, &tmp,
                plot_style.as_ref(),
                render_options,
            ).and_then(|_| crate::io::print_to_printer::print_existing_pdf(&tmp, &opts))
                    .map(|printer| format!("Sent to printer: {printer}"))
                    .map_err(|e| format!("Print failed: {e}"))
            };
            return self.run_plot_work(true, false, work);
        }

        let (wires, hatches, wipeouts, group_splits, page_w, page_h, ox, oy, rotation, scale, clip) =
            self.layout_plot_params();

        if preview {
            let tmp = crate::io::print_to_printer::temp_pdf_path("preview");
            let render_options = Self::pdf_plot_options(&d, group_splits);
            let work = move || {
                crate::io::pdf_export::export_pdf(
                &wires,
                &hatches,
                &wipeouts,
                page_w,
                page_h,
                ox,
                oy,
                rotation,
                scale,
                clip,
                &tmp,
                plot_style.as_ref(),
                render_options,
            ).and_then(|_| crate::io::print_to_printer::open_in_viewer(&tmp))
                    .map(|_| "Opened plot preview.".to_string()).map_err(|e| format!("Preview failed: {e}"))
            };
            return self.run_plot_work(d.background, true, work);
        }

        if d.to_file {
            // Reuse the tested PDF export flow (it opens a save dialog).
            return Task::done(Message::PlotExport);
        }

        let opts = self.plot_print_options(&d, group_splits);
        self.command_line.push_info(crate::t!("Sending to system printer…").as_ref());
        let work = move || {
            iced::futures::executor::block_on(
                    crate::io::print_to_printer::print_wires_with(
                    wires, hatches, wipeouts, page_w, page_h, ox, oy, rotation, scale, clip,
                    plot_style, opts,
                ))
                .map(|printer| format!("Sent to printer: {printer}"))
                .map_err(|error| format!("Print failed: {error}"))
        };
        self.run_plot_work(true, false, work)
    }

    /// Build a [`PrintOptions`](crate::io::print_to_printer::PrintOptions) from
    /// the dialog state.
    fn plot_print_options(
        &self,
        d: &crate::ui::window::plot::PlotDialogState,
        group_splits: crate::io::pdf_export::PlotGroupSplits,
    ) -> crate::io::print_to_printer::PrintOptions {
        crate::io::print_to_printer::PrintOptions {
            printer: d.printer.clone(),
            copies: d.copies.trim().parse::<u32>().unwrap_or(1).max(1),
            quality: Some(d.quality.clone()),
            render: Self::pdf_plot_options(d, group_splits),
        }
    }

    fn pdf_plot_options(
        d: &crate::ui::window::plot::PlotDialogState,
        group_splits: crate::io::pdf_export::PlotGroupSplits,
    ) -> crate::io::pdf_export::PdfPlotOptions {
        crate::io::pdf_export::PdfPlotOptions {
            object_lineweights: d.lineweights,
            scale_lineweights: d.scale_lw && !d.fit_to_paper,
            transparency: d.transparency,
            stamp: d.stamp,
            merge_lines: d.merge_lines,
            group_splits,
        }
    }

    fn dialog_plot_style(
        &self,
        d: &crate::ui::window::plot::PlotDialogState,
    ) -> Option<crate::io::plot_style::PlotStyleTable> {
        if d.style_name.is_empty() || d.style_missing || !d.apply_plot_styles {
            return None;
        }
        self.active_plot_style
            .as_ref()
            .filter(|table| table.name.eq_ignore_ascii_case(&d.style_name))
            .cloned()
    }

    fn window_plot_job(&self) -> Option<ClippedPlotParams> {
        self.area_plot_job(self.plot_window?)
    }

    fn display_plot_job(&self) -> Option<ClippedPlotParams> {
        self.area_plot_job(self.display_plot_window()?)
    }

    fn limits_plot_job(&self) -> Option<ClippedPlotParams> {
        let (min, max) = self.tabs[self.active_tab]
            .scene
            .current_drawing_limits()?;
        self.area_plot_job((min.x, min.y, max.x, max.y))
    }

    fn named_view_plot_job(&self, name: &str) -> Option<ClippedPlotParams> {
        let view = self.tabs[self.active_tab]
            .scene
            .document
            .views
            .iter()
            .find(|view| {
                view.name.eq_ignore_ascii_case(name)
                    && view.paper_space == (self.tabs[self.active_tab].scene.current_layout != "Model")
            })?;
        let half_w = view.width.abs() * 0.5;
        let half_h = view.height.abs() * 0.5;
        (half_w > 1e-9 && half_h > 1e-9).then_some(())?;
        self.area_plot_job((
            view.center.x - half_w,
            view.center.y - half_h,
            view.center.x + half_w,
            view.center.y + half_h,
        ))
    }

    fn extents_plot_job(&self) -> Option<ClippedPlotParams> {
        let scene = &self.tabs[self.active_tab].scene;
        if scene.current_layout == "Model" {
            let (min, max) = scene.model_space_extents()?;
            return self.area_plot_job((
                min.x as f64,
                min.y as f64,
                max.x as f64,
                max.y as f64,
            ));
        }
        let (wires, hatches, wipeouts, _) =
            plot_scene_content(
                scene,
                self.plot_dialog.paperspace_last,
                plot_render_mode_override(&self.plot_dialog),
            );
        let extents = plot_content_extents(
            &wires
                .iter()
                .filter(|wire| wire.name != "paper_printable_area")
                .cloned()
                .collect::<Vec<_>>(),
            &hatches,
            &wipeouts,
        )?;
        self.area_plot_job(extents)
    }

    /// Current visible rectangle in the active space. The result is deliberately
    /// not clamped to the paper sheet: Display and Window may include the grey
    /// canvas outside the sheet, matching Model-space plotting.
    fn display_plot_window(&self) -> Option<(f64, f64, f64, f64)> {
        let scene = &self.tabs[self.active_tab].scene;
        let (canvas_w, canvas_h) = scene.selection.borrow().vp_size;
        let viewport = if scene.current_layout == "Model" {
            scene.active_model_tile_bounds(canvas_w, canvas_h)
        } else {
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: canvas_w,
                height: canvas_h,
            }
        };
        if viewport.width < 1.0 || viewport.height < 1.0 {
            return None;
        }
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: viewport.width,
            height: viewport.height,
        };
        let camera = scene.camera.borrow();
        let mut x0 = f64::INFINITY;
        let mut y0 = f64::INFINITY;
        let mut x1 = f64::NEG_INFINITY;
        let mut y1 = f64::NEG_INFINITY;
        for point in [
            iced::Point::new(0.0, 0.0),
            iced::Point::new(bounds.width, 0.0),
            iced::Point::new(bounds.width, bounds.height),
            iced::Point::new(0.0, bounds.height),
        ] {
            let world = camera.unproject_on_plane(
                point,
                bounds,
                glam::Vec3::Z,
                glam::DVec3::ZERO,
            );
            x0 = x0.min(world.x);
            y0 = y0.min(world.y);
            x1 = x1.max(world.x);
            y1 = y1.max(world.y);
        }
        (x0.is_finite()
            && y0.is_finite()
            && x1.is_finite()
            && y1.is_finite()
            && x1 - x0 > 1e-6
            && y1 - y0 > 1e-6)
            .then_some((x0, y0, x1, y1))
    }

    /// Render a selected rectangle through one shared Model/Paper path. The
    /// window may lie partly or wholly outside a paper sheet.
    fn area_plot_job(&self, window: (f64, f64, f64, f64)) -> Option<ClippedPlotParams> {
        use crate::io::paper_sizes::{window_to_sheet, PlotScale};
        let i = self.active_tab;
        let (x0, y0, x1, y1) = window;
        if (x1 - x0) < 1e-6 || (y1 - y0) < 1e-6 {
            return None;
        }
        let (sheet_w, sheet_h) = plot_dialog_sheet_mm(&self.plot_dialog);
        let win_w = (x1 - x0).max(1e-9);
        let win_h = (y1 - y0).max(1e-9);
        let scale_sel = if self.plot_dialog.fit_to_paper {
            PlotScale::Fit
        } else {
            let factor = plot_dialog_scale_factor(&self.plot_dialog);
            if factor > 0.0 {
                let mm_per_unit = if self.tabs[i].scene.current_layout == "Model" {
                    1.0
                } else {
                    1.0 / self.tabs[i].scene.paper_space_unit_factor().max(1e-9)
                };
                PlotScale::Ratio(factor * mm_per_unit)
            } else {
                PlotScale::Fit
            }
        };
        let (scale, centered_x, centered_y) =
            window_to_sheet((win_w, win_h), (sheet_w, sheet_h), scale_sel);
        let target_x = if self.plot_dialog.center {
            centered_x
        } else {
            self.plot_dialog.offset_x.parse::<f64>().unwrap_or(0.0)
        };
        let target_y = if self.plot_dialog.center {
            centered_y
        } else {
            self.plot_dialog.offset_y.parse::<f64>().unwrap_or(0.0)
        };
        let scene = &self.tabs[i].scene;
        let (wx0, wy0, wx1, wy1) = (x0 as f32, y0 as f32, x1 as f32, y1 as f32);
        let (all_wires, hatches, wipeouts, mut group_splits) =
            plot_scene_content(
                scene,
                self.plot_dialog.paperspace_last,
                plot_render_mode_override(&self.plot_dialog),
            );
        let first_wire_count = all_wires[..group_splits.wires.min(all_wires.len())]
            .iter()
            .filter(|w| w.name != "paper_printable_area")
            .filter(|w| w.aabb[0] <= wx1 && w.aabb[2] >= wx0 && w.aabb[1] <= wy1 && w.aabb[3] >= wy0)
            .count();
        let wires: Vec<_> = all_wires
            .iter()
            .filter(|w| w.name != "paper_printable_area")
            .filter(|w| w.aabb[0] <= wx1 && w.aabb[2] >= wx0 && w.aabb[1] <= wy1 && w.aabb[3] >= wy0)
            .cloned()
            .collect();
        group_splits.wires = first_wire_count;
        let offset_x = (target_x / scale) - x0;
        let offset_y = (target_y / scale) - y0;
        let clip = Some((
            (target_x / scale) as f32,
            (target_y / scale) as f32,
            win_w as f32,
            win_h as f32,
        ));
        let rotation = if self.plot_dialog.upside_down { 180 } else { 0 };
        let (page_w, page_h) = match rotation {
            90 | 270 => (sheet_h, sheet_w),
            _ => (sheet_w, sheet_h),
        };
        Some((
            wires,
            hatches,
            wipeouts,
            group_splits,
            page_w,
            page_h,
            offset_x,
            offset_y,
            rotation,
            scale as f32,
            clip,
        ))
    }

    pub(super) fn on_plot_style_panel_apply(&mut self) -> Task<Message> {
                let aci = self.plotstyle_panel_aci as usize;
                if let Some(table) = self.active_plot_style.as_mut() {
                    if let Some(entry) = table.aci_entries.get_mut(aci) {
                        // Parse color.
                        let color_str = self.ps_color_buf.trim();
                        if color_str.is_empty() {
                            entry.color = None;
                        } else if color_str.starts_with('#') && color_str.len() == 7 {
                            let r = u8::from_str_radix(&color_str[1..3], 16).unwrap_or(0);
                            let g = u8::from_str_radix(&color_str[3..5], 16).unwrap_or(0);
                            let b = u8::from_str_radix(&color_str[5..7], 16).unwrap_or(0);
                            entry.color = Some([r, g, b]);
                        }
                        if let Ok(lw) = self.ps_lineweight_buf.trim().parse::<u8>() {
                            entry.lineweight = lw;
                        }
                        if let Ok(sc) = self.ps_screening_buf.trim().parse::<u8>() {
                            entry.screening = sc.min(100);
                        }

                    }
                } else {
                    // No table loaded: create an identity table and apply.
                    let mut table = crate::io::plot_style::PlotStyleTable::identity("Custom.ctb");
                    if let Some(entry) = table.aci_entries.get_mut(aci) {
                        let color_str = self.ps_color_buf.trim();
                        if color_str.starts_with('#') && color_str.len() == 7 {
                            let r = u8::from_str_radix(&color_str[1..3], 16).unwrap_or(0);
                            let g = u8::from_str_radix(&color_str[3..5], 16).unwrap_or(0);
                            let b = u8::from_str_radix(&color_str[5..7], 16).unwrap_or(0);
                            entry.color = Some([r, g, b]);
                        }
                        if let Ok(lw) = self.ps_lineweight_buf.trim().parse::<u8>() {
                            entry.lineweight = lw;
                        }
                        if let Ok(sc) = self.ps_screening_buf.trim().parse::<u8>() {
                            entry.screening = sc.min(100);
                        }
                    }
                    self.active_plot_style = Some(table);
                    self.command_line
                        .push_output(crate::tf!("Created new CTB table, ACI {aci} updated.").as_ref());
                }
                Task::none()
    }

    pub(super) fn on_plot_style_panel_save(&mut self) -> Task<Message> {
                if self.active_plot_style.is_none() {
                    self.command_line
                        .push_error(crate::t!("No plot style table loaded. Load or create one first.").as_ref());
                    return Task::none();
                }
                let default_name = self
                    .active_plot_style
                    .as_ref()
                    .map(|t| t.name.clone())
                    .unwrap_or("export.ctb".into());
                Task::perform(
                    async move {
                        let dialog = crate::sys::file_dialog()
                            .set_title("Save Plot Style Table")
                            .set_file_name(&default_name)
                            .add_filter("Plot Style Files", &["ctb", "CTB"])
                            .add_filter("All Files", &["*"]);
                        #[cfg(not(target_arch = "wasm32"))]
                        let dialog = match crate::io::plot_style::ensure_plot_styles_dir() {
                            Ok(dir) => dialog.set_directory(dir),
                            Err(_) => dialog,
                        };
                        dialog.save_file().await
                            .map(|h| crate::sys::handle_path(&h))
                    },
                    Message::PlotStylePanelSavePath,
                )
    }
}
