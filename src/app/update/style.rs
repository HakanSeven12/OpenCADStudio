//! `style` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{parse_coord, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis, CoordKind};
use crate::app::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
use crate::scene::model::object::GripApply;
use crate::scene::pick::grip::{find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit};
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::window::graphic_attributes::GraphicAttributesMsg;
use crate::ui::PropertiesPanel;
use document::types::Color as AcadColor;
use document::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};

impl OpenCADStudio {
    pub(in crate::app) fn mlstyle_mut(
        &mut self,
        tab: usize,
    ) -> Option<&mut document::objects::MLineStyle> {
        use document::objects::ObjectType;
        let name = self.mlstyle_selected.clone();
        self.tabs[tab]
            .scene
            .document
            .objects
            .values_mut()
            .find_map(|object| match object {
                ObjectType::MLineStyle(style) if style.name == name => Some(style),
                _ => None,
            })
    }

    pub(in crate::app) fn load_mlstyle_bufs(&mut self, tab: usize) {
        use document::objects::ObjectType;
        let name = self.mlstyle_selected.clone();
        let Some((description, start_angle, end_angle, fill_color, elements)) = self.tabs[tab]
            .scene
            .document
            .objects
            .values()
            .find_map(|object| match object {
                ObjectType::MLineStyle(style) if style.name == name => Some((
                    style.description.clone(),
                    style.start_angle.to_degrees(),
                    style.end_angle.to_degrees(),
                    style.fill_color.index().unwrap_or(256),
                    style
                        .elements
                        .iter()
                        .map(|element| {
                            [
                                format!("{}", element.offset),
                                element.color.index().unwrap_or(256).to_string(),
                                element.linetype.clone(),
                            ]
                        })
                        .collect::<Vec<_>>(),
                )),
                _ => None,
            })
        else {
            return;
        };
        self.mln_description = description;
        self.mln_start_angle = format!("{start_angle:.4}");
        self.mln_end_angle = format!("{end_angle:.4}");
        self.mln_fill_color = fill_color.to_string();
        self.mln_elements = elements;
    }

    pub(in crate::app) fn stage_mlstyle_bufs(&mut self) {
        let tab = self.active_tab;
        let description = self.mln_description.clone();
        let start_angle = self.mln_start_angle.trim().parse::<f64>().ok();
        let end_angle = self.mln_end_angle.trim().parse::<f64>().ok();
        let fill_color = self.mln_fill_color.trim().parse::<i16>().ok();
        let elements = self.mln_elements.clone();
        if let Some(style) = self.mlstyle_mut(tab) {
            style.description = description;
            if let Some(value) = start_angle {
                style.start_angle = value.to_radians();
            }
            if let Some(value) = end_angle {
                style.end_angle = value.to_radians();
            }
            if let Some(value) = fill_color {
                style.fill_color = document::types::Color::from_index(value);
            }
            for (element, values) in style.elements.iter_mut().zip(elements) {
                if let Ok(value) = values[0].trim().parse::<f64>() {
                    element.offset = value;
                }
                if let Ok(value) = values[1].trim().parse::<i16>() {
                    element.color = document::types::Color::from_index(value);
                }
                element.linetype = values[2].clone();
            }
        }
    }

    pub(in crate::app) fn tablestyle_mut(
        &mut self,
        tab: usize,
    ) -> Option<&mut document::objects::TableStyle> {
        use document::objects::ObjectType;
        let name = self.tablestyle_selected.clone();
        self.tabs[tab]
            .scene
            .document
            .objects
            .values_mut()
            .find_map(|o| match o {
                ObjectType::TableStyle(s) if s.name == name => Some(s),
                _ => None,
            })
    }

    /// Gives the selected table style its named cell styles (a style made
    /// here or read from an older file only has its three rows) and fills
    /// the editor buffers from the edited cell style.
    pub(in crate::app) fn load_tablestyle_bufs(&mut self, tab: usize) {
        let text_styles: Vec<(String, document::Handle)> = self.tabs[tab]
            .scene
            .document
            .text_styles
            .iter()
            .map(|style| (style.name.clone(), style.handle))
            .collect();
        let wanted = self.ts_cell.clone();
        let Some(style) = self.tablestyle_mut(tab) else {
            return;
        };
        style.ensure_cell_styles(|row| {
            row.text_style_handle
                .or_else(|| {
                    text_styles
                        .iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case(&row.text_style_name))
                        .or_else(|| text_styles.iter().find(|(name, _)| name == "Standard"))
                        .map(|(_, handle)| *handle)
                })
                .unwrap_or(document::Handle::NULL)
        });
        let description = style.description.clone();
        let margins = (style.horizontal_margin, style.vertical_margin);
        let cell = style
            .cell_style(&wanted)
            .or_else(|| style.cell_style(document::objects::DATA_CELL_STYLE))
            .cloned();
        self.ts_description = description;
        let Some(cell) = cell else {
            return;
        };
        let data = &cell.cell_style;
        let format = &data.content_format;
        self.ts_cell = cell.name.clone();
        self.ts_cell_name.clear();
        self.ts_cell_height = crate::entities::common::format_length(format.text_height);
        self.ts_cell_angle = crate::entities::common::format_length(format.rotation.to_degrees());
        let (horizontal, vertical) = data.margins().unwrap_or(margins);
        self.ts_cell_hmargin = crate::entities::common::format_length(horizontal);
        self.ts_cell_vmargin = crate::entities::common::format_length(vertical);
        self.ts_cell_format = format.value_format_string.clone();
        for (slot, edge) in document::objects::TABLE_GRID_EDGES.iter().enumerate() {
            let spacing = data.grid(*edge).map_or(0.25 * format.text_height, |grid| {
                grid.border.double_line_spacing
            });
            self.ts_edge_spacing[slot] = crate::entities::common::format_length(spacing);
        }
    }

    /// Edits the cell style the table style editor shows, then refreshes
    /// the style's row projection (DXF groups, older files).
    pub(in crate::app) fn edit_table_cell_style(
        &mut self,
        edit: impl FnOnce(&mut document::objects::NamedTableCellStyle),
    ) {
        let i = self.active_tab;
        let name = self.ts_cell.clone();
        if let Some(style) = self.tablestyle_mut(i) {
            if let Some(cell) = style.cell_style_mut(&name) {
                edit(cell);
            }
            style.sync_rows_from_cell_styles();
        }
    }

    /// An external-reference text style: flagged, or merged in as
    /// `XREF|Name` (the merge does not set the flag).
    pub(in crate::app) fn selected_textstyle_is_read_only(&self) -> bool {
        self.tabs[self.active_tab]
            .scene
            .document
            .text_styles
            .get(&self.textstyle_selected)
            .is_none_or(|style| style.xref_dependent || style.name.contains('|'))
    }

    /// Load the selected text style into the editor's draft.
    pub(in crate::app) fn load_textstyle_draft(&mut self, tab: usize) {
        let doc = &self.tabs[tab].scene.document;
        // One walk of the drawing gives the list's counts and the texts of
        // every style.
        let users = crate::app::text_style_users::users_by_style(doc);
        self.textstyle_counts = users
            .iter()
            .map(|(name, texts)| (name.clone(), texts.len()))
            .collect();
        if let Some(s) = doc.text_styles.get(&self.textstyle_selected) {
            self.textstyle_draft = crate::app::text_style_draft::TextStyleDraft::load(s);
        }
        self.textstyle_font_picker.open = false;
        self.textstyle_bigfont_picker.open = false;
        self.textstyle_users = users;
    }

    /// What Apply would change now: every staged text style, with the draft
    /// written over the selected one as Apply writes it.
    pub(in crate::app) fn textstyle_pending_changes(
        &self,
    ) -> Vec<crate::app::text_style_users::StyleChange> {
        let edited = self.textstyle_draft.resolved();
        let read_only = self.selected_textstyle_is_read_only();
        crate::app::text_style_users::pending_changes(
            |handle| self.text_style_baseline(handle),
            self.tabs[self.active_tab]
                .scene
                .document
                .text_styles
                .iter()
                .map(|s| {
                    if !read_only && s.handle == edited.handle {
                        &edited
                    } else {
                        s
                    }
                }),
            &self.textstyle_users,
        )
    }

    /// The texts that use the selected text style.
    pub(in crate::app) fn selected_textstyle_users(
        &self,
    ) -> &[crate::app::text_style_users::TextUser] {
        self.textstyle_users
            .get(&self.textstyle_selected.to_ascii_lowercase())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(super) fn on_text_style_dialog_open(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let cur = self.tabs[i]
            .scene
            .document
            .header
            .current_text_style_name
            .clone();
        let exists = self.tabs[i].scene.document.text_styles.get(&cur).is_some();
        self.textstyle_selected = if exists {
            cur
        } else {
            self.tabs[i]
                .scene
                .document
                .text_styles
                .iter()
                .next()
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Standard".to_string())
        };
        self.load_textstyle_draft(i);
        // The head marks a missing font before the list is first opened.
        self.textstyle_font_picker.open = false;
        self.textstyle_font_picker
            .refresh(&self.tabs[i].scene.document, false);
        self.textstyle_bigfont_picker.open = false;
        self.textstyle_bigfont_picker
            .refresh_bigfont(&self.tabs[i].scene.document);
        self.active_modal = Some(crate::app::ModalKind::TextStyle);
        self.style_stage_begin();
        Task::none()
    }

    /// Write the draft into the selected text style (staged, no commit), so
    /// edits survive switching to another style as well as Apply. Values the
    /// user did not change stay exactly as stored.
    pub(in crate::app) fn flush_textstyle_draft(&mut self) {
        if self.selected_textstyle_is_read_only() {
            return;
        }
        let i = self.active_tab;
        let edited = self.textstyle_draft.resolved();
        if let Some(s) = self.tabs[i]
            .scene
            .document
            .text_styles
            .get_mut(&self.textstyle_selected)
        {
            if *s != edited {
                *s = edited;
            }
        }
    }

    /// The text style manager's list: each style's text count and lock.
    /// "Styles in use" keeps the used ones, the current and the selected. A
    /// shape file entry (linetype symbols) has no name and is not listed.
    pub(in crate::app) fn textstyle_list(
        &self,
    ) -> (Vec<String>, Vec<crate::ui::style::style_manager::StyleRow>) {
        let doc = &self.tabs[self.active_tab].scene.document;
        let current = &doc.header.current_text_style_name;
        doc.text_styles
            .iter()
            .filter(|s| !s.is_shape_file)
            .map(|s| {
                let count = self
                    .textstyle_counts
                    .get(&s.name.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0);
                let row = crate::ui::style::style_manager::StyleRow {
                    count,
                    locked: s.xref_dependent || s.name.contains('|'),
                };
                (s.name.clone(), row)
            })
            .filter(|(name, row)| {
                !self.textstyle_in_use_only
                    || row.count > 0
                    || name.eq_ignore_ascii_case(current)
                    || name.eq_ignore_ascii_case(&self.textstyle_selected)
            })
            .unzip()
    }

    /// What the text style manager's preview draws: `edited` resolved as the
    /// drawing resolves a style (the SHX file on disk, its big font, the
    /// TrueType face of the bold and italic flags).
    pub(in crate::app) fn textstyle_preview(
        &self,
        edited: &document::tables::TextStyle,
    ) -> crate::ui::style::textstyle::Preview {
        let doc = &self.tabs[self.active_tab].scene.document;
        let resolved = crate::entities::text_support::resolve_text_style_record(edited, doc);
        let ttf = !edited.true_type_font.trim().is_empty();
        let mut font_label = if ttf {
            edited.true_type_font.clone()
        } else {
            edited.font_file.clone()
        };
        if self.textstyle_draft.big_font_on && !edited.big_font_file.trim().is_empty() {
            font_label = format!("{font_label} + {}", edited.big_font_file);
        }
        if edited.height > 0.0 {
            let mode = if edited.annotative {
                crate::t!("Paper height")
            } else {
                crate::t!("Drawing height")
            };
            let height = crate::entities::common::format_length(edited.height);
            font_label = format!("{font_label} · {mode} {height}");
        }
        crate::ui::style::textstyle::Preview {
            font: resolved.font_name,
            width_factor: if resolved.is_backward {
                -resolved.width_factor
            } else {
                resolved.width_factor
            },
            oblique: resolved.oblique_angle,
            upside_down: resolved.is_upside_down,
            vertical: resolved.is_vertical && !ttf,
            text: if self.textstyle_sample.trim().is_empty() {
                crate::ui::style::textstyle::default_sample(self.textstyle_draft.big_font_on)
                    .to_string()
            } else {
                self.textstyle_sample.clone()
            },
            caption: font_label,
        }
    }

    pub(super) fn on_text_style_apply(&mut self) -> Task<Message> {
        // A field that cannot be applied keeps Apply off (the window says
        // which); nothing is written until it is fixed.
        if self.textstyle_draft.invalid_fields() > 0 {
            return Task::none();
        }
        self.flush_textstyle_draft();
        // Opened from another manager, Apply keeps the changes in that
        // manager's transaction.
        if !self.style_stage_child_apply() {
            self.style_stage_commit();
        }
        // Re-base the draft on the stored values so a second Apply is a no-op.
        self.load_textstyle_draft(self.active_tab);
        Task::none()
    }

    pub(super) fn on_table_cell_style_new(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let source = self.ts_cell.clone();
        let typed = self.ts_cell_name.trim().to_string();
        let Some(style) = self.tablestyle_mut(i) else {
            return Task::none();
        };
        let name = if typed.is_empty() {
            style.unique_cell_style_name(crate::t!("Cell style").as_ref())
        } else {
            typed
        };
        if style.create_cell_style(&name, Some(&source)).is_none() {
            self.command_line
                .push_error(crate::t!("A cell style with this name already exists.").as_ref());
            return Task::none();
        }
        self.ts_cell = name;
        self.load_tablestyle_bufs(i);
        Task::none()
    }

    pub(super) fn on_table_cell_style_rename(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let name = self.ts_cell.clone();
        let new_name = self.ts_cell_name.trim().to_string();
        let Some(style) = self.tablestyle_mut(i) else {
            return Task::none();
        };
        if style.rename_cell_style(&name, &new_name) {
            style.sync_rows_from_cell_styles();
            self.ts_cell = new_name;
            self.load_tablestyle_bufs(i);
        } else if !new_name.is_empty() && !document::objects::is_row_cell_style(&name) {
            self.command_line
                .push_error(crate::t!("A cell style with this name already exists.").as_ref());
        }
        Task::none()
    }

    pub(super) fn on_table_cell_style_delete(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let name = self.ts_cell.clone();
        // A cell style tables of the style still use stays.
        if self.table_cell_style_in_use(i, &name) {
            self.command_line
                .push_error(crate::t!("The cell style is in use and cannot be deleted.").as_ref());
            return Task::none();
        }
        if let Some(style) = self.tablestyle_mut(i) {
            if style.delete_cell_style(&name) {
                style.sync_rows_from_cell_styles();
                self.ts_cell = document::objects::DATA_CELL_STYLE.to_string();
                self.load_tablestyle_bufs(i);
            }
        }
        Task::none()
    }

    /// Whether a table of the selected style uses the named cell style.
    fn table_cell_style_in_use(&self, tab: usize, name: &str) -> bool {
        use document::objects::ObjectType;
        let document = &self.tabs[tab].scene.document;
        let found = document
            .objects
            .iter()
            .find_map(|(handle, object)| match object {
                ObjectType::TableStyle(style) if style.name == self.tablestyle_selected => style
                    .cell_style(name)
                    .map(|cell| (*handle, cell.id, style.template())),
                _ => None,
            });
        let Some((handle, id, template)) = found else {
            return false;
        };
        let uses =
            |table: &document::entities::Table| {
                table.rows.iter().any(|row| {
                    row.style_id == id || row.cells.iter().any(|cell| cell.style_id == id)
                }) || table.columns.iter().any(|column| column.style_id == id)
            };
        // The style's own starting table counts too.
        let template_uses = template
            .and_then(|template| match document.objects.get(&template) {
                Some(ObjectType::TableTemplate(table)) => Some(uses(table)),
                _ => None,
            })
            .unwrap_or(false);
        template_uses
            || document.entities().any(|entity| match entity {
                document::EntityType::Table(table) if table.table_style_handle == Some(handle) => {
                    uses(table)
                }
                _ => false,
            })
    }

    pub(super) fn on_table_cell_style_edit(
        &mut self,
        field: &'static str,
        value: String,
    ) -> Task<Message> {
        let number = || value.trim().parse::<f64>().ok().filter(|v| v.is_finite());
        match field {
            "name" => self.ts_cell_name = value,
            "height" => {
                if let Some(height) = number().filter(|h| *h > 0.0) {
                    self.edit_table_cell_style(|cell| {
                        cell.cell_style.content_format.text_height = height;
                    });
                }
                self.ts_cell_height = value;
            }
            "angle" => {
                if let Some(angle) = number() {
                    self.edit_table_cell_style(|cell| {
                        cell.cell_style.content_format.rotation = angle.to_radians();
                    });
                }
                self.ts_cell_angle = value;
            }
            "hmargin" | "vmargin" => {
                if field == "hmargin" {
                    self.ts_cell_hmargin = value;
                } else {
                    self.ts_cell_vmargin = value;
                }
                let horizontal = self.ts_cell_hmargin.trim().parse::<f64>().ok();
                let vertical = self.ts_cell_vmargin.trim().parse::<f64>().ok();
                if let (Some(h), Some(v)) = (horizontal, vertical) {
                    if h >= 0.0 && v >= 0.0 {
                        self.edit_table_cell_style(|cell| cell.cell_style.set_margins(h, v));
                    }
                }
            }
            "format" => {
                let format = value.clone();
                self.edit_table_cell_style(|cell| {
                    cell.cell_style.content_format.value_format_string = format;
                });
                self.ts_cell_format = value;
            }
            "align" => {
                if let Ok(align) = value.parse::<i32>() {
                    self.edit_table_cell_style(|cell| {
                        cell.cell_style.content_format.cell_alignment = align;
                    });
                }
            }
            "class" => {
                if let Ok(class) = value.parse::<i32>() {
                    self.edit_table_cell_style(|cell| cell.style_type = class);
                }
            }
            "datatype" => {
                let parsed = value
                    .split_once('/')
                    .and_then(|(d, u)| Some((d.parse::<i32>().ok()?, u.parse::<i32>().ok()?)));
                if let Some((data, unit)) = parsed {
                    self.edit_table_cell_style(|cell| {
                        cell.cell_style.content_format.value_data_type = data;
                        cell.cell_style.content_format.value_unit_type = unit;
                    });
                }
            }
            "textstyle" => {
                if let Ok(handle) = u64::from_str_radix(value.trim(), 16) {
                    self.edit_table_cell_style(|cell| {
                        cell.cell_style.content_format.text_style = document::Handle::new(handle);
                    });
                }
            }
            "textcolor" | "fillcolor" => {
                let color = match value.trim().parse::<i16>() {
                    Ok(0) => document::types::Color::ByBlock,
                    Ok(256) => document::types::Color::ByLayer,
                    Ok(257) => document::types::Color::None,
                    Ok(index @ 1..=255) => document::types::Color::Index(index as u8),
                    _ => return Task::none(),
                };
                self.edit_table_cell_style(|cell| {
                    if field == "textcolor" {
                        cell.cell_style.content_format.content_color = color;
                    } else {
                        cell.cell_style.background_color = color;
                    }
                });
            }
            "merge" => {
                let on = value == "1";
                self.edit_table_cell_style(|cell| cell.cell_style.set_merge_all(on));
            }
            _ => {}
        }
        Task::none()
    }

    pub(super) fn on_table_cell_edge_edit(
        &mut self,
        edge: u8,
        field: &'static str,
        value: String,
    ) -> Task<Message> {
        use document::objects::TableBorderType;
        let Some(mask) = document::objects::TABLE_GRID_EDGES
            .get(edge as usize)
            .copied()
        else {
            return Task::none();
        };
        match field {
            "visible" => {
                let hidden = value != "1";
                self.edit_table_cell_style(|cell| cell.cell_style.set_edge_hidden(mask, hidden));
            }
            "lw" => {
                if let Ok(weight) = value.parse::<i16>() {
                    self.edit_table_cell_style(|cell| {
                        cell.cell_style.grid_mut(mask).border.line_weight =
                            document::types::LineWeight::from_value(weight);
                    });
                }
            }
            "linetype" => {
                let handle = u64::from_str_radix(value.trim(), 16)
                    .map(document::Handle::new)
                    .unwrap_or(document::Handle::NULL);
                self.edit_table_cell_style(|cell| {
                    cell.cell_style.grid_mut(mask).line_type = handle
                });
            }
            "color" => {
                let color = match value.trim().parse::<i16>() {
                    Ok(0) => document::types::Color::ByBlock,
                    Ok(256) => document::types::Color::ByLayer,
                    Ok(index @ 1..=255) => document::types::Color::Index(index as u8),
                    _ => return Task::none(),
                };
                self.edit_table_cell_style(|cell| {
                    cell.cell_style.grid_mut(mask).border.color = color
                });
            }
            "double" => {
                let double = value == "1";
                self.edit_table_cell_style(|cell| {
                    cell.cell_style.grid_mut(mask).border.border_type = if double {
                        TableBorderType::Double
                    } else {
                        TableBorderType::Single
                    };
                });
            }
            "spacing" => {
                if let Some(spacing) = value.trim().parse::<f64>().ok().filter(|v| *v > 0.0) {
                    self.edit_table_cell_style(|cell| {
                        cell.cell_style.grid_mut(mask).border.double_line_spacing = spacing;
                    });
                }
                if let Some(slot) = self.ts_edge_spacing.get_mut(edge as usize) {
                    *slot = value;
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// Select table to start from: the dialog waits while a table is picked.
    pub(super) fn on_table_style_template_pick(&mut self) -> Task<Message> {
        let i = self.active_tab;
        self.active_modal = None;
        let command = crate::modules::annotate::table_cmd::TableTemplatePickCommand;
        self.command_line
            .push_info(&crate::command::CadCommand::prompt(&command));
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    /// The pick ended: a table becomes the starting table and the window
    /// comes back; another object is refused and asked again.
    pub(in crate::app) fn table_template_pick_done(&mut self, handle: Option<document::Handle>) {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        let is_table = handle.is_some_and(|handle| {
            matches!(
                self.tabs[i].scene.document.get_entity(handle),
                Some(document::EntityType::Table(_))
            )
        });
        if handle.is_some() && !is_table {
            self.command_line
                .push_error(crate::t!("The object is not a table.").as_ref());
            let _ = self.on_table_style_template_pick();
            return;
        }
        self.table_style_template_picked(handle);
    }

    /// Back from the pick: the picked table becomes the starting table.
    fn table_style_template_picked(&mut self, table: Option<document::Handle>) {
        use document::objects::ObjectType;
        let i = self.active_tab;
        let picked =
            table.and_then(
                |handle| match self.tabs[i].scene.document.get_entity(handle) {
                    Some(document::EntityType::Table(table)) => Some(table.as_ref().clone()),
                    _ => None,
                },
            );
        let selected = self.tablestyle_selected.clone();
        if let Some(mut template) = picked {
            let document = &mut self.tabs[i].scene.document;
            let style_handle = document
                .objects
                .iter()
                .find_map(|(handle, object)| match object {
                    ObjectType::TableStyle(style) if style.name == selected => Some(*handle),
                    _ => None,
                });
            if let Some(style_handle) = style_handle {
                let handle = document.allocate_handle();
                crate::entities::table::detach_fields(&mut template);
                template.common = document::entities::EntityCommon::default();
                template.common.handle = handle;
                template.common.owner_handle = style_handle;
                template.common.reactors = vec![style_handle];
                template.table_style_handle = Some(style_handle);
                template.block_record_handle = None;
                template.block_name.clear();
                template.insertion_point = document::types::Vector3::ZERO;
                document
                    .objects
                    .insert(handle, ObjectType::TableTemplate(template));
                let old = match document.objects.get_mut(&style_handle) {
                    Some(ObjectType::TableStyle(style)) => {
                        let old = style.template();
                        style.modern_cell_style_handle = handle;
                        style.raw_dxf_codes = None;
                        old
                    }
                    _ => None,
                };
                if let Some(old) = old {
                    document.objects.remove(&old);
                }
            }
        }
        self.active_modal = Some(crate::app::ModalKind::TableStyle);
    }

    /// Remove the starting table.
    pub(super) fn on_table_style_template_remove(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let old = self.tablestyle_mut(i).and_then(|style| {
            let old = style.template();
            style.modern_cell_style_handle = document::Handle::NULL;
            style.raw_dxf_codes = None;
            old
        });
        if let Some(old) = old {
            self.tabs[i].scene.document.objects.remove(&old);
        }
        Task::none()
    }

    pub(super) fn on_ml_style_dialog_open(&mut self) -> Task<Message> {
        use document::objects::ObjectType;
        let i = self.active_tab;
        let cur = self.tabs[i].scene.document.header.multiline_style.clone();
        let exists = self.tabs[i]
            .scene
            .document
            .objects
            .values()
            .any(|o| matches!(o, ObjectType::MLineStyle(s) if s.name == cur));
        self.mlstyle_selected = if exists {
            cur
        } else {
            self.tabs[i]
                .scene
                .document
                .objects
                .values()
                .find_map(|o| {
                    if let ObjectType::MLineStyle(s) = o {
                        Some(s.name.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "Standard".to_string())
        };
        self.active_modal = Some(crate::app::ModalKind::MlStyle);
        self.load_mlstyle_bufs(i);
        self.style_stage_begin();
        Task::none()
    }

    pub(super) fn on_color_window_pick(
        &mut self,
        color: document::types::Color,
        name: Option<String>,
    ) -> Task<Message> {
        if let Some(crate::app::ColorPickTarget::Interface(target)) = self
            .color_picker
            .as_ref()
            .map(|picker| picker.target.clone())
        {
            self.color_picker = None;
            let Some((r, g, b)) = crate::ui::color_select::math::color_rgb(color) else {
                return Task::none();
            };
            // Hand the result to the same handler the typed hex field uses, so
            // the picker and the field cannot drift apart on validation or
            // persistence.
            let hex = crate::app::config::rgb_to_hex([r, g, b]);
            use crate::app::UiColor;
            return Task::done(match target {
                UiColor::ModelBackground => Message::ModelSpaceBgChanged(hex),
                UiColor::PaperBackground => Message::PaperSpaceBgChanged(hex),
                UiColor::DeskBackground => Message::DeskSpaceBgChanged(hex),
                UiColor::Theme(index) => Message::OptionsThemeColorChanged(index, hex),
                UiColor::Crosshair => Message::CrosshairColorChanged(hex),
                UiColor::SnapMarker => Message::SnapMarkerColorChanged(hex),
                UiColor::CommandText => Message::CommandTextColorChanged(hex),
            });
        }
        if matches!(
            self.color_picker.as_ref().map(|picker| &picker.target),
            Some(crate::app::ColorPickTarget::PlotStyle)
        ) {
            self.color_picker = None;

            if let Some((r, g, b)) = crate::ui::color_select::math::color_rgb(color) {
                self.ps_color_buf = crate::ui::color_select::math::hex_string(r, g, b);
            }

            return self.on_plot_style_panel_apply();
        }
        let s = crate::ui::color_select::color_to_aci_string(color);
        let edit = match self.color_picker.take().map(|picker| picker.target) {
            Some(crate::app::ColorPickTarget::Gradient(index)) => {
                Some(Message::GraphicAttributes(GraphicAttributesMsg::Gradient(
                    crate::ui::window::gradient_editor::GradientMsg::Color(index, color),
                )))
            }
            Some(crate::app::ColorPickTarget::ViewBackground(slot)) => {
                crate::ui::color_select::math::color_rgb(color).map(|(r, g, b)| {
                    Message::PdfDialog(crate::ui::window::pdf_dialogs::PdfDialogMsg::BgColor(
                        slot,
                        [r, g, b],
                    ))
                })
            }
            Some(crate::app::ColorPickTarget::GraphicAttributesLine) => Some(
                Message::GraphicAttributes(GraphicAttributesMsg::LineColor(color)),
            ),
            Some(crate::app::ColorPickTarget::GraphicAttributesSolid) => Some(
                Message::GraphicAttributes(GraphicAttributesMsg::FillColor(color)),
            ),
            Some(crate::app::ColorPickTarget::DimStyle(f)) => Some(Message::DimStyleDlg(
                crate::ui::style::dimstyle::DsMsg::Color(f, color),
            )),
            Some(crate::app::ColorPickTarget::MLeader(f)) => {
                Some(Message::MLeaderStyleEdit { field: f, value: s })
            }
            Some(crate::app::ColorPickTarget::Table(f)) => {
                Some(Message::TableStyleCellEdit { field: f, value: s })
            }
            Some(crate::app::ColorPickTarget::Properties) => {
                return self.set_property_color(color, name);
            }
            Some(crate::app::ColorPickTarget::PropertiesBg) => {
                Some(Message::PropBgColorChanged(color))
            }
            Some(crate::app::ColorPickTarget::PropertiesField(field)) => {
                Some(Message::PropColorFieldChanged { field, color })
            }
            Some(crate::app::ColorPickTarget::MText) => Some(Message::MTextColorChanged(color)),
            Some(crate::app::ColorPickTarget::MTextMask) => {
                Some(Message::MTextMaskColor(Some(color)))
            }
            Some(crate::app::ColorPickTarget::Ribbon) => {
                let task = self.update(Message::RibbonColorChanged(color));
                self.ribbon.active_color_book = name.map(|name| (color, name));
                return task;
            }
            Some(crate::app::ColorPickTarget::RibbonLayer(layer)) => {
                return self.set_ribbon_layer_color(&layer, color, name.as_deref());
            }
            Some(crate::app::ColorPickTarget::Layer(layer)) => {
                let i = self.active_tab;
                match self.layer_panel_index(i, &layer) {
                    Some(idx) => {
                        self.tabs[i].layers.selected = Some(idx);
                        return self.set_layer_color(color, name.as_deref());
                    }
                    None => None,
                }
            }
            Some(crate::app::ColorPickTarget::LayerViewport(layer, viewport)) => {
                self.set_layer_viewport_color(self.active_tab, &layer, viewport, color);
                None
            }
            Some(crate::app::ColorPickTarget::LayerState(idx)) => {
                Some(Message::LayerStateEditorLayerColor(idx, color))
            }
            // Handled before this match.
            Some(
                crate::app::ColorPickTarget::PlotStyle | crate::app::ColorPickTarget::Interface(_),
            ) => None,
            None => None,
        };
        if let Some(m) = edit {
            self.update(m)
        } else {
            Task::none()
        }
    }
}

