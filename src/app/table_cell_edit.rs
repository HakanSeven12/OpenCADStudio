//! Table editing owns an MText draft and a read-only picture preview.
use super::{Message, OpenCADStudio};
use crate::command::CadCommand;
use document::{EntityType, Handle};
use glam::DVec3;
use iced::Task;

pub(crate) struct TableCellEditor {
    pub tab_id: u64,
    pub handle: Handle,
    pub row: usize,
    pub column: usize,
    pub original: String,
    pub pending_move: Option<(isize, isize, bool)>,
    pub picture: crate::entities::table::CellPicture,
}

/// Affine screen frame shared by text drawing, caret placement and picking.
#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct CellCanvas {
    pub origin: iced::Point,
    pub x: iced::Vector,
    pub y: iced::Vector,
    pub corners: [iced::Point; 4],
    pub height: f32,
}

impl CellCanvas {
    pub fn project(&self, x: f32, y: f32) -> iced::Point {
        self.origin + self.x * x + self.y * y
    }

    pub fn unproject(&self, p: iced::Point) -> (f32, f32) {
        let p = p - self.origin;
        let det = self.x.x * self.y.y - self.x.y * self.y.x;
        (
            (p.x * self.y.y - p.y * self.y.x) / det,
            (self.x.x * p.y - self.x.y * p.x) / det,
        )
    }

    pub fn contains(&self, p: iced::Point) -> bool {
        let mut sign = 0.0_f32;
        for n in 0..4 {
            let a = self.corners[n];
            let b = self.corners[(n + 1) % 4];
            let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
            if cross.abs() > 0.01 {
                if sign != 0.0 && cross.signum() != sign {
                    return false;
                }
                sign = cross.signum();
            }
        }
        true
    }
}

impl OpenCADStudio {
    pub(in crate::app) fn open_table_cell_editor(
        &mut self,
        handle: Handle,
        row: usize,
        column: usize,
    ) -> bool {
        if self.table_cell_editor.is_some() && !self.commit_table_cell_editor() {
            return false;
        }
        let i = self.active_tab;
        if self.tabs[i].scene.is_layer_locked(handle) {
            return false;
        }
        let scene = &self.tabs[i].scene;
        let Some(EntityType::Table(table)) = scene.document.get_entity(handle) else {
            return false;
        };
        let (row, column, _, _) =
            crate::entities::table::widen_to_merged(table, (row, column, row, column));
        let Some(cell) = table.cell(row, column) else {
            return false;
        };
        use document::entities::table::CellStateFlags as F;
        if cell.state.intersects(F::CONTENT_LOCKED | F::CONTENT_READ_ONLY) {
            return false;
        }
        let Some(picture) =
            crate::entities::table::cell_picture(&scene.document, table, row, column)
        else {
            return false;
        };
        let initial = picture.text.value.clone();
        let p = picture.text.insertion_point;
        let pos = DVec3::new(p.x, p.y, p.z);
        let template = picture.text.clone();
        self.tabs[i].active_cmd = None;
        self.command_line.input.clear();
        self.open_mtext_editor(pos, None, &initial, template.height, Some(template));
        // The table is an existing field host; committing is routed below.
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.editing = Some(handle);
        }
        self.mtext_table_cell_fields(handle, row, column);
        self.table_cell_editor = Some(TableCellEditor {
            tab_id: self.tabs[i].id,
            handle,
            row,
            column,
            original: initial,
            pending_move: None,
            picture,
        });
        let scene = &mut self.tabs[i].scene;
        scene.select_table_cells(crate::scene::TableCellSelection {
            table: handle,
            anchor: (row, column),
            end: (row, column),
        });
        self.tabs[i].snap_result = None;
        self.snap_dwell = None;
        self.refresh_table_cell_picture();
        true
    }

    pub(in crate::app) fn refresh_table_cell_picture(&mut self) {
        let Some(cell) = self.table_cell_editor.as_ref() else { return; };
        let i = self.active_tab;
        let scene = &mut self.tabs[i].scene;
        let Some(entity) = scene.document.get_entity(cell.handle) else { return; };
        let preview = if let Some(name) = cell.picture.block_name.as_deref() {
            let excluded = cell.picture.members.iter().copied().collect();
            let cache = crate::scene::cache::block_cache::BlockCache::build_for_block_excluding(
                &scene.document, name, 1.0, None, true, scene.bg_color, None,
                &Default::default(), &excluded,
            );
            crate::scene::tessellate_entity(&scene.document, &Default::default(), None,
                scene.bg_color, 1.0, None, entity, Some(&cache), None, None, None, &Default::default())
        } else if let EntityType::Table(table) = entity {
            let mut table = (**table).clone();
            if let Some(cell) = table.cell_mut(cell.row, cell.column) {
                cell.contents.clear();
            }
            crate::entities::table::tessellate_table(&table, &scene.document, false,
                [0.92, 0.92, 0.92, 1.0], 1.0, 1.0)
        } else { Vec::new() };
        scene.set_command_preview_hidden(&[cell.handle]);
        scene.set_preview_wires(preview);
    }

    pub(in crate::app) fn close_table_cell_editor(&mut self) {
        let Some(cell) = self.table_cell_editor.take() else { return; };
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == cell.tab_id) {
            tab.scene.set_command_preview_hidden(&[]);
            tab.scene.set_preview_wires(Vec::new());
            tab.snap_result = None;
        }
        self.mtext_editor = None;
    }

    pub(in crate::app) fn commit_table_cell_editor(&mut self) -> bool {
        let (Some(cell), Some(ed)) = (self.table_cell_editor.as_ref(), self.mtext_editor.as_ref())
        else {
            return false;
        };
        let Some(i) = self.tabs.iter().position(|t| t.id == cell.tab_id) else {
            self.close_table_cell_editor();
            return true;
        };
        let (handle, row, column) = (cell.handle, cell.row, cell.column);
        if ed.draft_error().is_some() || self.tabs[i].scene.is_layer_locked(handle) {
            return false;
        }
        let value = ed.build_mtext().value;
        let fields = ed.field_host.clone();
        if value == cell.original && !fields.as_ref().is_some_and(|f| f.has_changes()) {
            self.close_table_cell_editor();
            return true;
        }
        let Some(EntityType::Table(table)) = self.tabs[i].scene.document.get_entity(handle) else {
            self.close_table_cell_editor();
            return true;
        };
        if !self.table_cell_entry_typed
            && fields.is_none()
            && !crate::entities::table::typed_text_fits(
                &self.tabs[i].scene.document, table, row, column, &value,
            )
        {
            self.ask_table_editor_value_type(handle, row, column, value);
            return false;
        }
        let mut command = crate::modules::annotate::table_cmd::TableCellEditCommand::new(
            handle, table, row, column,
        );
        if std::mem::take(&mut self.table_cell_entry_typed) {
            let (data_type, unit_type, picture) = crate::entities::table::entered_value_type(&value);
            command = command.with_value_type(data_type, unit_type, &picture);
        }
        let Some(crate::command::CmdResult::ReplaceMany(mut replacements, _)) =
            command.on_text_input(&value)
        else {
            return false;
        };
        let Some((_, mut entities)) = replacements.pop() else {
            return false;
        };
        let Some(entity) = entities.pop() else {
            return false;
        };
        self.push_undo_snapshot(i, "TABLE CELL");
        self.close_table_cell_editor();
        if let Some(fields) = fields {
            if let Some(target) = self.tabs[i].scene.document.get_entity_mut(handle) {
                *target = entity;
            }
            fields.save_table_cell(&mut self.tabs[i].scene.document, handle, row, column, &value);
            if let Some(EntityType::Table(table)) = self.tabs[i].scene.document.get_entity_mut(handle) {
                table.block_record_handle = None;
            }
            self.tabs[i].scene.bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
        } else {
            self.tabs[i].scene.update_entity(entity);
        }
        self.tabs[i].dirty = true;
        self.finish_pending_history(i);
        self.refresh_properties();
        true
    }

    pub(in crate::app) fn table_editor_step(
        &mut self,
        dr: isize,
        dc: isize,
        tab: bool,
    ) -> Task<Message> {
        let Some(cell) = self.table_cell_editor.as_ref() else {
            return Task::none();
        };
        let (handle, row, column) = (cell.handle, cell.row, cell.column);
        self.table_cell_editor.as_mut().unwrap().pending_move = Some((dr, dc, tab));
        if !self.commit_table_cell_editor() {
            return Task::none();
        }
        let i = self.active_tab;
        let Some(EntityType::Table(table)) = self.tabs[i].scene.document.get_entity(handle) else {
            return Task::none();
        };
        let columns = table.column_count();
        let rows = table.row_count();
        let span = crate::entities::table::widen_to_merged(table, (row, column, row, column));
        let mut next = if tab {
            (row * columns + column)
                .checked_add_signed(dc)
                .map(|n| (n / columns, n % columns))
        } else {
            Some((
                if dr > 0 { span.2 + 1 } else { row.saturating_add_signed(dr) },
                if dc > 0 { span.3 + 1 } else { column.saturating_add_signed(dc) },
            ))
        };
        // Linear navigation skips merged followers and content-locked cells.
        if tab {
            while let Some((r, c)) = next.filter(|(r, _)| *r < rows) {
                let owner = crate::entities::table::widen_to_merged(table, (r, c, r, c));
                use document::entities::table::CellStateFlags as F;
                if (owner.0, owner.1) == (r, c)
                    && table.cell(r, c).is_some_and(|cell| {
                        !cell.state.intersects(F::CONTENT_LOCKED | F::CONTENT_READ_ONLY)
                    })
                {
                    break;
                }
                next = (r * columns + c)
                    .checked_add_signed(dc)
                    .map(|n| (n / columns, n % columns));
            }
        }
        if tab && dc < 0 && next.is_none() {
            next = Some((row, column));
        }
        if tab && dc > 0 && next.is_some_and(|(r, _)| r >= rows) {
            let mut extended = (**table).clone();
            let mut added = extended.rows.last().unwrap().clone();
            for cell in &mut added.cells {
                cell.contents.clear();
            }
            extended.rows.push(added);
            extended.block_record_handle = None;
            self.push_undo_snapshot(i, "TABLE INSERT ROW");
            self.tabs[i].scene.update_entity(EntityType::Table(Box::new(extended)));
            self.finish_pending_history(i);
            self.tabs[i].dirty = true;
            self.open_table_cell_editor(handle, rows, 0);
            return self.unfocus_widgets();
        }
        if let Some((r, c)) = next.filter(|(r, c)| *r < rows && *c < columns) {
            self.open_table_cell_editor(handle, r, c);
        }
        self.unfocus_widgets()
    }

    pub(in crate::app) fn table_cell_canvas(&self) -> Option<CellCanvas> {
        let cell = self.table_cell_editor.as_ref()?;
        let ed = self.mtext_editor.as_ref()?;
        let tab = &self.tabs[self.active_tab];
        if tab.id != cell.tab_id || tab.active_cmd.is_some() {
            return None;
        }
        let size = tab.scene.selection.borrow().view.vp_size;
        let (camera, bounds) = tab.scene.viewport_edit_frame(size).unwrap_or_else(||
            (tab.scene.camera.borrow().clone(), tab.scene.active_model_tile_bounds(size.0, size.1)));
        let project = |p| camera.project(p, bounds).map(|p| iced::Point::new(p.x + bounds.x, p.y + bounds.y));
        let world = |x, y| {
            let p = cell.picture.text.insertion_point;
            let p = cell.picture.transform.apply(document::types::Vector3::new(p.x + x, p.y + y, p.z));
            DVec3::new(p.x, p.y, p.z)
        };
        let origin = project(world(0.0, 0.0))?;
        let x = project(world(1.0, 0.0))? - origin;
        let y = project(world(0.0, 1.0))? - origin;
        if (x.x * y.y - x.y * y.x).abs() < 1e-8 {
            return None;
        }
        let corners = cell.picture.corners.map(project);
        Some(CellCanvas { origin, x, y, height: ed.height_value() as f32,
            corners: [corners[0]?, corners[1]?, corners[2]?, corners[3]?] })
    }

    pub(in crate::app) fn table_editor_click(
        &mut self,
        point: iced::Point,
    ) -> Task<Message> {
        let Some(cell) = self.table_cell_editor.as_ref() else {
            return Task::none();
        };
        let handle = cell.handle;
        let i = self.active_tab;
        let scene = &self.tabs[i].scene;
        let size = scene.selection.borrow().view.vp_size;
        let (camera, bounds) = scene.viewport_edit_frame(size).unwrap_or_else(||
            (scene.camera.borrow().clone(), scene.active_model_tile_bounds(size.0, size.1)));
        let world = camera.pick_on_target_plane(point, bounds);
        let next = match scene.document.get_entity(handle) {
            Some(EntityType::Table(table)) => {
                let style = table.table_style_handle.and_then(|h| match scene.document.objects.get(&h) {
                    Some(document::objects::ObjectType::TableStyle(s)) => Some(s), _ => None,
                });
                crate::modules::annotate::table_cmd::table_cell_at(table, style, world)
            }
            _ => None,
        };
        if self.commit_table_cell_editor() {
            if let Some(hit) = next.filter(|hit| !hit.locked) {
                self.open_table_cell_editor(handle, hit.row, hit.column);
                return self.unfocus_widgets();
            }
        }
        Task::none()
    }
}
