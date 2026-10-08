//! The right-edge underlay and xref tools: their state comes from the
//! selection, their buttons edit it.

use super::*;
use codec::entities::{Underlay, UnderlayDisplayFlags};
use crate::ui::ribbon::UnderlayContext;

impl OpenCADStudio {
    /// The selected underlays when they are all of one kind, or nothing
    /// when anything else (or another kind) is selected.
    fn selected_pdf_underlays(&self, i: usize) -> Vec<(codec::Handle, Underlay)> {
        let mut out: Vec<(codec::Handle, Underlay)> = Vec::new();
        for (handle, entity) in self.tabs[i].scene.selected_entities() {
            match entity {
                codec::EntityType::Underlay(u)
                    if out.first().is_none_or(|(_, first)| first.underlay_type == u.underlay_type) =>
                {
                    out.push((handle, u.clone()));
                }
                _ => return Vec::new(),
            }
        }
        out
    }

    /// The selected xrefs, or nothing when anything else is selected.
    pub(in crate::app) fn selected_xrefs(&self, i: usize) -> Vec<codec::Handle> {
        let doc = &self.tabs[i].scene.document;
        let mut out = Vec::new();
        for (handle, entity) in self.tabs[i].scene.selected_entities() {
            let xref = match entity {
                codec::EntityType::Insert(ins) => doc
                    .block_records
                    .get(&ins.block_name)
                    .is_some_and(|br| br.flags.is_xref || br.flags.is_xref_overlay),
                _ => false,
            };
            if !xref {
                return Vec::new();
            }
            out.push(handle);
        }
        out
    }

    /// The selection context for the right-edge tools: the selected PDF
    /// underlay's switches while only underlays are selected, or that only
    /// xrefs are.
    pub(in crate::app) fn sync_underlay_tab(&mut self) {
        let i = self.active_tab;
        let xref = !self.tabs[i].is_start && !self.selected_xrefs(i).is_empty();
        let context = if self.tabs[i].is_start {
            None
        } else {
            self.selected_pdf_underlays(i).first().map(|(_, u)| {
                UnderlayContext {
                    kind: u.underlay_type,
                    monochrome: u.flags.contains(UnderlayDisplayFlags::MONOCHROME),
                    shown: u.flags.contains(UnderlayDisplayFlags::ON),
                    snap: crate::scene::model::pdf_vector::underlay_osnap(u.underlay_type),
                }
            })
        };
        self.ribbon.set_underlay_context(context, xref);
        let clouds = if self.tabs[i].is_start { Vec::new() } else { self.selected_point_clouds(i) };
        self.ribbon.set_point_cloud_context(clouds.first().map(|(_, data)| data.show_cropping));
    }

    /// The selected point clouds, or nothing when anything else is selected.
    fn selected_point_clouds(&self, i: usize) -> Vec<(codec::Handle, codec::entities::PointCloudExData)> {
        let mut out = Vec::new();
        for (handle, entity) in self.tabs[i].scene.selected_entities() {
            match entity {
                codec::EntityType::Extended(extended) => match &extended.data {
                    codec::entities::ExtendedEntityData::PointCloudEx(data) => out.push((handle, data.clone())),
                    _ => return Vec::new(),
                },
                _ => return Vec::new(),
            }
        }
        out
    }

    /// POINTCLOUDSTYLIZE's result: the clouds that carry the data take the
    /// stylization (one undo step); the rest are named.
    fn stylize_point_clouds(&mut self, i: usize, handles: &[codec::Handle], stylization: i16) {
        let mut done = Vec::new();
        for &handle in handles {
            if self.tabs[i].scene.is_layer_locked(handle) {
                continue;
            }
            let document = &self.tabs[i].scene.document;
            let Some(codec::EntityType::Extended(extended)) = document.get_entity(handle) else {
                continue;
            };
            let codec::entities::ExtendedEntityData::PointCloudEx(data) = &extended.data else {
                continue;
            };
            let cloud = crate::scene::model::point_cloud::resolve_source(document, data)
                .and_then(|path| crate::scene::model::point_cloud::load(&path));
            let supported = match (stylization, &cloud) {
                (6, _) => false,
                (_, None) => true,
                (1, Some(c)) => c.has_rgb,
                (5, Some(c)) => c.has_intensity,
                (3, Some(c)) => c.has_normals,
                _ => true,
            };
            if supported {
                done.push(handle);
            } else {
                self.command_line.push_info(&format!(
                    "The selected point cloud({}) does not support this stylization type.",
                    data.name
                ));
            }
        }
        if !done.is_empty() {
            self.push_undo_snapshot(i, "POINTCLOUDSTYLIZE");
            for handle in &done {
                if let Some(codec::EntityType::Extended(extended)) = self.tabs[i].scene.document.get_entity_mut(*handle) {
                    if let codec::entities::ExtendedEntityData::PointCloudEx(data) = &mut extended.data {
                        crate::entities::extended::set_point_cloud_stylization(data, stylization);
                    }
                }
            }
            let changes: Vec<_> = done.iter().map(|h| (*h, crate::scene::ChangeKind::Modified)).collect();
            self.tabs[i].scene.bump_entities(&changes);
            self.tabs[i].dirty = true;
            self.refresh_properties();
        }
        self.command_line.push_output(&format!("{} point cloud(s) stylized", done.len()));
    }

    /// One undo step that edits every selected point cloud not locked.
    fn edit_selected_point_clouds(
        &mut self,
        i: usize,
        label: &str,
        edit: impl Fn(&mut codec::entities::PointCloudExData),
    ) {
        let handles: Vec<_> = self
            .selected_point_clouds(i)
            .into_iter()
            .map(|(h, _)| h)
            .filter(|h| !self.tabs[i].scene.is_layer_locked(*h))
            .collect();
        if handles.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, label);
        for handle in &handles {
            if let Some(codec::EntityType::Extended(extended)) = self.tabs[i].scene.document.get_entity_mut(*handle) {
                if let codec::entities::ExtendedEntityData::PointCloudEx(data) = &mut extended.data {
                    edit(data);
                }
            }
        }
        let changes: Vec<_> = handles.iter().map(|h| (*h, crate::scene::ChangeKind::Modified)).collect();
        self.tabs[i].scene.bump_entities(&changes);
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }

    /// A Point Cloud Manager click: a switch writes the cloud's hidden scans
    /// and regions as one undo step; the rest is palette state.
    pub(in crate::app) fn update_pc_manager(&mut self, message: crate::ui::window::pc_manager::PcManagerMsg) -> Task<Message> {
        use crate::scene::model::point_cloud::UNASSIGNED_OFF;
        use crate::ui::window::pc_manager::{self, PcManagerMsg, Row};
        let i = self.active_tab;
        match message {
            PcManagerMsg::Toggle(handle, row) => {
                let Some(cloud) =
                    pc_manager::clouds(&self.tabs[i].scene.document, &[]).into_iter().find(|c| c.handle == handle)
                else {
                    return Task::none();
                };
                if self.tabs[i].scene.is_layer_locked(handle) {
                    return Task::none();
                }
                // A scan named by its file name (automation) is the scan with that name.
                let row = match row {
                    Row::Scan(key) => Row::Scan(
                        cloud.scans.iter().find(|(name, _)| *name == key).map_or(key, |(_, id)| id.clone()),
                    ),
                    row => row,
                };
                let hidden = pc_manager::toggled(&cloud.hidden, &cloud.scans, &row);
                self.push_undo_snapshot(i, "POINTCLOUDMANAGER");
                if let Some(codec::EntityType::Extended(extended)) = self.tabs[i].scene.document.get_entity_mut(handle) {
                    if let codec::entities::ExtendedEntityData::PointCloudEx(data) = &mut extended.data {
                        data.hidden_regions = if hidden.iter().any(|h| h == UNASSIGNED_OFF) { vec![0] } else { Vec::new() };
                        data.hidden_scans = hidden.into_iter().filter(|h| h != UNASSIGNED_OFF).collect();
                    }
                }
                self.tabs[i].scene.bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                self.tabs[i].dirty = true;
            }
            PcManagerMsg::Expand(key) => {
                if !self.pc_manager.collapsed.remove(&key) {
                    self.pc_manager.collapsed.insert(key);
                }
            }
            PcManagerMsg::Select(key) => self.pc_manager.selected = Some(key),
            PcManagerMsg::Search(text) => self.pc_manager.search = text,
            PcManagerMsg::CollapseAll => {
                let clouds = pc_manager::clouds(&self.tabs[i].scene.document, &[]);
                self.pc_manager.collapsed.extend(pc_manager::folding_keys(&clouds));
            }
            PcManagerMsg::ExpandAll => self.pc_manager.collapsed.clear(),
        }
        Task::none()
    }

    /// One undo step that edits every selected PDF underlay.
    fn edit_selected_underlays(&mut self, i: usize, label: &str, edit: impl Fn(&mut Underlay)) {
        let handles: Vec<_> = self
            .selected_pdf_underlays(i)
            .into_iter()
            .map(|(h, _)| h)
            .filter(|h| !self.tabs[i].scene.is_layer_locked(*h))
            .collect();
        if handles.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, label);
        for handle in &handles {
            if let Some(codec::EntityType::Underlay(u)) =
                self.tabs[i].scene.document.get_entity_mut(*handle)
            {
                edit(u);
            }
            self.tabs[i].scene.reseed_derived_caches(*handle);
        }
        let changes: Vec<_> = handles
            .iter()
            .map(|h| (*h, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }

    /// The right-edge tools (host-only command names).
    pub(super) fn dispatch_pdf_underlay(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            "_PDFULMONO" => {
                let on = !self
                    .selected_pdf_underlays(i)
                    .first()
                    .is_some_and(|(_, u)| u.flags.contains(UnderlayDisplayFlags::MONOCHROME));
                self.edit_selected_underlays(i, "PDFADJUST", |u| u.set_monochrome(on));
            }
            "_PDFULSHOW" => {
                let on = !self
                    .selected_pdf_underlays(i)
                    .first()
                    .is_some_and(|(_, u)| u.flags.contains(UnderlayDisplayFlags::ON));
                self.edit_selected_underlays(i, "PDFUNDERLAY", |u| u.set_on(on));
            }
            // Enable Snap switches PDFOSNAP, DWFOSNAP or DGNOSNAP.
            "_PDFULSNAP" => {
                let Some((_, underlay)) = self.selected_pdf_underlays(i).into_iter().next() else {
                    return Some(Task::none());
                };
                let kind = underlay.underlay_type;
                let on = !crate::scene::model::pdf_vector::underlay_osnap(kind);
                crate::scene::model::pdf_vector::set_underlay_osnap(kind, on);
                self.tabs[i].scene.reseed_underlays();
                self.sync_underlay_tab();
            }
            // External Reference tab: the selected xrefs.
            "_XREFEDIT" | "_XREFOPEN" => {
                let Some(&handle) = self.selected_xrefs(i).first() else {
                    return Some(Task::none());
                };
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.select_entity(handle, true);
                let command = if cmd == "_XREFEDIT" { "REFEDIT" } else { "XOPEN" };
                return Some(self.dispatch_command(command));
            }
            "_XREFCLIP" => {
                use crate::command::CadCommand;
                let inserts = self.selected_xrefs(i);
                if inserts.is_empty() {
                    return Some(Task::none());
                }
                let clipped = inserts.iter().any(|h| {
                    crate::scene::pick::xclip::filter_handle(&self.tabs[i].scene.document, *h).is_some()
                });
                let (command, first) =
                    crate::modules::insert::xclip::XclipCommand::start_new_boundary(inserts, clipped);
                if let crate::command::CmdResult::ReportMeasurement(text) = first {
                    for line in text.lines() {
                        self.command_line.push_output(line);
                    }
                }
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            "_XREFUNCLIP" => {
                let inserts = self.selected_xrefs(i);
                if !inserts.is_empty() {
                    self.apply_xclip(i, inserts, crate::modules::insert::xclip::XclipAction::Delete);
                }
            }
            // Point cloud tools: a crop of the first selected cloud, or the
            // crops of every selected one.
            "_PCCROPRECT" | "_PCCROPPOLY" | "_PCCROPCIRC" => {
                use crate::command::CadCommand;
                let Some(&(handle, _)) = self.selected_point_clouds(i).first() else {
                    return Some(Task::none());
                };
                if self.tabs[i].scene.is_layer_locked(handle) {
                    return Some(Task::none());
                }
                let Some(codec::EntityType::Extended(cloud)) = self.tabs[i].scene.document.get_entity(handle).cloned() else {
                    return Some(Task::none());
                };
                let option = match cmd {
                    "_PCCROPPOLY" => "P",
                    "_PCCROPCIRC" => "C",
                    _ => "",
                };
                let command = crate::modules::insert::pc_crop::PointCloudCropCommand::for_cloud(handle, *cloud, option);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            "POINTCLOUDSTYLIZE" => {
                use crate::command::CadCommand;
                let command = crate::modules::draw::select::SelectObjectsCommand::with_prompt(
                    "POINTCLOUDSTYLIZE",
                    "_PCSTYLIZE",
                    "Select point cloud objects:",
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            // The chosen objects' point clouds, then the option.
            "_PCSTYLIZE" => {
                use crate::command::CadCommand;
                let handles: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .filter(|(_, entity)| crate::scene::is_point_cloud(entity))
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    return Some(self.finish_dispatch(cmd));
                }
                let command = crate::modules::insert::pc_stylize::PointCloudStylizeCommand::new(handles);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            c if c.starts_with("_PCSTYLIZEAPPLY ") => {
                let mut parts = c.split_whitespace().skip(1);
                let stylization: i16 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                let handles: Vec<codec::Handle> =
                    parts.filter_map(|h| u64::from_str_radix(h, 16).ok()).map(codec::Handle::new).collect();
                self.stylize_point_clouds(i, &handles, stylization);
            }
            "_PCCROPSHOW" => {
                let on = !self.selected_point_clouds(i).first().is_some_and(|(_, data)| data.show_cropping);
                self.edit_selected_point_clouds(i, "POINTCLOUDCROP", |data| data.show_cropping = on);
            }
            "_PCCROPINVERT" => self.edit_selected_point_clouds(i, "POINTCLOUDCROP", |data| {
                for crop in &mut data.croppings {
                    crop.inverted = !crop.inverted;
                }
            }),
            // The Point Cloud Manager docks on the right like External
            // References and opens expanded.
            "POINTCLOUDMANAGER" => {
                let id = crate::ui::dock::PanelId::PointCloudManager;
                self.pc_manager.show = true;
                if self.dock.location(id).is_none() {
                    self.dock.dock(id, crate::app::config::DockSide::Right, usize::MAX);
                }
                self.dock_expanded = Some(id);
            }
            "POINTCLOUDMANAGERCLOSE" => {
                self.pc_manager.show = false;
                if self.dock_expanded == Some(crate::ui::dock::PanelId::PointCloudManager) {
                    self.dock_expanded = None;
                }
            }
            "_PCUNCROP" =>self.edit_selected_point_clouds(i, "POINTCLOUDUNCROP", |data| data.croppings.clear()),
            "_PDFULUNCLIP" => {
                self.edit_selected_underlays(i, "PDFCLIP", |u| {
                    u.clip_boundary_vertices.clear();
                    u.clip_inverted = false;
                    u.flags -= UnderlayDisplayFlags::CLIPPING;
                });
            }
            "_PDFULCLIP" => {
                use crate::command::CadCommand;
                let Some((handle, underlay)) = self.selected_pdf_underlays(i).into_iter().next()
                else {
                    return Some(Task::none());
                };
                let (command, first) =
                    crate::modules::insert::pdf_clip::PdfClipCommand::new_boundary(handle, underlay);
                if let crate::command::CmdResult::ReportMeasurement(text) = first {
                    for line in text.lines() {
                        self.command_line.push_output(line);
                    }
                }
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            "_PDFULIMPORT" => {
                use crate::command::CadCommand;
                let Some((handle, _)) = self.selected_pdf_underlays(i).into_iter().next() else {
                    return Some(Task::none());
                };
                let command = crate::modules::insert::pdf_import::PdfImportCommand::for_underlay(handle);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }
}
