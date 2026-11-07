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
    /// What differs from the drawing's layer of that name (colour,
    /// linetype, lineweight), empty when it is the same.
    pub differs: Vec<&'static str>,
}

#[derive(Clone, Debug)]
pub struct LayerImportState {
    pub source: String,
    pub rows: Vec<ImportRow>,
    /// The source drawing's linetypes, for the layers that use them.
    pub linetypes: Vec<LineType>,
    /// The styles their shape and text segments point to.
    pub linetype_styles: Vec<document::tables::TextStyle>,
    pub on_conflict: OnConflict,
    pub search: String,
    /// The source's layer groups, by layer name.
    pub groups: Vec<(String, Vec<String>)>,
    /// Bring the groups whose layers come in.
    pub bring_groups: bool,
    /// The source's property filters, brought with the groups.
    pub filters: Vec<super::layer_filter_store::StoredLayerFilter>,
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
    /// Tick only the layers the drawing hasn't got.
    OnlyNew,
    BringGroups(bool),
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub imported: usize,
    pub overwritten: usize,
    pub skipped: usize,
    /// The drawing's layers that took the imported properties.
    pub overwritten_names: Vec<String>,
    /// Each imported or overwritten layer: its name in the source and the
    /// name it has in the drawing now (a renamed one is "X (2)").
    pub targets: Vec<(String, String)>,
}

/// Bring the ticked layers of `state` into `document`.
pub fn import_layers(document: &mut Document, state: &LayerImportState) -> ImportReport {
    let mut report = ImportReport::default();
    // The linetypes the imported layers use come along when the drawing
    // lacks them, with the styles their segments point to, as a paste
    // brings them.
    let mut deps = crate::app::ClipboardDeps::default();
    let bring_linetype = |document: &Document, deps: &mut crate::app::ClipboardDeps, name: &str| {
        if document.line_types.contains(name)
            || deps.linetypes.iter().any(|lt| lt.name.eq_ignore_ascii_case(name))
        {
            return;
        }
        let Some(linetype) = state
            .linetypes
            .iter()
            .find(|lt| lt.name.eq_ignore_ascii_case(name))
        else {
            return;
        };
        for complex in linetype.elements.iter().filter_map(|e| e.complex.as_ref()) {
            if let Some(style) = state
                .linetype_styles
                .iter()
                .find(|style| style.handle == complex.style_handle)
            {
                if !deps.linetype_styles.iter().any(|s| s.handle == style.handle) {
                    deps.linetype_styles.push(style.clone());
                }
            }
        }
        deps.linetypes.push(linetype.clone());
    };
    for row in state.rows.iter().filter(|row| row.ticked) {
        let source = &row.layer;
        let exists = document.layers.contains(&source.name);
        if !(exists && state.on_conflict == OnConflict::Skip) {
            bring_linetype(document, &mut deps, &source.line_type);
        }
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
                report.overwritten_names.push(source.name.clone());
                report.targets.push((source.name.clone(), source.name.clone()));
                continue;
            }
            (true, OnConflict::Rename) => (2..)
                .map(|n| format!("{} ({n})", source.name))
                .find(|name| !document.layers.contains(name))
                .unwrap_or_default(),
        };
        let mut layer = source.clone();
        layer.name = name.clone();
        layer.handle = document.allocate_handle();
        // Objects of the other drawing don't come along.
        layer.material = Handle::NULL;
        layer.plotstyle_handle = Handle::NULL;
        if document.layers.add(layer).is_ok() {
            report.imported += 1;
            report.targets.push((source.name.clone(), name));
        }
    }
    crate::app::command_driver::clipboard::merge_table_records(document, &deps);
    report
}

impl OpenCADStudio {
    pub(super) fn on_layer_import(&mut self, msg: LayerImportMsg) -> Task<Message> {
        match msg {
            LayerImportMsg::Pick => return pick(),
            #[cfg(not(target_arch = "wasm32"))]
            LayerImportMsg::Picked(path) => match crate::io::load_file(&path) {
                Ok(document) => {
                    // The whole path, as the mockup's Source field shows it.
                    self.layer_import_open(path.display().to_string(), &document);
                    return self.layers_dialog_to_front();
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
                    LayerImportMsg::OnlyNew => {
                        for row in &mut state.rows {
                            row.ticked = !row.exists;
                        }
                    }
                    LayerImportMsg::BringGroups(on) => state.bring_groups = on,
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
            .map(|layer| {
                let mine = here
                    .layers
                    .iter()
                    .find(|l| l.name.eq_ignore_ascii_case(&layer.name));
                let mut differs = Vec::new();
                if let Some(mine) = mine {
                    if mine.color != layer.color {
                        differs.push("color");
                    }
                    if !mine.line_type.eq_ignore_ascii_case(&layer.line_type) {
                        differs.push("linetype");
                    }
                    if mine.line_weight != layer.line_weight {
                        differs.push("lineweight");
                    }
                }
                ImportRow {
                    // New layers are ticked; same-named ones wait to be chosen.
                    exists: mine.is_some(),
                    ticked: mine.is_none(),
                    differs,
                    layer,
                }
            })
            .collect();
        // The source's property filters: they read properties, not layers,
        // so they all come along.
        let filters = super::layer_filter_store::stored_layer_filters(document);
        // The source's groups, by layer name, to bring with their layers.
        let groups: Vec<(String, Vec<String>)> = super::layer_groups::layer_groups(document)
            .into_iter()
            .map(|group| {
                let names = group
                    .layers
                    .iter()
                    .filter_map(|h| document.layers.iter().find(|l| l.handle == *h))
                    .map(|l| l.name.clone())
                    .collect();
                (group.name, names)
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
            linetype_styles: document.text_styles.iter().cloned().collect(),
            on_conflict: OnConflict::Skip,
            search: String::new(),
            bring_groups: !groups.is_empty() || !filters.is_empty(),
            groups,
            filters,
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
        if state.bring_groups && !state.filters.is_empty() {
            // Filters the drawing has not got, in the same undo step.
            let document = &mut self.tabs[i].scene.document;
            let mut stored = super::layer_filter_store::stored_layer_filters(document);
            for filter in &state.filters {
                if !stored
                    .iter()
                    .any(|s| s.name.eq_ignore_ascii_case(&filter.name))
                {
                    stored.push(filter.clone());
                }
            }
            super::layer_filter_store::write_layer_filters(document, &stored);
        }
        if state.bring_groups {
            // Only the layers that came in join their groups, under the name
            // they got here; a skipped one leaves the drawing's layer alone.
            let document = &mut self.tabs[i].scene.document;
            for (group, names) in &state.groups {
                let mut members: Vec<Handle> = super::layer_groups::layer_groups(document)
                    .into_iter()
                    .find(|g| g.name.eq_ignore_ascii_case(group))
                    .map(|g| g.layers)
                    .unwrap_or_default();
                let mut added = false;
                for name in names.iter().filter_map(|n| {
                    report
                        .targets
                        .iter()
                        .find(|(source, _)| source.eq_ignore_ascii_case(n))
                        .map(|(_, target)| target)
                }) {
                    if let Some(layer) = document
                        .layers
                        .iter()
                        .find(|l| l.name.eq_ignore_ascii_case(name))
                    {
                        if !members.contains(&layer.handle) {
                            members.push(layer.handle);
                            added = true;
                        }
                    }
                }
                if added {
                    super::layer_groups::store_layer_group(document, group, &members);
                }
            }
        }
        if report.imported + report.overwritten == 0 {
            self.discard_last_undo_entry(i);
        } else {
            let names = &report.overwritten_names;
            self.tabs[i].scene.invalidate_layer_visibility(names);
            self.tabs[i].scene.invalidate_layer_dependencies(names);
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
    use crate::ui::style::form::{dialog_button, dialog_button_styled_opt};
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
        // As in the mockup: Source with Browse…; All · None · Only new and
        // the search; a table of the source's layers with what the drawing
        // has of each; what a same-named layer does and what else comes
        // along; a summary line over Cancel and Import.
        const W_CHECK: f32 = 28.0;
        const W_NAME: f32 = 170.0;
        const W_COLOR: f32 = 100.0;
        const W_LT: f32 = 110.0;
        const W_LW: f32 = 80.0;
        let header_cell =
            |label: std::borrow::Cow<'static, str>, w: Length| -> Element<'a, Message> {
                muted(label.into_owned()).width(w).into()
            };
        let header = container(
            row![
                Space::new().width(W_CHECK),
                header_cell(t!("Name"), Length::Fixed(W_NAME)),
                header_cell(t!("Color"), Length::Fixed(W_COLOR)),
                header_cell(t!("Linetype"), Length::Fixed(W_LT)),
                header_cell(t!("Lineweight"), Length::Fixed(W_LW)),
                header_cell(t!("In this drawing"), Fill),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding([4, 8]);
        let search = state.search.to_lowercase();
        let mut list = column![].spacing(0);
        let mut shown = 0usize;
        for (at, row) in state.rows.iter().enumerate() {
            if !search.is_empty() && !row.layer.name.to_lowercase().contains(&search) {
                continue;
            }
            let swatch = container(Space::new().width(12).height(12)).style({
                let color = crate::ui::window::layers::iced_color_from_acad(&row.layer.color);
                move |theme: &Theme| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border {
                        color: theme.palette().background.base.text.scale_alpha(0.5),
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                }
            });
            let state_cell: Element<'a, Message> = if !row.exists {
                text(t!("New")).size(11).into()
            } else if row.differs.is_empty() {
                muted(t!("Exists, identical").into_owned()).into()
            } else {
                let what = row
                    .differs
                    .iter()
                    .map(|d| match *d {
                        "color" => t!("color").into_owned(),
                        "linetype" => t!("linetype").into_owned(),
                        _ => t!("lineweight").into_owned(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                row![
                    crate::ui::icons::themed_warning(crate::ui::icons::WARNING, 12.0),
                    text(crate::tf!("Exists, {} differs", what)).size(11),
                ]
                .spacing(5)
                .align_y(iced::Alignment::Center)
                .into()
            };
            let dim = !row.ticked;
            let cell = move |s: String, w: f32| {
                text(s)
                    .size(11)
                    .width(Length::Fixed(w))
                    .wrapping(iced::widget::text::Wrapping::None)
                    .style(move |theme: &Theme| iced::widget::text::Style {
                        color: dim.then(|| theme.palette().background.base.text.scale_alpha(0.55)),
                    })
            };
            let line = row![
                container(
                    checkbox(row.ticked)
                        .size(14)
                        .on_toggle(move |_| msg(LayerImportMsg::Toggle(at)))
                )
                .width(W_CHECK),
                cell(row.layer.name.clone(), W_NAME),
                row![
                    swatch,
                    cell(
                        crate::ui::color_select::color_display_name(row.layer.color),
                        W_COLOR - 18.0
                    )
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center)
                .width(Length::Fixed(W_COLOR)),
                cell(row.layer.line_type.clone(), W_LT),
                cell(
                    crate::ui::properties::LwItem(row.layer.line_weight).to_string(),
                    W_LW
                ),
                state_cell,
            ]
            .align_y(iced::Alignment::Center)
            .height(26);
            let stripe = shown % 2 == 1;
            shown += 1;
            list = list.push(container(line).padding([0, 8]).width(Fill).style(
                move |theme: &Theme| container::Style {
                    background:
                        stripe.then(|| Background::Color(theme.palette().background.weak.color)),
                    ..Default::default()
                },
            ));
        }
        let ticked = state.rows.iter().filter(|row| row.ticked).count();
        let new = state
            .rows
            .iter()
            .filter(|row| row.ticked && !row.exists)
            .count();
        let skipped = match state.on_conflict {
            OnConflict::Skip => state
                .rows
                .iter()
                .filter(|row| row.ticked && row.exists)
                .count(),
            _ => 0,
        };
        let fieldset = |title: std::borrow::Cow<'static, str>,
                        content: Element<'a, Message>|
         -> Element<'a, Message> {
            container(column![text(title).size(12), content].spacing(6))
                .padding(10)
                .width(Fill)
                .style(|theme: &Theme| container::Style {
                    border: Border {
                        color: theme.palette().background.neutral.color,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        };
        let example = state
            .rows
            .iter()
            .find(|row| row.exists && row.layer.name != "0")
            .map_or_else(|| "A-WALL".to_string(), |row| row.layer.name.clone());
        let mut conflict = column![].spacing(4);
        for (choice, caption) in [
            (OnConflict::Skip, t!("Skip it").into_owned()),
            (
                OnConflict::Overwrite,
                t!("Overwrite its properties").into_owned(),
            ),
            (
                OnConflict::Rename,
                crate::tf!("Import with a suffix, e.g. “{} (2)”", example).into_owned(),
            ),
        ] {
            conflict = conflict.push(
                iced::widget::radio(caption, choice, Some(state.on_conflict), |c| {
                    msg(LayerImportMsg::OnConflict(c))
                })
                .text_size(12)
                .size(14),
            );
        }
        let missing_lt: Vec<String> = {
            let mut names: Vec<String> = state
                .rows
                .iter()
                .filter(|row| row.ticked)
                .map(|row| row.layer.line_type.clone())
                .filter(|lt| {
                    !lt.eq_ignore_ascii_case("Continuous")
                        && !lt.eq_ignore_ascii_case("ByLayer")
                        && !lt.eq_ignore_ascii_case("ByBlock")
                })
                .collect();
            names.sort();
            names.dedup();
            names
        };
        let bring = column![
            checkbox(true)
                .label(if missing_lt.is_empty() {
                    t!("Missing linetypes").into_owned()
                } else {
                    format!("{} ({})", t!("Missing linetypes"), missing_lt.join(", "))
                })
                .text_size(12)
                .size(14),
            checkbox(state.bring_groups && !(state.groups.is_empty() && state.filters.is_empty()))
                .label(crate::tf!(
                    "Groups and filters ({})",
                    state.groups.len() + state.filters.len()
                ))
                .text_size(12)
                .size(14)
                .on_toggle_maybe(
                    (!(state.groups.is_empty() && state.filters.is_empty()))
                        .then_some(|on| msg(LayerImportMsg::BringGroups(on)))
                ),
        ]
        .spacing(4);
        let body = column![
            row![
                text(t!("Source")).size(12).width(Length::Fixed(50.0)),
                container(
                    text(state.source.clone())
                        .size(12)
                        .wrapping(iced::widget::text::Wrapping::None)
                )
                .clip(true)
                .padding([4, 8])
                .width(Fill)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(theme.palette().background.base.color)),
                    border: Border {
                        color: theme.palette().background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }),
                dialog_button(t!("Browse..."), msg(LayerImportMsg::Pick), false),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            row![
                dialog_button(t!("All"), msg(LayerImportMsg::All(true)), false),
                dialog_button(t!("None"), msg(LayerImportMsg::All(false)), false),
                dialog_button(t!("Only new"), msg(LayerImportMsg::OnlyNew), false),
                Space::new().width(Fill),
                text_input(&t!("Search…"), &state.search)
                    .on_input(|v| msg(LayerImportMsg::Search(v)))
                    .size(12)
                    .width(Length::Fixed(180.0)),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
            container(column![header, scrollable(list).height(Fill)])
                .width(Fill)
                .height(Fill)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(theme.palette().background.base.color)),
                    border: Border {
                        color: theme.palette().background.neutral.color,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }),
            row![
                fieldset(t!("When a name already exists"), conflict.into()),
                fieldset(t!("Also bring"), bring.into()),
            ]
            .spacing(10),
            row![
                muted(
                    crate::tf!(
                        "{} selected · {} new, {} skipped · one undo step",
                        ticked,
                        new,
                        skipped
                    )
                    .into_owned()
                ),
                Space::new().width(Fill),
                dialog_button(t!("Cancel"), msg(LayerImportMsg::Close), false),
                dialog_button_styled_opt(
                    t!("Import"),
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
                    differs: Vec::new(),
                    layer,
                })
                .collect(),
            linetypes: from.line_types.iter().cloned().collect(),
            linetype_styles: from.text_styles.iter().cloned().collect(),
            on_conflict,
            search: String::new(),
            groups: Vec::new(),
            bring_groups: false,
            filters: Vec::new(),
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
                skipped: 1,
                targets: vec![("Doors".into(), "Doors".into())],
                ..Default::default()
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

    /// #2610 (F1): a same-named layer says what differs; Only new ticks the
    /// new ones; the source's groups come along with their layers.
    #[test]
    fn import_differences_only_new_and_groups() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut from = source();
        let walls = from
            .layers
            .iter()
            .find(|l| l.name == "Walls")
            .unwrap()
            .handle;
        super::super::layer_groups::store_layer_group(&mut from, "Shell", &[walls]);
        // The drawing has its own, red "Doors".
        let mut doors = DocLayer::with_color("Doors", document::types::Color::Index(1));
        doors.handle = app.tabs[i].scene.document.allocate_handle();
        let _ = app.tabs[i].scene.document.layers.add(doors);
        app.layer_import_open("standard.dwg".into(), &from);
        let state = app.layer_import.as_ref().unwrap();
        let row = state.rows.iter().find(|r| r.layer.name == "Doors").unwrap();
        assert!(row.exists && row.differs.contains(&"color"));
        assert_eq!(state.groups.len(), 1);
        let _ = app.update(Message::LayerImport(LayerImportMsg::All(true)));
        let _ = app.update(Message::LayerImport(LayerImportMsg::OnlyNew));
        let state = app.layer_import.as_ref().unwrap();
        assert!(state.rows.iter().all(|r| r.ticked != r.exists));
        let _ = app.update(Message::LayerImport(LayerImportMsg::Import));
        let groups = super::super::layer_groups::layer_groups(&app.tabs[i].scene.document);
        let shell = groups
            .iter()
            .find(|g| g.name == "Shell")
            .expect("the group came along");
        let walls_here = app.tabs[i]
            .scene
            .document
            .layers
            .iter()
            .find(|l| l.name == "Walls")
            .unwrap()
            .handle;
        assert_eq!(shell.layers, [walls_here]);
    }
}
