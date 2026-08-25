use super::OpenCADStudio;
use crate::ui;

impl OpenCADStudio {
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
        self.tabs[i]
            .layers
            .sync_with_viewports(&doc_layers, vp_info);
        self.sync_ribbon_layers();
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
        let infos: Vec<crate::ui::ribbon::LayerInfo> = self.tabs[i]
            .layers
            .layers
            .iter()
            .map(|l| crate::ui::ribbon::LayerInfo {
                name: l.name.clone(),
                color: crate::ui::window::layers::iced_color_from_acad(&l.color),
                visible: l.visible,
                frozen: l.frozen,
                locked: l.locked,
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

        let text_names: Vec<String> = doc.text_styles.iter().map(|s| s.name.clone()).collect();
        let active_text = doc.header.current_text_style_name.clone();
        let active_text = if text_names.contains(&active_text) {
            active_text
        } else {
            text_names
                .first()
                .cloned()
                .unwrap_or_default()
        };

        let dim_names: Vec<String> = doc.dim_styles.iter().map(|s| s.name.clone()).collect();
        let active_dim = doc.header.current_dimstyle_name.clone();
        let active_dim = if dim_names.contains(&active_dim) {
            active_dim
        } else {
            dim_names
                .first()
                .cloned()
                .unwrap_or_default()
        };

        let mleader_names: Vec<String> = doc
            .objects
            .values()
            .filter_map(|o| {
                if let acadrust::objects::ObjectType::MultiLeaderStyle(mls) = o {
                    Some(mls.name.clone())
                } else {
                    None
                }
            })
            .collect();
        let current_mleader =
            doc.header.current_mleader_style_name.clone();

        let active_mleader =
            mleader_names
                .iter()
                .find(|name| {
                    name.eq_ignore_ascii_case(
                        &current_mleader
                    )
                })
                .cloned()
                .or_else(|| {
                    mleader_names.first().cloned()
                })
                .unwrap_or_default();

        let table_names: Vec<String> = doc
            .objects
            .values()
            .filter_map(|o| {
                if let acadrust::objects::ObjectType::TableStyle(ts) = o {
                    Some(ts.name.clone())
                } else {
                    None
                }
            })
            .collect();
        let active_table = self.ribbon.active_table_style.clone();
        let active_table = if table_names.contains(&active_table) {
            active_table
        } else {
            table_names
                .first()
                .cloned()
                .unwrap_or_default()
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
