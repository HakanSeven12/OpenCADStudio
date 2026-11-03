use super::OpenCADStudio;
use crate::ui;

impl OpenCADStudio {
    /// Apply the current layer to the document header and all per-tab/UI
    /// creation state. Both built-in commands and plugin settings use this.
    pub(super) fn set_current_layer_name(&mut self, tab: usize, layer: &str) -> Result<(), String> {
        let handle = self.tabs[tab]
            .scene
            .document
            .layers
            .get(layer)
            .map(|entry| entry.handle)
            .ok_or_else(|| format!("layer {layer:?} does not exist"))?;
        self.tabs[tab].scene.document.header.current_layer_name = layer.to_owned();
        self.tabs[tab].scene.document.header.current_layer_handle = handle;
        self.tabs[tab].active_layer = layer.to_owned();
        self.tabs[tab].layers.current_layer = layer.to_owned();
        self.tabs[tab].dirty = true;
        if tab == self.active_tab {
            self.ribbon.active_layer = layer.to_owned();
            self.refresh_layer_panel();
        }
        Ok(())
    }
    pub(super) fn load_layer_state_editor(&mut self, selected: Option<String>) {
        let i = self.active_tab;
        if let Some(name) = selected {
            if let Some(state) = self.tabs[i].scene.document.layer_state(&name) {
                self.layer_state_name_buf = state.name.clone();
                self.layer_state_description_buf = state.description;
                self.layer_state_selected = Some(state.name);
                return;
            }
        }
        self.layer_state_selected = None;
        self.layer_state_description_buf.clear();
        let names: Vec<String> = self.tabs[i]
            .scene
            .document
            .layer_states()
            .into_iter()
            .map(|state| state.name)
            .collect();
        let n = (1usize..)
            .find(|n| {
                let candidate = format!("Layer State {n}");
                !names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&candidate))
            })
            .unwrap_or(1);
        self.layer_state_name_buf = format!("Layer State {n}");
    }

    /// Reload the `LayerPanel` cache from the document and push the
    /// fresh state through to the ribbon dropdown + every other
    /// layer-aware UI mirror. Use this whenever a command mutates the
    /// document's layer table directly (LAYOFF / LAYFRZ / LAYLCK …) so
    /// the panel + dropdown reflect the change. See #39.
    pub(super) fn refresh_layer_panel(&mut self) {
        let i = self.active_tab;
        let doc_layers = self.tabs[i].scene.document.layers.clone();
        let vp_info = self.tabs[i].scene.viewport_list();
        let names_before: Vec<String> = self.tabs[i]
            .layers
            .layers
            .iter()
            .map(|l| l.name.clone())
            .collect();
        self.tabs[i]
            .layers
            .sync_with_viewports(&doc_layers, vp_info);
        self.sync_ribbon_layers();
        // A walk in flight reads the same table: a layer added, renamed or
        // erased by any other command has to reach its rows, or the override
        // would go on hiding (or showing) what no longer exists.
        self.walk_rebuild();
        // The Properties palette lists layer names it read when it was built;
        // a layer added, removed or renamed must reach that list too (#1664).
        // Only then: the rebuild is costly for a large selection.
        let names_changed = self.tabs[i].layers.layers.len() != names_before.len()
            || self.tabs[i]
                .layers
                .layers
                .iter()
                .zip(&names_before)
                .any(|(layer, before)| layer.name != *before);
        if names_changed {
            self.refresh_properties();
        }
    }

    /// Pick up layers an entity edit registered on the fly (`ensure_layer`).
    pub(super) fn sync_registered_layers(&mut self, tab: usize) {
        if !std::mem::take(&mut self.tabs[tab].scene.layer_table_dirty) {
            return;
        }
        self.tabs[tab].dirty = true;
        self.refresh_layer_panel();
    }

    pub(super) fn sync_ribbon_layers(&mut self) {
        let i = self.active_tab;
        // The Start (welcome) tab has no document — leave the layer and
        // linetype dropdowns empty instead of showing a placeholder "0".
        if self.tabs[i].is_start {
            self.ribbon.set_layers(vec![], "");
            self.ribbon.set_available_linetypes(vec![]);
            self.sync_ribbon_styles();
            return;
        }
        let active = self.tabs[i].active_layer.clone();
        let vp_column = self.tabs[i].scene.active_viewport.and_then(|vp| {
            (self.tabs[i].scene.current_layout != "Model")
                .then(|| {
                    self.tabs[i]
                        .layers
                        .vp_cols
                        .iter()
                        .position(|c| c.handle == vp)
                })
                .flatten()
        });
        let infos: Vec<crate::ui::ribbon::LayerInfo> = self.tabs[i]
            .layers
            .layers
            .iter()
            // The reference's hidden system layers (`*ADSK_CONSTRAINTS`) stay out.
            .filter(|l| !l.name.starts_with('*'))
            .map(|l| crate::ui::ribbon::LayerInfo {
                stored_color: l.color,
                name: l.name.clone(),
                color: crate::ui::window::layers::iced_color_from_acad(&l.color),
                visible: l.visible,
                frozen: l.frozen,
                locked: l.locked,
                vp_frozen: vp_column
                    .map(|column| (column, l.vp_frozen.get(column).copied().unwrap_or(false))),
            })
            .collect();
        let names: Vec<String> = infos.iter().map(|l| l.name.clone()).collect();
        let active = if names.contains(&active) {
            active
        } else {
            "0".to_string()
        };
        self.tabs[i].active_layer = active.clone();
        self.tabs[i].layers.current_layer = active.clone();
        self.ribbon.set_layers(infos, &active);
        let mut states: Vec<String> = self.tabs[i]
            .scene
            .document
            .layer_states()
            .into_iter()
            .map(|state| state.name)
            .collect();
        states.sort_by_key(|name| name.to_lowercase());
        self.ribbon.layer_states = states;
        let lt_items: Vec<ui::properties::LinetypeItem> = self.tabs[i]
            .scene
            .document
            .line_types
            .iter()
            .map(|lt| {
                let name = if lt.name.eq_ignore_ascii_case("bylayer") {
                    "ByLayer".to_string()
                } else {
                    lt.name.clone()
                };
                let art = crate::io::linetypes::extract_pattern(&lt.description);
                ui::properties::LinetypeItem { name, art }
            })
            .chain(catalog_linetype_items(&self.tabs[i].scene.document))
            .collect();
        self.tabs[i].layers.sync_linetypes(lt_items.clone());
        self.ribbon.set_available_linetypes(lt_items);
        self.sync_ribbon_styles();
    }

    pub(super) fn sync_ribbon_styles(&mut self) {
        let i = self.active_tab;
        // The Start (welcome) tab has no real document — keep the Annotate
        // style dropdowns empty rather than showing a placeholder style.
        if self.tabs[i].is_start {
            self.ribbon
                .set_styles(vec![], "", vec![], "", vec![], "", vec![], "");
            return;
        }
        let doc = &self.tabs[i].scene.document;

        let text_names: Vec<String> = doc
            .text_styles
            .iter()
            .filter(|s| !s.is_shape_file)
            .map(|s| s.name.clone())
            .collect();
        let active_text = doc.header.current_text_style_name.clone();
        let active_text = if text_names.contains(&active_text) {
            active_text
        } else {
            text_names.first().cloned().unwrap_or_default()
        };

        let dim_names: Vec<String> = doc.dim_styles.iter().map(|s| s.name.clone()).collect();
        let active_dim = doc.header.current_dimstyle_name.clone();
        let active_dim = if dim_names.contains(&active_dim) {
            active_dim
        } else {
            dim_names.first().cloned().unwrap_or_default()
        };

        let mleader_names: Vec<String> = doc
            .objects
            .values()
            .filter_map(|o| {
                if let codec::objects::ObjectType::MultiLeaderStyle(mls) = o {
                    Some(mls.name.clone())
                } else {
                    None
                }
            })
            .collect();
        let current_mleader = doc.header.current_mleader_style_name.clone();

        let active_mleader = mleader_names
            .iter()
            .find(|name| name.eq_ignore_ascii_case(&current_mleader))
            .cloned()
            .or_else(|| mleader_names.first().cloned())
            .unwrap_or_default();

        let table_names: Vec<String> = doc
            .objects
            .values()
            .filter_map(|o| {
                if let codec::objects::ObjectType::TableStyle(ts) = o {
                    Some(ts.name.clone())
                } else {
                    None
                }
            })
            .collect();
        let active_table = doc.header.current_table_style_name.clone();
        let active_table = if table_names.contains(&active_table) {
            active_table
        } else {
            table_names.first().cloned().unwrap_or_default()
        };

        let active_mleader2 = active_mleader.clone();
        let active_table2 = active_table.clone();
        self.ribbon.set_styles(
            text_names,
            &active_text,
            dim_names,
            &active_dim,
            mleader_names,
            &active_mleader2,
            table_names,
            &active_table2,
        );
    }
}

/// The catalog linetypes the drawing lacks, offered after its own; a pick
/// loads the one chosen (`ensure_catalog_linetype`).
pub(super) fn catalog_linetype_items(
    document: &codec::Document,
) -> impl Iterator<Item = ui::properties::LinetypeItem> {
    crate::io::linetypes::catalog_missing_from(document)
        .into_iter()
        .map(|(name, description)| ui::properties::LinetypeItem {
            name,
            art: crate::io::linetypes::extract_pattern(&description),
        })
}

#[cfg(test)]
mod layer_dropdown_tests {
    use super::*;
    use crate::app::Message;
    use iced::advanced::widget::{operation::Scrollable, Id, Operation};
    use iced::futures::{executor::block_on, StreamExt};
    use iced::{Rectangle, Task, Vector};
    use iced_test::core::{renderer::Headless, shell, window};
    use iced_test::runtime::user_interface::{Cache, UserInterface};

    #[derive(Default)]
    struct ListProbe {
        bounds: Rectangle,
        offset: Vector,
        content_height: f32,
        active_row: Rectangle,
        labels: Vec<(String, Rectangle)>,
    }

    impl Operation for ListProbe {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }

        fn scrollable(
            &mut self,
            id: Option<&Id>,
            bounds: Rectangle,
            content: Rectangle,
            translation: Vector,
            _: &mut dyn Scrollable,
        ) {
            if id == Some(&Id::new(ui::ribbon::widgets::LAYER_LIST_SCROLL_ID)) {
                self.bounds = bounds;
                self.offset = translation;
                self.content_height = content.height;
            }
        }

        fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
            if id == Some(&Id::new(ui::ribbon::widgets::LAYER_ACTIVE_ROW_ID)) {
                self.active_row = bounds;
            }
        }

        fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
            self.labels.push((text.to_owned(), bounds));
        }
    }

    fn inspect(
        app: &OpenCADStudio,
        renderer: &mut iced::Renderer,
        cache: &mut Cache,
        task: Task<Message>,
        toggle_visibility: Option<&str>,
    ) -> (ListProbe, Vec<Message>) {
        let mut view = UserInterface::build(
            app.ribbon
                .dropdown_overlay(&[], &[], (1024., 768.), false, &[], &[], &[])
                .unwrap(),
            iced::Size::new(1024., 768.),
            std::mem::take(cache),
            renderer,
        );
        if let Some(mut actions) = iced_runtime::task::into_stream(task) {
            while let Some(action) = block_on(actions.next()) {
                if let iced_runtime::Action::Widget(mut operation) = action {
                    view.operate(renderer, operation.as_mut());
                    let _ = operation.finish();
                }
            }
        }
        let mut probe = ListProbe::default();
        view.operate(renderer, &mut probe);
        let mut messages = Vec::new();
        if let Some(name) = toggle_visibility {
            let label = probe
                .labels
                .iter()
                .find(|(text, _)| text == name)
                .unwrap()
                .1;
            let visible_y = label.center_y() - probe.offset.y;
            assert!(visible_y >= probe.bounds.y && visible_y < probe.bounds.y + 26.);
            // The visibility icon follows the checkmark column.
            let cursor =
                iced::mouse::Cursor::Available(iced::Point::new(probe.bounds.x + 36., visible_y));
            let events: Vec<_> = iced_test::simulator::click().collect();
            view.update(
                &window::Headless,
                &shell::Waker::noop(),
                &events,
                cursor,
                renderer,
                &mut messages,
            );
        }
        *cache = view.into_cache();
        (probe, messages)
    }

    #[test]
    fn layer_dropdown_scrolls_to_selection_without_changing_layer_order_or_current_layer() {
        let _one = crate::headless_ui_lock();
        let mut renderer = block_on(iced::Renderer::new(
            iced_test::core::renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .unwrap();
        for id in [ui::ribbon::widgets::LAYER_COMBO_ID, "QAT_LAYER_COMBO"] {
            let mut app = OpenCADStudio::new_for_test();
            app.automation_op(r#"{"op":"new"}"#);
            let i = app.active_tab;
            for n in 0..80 {
                let mut layer = codec::tables::Layer::new(format!("L{n:03}"));
                layer.handle = app.tabs[i].scene.document.allocate_handle();
                app.tabs[i].scene.document.layers.add(layer).unwrap();
            }
            let mut line = codec::entities::Line::new();
            line.common.layer = "L040".into();
            let handles = app.tabs[i]
                .scene
                .add_entities(vec![codec::EntityType::Line(line)]);
            app.tabs[i].scene.select_entities(&handles);
            app.refresh_layer_panel();
            app.sync_ribbon_from_selection();
            let original_order: Vec<_> = app
                .ribbon
                .layer_infos
                .iter()
                .map(|l| l.name.clone())
                .collect();
            let mut cache = Cache::default();

            let task = app.update(Message::ToggleRibbonDropdown(id.into()));
            assert_eq!(app.ribbon.active_layer, "L040");
            let (probe, messages) = inspect(&app, &mut renderer, &mut cache, task, Some("L040"));
            let initial_offset = probe.offset.y;
            assert!(initial_offset > 420.);
            // Iced rounds the scroll translation to whole pixels.
            assert!(
                (probe.active_row.y - probe.offset.y - probe.bounds.y).abs() <= 0.5,
                "row={:?}, offset={:?}, list={:?}",
                probe.active_row,
                probe.offset,
                probe.bounds,
            );
            let drawn_order: Vec<_> = probe
                .labels
                .iter()
                .filter(|(name, _)| original_order.contains(name))
                .map(|(name, _)| name.clone())
                .collect();
            assert_eq!(drawn_order, original_order);
            assert!(
                matches!(messages.as_slice(), [Message::LayerToggleVisible(name)] if name == "L040")
            );
            assert_eq!(app.tabs[i].active_layer, "0");

            // Searching a long list starts at the first match, even after an
            // initial jump far down the unfiltered list.
            let task = app.update(Message::RibbonLayerFilterChanged("L0".into()));
            let (probe, _) = inspect(&app, &mut renderer, &mut cache, task, None);
            assert_eq!(probe.offset.y, 0.);
            let _ = app.update(Message::ToggleRibbonDropdown(id.into()));
            let task = app.update(Message::ToggleRibbonDropdown(id.into()));
            assert!(app.ribbon.layer_filter.is_empty());
            let (probe, _) = inspect(&app, &mut renderer, &mut cache, task, None);
            assert_eq!(probe.offset.y, initial_offset);

            for message in messages {
                let _ = app.update(message);
            }
            assert!(
                app.tabs[i]
                    .scene
                    .document
                    .layers
                    .get("L040")
                    .unwrap()
                    .flags
                    .off
            );
            assert!(app.tabs[i].scene.selected.is_empty());
            assert_eq!(app.tabs[i].active_layer, "0");

            // Without a selection, use the current drawing layer. At the
            // end, ordinary scrolling stops at the bottom of the content.
            app.set_current_layer_name(i, "L079").unwrap();
            let _ = app.update(Message::CloseRibbonDropdown);
            let task = app.update(Message::ToggleRibbonDropdown(id.into()));
            let (probe, _) = inspect(&app, &mut renderer, &mut cache, task, None);
            assert_eq!(
                probe.offset.y,
                (probe.content_height - probe.bounds.height).round()
            );
            app.set_current_layer_name(i, "0").unwrap();
            let _ = app.update(Message::CloseRibbonDropdown);
            let task = app.update(Message::ToggleRibbonDropdown(id.into()));
            let (probe, _) = inspect(&app, &mut renderer, &mut cache, task, None);
            assert_eq!(probe.offset.y, 0.);
        }
    }
}
