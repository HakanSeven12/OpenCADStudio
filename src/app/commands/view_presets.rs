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
        let visual_styles = crate::io::visual_styles::offered(document);
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
                paper_space: view.paper_space,
            })
            .collect();
        let scene = &self.tabs[i].scene;
        let sheet_viewports = (scene.current_layout != "Model" && scene.active_viewport.is_none()).then(|| {
            scene
                .layout_content_viewports()
                .iter()
                .filter_map(|handle| match document.get_entity(*handle) {
                    Some(codec::EntityType::Viewport(value)) => Some((
                        handle.value(),
                        [
                            value.center.x - value.width / 2.0,
                            value.center.y - value.height / 2.0,
                            value.center.x + value.width / 2.0,
                            value.center.y + value.height / 2.0,
                        ],
                    )),
                    _ => None,
                })
                .collect()
        });
        ViewNames {
            views,
            visual_styles: visual_styles.into_iter().map(|(name, _)| name).collect(),
            live_sections: live_sections.into_iter().map(|(name, _)| name).collect(),
            sheet_viewports,
            // PICKBOX pixels in sheet units, at the sheet camera's zoom.
            sheet_pick_tolerance: {
                let height = scene.camera.borrow().ortho_size() as f64 * 2.0;
                let (_, pixels) = scene.selection.borrow().view.vp_size;
                f64::from(self.pick_box) * height / f64::from(pixels).max(1.0)
            },
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
    pub(crate) fn run_dash_view(&mut self, rest: &str, i: usize) -> Task<Message> {
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
                        let removed = self.tabs[i].scene.document.views.remove(name);
                        if let Some(view) = removed {
                            crate::scene::erase_background(
                                &mut self.tabs[i].scene.document,
                                view.background_handle,
                            );
                        }
                        self.tabs[i].scene.document.delete_layer_state(&snapshot_name(name));
                    }
                    self.tabs[i].dirty = true;
                }
            }
            "RESTOREVP" => {
                let (name, handle) = arg.split_once('\u{1f}').unwrap_or((arg, ""));
                let Some(view) = self.tabs[i].scene.document.views.get(name).cloned() else {
                    return Task::none();
                };
                let Some(handle) = u64::from_str_radix(handle, 16).ok().map(Handle::new) else {
                    return Task::none();
                };
                // The reference enters the selected viewport and restores the
                // view in it.
                let task = self.update(Message::EnterViewport(handle));
                self.restore_view_in_active_viewport(i, &view);
                return task;
            }
            "RESTORE" => {
                let Some(view) = self.tabs[i].scene.document.views.get(arg).cloned() else {
                    return Task::none();
                };
                if self.tabs[i].scene.current_layout != "Model"
                    && self.tabs[i].scene.active_viewport.is_some()
                    && !view.paper_space
                {
                    self.restore_view_in_active_viewport(i, &view);
                    return Task::none();
                }
                let before = self.tabs[i].scene.active_gaze_dir();
                self.tabs[i].scene.restore_named_view(&view);
                self.apply_view_render_mode(i, &view);
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
            // Background type chosen: its dialog takes over until Apply or Cancel.
            "BACKGROUND" => {
                if let Some((view, kind)) = arg.split_once('\u{1f}') {
                    self.open_view_background(i, view, kind);
                    return self.finish_dispatch("-VIEW");
                }
                settings_again = true;
            }
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

    /// A model-space view restored in the entered paper-space viewport, with
    /// its background and visual style.
    fn restore_view_in_active_viewport(&mut self, i: usize, view: &codec::tables::View) {
        let Some(viewport) = self.tabs[i].scene.active_viewport else { return };
        self.tabs[i].scene.restore_named_view_in_viewport(view, viewport);
        if let Some(ObjectType::VisualStyle(style)) =
            self.tabs[i].scene.document.objects.get(&view.visual_style_handle)
        {
            if let Some((mode, _)) = crate::modules::view::visual_style::mode_for_style_name(&style.description) {
                self.tabs[i].scene.set_active_viewport_render_mode(mode);
                self.tabs[i].scene.bump_geometry_no_blocks();
            }
        }
        self.tabs[i].dirty = true;
    }

    /// Add or replace a named view, keeping the record (and so its settings)
    /// of a view saved again under the same name.
    fn save_named_view(&mut self, i: usize, mut view: codec::tables::View) {
        self.push_undo_snapshot(i, "-VIEW");
        let background = self.tabs[i].scene.active_background();
        let document = &mut self.tabs[i].scene.document;
        view.background_handle = crate::scene::copy_background(document, background);
        let mut replaced = Handle::NULL;
        match document.views.get(&view.name).cloned() {
            Some(old) => {
                view.name = old.name;
                view.handle = old.handle;
                view.visual_style_handle = old.visual_style_handle;
                view.live_section_handle = old.live_section_handle;
                // The view keeps its background unless the viewport has
                // one, which the view then takes a copy of.
                if view.background_handle.is_null() {
                    view.background_handle = old.background_handle;
                }
                replaced = old.background_handle;
            }
            None => {
                view.handle = document.allocate_handle();
                if view.visual_style_handle.is_null() {
                    view.visual_style_handle = crate::io::visual_styles::offered(document)
                        .into_iter()
                        .find(|(name, _)| name == "2D Wireframe")
                        .map_or(Handle::NULL, |(_, handle)| handle);
                }
            }
        }
        document.views.add_or_replace(view);
        crate::scene::erase_background(document, replaced);
        self.tabs[i].dirty = true;
    }

    /// One setting of -VIEW's sEttings option on the named view.
    fn apply_view_setting(&mut self, i: usize, setting: &str, name: &str, value: &str) {
        let Some(mut view) = self.tabs[i].scene.document.views.get(name).cloned() else {
            return;
        };
        self.push_undo_snapshot(i, "-VIEW");
        let document = &mut self.tabs[i].scene.document;
        let mut replaced = Handle::NULL;
        if view.handle.is_null() {
            view.handle = document.allocate_handle();
        }
        let find = |list: Vec<(String, Handle)>| {
            list.into_iter().find(|(entry, _)| entry.eq_ignore_ascii_case(value)).map_or(Handle::NULL, |(_, h)| h)
        };
        match setting {
            "BACKGROUND" => replaced = std::mem::replace(&mut view.background_handle, Handle::NULL),
            "VISUALSTYLE" => view.visual_style_handle = find(crate::io::visual_styles::offered(document)),
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
        crate::scene::erase_background(document, replaced);
        self.tabs[i].dirty = true;
    }
}

impl OpenCADStudio {
    /// The Background dialog for a named view, on the type typed at the
    /// prompt and filled from the view's background where it has one.
    fn open_view_background(&mut self, i: usize, view: &str, kind: &str) {
        use crate::ui::window::pdf_dialogs::{BgKind, BgPosition, ViewBackgroundState};
        use codec::objects::ClassObjectData as D;
        let unpack = |c: u32| [(c >> 16) as u8, (c >> 8) as u8, c as u8];
        let canvas = self.tabs[i].scene.bg_color;
        let mut state = ViewBackgroundState {
            view: view.to_string(),
            kind: match kind {
                "SOLID" => BgKind::Solid,
                "GRADIENT" => BgKind::Gradient,
                "IMAGE" => BgKind::Image,
                _ => BgKind::None,
            },
            // The reference's defaults.
            solid: [128, 128, 255],
            top: [204, 204, 204],
            middle: [128, 128, 128],
            bottom: [51, 51, 51],
            three: false,
            rotation: "0".into(),
            image: String::new(),
            position: BgPosition::Center,
            aspect: false,
            offset: ["0".into(), "0".into()],
            scale: ["1".into(), "1".into()],
            scale_base: [1.0, 1.0],
            preview: None,
            viewport: {
                let (width, height) = self.tabs[i].scene.selection.borrow().view.vp_size;
                [width as f32, height as f32]
            },
            canvas: [0, 1, 2].map(|k| (canvas[k].clamp(0.0, 1.0) * 255.0).round() as u8),
        };
        let document = &self.tabs[i].scene.document;
        let current = document.views.get(view).map_or(Handle::NULL, |v| v.background_handle);
        if let Some(ObjectType::ClassObject(object)) = document.objects.get(&current) {
            match &object.data {
                D::SolidBackground(value) => state.solid = unpack(value.color),
                D::GradientBackground(value) => {
                    (state.top, state.middle, state.bottom) = (
                        unpack(value.color_top),
                        unpack(value.color_middle),
                        unpack(value.color_bottom),
                    );
                    state.three = value.height > 0.0;
                    state.rotation = crate::ui::window::pdf_dialogs::short(value.rotation);
                }
                D::ImageBackground(value) => {
                    state.image = value.filename.clone();
                    state.position = if value.use_tiling {
                        BgPosition::Tile
                    } else if value.fit_to_screen {
                        BgPosition::Stretch
                    } else {
                        BgPosition::Center
                    };
                    state.aspect = value.maintain_aspect_ratio;
                    state.offset = [value.offset.x, value.offset.y].map(crate::ui::window::pdf_dialogs::short);
                    state.scale = [value.scale.x, value.scale.y].map(crate::ui::window::pdf_dialogs::short);
                    state.scale_base = [value.scale.x, value.scale.y];
                    state.preview = crate::ui::window::pdf_dialogs::bg_preview(&value.filename);
                }
                _ => {}
            }
        }
        self.view_background = Some(state);
        self.active_modal = Some(crate::app::ModalKind::ViewBackground);
    }

    pub(in crate::app) fn update_view_background(
        &mut self,
        message: crate::ui::window::pdf_dialogs::PdfDialogMsg,
    ) -> Task<Message> {
        use crate::ui::window::pdf_dialogs::{BgKind, BgPosition, PdfDialogMsg as M};
        use codec::objects::{
            ClassObjectData as D, GradientBackground, ImageBackground, SolidBackground,
        };
        let i = self.active_tab;
        let Some(state) = self.view_background.as_mut() else {
            return Task::none();
        };
        match message {
            M::BgKind(kind) => state.kind = kind,
            M::BgThree(on) => state.three = on,
            M::BgRotation(value) => state.rotation = value,
            M::BgImage(value) => {
                state.preview = crate::ui::window::pdf_dialogs::bg_preview(&value);
                state.image = value;
            }
            M::BgPosition(value) => state.position = value,
            M::BgAspect(on) => state.aspect = on,
            M::BgOffset(value) => state.offset = value,
            M::BgScale(value) => {
                // Keeping the aspect ratio, both scales follow the one changed.
                state.scale = if state.aspect {
                    let changed = if value[0] != state.scale[0] { 0 } else { 1 };
                    [value[changed].clone(), value[changed].clone()]
                } else {
                    value
                };
            }
            M::BgStep(field, dir) => {
                use crate::ui::window::pdf_dialogs::{bg_step, short};
                let current = |text: &String, default: f64| text.trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(default);
                if field < 2 {
                    let k = field as usize;
                    state.offset[k] = short(bg_step(field, current(&state.offset[k], 0.0), dir));
                } else {
                    let k = field as usize - 2;
                    let next = short(bg_step(field, current(&state.scale[k], state.scale_base[k]), dir));
                    if state.aspect {
                        state.scale = [next.clone(), next];
                    } else {
                        state.scale[k] = next;
                    }
                }
            }
            M::BgReset => {
                state.offset = ["0".into(), "0".into()];
                state.scale = ["1".into(), "1".into()];
            }
            M::BgColor(slot, rgb) => match slot {
                0 => state.solid = rgb,
                1 => state.top = rgb,
                2 => state.middle = rgb,
                _ => state.bottom = rgb,
            },
            M::BgPickColor(slot) => {
                let [r, g, b] = match slot {
                    0 => state.solid,
                    1 => state.top,
                    2 => state.middle,
                    _ => state.bottom,
                };
                return Task::done(Message::OpenColorWindow(
                    crate::app::ColorPickTarget::ViewBackground(slot),
                    codec::types::Color::Rgb { r, g, b },
                ));
            }
            M::BgBrowse => {
                return Task::perform(
                    async {
                        crate::sys::file_dialog()
                            .set_title(crate::tr!("view-background", "select-image"))
                            .add_filter(
                                crate::tr!("view-background", "image-files"),
                                &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "gif", "tga"],
                            )
                            .pick_file()
                            .await
                            .map(|handle| crate::sys::handle_path(&handle))
                    },
                    |path| match path {
                        Some(path) => Message::PdfDialog(M::BgImage(path.display().to_string())),
                        None => Message::Noop,
                    },
                );
            }
            M::BgApply => {
                // Colours are stored as true colours (flag byte 0xC2), as the
                // reference stores them.
                let pack = |c: [u8; 3]| {
                    0xC200_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32
                };
                let data = match state.kind {
                    BgKind::None => None,
                    BgKind::Solid => Some(D::SolidBackground(SolidBackground {
                        class_version: 1,
                        color: pack(state.solid),
                    })),
                    BgKind::Gradient => {
                        let Some(rotation) = state
                            .rotation
                            .trim()
                            .parse::<f64>()
                            .ok()
                            .filter(|v| v.is_finite())
                        else {
                            self.command_line
                                .push_error(crate::t!("Requires a number.").as_ref());
                            return Task::none();
                        };
                        Some(D::GradientBackground(GradientBackground {
                            class_version: 1,
                            color_top: pack(state.top),
                            color_middle: pack(state.middle),
                            color_bottom: pack(state.bottom),
                            horizon: 0.5,
                            // The reference marks a three-colour gradient by its
                            // middle band's height.
                            height: if state.three { 0.33 } else { 0.0 },
                            rotation,
                        }))
                    }
                    BgKind::Image => {
                        if !std::path::Path::new(state.image.trim()).is_file() {
                            self.command_line.push_error("Invalid Background Image File!");
                            return Task::none();
                        }
                        use crate::ui::window::pdf_dialogs::{BG_OFFSET_LIMIT, BG_SCALE_LIMIT};
                        let number = |text: &String| text.trim().parse::<f64>().ok().filter(|v| v.is_finite());
                        let (Some(ox), Some(oy), Some(sx), Some(sy)) = (
                            number(&state.offset[0]),
                            number(&state.offset[1]),
                            number(&state.scale[0]),
                            number(&state.scale[1]),
                        ) else {
                            self.command_line.push_error(crate::t!("Requires a number.").as_ref());
                            return Task::none();
                        };
                        // Within the reference's slider ranges; a zero scale
                        // is ignored there, so the image keeps its scale.
                        let scale = |value: f64, base: f64| {
                            if value.abs() < 1e-9 { base } else { value.clamp(-BG_SCALE_LIMIT, BG_SCALE_LIMIT) }
                        };
                        Some(D::ImageBackground(ImageBackground {
                            class_version: 1,
                            filename: state.image.trim().to_string(),
                            fit_to_screen: state.position == BgPosition::Stretch,
                            maintain_aspect_ratio: state.aspect,
                            use_tiling: state.position == BgPosition::Tile,
                            offset: codec::types::Vector2::new(
                                ox.clamp(-BG_OFFSET_LIMIT, BG_OFFSET_LIMIT),
                                oy.clamp(-BG_OFFSET_LIMIT, BG_OFFSET_LIMIT),
                            ),
                            scale: codec::types::Vector2::new(
                                scale(sx, state.scale_base[0]),
                                scale(sy, state.scale_base[1]),
                            ),
                        }))
                    }
                };
                let name = state.view.clone();
                self.view_background = None;
                self.close_active_modal();
                if self.tabs[i].scene.document.views.get(&name).is_some() {
                    self.push_undo_snapshot(i, "-VIEW");
                    let document = &mut self.tabs[i].scene.document;
                    let handle = data.map_or(Handle::NULL, |data| {
                        crate::scene::add_background(document, data)
                    });
                    if let Some(view) = document.views.get_mut(&name) {
                        let old = std::mem::replace(&mut view.background_handle, handle);
                        crate::scene::erase_background(document, old);
                    }
                    self.tabs[i].dirty = true;
                }
                return self.run_dash_view("SETTINGS", i);
            }
            _ => {}
        }
        Task::none()
    }
}

impl OpenCADStudio {
    /// A restored view's visual style becomes the model tab's render mode
    /// (the closest of OCS's modes); a view without one leaves it.
    pub(crate) fn apply_view_render_mode(&mut self, i: usize, view: &codec::tables::View) {
        if self.tabs[i].scene.current_layout != "Model" {
            return;
        }
        let Some(ObjectType::VisualStyle(style)) =
            self.tabs[i].scene.document.objects.get(&view.visual_style_handle)
        else {
            return;
        };
        use crate::modules::view::visual_style;
        let Some((mode, named)) = visual_style::mode_for_style_name(&style.description) else {
            return;
        };
        // A style of its own keeps its name; the render modes keep theirs.
        let label = if named { style.description.clone() } else { visual_style::label_for(mode).to_string() };
        self.apply_render_mode(i, mode, &label);
    }
}
