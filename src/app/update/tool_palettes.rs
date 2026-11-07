//! Update handlers and workflow logic for the dockable Tool Palettes panel
//! (`PanelId::ToolPalettes`, `TOOLPALETTES` / `TP` and `TOOLPALETTESCLOSE`).

use crate::app::{ClipboardDeps, Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::insert::insert_block::InsertBlockCommand;
use crate::ui::window::block_palette::BlockPreview;
use crate::ui::window::tool_palettes::{
    user_block_names, BlockTool, ToolPaletteTab, ToolPalettesMsg,
};
use document::entities::Insert;
use document::types::Vector3;
use document::EntityType;
use iced::Task;
use std::rc::Rc;

impl OpenCADStudio {
    /// Open or close the dockable Tool Palettes panel.
    pub(in crate::app) fn set_tool_palettes_open(&mut self, open: bool) -> Task<Message> {
        let id = crate::ui::dock::PanelId::ToolPalettes;
        if open {
            self.refresh_tool_palettes();
        } else {
            self.tool_palettes.placing = None;
        }
        let task = self.dock_set_open(id, open);
        self.ribbon.show_tool_palettes = self.dock_panel_visible(id);
        self.save_config();
        task
    }

    /// Refresh preview wires and default block selection for the Tool Palettes panel.
    pub(in crate::app) fn refresh_tool_palettes(&mut self) {
        self.tool_palettes.ensure_non_empty();
        let i = self.active_tab;
        if i >= self.tabs.len() {
            return;
        }
        let available = user_block_names(&self.tabs[i].scene.document);
        if self
            .tool_palettes
            .selected_drawing_block
            .as_ref()
            .is_none_or(|cur| !available.iter().any(|b| b == cur))
        {
            self.tool_palettes.selected_drawing_block = available.first().cloned();
        }
        for tab in &mut self.tool_palettes.palettes {
            for tool in &mut tab.tools {
                if self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get(&tool.name)
                    .is_some()
                {
                    let wires = self.tabs[i].scene.block_preview_wires(&tool.name);
                    let preview = BlockPreview::default();
                    preview.fill(wires);
                    tool.preview = Rc::new(preview);
                } else if tool.preview.wires().is_none() {
                    if let Some(deps) = &tool.captured {
                        if let Some(wires) = preview_wires_from_captured(&tool.name, deps) {
                            let preview = BlockPreview::default();
                            preview.fill(wires);
                            tool.preview = Rc::new(preview);
                        }
                    }
                }
            }
        }
    }

    /// Capture a reusable `BlockTool` from block definition `block_name` in tab `tab_idx`.
    pub(in crate::app) fn capture_block_tool(
        &mut self,
        tab_idx: usize,
        block_name: &str,
    ) -> Option<BlockTool> {
        let tab = self.tabs.get_mut(tab_idx)?;
        let br = tab.scene.document.block_records.get(block_name)?;
        if br.flags.is_xref
            || br.is_anonymous()
            || br.name.eq_ignore_ascii_case("*Model_Space")
            || br.name.eq_ignore_ascii_case("*Paper_Space")
        {
            return None;
        }
        let canonical_name = br.name.clone();
        let record_handle = br.handle;
        let description = br.description.clone();
        let dynamic = tab.scene.document.is_dynamic_block(record_handle)
            || crate::scene::block_authoring::export_graph(&tab.scene.document, record_handle)
                .is_some();
        let annotative = crate::scene::annotative::block_is_annotative(&tab.scene.document, &canonical_name);
        let probe = EntityType::Insert(Insert::new(canonical_name.clone(), Vector3::ZERO));
        let captured = ClipboardDeps::capture(&tab.scene.document, &[probe]);
        let wires = tab.scene.block_preview_wires(&canonical_name);
        let preview = BlockPreview::default();
        preview.fill(wires);
        let source = tab
            .current_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        Some(BlockTool {
            name: canonical_name.clone(),
            label: canonical_name,
            description,
            source,
            dynamic,
            annotative,
            scale: 1.0,
            scale_xyz: None,
            rotation_deg: 0.0,
            explode: false,
            captured: Some(captured),
            preview: Rc::new(preview),
        })
    }

    /// Add or update a `BlockTool` in the active palette tab from a block definition in the active drawing.
    pub(in crate::app) fn add_block_to_tool_palette(&mut self, block_name: &str) -> bool {
        let i = self.active_tab;
        let Some(tool) = self.capture_block_tool(i, block_name) else {
            self.command_line.push_error(&crate::tr!(
                "tool-palettes",
                "tool-unavailable",
                name = block_name
            ));
            return false;
        };
        let palette_name = self.tool_palettes.active_tab().name.clone();
        let tool_name = tool.name.clone();
        let active = self.tool_palettes.active_tab_mut();
        let updated = if let Some(existing) = active
            .tools
            .iter_mut()
            .find(|t| t.name.eq_ignore_ascii_case(&tool_name))
        {
            existing.name = tool.name;
            existing.description = tool.description;
            existing.source = tool.source;
            existing.dynamic = tool.dynamic;
            existing.annotative = tool.annotative;
            existing.captured = tool.captured;
            existing.preview = tool.preview;
            true
        } else {
            active.tools.push(tool);
            false
        };
        if updated {
            self.command_line.push_info(&crate::tr!(
                "tool-palettes",
                "updated-tool",
                name = tool_name,
                palette = palette_name
            ));
        } else {
            self.command_line.push_info(&crate::tr!(
                "tool-palettes",
                "added-tool",
                name = tool_name,
                palette = palette_name
            ));
        }
        self.save_config();
        true
    }

    /// Add reusable block tool(s) from the selected `INSERT` entity/entities in the active drawing.
    ///
    /// Resolves evaluated anonymous dynamic block references (`*U…`) back to their
    /// canonical dynamic block definition (`dynamic_definition_for_insert`) and
    /// preserves the reference's uniform scale and rotation on the created tool.
    pub(in crate::app) fn add_selected_blocks_to_tool_palette(&mut self) -> usize {
        let i = self.active_tab;
        let handles = self.tabs[i].scene.selected_handles_in_order();
        let mut specs: Vec<(String, [f64; 3], f64)> = Vec::new();
        for h in handles {
            let Some(EntityType::Insert(ins)) = self.tabs[i].scene.document.get_entity(h) else {
                continue;
            };
            let canonical_name = self.tabs[i]
                .scene
                .document
                .dynamic_definition_for_insert(h)
                .and_then(|def_h| {
                    self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .iter()
                        .find(|br| br.handle == def_h)
                        .map(|br| br.name.clone())
                })
                .unwrap_or_else(|| ins.block_name.clone());
            if canonical_name.starts_with('*') {
                continue;
            }
            let scale = [ins.x_scale(), ins.y_scale(), ins.z_scale()];
            let rotation_deg = ins.rotation.to_degrees();
            if scale.iter().any(|v| !v.is_finite() || v.abs() <= 1e-9) || !rotation_deg.is_finite() {
                continue;
            }
            if !specs
                .iter()
                .any(|(n, _, _)| n.eq_ignore_ascii_case(&canonical_name))
            {
                specs.push((canonical_name, scale, rotation_deg));
            }
        }
        if specs.is_empty() {
            self.command_line
                .push_error(&crate::tr!("tool-palettes", "select-block-first"));
            return 0;
        }
        let mut added = 0;
        for (name, scale, rotation_deg) in specs {
            if self.add_block_to_tool_palette(&name) {
                if let Some(tool) = self
                    .tool_palettes
                    .active_tab_mut()
                    .tools
                    .iter_mut()
                    .find(|t| t.name.eq_ignore_ascii_case(&name))
                {
                    tool.scale = scale[0].abs();
                    tool.scale_xyz = Some(scale);
                    tool.rotation_deg = rotation_deg;
                }
                added += 1;
            }
        }
        if added > 0 {
            self.save_config();
        }
        added
    }

    /// Import a captured block tool's definition, dependencies, and dynamic `BlockGraph`
    /// into tab `i` if not already present (or when `redefine` is true).
    fn ensure_tool_block_in_drawing(&mut self, i: usize, tool: &BlockTool, redefine: bool) -> bool {
        let already_exists = self.tabs[i]
            .scene
            .document
            .block_records
            .get(&tool.name)
            .is_some();
        if already_exists && !redefine {
            return true;
        }
        let Some(deps) = tool.captured.clone() else {
            return already_exists;
        };
        if deps.blocks.is_empty() {
            return already_exists;
        }
        self.merge_dependencies(i, &deps);
        let apps = crate::app::command_driver::clipboard::eed_app_map(
            &self.tabs[i].scene.document,
            &deps,
        );
        let mut created = Vec::new();
        for def in &deps.blocks {
            let scene = &mut self.tabs[i].scene;
            let mut entities = def.entities.clone();
            for entity in &mut entities {
                crate::app::command_driver::clipboard::remap_eed_apps(entity, &apps);
            }
            if redefine && def.name.eq_ignore_ascii_case(&tool.name) {
                if scene.redefine_block_raw(&def.name, def.base_point, entities).is_err() {
                    return false;
                }
            } else if scene.document.block_records.get(&def.name).is_none() {
                if entities.iter().any(|entity| matches!(entity, EntityType::Insert(reference)
                    if crate::scene::block_service::would_create_cycle(&scene.document, &def.name, &reference.block_name))) {
                    return false;
                }
                scene.define_block_raw(&def.name, def.base_point, entities);
                created.push(def);
            }
        }
        use crate::app::command_driver::clipboard::{block_record_map, import_block_def_graph};
        let document = &mut self.tabs[i].scene.document;
        let records = block_record_map(document, &deps.blocks);
        for def in created {
            import_block_def_graph(document, def, &records);
        }
        if tool.annotative {
            if let Some(br_handle) = self.tabs[i]
                .scene
                .document
                .block_records
                .get(&tool.name)
                .map(|br| br.handle)
            {
                crate::scene::annotative::set_entity_annotative(
                    &mut self.tabs[i].scene.document,
                    br_handle,
                    true,
                );
            }
        }
        self.tabs[i].scene.populate_meshes_from_document();
        self.tabs[i].dirty = true;
        self.refresh_block_palette();
        true
    }

    /// Insert the tool at `tool_idx` from the active palette into the active drawing tab.
    pub(in crate::app) fn insert_from_tool_palette(
        &mut self,
        tool_idx: usize,
        drop_at: Option<glam::DVec3>,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            self.command_line
                .push_info(crate::t!("No drawing open — use New or Open first.").as_ref());
            return Task::none();
        }
        let Some(tool) = self
            .tool_palettes
            .active_tab()
            .tools
            .get(tool_idx)
            .cloned()
        else {
            return Task::none();
        };

        // Refuse unsupported redefinition before changing definitions or tables.
        if self.block_redefine_mode == 2 {
            if let Some(record) = self.tabs[i].scene.document.block_records.get(&tool.name) {
                if tool.dynamic || crate::scene::block_authoring::export_graph(&self.tabs[i].scene.document, record.handle).is_some() {
                    self.command_line.push_error(&crate::tr!("tool-palettes", "redefine-dynamic"));
                    return Task::none();
                }
            }
        }
        if let Some(container) = self.tabs[i].scene.block_edit_block.and_then(|handle|
            self.tabs[i].scene.document.block_records.iter().find(|r| r.handle == handle).map(|r| r.name.clone())) {
            let captured_cycle = tool.captured.as_ref().is_some_and(|deps|
                deps.blocks.iter().any(|def| def.name.eq_ignore_ascii_case(&container)));
            if captured_cycle || crate::scene::block_service::would_create_cycle(&self.tabs[i].scene.document, &container, &tool.name) {
                self.command_line.push_error(&crate::scene::block_service::circular_dependency_error(&container, &tool.name));
                return Task::none();
            }
        }

        let needs_import = self.tabs[i]
            .scene
            .document
            .block_records
            .get(&tool.name)
            .is_none()
            || self.block_redefine_mode == 2;

        let before_import = needs_import.then(|| self.tabs[i].scene.document.clone());
        let before_dirty = self.tabs[i].dirty;
        if needs_import && tool.captured.is_some() {
            // Open an undo snapshot before importing the block definition so
            // that the imported definition, dynamic BlockGraph, and placed
            // INSERT reference are grouped into a single undo/redo step.
            self.push_undo_snapshot(i, "INSERT");
        }

        if !self.ensure_tool_block_in_drawing(i, &tool, self.block_redefine_mode == 2) {
            if let Some(before) = before_import {
                self.tabs[i].scene.document = before;
                self.tabs[i].scene.populate_meshes_from_document();
                self.tabs[i].dirty = before_dirty;
            }
            self.drop_unchanged_pending_history(i);
            self.command_line.push_error(&crate::tr!(
                "tool-palettes",
                "tool-unavailable",
                name = tool.name
            ));
            return Task::none();
        }

        let Some(canonical_name) = self.tabs[i]
            .scene
            .document
            .block_records
            .get(&tool.name)
            .map(|br| br.name.clone())
        else {
            if let Some(before) = before_import {
                self.tabs[i].scene.document = before;
                self.tabs[i].scene.populate_meshes_from_document();
                self.tabs[i].dirty = before_dirty;
            }
            self.drop_unchanged_pending_history(i);
            return Task::none();
        };

        if let Some(edit_handle) = self.tabs[i].scene.block_edit_block {
            let container_name = self.tabs[i]
                .scene
                .document
                .block_records
                .iter()
                .find(|br| br.handle == edit_handle)
                .map(|br| br.name.clone());
            if let Some(container) = container_name {
                if crate::scene::block_service::would_create_cycle(
                    &self.tabs[i].scene.document,
                    &container,
                    &canonical_name,
                ) {
                    if let Some(before) = before_import {
                        self.tabs[i].scene.document = before;
                        self.tabs[i].scene.populate_meshes_from_document();
                        self.tabs[i].dirty = before_dirty;
                    }
                    self.drop_unchanged_pending_history(i);
                    self.command_line.push_error(
                        &crate::scene::block_service::circular_dependency_error(
                            &container,
                            &canonical_name,
                        ),
                    );
                    return Task::none();
                }
            }
        }

        let mut opts = self.tool_palettes.options.clone();
        if drop_at.is_some() {
            opts.insertion_point = false;
            opts.scale = false;
            opts.rotation = false;
            opts.repeat = false;
        }
        if !opts.scale {
            let [x, y, z] = tool.scale_xyz.unwrap_or([tool.scale; 3]);
            opts.x = x;
            opts.y = y;
            opts.z = z;
            opts.uniform = x == y && y == z;
        }
        if !opts.rotation {
            opts.angle = tool.rotation_deg;
        }
        if tool.explode {
            opts.explode = true;
        }

        self.reset_command_start_state(i);
        let tab = &mut self.tabs[i];
        let wires = tab.scene.block_preview_wires(&canonical_name);
        let mut cmd = InsertBlockCommand::palette(canonical_name.clone(), wires, &opts);
        if let Some(record) = tab.scene.document.block_records.get(&canonical_name) {
            let doc = &tab.scene.document;
            cmd.set_insertion_points(
                crate::scene::render_graph::block_base_point(doc, &canonical_name),
                crate::scene::block_grips::insertion_points(doc, record.handle),
            );
        }
        if let Some(point) = drop_at {
            cmd.set_point(point);
        }
        let plane = if tab.editing_model_space() {
            tab.ucs_xform().working_plane()
        } else {
            crate::command::WorkingPlane::default()
        };
        cmd.set_working_plane(plane);
        let first = cmd.initial();
        let prompt = cmd.prompt();
        tab.active_cmd = Some(Box::new(cmd));
        self.tool_palettes.placing = Some(canonical_name);
        let task = match first {
            Some(result) => self.apply_cmd_result(result),
            None => {
                self.command_line.push_info(&prompt);
                Task::none()
            }
        };
        if self.tabs[i].active_cmd.is_none() {
            self.tool_palettes.placing = None;
            self.finish_pending_history(i);
        }
        self.refresh_tool_palettes();
        task
    }

    pub(in crate::app) fn on_tool_palettes(&mut self, msg: ToolPalettesMsg) -> Task<Message> {
        match msg {
            ToolPalettesMsg::SelectPalette(idx) => {
                if idx < self.tool_palettes.palettes.len() {
                    self.tool_palettes.active_palette = idx;
                    self.save_config();
                }
                Task::none()
            }
            ToolPalettesMsg::NewPaletteNameChanged(name) => {
                self.tool_palettes.new_palette_name = name;
                Task::none()
            }
            ToolPalettesMsg::NewPalette => {
                let raw = self.tool_palettes.new_palette_name.trim().to_string();
                let name = if raw.is_empty() {
                    let mut n = self.tool_palettes.palettes.len() + 1;
                    loop {
                        let candidate = crate::tr!("tool-palettes", "new-tab-name", number = n);
                        if !self
                            .tool_palettes
                            .palettes
                            .iter()
                            .any(|p| p.name.eq_ignore_ascii_case(&candidate))
                        {
                            break candidate;
                        }
                        n += 1;
                    }
                } else {
                    raw
                };
                self.tool_palettes.new_palette_name.clear();
                self.tool_palettes.palettes.push(ToolPaletteTab {
                    name,
                    tools: Vec::new(),
                });
                self.tool_palettes.active_palette = self.tool_palettes.palettes.len() - 1;
                self.save_config();
                Task::none()
            }
            ToolPalettesMsg::DeletePalette(idx) => {
                if self.tool_palettes.palettes.len() > 1
                    && idx < self.tool_palettes.palettes.len()
                {
                    self.tool_palettes.palettes.remove(idx);
                    if idx < self.tool_palettes.active_palette {
                        self.tool_palettes.active_palette -= 1;
                    }
                    self.tool_palettes.ensure_non_empty();
                    self.save_config();
                }
                Task::none()
            }
            ToolPalettesMsg::SearchChanged(search) => {
                self.tool_palettes.search = search;
                Task::none()
            }
            ToolPalettesMsg::SelectDrawingBlock(name) => {
                self.tool_palettes.selected_drawing_block = Some(name);
                Task::none()
            }
            ToolPalettesMsg::AddDrawingBlock(name) => {
                self.add_block_to_tool_palette(&name);
                Task::none()
            }
            ToolPalettesMsg::AddFromSelection => {
                self.add_selected_blocks_to_tool_palette();
                Task::none()
            }
            ToolPalettesMsg::InsertTool(idx) => self.insert_from_tool_palette(idx, None),
            ToolPalettesMsg::DropTool(idx, pt) => self.insert_from_tool_palette(idx, Some(pt)),
            ToolPalettesMsg::RemoveTool(idx) => {
                let active = self.tool_palettes.active_tab_mut();
                if idx < active.tools.len() {
                    active.tools.remove(idx);
                    self.save_config();
                }
                Task::none()
            }
            ToolPalettesMsg::MoveToolUp(idx) => {
                let active = self.tool_palettes.active_tab_mut();
                if idx > 0 && idx < active.tools.len() {
                    active.tools.swap(idx - 1, idx);
                    self.save_config();
                }
                Task::none()
            }
            ToolPalettesMsg::MoveToolDown(idx) => {
                let active = self.tool_palettes.active_tab_mut();
                if idx + 1 < active.tools.len() {
                    active.tools.swap(idx, idx + 1);
                    self.save_config();
                }
                Task::none()
            }
            ToolPalettesMsg::TogglePromptInsertionPoint(on) => {
                self.tool_palettes.options.insertion_point = on;
                self.save_config();
                Task::none()
            }
            ToolPalettesMsg::ToggleExplode(on) => {
                self.tool_palettes.options.explode = on;
                self.save_config();
                Task::none()
            }
        }
    }
}

/// Build preview wires from a captured `ClipboardDeps` snapshot when the block
/// is not yet loaded in the active drawing tab.
fn preview_wires_from_captured(
    block_name: &str,
    deps: &ClipboardDeps,
) -> Option<Vec<crate::scene::WireModel>> {
    let mut doc = document::Document::new();
    for def in &deps.blocks {
        crate::app::command_driver::clipboard::define_block_raw_doc(
            &mut doc,
            &def.name,
            def.base_point,
            &def.entities,
        );
    }
    let records = crate::app::command_driver::clipboard::block_record_map(&doc, &deps.blocks);
    for def in &deps.blocks {
        crate::app::command_driver::clipboard::import_block_def_graph(&mut doc, def, &records);
    }
    if doc.block_records.get(block_name).is_none() {
        return None;
    }
    let mut scene = crate::scene::Scene::new();
    scene.document = doc;
    Some(scene.block_preview_wires(block_name))
}
