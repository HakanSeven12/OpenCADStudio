// Layers, tables, linetypes, layouts, blocks and styles.

use super::validation::{block_nests, validated_layer_name, validated_symbol_name};
use super::*;

impl<'a> HostSession<'a> {
    pub fn add_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> Option<Handle> {
        let trimmed = config.name.trim();
        if trimmed.is_empty() {
            return None;
        }

        let doc = self.document_mut();
        if doc.layers.contains(trimmed) {
            return None;
        }

        let resolved_lt = match config.linetype {
            // A name the drawing does not carry would leave the LAYER record
            // pointing at no LTYPE, so it is refused rather than written.
            Some(ref lt_name) => resolve_linetype(doc, lt_name)?,
            None => "Continuous".to_string(),
        };

        let mut layer = document::tables::Layer::new(trimmed);
        let handle = doc.allocate_handle();
        layer.handle = handle;
        layer.color = config.color.unwrap_or(document::types::Color::Index(7));
        layer.line_type = resolved_lt;
        layer.line_weight = config
            .lineweight
            .unwrap_or(document::types::LineWeight::ByLayer);
        layer.flags.off = config.off.unwrap_or(false);
        if let Some(frz) = config.frozen {
            if frz {
                layer.freeze();
            } else {
                layer.thaw();
            }
        }
        layer.flags.locked = config.locked.unwrap_or(false);
        layer.is_plottable = config.plottable.unwrap_or(true);
        layer.transparency = config
            .transparency
            .unwrap_or(document::types::Transparency::ByLayer);
        layer.description = config.description.unwrap_or_default();

        let _ = doc.layers.add(layer);

        self.app.tabs[self.tab].dirty = true;
        self.app.tabs[self.tab]
            .scene
            .invalidate_layer_dependencies(&[trimmed.to_string()]);
        self.app.refresh_layer_panel();
        self.publish_document_view();
        Some(handle)
    }

    /// Validated, undoable layer-table operations (API v7, additive). Every
    /// refusal happens before the undo step is recorded, so a failure leaves the
    /// drawing and its history untouched.
    pub fn table_operation(
        &mut self,
        operation: ocs_plugin_api::host::TableOperation,
    ) -> Result<Handle, String> {
        use ocs_plugin_api::host::TableOperation;
        let layer_handle = |doc: &Document, name: &str| -> Result<Handle, String> {
            doc.layers
                .get(name)
                .map(|layer| layer.handle)
                .ok_or_else(|| format!("layer {name:?} does not exist"))
        };
        let protected = |name: &str| {
            let name = name.trim();
            name == "0" || name.eq_ignore_ascii_case("Defpoints")
        };
        let same_name = |a: &str, b: &str| a.trim().to_uppercase() == b.trim().to_uppercase();
        let check_color = |color: &document::types::Color| match color {
            document::types::Color::ByLayer
            | document::types::Color::ByBlock
            | document::types::Color::None => {
                Err("a layer color must be an index 1-255 or an RGB value".to_owned())
            }
            document::types::Color::Index(0) => Err("a layer color index must be 1-255".to_owned()),
            _ => Ok(()),
        };
        let check_config = |config: &ocs_plugin_api::host::LayerConfig| -> Result<(), String> {
            if let Some(color) = &config.color {
                check_color(color)?;
            }
            if let Some(document::types::LineWeight::Value(value)) = config.lineweight {
                if !(0..=211).contains(&value) {
                    return Err(
                        "a layer lineweight must be between 0 and 211 (1/100 mm)".to_owned()
                    );
                }
            }
            Ok(())
        };
        match operation {
            TableOperation::LayerCreate { config } => {
                let name = validated_layer_name(&config.name)?;
                if self.document().layers.contains(&name) {
                    return Err(format!("layer {name:?} already exists"));
                }
                check_config(&config)?;
                if let Some(linetype) = &config.linetype {
                    if resolve_linetype(self.document_mut(), linetype).is_none() {
                        return Err(format!(
                            "linetype {linetype:?} does not exist in this drawing"
                        ));
                    }
                }
                self.push_undo("Create layer");
                let config = ocs_plugin_api::host::LayerConfig { name, ..config };
                self.add_layer(config)
                    .ok_or_else(|| "the layer could not be created".to_owned())
            }
            TableOperation::LayerModify { config } => {
                let name = config.name.trim().to_owned();
                let handle = layer_handle(self.document(), &name)?;
                if config.color.is_none()
                    && config.linetype.is_none()
                    && config.lineweight.is_none()
                    && config.off.is_none()
                    && config.frozen.is_none()
                    && config.locked.is_none()
                    && config.plottable.is_none()
                    && config.transparency.is_none()
                    && config.description.is_none()
                {
                    return Err("no layer properties to change".to_owned());
                }
                check_config(&config)?;
                if config.frozen == Some(true)
                    && same_name(&self.document().header.current_layer_name, &name)
                {
                    return Err("the current layer cannot be frozen".to_owned());
                }
                if let Some(linetype) = &config.linetype {
                    if resolve_linetype(self.document_mut(), linetype).is_none() {
                        return Err(format!(
                            "linetype {linetype:?} does not exist in this drawing"
                        ));
                    }
                }
                self.push_undo("Modify layer");
                if self.modify_layer(config) {
                    Ok(handle)
                } else {
                    Err("the layer could not be changed".to_owned())
                }
            }
            TableOperation::LayerRename { from, to } => {
                let handle = layer_handle(self.document(), from.trim())?;
                if protected(&from) {
                    return Err(format!("layer {:?} cannot be renamed", from.trim()));
                }
                let to = validated_layer_name(&to)?;
                if from.trim() == to {
                    return Err("the new layer name is the same as the current one".to_owned());
                }
                if !same_name(&from, &to) && self.document().layers.contains(&to) {
                    return Err(format!("layer {to:?} already exists"));
                }
                self.push_undo("Rename layer");
                if !self.app.tabs[self.tab].rename_layer(from.trim(), &to) {
                    return Err("the layer could not be renamed".to_owned());
                }
                self.finish_layer_change(&[from.trim().to_owned(), to]);
                Ok(handle)
            }
            TableOperation::LayerDelete {
                name,
                erase_objects,
            } => {
                let name = name.trim().to_owned();
                let handle = layer_handle(self.document(), &name)?;
                if protected(&name) {
                    return Err(format!("layer {name:?} cannot be deleted"));
                }
                if same_name(&self.document().header.current_layer_name, &name) {
                    return Err("the current layer cannot be deleted".to_owned());
                }
                if name.contains('|') {
                    return Err("an externally referenced layer cannot be deleted".to_owned());
                }
                let key = name.to_uppercase();
                let on_layer: Vec<Handle> = self
                    .document()
                    .entities()
                    .filter(|entity| entity.common().layer.to_uppercase() == key)
                    .map(|entity| entity.common().handle)
                    .collect();
                if !on_layer.is_empty() && !erase_objects {
                    return Err(format!(
                        "layer {name:?} still holds {} object(s); pass erase_objects=True to erase them with the layer",
                        on_layer.len()
                    ));
                }
                self.push_undo("Delete layer");
                self.document_mut().layers.remove(&name);
                if !on_layer.is_empty() {
                    self.erase_entities_recording_designations(&on_layer);
                }
                self.finish_layer_change(&[name]);
                Ok(handle)
            }
            TableOperation::LayerSetCurrent { name } => {
                let name = name.trim().to_owned();
                let handle = layer_handle(self.document(), &name)?;
                let stored = self
                    .document()
                    .layers
                    .get(&name)
                    .map(|l| l.name.clone())
                    .unwrap_or(name);
                self.app.set_current_layer_name(self.tab, &stored)?;
                Ok(handle)
            }
            other @ (TableOperation::BlockCreate { .. }
            | TableOperation::BlockModify { .. }
            | TableOperation::BlockRename { .. }
            | TableOperation::BlockDelete { .. }
            | TableOperation::BlockEntityAdd { .. }) => self.block_operation(other),
            other @ (TableOperation::LinetypeCreate { .. }
            | TableOperation::LinetypeModify { .. }
            | TableOperation::LinetypeRename { .. }
            | TableOperation::LinetypeDelete { .. }) => self.linetype_operation(other),
            other @ (TableOperation::LayoutCreate { .. }
            | TableOperation::LayoutRename { .. }
            | TableOperation::LayoutDelete { .. }
            | TableOperation::LayoutSetCurrent { .. }
            | TableOperation::LayoutSetPage { .. }
            | TableOperation::LayoutSetTabOrder { .. }) => self.layout_operation(other),
            TableOperation::CustomPropertiesSet { properties } => {
                self.custom_properties_set(properties)
            }
            other => self.style_operation(other),
        }
    }

    /// Drive a real OCS command one step at a time (see
    /// `ocs_plugin_api::host::CommandRequest`). The same primitives the
    /// automation channel uses, run synchronously on this thread, so a step
    /// never waits on anything that has to arrive from outside.
    /// The user's SCRIPTCOMMANDS switch. Every route a script can take to the
    /// command line goes through this, not only `run_command`.
    pub(super) fn script_commands_allowed(&self) -> Result<(), String> {
        if self.app.script_commands {
            Ok(())
        } else {
            Err("running commands from a script is turned off (SCRIPTCOMMANDS = 0); turn it on at the command line with SCRIPTCOMMANDS 1".to_owned())
        }
    }

    /// Whether a script may run this command line.
    ///
    /// The alias is resolved first: the dispatcher resolves it too, so a
    /// denylist that reads the token as typed would let a user-defined alias
    /// (`Q` for `QUIT`, added with ALIASEDIT) walk straight past it.
    pub(super) fn script_command_allowed(&self, line: &str) -> Result<(), String> {
        const DENIED: &[&str] = &[
            "QUIT",
            "EXIT",
            "CLOSE",
            "CLOSEALL",
            "NEW",
            "QNEW",
            "OPEN",
            "SAVE",
            "QSAVE",
            "SAVEAS",
            "SAVEALL",
            "RECOVER",
            "SCRIPT",
            "RUNSCRIPT",
            "SCRIPTCOMMANDS",
        ];
        let Some(token) = line.split_whitespace().next() else {
            return Ok(());
        };
        let token = token.trim_start_matches(['\'', '_']);
        let resolved = self.app.resolve_alias(token);
        let first = resolved.as_deref().unwrap_or(token).to_uppercase();
        let first = first.split_whitespace().next().unwrap_or(&first);
        if DENIED.contains(&first) || first.starts_with("PY_") {
            Err(format!("the command {first} cannot be run from a script"))
        } else {
            Ok(())
        }
    }

    /// Simple linetype create/modify/rename/delete.
    fn linetype_operation(
        &mut self,
        operation: ocs_plugin_api::host::TableOperation,
    ) -> Result<Handle, String> {
        use crate::scene::linetype_service as service;
        use ocs_plugin_api::host::TableOperation;
        let refused = |error: service::LinetypeError| error.to_string();
        // Loading the standard linetypes first means a name that already exists as a
        // standard pattern (Dashed, Center, ...) is seen as taken.
        crate::io::linetypes::populate_document(self.document_mut());
        match operation {
            TableOperation::LinetypeCreate {
                name,
                description,
                pattern,
            } => {
                let linetype = service::plan_create(self.document(), &name, &description, &pattern)
                    .map_err(refused)?;
                self.push_undo("Create linetype");
                let name = linetype.name.clone();
                let handle =
                    service::apply_create(self.document_mut(), linetype).map_err(refused)?;
                self.finish_linetype_change(&[name]);
                Ok(handle)
            }
            TableOperation::LinetypeModify {
                name,
                description,
                pattern,
            } => {
                let linetype = service::plan_modify(
                    self.document(),
                    &name,
                    description.as_deref(),
                    pattern.as_deref(),
                )
                .map_err(refused)?;
                self.push_undo("Modify linetype");
                let name = linetype.name.clone();
                let handle = service::apply_modify(self.document_mut(), linetype);
                self.finish_linetype_change(&[name]);
                Ok(handle)
            }
            TableOperation::LinetypeRename { from, to } => {
                if from.trim() != to.trim() && from.trim().eq_ignore_ascii_case(to.trim()) {
                    return Err("the new linetype name matches the current one (a case-only change is not supported)".to_owned());
                }
                let rename = service::plan_rename(self.document(), &from, &to).map_err(refused)?;
                self.push_undo("Rename linetype");
                let handle =
                    service::apply_rename(self.document_mut(), &rename).map_err(refused)?;
                self.finish_linetype_change(&[rename.old, rename.new]);
                Ok(handle)
            }
            TableOperation::LinetypeDelete { name } => {
                let linetype = service::plan_delete(self.document(), &name).map_err(refused)?;
                self.push_undo("Delete linetype");
                let handle = service::apply_delete(self.document_mut(), &linetype);
                self.finish_linetype_change(&[linetype.name]);
                Ok(handle)
            }
            _ => Err("unsupported table operation".to_owned()),
        }
    }

    fn finish_linetype_change(&mut self, names: &[String]) {
        self.app.tabs[self.tab].dirty = true;
        self.app.tabs[self.tab]
            .scene
            .invalidate_linetype_dependencies(names);
        if self.tab == self.app.active_tab {
            self.app.sync_ribbon_layers();
            self.app.sync_ribbon_from_selection();
        }
        self.publish_document_view();
    }

    /// Paper-space layout operations (create, rename, delete, switch, page).
    /// They act on the active document because switching layouts drives the
    /// application's view state.
    fn layout_operation(
        &mut self,
        operation: ocs_plugin_api::host::TableOperation,
    ) -> Result<Handle, String> {
        use document::objects::ObjectType;
        use ocs_plugin_api::host::TableOperation;
        if self.tab != self.app.active_tab {
            return Err(
                "layout operations act on the active document; switch to it first".to_owned(),
            );
        }
        let find = |doc: &Document, name: &str| -> Result<(String, Handle), String> {
            doc.objects
                .values()
                .find_map(|object| match object {
                    ObjectType::Layout(layout)
                        if !layout.block_record.is_null()
                            && layout.name.eq_ignore_ascii_case(name) =>
                    {
                        Some((layout.name.clone(), layout.handle))
                    }
                    _ => None,
                })
                .ok_or_else(|| format!("layout {name:?} does not exist"))
        };
        let names_taken = |app: &crate::app::OpenCADStudio, tab: usize, name: &str| {
            app.tabs[tab]
                .scene
                .layout_names()
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(name))
        };
        match operation {
            TableOperation::LayoutCreate { name } => {
                let name = validated_symbol_name(&name, "a layout name")?;
                if names_taken(self.app, self.tab, &name) {
                    return Err(format!("layout {name:?} already exists"));
                }
                self.push_undo("Create layout");
                let handle = self
                    .document_mut()
                    .add_layout(&name)
                    .map_err(|e| format!("the layout could not be created: {e}"))?;
                let flags = i16::from(self.document().header.paper_space_linetype_scaling)
                    | (i16::from(self.document().header.paper_space_limit_check) << 1);
                let plot_style = self
                    .app
                    .active_plot_style
                    .as_ref()
                    .map(|style| style.name.clone())
                    .unwrap_or_default();
                for object in self.document_mut().objects.values_mut() {
                    if let ObjectType::Layout(layout) = object {
                        if layout.name == name {
                            layout.flags = flags;
                            crate::scene::apply_default_page_setup(layout, &plot_style);
                            break;
                        }
                    }
                }
                self.app.tabs[self.tab].scene.ensure_sheet_viewport(&name);
                self.finish_layout_change();
                Ok(handle)
            }
            TableOperation::LayoutRename { from, to } => {
                let (from, handle) = find(self.document(), from.trim())?;
                if from.eq_ignore_ascii_case("Model") {
                    return Err("the Model layout cannot be renamed".to_owned());
                }
                let to = validated_symbol_name(&to, "a layout name")?;
                if to.eq_ignore_ascii_case("Model") {
                    return Err("a layout cannot be named Model".to_owned());
                }
                if from.eq_ignore_ascii_case(&to) {
                    return Err("the new layout name matches the current one (a case-only change is not supported)".to_owned());
                }
                if names_taken(self.app, self.tab, &to) {
                    return Err(format!("layout {to:?} already exists"));
                }
                self.push_undo("Rename layout");
                self.app.tabs[self.tab].scene.rename_layout(&from, &to);
                // The layout dictionary is keyed by name; keep it in step.
                let dict = self.document().header.acad_layout_dict_handle;
                if let Some(ObjectType::Dictionary(dictionary)) =
                    self.document_mut().objects.get_mut(&dict)
                {
                    for (key, _) in dictionary.entries.iter_mut() {
                        if *key == from {
                            *key = to.clone();
                        }
                    }
                }
                if self.app.tabs[self.tab].scene.current_layout == from {
                    self.app.tabs[self.tab].scene.set_current_layout(to);
                }
                self.finish_layout_change();
                Ok(handle)
            }
            TableOperation::LayoutDelete { name } => {
                let (name, handle) = find(self.document(), name.trim())?;
                if name.eq_ignore_ascii_case("Model") {
                    return Err("the Model layout cannot be deleted".to_owned());
                }
                self.push_undo("Delete layout");
                if self.app.tabs[self.tab].scene.current_layout == name {
                    let _ = self.app.on_layout_switch("Model".to_owned());
                }
                if !self.app.tabs[self.tab].scene.delete_layout(&name) {
                    return Err("the layout could not be deleted".to_owned());
                }
                self.finish_layout_change();
                Ok(handle)
            }
            TableOperation::LayoutSetTabOrder { names } => {
                let current: Vec<String> = self.app.tabs[self.tab]
                    .scene
                    .layout_names()
                    .into_iter()
                    .filter(|name| !name.eq_ignore_ascii_case("Model"))
                    .collect();
                let mut ordered = Vec::with_capacity(names.len());
                for name in &names {
                    let name = name.trim();
                    let Some(found) = current.iter().find(|n| n.eq_ignore_ascii_case(name)) else {
                        return Err(format!("layout {name:?} is not a paper-space layout"));
                    };
                    if ordered.contains(found) {
                        return Err(format!("layout {found:?} is listed twice"));
                    }
                    ordered.push(found.clone());
                }
                if let Some(missing) = current.iter().find(|n| !ordered.contains(n)) {
                    return Err(format!(
                        "the order must list every paper-space layout; {missing:?} is missing"
                    ));
                }
                if ordered == current {
                    return Ok(Handle::NULL);
                }
                self.push_undo("Layout order");
                self.app.tabs[self.tab].scene.set_layout_tab_order(&ordered);
                self.finish_layout_change();
                Ok(Handle::NULL)
            }
            TableOperation::LayoutSetCurrent { name } => {
                let name = name.trim();
                if name.eq_ignore_ascii_case("Model") {
                    let _ = self.app.on_layout_switch("Model".to_owned());
                    return Ok(Handle::NULL);
                }
                let (name, handle) = find(self.document(), name)?;
                let _ = self.app.on_layout_switch(name);
                self.publish_document_view();
                Ok(handle)
            }
            TableOperation::LayoutSetPage {
                name,
                paper_size,
                rotation,
                scale,
            } => {
                let (name, handle) = find(self.document(), name.trim())?;
                if name.eq_ignore_ascii_case("Model") {
                    return Err("the Model layout has no sheet setup".to_owned());
                }
                if paper_size.is_none() && rotation.is_none() && scale.is_none() {
                    return Err("no page setup properties to change".to_owned());
                }
                if let Some([width, height]) = paper_size {
                    if !(width.is_finite()
                        && height.is_finite()
                        && width > 0.0
                        && height > 0.0
                        && width <= 100_000.0
                        && height <= 100_000.0)
                    {
                        return Err(
                            "the paper size must be positive and at most 100000 mm each way"
                                .to_owned(),
                        );
                    }
                }
                let rotation_code = match rotation {
                    None => None,
                    Some(0) => Some(document::objects::PlotRotation::None),
                    Some(90) => Some(document::objects::PlotRotation::Degrees90),
                    Some(180) => Some(document::objects::PlotRotation::Degrees180),
                    Some(270) => Some(document::objects::PlotRotation::Degrees270),
                    Some(_) => {
                        return Err("the plot rotation must be 0, 90, 180 or 270 degrees".to_owned())
                    }
                };
                if let Some([numerator, denominator]) = scale {
                    if !(numerator.is_finite()
                        && denominator.is_finite()
                        && numerator > 0.0
                        && denominator > 0.0)
                    {
                        return Err(
                            "the plot scale numerator and denominator must be positive".to_owned()
                        );
                    }
                }
                self.push_undo("Layout page setup");
                for object in self.document_mut().objects.values_mut() {
                    let ObjectType::Layout(layout) = object else {
                        continue;
                    };
                    if layout.name != name {
                        continue;
                    }
                    if let Some([width, height]) = paper_size {
                        layout.paper_width = width;
                        layout.paper_height = height;
                        layout.paper_size = String::new();
                    }
                    if let Some(code) = rotation_code {
                        layout.plot_rotation = code.to_code();
                    }
                    if let Some([numerator, denominator]) = scale {
                        layout.plot_scale_numerator = numerator;
                        layout.plot_scale_denominator = denominator;
                        layout.plot_scale_factor = numerator / denominator;
                        layout.plot_scale_type =
                            document::objects::ScaledType::CustomScale.to_code();
                        layout.plot_flags.use_standard_scale = false;
                    }
                    let quarter_turn = layout.plot_rotation
                        == document::objects::PlotRotation::Degrees90.to_code()
                        || layout.plot_rotation
                            == document::objects::PlotRotation::Degrees270.to_code();
                    let (x, y) = if quarter_turn {
                        (layout.paper_height, layout.paper_width)
                    } else {
                        (layout.paper_width, layout.paper_height)
                    };
                    layout.min_limits = (0.0, 0.0);
                    layout.max_limits = (x, y);
                    layout.min_extents = (0.0, 0.0, 0.0);
                    layout.max_extents = (x, y, 0.0);
                    break;
                }
                self.app.tabs[self.tab]
                    .scene
                    .invalidate_display_plot_style();
                self.finish_layout_change();
                Ok(handle)
            }
            _ => Err("unsupported table operation".to_owned()),
        }
    }

    /// `TableOperation::CustomPropertiesSet`: what DWGPROPS OK does for the
    /// Custom tab. Listed names replace a property of the same name (trimmed,
    /// ignoring case) or are appended; the rest are kept.
    fn custom_properties_set(
        &mut self,
        properties: Vec<(String, String)>,
    ) -> Result<Handle, String> {
        crate::app::drawing_properties::check_custom_names(&properties)?;
        let mut list = self.document().summary_info.custom_properties.clone();
        for (name, value) in properties {
            let key = name.trim().to_lowercase();
            match list
                .iter_mut()
                .find(|(n, _)| n.trim().to_lowercase() == key)
            {
                Some(entry) => entry.1 = value,
                None => list.push((name.trim().to_owned(), value)),
            }
        }
        if list == self.document().summary_info.custom_properties {
            return Ok(Handle::NULL);
        }
        self.push_undo("Drawing properties");
        let tab = &mut self.app.tabs[self.tab];
        tab.scene.document.summary_info.custom_properties = list;
        tab.dirty = true;
        // Fields showing a custom property read it again, as after DWGPROPS OK.
        tab.scene.update_summary_field_hosts();
        self.publish_document_view();
        Ok(Handle::NULL)
    }

    fn finish_layout_change(&mut self) {
        self.app.tabs[self.tab].dirty = true;
        // The layout tab bar reads `cached_layout_names`, keyed on this epoch.
        self.app.tabs[self.tab].scene.bump_layout_epoch();
        self.app.tabs[self.tab].scene.bump_geometry();
        self.publish_document_view();
    }

    /// Block definition operations: create from entities, modify settings,
    /// rename (inserts follow) and delete an unreferenced definition.
    fn block_operation(
        &mut self,
        operation: ocs_plugin_api::host::TableOperation,
    ) -> Result<Handle, String> {
        use document::entities::EntityType as E;
        use ocs_plugin_api::host::TableOperation;
        let editable_block = |doc: &Document, name: &str| -> Result<(String, Handle), String> {
            let record = doc
                .block_records
                .get(name)
                .ok_or_else(|| format!("block {name:?} does not exist"))?;
            if record.is_layout()
                || record.is_anonymous()
                || record.flags.is_xref
                || record.flags.is_xref_overlay
                || record.name.contains('|')
            {
                return Err(format!(
                    "block {:?} is a layout, anonymous or externally referenced block",
                    record.name
                ));
            }
            Ok((record.name.clone(), record.handle))
        };
        match operation {
            TableOperation::BlockCreate {
                name,
                entities,
                base_point,
                erase_originals,
                description,
            } => {
                let name = validated_symbol_name(&name, "a block name")?;
                if name.starts_with('*') {
                    return Err("a block name cannot start with '*'".to_owned());
                }
                if self.document().block_records.get(&name).is_some() {
                    return Err(format!("block {name:?} already exists"));
                }
                if base_point.iter().any(|v| !v.is_finite()) {
                    return Err("the block base point must be finite".to_owned());
                }
                if entities.is_empty() {
                    return Err("a block needs at least one entity".to_owned());
                }
                let mut seen = std::collections::HashSet::new();
                let mut sources = Vec::with_capacity(entities.len());
                for handle in &entities {
                    if !seen.insert(handle.value()) {
                        return Err(format!("entity {} is listed twice", handle.value()));
                    }
                    let entity = self
                        .document()
                        .get_entity(*handle)
                        .ok_or_else(|| format!("entity {} does not exist", handle.value()))?;
                    let owner = entity.common().owner_handle;
                    let in_canvas = self
                        .document()
                        .block_records
                        .iter()
                        .any(|record| record.handle == owner && record.is_layout());
                    if !in_canvas {
                        return Err(format!(
                            "entity {} is not in model or paper space",
                            handle.value()
                        ));
                    }
                    if matches!(
                        entity,
                        E::Viewport(_)
                            | E::Block(_)
                            | E::BlockEnd(_)
                            | E::AttributeEntity(_)
                            | E::Unknown(_)
                    ) {
                        return Err(format!(
                            "entity {} of this kind cannot be placed in a block",
                            handle.value()
                        ));
                    }
                    sources.push(entity.clone());
                }
                if let Some(text) = &description {
                    if text.chars().any(|c| c.is_control() && c != '\n') {
                        return Err(
                            "a block description cannot contain control characters".to_owned()
                        );
                    }
                }
                self.push_undo("Create block");
                let base = glam::DVec3::new(base_point[0], base_point[1], base_point[2]);
                self.app.tabs[self.tab]
                    .scene
                    .define_block_from_owned_entities(sources, &name, base)?;
                let (stored, handle) = editable_block(self.document(), &name)?;
                if let Some(record) = self.document_mut().block_records.get_mut(&stored) {
                    record.base_point =
                        document::types::Vector3::new(base_point[0], base_point[1], base_point[2]);
                    if let Some(text) = description {
                        record.description = text;
                    }
                }
                let marker = self
                    .document()
                    .block_records
                    .get(&stored)
                    .map(|r| r.block_entity_handle);
                if let Some(marker) = marker {
                    if let Some(E::Block(block)) = self.document_mut().get_entity_mut(marker) {
                        block.base_point = document::types::Vector3::new(
                            base_point[0],
                            base_point[1],
                            base_point[2],
                        );
                    }
                }
                if erase_originals {
                    self.erase_entities_recording_designations(&entities);
                }
                self.finish_block_change();
                Ok(handle)
            }
            TableOperation::BlockEntityAdd { block, mut entity } => {
                let (name, record_handle) = editable_block(self.document(), block.trim())?;
                if matches!(
                    entity,
                    E::Viewport(_)
                        | E::Block(_)
                        | E::BlockEnd(_)
                        | E::AttributeEntity(_)
                        | E::Unknown(_)
                        | E::RasterImage(_)
                ) {
                    return Err(
                        "an entity of this kind cannot be added to a block from a script"
                            .to_owned(),
                    );
                }
                if self.app.tabs[self.tab].scene.block_edit_block.is_some() {
                    return Err(
                        "finish the open block editor before adding entities to a block".to_owned(),
                    );
                }
                if let E::Insert(insert) = &entity {
                    if insert.block_name.eq_ignore_ascii_case(&name)
                        || block_nests(self.document(), &insert.block_name, &name, 0)
                    {
                        return Err(format!(
                            "inserting {:?} into {name:?} would nest a block inside itself",
                            insert.block_name
                        ));
                    }
                }
                entity.common_mut().owner_handle = record_handle;
                ocs_plugin_api::entity_coverage::validate_new_canvas_entity(&entity)?;
                ocs_plugin_api::entity_coverage::bind_canvas_entity_references(
                    self.document(),
                    &mut entity,
                )?;
                if self
                    .document()
                    .layers
                    .get(&entity.common().layer)
                    .is_some_and(|layer| layer.is_locked())
                {
                    return Err(format!("layer {:?} is locked", entity.common().layer));
                }
                self.push_undo("Add to block");
                entity.common_mut().handle = Handle::NULL;
                entity.common_mut().owner_handle = record_handle;
                // The scene routes an add into the block being edited; reuse that
                // path so the entity gets the same preparation as any new entity.
                self.app.tabs[self.tab].scene.block_edit_block = Some(record_handle);
                let handle = self.add_entity(entity);
                self.app.tabs[self.tab].scene.block_edit_block = None;
                if handle.is_null() {
                    return Err(format!("the entity could not be added to block {name:?}"));
                }
                self.finish_block_change();
                Ok(handle)
            }
            TableOperation::BlockModify {
                name,
                description,
                explodable,
                scale_uniformly,
            } => {
                let (name, handle) = editable_block(self.document(), name.trim())?;
                if description.is_none() && explodable.is_none() && scale_uniformly.is_none() {
                    return Err("no block properties to change".to_owned());
                }
                if let Some(text) = &description {
                    if text.chars().any(|c| c.is_control() && c != '\n') {
                        return Err(
                            "a block description cannot contain control characters".to_owned()
                        );
                    }
                }
                self.push_undo("Modify block");
                if let Some(record) = self.document_mut().block_records.get_mut(&name) {
                    if let Some(text) = description {
                        record.description = text;
                    }
                    if let Some(value) = explodable {
                        record.explodable = value;
                    }
                    if let Some(value) = scale_uniformly {
                        record.scale_uniformly = value;
                    }
                }
                self.finish_block_change();
                Ok(handle)
            }
            TableOperation::BlockRename { from, to } => {
                let from = from.trim().to_owned();
                let to = to.trim().to_owned();
                let open_edits = self.open_block_edits();
                let handle = self
                    .document()
                    .block_records
                    .get(&from)
                    .map(|record| record.handle)
                    .unwrap_or(Handle::NULL);
                self.push_undo(&format!("Rename block {from} to {to}"));
                match crate::scene::block_service::rename(
                    &mut self.app.tabs[self.tab].scene,
                    &from,
                    &to,
                    &open_edits,
                ) {
                    Ok(_) => {
                        self.finish_block_change();
                        Ok(handle)
                    }
                    Err(error) => {
                        self.app.discard_last_undo_entry(self.tab);
                        Err(error.to_string())
                    }
                }
            }
            TableOperation::BlockDelete { name } => {
                let name = name.trim().to_owned();
                // The external API has no force flag, so a referenced block is
                // always refused here — same rule as the palette.
                let open_edits = self.open_block_edits();
                let handle = self
                    .document()
                    .block_records
                    .get(&name)
                    .map(|record| record.handle)
                    .unwrap_or(Handle::NULL);
                self.push_undo(&format!("Delete block {name}"));
                match crate::scene::block_service::delete(
                    &mut self.app.tabs[self.tab].scene,
                    &name,
                    crate::scene::block_service::DeleteOptions {
                        force: false,
                        open_edits: &open_edits,
                    },
                ) {
                    Ok(_) => {
                        self.finish_block_change();
                        Ok(handle)
                    }
                    Err(error) => {
                        self.app.discard_last_undo_entry(self.tab);
                        Err(error.to_string())
                    }
                }
            }
            _ => Err("unsupported table operation".to_owned()),
        }
    }

    /// Block records this document has open in a BEDIT session. Nested
    /// sessions all count, so the whole stack is handed to the block service,
    /// which refuses delete/rename and skips PURGE for every one of them.
    fn open_block_edits(&self) -> Vec<Handle> {
        self.app.tabs[self.tab]
            .block_edits
            .iter()
            .map(|session| session.br_handle)
            .collect()
    }

    fn finish_block_change(&mut self) {
        self.app.tabs[self.tab].dirty = true;
        self.app.tabs[self.tab].scene.invalidate_dependency_index();
        self.app.tabs[self.tab].scene.bump_geometry();
        self.publish_document_view();
    }

    /// Text and dimension style operations. They act on the active document
    /// because the shared style machinery (in-use scan, rename, ribbon sync) is
    /// keyed by the active tab.
    fn style_operation(
        &mut self,
        operation: ocs_plugin_api::host::TableOperation,
    ) -> Result<Handle, String> {
        use crate::app::style_ops::StyleKind;
        use ocs_plugin_api::host::{TableOperation, TableStyleKind};
        if self.tab != self.app.active_tab {
            return Err(
                "style operations act on the active document; switch to it first".to_owned(),
            );
        }
        let kind_of = |kind: TableStyleKind| match kind {
            TableStyleKind::Text => StyleKind::Text,
            TableStyleKind::Dim => StyleKind::Dim,
        };
        let stored = |doc: &Document,
                      kind: TableStyleKind,
                      name: &str|
         -> Result<(String, Handle), String> {
            let found = match kind {
                TableStyleKind::Text => doc
                    .text_styles
                    .get(name)
                    .map(|s| (s.name.clone(), s.handle)),
                TableStyleKind::Dim => doc.dim_styles.get(name).map(|s| (s.name.clone(), s.handle)),
            };
            found.ok_or_else(|| {
                format!(
                    "{} style {name:?} does not exist",
                    match kind {
                        TableStyleKind::Text => "text",
                        TableStyleKind::Dim => "dimension",
                    }
                )
            })
        };
        match operation {
            TableOperation::TextStyleCreate { config } => {
                let name = validated_symbol_name(&config.name, "a text style name")?;
                if self.app.style_exists(StyleKind::Text, &name) {
                    return Err(format!("text style {name:?} already exists"));
                }
                let mut style = document::tables::TextStyle::new(name.clone());
                apply_text_style_config(&mut style, &config)?;
                self.push_undo("Create text style");
                let handle = self.document_mut().allocate_handle();
                style.handle = handle;
                self.document_mut()
                    .text_styles
                    .add(style)
                    .map_err(|e| format!("the text style could not be created: {e}"))?;
                self.finish_style_change(StyleKind::Text);
                Ok(handle)
            }
            TableOperation::TextStyleModify { config } => {
                let (name, handle) =
                    stored(self.document(), TableStyleKind::Text, config.name.trim())?;
                let mut style = self
                    .document()
                    .text_styles
                    .get(&name)
                    .cloned()
                    .expect("checked above");
                let before = style.clone();
                if config
                    == (ocs_plugin_api::host::TextStyleConfig {
                        name: config.name.clone(),
                        ..Default::default()
                    })
                {
                    return Err("no text style properties to change".to_owned());
                }
                apply_text_style_config(&mut style, &config)?;
                if style == before {
                    return Ok(handle);
                }
                self.push_undo("Modify text style");
                if let Some(slot) = self.document_mut().text_styles.get_mut(&name) {
                    *slot = style;
                }
                self.finish_style_change(StyleKind::Text);
                Ok(handle)
            }
            TableOperation::DimStyleCreate {
                name,
                copy_from,
                properties,
            } => {
                let name = validated_symbol_name(&name, "a dimension style name")?;
                if self.app.style_exists(StyleKind::Dim, &name) {
                    return Err(format!("dimension style {name:?} already exists"));
                }
                let base = match &copy_from {
                    Some(source) => {
                        let (source, _) =
                            stored(self.document(), TableStyleKind::Dim, source.trim())?;
                        self.document()
                            .dim_styles
                            .get(&source)
                            .cloned()
                            .expect("checked above")
                    }
                    None => document::tables::DimStyle::new(name.clone()),
                };
                let mut style = apply_dim_style_json(self.document(), &base, &properties)?;
                self.push_undo("Create dimension style");
                let handle = self.document_mut().allocate_handle();
                style.handle = handle;
                style.name = name;
                self.document_mut()
                    .dim_styles
                    .add(style)
                    .map_err(|e| format!("the dimension style could not be created: {e}"))?;
                self.finish_style_change(StyleKind::Dim);
                Ok(handle)
            }
            TableOperation::DimStyleModify { name, properties } => {
                let (name, handle) = stored(self.document(), TableStyleKind::Dim, name.trim())?;
                let base = self
                    .document()
                    .dim_styles
                    .get(&name)
                    .cloned()
                    .expect("checked above");
                let style = apply_dim_style_json(self.document(), &base, &properties)?;
                if style == base {
                    return Ok(handle);
                }
                self.push_undo("Modify dimension style");
                if let Some(slot) = self.document_mut().dim_styles.get_mut(&name) {
                    *slot = style;
                }
                self.finish_style_change(StyleKind::Dim);
                Ok(handle)
            }
            TableOperation::StyleRename { kind, from, to } => {
                let (from, handle) = stored(self.document(), kind, from.trim())?;
                if from.eq_ignore_ascii_case("Standard") {
                    return Err("the Standard style cannot be renamed".to_owned());
                }
                let to = validated_symbol_name(&to, "a style name")?;
                if from.eq_ignore_ascii_case(&to) {
                    return Err("the new style name matches the current one (a case-only change is not supported)".to_owned());
                }
                if self.app.style_exists(kind_of(kind), &to) {
                    return Err(format!("style {to:?} already exists"));
                }
                self.push_undo("Rename style");
                self.app.rename_style_storage(kind_of(kind), &from, &to);
                // dimtxsty follow lives in rename_style_storage (#42).
                self.finish_style_change(kind_of(kind));
                Ok(handle)
            }
            TableOperation::StyleDelete { kind, name } => {
                let (name, handle) = stored(self.document(), kind, name.trim())?;
                if name.eq_ignore_ascii_case("Standard") {
                    return Err("the Standard style cannot be deleted".to_owned());
                }
                if self.app.style_in_use(kind_of(kind), &name) {
                    return Err(format!("style {name:?} is current or still in use"));
                }
                self.push_undo("Delete style");
                if !self.app.remove_style_storage(kind_of(kind), &name) {
                    return Err("the style could not be deleted".to_owned());
                }
                self.finish_style_change(kind_of(kind));
                Ok(handle)
            }
            TableOperation::StyleSetCurrent { kind, name } => {
                let (name, handle) = stored(self.document(), kind, name.trim())?;
                // P1-16 route consistency: the "set current" arm runs the same
                // undo scope + finish path as the rename/delete arms above
                // (it used to bypass both).
                self.push_undo("Set current style");
                let header = &mut self.document_mut().header;
                match kind {
                    TableStyleKind::Text => {
                        header.current_text_style_handle = handle;
                        header.current_text_style_name = name.clone();
                        self.app.ribbon.active_text_style = name;
                    }
                    TableStyleKind::Dim => {
                        header.current_dimstyle_handle = handle;
                        header.current_dimstyle_name = name.clone();
                        self.app.ribbon.active_dim_style = name;
                    }
                }
                self.finish_style_change(kind_of(kind));
                Ok(handle)
            }
            _ => Err("unsupported table operation".to_owned()),
        }
    }

    fn finish_style_change(&mut self, kind: crate::app::style_ops::StyleKind) {
        self.app.tabs[self.tab].dirty = true;
        self.app.after_style_change(kind);
        self.app.tabs[self.tab].scene.bump_geometry();
        self.publish_document_view();
    }

    fn finish_layer_change(&mut self, names: &[String]) {
        self.app.tabs[self.tab].dirty = true;
        self.app.tabs[self.tab]
            .scene
            .invalidate_layer_dependencies(names);
        self.app.refresh_layer_panel();
        self.publish_document_view();
    }

    pub fn modify_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> bool {
        let trimmed = config.name.trim();
        if trimmed.is_empty() {
            return false;
        }

        let resolved_lt = match config.linetype {
            Some(ref lt_name) => match resolve_linetype(self.document_mut(), lt_name) {
                Some(name) => Some(name),
                None => return false,
            },
            None => None,
        };

        if self.document().layers.get(trimmed).is_none() {
            return false;
        }
        // P1-16 route consistency: the direct route opens its own undo scope
        // exactly like the table route, so a plugin-driven layer edit is one
        // undoable step instead of landing outside history. Refusals above
        // never touch the scope.
        let own_scope = self.ensure_xrecord_undo_scope("Modify layer");
        let doc = self.document_mut();
        let Some(existing) = doc.layers.get_mut(trimmed) else {
            // Checked above; an empty scope is closed to keep the contract.
            if own_scope {
                self.app.finish_pending_history(self.tab);
            }
            return false;
        };

        if let Some(c) = config.color {
            existing.color = c;
            existing.color_name = None;
            existing.book_name = None;
        }
        if let Some(lt) = resolved_lt {
            existing.line_type = lt;
        }
        if let Some(lw) = config.lineweight {
            existing.line_weight = lw;
        }
        if let Some(off) = config.off {
            existing.flags.off = off;
        }
        if let Some(frz) = config.frozen {
            if frz {
                existing.freeze();
            } else {
                existing.thaw();
            }
        }
        if let Some(lck) = config.locked {
            existing.flags.locked = lck;
        }
        if let Some(plt) = config.plottable {
            existing.is_plottable = plt;
        }
        if let Some(tr) = config.transparency {
            existing.transparency = tr;
        }
        if let Some(desc) = config.description {
            existing.description = desc;
        }

        self.app.tabs[self.tab].dirty = true;
        self.app.tabs[self.tab]
            .scene
            .invalidate_layer_dependencies(&[trimmed.to_string()]);
        self.app.refresh_layer_panel();
        self.publish_document_view();
        if own_scope {
            self.app.finish_pending_history(self.tab);
        }
        true
    }
}

fn apply_text_style_config(
    style: &mut document::tables::TextStyle,
    config: &ocs_plugin_api::host::TextStyleConfig,
) -> Result<(), String> {
    let text = |what: &str, value: &str| {
        if value.chars().any(|c| c.is_control()) || value.chars().count() > 255 {
            Err(format!("{what} must be text of at most 255 characters"))
        } else {
            Ok(value.trim().to_owned())
        }
    };
    if let Some(height) = config.height {
        if !height.is_finite() || height < 0.0 {
            return Err(
                "text style height must be finite and not negative (0 = variable)".to_owned(),
            );
        }
        style.height = height;
    }
    if let Some(width) = config.width_factor {
        if !width.is_finite() || width <= 0.0 || width > 100.0 {
            return Err(
                "text style width factor must be greater than 0 and at most 100".to_owned(),
            );
        }
        style.width_factor = width;
    }
    if let Some(angle) = config.oblique_angle {
        if !angle.is_finite() || angle.abs() > 85f64.to_radians() + 1e-9 {
            return Err("text style oblique angle must be within 85 degrees either way".to_owned());
        }
        style.oblique_angle = angle;
    }
    if let Some(font) = &config.font_file {
        style.font_file = text("font file", font)?;
    }
    if let Some(font) = &config.big_font_file {
        style.big_font_file = text("big font file", font)?;
    }
    if let Some(font) = &config.true_type_font {
        style.true_type_font = text("TrueType font", font)?;
    }
    if let Some(value) = config.backward {
        style.flags.backward = value;
    }
    if let Some(value) = config.upside_down {
        style.flags.upside_down = value;
    }
    if let Some(value) = config.vertical {
        style.is_vertical = value;
    }
    if let Some(value) = config.annotative {
        style.annotative = value;
    }
    if style.font_file.is_empty() && style.true_type_font.is_empty() {
        return Err("a text style needs a font file or a TrueType font".to_owned());
    }
    Ok(())
}

/// Apply a JSON object of DimStyle fields on top of `base`. Handles and xref
/// fields belong to the host and the name is set separately, so those keys are
/// refused; unknown keys and wrongly typed values are refused too. `dimtxsty`
/// must name an existing text style, whose handle is linked here.
fn apply_dim_style_json(
    doc: &Document,
    base: &document::tables::DimStyle,
    properties: &str,
) -> Result<document::tables::DimStyle, String> {
    let patch: serde_json::Value = serde_json::from_str(properties)
        .map_err(|e| format!("dimension style properties are not valid JSON: {e}"))?;
    let serde_json::Value::Object(patch) = patch else {
        return Err("dimension style properties must be an object".to_owned());
    };
    let mut value = serde_json::to_value(base).map_err(|e| e.to_string())?;
    let object = value
        .as_object_mut()
        .ok_or("dimension style did not serialize to an object")?;
    for (key, new) in patch {
        if key == "handle" || key == "name" || key.starts_with("xref_") || key.ends_with("_handle")
        {
            return Err(format!(
                "dimension style property {key:?} is managed by the host"
            ));
        }
        if !object.contains_key(&key) {
            return Err(format!("unknown dimension style property {key:?}"));
        }
        object.insert(key, new);
    }
    let mut style: document::tables::DimStyle =
        serde_json::from_value(value).map_err(|e| format!("invalid dimension style value: {e}"))?;
    if !(style.dimscale.is_finite() && style.dimscale > 0.0) {
        return Err("dimscale must be greater than zero".to_owned());
    }
    if !(style.dimtxt.is_finite() && style.dimtxt > 0.0) {
        return Err("dimtxt (text height) must be greater than zero".to_owned());
    }
    if !(style.dimasz.is_finite() && style.dimasz >= 0.0) {
        return Err("dimasz (arrow size) cannot be negative".to_owned());
    }
    let Some(text_style) = doc.text_styles.get(&style.dimtxsty) else {
        return Err(format!(
            "dimtxsty {:?} is not a text style in this drawing",
            style.dimtxsty
        ));
    };
    style.dimtxsty = text_style.name.clone();
    style.dimtxsty_handle = text_style.handle;
    Ok(style)
}

/// The stored spelling of `name` in the drawing's linetype table, loading it
/// from the catalog first when the drawing lacks it, as a pick in a linetype
/// list does: that one linetype, whichever the catalog has, and no other.
/// `None` when neither has it: a layer must not reference a linetype that
/// does not exist.
fn resolve_linetype(doc: &mut document::Document, name: &str) -> Option<String> {
    crate::io::linetypes::ensure_catalog_linetype(doc, name);
    doc.line_types.get(name).map(|lt| lt.name.clone())
}
