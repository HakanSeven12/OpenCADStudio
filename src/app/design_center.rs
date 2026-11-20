//! DESIGN CENTER: lists the named content of an open or saved drawing and
//! adds the picked items to the active one through the paste's record and
//! block merge (`merge_named_content`), so they come with what they need.

use super::{Message, ModalKind, OpenCADStudio};
use crate::ui::window::design_center::{DesignCenterMsg, Source, State, KINDS};
use document::TableEntry;
use iced::Task;

/// The items of `kind` in `doc`: no anonymous, layout or xref-dependent
/// names, and no linetypes every drawing has.
fn items(doc: &document::Document, kind: &str) -> Vec<String> {
    let names: Vec<String> = match kind {
        "BLOCK" => doc
            .block_records
            .iter()
            .filter(|br| !br.is_standard() && !br.is_layout() && !br.name.starts_with('*'))
            .map(|br| br.name.clone())
            .collect(),
        "DIMSTYLE" => doc.dim_styles.iter().map(|s| s.name.clone()).collect(),
        "LAYER" => doc.layers.iter().map(|l| l.name.clone()).collect(),
        "LINETYPE" => doc
            .line_types
            .iter()
            .map(|l| l.name.clone())
            .filter(|name| {
                !["BYLAYER", "BYBLOCK", "CONTINUOUS"]
                    .iter()
                    .any(|fixed| name.eq_ignore_ascii_case(fixed))
            })
            .collect(),
        "MLEADERSTYLE" => doc
            .objects
            .values()
            .filter_map(|o| match o {
                document::objects::ObjectType::MultiLeaderStyle(s) => Some(s.name.clone()),
                _ => None,
            })
            .collect(),
        _ => doc
            .text_styles
            .iter()
            .filter(|s| !s.is_shape_file)
            .map(|s| s.name.clone())
            .collect(),
    };
    let mut names: Vec<String> = names
        .into_iter()
        .filter(|name| !name.is_empty() && !name.contains('|'))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names
}

/// An entity that refers to item `name` of `kind`, so the paste's capture
/// gathers the item and everything it names (a layer's linetype, a
/// dimension style's text style and arrow blocks, nested blocks …).
fn probe(doc: &document::Document, kind: &str, name: &str) -> Option<document::EntityType> {
    use document::entities::{Insert, Leader, Line, MultiLeader, Text};
    use document::EntityType as E;
    let mut entity = match kind {
        "BLOCK" => E::Insert(Insert::new(
            name.to_string(),
            document::types::Vector3::ZERO,
        )),
        "DIMSTYLE" => {
            let mut leader = Leader::new();
            leader.dimension_style = name.to_string();
            E::Leader(leader)
        }
        "LAYER" => {
            let mut line = Line::new();
            line.common.linetype = doc.layers.get(name)?.line_type.clone();
            line.common.layer = name.to_string();
            return Some(E::Line(line));
        }
        "LINETYPE" => {
            let mut line = Line::new();
            line.common.linetype = name.to_string();
            E::Line(line)
        }
        "MLEADERSTYLE" => {
            let mut leader = MultiLeader::new();
            leader.style_handle = doc.objects.iter().find_map(|(&h, o)| match o {
                document::objects::ObjectType::MultiLeaderStyle(s) if s.name == name => Some(h),
                _ => None,
            });
            E::MultiLeader(Box::new(leader))
        }
        _ => {
            let mut text = Text::new();
            text.style = name.to_string();
            E::Text(text)
        }
    };
    // Layer 0 is in every drawing, so the probe brings no other layer.
    entity.common_mut().layer = "0".to_string();
    Some(entity)
}

impl OpenCADStudio {
    pub(in crate::app) fn open_design_center(&mut self) {
        let mut sources: Vec<(String, Source)> = self
            .tabs
            .iter()
            .filter(|t| !t.is_start)
            .map(|t| (t.tab_display_name(), Source::Tab(t.id)))
            .collect();
        let active = self.tabs[self.active_tab].id;
        let source = sources
            .iter()
            .position(|(_, s)| matches!(s, Source::Tab(id) if *id == active))
            .unwrap_or(0);
        // Drawings browsed earlier stay listed.
        if let Some(previous) = self.design_center.take() {
            sources.extend(
                previous
                    .sources
                    .into_iter()
                    .filter(|(_, s)| matches!(s, Source::File(_))),
            );
        }
        let mut state = State {
            sources,
            source,
            kind: KINDS.iter().position(|k| *k == "LAYER").unwrap_or(0),
            items: Vec::new(),
            picked: Default::default(),
        };
        state.items = self.design_center_items(&state);
        self.design_center = Some(state);
        self.active_modal = Some(ModalKind::DesignCenter);
        self.reset_modal_geometry();
    }

    fn design_center_doc<'a>(&'a self, state: &'a State) -> Option<&'a document::Document> {
        match &state.sources.get(state.source)?.1 {
            Source::Tab(id) => self
                .tabs
                .iter()
                .find(|t| t.id == *id)
                .map(|t| &t.scene.document),
            Source::File(doc) => Some(doc),
        }
    }

    fn design_center_items(&self, state: &State) -> Vec<String> {
        self.design_center_doc(state)
            .map(|doc| items(doc, KINDS[state.kind]))
            .unwrap_or_default()
    }

    pub(in crate::app) fn on_design_center(&mut self, msg: DesignCenterMsg) -> Task<Message> {
        let Some(mut state) = self.design_center.take() else {
            return Task::none();
        };
        let mut task = Task::none();
        match msg {
            DesignCenterMsg::Source(index) => {
                state.source = index;
                state.picked.clear();
                state.items = self.design_center_items(&state);
            }
            DesignCenterMsg::Kind(kind) => {
                state.kind = kind;
                state.picked.clear();
                state.items = self.design_center_items(&state);
            }
            DesignCenterMsg::Toggle(name) => {
                if !state.picked.remove(&name) {
                    state.picked.insert(name);
                }
            }
            DesignCenterMsg::SelectAll => state.picked = state.items.iter().cloned().collect(),
            #[cfg(target_arch = "wasm32")]
            DesignCenterMsg::Browse => {}
            #[cfg(not(target_arch = "wasm32"))]
            DesignCenterMsg::Browse => {
                task = Task::perform(
                    async {
                        match rfd::AsyncFileDialog::new()
                            .set_title(crate::tr!("design-center", "browse-title"))
                            .add_filter(
                                crate::t!("DWG/DXF Files").as_ref(),
                                &["dwg", "dxf", "DWG", "DXF"],
                            )
                            .pick_file()
                            .await
                        {
                            Some(h) => Ok(crate::sys::handle_path(&h)),
                            None => Err(String::new()),
                        }
                    },
                    |r| Message::DesignCenter(DesignCenterMsg::Picked(r)),
                );
            }
            DesignCenterMsg::Picked(Err(_)) => {}
            DesignCenterMsg::Picked(Ok(path)) => match crate::io::load_file(&path) {
                Ok(doc) => {
                    let label = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    state.sources.push((label, Source::File(Box::new(doc))));
                    state.source = state.sources.len() - 1;
                    state.picked.clear();
                    state.items = self.design_center_items(&state);
                }
                Err(e) => self.command_line.push_error(&e.to_string()),
            },
            DesignCenterMsg::Add => self.design_center_add(&state),
        }
        self.design_center = Some(state);
        task
    }

    /// Add the picked items to the active drawing; names it already has
    /// keep their definitions.
    fn design_center_add(&mut self, state: &State) {
        let i = self.active_tab;
        let kind = KINDS[state.kind];
        let Some(source) = self.design_center_doc(state) else {
            return;
        };
        if matches!(&state.sources[state.source].1, Source::Tab(id) if *id == self.tabs[i].id) {
            self.command_line
                .push_error(&crate::tr!("design-center", "same-drawing"));
            return;
        }
        let picked: Vec<&String> = state.picked.iter().collect();
        let probes: Vec<document::EntityType> = picked
            .iter()
            .filter_map(|name| probe(source, kind, name))
            .collect();
        let deps = crate::app::ClipboardDeps::capture(source, &probes);
        let target = &self.tabs[i].scene.document;
        let existing = items(target, kind);
        let skipped = picked
            .iter()
            .filter(|name| existing.iter().any(|e| e.eq_ignore_ascii_case(name)))
            .count();

        self.push_undo_snapshot(i, "ADCENTER");
        let scene = &mut self.tabs[i].scene;
        let blocks_before = scene.document.block_records.len();
        crate::app::command_driver::clipboard::merge_named_content(&mut scene.document, &deps);
        if scene.document.block_records.len() > blocks_before {
            scene.populate_meshes_from_document();
            scene.bump_geometry();
        }
        self.tabs[i].dirty = true;
        self.refresh_layer_panel();
        self.refresh_properties();
        let added = picked.len() - skipped;
        self.command_line.push_info(&crate::tr!(
            "design-center",
            "added",
            added = added,
            skipped = skipped
        ));
    }
}
