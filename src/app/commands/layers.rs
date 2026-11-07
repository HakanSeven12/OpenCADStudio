use super::*;

fn restore_layiso_layer(
    current: &mut document::tables::Layer,
    saved: &crate::app::tab_document::LayisoLayerSnapshot,
    mode: crate::app::tab_document::LayisoMode,
) -> bool {
    let mut changed = false;
    if current.flags.off == saved.after.flags.off {
        current.flags.off = saved.before.flags.off;
        changed = true;
    }
    if mode == crate::app::tab_document::LayisoMode::LockAndFade
        && current.flags.locked == saved.after.flags.locked
    {
        current.flags.locked = saved.before.flags.locked;
        changed = true;
    }
    changed
}

impl OpenCADStudio {
    /// A command turned one layer off (or back on): one undo step, and the
    /// command stays for its next pick.
    pub(in crate::app) fn set_layer_on_from_command(&mut self, i: usize, layer: &str, on: bool) {
        let names = vec![layer.to_string()];
        let undo = self.begin_layer_undo(i, "LAYOFF", &names);
        if let Some(record) = self.tabs[i].scene.document.layers.get_mut(layer) {
            if on {
                record.turn_on();
            } else {
                record.turn_off();
            }
        }
        self.tabs[i].scene.invalidate_layer_visibility(&names);
        self.tabs[i].dirty = true;
        self.commit_layer_undo(i, undo);
        self.refresh_layer_panel();
        if !on {
            self.command_line
                .push_info(crate::t!("Layer(s) turned off.").as_ref());
        }
        if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt()) {
            self.command_line.push_info(&prompt);
        }
    }

    pub(super) fn dispatch_layers(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        if let Some(task) = self.dispatch_layer_filter(cmd, i) {
            return Some(task);
        }
        if let Some(task) = self.dispatch_layer_group(cmd, i) {
            return Some(task);
        }
        match cmd {
            cmd if cmd.starts_with("GRAPHICATTRIBUTELAYER ") => {
                let name = cmd.trim_start_matches("GRAPHICATTRIBUTELAYER").trim();
                return Some(self.create_graphic_attributes_layer(name));
            }
            // ── Layer object commands ──────────────────────────────────────
            "LAYOFF" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    let cmd = crate::modules::draw::layers::layoff::LayerOffCommand::default();
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let layers: rustc_hash::FxHashSet<String> = self.tabs[i]
                        .scene
                        .selected_entities()
                        .into_iter()
                        .map(|(_, e)| e.common().layer.clone())
                        .collect();
                    let names: Vec<String> = layers.iter().cloned().collect();
                    let undo = self.begin_layer_undo(i, "LAYOFF", &names);
                    for name in &layers {
                        if name == "0" {
                            continue;
                        }
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                            dl.turn_off();
                        }
                    }
                    self.tabs[i].scene.invalidate_layer_visibility(&names);
                    self.tabs[i].dirty = true;
                    self.commit_layer_undo(i, undo);
                    self.refresh_layer_panel();
                    self.command_line
                        .push_info(crate::t!("Layer(s) turned off.").as_ref());
                }
            }

            "LAYFRZ" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("LAYFRZ");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let layers: rustc_hash::FxHashSet<String> = self.tabs[i]
                        .scene
                        .selected_entities()
                        .into_iter()
                        .map(|(_, e)| e.common().layer.clone())
                        .collect();
                    let names: Vec<String> = layers.iter().cloned().collect();
                    let undo = self.begin_layer_undo(i, "LAYFRZ", &names);
                    for name in &layers {
                        if name == "0" {
                            continue;
                        }
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                            dl.freeze();
                        }
                    }
                    self.tabs[i].scene.invalidate_layer_visibility(&names);
                    self.tabs[i].dirty = true;
                    self.commit_layer_undo(i, undo);
                    self.refresh_layer_panel();
                    self.command_line
                        .push_info(crate::t!("Layer(s) frozen.").as_ref());
                }
            }

            // LAYWALK — the non-modal layer walk. The pallet is the whole
            // command: a second LAYWALK closes the one it just opened, the way
            // the ribbon button toggles it.
            "LAYWALK" => return Some(self.toggle_layer_walk()),
            // The follow-up to the pallet's *Select Objects* button: the pick
            // relaunches this name once the objects are in hand.
            "LAYWALK_SELECT" => {
                self.walk_picked_objects();
            }

            // DWGUNITS — change what one drawing unit measures, and convert the
            // model to match. The units pill only relabels, which is all the
            // label is for; this is the one place geometry moves. (#668)
            "DWGUNITS" => {
                use crate::command::KeywordCommand;
                use crate::modules::draw::units;
                // Buttons read as full names; the token behind each is the
                // abbreviation, since that is what gets typed. Unitless is left
                // off — there is nothing to convert to.
                let choices: Vec<(&str, &str, Option<&str>)> = units::all()
                    .filter(|&(code, _)| code != 0)
                    .map(|(code, label)| (label, units::short(code), None))
                    .collect();
                let current = crate::t!(units::label(
                    self.tabs[i].scene.document.header.insertion_units
                ));
                self.command_line
                    .push_info(crate::tf!("DWGUNITS  current unit: {current}").as_ref());
                let c = KeywordCommand::new("DWGUNITS", "DWGUNITS  Convert drawing to:", choices);
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("DWGUNITS ") => {
                return Some(self.convert_drawing_units(cmd.trim_start_matches("DWGUNITS").trim()));
            }

            // LAYTRANS — translate this drawing's layers onto a set taken from
            // another drawing. Bare opens the mapping dialog; with a file it
            // maps every name the two drawings share and translates at once,
            // which is what most drawings coming from outside need. (#624)
            "LAYTRANS" => {
                return Some(self.open_layer_translator(None));
            }
            cmd if cmd.starts_with("LAYTRANS ") => {
                let path = std::path::PathBuf::from(cmd.trim_start_matches("LAYTRANS").trim());
                return Some(self.layer_translate_same(&path));
            }
            // -LAYTRANS — the scriptable translator. It never opens the
            // dialog, so a script can map source layers onto a standards file
            // (or a saved mapping list) and finish in one line.
            //   -LAYTRANS <standards> [FROM=TO …]   translate (same names when no pairs)
            //   -LAYTRANS SAVE <file> FROM=TO …     write a mapping list
            //   -LAYTRANS LOAD <file> [<standards>] read a list and translate
            //   -LAYTRANS LIST <standards>          show the target layers
            cmd if cmd.eq_ignore_ascii_case("-LAYTRANS")
                || cmd.to_ascii_uppercase().starts_with("-LAYTRANS ") =>
            {
                let rest = cmd
                    .trim_start_matches('-')
                    .split_once(char::is_whitespace)
                    .map(|(_, tail)| tail.trim())
                    .unwrap_or("");
                return Some(self.layer_translate_scripted(rest));
            }

            // LAYDEL <name> — delete a layer and erase the objects on it.
            "LAYDEL" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new("LAYDEL", "LAYDEL  layer to delete:");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("LAYDEL ") => {
                let name = cmd.trim_start_matches("LAYDEL").trim();
                if name.is_empty() {
                    self.command_line
                        .push_info(crate::t!("Usage: LAYDEL <layer name>").as_ref());
                    return Some(Task::none());
                }
                let resolved = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .names()
                    .find(|k| k.eq_ignore_ascii_case(name))
                    .map(|s| s.to_string());
                let Some(layer) = resolved else {
                    self.command_line
                        .push_error(crate::tf!("LAYDEL: no layer named \"{name}\".").as_ref());
                    return Some(Task::none());
                };
                if layer == "0" {
                    self.command_line
                        .push_error(crate::t!("LAYDEL: layer \"0\" cannot be deleted.").as_ref());
                    return Some(Task::none());
                }
                if layer.eq_ignore_ascii_case(&self.tabs[i].active_layer) {
                    self.command_line.push_error(
                        crate::t!("LAYDEL: cannot delete the current layer. Make another layer current first.").as_ref(),
                    );
                    return Some(Task::none());
                }
                let handles: Vec<document::Handle> = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter(|e| e.common().layer == layer)
                    .map(|e| e.common().handle)
                    .collect();
                self.push_undo_snapshot(i, "LAYDEL");
                let n = handles.len();
                if !handles.is_empty() {
                    self.tabs[i].scene.erase_entities(&handles);
                }
                self.tabs[i].scene.document.layers.remove(&layer);
                self.tabs[i].scene.bump_geometry();
                self.tabs[i].dirty = true;
                self.refresh_layer_panel();
                self.command_line.push_output(
                    crate::tf!("LAYDEL: deleted layer \"{layer}\" and {n} object(s).").as_ref(),
                );
            }

            // Interactive LAYMRG gathers source layers and confirms a target.
            // LAYMRG <source> <target> keeps the immediate form for scripts.
            "LAYMRG" => {
                let selected = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                let (c, message) = crate::modules::draw::layers::laymrg::LayMrgCommand::new(
                    &self.tabs[i].scene,
                    &self.tabs[i].active_layer,
                    selected,
                );
                if let Some(message) = message {
                    self.command_line.push_info(&message);
                }
                self.command_line.push_info(&c.prompt());
                self.command_line.set_step_options(c.options());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("LAYMRG ") => {
                let rest = cmd.trim_start_matches("LAYMRG").trim();
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() != 2 {
                    self.command_line.push_info(
                        crate::t!("Usage: LAYMRG <source layer> <target layer>").as_ref(),
                    );
                    return Some(Task::none());
                }
                let keys: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .names()
                    .map(|s| s.to_string())
                    .collect();
                let src = keys
                    .iter()
                    .find(|k| k.eq_ignore_ascii_case(parts[0]))
                    .cloned();
                let dst = keys
                    .iter()
                    .find(|k| k.eq_ignore_ascii_case(parts[1]))
                    .cloned();
                let (Some(src), Some(dst)) = (src, dst) else {
                    self.command_line.push_error(
                        crate::t!("LAYMRG: source and target layers must both exist.").as_ref(),
                    );
                    return Some(Task::none());
                };
                if src == dst {
                    self.command_line.push_error(
                        crate::t!("LAYMRG: source and target are the same layer.").as_ref(),
                    );
                    return Some(Task::none());
                }
                if src == "0" {
                    self.command_line.push_error(
                        crate::t!("LAYMRG: layer \"0\" cannot be merged away.").as_ref(),
                    );
                    return Some(Task::none());
                }
                if src.eq_ignore_ascii_case(&self.tabs[i].active_layer) {
                    self.command_line.push_error(
                        crate::t!("LAYMRG: cannot merge the current layer. Make another layer current first.").as_ref(),
                    );
                    return Some(Task::none());
                }
                self.merge_layers(i, vec![src], dst);
            }

            // LAYERSTATE — save / restore named snapshots of all layer states
            // in the drawing's native ACAD_LAYERSTATES dictionary.
            // LAYERSTATE SAVE <name> | RESTORE <name> [filter] | RENAME <old> <new>
            //            DELETE <name> | EXPORT <name> <file.las> | IMPORT <file.las>
            // The `-LAYERSTATE` spelling is the scriptable one and never opens
            // the manager dialog.
            "LAYERSTATE" | "LAS" | "LMAN" => {
                return Some(Task::done(Message::LayerStateManagerOpen));
            }
            cmd if cmd.eq_ignore_ascii_case("-LAYERSTATE")
                || cmd.to_ascii_uppercase().starts_with("-LAYERSTATE ")
                || cmd.starts_with("LAYERSTATE ")
                || cmd.starts_with("LAS ")
                || cmd.starts_with("LMAN ") =>
            {
                let line = cmd.trim_start_matches('-');
                let rest = line
                    .trim_start_matches("LAYERSTATE")
                    .trim_start_matches("LMAN")
                    .trim_start_matches("LAS")
                    .trim();
                let mut parts = rest.splitn(2, char::is_whitespace);
                let sub = parts.next().unwrap_or("").to_uppercase();
                let arg_text = parts.next().unwrap_or("").trim();
                let (arg, tail) = match arg_text.split_once(char::is_whitespace) {
                    Some((head, tail)) => (head, tail.trim()),
                    None => (arg_text, ""),
                };
                match sub.as_str() {
                    "" | "?" | "LIST" => {
                        let states = self.tabs[i].scene.document.layer_states();
                        if states.is_empty() {
                            self.command_line.push_info(
                                crate::t!(
                                    "LAYERSTATE: no saved states. Use LAYERSTATE SAVE <name>."
                                )
                                .as_ref(),
                            );
                        } else {
                            let mut names: Vec<&str> =
                                states.iter().map(|state| state.name.as_str()).collect();
                            names.sort_unstable();
                            self.command_line.push_output(
                                crate::tf!("Saved layer states: {}", names.join(", ")).as_ref(),
                            );
                        }
                    }
                    "SAVE" | "S" => {
                        if arg.is_empty() {
                            self.command_line
                                .push_info(crate::t!("Usage: LAYERSTATE SAVE <name>").as_ref());
                        } else {
                            let description = self.tabs[i]
                                .scene
                                .document
                                .layer_state(arg)
                                .map(|state| state.description)
                                .unwrap_or_default();
                            self.push_undo_snapshot(i, "LAYERSTATE SAVE");
                            self.tabs[i]
                                .scene
                                .document
                                .capture_layer_state(arg, description);
                            self.tabs[i].dirty = true;
                            self.command_line
                                .push_output(crate::tf!("LAYERSTATE: saved \"{arg}\".").as_ref());
                        }
                    }
                    "RESTORE" | "R" => {
                        if arg.is_empty() {
                            self.command_line
                                .push_info(crate::t!("Usage: LAYERSTATE RESTORE <name> [filter]").as_ref());
                        } else if self.tabs[i].scene.document.layer_state(arg).is_none() {
                            self.command_line.push_error(
                                crate::tf!("LAYERSTATE: no saved state named \"{arg}\".").as_ref(),
                            );
                        } else {
                            let mask = match parse_layer_state_filter(tail) {
                                Ok(mask) => mask,
                                Err(error) => {
                                    self.command_line.push_error(&error);
                                    return Some(self.finish_dispatch(cmd));
                                }
                            };
                            let names: Vec<String> = self.tabs[i]
                                .scene
                                .document
                                .layers
                                .iter()
                                .map(|layer| layer.name.clone())
                                .collect();
                            self.push_undo_snapshot(i, "LAYERSTATE RESTORE");
                            let n = self.restore_layer_state_filtered(i, arg, mask);
                            self.tabs[i].active_layer = self.tabs[i]
                                .scene
                                .document
                                .header
                                .current_layer_name
                                .clone();
                            self.tabs[i].scene.invalidate_layer_dependencies(&names);
                            self.tabs[i].dirty = true;
                            self.refresh_layer_panel();
                            let filtered = if mask.is_some() { " [filtered]" } else { "" };
                            self.command_line.push_output(
                                crate::tf!(
                                    "LAYERSTATE: restored \"{arg}\" ({n} layer(s)){filtered}."
                                )
                                .as_ref(),
                            );
                        }
                    }
                    "RENAME" | "REN" => {
                        let new_name = tail.trim_matches('"');
                        if arg.is_empty() || new_name.is_empty() {
                            self.command_line.push_info(
                                crate::t!("Usage: LAYERSTATE RENAME <old> <new>").as_ref(),
                            );
                        } else if self.tabs[i].scene.document.layer_state(arg).is_none() {
                            self.command_line.push_error(
                                crate::tf!("LAYERSTATE: no saved state named \"{arg}\".").as_ref(),
                            );
                        } else if self.tabs[i]
                            .scene
                            .document
                            .layer_state(new_name)
                            .is_some()
                        {
                            self.command_line.push_error(
                                crate::tf!("LAYERSTATE: a state named \"{new_name}\" already exists.")
                                    .as_ref(),
                            );
                        } else {
                            self.push_undo_snapshot(i, "LAYERSTATE RENAME");
                            if self
                                .tabs[i]
                                .scene
                                .document
                                .rename_layer_state(arg, new_name)
                            {
                                self.tabs[i].dirty = true;
                                self.command_line.push_output(
                                    crate::tf!("LAYERSTATE: renamed \"{arg}\" to \"{new_name}\".")
                                        .as_ref(),
                                );
                            } else {
                                self.command_line.push_error(
                                    crate::tf!("LAYERSTATE: could not rename \"{arg}\".").as_ref(),
                                );
                            }
                        }
                    }
                    "DELETE" | "D" => {
                        if arg.is_empty() {
                            self.command_line
                                .push_info(crate::t!("Usage: LAYERSTATE DELETE <name>").as_ref());
                        } else if self.tabs[i].scene.document.layer_state(arg).is_none() {
                            self.command_line.push_error(
                                crate::tf!("LAYERSTATE: no saved state named \"{arg}\".").as_ref(),
                            );
                        } else {
                            self.push_undo_snapshot(i, "LAYERSTATE DELETE");
                            self.tabs[i].scene.document.delete_layer_state(arg);
                            self.tabs[i].dirty = true;
                            self.command_line
                                .push_output(crate::tf!("LAYERSTATE: deleted \"{arg}\".").as_ref());
                        }
                    }
                    "EXPORT" | "E" => {
                        let path_text = tail.trim().trim_matches('"');
                        if arg.is_empty() || path_text.is_empty() {
                            self.command_line.push_info(
                                crate::t!("Usage: LAYERSTATE EXPORT <name> <file.las>").as_ref(),
                            );
                        } else {
                            match self.tabs[i].scene.document.layer_state(arg) {
                                Some(state) => {
                                    match std::fs::write(path_text, layer_state_to_las(&state)) {
                                        Ok(()) => self.command_line.push_output(
                                            crate::tf!("LAYERSTATE: exported \"{arg}\" to {path_text}.")
                                                .as_ref(),
                                        ),
                                        Err(error) => self.command_line.push_error(
                                            crate::tf!("LAYERSTATE: {error}").as_ref(),
                                        ),
                                    }
                                }
                                None => self.command_line.push_error(
                                    crate::tf!("LAYERSTATE: no saved state named \"{arg}\".").as_ref(),
                                ),
                            }
                        }
                    }
                    "IMPORT" | "I" => {
                        let path_text = arg_text.trim().trim_matches('"');
                        if path_text.is_empty() {
                            self.command_line.push_info(
                                crate::t!("Usage: LAYERSTATE IMPORT <file.las>").as_ref(),
                            );
                        } else {
                            match std::fs::read_to_string(path_text)
                                .map_err(|error| error.to_string())
                                .and_then(|text| layer_state_from_las(&text))
                            {
                                Ok(state) => {
                                    let name = state.name.clone();
                                    self.push_undo_snapshot(i, "LAYERSTATE IMPORT");
                                    self.tabs[i].scene.document.store_layer_state(state);
                                    self.tabs[i].dirty = true;
                                    self.command_line.push_output(
                                        crate::tf!("LAYERSTATE: imported \"{name}\" from {path_text}.")
                                            .as_ref(),
                                    );
                                }
                                Err(error) => self.command_line.push_error(
                                    crate::tf!("LAYERSTATE: {error}").as_ref(),
                                ),
                            }
                        }
                    }
                    _ => {
                        self.command_line.push_info(
                            crate::t!(
                                "Usage: LAYERSTATE SAVE|RESTORE|RENAME|DELETE|EXPORT|IMPORT <name> | ? (list)"
                            )
                            .as_ref(),
                        );
                    }
                }
            }

            "LAYLCK" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("LAYLCK");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let layers: rustc_hash::FxHashSet<String> = self.tabs[i]
                        .scene
                        .selected_entities()
                        .into_iter()
                        .map(|(_, e)| e.common().layer.clone())
                        .collect();
                    let names: Vec<String> = layers.iter().cloned().collect();
                    let undo = self.begin_layer_undo(i, "LAYLCK", &names);
                    for name in &layers {
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                            dl.lock();
                        }
                    }
                    // Layer locking changes the Model-view display colour as
                    // well as editability, including block dependencies.
                    self.tabs[i].scene.invalidate_layer_dependencies(&names);
                    self.tabs[i].dirty = true;
                    self.commit_layer_undo(i, undo);
                    self.refresh_layer_panel();
                    self.refresh_properties();
                    self.command_line
                        .push_info(crate::t!("Layer(s) locked.").as_ref());
                }
            }

            "LAYMCUR" => {
                let entities = self.tabs[i].scene.selected_entities();
                if entities.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    // LAYMCUR acts on one object's layer; apply on the first pick.
                    let cmd = SelectObjectsCommand::instant("LAYMCUR");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let layer = entities[0].1.common().layer.clone();
                    // Keep the document header (CLAYER) in sync, not just the
                    // per-tab default, so a later no-selection ribbon refresh
                    // (e.g. after Esc) doesn't snap back to the stale header
                    // layer. See #93.
                    let handle = self.tabs[i]
                        .scene
                        .document
                        .layers
                        .get(&layer)
                        .map(|l| l.handle)
                        .unwrap_or(document::types::Handle::NULL);
                    self.tabs[i].scene.document.header.current_layer_name = layer.clone();
                    self.tabs[i].scene.document.header.current_layer_handle = handle;
                    self.tabs[i].active_layer = layer.clone();
                    self.ribbon.active_layer = layer.clone();
                    self.tabs[i].layers.current_layer = layer.clone();
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_info(crate::tf!("Current layer set to \"{layer}\".").as_ref());
                    self.refresh_layer_panel();
                }
            }

            "LAYON" => {
                let names = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
                    .map(|l| l.name.clone())
                    .collect::<Vec<_>>();
                let undo = self.begin_layer_undo(i, "LAYON", &names);
                for name in &names {
                    if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(&name) {
                        dl.turn_on();
                    }
                }
                self.tabs[i].scene.invalidate_layer_visibility(&names);
                self.tabs[i].dirty = true;
                self.commit_layer_undo(i, undo);
                self.refresh_layer_panel();
                self.command_line
                    .push_info(crate::t!("All layers turned on.").as_ref());
            }

            "LAYTHW" => {
                let names = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
                    .map(|l| l.name.clone())
                    .collect::<Vec<_>>();
                let undo = self.begin_layer_undo(i, "LAYTHW", &names);
                for name in &names {
                    if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(&name) {
                        dl.thaw();
                    }
                }
                self.tabs[i].scene.invalidate_layer_visibility(&names);
                self.tabs[i].dirty = true;
                self.commit_layer_undo(i, undo);
                self.refresh_layer_panel();
                self.command_line
                    .push_info(crate::t!("All layers thawed.").as_ref());
            }

            "LAYULK" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("LAYULK");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let layers: rustc_hash::FxHashSet<String> = self.tabs[i]
                        .scene
                        .selected_entities()
                        .into_iter()
                        .map(|(_, e)| e.common().layer.clone())
                        .collect();
                    let names: Vec<String> = layers.iter().cloned().collect();
                    let undo = self.begin_layer_undo(i, "LAYULK", &names);
                    for name in &layers {
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                            dl.unlock();
                        }
                    }
                    // Layer unlocking restores the Model-view display colour
                    // as well as editability, including block dependencies.
                    self.tabs[i].scene.invalidate_layer_dependencies(&names);
                    self.tabs[i].dirty = true;
                    self.commit_layer_undo(i, undo);
                    self.refresh_layer_panel();
                    self.refresh_properties();
                    self.command_line
                        .push_info(crate::t!("Layer(s) unlocked.").as_ref());
                }
            }

            // LAYISO — isolate the layers used by the selected entities.
            "LAYISO" => self.start_layiso(i),
            "LAYERP" => self.layer_previous(i),

            // LAYISO Settings is kept as a command-line subcommand so it is
            // available both from the selection prompt and from scripts.
            "LAYISO SETTINGS" => {
                use crate::command::KeywordCommand;
                let command = KeywordCommand::new(
                    "LAYISO SETTINGS",
                    "LAYISO  Settings [Off / Lock and Fade]:",
                    vec![
                        ("Off", "OFF", None),
                        ("Lock and Fade", "LOCKANDFADE", None),
                    ],
                );
                self.command_line.push_info(&command.prompt());
                self.command_line.set_step_options(command.options());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            "LAYISO SETTINGS OFF" => {
                self.tabs[i].layiso_mode = crate::app::tab_document::LayisoMode::Off;
                self.command_line
                    .push_info(crate::t!("LAYISO Settings: Off.").as_ref());
                self.start_layiso(i);
            }

            "LAYISO SETTINGS LOCKANDFADE" => {
                self.tabs[i].layiso_mode = crate::app::tab_document::LayisoMode::LockAndFade;
                self.command_line
                    .push_info(crate::t!("LAYISO Settings: Lock and Fade.").as_ref());
                self.start_layiso(i);
            }

            // ISOLATEOBJECTS — hide every object except the current selection
            "ISOLATEOBJECTS" => {
                if self.tabs[i].scene.selected.is_empty() {
                    self.command_line.push_error(
                        crate::t!("ISOLATEOBJECTS: select the objects to isolate first.").as_ref(),
                    );
                } else {
                    let n = self.tabs[i].scene.selected.len();
                    let before = self.tabs[i].scene.object_isolation.clone();
                    let selected_before = self.tabs[i].scene.selected.iter().copied().collect();
                    self.tabs[i].scene.isolate_selected();
                    self.push_object_visibility_history(
                        i,
                        "ISOLATEOBJECTS",
                        before,
                        selected_before,
                    );
                    self.command_line.push_info(
                        crate::tf!("Isolated {n} object(s). UNISOLATEOBJECTS to restore.").as_ref(),
                    );
                }
            }

            // HIDEOBJECTS — hide the current selection
            "HIDEOBJECTS" => {
                if self.tabs[i].scene.selected.is_empty() {
                    self.command_line.push_error(
                        crate::t!("HIDEOBJECTS: select the objects to hide first.").as_ref(),
                    );
                } else {
                    let n = self.tabs[i].scene.selected.len();
                    let before = self.tabs[i].scene.object_isolation.clone();
                    let selected_before = self.tabs[i].scene.selected.iter().copied().collect();
                    self.tabs[i].scene.hide_selected();
                    self.push_object_visibility_history(i, "HIDEOBJECTS", before, selected_before);
                    self.refresh_properties();
                    self.command_line.push_info(
                        crate::tf!("Hid {n} object(s). UNISOLATEOBJECTS to restore.").as_ref(),
                    );
                }
            }

            // UNISOLATEOBJECTS — bring back everything hidden by Isolate / Hide,
            // including objects the drawing was saved with as invisible (e.g.
            // hidden by another CAD program's HIDEOBJECTS).
            "UNISOLATEOBJECTS" => {
                let session = self.tabs[i].scene.is_isolation_active();
                if session {
                    let before = self.tabs[i].scene.object_isolation.clone();
                    let selected_before = self.tabs[i].scene.selected.iter().copied().collect();
                    self.tabs[i].scene.end_isolation();
                    self.push_object_visibility_history(
                        i,
                        "UNISOLATEOBJECTS",
                        before,
                        selected_before,
                    );
                }
                let dirty_before = self.tabs[i].dirty;
                let originals = self.tabs[i].scene.show_persisted_hidden_objects();
                let shown = !originals.is_empty();
                if shown {
                    self.push_entity_group_history(
                        i,
                        "UNISOLATEOBJECTS",
                        originals,
                        Vec::new(),
                        dirty_before,
                    );
                    self.tabs[i].dirty = true;
                }
                if session || shown {
                    self.command_line
                        .push_info(crate::t!("Isolation ended — all objects shown.").as_ref());
                } else {
                    self.command_line
                        .push_info(crate::t!("No hidden objects.").as_ref());
                }
            }

            // LAYUNISO — restore the last LAYISO state for this drawing.
            "LAYUNISO" => {
                let Some(state) = self.tabs[i].layiso_state.clone() else {
                    self.command_line.push_info(
                        crate::t!("LAYUNISO: no layers were isolated by LAYISO.").as_ref(),
                    );
                    return Some(Task::none());
                };
                let names: Vec<String> = state
                    .layers
                    .iter()
                    .map(|layer| layer.name.clone())
                    .collect();
                let layiso_history_before = self.tabs[i].layiso_state.clone();
                let history_len = self.tabs[i].history.undo_stack.len();
                let undo = self.begin_layer_undo(i, "LAYUNISO", &names);
                let mut changed = Vec::new();
                for saved in &state.layers {
                    let Some(current) = self.tabs[i].scene.document.layers.get_mut(&saved.name)
                    else {
                        continue;
                    };
                    // Restore only fields that still have the value LAYISO
                    // wrote. This preserves independent edits made after
                    // LAYISO where the current and saved values differ.
                    if restore_layiso_layer(current, saved, state.mode) {
                        changed.push(saved.name.clone());
                    }
                }
                if !changed.is_empty() {
                    self.tabs[i].scene.invalidate_layer_visibility(&changed);
                    self.tabs[i].scene.invalidate_layer_dependencies(&changed);
                    self.tabs[i].dirty = true;
                }
                self.tabs[i].layiso_state = None;
                self.commit_layer_undo(i, undo);
                self.record_layiso_history(
                    i,
                    history_len,
                    layiso_history_before,
                    None,
                );
                self.refresh_layer_panel();
                self.command_line
                    .push_info(crate::t!("LAYUNISO: all layers restored.").as_ref());
            }

            "QLAYER" => {
                use crate::modules::draw::layers::quick_layer::{
                    QuickLayerCommand, QuickLayerEntry,
                };

                let layers: Vec<QuickLayerEntry> = self.tabs[i]
                    .layers
                    .layers
                    .iter()
                    .filter(|layer| !layer.name.starts_with('*'))
                    .map(|layer| {
                        QuickLayerEntry::new(
                            layer.name.clone(),
                            layer.color.clone(),
                            layer.visible,
                            layer.frozen,
                            layer.locked,
                        )
                    })
                    .collect();

                let selected: Vec<document::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(handle, _)| handle)
                    .collect();

                let command = QuickLayerCommand::new_with_entries(layers, selected);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            cmd if cmd.starts_with("QLAYER_APPLY_HEX ") => {
                use crate::modules::draw::layers::quick_layer::QuickLayerCommand;

                let encoded = cmd.trim_start_matches("QLAYER_APPLY_HEX").trim();

                let Some(requested) = QuickLayerCommand::decode_layer_name(encoded) else {
                    self.command_line
                        .push_error(crate::t!("QLAYER: invalid encoded layer name.").as_ref());
                    return Some(Task::none());
                };

                let resolved = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .names()
                    .find(|name| name.eq_ignore_ascii_case(&requested))
                    .map(|name| name.to_string());

                let Some(layer) = resolved else {
                    self.command_line
                        .push_error(crate::tf!("QLAYER: no layer named \"{requested}\".").as_ref());
                    return Some(Task::none());
                };

                let handles: Vec<document::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(handle, _)| handle)
                    .collect();

                if handles.is_empty() {
                    match self.set_current_layer_name(i, &layer) {
                        Ok(()) => {
                            self.command_line.push_info(
                                crate::tf!("QLAYER: current layer set to \"{layer}\".").as_ref(),
                            );
                        }
                        Err(error) => {
                            self.command_line.push_error(&error);
                        }
                    }
                } else {
                    let count = handles.len();

                    self.apply_property_op(i, "QLAYER", &handles, |app, handle| {
                        if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                            crate::scene::view::dispatch::apply_common_prop(
                                entity, "layer", &layer,
                            );
                        }
                    });

                    self.ribbon.active_layer = layer.clone();

                    self.command_line.push_info(
                        crate::tf!("QLAYER: moved {count} object(s) to layer \"{layer}\".")
                            .as_ref(),
                    );
                }
            }

            // LAYCUR: the selected objects move to the current layer.
            "LAYCUR" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("LAYCUR");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let layer = self.tabs[i]
                        .scene
                        .document
                        .header
                        .current_layer_name
                        .clone();
                    let editable: Vec<_> = handles
                        .into_iter()
                        .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                        .collect();
                    self.push_undo_snapshot(i, "LAYCUR");
                    for handle in &editable {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(*handle) {
                            entity.common_mut().layer = layer.clone();
                        }
                    }
                    // The new layer's ByLayer colour, linetype and lineweight
                    // show at once.
                    self.invalidate_property_targets(i, &editable);
                    self.tabs[i].dirty = true;
                    self.tabs[i].scene.deselect_all();
                    self.command_line.push_info(
                        crate::tf!("{} object(s) changed to layer \"{layer}\".", editable.len())
                            .as_ref(),
                    );
                }
            }

            "LAYMATCH" | "LAYMCH" | "COPYTOLAYER" => {
                use crate::modules::draw::layers::match_layer::LayMatchCommand;
                let dest: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                // The preselection is consumed so it cannot become the
                // source object picked next.
                self.tabs[i].scene.deselect_all();
                let cmd = if cmd == "COPYTOLAYER" {
                    LayMatchCommand::copy_to_layer(dest)
                } else {
                    LayMatchCommand::new(dest)
                };
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "MATCHPROP" => {
                use crate::modules::draw::properties::match_prop::MatchPropCommand;
                // One preselected object is the source. Multiple objects are
                // ambiguous, so keep the explicit source-pick prompt for them.
                let src = match self.tabs[i].scene.selected_entities().as_slice() {
                    [(handle, _)] => *handle,
                    _ => document::Handle::NULL,
                };
                // Consume the preselection so it cannot become a destination.
                self.tabs[i].scene.deselect_all();
                let last = self.tabs[i].last_matchprop_src;
                let last = match self.tabs[i].scene.document.get_entity(last) {
                    Some(_) => last,
                    None => document::Handle::NULL,
                };
                let cmd = MatchPropCommand::new(src).with_last(last);
                if !src.is_null() {
                    self.command_line.push_output(
                        &crate::entities::match_props::active_settings_line(
                            crate::entities::match_props::match_mode(),
                        ),
                    );
                    self.command_line.set_step_options(cmd.options());
                }
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
                self.refresh_active_cmd_preview(i);
            }

            // Named group editing: rename a group, or add/remove members
            // without dissolving it. `GROUP` on its own still creates one.
            cmd if cmd.starts_with("GROUP ") => {
                let rest = cmd.strip_prefix("GROUP").unwrap_or("").trim();
                let parts: Vec<&str> = rest.split_whitespace().collect();
                let sub = parts
                    .first()
                    .map(|p| p.to_ascii_uppercase())
                    .unwrap_or_default();
                match sub.as_str() {
                    "LIST" | "?" => {
                        let mut rows: Vec<String> = self.tabs[i]
                            .scene
                            .groups()
                            .map(|group| format!("{} ({} object(s))", group.name, group.entities.len()))
                            .collect();
                        rows.sort();
                        if rows.is_empty() {
                            self.command_line
                                .push_info(crate::t!("GROUP: no groups in this drawing.").as_ref());
                        } else {
                            self.command_line.push_output(
                                &crate::tf!("Groups:\n  {}", rows.join("\n  ")).as_ref(),
                            );
                        }
                    }
                    "RENAME" | "NAME" => {
                        let old = parts.get(1).copied().unwrap_or("");
                        let new = parts.get(2).copied().unwrap_or("");
                        if old.is_empty() || new.is_empty() {
                            self.command_line.push_error(
                                crate::t!("Usage: GROUP RENAME <old name> <new name>").as_ref(),
                            );
                        } else if !self.tabs[i]
                            .scene
                            .groups()
                            .any(|group| group.name.eq_ignore_ascii_case(old))
                        {
                            self.command_line
                                .push_error(&crate::tf!("GROUP: no group named '{old}'.").as_ref());
                        } else if self.tabs[i]
                            .scene
                            .groups()
                            .any(|group| group.name.eq_ignore_ascii_case(new))
                        {
                            self.command_line.push_error(
                                &crate::tf!("GROUP: a group named '{new}' already exists.").as_ref(),
                            );
                        } else {
                            let undo = self.begin_group_undo(i, "GROUP RENAME");
                            let renamed = self.tabs[i].scene.rename_group_named(old, new);
                            self.tabs[i].dirty = true;
                            self.commit_group_undo(i, undo);
                            if renamed {
                                self.command_line.push_output(
                                    &crate::tf!("GROUP: renamed '{old}' to '{new}'.").as_ref(),
                                );
                            }
                        }
                    }
                    "ADD" | "REMOVE" => {
                        let adding = sub == "ADD";
                        let name = parts.get(1).copied().unwrap_or("");
                        if name.is_empty() {
                            self.command_line.push_error(
                                &crate::tf!(
                                    "Usage: GROUP {} <group name> [handle ...]",
                                    if adding { "ADD" } else { "REMOVE" }
                                )
                                .as_ref(),
                            );
                        } else if !self.tabs[i]
                            .scene
                            .groups()
                            .any(|group| group.name.eq_ignore_ascii_case(name))
                        {
                            self.command_line
                                .push_error(&crate::tf!("GROUP: no group named '{name}'.").as_ref());
                        } else {
                            let mut handles: Vec<document::Handle> = parts
                                .get(2..)
                                .unwrap_or(&[])
                                .iter()
                                .filter_map(|token| {
                                    u64::from_str_radix(
                                        token.trim_start_matches(['_', '.']),
                                        16,
                                    )
                                    .ok()
                                    .map(document::Handle::new)
                                })
                                .collect();
                            if handles.is_empty() {
                                handles = self.tabs[i]
                                    .scene
                                    .selected_entities()
                                    .into_iter()
                                    .map(|(handle, _)| handle)
                                    .collect();
                            }
                            let members = self.tabs[i]
                                .scene
                                .groups()
                                .find(|group| group.name.eq_ignore_ascii_case(name))
                                .map(|group| group.entities.clone())
                                .unwrap_or_default();
                            handles.retain(|handle| {
                                adding != members.contains(handle)
                                    && self.tabs[i].scene.document.get_entity(*handle).is_some()
                            });
                            if handles.is_empty() {
                                self.command_line.push_info(
                                    &crate::tf!(
                                        "GROUP: nothing to {}.",
                                        if adding { "add" } else { "remove" }
                                    )
                                    .as_ref(),
                                );
                            } else {
                                let undo = self.begin_group_undo(
                                    i,
                                    if adding { "GROUP ADD" } else { "GROUP REMOVE" },
                                );
                                let mut changed = 0usize;
                                for handle in handles {
                                    let ok = if adding {
                                        self.tabs[i]
                                            .scene
                                            .add_entity_to_group_named(name, handle)
                                    } else {
                                        self.tabs[i]
                                            .scene
                                            .remove_entity_from_group_named(name, handle)
                                    };
                                    if ok {
                                        changed += 1;
                                    }
                                }
                                self.tabs[i].dirty = true;
                                self.commit_group_undo(i, undo);
                                self.command_line.push_output(
                                    &crate::tf!(
                                        "GROUP: {} {} object(s) {} '{name}'.",
                                        if adding { "added" } else { "removed" },
                                        changed,
                                        if adding { "to" } else { "from" }
                                    )
                                    .as_ref(),
                                );
                            }
                        }
                    }
                    _ => {
                        self.command_line.push_error(
                            crate::t!(
                                "Usage: GROUP [LIST | RENAME <old> <new> | ADD <name> [handle ...] | REMOVE <name> [handle ...]]"
                            )
                            .as_ref(),
                        );
                    }
                }
            }

            "GROUP" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("GROUP");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let auto_name =
                        super::super::helpers::next_group_auto_name(&self.tabs[i].scene);
                    use crate::modules::draw::groups::group::GroupCommand;
                    let cmd = GroupCommand::new(handles, auto_name);
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            "UNGROUP" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::groups::ungroup::UngroupCommand;
                    let cmd = UngroupCommand::new();
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let undo = self.begin_group_undo(i, "UNGROUP");
                    let count = self.tabs[i].scene.delete_groups_containing(&handles);
                    self.tabs[i].dirty = true;
                    self.commit_group_undo(i, undo);
                    if count > 0 {
                        self.command_line
                            .push_info(crate::tf!("{} group(s) dissolved.", count).as_ref());
                    } else {
                        self.command_line
                            .push_info(crate::t!("No groups found for selected objects.").as_ref());
                    }
                }
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }
}

#[cfg(test)]
mod layiso_tests {
    use super::*;
    use crate::app::tab_document::{LayisoLayerSnapshot, LayisoMode};
    use document::tables::Layer;
    use document::types::Transparency;

    fn snapshot(before: Layer, after: Layer) -> LayisoLayerSnapshot {
        LayisoLayerSnapshot {
            name: before.name.clone(),
            before,
            after,
        }
    }

    #[test]
    fn off_mode_restores_an_originally_on_layer() {
        let before = Layer::new("A");
        let mut after = before.clone();
        after.turn_off();
        let saved = snapshot(before, after.clone());

        assert!(restore_layiso_layer(&mut after, &saved, LayisoMode::Off));
        assert!(!after.is_off());
    }

    #[test]
    fn off_mode_preserves_an_originally_off_layer() {
        let mut before = Layer::new("A");
        before.turn_off();
        let after = before.clone();
        let mut current = after.clone();
        let saved = snapshot(before, after);

        assert!(restore_layiso_layer(&mut current, &saved, LayisoMode::Off));
        assert!(current.is_off());
    }

    #[test]
    fn lock_and_fade_restores_only_its_own_fields() {
        let before = Layer::new("A");
        let mut after = before.clone();
        after.lock();
        after.transparency = Transparency::from_percent(0.5);
        let saved = snapshot(before, after.clone());

        // An unrelated transparency edit after LAYISO must survive, while
        // the lock still returns to its original state.
        after.transparency = Transparency::from_percent(0.2);
        assert!(restore_layiso_layer(
            &mut after,
            &saved,
            LayisoMode::LockAndFade
        ));
        assert!(!after.is_locked());
        assert_eq!(after.transparency, Transparency::from_percent(0.2));
    }
}

impl OpenCADStudio {
    /// Translate every layer the two drawings name alike, in one step.
    ///
    /// The common case for a drawing arriving from outside: the names already
    /// line up and only the properties are wrong, so there is nothing to sit in
    /// a dialog and decide.
    pub(in crate::app) fn layer_translate_same(&mut self, path: &std::path::Path) -> Task<Message> {
        use crate::modules::draw::layers::laytrans;
        let i = self.active_tab;
        let targets = match laytrans::load_targets(path) {
            Ok(targets) => targets,
            Err(why) => {
                self.command_line
                    .push_error(crate::tf!("LAYTRANS: {why}.").as_ref());
                return Task::none();
            }
        };
        let current = self.tabs[i].active_layer.clone();
        let sources = laytrans::source_layers(&self.tabs[i].scene, &current);
        let mappings = laytrans::map_same(&sources, &targets);
        if mappings.is_empty() {
            self.command_line.push_info(
                crate::t!(
                    "LAYTRANS: no layer names in common — use LAYTRANS with no file to map them."
                )
                .as_ref(),
            );
            return Task::none();
        }
        self.push_undo_snapshot(i, "LAYTRANS");
        let report = laytrans::translate(
            &mut self.tabs[i].scene,
            &mappings,
            &targets,
            &current,
            laytrans::Options::default(),
        );
        self.finish_layer_translation(i, report)
    }

    /// Apply a mapping list against an optional standards set.
    ///
    /// Names are resolved to the drawing's own spelling before the move, so a
    /// mapping typed in lower case still hits the layer it names. A target
    /// that neither the drawing nor the standards set has is left for
    /// `translate` to report instead of silently guessing a new layer.
    pub(in crate::app) fn apply_layer_translation(
        &mut self,
        i: usize,
        targets: Vec<crate::modules::draw::layers::laytrans::TargetLayer>,
        mappings: &[crate::modules::draw::layers::laytrans::Mapping],
    ) -> Task<Message> {
        use crate::modules::draw::layers::laytrans;
        let resolved: Vec<laytrans::Mapping> = mappings
            .iter()
            .map(|mapping| {
                let from = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .names()
                    .find(|name| name.eq_ignore_ascii_case(&mapping.from))
                    .map(str::to_string)
                    .unwrap_or_else(|| mapping.from.clone());
                let to = if let Some(target) = targets
                    .iter()
                    .find(|target| target.name.eq_ignore_ascii_case(&mapping.to))
                {
                    target.name.clone()
                } else if let Some(layer) = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
                    .find(|layer| layer.name.eq_ignore_ascii_case(&mapping.to))
                {
                    layer.name.clone()
                } else {
                    mapping.to.clone()
                };
                laytrans::Mapping { from, to }
            })
            .collect();
        let current = self.tabs[i].active_layer.clone();
        let sources: Vec<String> = self.tabs[i]
            .scene
            .document
            .layers
            .names()
            .map(str::to_string)
            .collect();
        let mut runnable = Vec::new();
        for mapping in resolved {
            if !sources
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&mapping.from))
            {
                self.command_line.push_error(
                    crate::tf!(
                        "LAYTRANS: no layer named \"{}\" in this drawing.",
                        mapping.from
                    )
                    .as_ref(),
                );
                continue;
            }
            runnable.push(mapping);
        }
        if runnable.is_empty() {
            self.command_line
                .push_info(crate::t!("LAYTRANS: nothing to translate.").as_ref());
            return Task::none();
        }
        self.push_undo_snapshot(i, "LAYTRANS");
        let report = laytrans::translate(
            &mut self.tabs[i].scene,
            &runnable,
            &targets,
            &current,
            laytrans::Options::default(),
        );
        for (from, to, moved) in &report.translated {
            self.command_line.push_output(
                crate::tf!("LAYTRANS: \"{from}\" -> \"{to}\" ({moved} object(s)).").as_ref(),
            );
        }
        self.finish_layer_translation(i, report)
    }

    /// The `-LAYTRANS` line parser: translate, save a mapping list, load one,
    /// or list the standards layers.
    pub(in crate::app) fn layer_translate_scripted(&mut self, rest: &str) -> Task<Message> {
        use crate::modules::draw::layers::laytrans;
        let i = self.active_tab;
        let mut words = rest.split_whitespace();
        let Some(first) = words.next() else {
            self.command_line.push_info(
                crate::t!(
                    "Usage: -LAYTRANS <standards> [FROM=TO …] | -LAYTRANS SAVE <file> FROM=TO … | -LAYTRANS LOAD <file> [<standards>] | -LAYTRANS LIST <standards>"
                )
                .as_ref(),
            );
            return Task::none();
        };
        match first.to_ascii_uppercase().as_str() {
            "SAVE" => {
                let Some(path) = words.next() else {
                    self.command_line.push_info(
                        crate::t!("Usage: -LAYTRANS SAVE <file> FROM=TO …").as_ref(),
                    );
                    return Task::none();
                };
                let mut mappings = Vec::new();
                for token in words {
                    match laytrans::Mapping::parse(token) {
                        Some(mapping) => mappings.push(mapping),
                        None => {
                            self.command_line.push_error(
                                crate::tf!(
                                    "LAYTRANS: '{}' is not a FROM=TO mapping.",
                                    token
                                )
                                .as_ref(),
                            );
                            return Task::none();
                        }
                    }
                }
                if mappings.is_empty() {
                    self.command_line.push_info(
                        crate::t!("LAYTRANS: name at least one FROM=TO mapping to save.").as_ref(),
                    );
                    return Task::none();
                }
                match laytrans::save_mappings(std::path::Path::new(path), &mappings) {
                    Ok(()) => self.command_line.push_output(
                        crate::tf!("LAYTRANS: saved {} mapping(s) to {path}.", mappings.len())
                            .as_ref(),
                    ),
                    Err(why) => self
                        .command_line
                        .push_error(crate::tf!("LAYTRANS: could not save — {why}.").as_ref()),
                }
                Task::none()
            }
            "LOAD" => {
                let Some(path) = words.next() else {
                    self.command_line.push_info(
                        crate::t!("Usage: -LAYTRANS LOAD <file> [<standards>]").as_ref(),
                    );
                    return Task::none();
                };
                let mappings = match laytrans::load_mappings(std::path::Path::new(path)) {
                    Ok(mappings) => mappings,
                    Err(why) => {
                        self.command_line.push_error(
                            crate::tf!("LAYTRANS: could not read — {why}.").as_ref(),
                        );
                        return Task::none();
                    }
                };
                if mappings.is_empty() {
                    self.command_line.push_error(
                        crate::tf!("LAYTRANS: no mappings in {path}.").as_ref(),
                    );
                    return Task::none();
                }
                let targets = match words.next() {
                    Some(standards) => match laytrans::load_targets(std::path::Path::new(standards))
                    {
                        Ok(targets) => targets,
                        Err(why) => {
                            self.command_line.push_error(
                                crate::tf!("LAYTRANS: {why}.").as_ref(),
                            );
                            return Task::none();
                        }
                    },
                    None => Vec::new(),
                };
                self.apply_layer_translation(i, targets, &mappings)
            }
            "LIST" => {
                let Some(path) = words.next() else {
                    self.command_line.push_info(
                        crate::t!("Usage: -LAYTRANS LIST <standards>").as_ref(),
                    );
                    return Task::none();
                };
                match laytrans::load_targets(std::path::Path::new(path)) {
                    Ok(targets) => {
                        self.command_line.push_output(
                            crate::tf!("LAYTRANS: {n} target layer(s) in {path}:", n = targets.len())
                                .as_ref(),
                        );
                        for target in targets {
                            self.command_line.push_output(
                                crate::tf!("  {}", target.name).as_ref(),
                            );
                        }
                    }
                    Err(why) => self
                        .command_line
                        .push_error(crate::tf!("LAYTRANS: {why}.").as_ref()),
                }
                Task::none()
            }
            _ => {
                let targets = match laytrans::load_targets(std::path::Path::new(first)) {
                    Ok(targets) => targets,
                    Err(why) => {
                        self.command_line
                            .push_error(crate::tf!("LAYTRANS: {why}.").as_ref());
                        return Task::none();
                    }
                };
                let mut mappings = Vec::new();
                for token in words {
                    match laytrans::Mapping::parse(token) {
                        Some(mapping) => mappings.push(mapping),
                        None => {
                            self.command_line.push_error(
                                crate::tf!(
                                    "LAYTRANS: '{}' is not a FROM=TO mapping.",
                                    token
                                )
                                .as_ref(),
                            );
                            return Task::none();
                        }
                    }
                }
                if mappings.is_empty() {
                    let current = self.tabs[i].active_layer.clone();
                    let sources = laytrans::source_layers(&self.tabs[i].scene, &current);
                    mappings = laytrans::map_same(&sources, &targets);
                    if mappings.is_empty() {
                        self.command_line.push_info(
                            crate::t!(
                                "LAYTRANS: no layer names in common — name the pairs as FROM=TO."
                            )
                            .as_ref(),
                        );
                        return Task::none();
                    }
                }
                self.apply_layer_translation(i, targets, &mappings)
            }
        }
    }

    /// Relabel the drawing's unit and convert the model to suit.
    ///
    /// The label always changes — that is what was asked for. The geometry
    /// changes with it whenever both units measure something, which is what
    /// makes this different from picking a unit off the status bar. Unitless
    /// measures nothing, so a drawing coming from or going to it is relabelled
    /// and left alone.
    pub(in crate::app) fn convert_drawing_units(&mut self, name: &str) -> Task<Message> {
        use crate::command::EntityTransform;
        use crate::modules::draw::units;

        let i = self.active_tab;
        let Some(to) = units::code_for_keyword(name) else {
            self.command_line
                .push_error(crate::t!("DWGUNITS: not a unit this drawing can use.").as_ref());
            return Task::none();
        };
        let from = self.tabs[i].scene.document.header.insertion_units;
        if from == to {
            let label = units::label(to);
            self.command_line
                .push_output(crate::tf!("DWGUNITS: already in {label}.").as_ref());
            return Task::none();
        }

        self.push_undo_snapshot(i, "DWGUNITS");
        self.tabs[i].scene.document.header.insertion_units = to;
        self.tabs[i].scene.sync_annotation_scale_from_header();

        let factor = units::conversion_factor(from, to).filter(|f| (f - 1.0).abs() > 1e-12);
        let (from_label, to_label) = (units::label(from), units::label(to));
        let Some(factor) = factor else {
            self.tabs[i].dirty = true;
            self.command_line.push_output(
                crate::tf!(
                    "DWGUNITS: now {to_label}. Nothing was converted — {from_label} has no size to convert from."
                )
                .as_ref(),
            );
            return Task::none();
        };

        let handles = units::model_space_handles(&self.tabs[i].scene);
        let moved = handles.len();
        // A locked layer is protected from editing, not from the drawing
        // changing what its numbers mean. Converting all but the locked layers
        // would leave the drawing at two scales at once, so the locks are
        // lifted for the duration and put back.
        let relocked: Vec<String> = self.tabs[i]
            .scene
            .document
            .layers
            .iter()
            .filter(|layer| layer.is_locked())
            .map(|layer| layer.name.clone())
            .collect();
        for name in &relocked {
            if let Some(layer) = self.tabs[i].scene.document.layers.get_mut(name) {
                layer.flags.locked = false;
            }
        }
        self.tabs[i].scene.transform_entities(
            &handles,
            &EntityTransform::Scale {
                center: glam::DVec3::ZERO,
                factor,
            },
        );
        for name in &relocked {
            if let Some(layer) = self.tabs[i].scene.document.layers.get_mut(name) {
                layer.flags.locked = true;
            }
        }

        // Model framing follows model units; sheet framing stays in paper units.
        let sheets = self.tabs[i].scene.sheet_viewport_handles();
        let scaled: std::collections::HashSet<document::Handle> = handles.iter().copied().collect();
        let mut reframed = 0usize;
        for entity in self.tabs[i].scene.document.entities_mut() {
            let handle = entity.common().handle;
            let document::entities::EntityType::Viewport(vp) = entity else {
                continue;
            };
            let is_sheet = !scaled.contains(&handle) && sheets.contains(&handle);
            if is_sheet {
                continue;
            }
            vp.view_center = vp.view_center * factor;
            vp.view_height *= factor;
            if vp.custom_scale.abs() > 1e-12 {
                vp.custom_scale /= factor;
            }
            if !scaled.contains(&handle) {
                vp.view_target = vp.view_target * factor;
            }
            reframed += 1;
        }

        // Keep the active model-space annotation scale in sync as well.
        // annotation_scale is drawing/paper, while CANNOSCALEVALUE stores
        // the reciprocal paper/drawing factor.
        if self.tabs[i].scene.annotation_scale.abs() > 1.0e-12 {
            self.tabs[i].scene.annotation_scale *= factor as f32;
        }

        if self.tabs[i]
            .scene
            .document
            .header
            .annotation_scale_value
            .abs()
            > 1.0e-12
        {
            self.tabs[i].scene.document.header.annotation_scale_value /= factor;
        }
        // Sizes the drawing keeps as settings rather than as geometry: dash
        // lengths, default heights and widths, the radii the fillet and chamfer
        // commands start from. They are all lengths in the unit that just
        // changed, so they change with it or the next line drawn comes out at
        // the old scale. Angles and screen-relative sizes are left alone —
        // PDSIZE is a percentage of the screen when negative.
        let header = &mut self.tabs[i].scene.document.header;
        for length in [
            &mut header.linetype_scale,
            &mut header.current_entity_linetype_scale,
            &mut header.text_height,
            &mut header.trace_width,
            &mut header.sketch_increment,
            &mut header.thickness,
            &mut header.polyline_width,
            &mut header.fillet_radius,
            &mut header.chamfer_distance_a,
            &mut header.chamfer_distance_b,
            &mut header.chamfer_length,
        ] {
            *length *= factor;
        }
        if header.point_display_size > 0.0 {
            header.point_display_size *= factor;
        }

        // A dimension style holds its text height and arrow size in drawing
        // units and multiplies them by DIMSCALE, so moving that one factor
        // rescales the whole style. Zero means "take it from the viewport",
        // which is not a length and stays as it is.
        for style in self.tabs[i].scene.document.dim_styles.iter_mut() {
            if style.dimscale.abs() > 1e-12 {
                style.dimscale *= factor;
            }
        }

        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(
            crate::tf!(
                "DWGUNITS: {from_label} to {to_label} — {moved} object(s) scaled by {factor}, {reframed} viewport(s) reframed."
            )
            .as_ref(),
        );
        Task::none()
    }

    /// Open the translator, optionally with a standard already loaded.
    pub(in crate::app) fn open_layer_translator(
        &mut self,
        preload: Option<std::path::PathBuf>,
    ) -> Task<Message> {
        self.layer_translator.get_or_insert_with(Default::default);
        self.active_modal = Some(crate::app::ModalKind::LayerTranslator);
        match preload {
            Some(path) => Task::done(Message::LayerTranslatorLoaded(path)),
            None => Task::none(),
        }
    }

    /// Read or write the mapping list as plain `from -> to` lines.
    ///
    /// A drawing standards file is where AutoCAD keeps these, but writing them
    /// there would mean inventing a place inside the DWG for something no other
    /// application reads back. A small text file beside the drawing carries the
    /// same information without touching the drawing's own structure.
    pub(in crate::app) fn layer_translator_mappings_file(
        &mut self,
        path: &std::path::Path,
        save: bool,
    ) {
        use crate::modules::draw::layers::laytrans::Mapping;
        if save {
            let Some(state) = self.layer_translator.as_ref() else {
                return;
            };
            match crate::modules::draw::layers::laytrans::save_mappings(path, &state.mappings) {
                Ok(()) => self.command_line.push_output(
                    crate::tf!("LAYTRANS: saved {n} mapping(s).", n = state.mappings.len())
                        .as_ref(),
                ),
                Err(why) => self
                    .command_line
                    .push_error(crate::tf!("LAYTRANS: could not save — {why}.").as_ref()),
            }
            return;
        }
        let parsed: Vec<Mapping> = match crate::modules::draw::layers::laytrans::load_mappings(path)
        {
            Ok(parsed) => parsed,
            Err(why) => {
                self.command_line
                    .push_error(crate::tf!("LAYTRANS: could not read — {why}.").as_ref());
                return;
            }
        };
        let count = parsed.len();
        if let Some(state) = self.layer_translator.as_mut() {
            state.mappings = parsed;
        }
        self.command_line
            .push_output(crate::tf!("LAYTRANS: loaded {count} mapping(s).").as_ref());
    }

    /// Write what the last translation did beside the drawing.
    pub(in crate::app) fn write_layer_translation_log(&mut self, i: usize) {
        let Some(report) = self.last_layer_translation.as_ref() else {
            return;
        };
        let base = self.tabs[i]
            .current_path
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from(&self.tabs[i].tab_title));
        let path = base.with_extension("laytrans.log");
        match std::fs::write(&path, report.to_log()) {
            Ok(()) => self
                .command_line
                .push_output(crate::tf!("LAYTRANS: log written to {}.", path.display()).as_ref()),
            Err(why) => self
                .command_line
                .push_error(crate::tf!("LAYTRANS: could not write log — {why}.").as_ref()),
        }
    }

    /// Apply a translation's result: mark the drawing, refresh the panel and
    /// say what moved. Shared by the command and the dialog so both report the
    /// same way.
    pub(in crate::app) fn finish_layer_translation(
        &mut self,
        i: usize,
        report: crate::modules::draw::layers::laytrans::Report,
    ) -> Task<Message> {
        let layers = report.translated.len();
        let objects = report.objects();
        self.tabs[i].dirty = true;
        self.refresh_layer_panel();
        self.refresh_properties();
        for (layer, reason) in &report.skipped {
            self.command_line
                .push_info(crate::tf!("LAYTRANS: skipped \"{layer}\" — {reason}.").as_ref());
        }
        self.command_line.push_output(
            crate::tf!("LAYTRANS: translated {layers} layer(s), {objects} object(s).").as_ref(),
        );
        self.last_layer_translation = Some(report);
        Task::none()
    }
}

impl OpenCADStudio {
    fn record_layiso_history(
        &mut self,
        i: usize,
        history_len: usize,
        before: Option<crate::app::tab_document::LayisoState>,
        after: Option<crate::app::tab_document::LayisoState>,
    ) {
        if self.tabs[i].history.undo_stack.len() == history_len {
            return;
        }
        if let Some(crate::app::tab_document::HistorySnapshot::Delta(delta)) =
            self.tabs[i].history.undo_stack.last_mut()
        {
            delta.layiso_state = Some((before, after));
        }
    }

    fn start_layiso(&mut self, i: usize) {
        let selected_layers: rustc_hash::FxHashSet<String> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(_, entity)| entity.common().layer.clone())
            .collect();

        if selected_layers.is_empty() {
            use crate::modules::draw::select::SelectObjectsCommand;
            let command = SelectObjectsCommand::with_settings(
                "LAYISO",
                "LAYISO",
                "Select objects or [Settings]:",
                || crate::command::CmdResult::Dispatch("LAYISO SETTINGS".to_string()),
            );
            self.command_line.push_info(&command.prompt());
            self.command_line.set_step_options(command.options());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return;
        }

        self.apply_layiso(i, selected_layers);
    }

    fn apply_layiso(
        &mut self,
        i: usize,
        selected_layers: rustc_hash::FxHashSet<String>,
    ) {
        let mode = self.tabs[i].layiso_mode;
        let layiso_history_before = self.tabs[i].layiso_state.clone();
        let current_layer = self.tabs[i].active_layer.clone();
        let before: Vec<(String, document::tables::Layer)> = self.tabs[i]
            .scene
            .document
            .layers
            .iter()
            .map(|layer| (layer.name.clone(), layer.clone()))
            .collect();

        let names: Vec<String> = before
            .iter()
            .filter(|(name, layer)| {
                !selected_layers.contains(name)
                    // AutoCAD does not hide the current layer, and layer 0
                    // is treated as a special protected layer by layer tools.
                    && name != &current_layer
                    && name != "0"
                    && match mode {
                        crate::app::tab_document::LayisoMode::Off => !layer.is_off(),
                        crate::app::tab_document::LayisoMode::LockAndFade => !layer.is_locked(),
                    }
            })
            .map(|(name, _)| name.clone())
            .collect();

        let undo = self.begin_layer_undo(i, "LAYISO", &names);
        for name in &names {
            if let Some(layer) = self.tabs[i].scene.document.layers.get_mut(name) {
                match mode {
                    crate::app::tab_document::LayisoMode::Off => layer.turn_off(),
                    // A locked layer already draws faded; its saved
                    // transparency is left alone.
                    crate::app::tab_document::LayisoMode::LockAndFade => layer.lock(),
                }
            }
        }

        let after_by_name: std::collections::HashMap<_, _> = self.tabs[i]
            .scene
            .document
            .layers
            .iter()
            .map(|layer| (layer.name.clone(), layer.clone()))
            .collect();
        let snapshots = before
            .into_iter()
            .filter_map(|(name, before)| {
                after_by_name
                    .get(&name)
                    .cloned()
                    .map(|after| crate::app::tab_document::LayisoLayerSnapshot {
                        name,
                        before,
                        after,
                    })
            })
            .collect();
        let layiso_history_after = Some(crate::app::tab_document::LayisoState {
            mode,
            layers: snapshots,
        });
        self.tabs[i].layiso_state = layiso_history_after.clone();

        if !names.is_empty() {
            self.tabs[i].scene.invalidate_layer_visibility(&names);
            self.tabs[i].scene.invalidate_layer_dependencies(&names);
            self.tabs[i].dirty = true;
        }
        let history_len = self.tabs[i].history.undo_stack.len();
        self.commit_layer_undo(i, undo);
        self.record_layiso_history(
            i,
            history_len,
            layiso_history_before,
            layiso_history_after,
        );
        self.refresh_layer_panel();
        self.command_line.push_info(
            crate::tf!("LAYISO: isolated {} layer(s).", selected_layers.len()).as_ref(),
        );
    }

    /// Restore the saved layer state `name`, returning how many layers it
    /// touched. Inside a layout viewport the state applies to that viewport
    /// alone: its frozen layers become the viewport's freezes, and colour,
    /// linetype, lineweight and transparency become viewport overrides where
    /// they differ from the layer. Layers the state turns off are frozen in
    /// the viewport; lock and plot have no per-viewport form and apply to the
    /// drawing.
    /// Give `layer` its own `color` in the layout viewport `viewport`.
    pub(in crate::app) fn set_layer_viewport_color(
        &mut self,
        i: usize,
        layer: &str,
        viewport: document::Handle,
        color: document::types::Color,
    ) {
        use document::objects::{KnownXRecordKind as Kind, XRecordValue as Value};
        let Some(layer) = self.tabs[i].scene.document.layers.get(layer).map(|l| l.handle) else {
            return;
        };
        let Some(packed) = crate::scene::view::render::pack_layer_override_color(&color) else {
            return;
        };
        self.push_undo_snapshot(i, "VP Color".to_string());
        let scene = &mut self.tabs[i].scene;
        scene.document.set_layer_viewport_override(
            layer,
            Kind::LayerViewportColorOverride,
            viewport,
            Value::Int32(packed),
        );
        scene.bump_geometry();
        self.tabs[i].dirty = true;
        let doc_layers = self.tabs[i].scene.document.layers.clone();
        let vp_info = self.tabs[i].scene.viewport_list();
        self.tabs[i].layers.sync_with_viewports(&doc_layers, vp_info);
    }

    pub(in crate::app) fn restore_layer_state_here(&mut self, i: usize, name: &str) -> usize {
        self.restore_layer_state_filtered(i, name, None)
    }

    /// Restore a named layer state, optionally limited to a property mask
    /// (`None` uses the properties the state itself captured). Inside a paper
    /// viewport the same properties become viewport overrides, so a state can
    /// drive one sheet without touching the others.
    pub(in crate::app) fn restore_layer_state_filtered(
        &mut self,
        i: usize,
        name: &str,
        only: Option<document::layer_state::LayerStateMask>,
    ) -> usize {
        use document::layer_state::LayerStateMask as Mask;
        use document::objects::{KnownXRecordKind as Kind, XRecordValue as Value};
        use document::EntityType;
        let scene = &mut self.tabs[i].scene;
        let viewport = scene.active_viewport.filter(|handle| {
            scene.current_layout != "Model"
                && matches!(
                    scene.document.get_entity(*handle),
                    Some(EntityType::Viewport(_))
                )
        });
        let document = &mut scene.document;
        let Some(viewport) = viewport else {
            let Some(mask) = only else {
                return document.restore_layer_state(name).unwrap_or(0);
            };
            let Some(state) = document.layer_state(name) else {
                return 0;
            };
            let mut restored = 0;
            for saved in &state.layers {
                let layer_name = if !saved.layer_handle.is_null() {
                    document
                        .layers
                        .iter()
                        .find(|layer| layer.handle == saved.layer_handle)
                        .map(|layer| layer.name.clone())
                } else {
                    None
                }
                .unwrap_or_else(|| saved.layer_name.clone());
                let Some(layer) = document.layers.get_mut(&layer_name) else {
                    continue;
                };
                if mask.contains(Mask::ON) {
                    layer.flags.off = saved.off;
                }
                if mask.contains(Mask::FROZEN) {
                    layer.flags.frozen = saved.frozen;
                }
                if mask.contains(Mask::LOCKED) {
                    layer.flags.locked = saved.locked;
                }
                if mask.contains(Mask::PLOT) {
                    layer.is_plottable = saved.plottable;
                }
                if mask.contains(Mask::NEW_VIEWPORT) {
                    layer.flags.frozen_in_new_viewport = saved.new_viewport_frozen;
                }
                if mask.contains(Mask::COLOR) {
                    layer.color = saved.color;
                    layer.color_name.clone_from(&saved.color_name);
                    layer.book_name.clone_from(&saved.book_name);
                }
                if mask.contains(Mask::LINE_TYPE) && !saved.line_type.is_empty() {
                    layer.line_type.clone_from(&saved.line_type);
                }
                if mask.contains(Mask::LINE_WEIGHT) {
                    layer.line_weight = saved.line_weight;
                }
                if mask.contains(Mask::PLOT_STYLE) {
                    layer.plot_style.clone_from(&saved.plot_style);
                }
                if mask.contains(Mask::TRANSPARENCY) {
                    if let Some(transparency) = saved.transparency {
                        layer.transparency = transparency;
                    }
                }
                restored += 1;
            }
            if mask.contains(Mask::CURRENT_VIEWPORT) {
                if let Some((handle, name)) = document
                    .layers
                    .get(&state.current_layer)
                    .map(|layer| (layer.handle, layer.name.clone()))
                {
                    document.header.current_layer_handle = handle;
                    document.header.current_layer_name = name;
                }
            }
            return restored;
        };
        let Some(state) = document.layer_state(name) else {
            return 0;
        };
        let mask = only.unwrap_or(state.mask);
        let mut restored = 0;
        let (mut freeze, mut thaw) = (Vec::new(), Vec::new());
        for saved in &state.layers {
            let found = document
                .layers
                .iter()
                .find(|layer| !saved.layer_handle.is_null() && layer.handle == saved.layer_handle)
                .or_else(|| document.layers.get(&saved.layer_name))
                .map(|layer| layer.name.clone());
            let Some(layer_name) = found else {
                continue;
            };
            let line_type_handle = document
                .line_types
                .iter()
                .find(|line_type| line_type.name.eq_ignore_ascii_case(&saved.line_type))
                .map(|line_type| line_type.handle);
            let Some(layer) = document.layers.get_mut(&layer_name) else {
                continue;
            };
            restored += 1;
            if mask.contains(Mask::LOCKED) {
                layer.flags.locked = saved.locked;
            }
            if mask.contains(Mask::PLOT) {
                layer.is_plottable = saved.plottable;
            }
            if mask.contains(Mask::NEW_VIEWPORT) {
                layer.flags.frozen_in_new_viewport = saved.new_viewport_frozen;
            }
            let layer = layer.clone();
            // On / off has no per-viewport form; a layer the state turns off
            // is frozen in this viewport instead, so other viewports keep
            // their own visibility.
            let (by_freeze, by_off) =
                (mask.contains(Mask::FROZEN), mask.contains(Mask::ON));
            if by_freeze || by_off {
                let hidden = (by_freeze && saved.frozen) || (by_off && saved.off);
                if hidden { &mut freeze } else { &mut thaw }.push(layer.handle);
            }
            // (kind, saved value, whether it differs from the layer's own)
            let mut overrides: Vec<(Kind, Option<Value>, bool)> = Vec::new();
            if mask.contains(Mask::COLOR) {
                let packed = crate::scene::view::render::pack_layer_override_color(&saved.color);
                overrides.push((
                    Kind::LayerViewportColorOverride,
                    packed.map(Value::Int32),
                    saved.color != layer.color,
                ));
            }
            if mask.contains(Mask::LINE_TYPE) {
                overrides.push((
                    Kind::LayerViewportLinetypeOverride,
                    line_type_handle.map(Value::Handle),
                    !saved.line_type.eq_ignore_ascii_case(&layer.line_type),
                ));
            }
            if mask.contains(Mask::LINE_WEIGHT) {
                overrides.push((
                    Kind::LayerViewportLineweightOverride,
                    Some(Value::Int32(saved.line_weight.value() as i32)),
                    saved.line_weight != layer.line_weight,
                ));
            }
            if let (true, Some(transparency)) =
                (mask.contains(Mask::TRANSPARENCY), saved.transparency)
            {
                // The opacity byte under the "by alpha" method, as the format packs it.
                let packed = 0x0200_0000u32 | (255 - transparency.alpha()) as u32;
                overrides.push((
                    Kind::LayerViewportAlphaOverride,
                    Some(Value::Int32(packed as i32)),
                    transparency != layer.transparency,
                ));
            }
            // An override is written where the state differs from the layer,
            // and where one already exists for this viewport, so it cannot
            // keep an older value.
            let overrides: Vec<(Kind, Value)> = overrides
                .into_iter()
                .filter_map(|(kind, value, differs)| {
                    let existing = document
                        .layer_viewport_overrides(layer.handle, kind)
                        .iter()
                        .any(|(handle, _)| *handle == viewport);
                    value
                        .filter(|_| differs || existing)
                        .map(|value| (kind, value))
                })
                .collect();
            for (kind, value) in overrides {
                document.set_layer_viewport_override(layer.handle, kind, viewport, value);
            }
        }
        if let Some(EntityType::Viewport(vp)) = document.get_entity_mut(viewport) {
            vp.frozen_layers.retain(|handle| !thaw.contains(handle));
            for handle in freeze {
                if !vp.frozen_layers.contains(&handle) {
                    vp.frozen_layers.push(handle);
                }
            }
        }
        scene.bump_geometry();
        let doc_layers = self.tabs[i].scene.document.layers.clone();
        let vp_info = self.tabs[i].scene.viewport_list();
        self.tabs[i]
            .layers
            .sync_with_viewports(&doc_layers, vp_info);
        restored
    }
}


/// Parse the optional property filter of `-LAYERSTATE RESTORE <name> <filter>`.
/// `None` means "every property the state captured".
fn parse_layer_state_filter(
    text: &str,
) -> Result<Option<document::layer_state::LayerStateMask>, String> {
    use document::layer_state::LayerStateMask as Mask;
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let mut mask = Mask::NONE;
    for token in text.split(|c: char| c == ',' || c.is_whitespace()) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let bit = match token.to_ascii_uppercase().as_str() {
            "ALL" => return Ok(None),
            "ON" | "OFF" => Mask::ON,
            "FROZEN" | "FREEZE" => Mask::FROZEN,
            "LOCKED" | "LOCK" => Mask::LOCKED,
            "PLOT" | "PLOTTABLE" => Mask::PLOT,
            "NEWVIEWPORT" | "NEWVP" => Mask::NEW_VIEWPORT,
            "COLOR" | "COLOUR" => Mask::COLOR,
            "LINETYPE" | "LTYPE" => Mask::LINE_TYPE,
            "LINEWEIGHT" | "LWEIGHT" => Mask::LINE_WEIGHT,
            "PLOTSTYLE" => Mask::PLOT_STYLE,
            "TRANSPARENCY" | "ALPHA" => Mask::TRANSPARENCY,
            "CURRENT" | "CURRENTLAYER" => Mask::CURRENT_VIEWPORT,
            other => {
                return Err(crate::tf!(
                    "LAYERSTATE: unknown restore property '{other}' (use ON, FROZEN, LOCKED, COLOR, LINETYPE, LINEWEIGHT, PLOT, PLOTSTYLE, TRANSPARENCY or ALL)."
                )
                .to_string())
            }
        };
        mask |= bit;
    }
    Ok(if mask == Mask::NONE { None } else { Some(mask) })
}

fn las_text(value: &str) -> String {
    value.replace(['\t', '\n', '\r'], " ")
}

fn las_color(color: document::types::Color) -> String {
    use document::types::Color;
    match color {
        Color::ByLayer => "bylayer".to_string(),
        Color::ByBlock => "byblock".to_string(),
        Color::None => "none".to_string(),
        Color::Index(index) => format!("index:{index}"),
        Color::Rgb { r, g, b } => format!("rgb:{r},{g},{b}"),
    }
}

fn las_parse_color(text: &str) -> document::types::Color {
    use document::types::Color;
    if let Some(rest) = text.strip_prefix("index:") {
        return rest.parse::<u8>().map(Color::Index).unwrap_or(Color::Index(7));
    }
    if let Some(rest) = text.strip_prefix("rgb:") {
        let parts: Vec<&str> = rest.split(',').collect();
        if parts.len() == 3 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                parts[0].parse::<u8>(),
                parts[1].parse::<u8>(),
                parts[2].parse::<u8>(),
            ) {
                return Color::Rgb { r, g, b };
            }
        }
    }
    match text {
        "bylayer" => Color::ByLayer,
        "byblock" => Color::ByBlock,
        "none" => Color::None,
        _ => Color::Index(7),
    }
}

fn las_alpha(transparency: Option<document::types::Transparency>) -> String {
    use document::types::Transparency;
    match transparency {
        Some(Transparency::ByLayer) => "bylayer".to_string(),
        Some(Transparency::ByBlock) => "byblock".to_string(),
        Some(Transparency::Explicit(alpha)) => format!("explicit:{alpha}"),
        None => "byblock".to_string(),
    }
}

fn las_parse_alpha(text: &str) -> Option<document::types::Transparency> {
    use document::types::Transparency;
    if let Some(rest) = text.strip_prefix("explicit:") {
        return rest.parse::<u8>().ok().map(Transparency::Explicit);
    }
    match text {
        "bylayer" => Some(Transparency::ByLayer),
        "byblock" => Some(Transparency::ByBlock),
        _ => None,
    }
}

/// Write a layer state as the line-oriented `.las` interchange text. Every
/// property the state captured round-trips, including the current layer.
fn layer_state_to_las(state: &document::layer_state::LayerState) -> String {
    let mut out = String::from("OCSLAS\t1\n");
    out.push_str(&format!("NAME\t{}\n", las_text(&state.name)));
    out.push_str(&format!("DESC\t{}\n", las_text(&state.description)));
    out.push_str(&format!("MASK\t{}\n", state.mask.bits()));
    out.push_str(&format!("CURRENT\t{}\n", las_text(&state.current_layer)));
    for layer in &state.layers {
        out.push_str(&format!(
            "LAYER\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            las_text(&layer.layer_name),
            u8::from(layer.off),
            u8::from(layer.frozen),
            u8::from(layer.locked),
            u8::from(layer.plottable),
            u8::from(layer.new_viewport_frozen),
            las_color(layer.color),
            las_text(&layer.line_type),
            layer.line_weight.value(),
            las_text(&layer.plot_style),
            las_alpha(layer.transparency),
        ));
    }
    out.push_str("END\n");
    out
}

fn layer_state_from_las(text: &str) -> Result<document::layer_state::LayerState, String> {
    use document::layer_state::{LayerState, LayerStateLayer, LayerStateMask};
    let mut state = LayerState {
        name: String::new(),
        description: String::new(),
        mask: LayerStateMask::SUPPORTED,
        hidden: false,
        current_layer: "0".to_string(),
        layers: Vec::new(),
    };
    for line in text.lines() {
        let mut fields = line.split('\t');
        match fields.next().unwrap_or("") {
            "NAME" => state.name = fields.next().unwrap_or("").to_string(),
            "DESC" => state.description = fields.next().unwrap_or("").to_string(),
            "MASK" => {
                if let Some(bits) = fields.next().and_then(|value| value.parse::<u32>().ok()) {
                    state.mask = LayerStateMask::from_bits(bits);
                }
            }
            "CURRENT" => {
                state.current_layer = fields.next().unwrap_or("0").to_string();
            }
            "LAYER" => {
                let values: Vec<&str> = fields.collect();
                if values.len() < 10 {
                    continue;
                }
                state.layers.push(LayerStateLayer {
                    layer_handle: document::types::Handle::NULL,
                    layer_name: values[0].to_string(),
                    off: values[1] == "1",
                    frozen: values[2] == "1",
                    locked: values[3] == "1",
                    plottable: values[4] == "1",
                    new_viewport_frozen: values[5] == "1",
                    color: las_parse_color(values[6]),
                    color_name: None,
                    book_name: None,
                    line_type: values[7].to_string(),
                    line_weight: values[8]
                        .parse::<i16>()
                        .map(document::types::LineWeight::from_value)
                        .unwrap_or_default(),
                    plot_style: values[9].to_string(),
                    transparency: values.get(10).and_then(|value| las_parse_alpha(value)),
                });
            }
            _ => {}
        }
    }
    if state.name.is_empty() {
        return Err(crate::t!("LAYERSTATE: the file holds no named layer state.").to_string());
    }
    Ok(state)
}
impl OpenCADStudio {
    pub(in crate::app) fn handle_merge_layers(&mut self, sources: Vec<String>, target: String) {
        let i = self.active_tab;
        self.merge_layers(i, sources, target);
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.command_line.set_step_options(Vec::new());
        self.restore_pre_cmd_tangent();
    }

    pub(in crate::app) fn merge_layers(&mut self, i: usize, sources: Vec<String>, target: String) {
        // Validate the whole operation before the one snapshot or any mutation.
        if sources.is_empty()
            || !self.tabs[i].scene.document.layers.contains(&target)
            || sources.iter().any(|src| {
                src == "0"
                    || src == &target
                    || src.eq_ignore_ascii_case(&self.tabs[i].active_layer)
                    || !self.tabs[i].scene.document.layers.contains(src)
            })
        {
            return;
        }
        self.push_undo_snapshot(i, "LAYMRG");
        for src in sources {
            let moved = crate::modules::draw::layers::laytrans::merge_layer(
                &mut self.tabs[i].scene,
                &src,
                &target,
                false,
            );
            let dst = &target;
            self.command_line.push_output(
                crate::tf!("LAYMRG: merged \"{src}\" into \"{dst}\" ({moved} object(s)).").as_ref(),
            );
        }
        self.tabs[i].scene.invalidate_dependency_index();
        self.tabs[i]
            .scene
            .invalidate_layer_dependencies(std::slice::from_ref(&target));
        self.tabs[i].dirty = true;
        self.refresh_layer_panel();
    }
}

#[cfg(test)]
#[path = "laymrg_tests.rs"]
mod laymrg_tests;
