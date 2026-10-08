//! COUNT and its family: count mode (COUNT, COUNTAREA, COUNTCLOSE), the
//! Count palette (COUNTLIST, COUNTLISTCLOSE), count fields (COUNTFIELD,
//! UPDATEFIELD), count tables (COUNTTABLE) and the COUNT* variables.

use crate::app::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::annotate::count_cmd::{CountCommand, CountTableCommand, UpdateFieldCommand};
use crate::modules::annotate::field_cmd::FieldPlaceCommand;
use crate::ui::window::count_palette::{aci_rgba, CountMode, CountMsg, CountTarget, RowAction, AREA_LAYER};
use codec::count::{area_json, block_json, single_json, CountKey};
use codec::{EntityType, Handle};
use iced::Task;

/// The variables this family answers for.
pub(super) const COUNT_SYSVARS: &[&str] =
    &["COUNTNUMBER", "COUNTSERVICE", "COUNTPALETTESTATE", "COUNTCOLOR", "COUNTERRORCOLOR"];

fn hex(h: Handle) -> String {
    format!("{:X}", h.value())
}

fn parse_handle(s: &str) -> Option<Handle> {
    u64::from_str_radix(s.trim(), 16).ok().map(Handle::new)
}

fn parse_xy(s: &str) -> Option<[f64; 2]> {
    let (x, y) = s.split_once(',')?;
    Some([x.trim().parse().ok()?, y.trim().parse().ok()?])
}

/// A field referring to `boundary` keeps the count area's polyline.
fn boundary_in_use(doc: &codec::CadDocument, boundary: Handle) -> bool {
    let needle = format!("\"boundaryObjectHandle\":\"{}\"", hex(boundary));
    doc.fields.values().any(|f| f.code.contains(&needle))
}

impl OpenCADStudio {
    pub(super) fn dispatch_count(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let cmd = cmd.trim();
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        let verb = verb.to_ascii_uppercase();
        let rest = rest.trim();
        if verb == "SETVAR" {
            let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let name = name.to_ascii_uppercase();
            if !COUNT_SYSVARS.contains(&name.as_str()) {
                return None;
            }
            self.count_sysvar(i, &name, Some(value.trim()).filter(|v| !v.is_empty()));
            return Some(Task::none());
        }
        if COUNT_SYSVARS.contains(&verb.as_str()) {
            self.count_sysvar(i, &verb, Some(rest).filter(|v| !v.is_empty()));
            return Some(Task::none());
        }
        match verb.as_str() {
            "COUNT" => {
                let preselected = self.tabs[i].scene.selected_handles_in_order();
                let current = matches!(rest.trim_start_matches('_').to_ascii_uppercase().as_str(), "C");
                if current && !preselected.is_empty() {
                    // Count Selection: the selection is the target.
                    let found = preselected.len();
                    self.command_line.push_output(&crate::tf!("{found} found"));
                    let handles: Vec<String> = preselected.iter().map(|h| hex(*h)).collect();
                    self.tabs[i].scene.deselect_all();
                    return Some(self.dispatch_command(&format!("_COUNTRUN C T {}", handles.join(","))));
                }
                // Plain COUNT asks for its targets itself.
                self.tabs[i].scene.deselect_all();
                let command = if current { CountCommand::with_area("C") } else { CountCommand::new(false) };
                Some(self.start_count_command(i, Box::new(command)))
            }
            "COUNTAREA" => Some(self.start_count_command(i, Box::new(CountCommand::new(true)))),
            "COUNTCLOSE" => {
                self.close_count(i);
                Some(Task::none())
            }
            "COUNTLIST" => {
                self.set_count_palette(true);
                Some(Task::none())
            }
            "COUNTLISTCLOSE" => {
                self.set_count_palette(false);
                Some(Task::none())
            }
            "_COUNTLISTTOGGLE" => {
                let open = !self.count_palette.show;
                self.set_count_palette(open);
                Some(Task::none())
            }
            "COUNTFIELD" => {
                match self.count_field_code(i) {
                    Some(code) => return Some(self.start_count_field(i, code)),
                    None if self.tabs[i].count.is_some() => {}
                    None => self
                        .command_line
                        .push_error(&crate::t!("** COUNTFIELD command only available during Count. **")),
                }
                Some(Task::none())
            }
            "COUNTTABLE" => {
                let doc = &self.tabs[i].scene.document;
                let mut blocks = Vec::new();
                let mut others = [0usize; 3];
                for br in doc.block_records.iter() {
                    let upper = br.name.to_ascii_uppercase();
                    if upper == "*MODEL_SPACE" || upper.starts_with("*PAPER_SPACE") {
                        continue;
                    }
                    if br.flags.is_xref || br.flags.is_xref_overlay {
                        others[0] += 1;
                    } else if br.name.contains('|') {
                        others[1] += 1;
                    } else if br.name.starts_with('*') {
                        others[2] += 1;
                    } else {
                        blocks.push(br.name.clone());
                    }
                }
                blocks.sort_by_key(|b| b.to_ascii_uppercase());
                Some(self.start_count_command(i, Box::new(CountTableCommand::new(blocks, others))))
            }
            "UPDATEFIELD" => {
                let preselected = self.tabs[i].scene.selected_handles_in_order();
                if !preselected.is_empty() {
                    self.update_fields(i, &preselected);
                    return Some(Task::none());
                }
                Some(self.start_count_command(i, Box::new(UpdateFieldCommand::new())))
            }
            "_UPDATEFIELDRUN" => {
                let handles: Vec<Handle> = rest.split(',').filter_map(parse_handle).collect();
                self.tabs[i].scene.deselect_all();
                self.update_fields(i, &handles);
                Some(Task::none())
            }
            "_COUNTRUN" => {
                self.count_run(i, rest);
                Some(Task::none())
            }
            "_COUNTAREASET" => {
                let tokens: Vec<&str> = rest.split_whitespace().collect();
                if let Some((area, boundary)) = self.resolve_count_area(i, &tokens) {
                    let mut mode = self.tabs[i].count.take().unwrap_or_default();
                    if mode.boundary != boundary {
                        self.drop_count_boundary(i, mode.boundary);
                    }
                    mode.area = area;
                    mode.boundary = boundary;
                    mode.cursor = None;
                    self.tabs[i].count = Some(mode);
                    self.apply_count_display(i);
                    self.command_line.push_output(&crate::t!("count area is active."));
                }
                Some(Task::none())
            }
            "_COUNTTABLEPLACE" => {
                let (point, names) = rest.split_once(' ').unwrap_or((rest, ""));
                let p: Vec<f64> = point.split(',').filter_map(|v| v.parse().ok()).collect();
                if p.len() == 3 {
                    let names: Vec<String> = names.split('|').filter(|n| !n.is_empty()).map(str::to_string).collect();
                    self.place_count_table(i, [p[0], p[1], p[2]], names);
                }
                Some(Task::none())
            }
            _ => None,
        }
    }

    fn start_count_command(&mut self, i: usize, command: Box<dyn CadCommand>) -> Task<Message> {
        self.reset_command_start_state(i);
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(command);
        self.push_ucs_to_cmd(i);
        self.sync_dyn_fields();
        self.focus_cmd_input()
    }

    /// COUNTNUMBER / COUNTPALETTESTATE are read-only; COUNTSERVICE,
    /// COUNTCOLOR and COUNTERRORCOLOR take a value (asked for when none is
    /// given).
    fn count_sysvar(&mut self, i: usize, name: &str, value: Option<&str>) {
        let read_only = match name {
            "COUNTNUMBER" => Some(
                self.tabs[i]
                    .count
                    .as_ref()
                    .filter(|m| m.target.is_some())
                    .map(|m| m.count(&self.tabs[i].scene.document))
                    .unwrap_or(0),
            ),
            "COUNTPALETTESTATE" => Some(self.count_palette.show as usize),
            _ => None,
        };
        if let Some(v) = read_only {
            self.command_line.push_output(&crate::tf!("{name} = {v} (read only)"));
            return;
        }
        let (current, min, max) = match name {
            "COUNTSERVICE" => (self.count_palette.service as i32, 0, 1),
            "COUNTCOLOR" => (self.count_palette.color as i32, 1, 255),
            _ => (self.count_palette.error_color as i32, 1, 255),
        };
        let ask = |app: &mut Self| {
            app.command_line.push_output(&crate::tf!("Enter new value for {name} <{current}>:"));
            app.pending_setvar = Some(name.to_string());
        };
        let Some(value) = value else {
            ask(self);
            return;
        };
        match value.parse::<i32>() {
            Ok(n) if (min..=max).contains(&n) => {
                match name {
                    "COUNTSERVICE" => self.count_palette.service = n == 1,
                    "COUNTCOLOR" => self.count_palette.color = n as i16,
                    _ => self.count_palette.error_color = n as i16,
                }
                self.persist_settings_if_changed();
                self.apply_count_display(i);
            }
            _ => {
                self.command_line.push_error(&if max == 1 {
                    crate::t!("Requires 0 or 1 only.")
                } else {
                    crate::tf!("Requires an integer between {min} and {max}.")
                });
                ask(self);
            }
        }
    }

    /// Open or close the Count palette (docked on the right, expanded).
    pub(in crate::app) fn set_count_palette(&mut self, open: bool) {
        let id = crate::ui::dock::PanelId::Count;
        self.count_palette.show = open;
        self.ribbon.set_count_palette(open);
        if open {
            if self.dock.location(id).is_none() {
                self.dock.dock(id, crate::app::config::DockSide::Right, usize::MAX);
            }
            self.dock_expanded = Some(id);
        } else if self.dock_expanded == Some(id) {
            self.dock_expanded = None;
        }
    }

    /// Recolour the drawing for count mode: counted references, duplicates,
    /// the rest faded (nothing changes without a target).
    pub(in crate::app) fn apply_count_display(&mut self, i: usize) {
        let computed = self.tabs[i].count.as_ref().map(|m| m.compute(&self.tabs[i].scene.document));
        if let (Some(mode), Some(c)) = (self.tabs[i].count.as_mut(), computed) {
            mode.cache = Some(c);
        }
        let display = self.tabs[i].count.as_ref().filter(|m| m.target.is_some()).map(|m| {
            let r = m.result(&self.tabs[i].scene.document);
            crate::scene::CountDisplay {
                counted: r.counted.iter().copied().collect(),
                errors: r.errors.iter().chain(&r.overlapped).copied().collect(),
                color: aci_rgba(self.count_palette.color),
                error_color: aci_rgba(self.count_palette.error_color),
            }
        });
        self.tabs[i].scene.set_count_display(display);
        let epoch = self.tabs[i].scene.geometry_epoch;
        if let Some(mode) = self.tabs[i].count.as_mut() {
            mode.epoch = epoch;
        }
    }

    /// After the drawing changes, count mode colours its references again.
    pub(in crate::app) fn refresh_count_if_stale(&mut self) {
        let i = self.active_tab;
        let Some(mode) = self.tabs[i].count.as_ref().filter(|m| m.epoch != self.tabs[i].scene.geometry_epoch) else {
            return;
        };
        if let Some((h, _)) = mode.boundary {
            match self.tabs[i].scene.document.get_entity(h) {
                // The boundary is gone (erased, undone): ask, or do what was
                // chosen for good.
                None => {
                    if self.active_modal == Some(crate::app::ModalKind::CountInvalidArea) {
                        return;
                    }
                    match self.count_palette.invalid_choice {
                        1 => self.count_invalid_undo(i),
                        2 => self.count_invalid_continue(i),
                        _ => {
                            self.count_palette.invalid_always = false;
                            self.active_modal = Some(crate::app::ModalKind::CountInvalidArea);
                        }
                    }
                    return;
                }
                // An edited boundary moves the area with it.
                Some(EntityType::LwPolyline(pl)) => {
                    let ring: Vec<[f64; 2]> = pl.vertices.iter().map(|v| [v.location.x, v.location.y]).collect();
                    if let Some(mode) = self.tabs[i].count.as_mut() {
                        mode.area = Some(ring);
                    }
                }
                Some(_) => {}
            }
        }
        self.apply_count_display(i);
    }

    /// Invalid Area › Undo (also ✕ and Cancel): the change that took the
    /// boundary away is reverted — redone when an undo took it, undone
    /// otherwise. When that does not bring the boundary back, the history
    /// step is put back and the area is dropped as with Continue.
    fn count_invalid_undo(&mut self, i: usize) {
        let Some((boundary, _)) = self.tabs[i].count.as_ref().and_then(|m| m.boundary) else {
            return;
        };
        // An open step (the edit that just erased the boundary) goes on the
        // undo stack first; after that, something to redo means the last
        // history step was an undo.
        self.finish_pending_history(i);
        let redo = !self.tabs[i].history.redo_stack.is_empty();
        if redo || !self.tabs[i].history.undo_stack.is_empty() {
            if redo {
                self.redo_active_tab();
            } else {
                self.undo_active_tab();
            }
            if self.tabs[i].scene.document.get_entity(boundary).is_some() {
                self.apply_count_display(i);
                return;
            }
            if redo {
                self.undo_active_tab();
            } else {
                self.redo_active_tab();
            }
        }
        self.count_invalid_continue(i);
    }

    /// Invalid Area › Continue: the area goes, the count covers all of model space.
    fn count_invalid_continue(&mut self, i: usize) {
        if let Some(mode) = self.tabs[i].count.as_mut() {
            mode.area = None;
            mode.boundary = None;
        }
        self.apply_count_display(i);
    }

    fn close_invalid_area(&mut self, choice: u8) {
        if self.active_modal == Some(crate::app::ModalKind::CountInvalidArea) {
            self.close_active_modal();
        }
        if self.count_palette.invalid_always {
            self.count_palette.invalid_choice = choice;
            self.persist_settings_if_changed();
        }
    }

    /// COUNTCLOSE: leave count mode; the area polyline count mode drew goes
    /// unless a field counts in it.
    pub(in crate::app) fn close_count(&mut self, i: usize) {
        if let Some(mode) = self.tabs[i].count.take() {
            self.drop_count_boundary(i, mode.boundary);
        }
        self.tabs[i].scene.set_count_display(None);
    }

    fn drop_count_boundary(&mut self, i: usize, boundary: Option<(Handle, bool)>) {
        if let Some((h, true)) = boundary {
            let doc = &self.tabs[i].scene.document;
            if doc.get_entity(h).is_some() && !boundary_in_use(doc, h) {
                // Count mode removes its own polyline (the area layer is
                // locked) outside the history: an open step must not take the
                // removal in, or undoing it would bring back an orphan area.
                self.finish_pending_history(i);
                self.tabs[i].scene.rollback_new_entities(&[h]);
                self.tabs[i].dirty = true;
            }
        }
    }

    /// The count area of `C`, `E`, `K`, `R a b`, `P a b c …` or `O h`, and
    /// its boundary polyline (rectangles and polygons are drawn on the
    /// `0-CountArea` layer). `None` when the area cannot be had.
    fn resolve_count_area(
        &mut self,
        i: usize,
        tokens: &[&str],
    ) -> Option<(Option<Vec<[f64; 2]>>, Option<(Handle, bool)>)> {
        let kind = tokens.first().copied().unwrap_or("E");
        let points = || tokens[1..].iter().map_while(|t| parse_xy(t)).collect::<Vec<_>>();
        match kind {
            "E" => Some((None, None)),
            "K" => {
                let mode = self.tabs[i].count.as_ref();
                Some((mode.and_then(|m| m.area.clone()), mode.and_then(|m| m.boundary)))
            }
            // Current area counts all of model space (the view and an area
            // in use take no part).
            "C" => Some((None, None)),
            "R" => {
                let p = points();
                let (a, b) = (p.first()?, p.get(1)?);
                let ring = vec![[a[0], a[1]], [b[0], a[1]], [b[0], b[1]], [a[0], b[1]]];
                let h = self.draw_count_area(i, &ring)?;
                Some((Some(ring), Some((h, true))))
            }
            "P" => {
                let ring = points();
                (ring.len() >= 3).then_some(())?;
                let h = self.draw_count_area(i, &ring)?;
                Some((Some(ring), Some((h, true))))
            }
            "O" => {
                let h = parse_handle(tokens.get(1)?)?;
                let ring = codec::count::boundary_polygon(&self.tabs[i].scene.document, h)?;
                Some((Some(ring), Some((h, false))))
            }
            _ => None,
        }
    }

    /// A closed polyline on `0-CountArea` (locked, colour 152, not plotted,
    /// Continuous), whatever the current layer.
    fn draw_count_area(&mut self, i: usize, ring: &[[f64; 2]]) -> Option<Handle> {
        self.push_undo_snapshot(i, "COUNT");
        let doc = &mut self.tabs[i].scene.document;
        if !doc.layers.contains(AREA_LAYER) {
            let mut layer = codec::tables::Layer::new(AREA_LAYER);
            layer.handle = doc.allocate_handle();
            layer.color = codec::types::Color::from_index(152);
            layer.is_plottable = false;
            layer.flags.locked = true;
            layer.line_type = "Continuous".into();
            let _ = doc.layers.add(layer);
        }
        let mut pl = codec::entities::LwPolyline::new();
        pl.vertices = ring
            .iter()
            .map(|p| codec::entities::LwVertex::new(codec::types::Vector2::new(p[0], p[1])))
            .collect();
        pl.is_closed = true;
        pl.common.layer = AREA_LAYER.into();
        let h = self.commit_entity_handle_preserve_layer(EntityType::LwPolyline(pl));
        self.tabs[i].dirty = true;
        h
    }

    /// `_COUNTRUN <area> L` opens the palette; `_COUNTRUN <area> T <handles>`
    /// counts the targets and enters count mode.
    fn count_run(&mut self, i: usize, rest: &str) {
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let split = tokens.iter().position(|t| *t == "L" || *t == "T").unwrap_or(tokens.len());
        let (area_tokens, tail) = tokens.split_at(split);
        let targets: Vec<Handle> = match tail {
            ["T", handles, ..] => handles
                .split(',')
                .filter_map(parse_handle)
                .filter(|h| self.tabs[i].scene.document.get_entity(*h).is_some())
                .collect(),
            _ => Vec::new(),
        };
        // List all blocks: the palette, counting in the area when one was drawn.
        if targets.is_empty() {
            self.set_count_palette(true);
            if matches!(area_tokens.first(), Some(&"C") | Some(&"E") | None) {
                return;
            }
        }
        let Some((area, boundary)) = self.resolve_count_area(i, area_tokens) else {
            return;
        };
        let doc = &self.tabs[i].scene.document;
        let blocks: Vec<&str> = targets
            .iter()
            .filter_map(|h| match doc.get_entity(*h) {
                Some(EntityType::Insert(ins)) => Some(ins.block_name.as_str()),
                _ => None,
            })
            .collect();
        let target = match blocks.first() {
            Some(first) if blocks.len() == targets.len() && blocks.iter().all(|b| b.eq_ignore_ascii_case(first)) => {
                let name = doc.block_records.get(first).map(|b| b.name.clone()).unwrap_or_else(|| first.to_string());
                Some(CountTarget::Block { name, reference: targets.first().copied(), matching: [false; 3], picked: true })
            }
            None if targets.is_empty() => None,
            _ => Some(CountTarget::Group(targets.clone())),
        };
        // The picked targets do not stay selected.
        self.tabs[i].scene.deselect_all();
        self.refresh_properties();
        if let Some(old) = self.tabs[i].count.take().filter(|o| o.boundary != boundary) {
            self.drop_count_boundary(i, old.boundary);
        }
        let mode = CountMode { area, boundary, target, ..CountMode::default() };
        if let Some(name) = mode.target_name() {
            let n = mode.count(&self.tabs[i].scene.document);
            self.command_line.push_output(&crate::tf!("{name} ...... {n}"));
        }
        self.tabs[i].count = Some(mode);
        self.apply_count_display(i);
    }

    /// The field COUNTFIELD places for the current count: the picked
    /// reference (`single`), or the block with the match options (in the
    /// area's polyline when there is one).
    fn count_field_code(&self, i: usize) -> Option<String> {
        let mode = self.tabs[i].count.as_ref()?;
        match mode.target.as_ref()? {
            CountTarget::Group(handles) => Some(format!("\\AcCount {}", single_json(handles))),
            CountTarget::Block { reference: Some(r), matching: [false, false, false], picked: true, .. }
                if mode.boundary.is_none() =>
            {
                Some(format!("\\AcCount {}", single_json(&[*r])))
            }
            CountTarget::Block { name, .. } => {
                let instances = mode.instances(&self.tabs[i].scene.document);
                Some(self.block_field_code(i, name, &mode.key(&instances)))
            }
        }
    }

    /// `\AcCount` of a block and key, `\AcCount2` inside count mode's area polyline.
    fn block_field_code(&self, i: usize, name: &str, key: &CountKey) -> String {
        match self.tabs[i].count.as_ref().and_then(|m| m.boundary) {
            Some((boundary, _)) => format!("\\AcCount2 {}", area_json(name, key, boundary)),
            None => format!("\\AcCount {}", block_json(name, key)),
        }
    }

    /// Place a count field as multi-line text (`Specify start point or [Height/Justify]:`).
    pub(in crate::app) fn start_count_field(&mut self, i: usize, code: String) -> Task<Message> {
        let doc = &self.tabs[i].scene.document;
        let value = crate::entities::field::evaluate(doc, &code, &[], None).unwrap_or_else(|| "####".into());
        let defaults = crate::scene::creation_style::current_text_defaults(doc);
        self.command_line.push_output(&crate::tf!(
            "MTEXT Current text style:  \"{}\"  Text height:  {:.4}",
            defaults.style_name, defaults.height
        ));
        let command = FieldPlaceCommand::new(value, (code, Vec::new()), defaults.style_name, defaults.height).named("MTEXT");
        self.start_count_command(i, Box::new(command))
    }

    /// UPDATEFIELD: every field the objects host is evaluated again and its
    /// text written back, whatever FIELDEVAL says. A block reference brings
    /// its attributes and the texts of its block definition; a table, the
    /// cell texts of its drawing (its `*T` block).
    fn update_fields(&mut self, i: usize, handles: &[Handle]) {
        // Every run is an undo step, as in the reference, even with nothing
        // selected, no field found or no value changed.
        self.push_undo_mark(i, "UPDATEFIELD");
        if handles.is_empty() {
            return;
        }
        let doc = &self.tabs[i].scene.document;
        let mut hosts = handles.to_vec();
        for h in handles {
            let record = match doc.get_entity(*h) {
                Some(EntityType::Insert(insert)) => {
                    hosts.extend(insert.attributes.iter().map(|a| a.common.handle));
                    doc.block_records.get(&insert.block_name)
                }
                Some(EntityType::Table(t)) => t
                    .block_record_handle
                    .and_then(|r| doc.block_records.iter().find(|b| b.handle == r))
                    .or_else(|| doc.block_records.get(&t.block_name).filter(|_| !t.block_name.is_empty())),
                _ => None,
            };
            if let Some(record) = record {
                hosts.extend(record.entity_handles.iter().copied());
            }
        }
        let attributes: Vec<&codec::entities::EntityCommon> = handles
            .iter()
            .filter_map(|h| match doc.get_entity(*h) {
                Some(EntityType::Insert(insert)) => Some(insert.attributes.iter().map(|a| &a.common)),
                _ => None,
            })
            .flatten()
            .collect();
        let hosting = hosts.iter().any(|h| match doc.get_entity(*h) {
            Some(entity) => crate::entities::field::hosts_field(doc, entity),
            None => attributes.iter().any(|common| {
                common.handle == *h && crate::entities::field::common_hosts_field(doc, common)
            }),
        });
        if !hosting {
            self.command_line.push_output(&crate::tf!("{found} field(s) found.", found = 0));
            self.command_line.push_output(&crate::tf!("{updated} field(s) updated.", updated = 0));
            return;
        }
        // The reference counts fields, not the texts holding them, and
        // reports every field it evaluated as updated, changed or not.
        let (changed, found) = self.tabs[i].scene.update_fields(32, Some(&hosts));
        if changed > 0 {
            self.tabs[i].dirty = true;
        }
        self.command_line.push_output(&crate::tf!("{found} field(s) found."));
        self.command_line.push_output(&crate::tf!("{updated} field(s) updated.", updated = found));
    }

    /// A count table at `point`: `Item` | `Count`, one row per block (all
    /// counted blocks when `names` is empty), sorted by name; the counts are
    /// fields.
    fn place_count_table(&mut self, i: usize, point: [f64; 3], names: Vec<String>) {
        let doc = &self.tabs[i].scene.document;
        let area = self.tabs[i].count.as_ref().and_then(|m| m.area.clone());
        let instances = codec::count::block_instances(doc, area.as_deref());
        let mut names: Vec<String> = if names.is_empty() {
            codec::count::block_counts(&instances).into_iter().map(|c| c.name).collect()
        } else {
            names
                .iter()
                .map(|n| doc.block_records.get(n).map(|b| b.name.clone()).unwrap_or_else(|| n.clone()))
                .collect()
        };
        names.sort_by_key(|n| n.to_ascii_uppercase());
        names.dedup_by_key(|n| n.to_ascii_uppercase());
        let mut table = codec::entities::TableBuilder::new(names.len() + 1, 2)
            .at(codec::types::Vector3::new(point[0], point[1], point[2]))
            .row_height(0.36)
            .column_width(3.6)
            .build();
        table.set_column_width(1, 1.08);
        // As the reference writes it: no title row, data rows centred
        // (table and cell overrides), header cells without overrides.
        table.value_flags = 22;
        table.legacy_style_override = Some(codec::entities::table::LegacyTableStyleOverride {
            flags: 0x10001,
            title_suppressed: Some(true),
            row_alignments: vec![5],
            ..Default::default()
        });
        table.set_cell_text(0, 0, "Item");
        table.set_cell_text(0, 1, "Count");
        for (r, name) in names.iter().enumerate() {
            table.set_cell_text(r + 1, 0, name);
            if let Some(cell) = table.cell_mut(r + 1, 1) {
                // The count comes from the field; the cell keeps no text.
                let mut content = codec::entities::table::CellContent::text("");
                content.value.flags = 1;
                cell.contents = vec![content];
                cell.cell_type = codec::entities::table::CellType::Text;
            }
        }
        for (r, row) in table.rows.iter_mut().enumerate() {
            for cell in &mut row.cells {
                if let Some(content) = cell.contents.first_mut().filter(|c| c.value.flags != 1) {
                    content.value.flags = 6;
                }
                if r == 0 {
                    cell.flag = 0x40000;
                } else {
                    cell.flag = 0x40001;
                    let mut style = codec::entities::CellStyle::new();
                    style.override_flags = 0x01;
                    style.alignment = 5;
                    cell.style = Some(style);
                }
            }
        }
        let codes: Vec<String> =
            names.iter().map(|n| self.block_field_code(i, n, &CountKey::default())).collect();
        self.push_undo_snapshot(i, "COUNTTABLE");
        let Some(handle) = self.commit_entity_handle(EntityType::Table(Box::new(table))) else {
            return;
        };
        // The table's graphics block holds one MTEXT per cell; each count
        // cell's MTEXT owns its field, which the cell refers to.
        let doc = &mut self.tabs[i].scene.document;
        let fields = codes
            .iter()
            .enumerate()
            .map(|(r, code)| {
                let value = crate::entities::field::evaluate(doc, code, &[], None).unwrap_or_else(|| "####".into());
                (r + 1, 1, r"%<\_FldIdx 0>%".to_string(), vec![codec::fields::NewField::new(code.clone(), value)])
            })
            .collect();
        doc.build_table_block(handle, fields);
        self.tabs[i].scene.bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
        self.tabs[i].dirty = true;
    }

    /// Zoom to a counted object (← / →, an error of the report).
    fn zoom_to_insert(&mut self, i: usize, h: Handle) {
        let doc = &self.tabs[i].scene.document;
        let Some(entity) = doc.get_entity(h) else { return };
        let outline = codec::count::entity_outline(doc, entity);
        if outline.is_empty() {
            return;
        }
        let (mut lo, mut hi) = (glam::DVec3::splat(f64::MAX), glam::DVec3::splat(f64::MIN));
        for c in outline.iter().flat_map(codec::count::Piece::points) {
            let p = glam::DVec3::new(c.x, c.y, c.z);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        let pad = (hi - lo) * 1.5;
        self.handle_zoom_to_window(lo - pad, hi + pad);
    }

    fn open_count_target(&mut self, i: usize, name: String, key: CountKey) {
        let mut mode = self.tabs[i].count.take().unwrap_or_default();
        let instances = mode.instances(&self.tabs[i].scene.document);
        let reference = instances
            .iter()
            .find(|inst| inst.duplicate_of.is_none() && inst.name.eq_ignore_ascii_case(&name) && key.matches(inst))
            .map(|inst| inst.handle);
        mode.target = Some(CountTarget::Block {
            name,
            reference,
            matching: [key.layer.is_some(), key.scale.is_some(), key.mirror_state.is_some()],
            picked: false,
        });
        mode.cursor = None;
        self.tabs[i].count = Some(mode);
        self.apply_count_display(i);
    }

    /// The Field dialog's Show Count Instances: count mode on what the
    /// expression counts.
    pub(in crate::app) fn show_count_instances(&mut self, i: usize, json: &str) {
        match codec::count::parse_query(json) {
            Some(codec::count::CountQuery::Block { name, key, boundary }) => {
                if let Some(b) = boundary {
                    if let Some(ring) = codec::count::boundary_polygon(&self.tabs[i].scene.document, b) {
                        let mut mode = self.tabs[i].count.take().unwrap_or_default();
                        if mode.boundary.map(|(h, _)| h) != Some(b) {
                            self.drop_count_boundary(i, mode.boundary);
                            mode.boundary = Some((b, false));
                        }
                        mode.area = Some(ring);
                        self.tabs[i].count = Some(mode);
                    }
                }
                self.open_count_target(i, name, key);
            }
            Some(codec::count::CountQuery::Single(handles)) if !handles.is_empty() => {
                let hexes: Vec<String> = handles.iter().map(|h| hex(*h)).collect();
                let area = if self.tabs[i].count.is_some() { "K" } else { "E" };
                self.count_run(i, &format!("{area} T {}", hexes.join(",")));
            }
            _ => {}
        }
    }

    pub(in crate::app) fn on_count(&mut self, m: CountMsg) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            return Task::none();
        }
        match m {
            CountMsg::Search(s) => self.count_palette.search = s,
            CountMsg::Sort(by_count) => {
                if self.count_palette.by_count == by_count {
                    self.count_palette.descending = !self.count_palette.descending;
                } else {
                    self.count_palette.by_count = by_count;
                    self.count_palette.descending = false;
                }
            }
            CountMsg::Open(name, key) | CountMsg::Menu(name, key, RowAction::Review) => {
                if self.count_palette.table.is_none() {
                    self.open_count_target(i, name, key);
                }
            }
            CountMsg::Menu(name, key, RowAction::Field) => {
                let code = self.block_field_code(i, &name, &key);
                return self.start_count_field(i, code);
            }
            CountMsg::Menu(name, _, RowAction::Expand(k)) => {
                let upper = name.to_ascii_uppercase();
                let list = &mut self.count_palette.expanded;
                match list.iter_mut().find(|(n, _)| *n == upper) {
                    Some((_, e)) => e[k] = !e[k],
                    None => {
                        let mut e = [false; 3];
                        e[k] = true;
                        list.push((upper, e));
                    }
                }
            }
            CountMsg::CreateTable => self.count_palette.table = Some(Vec::new()),
            CountMsg::TableCheck(name, on) => {
                if let Some(checked) = self.count_palette.table.as_mut() {
                    let upper = name.to_ascii_uppercase();
                    checked.retain(|n| *n != upper);
                    if on {
                        checked.push(upper);
                    }
                }
            }
            CountMsg::TableAll(on) => {
                let names: Vec<String> = if on {
                    let area = self.tabs[i].count.as_ref().and_then(|m| m.area.clone());
                    let instances = codec::count::block_instances(&self.tabs[i].scene.document, area.as_deref());
                    crate::ui::window::count_palette::rows(&instances, &self.count_palette)
                        .into_iter()
                        .map(|r| r.name.to_ascii_uppercase())
                        .collect()
                } else {
                    Vec::new()
                };
                self.count_palette.table = Some(names);
            }
            CountMsg::TableCancel => self.count_palette.table = None,
            CountMsg::TableInsert => {
                let names = self.count_palette.table.take().unwrap_or_default();
                if names.is_empty() {
                    return Task::none();
                }
                return self.start_count_command(i, Box::new(CountTableCommand::placing(names)));
            }
            CountMsg::Back | CountMsg::Close => self.close_count(i),
            CountMsg::Match(k, on) => {
                if let Some(CountMode { target: Some(CountTarget::Block { matching, .. }), .. }) = self.tabs[i].count.as_mut() {
                    matching[k] = on;
                }
                self.apply_count_display(i);
            }
            CountMsg::ToggleDetails => self.count_palette.details_closed = !self.count_palette.details_closed,
            CountMsg::ToggleErrors => self.count_palette.errors_closed = !self.count_palette.errors_closed,
            CountMsg::ShowError(h) => self.zoom_to_insert(i, h),
            CountMsg::Prev | CountMsg::Next => {
                let Some(mode) = self.tabs[i].count.as_ref() else { return Task::none() };
                let result = mode.result(&self.tabs[i].scene.document);
                let n = result.counted.len();
                if n == 0 {
                    return Task::none();
                }
                let next = match (mode.cursor, matches!(m, CountMsg::Next)) {
                    (None, true) => 0,
                    (None, false) => n - 1,
                    (Some(c), true) => (c + 1) % n,
                    (Some(c), false) => (c + n - 1) % n,
                };
                let target = result.counted[next];
                if let Some(mode) = self.tabs[i].count.as_mut() {
                    mode.cursor = Some(next);
                }
                self.zoom_to_insert(i, target);
            }
            CountMsg::Area => return self.dispatch_command("COUNTAREA"),
            CountMsg::Select => {
                self.tabs[i].scene.deselect_all();
                let command = CountCommand::with_area(if self.tabs[i].count.is_some() { "K" } else { "C" });
                return self.start_count_command(i, Box::new(command));
            }
            CountMsg::Field => return self.dispatch_command("COUNTFIELD"),
            CountMsg::InvalidAlways(v) => self.count_palette.invalid_always = v,
            CountMsg::InvalidUndo => {
                self.close_invalid_area(1);
                self.count_invalid_undo(i);
            }
            CountMsg::InvalidContinue => {
                self.close_invalid_area(2);
                self.count_invalid_continue(i);
            }
        }
        Task::none()
    }
}
