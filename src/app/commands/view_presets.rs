//! What -VIEW carries out: the view presets (with the orthographic UCS of
//! UCSORTHO), named views saved with the UCS of UCSVIEW, and the per-view
//! settings of its sEttings option.

use super::super::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::view::view_cmd::{self, DashViewCommand, ViewInfo, ViewNames};
use codec::objects::ObjectType;
use codec::types::{Handle, Vector3};
use iced::Task;

/// The xrecord in a view's extension dictionary holding its category and
/// layer snapshot, as the reference keeps them.
const VIEW_INFO: &str = "ADSK_XREC_VTRVIEWINFO";

/// The layer state a view's layer snapshot is stored under.
fn snapshot_name(view: &str) -> String {
    format!("ACAD_VIEWS_{view}")
}

impl OpenCADStudio {
    /// Names -VIEW checks its input against, taken from the drawing.
    pub(super) fn dash_view_names(&self, i: usize) -> ViewNames {
        let document = &self.tabs[i].scene.document;
        let visual_styles: Vec<(String, Handle)> = match document
            .objects
            .get(&document.header.acad_visualstyle_dict_handle)
        {
            Some(ObjectType::Dictionary(dictionary)) => dictionary.entries.clone(),
            _ => Vec::new(),
        };
        let live_sections: Vec<(String, Handle)> = document
            .entities()
            .filter_map(|entity| match entity {
                codec::EntityType::Extended(extended) => match &extended.data {
                    codec::entities::ExtendedEntityData::SectionObject(section) => {
                        Some((section.name.clone(), extended.common.handle))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let name_of = |list: &[(String, Handle)], handle: Handle| {
            (!handle.is_null())
                .then(|| list.iter().find(|(_, h)| *h == handle).map(|(name, _)| name.clone()))
                .flatten()
        };
        let views = document
            .views
            .iter()
            .map(|view| ViewInfo {
                name: view.name.clone(),
                category: (!view.handle.is_null())
                    .then(|| document.xrecord(view.handle, VIEW_INFO))
                    .flatten()
                    .and_then(|record| record.get_string(300))
                    .unwrap_or_default()
                    .to_string(),
                visual_style: name_of(&visual_styles, view.visual_style_handle),
                live_section: name_of(&live_sections, view.live_section_handle),
            })
            .collect();
        ViewNames {
            views,
            visual_styles: visual_styles.into_iter().map(|(name, _)| name).collect(),
            live_sections: live_sections.into_iter().map(|(name, _)| name).collect(),
        }
    }

    /// Set a standard view: the world orthographic or isometric view of
    /// `preset`. An orthographic one also sets its orthographic UCS while
    /// UCSORTHO is on; asking for the view shown already keeps it.
    pub(in crate::app) fn apply_view_preset(&mut self, preset: &str) -> Task<Message> {
        use crate::scene::pipeline::viewcube::{
            FACE_BACK, FACE_BOTTOM, FACE_FRONT, FACE_LEFT, FACE_RIGHT, FACE_TOP,
        };
        use crate::scene::CubeRegion;
        let i = self.active_tab;
        let region = match preset {
            "TOP" => CubeRegion::Face(FACE_TOP),
            "BOTTOM" => CubeRegion::Face(FACE_BOTTOM),
            "FRONT" => CubeRegion::Face(FACE_FRONT),
            "BACK" => CubeRegion::Face(FACE_BACK),
            "LEFT" => CubeRegion::Face(FACE_LEFT),
            "RIGHT" => CubeRegion::Face(FACE_RIGHT),
            _ => {
                let (x, y) = match preset {
                    "SEISO" => (1.0, -1.0),
                    "NEISO" => (1.0, 1.0),
                    "NWISO" => (-1.0, 1.0),
                    _ => (-1.0, -1.0),
                };
                let want = glam::Vec3::new(x, y, 1.0).normalize();
                (18..26)
                    .map(CubeRegion::Corner)
                    .max_by(|a, b| a.snap_direction().dot(want).total_cmp(&b.snap_direction().dot(want)))
                    .expect("the cube has corners")
            }
        };
        if view_cmd::ucs_ortho() && self.tabs[i].active_block_edit.is_none() {
            if let Some((name, x, y, ortho)) = view_cmd::orthographic_ucs(preset) {
                // The top view's UCS is the world one.
                self.tabs[i].active_ucs = (ortho != 1).then(|| {
                    let mut ucs = codec::tables::Ucs::new(name);
                    ucs.origin = Vector3::ZERO;
                    ucs.x_axis = Vector3::new(x[0], x[1], x[2]);
                    ucs.y_axis = Vector3::new(y[0], y[1], y[2]);
                    ucs.ortho_type = ortho;
                    ucs
                });
                self.commit_active_ucs_change(i, "-VIEW");
            }
        }
        self.command_line.push_output(crate::t!("Regenerating model.").as_ref());
        self.snap_view_preset(region)
    }

    /// Carry out what -VIEW gathered (`-VIEW >VERB args`).
    pub(super) fn run_dash_view(&mut self, rest: &str, i: usize) -> Task<Message> {
        let (verb, arg) = rest.split_once(' ').unwrap_or((rest, ""));
        let mut settings_again = false;
        match verb {
            "END" => {
                if !arg.is_empty() {
                    self.command_line.push_output(arg);
                }
            }
            "PRESET" => return self.apply_view_preset(arg),
            "DELETE" => {
                let doomed: Vec<String> = view_cmd::matching(
                    self.tabs[i].scene.document.views.iter().map(|view| view.name.as_str()),
                    arg,
                )
                .into_iter()
                .map(str::to_string)
                .collect();
                if !doomed.is_empty() {
                    self.push_undo_snapshot(i, "-VIEW");
                    for name in &doomed {
                        self.tabs[i].scene.document.views.remove(name);
                        self.tabs[i].scene.document.delete_layer_state(&snapshot_name(name));
                    }
                    self.tabs[i].dirty = true;
                }
            }
            "RESTORE" => {
                let Some(view) = self.tabs[i].scene.document.views.get(arg).cloned() else {
                    return Task::none();
                };
                let before = self.tabs[i].scene.active_gaze_dir();
                self.tabs[i].scene.restore_named_view(&view);
                if view.ucs_associated && self.tabs[i].active_block_edit.is_none() {
                    let world = view.ucs_x_axis == Vector3::new(1.0, 0.0, 0.0)
                        && view.ucs_y_axis == Vector3::new(0.0, 1.0, 0.0)
                        && view.ucs_origin == Vector3::ZERO;
                    self.tabs[i].active_ucs = (!world).then(|| {
                        let name = view_cmd::PRESETS
                            .iter()
                            .filter_map(|preset| view_cmd::orthographic_ucs(preset))
                            .find(|(_, _, _, ortho)| *ortho == view.ucs_ortho_type)
                            .map_or("*ACTIVE*", |(name, ..)| name);
                        let mut ucs = codec::tables::Ucs::new(name);
                        ucs.origin = view.ucs_origin;
                        ucs.x_axis = view.ucs_x_axis;
                        ucs.y_axis = view.ucs_y_axis;
                        ucs.elevation = view.ucs_elevation;
                        ucs.ortho_type = view.ucs_ortho_type;
                        ucs
                    });
                    self.commit_active_ucs_change(i, "-VIEW");
                }
                if self.tabs[i].scene.active_gaze_dir().dot(before) < 0.9999 {
                    self.command_line.push_output(crate::t!("Regenerating model.").as_ref());
                }
            }
            "SAVE" => {
                let mut view = self.tabs[i].scene.current_as_named_view(arg);
                if view_cmd::ucs_view() {
                    self.command_line.push_output("UCSVIEW = 1  UCS will be saved with view");
                    self.store_ucs_in_view(i, &mut view);
                }
                self.save_named_view(i, view);
            }
            "WINDOW" => {
                let parts: Vec<&str> = arg.split('\u{1f}').collect();
                let corner = |s: &str| {
                    let (x, y) = s.split_once(',')?;
                    Some((x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?))
                };
                match (parts.first(), parts.get(1).and_then(|s| corner(s)), parts.get(2).and_then(|s| corner(s))) {
                    (Some(name), Some(a), Some(b)) if a.0 != b.0 && a.1 != b.1 => {
                        // A plan view of the window between the corners; the
                        // reference keeps no UCS with a window view.
                        let mut view = self.tabs[i].scene.current_as_named_view(name);
                        view.target = Vector3 { x: (a.0 + b.0) / 2.0, y: (a.1 + b.1) / 2.0, z: 0.0 };
                        view.direction = Vector3 { x: 0.0, y: 0.0, z: 1.0 };
                        view.width = (b.0 - a.0).abs();
                        view.height = (b.1 - a.1).abs();
                        self.save_named_view(i, view);
                    }
                    _ => self.command_line.push_output("Invalid window specification."),
                }
            }
            "SETTINGS" => settings_again = true,
            "UCSVIEW" => {
                view_cmd::set_ucs_view(arg == "1");
                self.save_config();
                settings_again = true;
            }
            "SET" => {
                let parts: Vec<&str> = arg.split('\u{1f}').collect();
                if let [setting, view, value] = parts[..] {
                    self.apply_view_setting(i, setting, view, value);
                }
                settings_again = true;
            }
            _ => {}
        }
        if settings_again {
            let command = DashViewCommand::settings(self.dash_view_names(i));
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
        }
        self.finish_dispatch("-VIEW")
    }

    /// Keep the current UCS with a view being saved (UCSVIEW).
    fn store_ucs_in_view(&self, i: usize, view: &mut codec::tables::View) {
        view.ucs_associated = true;
        match &self.tabs[i].active_ucs {
            Some(ucs) => {
                view.ucs_origin = ucs.origin;
                view.ucs_x_axis = ucs.x_axis;
                view.ucs_y_axis = ucs.y_axis;
                view.ucs_elevation = ucs.elevation;
                view.ucs_ortho_type = ucs.ortho_type;
                view.named_ucs_handle = ucs.named_ucs_handle;
            }
            None => {
                view.ucs_origin = Vector3::ZERO;
                view.ucs_x_axis = Vector3::new(1.0, 0.0, 0.0);
                view.ucs_y_axis = Vector3::new(0.0, 1.0, 0.0);
                view.ucs_elevation = 0.0;
                view.ucs_ortho_type = 0;
            }
        }
    }

    /// Add or replace a named view, keeping the record (and so its settings)
    /// of a view saved again under the same name.
    fn save_named_view(&mut self, i: usize, mut view: codec::tables::View) {
        self.push_undo_snapshot(i, "-VIEW");
        let document = &mut self.tabs[i].scene.document;
        match document.views.get(&view.name).cloned() {
            Some(old) => {
                view.name = old.name;
                view.handle = old.handle;
                view.visual_style_handle = old.visual_style_handle;
                view.live_section_handle = old.live_section_handle;
                view.background_handle = old.background_handle;
            }
            None => view.handle = document.allocate_handle(),
        }
        document.views.add_or_replace(view);
        self.tabs[i].dirty = true;
    }

    /// One setting of -VIEW's sEttings option on the named view.
    fn apply_view_setting(&mut self, i: usize, setting: &str, name: &str, value: &str) {
        self.push_undo_snapshot(i, "-VIEW");
        let document = &mut self.tabs[i].scene.document;
        let Some(mut view) = document.views.get(name).cloned() else {
            return;
        };
        if view.handle.is_null() {
            view.handle = document.allocate_handle();
        }
        let find = |list: Vec<(String, Handle)>| {
            list.into_iter().find(|(entry, _)| entry.eq_ignore_ascii_case(value)).map_or(Handle::NULL, |(_, h)| h)
        };
        match setting {
            "BACKGROUND" => view.background_handle = Handle::NULL,
            "VISUALSTYLE" => {
                let styles = match document.objects.get(&document.header.acad_visualstyle_dict_handle) {
                    Some(ObjectType::Dictionary(dictionary)) => dictionary.entries.clone(),
                    _ => Vec::new(),
                };
                view.visual_style_handle = find(styles);
            }
            "SECTION" => {
                let sections: Vec<(String, Handle)> = document
                    .entities()
                    .filter_map(|entity| match entity {
                        codec::EntityType::Extended(extended) => match &extended.data {
                            codec::entities::ExtendedEntityData::SectionObject(section) => {
                                Some((section.name.clone(), extended.common.handle))
                            }
                            _ => None,
                        },
                        _ => None,
                    })
                    .collect();
                view.live_section_handle = find(sections);
            }
            "CATEGORIZE" | "LAYER" => {
                let snapshot = snapshot_name(&view.name);
                let layer_name = match (setting, value) {
                    ("LAYER", "SAVE") => {
                        document.capture_layer_state(snapshot.clone(), "");
                        Some(snapshot)
                    }
                    ("LAYER", _) => {
                        document.delete_layer_state(&snapshot);
                        Some(String::new())
                    }
                    _ => None,
                };
                let record = document.ensure_xrecord(view.handle, VIEW_INFO);
                if let Some(ObjectType::XRecord(record)) = document.objects.get_mut(&record) {
                    let text = |record: &codec::objects::XRecord, code: i32| {
                        record.get_string(code).unwrap_or_default().to_string()
                    };
                    let category = if setting == "CATEGORIZE" { value.to_string() } else { text(record, 300) };
                    let layers = layer_name.unwrap_or_else(|| text(record, 302));
                    let mut rest: Vec<_> =
                        record.entries.iter().filter(|entry| !matches!(entry.code, 300 | 302)).cloned().collect();
                    if rest.is_empty() {
                        rest = vec![
                            codec::objects::XRecordEntry::bool(293, false),
                            codec::objects::XRecordEntry::bool(294, false),
                        ];
                    }
                    record.entries.clear();
                    record.add_string(300, category);
                    record.add_string(302, layers);
                    record.entries.extend(rest);
                }
            }
            _ => {}
        }
        document.views.add_or_replace(view);
        self.tabs[i].dirty = true;
    }
}
