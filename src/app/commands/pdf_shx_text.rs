//! PDFSHXTEXT: the selection, the settings (dialog or command line) and the
//! conversion of the recognised lines to MTEXT as one undo step.

use super::*;
use crate::modules::insert::pdf_shx_text::{
    self as shx, ShxSettingsCommand, Stroke, APPLY, OPEN_SETTINGS,
};
use crate::ui::window::pdf_shx_dialog::{ShxDialogMsg, ShxSettingsState};

const INTRO: &str = "Select geometry to convert to text...";

impl OpenCADStudio {
    fn drawing_path(&self, i: usize) -> Option<String> {
        self.tabs[i]
            .current_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .or_else(|| self.tabs[i].scene.document.source_path.clone())
    }

    /// PDFSHXTEXT: a pre-selection converts at once; otherwise the objects are
    /// asked for. PDFSHXTEXTSETTINGS 1 shows the settings first (once).
    pub(in crate::app) fn start_pdf_shx_text(&mut self, i: usize) {
        let mut settings = shx::settings();
        self.command_line.push_info(INTRO);
        if settings.show_settings {
            settings.show_settings = false;
            shx::set_settings(settings);
            self.save_config();
            self.tabs[i].scene.deselect_all();
            self.open_pdf_shx_settings(i);
            return;
        }
        let preselected = self.tabs[i].scene.selected_handles_in_order();
        if !preselected.is_empty() {
            self.command_line
                .push_output(&format!("{} found", preselected.len()));
            self.tabs[i].scene.deselect_all();
            self.convert_pdf_shx_text(i, preselected);
            return;
        }
        self.start_pdf_shx_gather(i);
    }

    /// The selection prompt (also where the settings return to).
    pub(in crate::app) fn start_pdf_shx_gather(&mut self, i: usize) {
        use crate::command::CadCommand;
        use crate::modules::draw::select::SelectObjectsCommand;
        self.tabs[i].scene.deselect_all();
        let command = SelectObjectsCommand::with_settings(
            "PDFSHXTEXT",
            APPLY,
            "Select objects or [SEttings]:",
            || crate::command::CmdResult::Dispatch(OPEN_SETTINGS.to_string()),
        );
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
    }

    /// The SEttings option: the dialog, or its command-line form with FILEDIA 0.
    pub(in crate::app) fn open_pdf_shx_settings(&mut self, i: usize) {
        if crate::sys::file_dia() {
            let sample = crate::tr!("pdf-shx", "sample");
            self.pdf_shx_dialog = Some(ShxSettingsState::new(
                shx::settings(),
                self.drawing_path(i),
                sample,
            ));
            self.active_modal = Some(crate::app::ModalKind::PdfShxSettings);
        } else {
            self.start_pdf_shx_settings_prompt(i);
        }
    }

    pub(in crate::app) fn start_pdf_shx_settings_prompt(&mut self, i: usize) {
        use crate::command::CadCommand;
        let command = ShxSettingsCommand::new();
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
    }

    pub(in crate::app) fn update_pdf_shx_dialog(&mut self, message: ShxDialogMsg) -> Task<Message> {
        let Some(state) = self.pdf_shx_dialog.as_mut() else {
            return Task::none();
        };
        if !matches!(message, ShxDialogMsg::DragDrop(_) | ShxDialogMsg::DragStart(_)) {
            state.dragging = None;
        }
        let count = state.settings.fonts.len();
        match message {
            ShxDialogMsg::Select(index) => {
                state.selected = index.min(count.saturating_sub(1));
                state.refresh_preview();
            }
            ShxDialogMsg::Toggle(index, on) => {
                if let Some(font) = state.settings.fonts.get_mut(index) {
                    font.enabled = on;
                }
                state.selected = index.min(count.saturating_sub(1));
                state.refresh_preview();
            }
            ShxDialogMsg::Up => {
                let from = state.selected;
                state.move_font(from, from.saturating_sub(1));
            }
            ShxDialogMsg::Down => {
                let from = state.selected;
                state.move_font(from, (from + 1).min(count.saturating_sub(1)));
            }
            ShxDialogMsg::Remove => {
                if count > 1 && state.selected < count {
                    state.settings.fonts.remove(state.selected);
                    state.selected = state.selected.min(count - 2);
                    state.refresh_preview();
                }
            }
            ShxDialogMsg::Add => {
                return Task::perform(
                    async {
                        crate::sys::file_dialog()
                            .set_title(crate::tr!("pdf-shx", "add-title"))
                            .add_filter(crate::tr!("pdf-shx", "shx-files"), &["shx", "SHX"])
                            .pick_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h))
                    },
                    |path| Message::PdfShxDialog(ShxDialogMsg::Added(path)),
                );
            }
            ShxDialogMsg::Added(path) => {
                let Some(path) = path else {
                    return Task::none();
                };
                let full = path.to_string_lossy().into_owned();
                let label = shx::font_label(&full);
                match state
                    .settings
                    .fonts
                    .iter()
                    .position(|f| f.label().eq_ignore_ascii_case(&label))
                {
                    // A listed font is turned on and chosen rather than added twice.
                    Some(index) => {
                        state.settings.fonts[index].enabled = true;
                        state.selected = index;
                    }
                    None => {
                        let builtin = shx::FONT_NAMES.iter().any(|n| n.eq_ignore_ascii_case(&label));
                        state.settings.fonts.push(shx::ShxFontEntry {
                            name: if builtin { label } else { full },
                            enabled: true,
                        });
                        state.selected = state.settings.fonts.len() - 1;
                    }
                }
                state.refresh_preview();
            }
            ShxDialogMsg::DragStart(index) => state.dragging = Some(index),
            ShxDialogMsg::DragDrop(index) => {
                if let Some(from) = state.dragging.take() {
                    state.move_font(from, index);
                    state.refresh_preview();
                }
            }
            ShxDialogMsg::Threshold(value) => state.set_threshold(value),
            ShxDialogMsg::ThresholdText(text) => {
                // A value outside 1-100 or not a number is ignored, the box
                // keeps what was typed until it is fixed.
                if let Some(v) = text.trim().parse::<u8>().ok().filter(|v| (1..=100).contains(v)) {
                    state.settings.threshold = v;
                }
                state.threshold_text = text;
            }
            ShxDialogMsg::Step(dir) => {
                let value = (i16::from(state.settings.threshold) + i16::from(dir)).clamp(1, 100);
                state.set_threshold(value as u8);
            }
            ShxDialogMsg::CurrentLayer(on) => state.settings.current_layer = on,
            ShxDialogMsg::BestFont(on) => state.settings.best_font = on,
            ShxDialogMsg::Sample(text) => {
                state.sample = text;
                state.refresh_preview();
            }
            ShxDialogMsg::Defaults => {
                let show = state.settings.show_settings;
                state.settings = shx::ShxTextSettings {
                    show_settings: show,
                    ..Default::default()
                };
                state.selected = 0;
                state.threshold_text = state.settings.threshold.to_string();
                state.refresh_preview();
            }
            ShxDialogMsg::Help => {
                self.command_line
                    .push_info(&crate::tr!("pdf-shx", "help"));
            }
            ShxDialogMsg::Apply => {
                let Some(state) = self.pdf_shx_dialog.take() else {
                    return Task::none();
                };
                shx::set_settings(state.settings);
                self.save_config();
                self.close_active_modal();
                let i = self.active_tab;
                self.start_pdf_shx_gather(i);
                return self.focus_cmd_input();
            }
        }
        Task::none()
    }

    /// Recognise the text in `handles` and replace it with MTEXT.
    pub(in crate::app) fn convert_pdf_shx_text(&mut self, i: usize, handles: Vec<document::Handle>) {
        let document = &self.tabs[i].scene.document;
        let mut strokes = Vec::new();
        for handle in handles {
            let Some(entity) = document.get_entity(handle) else {
                continue;
            };
            let layer = entity.common().layer.clone();
            for points in shx::entity_strokes(entity) {
                if !points.is_empty() {
                    strokes.push(Stroke {
                        handle,
                        layer: layer.clone(),
                        points,
                    });
                }
            }
        }
        let settings = shx::settings();
        let drawing = self.drawing_path(i);
        self.command_line.push_output("Converting geometry to text...");
        let result = shx::recognize(&strokes, &settings, |name| {
            shx::resolve_font(name, drawing.as_deref())
        });
        for (n, group) in result.groups.iter().enumerate() {
            self.command_line.push_output(&if group.converted {
                format!("Group {}: Succeeded - {} {}%", n + 1, group.font, group.percent)
            } else {
                format!(
                    "Group {}: Failed to meet success threshold - {} {}%",
                    n + 1,
                    group.font,
                    group.percent
                )
            });
        }
        let converted = result.groups.iter().filter(|g| g.converted).count();
        self.command_line.push_output(&format!(
            "{converted} of {} groups converted to text",
            result.groups.len()
        ));
        self.command_line
            .push_output(&format!("{} text objects created", result.lines.len()));
        if result.lines.is_empty() {
            return;
        }

        self.push_undo_snapshot(i, "PDFSHXTEXT");
        let current_layer = {
            let header = &self.tabs[i].scene.document.header;
            if header.current_layer_name.is_empty() {
                self.tabs[i].active_layer.clone()
            } else {
                header.current_layer_name.clone()
            }
        };
        let scene = &mut self.tabs[i].scene;
        let mut erase: Vec<document::Handle> = Vec::new();
        for line in result.lines {
            // A style named after the font, made when the drawing lacks it.
            if !scene.document.text_styles.contains(&line.font) {
                let mut style = document::tables::TextStyle::new(line.font.as_str());
                style.handle = scene.document.allocate_handle();
                style.font_file = if line.font_spec.to_ascii_lowercase().ends_with(".shx") {
                    line.font_spec.clone()
                } else {
                    format!("{}.shx", line.font)
                };
                let _ = scene.document.text_styles.add(style);
            }
            let mut mtext = document::entities::MText::new();
            mtext.value = line.value;
            mtext.insertion_point =
                document::types::Vector3::new(line.insertion[0], line.insertion[1], 0.0);
            mtext.height = line.height;
            mtext.rectangle_width = 0.0;
            mtext.attachment_point = document::entities::AttachmentPoint::BottomLeft;
            mtext.drawing_direction = document::entities::DrawingDirection::ByStyle;
            mtext.style = line.font;
            mtext.common.layer = if settings.current_layer {
                current_layer.clone()
            } else {
                line.layer
            };
            scene.add_entity(document::EntityType::MText(Box::new(mtext)));
            erase.extend(line.handles);
        }
        erase.sort();
        erase.dedup();
        scene.erase_entities(&erase);
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }

    /// The private commands PDFSHXTEXT runs between its prompts.
    /// Whether `cmd` was one of them.
    pub(in crate::app) fn dispatch_pdf_shx_text(&mut self, cmd: &str, i: usize) -> bool {
        match cmd {
            "PDFSHXTEXT" => self.start_pdf_shx_text(i),
            APPLY => {
                let handles = self.tabs[i].scene.selected_handles_in_order();
                self.tabs[i].scene.deselect_all();
                self.convert_pdf_shx_text(i, handles);
            }
            OPEN_SETTINGS => self.open_pdf_shx_settings(i),
            shx::RESUME => self.start_pdf_shx_gather(i),
            shx::SAVE_SETTINGS => {
                self.save_config();
                self.start_pdf_shx_settings_prompt(i);
            }
            _ => return false,
        }
        true
    }
}
