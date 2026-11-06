//! Import layers from another drawing into the Layer manager (#2610): pick a
//! drawing, tick its layers, choose what a layer of the same name does, and
//! import them with the linetypes they use, as one undo step.

use super::{Message, OpenCADStudio};
use document::tables::{Layer as DocLayer, LineType};
use document::{Document, Handle};
use iced::Task;

/// What a ticked layer does when the drawing has one of the same name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OnConflict {
    #[default]
    Skip,
    /// The drawing's layer takes the imported layer's properties.
    Overwrite,
    /// The imported layer comes in under a free name: `Walls (2)`.
    Rename,
}

#[derive(Clone, Debug)]
pub struct ImportRow {
    pub layer: DocLayer,
    pub ticked: bool,
    /// The drawing has a layer of this name.
    pub exists: bool,
}

#[derive(Clone, Debug)]
pub struct LayerImportState {
    pub source: String,
    pub rows: Vec<ImportRow>,
    /// The source drawing's linetypes, for the layers that use them.
    pub linetypes: Vec<LineType>,
    pub on_conflict: OnConflict,
    pub search: String,
}

#[derive(Clone, Debug)]
pub enum LayerImportMsg {
    Pick,
    #[cfg(not(target_arch = "wasm32"))]
    Picked(std::path::PathBuf),
    /// Web: the picked drawing, already parsed.
    #[cfg(target_arch = "wasm32")]
    PickedWeb(Result<Option<(String, Box<Document>)>, String>),
    Toggle(usize),
    All(bool),
    Search(String),
    OnConflict(OnConflict),
    Import,
    Close,
}

/// The layers a drawing offers: its own, not those it borrows from an xref.
fn offered_layers(document: &Document) -> Vec<DocLayer> {
    let mut layers: Vec<DocLayer> = document
        .layers
        .iter()
        .filter(|layer| !layer.name.contains('|') && !layer.flags.xref_dependent)
        .filter(|layer| !layer.name.starts_with('*'))
        .cloned()
        .collect();
    layers.sort_by_key(|layer| layer.name.to_lowercase());
    layers
}

/// What an import did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub imported: usize,
    pub overwritten: usize,
    pub skipped: usize,
}

/// Bring the ticked layers of `state` into `document`.
pub fn import_layers(document: &mut Document, state: &LayerImportState) -> ImportReport {
    let mut report = ImportReport::default();
    for row in state.rows.iter().filter(|row| row.ticked) {
        let source = &row.layer;
        // The linetype comes along when the drawing lacks it.
        if !document.line_types.contains(&source.line_type) {
            if let Some(linetype) = state
                .linetypes
                .iter()
                .find(|lt| lt.name.eq_ignore_ascii_case(&source.line_type))
            {
                let mut linetype = linetype.clone();
                linetype.handle = document.allocate_handle();
                let _ = document.line_types.add(linetype);
            }
        }
        let exists = document.layers.contains(&source.name);
        let name = match (exists, state.on_conflict) {
            (false, _) => source.name.clone(),
            (true, OnConflict::Skip) => {
                report.skipped += 1;
                continue;
            }
            (true, OnConflict::Overwrite) => {
                if let Some(layer) = document.layers.get_mut(&source.name) {
                    layer.color = source.color;
                    layer.color_name = source.color_name.clone();
                    layer.book_name = source.book_name.clone();
                    layer.line_type = source.line_type.clone();
                    layer.line_weight = source.line_weight;
                    layer.transparency = source.transparency;
                    layer.is_plottable = source.is_plottable;
                    layer.description = source.description.clone();
                }
                report.overwritten += 1;
                continue;
            }
            (true, OnConflict::Rename) => (2..)
                .map(|n| format!("{} ({n})", source.name))
                .find(|name| !document.layers.contains(name))
                .unwrap_or_default(),
        };
        let mut layer = source.clone();
        layer.name = name;
        layer.handle = document.allocate_handle();
        // Objects of the other drawing don't come along.
        layer.material = Handle::NULL;
        layer.plotstyle_handle = Handle::NULL;
        if document.layers.add(layer).is_ok() {
            report.imported += 1;
        }
    }
    report
}

impl OpenCADStudio {
    pub(super) fn on_layer_import(&mut self, msg: LayerImportMsg) -> Task<Message> {
        match msg {
            LayerImportMsg::Pick => return pick(),
            #[cfg(not(target_arch = "wasm32"))]
            LayerImportMsg::Picked(path) => match crate::io::load_file(&path) {
                Ok(document) => {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.layer_import_open(name, &document);
                }
                Err(why) => self
                    .command_line
                    .push_error(crate::tf!("Import layers: {why}.").as_ref()),
            },
            #[cfg(target_arch = "wasm32")]
            LayerImportMsg::PickedWeb(result) => match result {
                Ok(Some((name, document))) => self.layer_import_open(name, &document),
                Ok(None) => {}
                Err(why) => self
                    .command_line
                    .push_error(crate::tf!("Import layers: {why}.").as_ref()),
            },
            LayerImportMsg::Close => {
                self.layer_import = None;
                self.reset_modal_geometry();
            }
            LayerImportMsg::Import => self.layer_import_apply(),
            msg => {
                let Some(state) = self.layer_import.as_mut() else {
                    return Task::none();
                };
                match msg {
                    LayerImportMsg::Toggle(at) => {
                        if let Some(row) = state.rows.get_mut(at) {
                            row.ticked = !row.ticked;
                        }
                    }
                    LayerImportMsg::All(on) => {
                        let search = state.search.to_lowercase();
                        for row in &mut state.rows {
                            if search.is_empty() || row.layer.name.to_lowercase().contains(&search)
                            {
                                row.ticked = on;
                            }
                        }
                    }
                    LayerImportMsg::Search(text) => state.search = text,
                    LayerImportMsg::OnConflict(choice) => state.on_conflict = choice,
                    _ => {}
                }
            }
        }
        Task::none()
    }

    fn layer_import_open(&mut self, source: String, document: &Document) {
        let i = self.active_tab;
        let here = &self.tabs[i].scene.document;
        let rows: Vec<ImportRow> = offered_layers(document)
            .into_iter()
            .map(|layer| ImportRow {
                // New layers are ticked; same-named ones wait to be chosen.
                exists: here.layers.contains(&layer.name),
                ticked: !here.layers.contains(&layer.name),
                layer,
            })
            .collect();
        if rows.is_empty() {
            self.command_line
                .push_error(crate::t!("no layers found in that file").as_ref());
            return;
        }
        self.layer_import = Some(LayerImportState {
            source,
            rows,
            linetypes: document.line_types.iter().cloned().collect(),
            on_conflict: OnConflict::Skip,
            search: String::new(),
        });
        self.reset_modal_geometry();
    }

    fn layer_import_apply(&mut self) {
        let i = self.active_tab;
        let Some(state) = self.layer_import.take() else {
            return;
        };
        self.reset_modal_geometry();
        if !state.rows.iter().any(|row| row.ticked) {
            return;
        }
        self.push_undo_snapshot(i, "LAYER IMPORT");
        let report = import_layers(&mut self.tabs[i].scene.document, &state);
        if report.imported + report.overwritten == 0 {
            self.discard_last_undo_entry(i);
        } else {
            self.tabs[i].dirty = true;
            self.refresh_layer_panel();
        }
        self.command_line.push_output(
            crate::tf!(
                "{} layer(s) imported, {} updated, {} skipped.",
                report.imported,
                report.overwritten,
                report.skipped
            )
            .as_ref(),
        );
    }
}

fn pick() -> Task<Message> {
    let wrap = |m| Message::LayerImport(m);
    #[cfg(not(target_arch = "wasm32"))]
    {
        Task::perform(
            crate::io::pick_layer_standard_path(),
            move |path| match path {
                Some(path) => wrap(LayerImportMsg::Picked(path)),
                None => Message::Noop,
            },
        )
    }
    #[cfg(target_arch = "wasm32")]
    {
        Task::perform(
            async {
                crate::io::pick_and_parse_web(
                    crate::t!("Import layers").as_ref(),
                    crate::t!("Drawings and standards").as_ref(),
                    &["dwg", "dws", "dwt", "dxf", "DWG", "DWS", "DWT", "DXF"],
                )
                .await
                .map(|picked| picked.map(|(name, document)| (name, Box::new(document))))
            },
            move |loaded| wrap(LayerImportMsg::PickedWeb(loaded)),
        )
    }
}

// ── View ─────────────────────────────────────────────────────────────────────

mod view {
    use super::*;
    use crate::t;
    use crate::ui::style::form::{button_style, dialog_button, dialog_button_styled_opt};
    use iced::widget::{
        button, checkbox, column, container, row, scrollable, text, text_input, Space,
    };
    use iced::{Background, Border, Element, Fill, Length, Theme};

    fn msg(m: LayerImportMsg) -> Message {
        Message::LayerImport(m)
    }

    pub(in crate::app) fn content<'a>(
        state: &'a LayerImportState,
        sizing: crate::ui::modal::ModalSizing,
    ) -> Element<'a, Message> {
        let muted = |s: String| {
            text(s)
                .size(11)
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.palette().background.base.text.scale_alpha(0.7)),
                })
        };
        let search = state.search.to_lowercase();
        let mut list = column![].spacing(2);
        for (at, row) in state.rows.iter().enumerate() {
            if !search.is_empty() && !row.layer.name.to_lowercase().contains(&search) {
                continue;
            }
            let swatch = container(Space::new().width(12).height(12)).style({
                let color = crate::ui::window::layers::iced_color_from_acad(&row.layer.color);
                move |_: &Theme| container::Style {
                    background: Some(Background::Color(color)),
                    ..Default::default()
                }
            });
            let mut line = row![
                checkbox(row.ticked)
                    .label(row.layer.name.clone())
                    .text_size(12)
                    .size(14)
                    .on_toggle(move |_| msg(LayerImportMsg::Toggle(at)))
                    .width(Fill),
                swatch,
                text(row.layer.line_type.clone())
                    .size(11)
                    .width(Length::Fixed(110.0)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);
            if row.exists {
                line = line.push(muted(t!("Already in this drawing").into_owned()));
            }
            list = list.push(line);
        }
        let ticked = state.rows.iter().filter(|row| row.ticked).count();
        let mut conflict = row![text(t!("Same name:")).size(12)]
            .spacing(4)
            .align_y(iced::Alignment::Center);
        for (choice, caption) in [
            (OnConflict::Skip, t!("Skip")),
            (OnConflict::Overwrite, t!("Overwrite")),
            (OnConflict::Rename, t!("Rename")),
        ] {
            conflict = conflict.push(
                button(text(caption).size(12))
                    .on_press(msg(LayerImportMsg::OnConflict(choice)))
                    .style(button_style(state.on_conflict == choice))
                    .padding([4, 10]),
            );
        }
        let body = column![
            row![
                text(crate::tf!("Layers in {}", state.source))
                    .size(12)
                    .width(Fill),
                text_input(&t!("Search…"), &state.search)
                    .on_input(|v| msg(LayerImportMsg::Search(v)))
                    .size(12)
                    .width(Length::Fixed(180.0)),
                dialog_button(t!("All"), msg(LayerImportMsg::All(true)), false),
                dialog_button(t!("None"), msg(LayerImportMsg::All(false)), false),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
            container(scrollable(list).height(Length::Fixed(320.0)))
                .padding(8)
                .width(Fill)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(theme.palette().background.base.color)),
                    border: Border {
                        color: theme.palette().background.neutral.color,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }),
            conflict,
            row![
                Space::new().width(Fill),
                dialog_button(t!("Cancel"), msg(LayerImportMsg::Close), false),
                dialog_button_styled_opt(
                    crate::tf!("Import {}", ticked),
                    (ticked > 0).then_some(msg(LayerImportMsg::Import)),
                    button::primary,
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10)
        .width(sizing.width)
        .height(sizing.height);
        container(body)
            .padding(16)
            .width(sizing.width)
            .height(sizing.height)
            .style(|theme: &Theme| {
                let palette = theme.palette();
                container::Style {
                    background: Some(Background::Color(palette.background.weak.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
    }
}

impl OpenCADStudio {
    /// The import dialog in the shared movable modal frame.
    pub(super) fn layer_import_overlay<'a>(
        &self,
        state: &'a LayerImportState,
    ) -> iced::Element<'a, Message> {
        let measurement = view::content(state, crate::ui::modal::ModalSizing::INTRINSIC);
        let content = view::content(state, crate::ui::modal::ModalSizing::FILL);
        let content = crate::ui::modal::intrinsic(
            measurement,
            content,
            iced::Size::new(720.0, f32::INFINITY),
            self.modal_resize,
        );
        crate::ui::modal::modal(
            iced::widget::Space::new()
                .width(iced::Fill)
                .height(iced::Fill),
            crate::t!("Import layers"),
            content,
            Message::LayerImport(LayerImportMsg::Close),
            self.modal_offset,
            crate::ui::modal::ModalOptions::STANDARD,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Document {
        let mut document = Document::new();
        let mut hidden = LineType::new("HIDDEN");
        hidden.handle = document.allocate_handle();
        let _ = document.line_types.add(hidden);
        for (name, color, linetype) in [("Walls", 1, "HIDDEN"), ("Doors", 3, "Continuous")] {
            let mut layer = DocLayer::new(name);
            layer.handle = document.allocate_handle();
            layer.color = document::types::Color::Index(color);
            layer.line_type = linetype.into();
            let _ = document.layers.add(layer);
        }
        document
    }

    fn state_for(here: &Document, from: &Document, on_conflict: OnConflict) -> LayerImportState {
        LayerImportState {
            source: "standard.dwg".into(),
            rows: offered_layers(from)
                .into_iter()
                .map(|layer| ImportRow {
                    exists: here.layers.contains(&layer.name),
                    ticked: layer.name != "0",
                    layer,
                })
                .collect(),
            linetypes: from.line_types.iter().cloned().collect(),
            on_conflict,
            search: String::new(),
        }
    }

    /// #2610: new layers come in with their linetype; a same-named one is
    /// skipped, overwritten or renamed as chosen.
    #[test]
    fn import_brings_layers_and_their_linetypes() {
        let from = source();
        let mut here = Document::new();
        let mut walls = DocLayer::new("Walls");
        walls.handle = here.allocate_handle();
        let _ = here.layers.add(walls);

        let mut skip = here.clone();
        let state = state_for(&skip, &from, OnConflict::Skip);
        let report = import_layers(&mut skip, &state);
        assert_eq!(
            report,
            ImportReport {
                imported: 1,
                overwritten: 0,
                skipped: 1
            }
        );
        assert!(skip.layers.contains("Doors"));
        assert_eq!(
            skip.layers.get("Walls").unwrap().color,
            document::types::Color::Index(7)
        );

        let mut over = here.clone();
        let state = state_for(&over, &from, OnConflict::Overwrite);
        let report = import_layers(&mut over, &state);
        assert_eq!(report.overwritten, 1);
        let walls = over.layers.get("Walls").unwrap();
        assert_eq!(walls.color, document::types::Color::Index(1));
        assert_eq!(walls.line_type, "HIDDEN");
        assert!(
            over.line_types.contains("HIDDEN"),
            "the linetype came along"
        );

        let mut renamed = here.clone();
        let state = state_for(&renamed, &from, OnConflict::Rename);
        let report = import_layers(&mut renamed, &state);
        assert_eq!(report.imported, 2);
        assert!(renamed.layers.contains("Walls (2)"));
        assert!(renamed.layers.get("Walls (2)").unwrap().handle.is_valid());
    }

    #[test]
    fn import_from_the_dialog_is_one_undo_step() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.layer_import_open("standard.dwg".into(), &source());
        let state = app.layer_import.as_ref().expect("the dialog opens");
        assert!(state
            .rows
            .iter()
            .any(|row| row.layer.name == "Walls" && row.ticked));
        let zero = state
            .rows
            .iter()
            .position(|row| row.layer.name == "0")
            .unwrap();
        assert!(state.rows[zero].exists && !state.rows[zero].ticked);
        app.finish_pending_history(i);
        let before = app.tabs[i].history.undo_stack.len();
        let _ = app.update(Message::LayerImport(LayerImportMsg::Import));
        assert!(app.layer_import.is_none());
        assert!(app.tabs[i].scene.document.layers.contains("Walls"));
        assert!(app.tabs[i].scene.document.layers.contains("Doors"));
        assert_eq!(app.tabs[i].history.undo_stack.len(), before + 1);
        app.undo_active_tab();
        assert!(!app.tabs[i].scene.document.layers.contains("Walls"));
    }
}
