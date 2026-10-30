use super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_styleprops(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            // View / grid variables held by the app rather than the header,
            // typed bare or through SETVAR (#1524, #1618).
            cmd if matches!(
                cmd.strip_prefix("SETVAR ")
                    .unwrap_or(cmd)
                    .split_whitespace()
                    .next(),
                Some("PERSPECTIVE" | "GRIDMODE" | "GRIDUNIT" | "QPMODE" | "QPLOCATION")
            ) =>
            {
                let mut words = cmd.strip_prefix("SETVAR ").unwrap_or(cmd).splitn(2, ' ');
                let name = words.next().unwrap_or_default().to_string();
                let value = words.next().map(str::trim).filter(|v| !v.is_empty());
                let current = match name.as_str() {
                    "PERSPECTIVE" => {
                        let persp = self.tabs[i].scene.camera.borrow().projection
                            == crate::scene::Projection::Perspective;
                        (persp as i32).to_string()
                    }
                    "GRIDMODE" => (self.show_grid as i32).to_string(),
                    "QPMODE" => (self.quick_properties as i32).to_string(),
                    "QPLOCATION" => (self.quick_properties_float as i32).to_string(),
                    _ => format!("{},{}", self.grid_spacing_x, self.grid_spacing_y),
                };
                let Some(value) = value else {
                    self.command_line
                        .push_output(&crate::tf!("Enter new value for {name} <{current}>:"));
                    self.pending_setvar = Some(name);
                    return Some(Task::none());
                };
                match (name.as_str(), value) {
                    ("PERSPECTIVE", "0" | "1") => {
                        return Some(Task::done(Message::SetProjection(value == "0")));
                    }
                    ("GRIDMODE", "0" | "1") => {
                        if (value == "1") != self.show_grid {
                            return Some(Task::done(Message::ToggleGrid));
                        }
                    }
                    // 2 (only objects with configured properties) shows the
                    // panel for every object, as no per-type list exists.
                    ("QPMODE", "0" | "1" | "2") => {
                        if (value != "0") != self.quick_properties {
                            return Some(Task::done(Message::ToggleQuickProperties));
                        }
                    }
                    ("QPLOCATION", "0" | "1") => {
                        if (value == "1") != self.quick_properties_float {
                            self.quick_properties_float = value == "1";
                            self.persist_settings_if_changed();
                        }
                    }
                    ("GRIDUNIT", _) => {
                        let mut parts = value.split(',').map(|p| p.trim().parse::<f32>());
                        let x = parts.next().and_then(Result::ok).filter(|v| *v > 0.0);
                        let y = match parts.next() {
                            Some(p) => p.ok().filter(|v| *v > 0.0),
                            None => x,
                        };
                        let (Some(x), Some(y)) = (x, y) else {
                            self.command_line.push_error(
                                crate::t!("Requires two positive values (x,y) or one for both.")
                                    .as_ref(),
                            );
                            return Some(Task::none());
                        };
                        self.grid_spacing_x = x;
                        self.grid_spacing_y = y;
                        self.sync_vport_display(i);
                        self.persist_settings_if_changed();
                    }
                    _ => self
                        .command_line
                        .push_error(crate::t!("Requires 0 or 1").as_ref()),
                }
                return Some(Task::none());
            }
            "CETRANSPARENCY" => return self.dispatch_styleprops("SETVAR CETRANSPARENCY", i),
            cmd if cmd.starts_with("CETRANSPARENCY ") => {
                return self.dispatch_styleprops(&format!("SETVAR {cmd}"), i);
            }
            "FRAMES0" => return self.dispatch_styleprops("SETVAR FRAME 0", i),
            "FRAMES1" => return self.dispatch_styleprops("SETVAR FRAME 1", i),
            "FRAMES2" => return self.dispatch_styleprops("SETVAR FRAME 2", i),
            // The mixed state is shown, never chosen.
            "FRAMES3" => return Some(Task::none()),
            "UOSNAP0" => return self.dispatch_styleprops("SETVAR UOSNAP 0", i),
            "UOSNAP1" => return self.dispatch_styleprops("SETVAR UOSNAP 1", i),
            // COLOR <ByLayer|ByBlock|1-255|name> — the colour applied to new
            // objects (CECOLOR). Bare COLOR reports the current value.
            "COLOR" | "COLOUR" | "CECOLOR" | "DDCOLOR" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "COLOR",
                    "COLOR  new object colour  [ByLayer / ByBlock / 1-255 / red / yellow / green / cyan / blue / magenta / white]:",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            // BYLAYER is an immediate shortcut (sets ByLayer without prompting);
            // the `<name> <value>` forms and the bare-Enter getter route here too.
            cmd if cmd == "BYLAYER"
                || cmd.starts_with("COLOR ")
                || cmd.starts_with("COLOUR ")
                || cmd.starts_with("CECOLOR ")
                || cmd.starts_with("DDCOLOR ") =>
            {
                use codec::types::Color;
                let describe = |c: &Color| match c {
                    Color::ByLayer => "ByLayer".to_string(),
                    Color::ByBlock => "ByBlock".to_string(),
                    Color::Index(n) => crate::tf!("index {n}").into_owned(),
                    _ => "(custom)".to_string(),
                };
                let arg = if cmd == "BYLAYER" {
                    "BYLAYER".to_string()
                } else {
                    cmd.split_whitespace().nth(1).unwrap_or("").to_uppercase()
                };
                let color = match arg.as_str() {
                    "" => {
                        let c = self.tabs[i].scene.document.header.current_entity_color;
                        self.command_line.push_output(
                            crate::tf!("Current object colour: {}", describe(&c)).as_ref(),
                        );
                        return Some(Task::none());
                    }
                    "BYLAYER" => Some(Color::ByLayer),
                    "BYBLOCK" => Some(Color::ByBlock),
                    "RED" => Some(Color::Index(1)),
                    "YELLOW" => Some(Color::Index(2)),
                    "GREEN" => Some(Color::Index(3)),
                    "CYAN" => Some(Color::Index(4)),
                    "BLUE" => Some(Color::Index(5)),
                    "MAGENTA" => Some(Color::Index(6)),
                    "WHITE" => Some(Color::Index(7)),
                    n => n.parse::<i16>().ok().map(Color::from_index),
                };
                match color {
                    Some(c) => {
                        self.tabs[i].scene.document.header.current_entity_color = c;
                        self.ribbon.active_color = c;
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(
                            crate::tf!("Object colour set to {}.", describe(&c)).as_ref(),
                        );
                    }
                    None => {
                        self.command_line.push_error(
                            crate::t!("Usage: COLOR <ByLayer|ByBlock|1-255|red|yellow|green|cyan|blue|magenta|white>").as_ref(),
                        );
                    }
                }
            }

            // ── LINETYPE management ───────────────────────────────────────
            "LINETYPE" | "LT" => {
                use crate::command::KeywordCommand;
                let c = KeywordCommand::new(
                    "LINETYPE",
                    "LINETYPE  [List / Set / Load]:",
                    vec![
                        ("List", "LIST", None),
                        ("Load", "LOAD", None),
                        (
                            "Set",
                            "SET",
                            Some("LINETYPE SET  linetype name (ByLayer / ByBlock / …):"),
                        ),
                    ],
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("LINETYPE ") || cmd.starts_with("LT ") => {
                let raw_rest = cmd.split_once(' ').map(|(_, r)| r.trim()).unwrap_or("");
                let parts: Vec<&str> = raw_rest.split_whitespace().collect();
                let sub = parts.get(0).map(|s| s.to_uppercase()).unwrap_or_default();
                match sub.as_str() {
                    // Load the definitions of a `.lin` file (#1588).
                    "LOAD" | "L" => {
                        return Some(Task::perform(
                            crate::io::pick_linetype_file(),
                            Message::LinetypeLoaded,
                        ));
                    }
                    "" | "LIST" | "?" => {
                        let ltypes: Vec<String> = self.tabs[i]
                            .scene
                            .document
                            .line_types
                            .iter()
                            .map(|lt| format!("{} ({})", lt.name, lt.description))
                            .collect();
                        if ltypes.is_empty() {
                            self.command_line
                                .push_output(crate::t!("No linetypes defined.").as_ref());
                        } else {
                            self.command_line.push_output(
                                crate::tf!("Linetypes: {}", ltypes.join(", ")).as_ref(),
                            );
                        }
                    }
                    // Set the current linetype applied to newly drawn entities.
                    "SET" | "CURRENT" | "S" => {
                        let name = parts.get(1).copied().unwrap_or("");
                        if name.is_empty() {
                            self.command_line.push_info(
                                crate::t!("Usage: LINETYPE SET <name | ByLayer | ByBlock>")
                                    .as_ref(),
                            );
                        } else {
                            let canon = if name.eq_ignore_ascii_case("BYLAYER") {
                                Some(("ByLayer".to_string(), codec::types::Handle::NULL))
                            } else if name.eq_ignore_ascii_case("BYBLOCK") {
                                Some(("ByBlock".to_string(), codec::types::Handle::NULL))
                            } else {
                                // A catalog linetype loads on first use.
                                crate::io::linetypes::ensure_catalog_linetype(
                                    &mut self.tabs[i].scene.document,
                                    name,
                                );
                                self.tabs[i]
                                    .scene
                                    .document
                                    .line_types
                                    .iter()
                                    .find(|lt| lt.name.eq_ignore_ascii_case(name))
                                    .map(|lt| (lt.name.clone(), lt.handle))
                            };
                            match canon {
                                Some((nm, handle)) => {
                                    let h = &mut self.tabs[i].scene.document.header;
                                    h.current_linetype_name = nm.clone();
                                    h.current_linetype_handle = handle;
                                    self.tabs[i].dirty = true;
                                    self.command_line.push_output(
                                        crate::tf!("Current linetype set to {nm}.").as_ref(),
                                    );
                                }
                                None => {
                                    self.command_line.push_error(crate::tf!(
                                        "LINETYPE: \"{name}\" is not loaded. Use LINETYPE LIST to see available linetypes."
                                    ).as_ref());
                                }
                            }
                        }
                    }
                    _ => {
                        self.command_line
                            .push_info(crate::t!("Usage: LINETYPE LIST | SET <name>").as_ref());
                    }
                }
            }

            // ── PURGE unused definitions ──────────────────────────────────
            // Bare PURGE opens the Purge dialog (-PURGE asks on the command
            // line); PURGE <sub> (typed or scripted) runs directly.
            "PURGE" => self.open_purge_dialog(),
            cmd if cmd.starts_with("PURGE ") => {
                let sub = cmd
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("ALL")
                    .to_uppercase();
                // An optional third word limits the purge to matching names
                // (`PURGE LAYERS A*,TMP`).
                let pattern = cmd.split_whitespace().nth(2).unwrap_or("*").to_string();
                let wanted = |name: &str| {
                    pattern
                        .split(',')
                        .any(|p| crate::io::xref_model::wildcard_match(name, p.trim()))
                };
                // The Purge dialog's choice, when it runs the command.
                let only = self.purge_only.take();
                let chosen =
                    |kind: usize, name: &str| only.as_ref().is_none_or(|o| o.allows(kind, name));
                let all = sub == "ALL" || sub.is_empty();
                let do_layers = all || matches!(sub.as_str(), "LAYERS" | "LAYER" | "LA");
                let do_styles = all || matches!(sub.as_str(), "TEXTSTYLES" | "STYLES" | "ST");
                let do_lts = all || matches!(sub.as_str(), "LINETYPES" | "LTYPES" | "LT");
                let do_blocks = all || matches!(sub.as_str(), "BLOCKS" | "BLOCK" | "B");
                if !(do_layers || do_styles || do_lts || do_blocks) {
                    self.command_line.push_error(crate::tf!(
                        "PURGE: unknown option \"{sub}\" — use ALL, Blocks, LAyers, LineTypes or STyles."
                    ).as_ref());
                    return Some(Task::none());
                }

                // Purging a block frees whatever only its members referenced
                // (nested blocks, layers, linetypes, styles), so those become
                // purgeable on the next pass — repeat until a pass removes
                // nothing, so one PURGE does what re-running it used to.
                let (mut n_layers, mut n_styles, mut n_lts, mut n_blocks) =
                    (0usize, 0usize, 0usize, 0usize);
                let mut snapshot_pushed = false;
                loop {
                    // Collect names in use (immutable borrows — own scope per pass).
                    // An INSERT's attributes live inside it, not in the entity
                    // list, so their layer, style and linetype count separately.
                    let open_edits: Vec<codec::Handle> = self.tabs[i]
                        .block_edits
                        .iter()
                        .map(|session| session.br_handle)
                        .collect();
                    let [layer_remove, style_remove, lt_remove, block_remove] =
                        super::purge::purge_pass(
                            &self.tabs[i].scene.document,
                            &open_edits,
                            [do_layers, do_styles, do_lts, do_blocks],
                        );

                    let pick = |list: Vec<String>, kind: usize| -> Vec<String> {
                        list.into_iter().filter(|n| wanted(n) && chosen(kind, n)).collect()
                    };
                    let layer_remove = pick(layer_remove, 0);
                    let style_remove = pick(style_remove, 1);
                    let lt_remove = pick(lt_remove, 2);
                    let block_remove = pick(block_remove, 3);
                    if layer_remove.len()
                        + style_remove.len()
                        + lt_remove.len()
                        + block_remove.len()
                        == 0
                    {
                        break;
                    }
                    // Snapshot the pre-purge document once, before the first
                    // removal, so a single UNDO restores everything.
                    if !snapshot_pushed {
                        self.push_undo_snapshot(i, "Purge");
                        snapshot_pushed = true;
                    }

                    // Apply removals (mutable). Count what actually went: an
                    // entry the table cannot remove by name would otherwise be
                    // listed again on every pass and never let the loop end.
                    let doc = &mut self.tabs[i].scene.document;
                    let layers_gone = layer_remove
                        .iter()
                        .filter(|n| doc.layers.remove(n).is_some())
                        .count();
                    let styles_gone = style_remove
                        .iter()
                        .filter(|n| doc.text_styles.remove(n).is_some())
                        .count();
                    let lts_gone = lt_remove
                        .iter()
                        .filter(|n| doc.line_types.remove(n).is_some())
                        .count();
                    let mut blocks_gone = 0usize;
                    for name in &block_remove {
                        // The service erases the definition, its member
                        // entities, both BLOCK/ENDBLK delimiters and any stale
                        // draw-order table, so a purge and a delete leave the
                        // same document behind.
                        if crate::scene::block_service::erase(&mut self.tabs[i].scene, name)
                            .is_some()
                        {
                            blocks_gone += 1;
                        }
                    }
                    n_layers += layers_gone;
                    n_styles += styles_gone;
                    n_lts += lts_gone;
                    n_blocks += blocks_gone;
                    if layers_gone + styles_gone + lts_gone + blocks_gone == 0 {
                        break;
                    }
                }

                // Remove draw-order tables whose owning blocks are gone.
                let mut n_sortents = 0usize;
                if do_blocks {
                    let live_blocks: rustc_hash::FxHashSet<codec::Handle> = self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .iter()
                        .map(|br| br.handle)
                        .collect();
                    let orphans: Vec<codec::Handle> = self.tabs[i]
                        .scene
                        .document
                        .objects
                        .iter()
                        .filter_map(|(h, o)| match o {
                            codec::objects::ObjectType::SortEntitiesTable(s)
                                if !live_blocks.contains(&s.block_owner_handle) =>
                            {
                                Some(*h)
                            }
                            _ => None,
                        })
                        .collect();
                    if !orphans.is_empty() {
                        // `snapshot_pushed` is this purge's last use — no
                        // re-assignment needed (a block purge already snapshotted;
                        // a snapshot here only matters when nothing else changed).
                        if !snapshot_pushed {
                            self.push_undo_snapshot(i, "Purge");
                        }
                        for h in &orphans {
                            self.tabs[i].scene.document.objects.remove(h);
                        }
                        n_sortents = orphans.len();
                    }
                }

                let purged = n_layers + n_styles + n_lts + n_blocks;
                if purged > 0 || n_sortents > 0 {
                    self.tabs[i].dirty = true;
                    // Rebuild the layer/style/linetype panel + ribbon caches so
                    // the removed definitions disappear from the UI immediately.
                    self.refresh_layer_panel();
                    // Per-type breakdown so the user sees exactly what went.
                    let mut parts: Vec<String> = Vec::new();
                    if n_layers > 0 {
                        parts.push(crate::tf!("{n_layers} layer(s)").into_owned());
                    }
                    if n_styles > 0 {
                        parts.push(crate::tf!("{n_styles} text style(s)").into_owned());
                    }
                    if n_lts > 0 {
                        parts.push(crate::tf!("{n_lts} linetype(s)").into_owned());
                    }
                    if n_blocks > 0 {
                        parts.push(crate::tf!("{n_blocks} block(s)").into_owned());
                    }
                    if n_sortents > 0 {
                        parts.push(
                            crate::tf!("{n_sortents} stale draw-order table(s)").into_owned(),
                        );
                    }
                    self.command_line.push_output(
                        crate::tf!(
                            "PURGE: {} item(s) removed — {}.",
                            purged + n_sortents,
                            parts.join(", ")
                        )
                        .as_ref(),
                    );
                } else {
                    self.command_line
                        .push_output(crate::t!("PURGE: nothing to purge.").as_ref());
                }
            }

            // ── CHPROP — change entity properties from command line ───────
            "CHPROP" => {
                use crate::command::SelectThenKeywordCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenKeywordCommand::new(
                    "CHPROP",
                    "CHPROP  property  [Layer / Color / Linetype / LtScale / Transparency]:",
                    vec![
                        ("Layer", "LAYER", Some("CHPROP  new layer name:")),
                        (
                            "Color",
                            "COLOR",
                            Some("CHPROP  new colour (name / 1-255 / ByLayer):"),
                        ),
                        ("Linetype", "LINETYPE", Some("CHPROP  new linetype name:")),
                        ("LtScale", "LTSCALE", Some("CHPROP  new linetype scale:")),
                        (
                            "Transparency",
                            "TRANSPARENCY",
                            Some("CHPROP  transparency 0-90:"),
                        ),
                    ],
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("CHPROP ") => {
                // Usage: CHPROP <property> <value>
                // Applies to currently selected entities.
                // Properties: LAYER, COLOR, LINETYPE, LTSCALE
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                let prop = parts.get(1).map(|s| s.to_uppercase()).unwrap_or_default();
                let value = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();

                if prop.is_empty() {
                    self.command_line.push_info(
                        crate::t!(
                            "Usage: CHPROP <prop> <val>  (props: LAYER COLOR LINETYPE LTSCALE)"
                        )
                        .as_ref(),
                    );
                } else {
                    let handles: Vec<_> = self.tabs[i]
                        .scene
                        .selected_entities()
                        .into_iter()
                        .map(|(h, _)| h)
                        .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                        .collect();
                    if handles.is_empty() {
                        self.command_line
                            .push_error(crate::t!("CHPROP: no entities selected.").as_ref());
                    } else {
                        // Validate value early to give clear errors
                        let color_val: Option<codec::types::Color> = if prop == "COLOR" {
                            value
                                .parse::<i16>()
                                .ok()
                                .map(codec::types::Color::from_index)
                        } else {
                            None
                        };
                        let ltscale_val: Option<f64> = if prop == "LTSCALE" {
                            value.parse().ok()
                        } else {
                            None
                        };
                        let transparency_val: Option<codec::types::Transparency> =
                            if prop == "TRANSPARENCY" {
                                value
                                    .parse::<f64>()
                                    .ok()
                                    .map(codec::types::Transparency::from_percent)
                            } else {
                                None
                            };

                        if (prop == "COLOR" && color_val.is_none())
                            || (prop == "LTSCALE" && ltscale_val.is_none())
                            || (prop == "TRANSPARENCY" && transparency_val.is_none())
                        {
                            self.command_line.push_error(
                                crate::tf!("CHPROP: invalid value '{}' for {}.", value, prop)
                                    .as_ref(),
                            );
                        } else {
                            let known = matches!(
                                prop.as_str(),
                                "LAYER" | "LINETYPE" | "LT" | "LTSCALE" | "COLOR" | "TRANSPARENCY"
                            );
                            if !known {
                                self.command_line.push_error(crate::tf!(
                                "CHPROP: unknown property '{}'. Use: LAYER COLOR LINETYPE LTSCALE TRANSPARENCY", prop
                            ).as_ref());
                            } else {
                                let mut changed = 0usize;
                                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                                    if let Some(entity) =
                                        app.tabs[i].scene.document.get_entity_mut(handle)
                                    {
                                        let common = entity.common_mut();
                                        match prop.as_str() {
                                            "LAYER" => {
                                                common.layer = value.clone();
                                                changed += 1;
                                            }
                                            "LINETYPE" | "LT" => {
                                                common.linetype = value.clone();
                                                changed += 1;
                                            }
                                            "LTSCALE" => {
                                                common.linetype_scale = ltscale_val.unwrap();
                                                changed += 1;
                                            }
                                            "COLOR" => {
                                                common.color = color_val.unwrap();
                                                common.color_name = None;
                                                common.color_book_handle = None;
                                                changed += 1;
                                            }
                                            "TRANSPARENCY" => {
                                                common.transparency = transparency_val.unwrap();
                                                changed += 1;
                                            }
                                            _ => unreachable!(),
                                        }
                                    }
                                });
                                // Colour / linetype / ltscale / transparency /
                                // layer are baked into the cached wire geometry —
                                // re-tessellate the changed entities so they
                                // repaint immediately (issue #231 class).
                                self.command_line.push_output(
                                    crate::tf!("CHPROP: {} entity/entities updated.", changed)
                                        .as_ref(),
                                );
                            }
                        }
                    }
                }
            }

            // ── SETBYLAYER — clear color/linetype/lineweight overrides ────
            // Resets the selected entities' direct property overrides back to
            // ByLayer so they follow their layer again.
            "SETBYLAYERMODE" => {
                let command = crate::modules::draw::modify::setbylayer::ModeCommand;
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            value if value.starts_with("SETBYLAYERMODE ") => {
                if let Ok(mode) = value
                    .trim_start_matches("SETBYLAYERMODE ")
                    .trim()
                    .parse::<u8>()
                {
                    crate::modules::draw::modify::setbylayer::set_mode(mode);
                } else {
                    self.command_line
                        .push_error("SETBYLAYERMODE requires an integer from 0 to 255.");
                }
            }
            "SETBYLAYER" | "-SETBYLAYER" => {
                let command = crate::modules::draw::modify::setbylayer::SetByLayerCommand::new(
                    self.tabs[i].scene.selected.iter().copied().collect(),
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            cmd if cmd.starts_with("SETBYLAYER_APPLY ") => {
                let flags: Vec<_> = cmd.split_whitespace().skip(1).collect();
                if flags.len() != 2 || flags.iter().any(|flag| !matches!(*flag, "0" | "1")) {
                    return Some(Task::none());
                }
                let mask = crate::modules::draw::modify::setbylayer::mode();
                let change_byblock = flags[0] == "1";
                let include_blocks = flags[1] == "1";
                let mut handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if include_blocks {
                    let mut visited: std::collections::HashSet<_> =
                        handles.iter().copied().collect();
                    let mut index = 0;
                    while index < handles.len() {
                        let children = match self.tabs[i].scene.document.get_entity(handles[index])
                        {
                            Some(codec::EntityType::Insert(insert)) => self.tabs[i]
                                .scene
                                .document
                                .block_records
                                .get(&insert.block_name)
                                .map(|block| block.entity_handles.clone())
                                .unwrap_or_default(),
                            _ => Vec::new(),
                        };
                        for child in children {
                            if visited.insert(child) && !self.tabs[i].scene.is_layer_locked(child) {
                                handles.push(child);
                            }
                        }
                        index += 1;
                    }
                }
                if handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("SETBYLAYER: select entities first.").as_ref());
                } else {
                    let has_changes = handles.iter().any(|handle| {
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(*handle)
                            .is_some_and(|entity| {
                                let mut common = entity.common().clone();
                                crate::modules::draw::modify::setbylayer::apply_mask(
                                    &mut common,
                                    mask,
                                    change_byblock,
                                )
                            })
                    });
                    if !has_changes {
                        self.command_line.push_output(
                            crate::t!("SETBYLAYER: no properties required changes.").as_ref(),
                        );
                        return Some(Task::none());
                    }
                    self.push_undo_snapshot(i, "SETBYLAYER");
                    let mut changed = Vec::new();
                    for handle in &handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(*handle) {
                            if crate::modules::draw::modify::setbylayer::apply_mask(
                                entity.common_mut(),
                                mask,
                                change_byblock,
                            ) {
                                changed.push(*handle);
                            }
                        }
                    }
                    self.tabs[i].dirty = true;
                    let changes: Vec<_> = changed
                        .iter()
                        .copied()
                        .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.refresh_properties();
                    self.command_line.push_output(
                        crate::tf!(
                            "SETBYLAYER: reset {} entity/entities to ByLayer.",
                            changed.len()
                        )
                        .as_ref(),
                    );
                }
            }

            // OVERKILL gathers objects, exposes cleanup settings (dialog, or the
            // command line for -OVERKILL), then applies one undo group.
            "OVERKILL" | "-OVERKILL" => {
                use crate::modules::draw::modify::overkill;
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .collect();
                let dialog = cmd == "OVERKILL";
                if dialog && !handles.is_empty() {
                    let settings =
                        *overkill::remembered().lock().unwrap_or_else(|e| e.into_inner());
                    self.overkill_dialog = Some(crate::ui::window::overkill::State::new(settings));
                    self.active_modal = Some(crate::app::ModalKind::Overkill);
                } else {
                    let command = overkill::OverkillCommand::new(handles, dialog);
                    self.command_line.push_info(&command.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(command));
                }
            }
            value if value.starts_with("OVERKILL_APPLY ") => {
                use codec::objects::ObjectType;
                let values: Vec<_> = value.split_whitespace().skip(1).collect();
                if values.len() != 6 {
                    return Some(self.finish_dispatch(cmd));
                }
                let settings = crate::modules::draw::modify::overkill::Settings {
                    tolerance: values[0].parse::<f64>().unwrap_or(1e-6),
                    ignore: values[1].parse::<u16>().unwrap_or(0),
                    optimize: values[2] == "1",
                    overlap: values[3] == "1",
                    end_to_end: values[4] == "1",
                    associativity: values[5] == "1",
                };
                let scene = &self.tabs[i].scene;
                // Reactors that are groups only record membership; any other
                // live reactor (dimension association, hatch boundary, ...)
                // makes the object associative.
                let associative = |e: &codec::EntityType| {
                    e.common().reactors.iter().any(|r| {
                        scene.document.get_entity(*r).is_some()
                            || scene
                                .document
                                .objects
                                .get(r)
                                .is_some_and(|o| !matches!(o, ObjectType::Group(_)))
                    })
                };
                let selected = scene.selected_entities();
                let found = selected.len();
                let mut candidates: Vec<_> = selected
                    .into_iter()
                    .filter(|(h, e)| {
                        !scene.is_layer_locked(*h) && !(settings.associativity && associative(e))
                    })
                    .map(|(h, e)| (h, e.clone()))
                    .collect();
                let (changed, removed) =
                    crate::modules::draw::modify::overkill::run(&mut candidates, &settings);
                if !changed.is_empty() || !removed.is_empty() {
                    self.push_undo_snapshot(i, "OVERKILL");
                    let mut changes = Vec::new();
                    for (handle, entity) in candidates {
                        if !changed.contains(&handle) {
                            continue;
                        }
                        if let Some(target) = self.tabs[i].scene.document.get_entity_mut(handle) {
                            *target = entity;
                            changes.push((handle, crate::scene::ChangeKind::Modified));
                        }
                    }
                    self.tabs[i].scene.bump_entities(&changes);
                    let mut handles: Vec<_> = removed.iter().copied().collect();
                    handles.sort_by_key(|handle| handle.value());
                    self.tabs[i].scene.erase_entities(&handles);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                    self.command_line.push_output(
                        crate::tf!(
                            "OVERKILL: {} object(s) checked, {} removed, {} modified.",
                            found,
                            removed.len(),
                            changed.len()
                        )
                        .as_ref(),
                    );
                } else {
                    self.command_line.push_output(
                        crate::tf!(
                            "OVERKILL: {} object(s) checked, no duplicates found.",
                            found
                        )
                        .as_ref(),
                    );
                }
            }
            // ── PICKADD / PICKDRAG — selection UX (#226, app settings) ───
            // Bare form reports; `<name> 0|1` sets and persists. Defaults keep
            // today's behaviour (PICKADD 1, PICKDRAG 0).
            "PICKADD" | "PICKDRAG" => {
                use crate::command::ValuePromptCommand;
                let (name, prompt) = if cmd == "PICKADD" {
                    ("PICKADD", "PICKADD  1 = click adds to selection (default), 0 = click replaces  <Enter reports>:")
                } else {
                    ("PICKDRAG", "PICKDRAG  0 = press-drag lassoes (default), 1 = press-drag draws a rectangle  <Enter reports>:")
                };
                let c = ValuePromptCommand::new(name, prompt);
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("PICKADD ") || cmd.starts_with("PICKDRAG ") => {
                let is_add = cmd.starts_with("PICKADD");
                let arg = cmd.splitn(2, ' ').nth(1).unwrap_or("").trim();
                if arg.is_empty() {
                    let v = if is_add {
                        u8::from(self.pick_add)
                    } else {
                        u8::from(self.pick_drag_rect)
                    };
                    self.command_line.push_output(
                        crate::tf!("{} = {v}", if is_add { "PICKADD" } else { "PICKDRAG" })
                            .as_ref(),
                    );
                } else {
                    match arg {
                        "0" | "1" => {
                            let on = arg == "1";
                            if is_add {
                                self.pick_add = on;
                                self.command_line.push_output(
                                    crate::tf!(
                                        "PICKADD = {} ({})",
                                        arg,
                                        if on {
                                            "click adds to selection"
                                        } else {
                                            "click replaces selection, Shift toggles"
                                        }
                                    )
                                    .as_ref(),
                                );
                            } else {
                                self.pick_drag_rect = on;
                                self.command_line.push_output(
                                    crate::tf!(
                                        "PICKDRAG = {} ({})",
                                        arg,
                                        if on {
                                            "press-drag draws a rectangle"
                                        } else {
                                            "press-drag lassoes"
                                        }
                                    )
                                    .as_ref(),
                                );
                            }
                            self.persist_settings_if_changed();
                        }
                        _ => self.command_line.push_error(
                            crate::tf!(
                                "{}: expected 0 or 1.",
                                if is_add { "PICKADD" } else { "PICKDRAG" }
                            )
                            .as_ref(),
                        ),
                    }
                }
            }

            // ── COMMANDLINEFADETIME — command-line history fade time ───
            // How long history lines stay visible above the command window,
            // in ms (0–60000, default 3000). Bare form prompts for a value
            // (Enter reports current); 0 hides transient overlay lines.
            "COMMANDLINEFADETIME" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "COMMANDLINEFADETIME",
                    "COMMANDLINEFADETIME  overlay fade time in ms (0-60000, 0 = hide)  <Enter reports>:",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }

            // ── SETVAR — read / write system variables ───────────────────
            // SETVAR <name>          → report the value
            // SETVAR <name> <value>  → set it
            // SETVAR ?               → list supported variables
            // Numeric / boolean variables are settable; current-layer/linetype/
            // style names are read-only here (use their own commands to change
            // them, which validate the name).
            // System variables typeable directly (e.g. `MIRRTEXT 1`) as well as
            // through SETVAR. (LTSCALE/PDMODE/PDSIZE have their own commands.)
            cmd if matches!(
                cmd.split_whitespace().next().unwrap_or(""),
                "MIRRTEXT"
                    | "ZOOMWHEEL"
                    | "ZOOMFACTOR"
                    | "SHORTCUTMENU"
                    | "SHORTCUTMENUDURATION"
                    | "CURSORSIZE"
                    | "PICKBOX"
                    | "APERTURE"
                    | "HPASSOC"
                    | "HPGAPTOL"
                    | "COMMANDLINEOPACITY"
                    | "MENUBAR"
                    | "CURSORTYPE"
                    | "SNAPANG"
                    | "TEXTFILL"
                    | "CLIPROMPTLINES"
                    | "COMMANDLINEFADETIME"
                    | "ATTREQ"
                    | "ATTDIA"
                    | "DIMASSOC"
                    | "ANGBASE"
                    | "ANGDIR"
                    | "REGENMODE"
                    | "BLIPMODE"
                    | "SPLFRAME"
                    | "CAMERADISPLAY"
                    | "DELOBJ"
                    | "SOLIDHIST"
                    | "SHOWHIST"
                    | "PLINEGEN"
                    | "PSLTSCALE"
                    | "DISPSILH"
                    | "WORLDVIEW"
                    | "LIMCHECK"
                    | "DRAGMODE"
                    | "LUNITS"
                    | "LUPREC"
                    | "AUNITS"
                    | "AUPREC"
                    | "THICKNESS"
                    | "ELEVATION"
                    | "INSUNITS"
                    | "SPLINETYPE"
                    | "ISOLINES"
                    | "DIMASO"
                    | "DIMSHO"
                    | "QTEXTMODE"
                    | "PLIMCHECK"
                    | "VISRETAIN"
                    | "USRTIMER"
                    | "ATTMODE"
                    | "COORDS"
                    | "OSMODE"
                    | "PICKSTYLE"
                    | "SPLINESEGS"
                    | "SURFU"
                    | "SURFV"
                    | "SURFTYPE"
                    | "SHADEDGE"
                    | "MAXACTVP"
                    | "CMLJUST"
                    | "CMLSCALE"
                    | "CMLSTYLE"
                    | "TEXTQLTY"
                    | "SORTENTS"
                    | "FRAME"
                    | "IMAGEFRAME"
                    | "PDFFRAME"
                    | "DWFFRAME"
                    | "DGNFRAME"
                    | "PDFOSNAP"
                    | "DWFOSNAP"
                    | "DGNOSNAP"
                    | "UOSNAP"
                    | "FIELDDISPLAY"
                    | "FILEDIA"
                    | "SDI"
                    | "TRIMEXTENDMODE"
                    | "PELLIPSE"
                    | "SELECTIONCYCLING"
                    | "OSNAPZ"
                    | "SELECTSIMILARMODE"
                    | "UCSDETECT"
                    | "SURFACEASSOCIATIVITY"
                    | "DGNIMPORTMODE"
                    | "UCSORTHO"
                    | "UCSVIEW"
                    | "LAYOUTCREATEVIEWPORT"
                    | "PDFIMPORTMODE"
                    | "PDFIMPORTFILTER"
                    | "PDFIMPORTLAYERS"
                    | "PDFIMPORTIMAGEPATH"
                    | "PDFSHXLAYER"
                    | "PDFSHXBESTFONT"
                    | "PDFSHXTEXTSETTINGS"
                    | "XDWGFADECTL"
                    | "POINTCLOUDCLIPFRAME"
                    | "POINTCLOUDDENSITY"
                    | "POINTCLOUDPOINTSIZE"
                    | "POINTCLOUDLOCK"
                    | "POINTCLOUDAUTOUPDATE"
                    | "POINTCLOUDBOUNDARY"
                    | "POINTCLOUDRTDENSITY"
                    | "POINTCLOUDLOD"
                    | "POINTCLOUDPOINTMAX"
                    | "POINTCLOUDVISRETAIN"
                    | "POINTCLOUDSHADING"
                    | "POINTCLOUDCACHESIZE"
                    | "POINTCLOUD2DVSDISPLAY"
                    | "POINTCLOUDLIGHTING"
                    | "POINTCLOUDLIGHTSOURCE"
                    | "POINTCLOUDPOINTMAXLEGACY"
                    | "XCLIPFRAME"
                    | "WIPEOUTFRAME"
                    | "HALOGAP"
                    | "TRACEWID"
                    | "SKETCHINC"
                    | "SKPOLY"
                    | "SKTOLERANCE"
                    | "DONUTID"
                    | "DONUTOD"
                    | "CENTEREXE"
                    | "CENTERLAYER"
                    | "CENTERLTYPE"
                    | "CENTERLTSCALE"
                    | "CENTERLTYPEFILE"
                    | "CENTERCROSSSIZE"
                    | "CENTERCROSSGAP"
                    | "CENTERMARKEXE"
                    | "DIMCONTINUEMODE"
                    | "COLORTHEME"
                    | "SELECTIONAREA"
                    | "SELECTIONAREAOPACITY"
                    | "SELECTIONEFFECT"
                    | "SELECTIONEFFECTCOLOR"
                    | "VSEDGECOLOR"
                    | "WINDOWSAREACOLOR"
                    | "WINDOWAREACOLOR"
                    | "CROSSINGAREACOLOR"
                    | "SELECTIONPREVIEW"
                    | "GRIPSIZE"
                    | "GRIPCOLOR"
                    | "GRIPHOT"
                    | "GRIPHOVER"
                    | "GRIPOBJLIMIT"
                    | "CONSTRAINTSOLVEMODE"
                    | "CONSTRAINTINFER"
                    | "CONSTRAINTBARDISPLAY"
                    | "CONSTRAINTBARMODE"
                    | "CONSTRAINTNAMEFORMAT"
                    | "DYNCONSTRAINTDISPLAY"
                    | "CCONSTRAINTFORM"
                    | "DYNMODE"
                    | "TXT2MTXTCOMBINEMTEXT"
                    | "TXT2MTXTTEXTORDER"
                    | "TXT2MTXTWORDWRAP"
                    | "TXT2MTXTLINESPACING"
            ) =>
            {
                return self.dispatch_styleprops(&format!("SETVAR {cmd}"), i);
            }

            "SETVAR" => {
                use crate::command::TwoValuePromptCommand;
                let c = TwoValuePromptCommand::new(
                    "SETVAR",
                    "SETVAR  variable name:",
                    "SETVAR  new value (blank to read):",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("SETVAR ") => {
                let rest = cmd.strip_prefix("SETVAR").unwrap_or("").trim();
                let mut it = rest.splitn(2, char::is_whitespace);
                let name = it.next().unwrap_or("").to_uppercase();
                let value = it.next().map(|s| s.trim().to_string());
                if name.is_empty() || name == "?" {
                    self.command_line
                        .push_info(&super::plotvars::setvar_listing());
                    self.command_line.push_info("SETVAR (view): UCSFOLLOW");
                    self.command_line
                        .push_info(&super::blockvars::setvar_listing());
                    self.command_line
                        .push_info(&super::block_editor_vars::setvar_listing());
                    self.command_line.push_info(
                        crate::t!("SETVAR: CETRANSPARENCY LTSCALE CELTSCALE PDMODE PDSIZE TEXTSIZE ORTHOMODE FILLMODE MIRRTEXT FRAME IMAGEFRAME PDFFRAME WIPEOUTFRAME XCLIPFRAME POINTCLOUDCLIPFRAME ZOOMWHEEL ZOOMFACTOR SHORTCUTMENU SHORTCUTMENUDURATION CURSORSIZE PICKBOX CURSORTYPE SNAPANG TEXTFILL CLIPROMPTLINES COMMANDLINEFADETIME ATTREQ ATTDIA DIMASSOC DIMCONTINUEMODE CONSTRAINTSOLVEMODE CONSTRAINTINFER CONSTRAINTBARDISPLAY CONSTRAINTBARMODE CONSTRAINTNAMEFORMAT DYNCONSTRAINTDISPLAY DYNMODE ANGBASE ANGDIR SKETCHINC SKPOLY SKTOLERANCE DONUTID DONUTOD CENTEREXE CENTERLAYER CENTERLTYPE CENTERLTSCALE CENTERLTYPEFILE CENTERCROSSSIZE CENTERCROSSGAP CENTERMARKEXE COLORTHEME SELECTIONAREA SELECTIONAREAOPACITY SELECTIONEFFECT SELECTIONEFFECTCOLOR WINDOWSAREACOLOR CROSSINGAREACOLOR SELECTIONPREVIEW GRIPSIZE GRIPCOLOR GRIPHOT GRIPHOVER GRIPOBJLIMIT | CLAYER CELTYPE TEXTSTYLE (read-only)").as_ref(),
                    );
                } else {
                    // Point cloud settings, kept with the drawing.
                    if let Some(setting) = crate::scene::model::point_cloud::setting(&name) {
                        let document = &self.tabs[i].scene.document;
                        let current =
                            crate::scene::model::point_cloud::setting_value(document, setting);
                        match value.as_deref().map(|value| setting.check(value)) {
                            Some(Ok(new)) => {
                                if new != current {
                                    crate::io::set_drawing_variable(
                                        &mut self.tabs[i].scene.document,
                                        setting.name,
                                        &new.to_string(),
                                    );
                                    self.tabs[i].scene.bump_geometry();
                                    self.tabs[i].dirty = true;
                                }
                            }
                            // Refused: say why, and ask again unless the value ends it.
                            Some(Err(refusal)) => {
                                for line in &refusal.lines {
                                    self.command_line.push_error(line);
                                }
                                if refusal.ask_again {
                                    self.command_line.push_output(&format!(
                                        "Enter new value for {name} <{current}>:"
                                    ));
                                    self.pending_setvar = Some(name.clone());
                                }
                            }
                            None => {
                                self.command_line.push_output(&format!(
                                    "Enter new value for {name} <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "XDWGFADECTL" {
                        let current = crate::scene::cache::block_cache::xref_fade_ctl();
                        if let Some(value) = &value {
                            match value.parse::<i32>().ok().filter(|v| (-90..=90).contains(v)) {
                                Some(fade) => self.set_xref_fade(fade),
                                // Refused: say why and ask again.
                                None => {
                                    self.command_line
                                        .push_error("Requires an integer between -90 and 90.");
                                    self.command_line.push_output(&format!(
                                        "Enter new value for XDWGFADECTL <{current}>:"
                                    ));
                                    self.pending_setvar = Some(name.clone());
                                }
                            }
                        } else {
                            self.command_line.push_output(&format!(
                                "Enter new value for XDWGFADECTL <{current}>:"
                            ));
                            self.pending_setvar = Some(name.clone());
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "CETRANSPARENCY" {
                        let current = self.tabs[i].scene.document.current_entity_transparency();
                        if let Some(value) = &value {
                            match crate::scene::creation_style::parse_current_transparency(value) {
                                Some(transparency) => {
                                    if current != transparency {
                                        self.push_undo_snapshot(i, &name);
                                        if !self.tabs[i].scene.document.set_current_entity_transparency(transparency) {
                                            self.discard_last_undo_entry(i);
                                            self.command_line.push_error("CETRANSPARENCY: drawing variable dictionary is invalid.");
                                            return Some(self.finish_dispatch(cmd));
                                        }
                                        self.tabs[i].dirty = true;
                                        self.refresh_properties();
                                    }
                                    self.command_line.push_output(&format!("CETRANSPARENCY = {}", crate::scene::creation_style::current_transparency_label(transparency)));
                                }
                                None => self.command_line.push_error("CETRANSPARENCY: expected ByLayer (-1), ByBlock (-2), or an integer from 0 to 90."),
                            }
                        } else {
                            self.command_line.push_output(&format!(
                                "Enter new value for CETRANSPARENCY <{}>:",
                                crate::scene::creation_style::current_transparency_label(current)
                            ));
                            self.pending_setvar = Some(name.clone());
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    // Snapping to the geometry inside underlays (one switch for
                    // PDF underlays and underlays in general).
                    // The PDF Import Settings' options as bits.
                    if matches!(
                        name.as_str(),
                        "PDFIMPORTMODE" | "PDFIMPORTFILTER" | "PDFIMPORTLAYERS"
                    ) {
                        use crate::modules::insert::pdf_import::{
                            import_settings, set_import_settings, ImportLayers,
                        };
                        let mut settings = import_settings();
                        let (current, max) = match name.as_str() {
                            "PDFIMPORTMODE" => (settings.mode(), 31),
                            "PDFIMPORTFILTER" => (settings.filter(), 15),
                            _ => (settings.layers_value(), 2),
                        };
                        if let Some(value) = &value {
                            match value.parse::<i16>().ok().filter(|v| (0..=max).contains(v)) {
                                Some(v) => {
                                    match name.as_str() {
                                        "PDFIMPORTMODE" => settings.set_mode(v),
                                        "PDFIMPORTFILTER" => settings.set_filter(v),
                                        _ => {
                                            settings.layers = [
                                                ImportLayers::Pdf,
                                                ImportLayers::Object,
                                                ImportLayers::Current,
                                            ][v as usize]
                                        }
                                    }
                                    set_import_settings(settings);
                                }
                                None => self.command_line.push_error(&format!(
                                    "Requires an integer between 0 and {max}."
                                )),
                            }
                        } else {
                            self.command_line
                                .push_output(&format!("Enter new value for {name} <{current}>:"));
                            self.pending_setvar = Some(name.clone());
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    // A folder name; "." clears it.
                    if name == "PDFIMPORTIMAGEPATH" {
                        use crate::modules::insert::pdf_import::{image_path, set_image_path};
                        match &value {
                            Some(value) => {
                                let value = value.trim().trim_matches('"');
                                set_image_path(if value == "." {
                                    String::new()
                                } else {
                                    value.to_string()
                                });
                            }
                            None => {
                                self.command_line.push_output(&format!(
                                    "Enter new value for PDFIMPORTIMAGEPATH, or . for none <\"{}\">:",
                                    image_path()
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    // PDFSHXTEXT's profile switches, taken as 1 / 0 or ON / OFF;
                    // PDFSHXTEXTSETTINGS shows its value as a number.
                    if matches!(
                        name.as_str(),
                        "PDFSHXLAYER" | "PDFSHXBESTFONT" | "PDFSHXTEXTSETTINGS"
                    ) {
                        use crate::modules::insert::pdf_shx_text::{set_settings, settings};
                        let mut s = settings();
                        let on = match name.as_str() {
                            "PDFSHXLAYER" => s.current_layer,
                            "PDFSHXBESTFONT" => s.best_font,
                            _ => s.show_settings,
                        };
                        let current = if name == "PDFSHXTEXTSETTINGS" {
                            i16::from(on).to_string()
                        } else if on {
                            "ON".to_string()
                        } else {
                            "OFF".to_string()
                        };
                        let parsed = value.as_deref().map(|v| {
                            match v
                                .trim()
                                .trim_start_matches('_')
                                .to_ascii_uppercase()
                                .as_str()
                            {
                                "1" | "ON" => Some(true),
                                "0" | "OFF" => Some(false),
                                _ => None,
                            }
                        });
                        match parsed {
                            Some(Some(v)) => {
                                if v != on {
                                    match name.as_str() {
                                        "PDFSHXLAYER" => s.current_layer = v,
                                        "PDFSHXBESTFONT" => s.best_font = v,
                                        _ => s.show_settings = v,
                                    }
                                    set_settings(s);
                                    self.save_config();
                                }
                            }
                            Some(None) => {
                                self.command_line.push_error(
                                    crate::t!("Requires 0 or 1 only, or option keyword.").as_ref(),
                                );
                                self.command_line.push_output(&format!(
                                    "Enter new value for {name} <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&format!(
                                    "Enter new value for {name} <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "LAYOUTCREATEVIEWPORT" {
                        // A profile switch shown and taken as ON / OFF or 1 / 0.
                        let current = if crate::scene::layout_create_viewport() {
                            "ON"
                        } else {
                            "OFF"
                        };
                        let parsed = value.as_deref().map(|v| {
                            match v
                                .trim()
                                .trim_start_matches('_')
                                .to_ascii_uppercase()
                                .as_str()
                            {
                                "1" | "ON" => Some(true),
                                "0" | "OFF" => Some(false),
                                _ => None,
                            }
                        });
                        match parsed {
                            Some(Some(on)) => {
                                if crate::scene::layout_create_viewport() != on {
                                    crate::scene::set_layout_create_viewport(on);
                                    self.save_config();
                                }
                            }
                            Some(None) => {
                                self.command_line.push_error(
                                    crate::t!("Requires 0 or 1 only, or option keyword.").as_ref(),
                                );
                                self.command_line.push_output(&format!(
                                    "Enter new value for {name} <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&format!(
                                    "Enter new value for {name} <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if matches!(name.as_str(), "UCSORTHO" | "UCSVIEW") {
                        use crate::modules::view::view_cmd;
                        let current = i16::from(if name == "UCSORTHO" {
                            view_cmd::ucs_ortho()
                        } else {
                            view_cmd::ucs_view()
                        });
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<i16>().ok().filter(|v| (0..=1).contains(v)))
                        {
                            Some(Some(mode)) => {
                                if current != mode {
                                    if name == "UCSORTHO" {
                                        view_cmd::set_ucs_ortho(mode == 1);
                                    } else {
                                        view_cmd::set_ucs_view(mode == 1);
                                    }
                                    self.save_config();
                                }
                            }
                            Some(None) => {
                                self.command_line
                                    .push_error(crate::t!("Requires 0 or 1 only.").as_ref());
                                self.command_line.push_output(&format!(
                                    "Enter new value for {name} <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&format!(
                                    "Enter new value for {name} <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "DGNIMPORTMODE" {
                        // Where DGNIMPORT puts the model: 0 a new drawing, 1 the
                        // current one keeping its definitions, 2 the current one
                        // with prefixed names.
                        use crate::modules::insert::dgn_import::{set_settings, settings};
                        let mut s = settings();
                        let current = s.mode;
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<u8>().ok().filter(|v| *v <= 2))
                        {
                            Some(Some(mode)) => {
                                if current != mode {
                                    s.mode = mode;
                                    set_settings(s);
                                    self.save_config();
                                }
                            }
                            Some(None) => {
                                self.command_line
                                    .push_error("Requires an integer between 0 and 2.");
                                self.command_line.push_output(&format!(
                                    "Enter new value for DGNIMPORTMODE <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&format!(
                                    "Enter new value for DGNIMPORTMODE <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "SURFACEASSOCIATIVITY" {
                        use crate::scene::model::sweep_model;
                        let current = i16::from(sweep_model::surface_associativity());
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<i16>().ok().filter(|v| (0..=1).contains(v)))
                        {
                            Some(Some(mode)) => {
                                if current != mode {
                                    sweep_model::set_surface_associativity(mode == 1);
                                    self.save_config();
                                }
                            }
                            Some(None) => {
                                self.command_line
                                    .push_error(crate::t!("Requires 0 or 1 only.").as_ref());
                                self.command_line.push_output(&format!(
                                    "Enter new value for SURFACEASSOCIATIVITY <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&format!(
                                    "Enter new value for SURFACEASSOCIATIVITY <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if let Some(field) = match name.as_str() {
                        "TXT2MTXTCOMBINEMTEXT" => Some(0),
                        "TXT2MTXTTEXTORDER" => Some(1),
                        "TXT2MTXTWORDWRAP" => Some(2),
                        "TXT2MTXTLINESPACING" => Some(3),
                        _ => None,
                    } {
                        // TXT2MTXT's profile settings: 0 / 1 (TEXTORDER 1 =
                        // selection order).
                        let s = &mut self.txt2mtxt_settings;
                        let slot = match field {
                            0 => &mut s.combine,
                            1 => &mut s.select_order,
                            2 => &mut s.word_wrap,
                            _ => &mut s.uniform_spacing,
                        };
                        let current = i16::from(*slot);
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<i16>().ok().filter(|v| (0..=1).contains(v)))
                        {
                            Some(Some(mode)) => {
                                if current != mode {
                                    *slot = mode == 1;
                                    self.save_config();
                                }
                            }
                            Some(None) => {
                                self.command_line
                                    .push_error(crate::t!("Requires 0 or 1 only.").as_ref());
                                self.command_line.push_output(&format!(
                                    "Enter new value for {name} <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&format!(
                                    "Enter new value for {name} <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "FIELDDISPLAY" {
                        let current = i16::from(crate::entities::field::display());
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<i16>().ok().filter(|v| (0..=1).contains(v)))
                        {
                            Some(Some(mode)) => {
                                if current != mode {
                                    crate::entities::field::set_display(mode == 1);
                                    // A profile setting: redraw the fields of every open drawing.
                                    for tab in &mut self.tabs {
                                        let changes: Vec<_> = tab
                                            .scene
                                            .document
                                            .entities()
                                            .filter(|e| {
                                                crate::entities::field::hosts_field(
                                                    &tab.scene.document,
                                                    e,
                                                )
                                            })
                                            .map(|e| {
                                                (
                                                    e.common().handle,
                                                    crate::scene::ChangeKind::Modified,
                                                )
                                            })
                                            .collect();
                                        if !changes.is_empty() {
                                            tab.scene.bump_entities(&changes);
                                        }
                                    }
                                    self.save_config();
                                }
                            }
                            Some(None) => {
                                self.command_line
                                    .push_error(crate::t!("Requires 0 or 1 only.").as_ref());
                                self.command_line.push_output(&format!(
                                    "Enter new value for FIELDDISPLAY <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&format!(
                                    "Enter new value for FIELDDISPLAY <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "TRIMEXTENDMODE" {
                        use crate::modules::draw::modify::trim;
                        let current = i16::from(trim::quick_mode());
                        let prompt = format!("Enter new value for TRIMEXTENDMODE <{current}>:");
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<i16>().ok().filter(|v| (0..=1).contains(v)))
                        {
                            Some(Some(mode)) => {
                                if current != mode {
                                    trim::set_quick_mode(mode == 1);
                                    self.save_config();
                                }
                            }
                            Some(None) => {
                                self.command_line
                                    .push_error(crate::t!("Requires 0 or 1 only.").as_ref());
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "PELLIPSE" {
                        use crate::modules::draw::defaults;
                        // Saved in the drawing (DWG header); ELLIPSE reads it.
                        let header = &self.tabs[i].scene.document.header;
                        let current = i16::from(header.create_ellipse_as_polyline);
                        let prompt = format!("Enter new value for PELLIPSE <{current}>:");
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<i16>().ok().filter(|v| (0..=1).contains(v)))
                        {
                            Some(Some(mode)) => {
                                let header = &mut self.tabs[i].scene.document.header;
                                if header.create_ellipse_as_polyline != (mode == 1) {
                                    header.create_ellipse_as_polyline = mode == 1;
                                    self.tabs[i].dirty = true;
                                }
                                defaults::set_pellipse(mode == 1);
                            }
                            Some(None) => {
                                self.command_line
                                    .push_error(crate::t!("Requires 0 or 1 only.").as_ref());
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "UCSDETECT" {
                        let current = i16::from(self.dynamic_ucs);
                        let prompt = format!("Enter new value for UCSDETECT <{current}>:");
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<i16>().ok().filter(|v| (0..=1).contains(v)))
                        {
                            Some(Some(mode)) => {
                                self.dynamic_ucs = mode == 1;
                                self.persist_settings_if_changed();
                            }
                            Some(None) => {
                                self.command_line
                                    .push_error(crate::t!("Requires 0 or 1 only.").as_ref());
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "SELECTSIMILARMODE" {
                        let current = self.select_similar_mode;
                        let prompt = format!("Enter new value for SELECTSIMILARMODE <{current}>:");
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<u16>().ok().filter(|v| *v <= 255))
                        {
                            Some(Some(mode)) => {
                                if self.select_similar_mode != mode {
                                    self.select_similar_mode = mode;
                                    self.persist_settings_if_changed();
                                }
                            }
                            Some(None) => {
                                let (min, max) = (0, 255);
                                self.command_line.push_error(&crate::tf!(
                                    "Requires an integer between {min} and {max}."
                                ));
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "OSNAPZ" {
                        let current = i16::from(self.osnap_z);
                        let prompt = format!("Enter new value for OSNAPZ <{current}>:");
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<i16>().ok().filter(|v| (0..=1).contains(v)))
                        {
                            Some(Some(mode)) => {
                                if self.osnap_z != (mode != 0) {
                                    self.osnap_z = mode != 0;
                                    self.persist_settings_if_changed();
                                }
                            }
                            Some(None) => {
                                let (min, max) = (0, 1);
                                self.command_line.push_error(&crate::tf!(
                                    "Requires an integer between {min} and {max}."
                                ));
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "SELECTIONCYCLING" {
                        // 1 and 2 both turn cycling on; the candidate list
                        // always shows.
                        let current = if self.selection_cycling { 2 } else { 0 };
                        let prompt = format!("Enter new value for SELECTIONCYCLING <{current}>:");
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<i16>().ok().filter(|v| (0..=2).contains(v)))
                        {
                            Some(Some(mode)) => {
                                if self.selection_cycling != (mode != 0) {
                                    self.selection_cycling = mode != 0;
                                    self.cycle_candidates = None;
                                    self.persist_settings_if_changed();
                                }
                            }
                            Some(None) => {
                                let (min, max) = (0, 2);
                                self.command_line.push_error(&crate::tf!(
                                    "Requires an integer between {min} and {max}."
                                ));
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    // SDI 1: one drawing at a time (OPEN closes the current one).
                    if name == "SDI" {
                        let current = i16::from(self.sdi);
                        let prompt = format!("Enter new value for SDI <{current}>:");
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<i16>().ok().filter(|v| (0..=1).contains(v)))
                        {
                            Some(Some(mode)) => self.sdi = mode == 1,
                            Some(None) => {
                                self.command_line
                                    .push_error(crate::t!("Requires 0 or 1 only.").as_ref());
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&prompt);
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if name == "FILEDIA" {
                        let current = i16::from(crate::sys::file_dia());
                        match value
                            .as_deref()
                            .map(|v| v.trim().parse::<i16>().ok().filter(|v| (0..=1).contains(v)))
                        {
                            Some(Some(mode)) => {
                                if current != mode {
                                    crate::sys::set_file_dia(mode == 1);
                                    self.save_config();
                                }
                            }
                            Some(None) => {
                                self.command_line
                                    .push_error(crate::t!("Requires 0 or 1 only.").as_ref());
                                self.command_line.push_output(&format!(
                                    "Enter new value for FILEDIA <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                            None => {
                                self.command_line.push_output(&format!(
                                    "Enter new value for FILEDIA <{current}>:"
                                ));
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if matches!(
                        name.as_str(),
                        "PDFOSNAP" | "DWFOSNAP" | "DGNOSNAP" | "UOSNAP"
                    ) {
                        use crate::scene::model::pdf_vector as pv;
                        use codec::entities::UnderlayType;
                        // UOSNAP sets every kind and reads 2 while they differ.
                        let kind = match name.as_str() {
                            "PDFOSNAP" => Some(UnderlayType::Pdf),
                            "DWFOSNAP" => Some(UnderlayType::Dwf),
                            "DGNOSNAP" => Some(UnderlayType::Dgn),
                            _ => None,
                        };
                        let current = kind
                            .map_or_else(pv::uosnap, |kind| i16::from(pv::underlay_osnap(kind)));
                        if let Some(value) = &value {
                            match value
                                .parse::<i16>()
                                .ok()
                                .filter(|value| (0..=1).contains(value))
                            {
                                Some(mode) => {
                                    if current != mode {
                                        match kind {
                                            Some(kind) => pv::set_underlay_osnap(kind, mode == 1),
                                            None => pv::set_uosnap(mode == 1),
                                        }
                                        self.tabs[i].scene.reseed_underlays();
                                        self.sync_underlay_tab();
                                    }
                                }
                                None => self.command_line.push_error("Requires 0 or 1 only"),
                            }
                        } else {
                            self.command_line
                                .push_output(&format!("Enter new value for {name} <{current}>:"));
                            self.pending_setvar = Some(name.clone());
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if matches!(name.as_str(), "SHOWHIST" | "SOLIDHIST") {
                        let current = if name == "SHOWHIST" {
                            self.tabs[i]
                                .scene
                                .document
                                .header
                                .show_solid_history
                                .clamp(0, 2)
                        } else {
                            i16::from(self.tabs[i].scene.document.header.record_solid_history)
                        };
                        if let Some(value) = &value {
                            let maximum = if name == "SHOWHIST" { 2 } else { 1 };
                            match value
                                .parse::<i16>()
                                .ok()
                                .filter(|value| (0..=maximum).contains(value))
                            {
                                Some(mode) => {
                                    if current != mode {
                                        self.push_undo_snapshot(i, &name);
                                        if name == "SHOWHIST" {
                                            self.tabs[i].scene.document.header.show_solid_history =
                                                mode;
                                            self.tabs[i].scene.bump_geometry();
                                        } else {
                                            self.tabs[i]
                                                .scene
                                                .document
                                                .header
                                                .record_solid_history = mode != 0;
                                        }
                                        self.tabs[i].dirty = true;
                                        self.refresh_properties();
                                    }
                                    self.command_line
                                        .push_output(&crate::tf!("{name} = {mode}"));
                                }
                                None => self.command_line.push_error(&crate::tf!(
                                    "{name}: expected an integer from 0 to {maximum}."
                                )),
                            }
                        } else {
                            self.command_line.push_output(&crate::tf!(
                                "Enter new value for {name} <{current}>:"
                            ));
                            self.pending_setvar = Some(name.clone());
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    let frame_kind = crate::scene::frame::kind_for_name(&name);
                    if name == "FRAME" || frame_kind.is_some() {
                        let current = frame_kind.map_or_else(
                            || crate::scene::frame::master_mode(&self.tabs[i].scene.document),
                            |kind| crate::scene::frame::mode(&self.tabs[i].scene.document, kind),
                        );
                        match &value {
                            Some(value) => match value.parse::<i16>() {
                                Ok(mode @ 0..=2) => {
                                    if current != mode {
                                        let changed_kinds: Vec<_> = frame_kind.map_or_else(
                                            || {
                                                crate::scene::frame::ALL_KINDS
                                                    .into_iter()
                                                    .filter(|kind| {
                                                        crate::scene::frame::mode(
                                                            &self.tabs[i].scene.document,
                                                            *kind,
                                                        ) != mode
                                                    })
                                                    .collect()
                                            },
                                            |kind| vec![kind],
                                        );
                                        self.push_undo_snapshot(i, &name);
                                        if let Some(kind) = frame_kind {
                                            crate::scene::frame::set_mode(
                                                &mut self.tabs[i].scene.document,
                                                kind,
                                                mode,
                                            );
                                        } else {
                                            crate::scene::frame::set_master_mode(
                                                &mut self.tabs[i].scene.document,
                                                mode,
                                            );
                                        }
                                        let changes: Vec<_> = self.tabs[i]
                                            .scene
                                            .document
                                            .entities()
                                            .filter_map(|entity| {
                                                changed_kinds
                                                    .iter()
                                                    .any(|kind| {
                                                        crate::scene::frame::affected(entity, *kind)
                                                    })
                                                    .then_some((
                                                        entity.common().handle,
                                                        crate::scene::ChangeKind::Modified,
                                                    ))
                                            })
                                            .collect();
                                        if !changes.is_empty() {
                                            self.tabs[i].scene.bump_entities(&changes);
                                        }
                                        self.tabs[i].dirty = true;
                                        // IMAGEFRAME also lands in the profile.
                                        self.save_config();
                                    }
                                    self.command_line
                                        .push_output(crate::t!("Regenerating model.").as_ref());
                                }
                                // Out of range: say so and ask again.
                                _ => {
                                    self.command_line.push_error(
                                        crate::t!("Requires an integer between 0 and 2.").as_ref(),
                                    );
                                    self.command_line.push_output(
                                        crate::tf!("Enter new value for {name} <{current}>:")
                                            .as_ref(),
                                    );
                                    self.pending_setvar = Some(name.clone());
                                }
                            },
                            None => {
                                self.command_line.push_output(
                                    crate::tf!("Enter new value for {name} <{current}>:").as_ref(),
                                );
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if crate::app::commands::center_vars::is_center_variable(&name) {
                        let task = self.center_setvar(&name, value.as_deref(), i);
                        let done = self.finish_dispatch(cmd);
                        return Some(Task::batch([task, done]));
                    }
                    if name == "DIMCONTINUEMODE" {
                        let current = self.dimension_continue_mode;
                        if let Some(value) = &value {
                            match value.parse::<i16>() {
                                Ok(mode @ 0..=1) => {
                                    self.dimension_continue_mode = mode;
                                    self.persist_settings_if_changed();
                                    self.command_line
                                        .push_output(&crate::tf!("DIMCONTINUEMODE = {mode}"));
                                }
                                _ => self.command_line.push_error(
                                    crate::t!("SETVAR: DIMCONTINUEMODE requires 0 or 1.").as_ref(),
                                ),
                            }
                        } else {
                            self.command_line.push_output(
                                crate::tf!("Enter new value for {name} <{current}>:").as_ref(),
                            );
                            self.pending_setvar = Some(name.clone());
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if matches!(
                        name.as_str(),
                        "CONSTRAINTSOLVEMODE"
                            | "CONSTRAINTINFER"
                            | "CONSTRAINTBARDISPLAY"
                            | "CONSTRAINTBARMODE"
                            | "CONSTRAINTNAMEFORMAT"
                            | "DYNCONSTRAINTDISPLAY"
                            | "CCONSTRAINTFORM"
                    ) {
                        let current = match name.as_str() {
                            "CONSTRAINTSOLVEMODE" => i16::from(self.constraint_solve_mode),
                            "CONSTRAINTINFER" => i16::from(self.constraint_infer),
                            "CONSTRAINTBARDISPLAY" => self.constraint_bar_display,
                            "CONSTRAINTBARMODE" => self.constraint_bar_mode,
                            "CONSTRAINTNAMEFORMAT" => {
                                i16::from(self.tabs[i].scene.constraint_name_format)
                            }
                            "DYNCONSTRAINTDISPLAY" => {
                                i16::from(self.tabs[i].scene.dynamic_constraint_display)
                            }
                            "CCONSTRAINTFORM" => i16::from(self.constraint_form_annotational),
                            _ => unreachable!(),
                        };
                        let maximum = match name.as_str() {
                            "CONSTRAINTBARDISPLAY" => 3,
                            "CONSTRAINTBARMODE" => 4095,
                            "CONSTRAINTNAMEFORMAT" => 2,
                            _ => 1,
                        };
                        if let Some(value) = &value {
                            match value
                                .parse::<i16>()
                                .ok()
                                .filter(|value| (0..=maximum).contains(value))
                            {
                                Some(mode) => {
                                    match name.as_str() {
                                        "CONSTRAINTSOLVEMODE" => {
                                            self.constraint_solve_mode = mode != 0
                                        }
                                        "CONSTRAINTINFER" => self.constraint_infer = mode != 0,
                                        "CONSTRAINTBARDISPLAY" => {
                                            self.constraint_bar_display = mode
                                        }
                                        "CONSTRAINTBARMODE" => self.constraint_bar_mode = mode,
                                        "CONSTRAINTNAMEFORMAT" => {
                                            self.tabs[i].scene.constraint_name_format = mode as u8;
                                            self.tabs[i].scene.refresh_dynamic_dimension_texts();
                                            self.tabs[i].dirty = true;
                                        }
                                        "DYNCONSTRAINTDISPLAY" => {
                                            self.tabs[i].scene.dynamic_constraint_display =
                                                mode != 0;
                                            self.tabs[i].scene.refresh_hidden_dynamic_dimensions();
                                        }
                                        // The form new dimensional constraints take
                                        // (0 dynamic, 1 annotational), as DCFORM sets.
                                        "CCONSTRAINTFORM" => {
                                            self.constraint_form_annotational = mode != 0
                                        }
                                        _ => unreachable!(),
                                    }
                                    self.persist_settings_if_changed();
                                    self.command_line
                                        .push_output(&crate::tf!("{name} = {mode}"));
                                }
                                None => self.command_line.push_error(&crate::tf!(
                                    "{name}: expected an integer from 0 to {maximum}."
                                )),
                            }
                        } else {
                            self.command_line.push_output(
                                crate::tf!("Enter new value for {name} <{current}>:").as_ref(),
                            );
                            self.pending_setvar = Some(name.clone());
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    if matches!(name.as_str(), "DONUTID" | "DONUTOD") {
                        let current = if name == "DONUTID" {
                            crate::modules::draw::defaults::get_donut_inner_diameter()
                        } else {
                            crate::modules::draw::defaults::get_donut_outer_diameter()
                        };
                        if let Some(value) = &value {
                            let parsed = value
                                .trim()
                                .replace(',', ".")
                                .parse::<f64>()
                                .ok()
                                .filter(|number| number.is_finite());
                            let valid = parsed.filter(|number| {
                                if name == "DONUTID" {
                                    *number >= 0.0
                                } else {
                                    *number > 0.0
                                }
                            });
                            if let Some(number) = valid {
                                if name == "DONUTID" {
                                    crate::modules::draw::defaults::set_donut_inner_diameter(
                                        number,
                                    );
                                } else {
                                    crate::modules::draw::defaults::set_donut_outer_diameter(
                                        number,
                                    );
                                }
                                self.command_line
                                    .push_output(&crate::tf!("{name} = {number}"));
                            } else {
                                self.command_line.push_error(crate::t!(if name == "DONUTID" {
                                    "SETVAR: DONUTID requires a finite value greater than or equal to zero."
                                } else {
                                    "SETVAR: DONUTOD requires a finite value greater than zero."
                                }).as_ref());
                            }
                        } else {
                            self.command_line.push_output(
                                crate::tf!("Enter new value for {name} <{current}>:").as_ref(),
                            );
                            self.pending_setvar = Some(name.clone());
                        }
                        return Some(self.finish_dispatch(cmd));
                    }
                    // Parse a boolean given as 0/1 or ON/OFF.
                    let parse_bool = |s: &str| match s.to_uppercase().as_str() {
                        "1" | "ON" | "TRUE" => Some(true),
                        "0" | "OFF" | "FALSE" => Some(false),
                        _ => None,
                    };
                    let current_delete_objects = self.delete_objects;
                    let requested_delete_objects = (name == "DELOBJ")
                        .then(|| value.as_deref()?.parse::<i16>().ok())
                        .flatten()
                        .filter(|value| (0..=3).contains(value));
                    let outcome: Result<(String, bool), String> = {
                        let h = &mut self.tabs[i].scene.document.header;
                        match name.as_str() {
                            // The standalone LTSCALE and CELTSCALE commands
                            // require a positive value; reaching the same
                            // variable through SETVAR used to accept zero or a
                            // negative one, which degenerates every dash
                            // pattern in the drawing.
                            "LTSCALE" => match &value {
                                Some(v) => v
                                    .parse::<f64>()
                                    .ok()
                                    .filter(|x| x.is_finite() && *x > 0.0)
                                    .map(|x| {
                                        h.linetype_scale = x;
                                        (format!("LTSCALE = {x}"), true)
                                    })
                                    .ok_or_else(|| {
                                        "SETVAR: positive numeric value required.".into()
                                    }),
                                None => Ok((format!("LTSCALE = {}", h.linetype_scale), false)),
                            },
                            "CELTSCALE" => match &value {
                                Some(v) => v
                                    .parse::<f64>()
                                    .ok()
                                    .filter(|x| x.is_finite() && *x > 0.0)
                                    .map(|x| {
                                        h.current_entity_linetype_scale = x;
                                        (format!("CELTSCALE = {x}"), true)
                                    })
                                    .ok_or_else(|| {
                                        "SETVAR: positive numeric value required.".into()
                                    }),
                                None => Ok((
                                    format!("CELTSCALE = {}", h.current_entity_linetype_scale),
                                    false,
                                )),
                            },
                            "PDSIZE" => match &value {
                                Some(v) => v
                                    .parse::<f64>()
                                    .map(|x| {
                                        h.point_display_size = x;
                                        (format!("PDSIZE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: numeric value required.".into()),
                                None => Ok((format!("PDSIZE = {}", h.point_display_size), false)),
                            },
                            "TEXTSIZE" => match &value {
                                Some(v) => v
                                    .parse::<f64>()
                                    .map(|x| {
                                        h.text_height = x;
                                        (format!("TEXTSIZE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: numeric value required.".into()),
                                None => Ok((format!("TEXTSIZE = {}", h.text_height), false)),
                            },
                            "PDMODE" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.point_display_mode = x;
                                        (format!("PDMODE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("PDMODE = {}", h.point_display_mode), false)),
                            },
                            "ORTHOMODE" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.ortho_mode = b;
                                        (format!("ORTHOMODE = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((format!("ORTHOMODE = {}", h.ortho_mode as i32), false)),
                            },
                            "FILLMODE" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.fill_mode = b;
                                        (format!("FILLMODE = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((format!("FILLMODE = {}", h.fill_mode as i32), false)),
                            },
                            "MIRRTEXT" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.mirror_text = b;
                                        (format!("MIRRTEXT = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((format!("MIRRTEXT = {}", h.mirror_text as i32), false)),
                            },
                            "DYNMODE" => match &value {
                                Some(v) => match v.parse::<i16>() {
                                    Ok(mode) if (-3..=3).contains(&mode) => {
                                        self.dyn_mode = mode;
                                        self.dyn_input = mode > 0;
                                        Ok((format!("DYNMODE = {mode}"), true))
                                    }
                                    _ => Err("SETVAR: integer from -3 to 3 required.".into()),
                                },
                                None => Ok((format!("DYNMODE = {}", self.dyn_mode), false)),
                            },
                            "ZOOMWHEEL" => match &value {
                                Some(v) => match parse_bool(v) {
                                    Some(reversed) => {
                                        self.zoom_wheel_reversed = reversed;
                                        Ok((format!("ZOOMWHEEL = {}", reversed as i32), true))
                                    }
                                    None => Err("SETVAR: 0 or 1 required.".into()),
                                },
                                None => Ok((
                                    format!("ZOOMWHEEL = {}", self.zoom_wheel_reversed as i32),
                                    false,
                                )),
                            },
                            // The system variable keeps its own range. The
                            // Options window reaches further
                            // (`settings::ZOOM_FACTOR_MAX`) and may have
                            // stored a value above this, which a bare
                            // `SETVAR ZOOMFACTOR` still reports; only setting
                            // one is held to the range of the variable.
                            "ZOOMFACTOR" => match &value {
                                Some(v) => match v.parse::<i32>() {
                                    Ok(factor)
                                        if (crate::app::settings::ZOOM_FACTOR_MIN
                                            ..=crate::app::settings::ZOOM_FACTOR_MAX)
                                            .contains(&factor) =>
                                    {
                                        self.zoom_factor = factor;
                                        Ok((format!("ZOOMFACTOR = {factor}"), true))
                                    }
                                    _ => Err(format!(
                                        "SETVAR: integer from {} to {} required.",
                                        crate::app::settings::ZOOM_FACTOR_MIN,
                                        crate::app::settings::ZOOM_FACTOR_MAX
                                    )),
                                },
                                None => Ok((format!("ZOOMFACTOR = {}", self.zoom_factor), false)),
                            },
                            // Bit code of commercial solutions: 0 = no shortcut menus (right-click
                            // is Enter), 16 = time-sensitive; everything else is the
                            // regular shortcut menu. Reported as the defaults of commercial solutions
                            // (11 = default+edit+command menus, +16 when
                            // time-sensitive).
                            "SHORTCUTMENU" => {
                                use crate::app::settings::RightClickMode;
                                let code = |mode: RightClickMode| match mode {
                                    RightClickMode::EnterFirst => 0,
                                    RightClickMode::ShortcutMenu => 11,
                                    RightClickMode::TimeSensitive => 27,
                                };
                                match &value {
                                    Some(v) => match v.parse::<i32>() {
                                        Ok(bits) if (0..=31).contains(&bits) => {
                                            self.right_click_mode = if bits == 0 {
                                                RightClickMode::EnterFirst
                                            } else if bits & 16 != 0 {
                                                RightClickMode::TimeSensitive
                                            } else {
                                                RightClickMode::ShortcutMenu
                                            };
                                            Ok((
                                                format!(
                                                    "SHORTCUTMENU = {}",
                                                    code(self.right_click_mode)
                                                ),
                                                true,
                                            ))
                                        }
                                        _ => Err("SETVAR: integer from 0 to 31 required.".into()),
                                    },
                                    None => Ok((
                                        format!("SHORTCUTMENU = {}", code(self.right_click_mode)),
                                        false,
                                    )),
                                }
                            }
                            "SHORTCUTMENUDURATION" => match &value {
                                Some(v) => match v.parse::<i32>() {
                                    Ok(ms) if (100..=1000).contains(&ms) => {
                                        self.right_click_hold_ms = ms;
                                        Ok((format!("SHORTCUTMENUDURATION = {ms}"), true))
                                    }
                                    _ => Err("SETVAR: integer from 100 to 1000 required.".into()),
                                },
                                None => Ok((
                                    format!("SHORTCUTMENUDURATION = {}", self.right_click_hold_ms),
                                    false,
                                )),
                            },
                            "CURSORSIZE" => match &value {
                                Some(v) => match v.parse::<i32>() {
                                    Ok(size) if (1..=100).contains(&size) => {
                                        self.cursor_size = size;
                                        Ok((format!("CURSORSIZE = {size}"), true))
                                    }
                                    _ => Err("SETVAR: integer from 1 to 100 required.".into()),
                                },
                                None => Ok((format!("CURSORSIZE = {}", self.cursor_size), false)),
                            },
                            "PICKBOX" => match &value {
                                Some(v) => match v.parse::<i32>() {
                                    Ok(size) if (0..=50).contains(&size) => {
                                        self.pick_box = size;
                                        Ok((format!("PICKBOX = {size}"), true))
                                    }
                                    _ => Err("SETVAR: integer from 0 to 50 required.".into()),
                                },
                                None => Ok((format!("PICKBOX = {}", self.pick_box), false)),
                            },
                            "APERTURE" => {
                                let (min, max) = (
                                    crate::snap::MIN_OSNAP_RADIUS_PX,
                                    crate::snap::MAX_OSNAP_RADIUS_PX,
                                );
                                match &value {
                                    Some(v) => match v.parse::<i32>() {
                                        Ok(px) if (min as i32..=max as i32).contains(&px) => {
                                            self.snapper.osnap_radius_px = px as f32;
                                            Ok((format!("APERTURE = {px}"), true))
                                        }
                                        _ => Err(format!(
                                            "SETVAR: integer from {min} to {max} required."
                                        )),
                                    },
                                    None => Ok((
                                        format!("APERTURE = {}", self.snapper.osnap_radius_px),
                                        false,
                                    )),
                                }
                            }
                            "MENUBAR" => match value.as_deref() {
                                Some(v @ ("0" | "1")) => {
                                    self.show_menu_bar = v == "1";
                                    Ok((format!("MENUBAR = {v}"), true))
                                }
                                Some(_) => Err("SETVAR: 0 or 1 required.".into()),
                                None => Ok((
                                    format!("MENUBAR = {}", u8::from(self.show_menu_bar)),
                                    false,
                                )),
                            },
                            "HPASSOC" => match value.as_deref() {
                                Some(v @ ("0" | "1")) => {
                                    self.hatch_associative = v == "1";
                                    Ok((format!("HPASSOC = {v}"), true))
                                }
                                Some(_) => Err("SETVAR: 0 or 1 required.".into()),
                                None => Ok((
                                    format!("HPASSOC = {}", u8::from(self.hatch_associative)),
                                    false,
                                )),
                            },
                            "HPGAPTOL" => {
                                let max = crate::modules::draw::draw::hatch::MAX_GAP_TOLERANCE;
                                match value.as_deref().map(|v| v.replace(',', ".").parse::<f64>()) {
                                    Some(Ok(v)) if (0.0..=max).contains(&v) => {
                                        self.hatch_gap_tolerance = v;
                                        Ok((format!("HPGAPTOL = {v}"), true))
                                    }
                                    Some(_) => {
                                        Err(format!("SETVAR: value must be between 0 and {max}."))
                                    }
                                    None => Ok((
                                        format!("HPGAPTOL = {}", self.hatch_gap_tolerance),
                                        false,
                                    )),
                                }
                            }
                            "CURSORTYPE" => match &value {
                                Some(v) => match v.as_str() {
                                    "0" => {
                                        self.cursor_type =
                                            crate::app::settings::CursorType::Crosshair;
                                        Ok(("CURSORTYPE = 0".to_string(), true))
                                    }
                                    "1" => {
                                        self.cursor_type =
                                            crate::app::settings::CursorType::Pointer;
                                        Ok(("CURSORTYPE = 1".to_string(), true))
                                    }
                                    _ => Err("SETVAR: 0 or 1 required.".into()),
                                },
                                None => Ok((
                                    format!(
                                        "CURSORTYPE = {}",
                                        i32::from(
                                            self.cursor_type
                                                == crate::app::settings::CursorType::Pointer
                                        )
                                    ),
                                    false,
                                )),
                            },
                            "COLORTHEME" => match &value {
                                Some(v) => match v.as_str() {
                                    "0" => {
                                        if self.ui_theme.name == "Custom" {
                                            self.saved_custom_palette = Some(self.ui_theme.palette);
                                        }
                                        self.ui_theme.name = iced::Theme::Dark.to_string();
                                        self.ui_theme.palette =
                                            crate::app::config::UiThemePalette::from_iced(
                                                iced::Theme::Dark.seed(),
                                            );
                                        self.theme_color_inputs =
                                            self.ui_theme.palette.hex_values();
                                        self.active_theme = iced::Theme::Dark;
                                        self.sync_model_space_theme(true);
                                        Ok(("COLORTHEME = 0 (Dark)".to_string(), true))
                                    }
                                    "1" => {
                                        if self.ui_theme.name == "Custom" {
                                            self.saved_custom_palette = Some(self.ui_theme.palette);
                                        }
                                        self.ui_theme.name = iced::Theme::Light.to_string();
                                        self.ui_theme.palette =
                                            crate::app::config::UiThemePalette::from_iced(
                                                iced::Theme::Light.seed(),
                                            );
                                        self.theme_color_inputs =
                                            self.ui_theme.palette.hex_values();
                                        self.active_theme = iced::Theme::Light;
                                        self.sync_model_space_theme(true);
                                        Ok(("COLORTHEME = 1 (Light)".to_string(), true))
                                    }
                                    _ => Err("SETVAR: 0 (Dark) or 1 (Light) required.".into()),
                                },
                                None => {
                                    let bg = self.ui_theme.palette.background;
                                    let lum = 0.299 * (bg[0] as f32)
                                        + 0.587 * (bg[1] as f32)
                                        + 0.114 * (bg[2] as f32);
                                    let code = if lum > 128.0 { 1 } else { 0 };
                                    Ok((
                                        format!("COLORTHEME = {code} ({})", self.ui_theme.name),
                                        false,
                                    ))
                                }
                            },
                            "SELECTIONAREA" => match &value {
                                Some(v) => match v.as_str() {
                                    "0" => {
                                        self.model_space.selection_area = false;
                                        self.sync_model_space_theme(false);
                                        Ok(("SELECTIONAREA = 0".to_string(), true))
                                    }
                                    "1" => {
                                        self.model_space.selection_area = true;
                                        self.sync_model_space_theme(false);
                                        Ok(("SELECTIONAREA = 1".to_string(), true))
                                    }
                                    _ => Err("SETVAR: 0 or 1 required.".into()),
                                },
                                None => Ok((
                                    format!(
                                        "SELECTIONAREA = {}",
                                        if self.model_space.selection_area {
                                            1
                                        } else {
                                            0
                                        }
                                    ),
                                    false,
                                )),
                            },
                            "SELECTIONAREAOPACITY" => match &value {
                                Some(v) => match v.parse::<u8>() {
                                    Ok(op) if op <= 100 => {
                                        self.model_space.selection_opacity = op;
                                        self.sync_model_space_theme(false);
                                        Ok((format!("SELECTIONAREAOPACITY = {op}"), true))
                                    }
                                    _ => Err("SETVAR: integer from 0 to 100 required.".into()),
                                },
                                None => Ok((
                                    format!(
                                        "SELECTIONAREAOPACITY = {}",
                                        self.model_space.selection_opacity
                                    ),
                                    false,
                                )),
                            },
                            "SELECTIONEFFECT" => match &value {
                                Some(v) => match v.as_str() {
                                    "0" => {
                                        self.model_space.selection_effect = false;
                                        self.sync_model_space_theme(false);
                                        Ok(("SELECTIONEFFECT = 0".to_string(), true))
                                    }
                                    "1" => {
                                        self.model_space.selection_effect = true;
                                        self.sync_model_space_theme(false);
                                        Ok(("SELECTIONEFFECT = 1".to_string(), true))
                                    }
                                    _ => Err("SETVAR: 0 or 1 required.".into()),
                                },
                                None => Ok((
                                    format!(
                                        "SELECTIONEFFECT = {}",
                                        if self.model_space.selection_effect {
                                            1
                                        } else {
                                            0
                                        }
                                    ),
                                    false,
                                )),
                            },
                            "SELECTIONEFFECTCOLOR" => {
                                match &value {
                                    Some(v) => match v.parse::<u8>() {
                                        Ok(color) => {
                                            self.model_space.selection_highlight_color = color;
                                            self.sync_model_space_theme(false);
                                            Ok((format!("SELECTIONEFFECTCOLOR = {color}"), true))
                                        }
                                        _ => Err("SETVAR: integer from 0 (Theme) to 255 required."
                                            .into()),
                                    },
                                    None => Ok((
                                        format!(
                                            "SELECTIONEFFECTCOLOR = {}",
                                            self.model_space.selection_highlight_color
                                        ),
                                        false,
                                    )),
                                }
                            }
                            // An index, a colour name or R,G,B; BLACK (or 0)
                            // goes back to the default black edges.
                            "VSEDGECOLOR" => {
                                let shown = |rgb: Option<[u8; 3]>| match rgb {
                                    Some([r, g, b]) => format!("{r},{g},{b}"),
                                    None => "BLACK".to_string(),
                                };
                                match &value {
                                    Some(v) => match parse_edge_color(v) {
                                        Some(rgb) => {
                                            self.model_space.edge_color = rgb;
                                            self.sync_model_space_theme(false);
                                            Ok((format!("VSEDGECOLOR = {}", shown(rgb)), true))
                                        }
                                        None => Err(
                                            "SETVAR: a colour index, name or R,G,B required."
                                                .into(),
                                        ),
                                    },
                                    None => Ok((
                                        format!(
                                            "VSEDGECOLOR = {}",
                                            shown(self.model_space.edge_color)
                                        ),
                                        false,
                                    )),
                                }
                            }
                            "WINDOWSAREACOLOR" | "WINDOWAREACOLOR" => {
                                match &value {
                                    Some(v) => match v.parse::<u8>() {
                                        Ok(color) => {
                                            self.model_space.selection_window_color = color;
                                            self.sync_model_space_theme(false);
                                            Ok((format!("WINDOWSAREACOLOR = {color}"), true))
                                        }
                                        _ => Err("SETVAR: integer from 0 (Theme) to 255 required."
                                            .into()),
                                    },
                                    None => Ok((
                                        format!(
                                            "WINDOWSAREACOLOR = {}",
                                            self.model_space.selection_window_color
                                        ),
                                        false,
                                    )),
                                }
                            }
                            "CROSSINGAREACOLOR" => {
                                match &value {
                                    Some(v) => match v.parse::<u8>() {
                                        Ok(color) => {
                                            self.model_space.selection_crossing_color = color;
                                            self.sync_model_space_theme(false);
                                            Ok((format!("CROSSINGAREACOLOR = {color}"), true))
                                        }
                                        _ => Err("SETVAR: integer from 0 (Theme) to 255 required."
                                            .into()),
                                    },
                                    None => Ok((
                                        format!(
                                            "CROSSINGAREACOLOR = {}",
                                            self.model_space.selection_crossing_color
                                        ),
                                        false,
                                    )),
                                }
                            }
                            "SELECTIONPREVIEW" => match &value {
                                Some(v) => match v.parse::<u8>() {
                                    Ok(mode @ 0..=3) => {
                                        self.model_space.selection_preview = mode;
                                        Ok((format!("SELECTIONPREVIEW = {mode}"), true))
                                    }
                                    _ => Err("SETVAR: integer from 0 to 3 required.".into()),
                                },
                                None => Ok((
                                    format!(
                                        "SELECTIONPREVIEW = {}",
                                        self.model_space.selection_preview
                                    ),
                                    false,
                                )),
                            },
                            "GRIPSIZE" => match &value {
                                Some(v) => match v.parse::<u8>() {
                                    Ok(size @ 1..=25) => {
                                        self.model_space.grip_size = size;
                                        self.sync_model_space_theme(false);
                                        Ok((format!("GRIPSIZE = {size}"), true))
                                    }
                                    _ => Err("SETVAR: integer from 1 to 25 required.".into()),
                                },
                                None => Ok((
                                    format!("GRIPSIZE = {}", self.model_space.grip_size),
                                    false,
                                )),
                            },
                            "GRIPOBJLIMIT" => match &value {
                                Some(v) => match v.parse::<i32>() {
                                    Ok(limit @ 0..=32767) => {
                                        self.grip_object_limit = limit;
                                        Ok((format!("GRIPOBJLIMIT = {limit}"), true))
                                    }
                                    _ => Err(
                                        "SETVAR: integer from 0 to 32767 required (0 = no limit)."
                                            .into(),
                                    ),
                                },
                                None => Ok((
                                    format!("GRIPOBJLIMIT = {}", self.grip_object_limit),
                                    false,
                                )),
                            },
                            "GRIPCOLOR" => {
                                match &value {
                                    Some(v) => match v.parse::<u8>() {
                                        Ok(color) => {
                                            self.model_space.grip_color = color;
                                            self.sync_model_space_theme(false);
                                            Ok((format!("GRIPCOLOR = {color}"), true))
                                        }
                                        _ => Err("SETVAR: integer from 0 (Theme) to 255 required."
                                            .into()),
                                    },
                                    None => Ok((
                                        format!("GRIPCOLOR = {}", self.model_space.grip_color),
                                        false,
                                    )),
                                }
                            }
                            "GRIPHOT" => match &value {
                                Some(v) => {
                                    match v.parse::<u8>() {
                                        Ok(color) => {
                                            self.model_space.grip_hot = color;
                                            self.sync_model_space_theme(false);
                                            Ok((format!("GRIPHOT = {color}"), true))
                                        }
                                        _ => Err("SETVAR: integer from 0 (Theme) to 255 required."
                                            .into()),
                                    }
                                }
                                None => {
                                    Ok((format!("GRIPHOT = {}", self.model_space.grip_hot), false))
                                }
                            },
                            "GRIPHOVER" => {
                                match &value {
                                    Some(v) => match v.parse::<u8>() {
                                        Ok(color) => {
                                            self.model_space.grip_hover = color;
                                            self.sync_model_space_theme(false);
                                            Ok((format!("GRIPHOVER = {color}"), true))
                                        }
                                        _ => Err("SETVAR: integer from 0 (Theme) to 255 required."
                                            .into()),
                                    },
                                    None => Ok((
                                        format!("GRIPHOVER = {}", self.model_space.grip_hover),
                                        false,
                                    )),
                                }
                            }
                            "SNAPANG" => match &value {
                                Some(v) => match v.parse::<f32>() {
                                    Ok(angle) if angle.is_finite() => {
                                        self.snap_angle_deg = angle.rem_euclid(360.0);
                                        Ok((format!("SNAPANG = {}", self.snap_angle_deg), true))
                                    }
                                    _ => Err("SETVAR: finite numeric value required.".into()),
                                },
                                None => Ok((format!("SNAPANG = {}", self.snap_angle_deg), false)),
                            },
                            // Global (not stored in the drawing): fill vs. hollow
                            // TrueType text. The active tab re-tessellates below.
                            "TEXTFILL" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        crate::scene::text::sdf_atlas::set_textfill(b);
                                        (format!("TEXTFILL = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((
                                    format!(
                                        "TEXTFILL = {}",
                                        crate::scene::text::sdf_atlas::textfill() as i32
                                    ),
                                    false,
                                )),
                            },
                            "CLIPROMPTLINES" => match &value {
                                Some(v) => match v.parse::<i32>() {
                                    Ok(n) if (0..=50).contains(&n) => {
                                        self.cliprompt_lines = n;
                                        self.command_line.set_cliprompt_lines(n as u8);
                                        Ok((format!("CLIPROMPTLINES = {n}"), true))
                                    }
                                    _ => Err("SETVAR: integer from 0 to 50 required.".into()),
                                },
                                None => Ok((
                                    format!("CLIPROMPTLINES = {}", self.cliprompt_lines),
                                    false,
                                )),
                            },
                            "COMMANDLINEOPACITY" => match &value {
                                Some(v) => match v.parse::<u8>() {
                                    Ok(n) if (20..=100).contains(&n) => {
                                        self.command_line.set_opacity_percent(n);
                                        Ok((format!("COMMANDLINEOPACITY = {n}"), true))
                                    }
                                    _ => Err("SETVAR: integer from 20 to 100 required.".into()),
                                },
                                None => Ok((
                                    format!(
                                        "COMMANDLINEOPACITY = {}",
                                        self.command_line.opacity_percent()
                                    ),
                                    false,
                                )),
                            },
                            "COMMANDLINEFADETIME" => match &value {
                                Some(v) => match v.parse::<i32>() {
                                    Ok(n) if (0..=60000).contains(&n) => {
                                        self.commandline_fade_ms = n;
                                        self.command_line.set_commandline_fade_ms(n as u32);
                                        Ok((format!("COMMANDLINEFADETIME = {n}"), true))
                                    }
                                    _ => Err("SETVAR: integer from 0 to 60000 required.".into()),
                                },
                                None => Ok((
                                    format!("COMMANDLINEFADETIME = {}", self.commandline_fade_ms),
                                    false,
                                )),
                            },
                            "ATTREQ" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.attribute_request = b;
                                        (format!("ATTREQ = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => {
                                    Ok((format!("ATTREQ = {}", h.attribute_request as i32), false))
                                }
                            },
                            "ATTDIA" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.attribute_dialog = b;
                                        (format!("ATTDIA = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => {
                                    Ok((format!("ATTDIA = {}", h.attribute_dialog as i32), false))
                                }
                            },
                            "DIMASSOC" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.dimension_associativity = x;
                                        (format!("DIMASSOC = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => {
                                    Ok((format!("DIMASSOC = {}", h.dimension_associativity), false))
                                }
                            },
                            "ANGBASE" => match &value {
                                Some(v) => v
                                    .parse::<f64>()
                                    .map(|x| {
                                        h.angle_base = x;
                                        (format!("ANGBASE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: numeric value required.".into()),
                                None => Ok((format!("ANGBASE = {}", h.angle_base), false)),
                            },
                            "ANGDIR" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.angle_direction = x;
                                        (format!("ANGDIR = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("ANGDIR = {}", h.angle_direction), false)),
                            },
                            "REGENMODE" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.regen_mode = b;
                                        (format!("REGENMODE = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((format!("REGENMODE = {}", h.regen_mode as i32), false)),
                            },
                            "BLIPMODE" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.blip_mode = b;
                                        (format!("BLIPMODE = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((format!("BLIPMODE = {}", h.blip_mode as i32), false)),
                            },
                            "SPLFRAME" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.spline_frame = b;
                                        (format!("SPLFRAME = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => {
                                    Ok((format!("SPLFRAME = {}", h.spline_frame as i32), false))
                                }
                            },
                            "CAMERADISPLAY" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.camera_display = b;
                                        (format!("CAMERADISPLAY = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((
                                    format!("CAMERADISPLAY = {}", h.camera_display as i32),
                                    false,
                                )),
                            },
                            "DELOBJ" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .ok()
                                    .filter(|value| (0..=3).contains(value))
                                    .map(|value| (format!("DELOBJ = {value}"), true))
                                    .ok_or_else(|| "SETVAR: integer from 0 to 3 required.".into()),
                                None => Ok((format!("DELOBJ = {current_delete_objects}"), false)),
                            },
                            "PLINEGEN" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.polyline_linetype_generation = b;
                                        (format!("PLINEGEN = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((
                                    format!("PLINEGEN = {}", h.polyline_linetype_generation as i32),
                                    false,
                                )),
                            },
                            "PSLTSCALE" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.paper_space_linetype_scaling = b;
                                        (format!("PSLTSCALE = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((
                                    format!(
                                        "PSLTSCALE = {}",
                                        h.paper_space_linetype_scaling as i32
                                    ),
                                    false,
                                )),
                            },
                            "DISPSILH" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.display_silhouette = b;
                                        (format!("DISPSILH = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((
                                    format!("DISPSILH = {}", h.display_silhouette as i32),
                                    false,
                                )),
                            },
                            "WORLDVIEW" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.world_view = b;
                                        (format!("WORLDVIEW = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((format!("WORLDVIEW = {}", h.world_view as i32), false)),
                            },
                            "LIMCHECK" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.limit_check = b;
                                        (format!("LIMCHECK = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((format!("LIMCHECK = {}", h.limit_check as i32), false)),
                            },
                            "DRAGMODE" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.drag_mode = x;
                                        (format!("DRAGMODE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("DRAGMODE = {}", h.drag_mode), false)),
                            },
                            "LUNITS" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.linear_unit_format = x;
                                        (format!("LUNITS = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("LUNITS = {}", h.linear_unit_format), false)),
                            },
                            "LUPREC" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        let x = x.clamp(0, 8);
                                        h.linear_unit_precision = x;
                                        (format!("LUPREC = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => {
                                    Ok((format!("LUPREC = {}", h.linear_unit_precision), false))
                                }
                            },
                            "AUNITS" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.angular_unit_format = x;
                                        (format!("AUNITS = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("AUNITS = {}", h.angular_unit_format), false)),
                            },
                            "AUPREC" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        let x = x.clamp(0, 8);
                                        h.angular_unit_precision = x;
                                        (format!("AUPREC = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => {
                                    Ok((format!("AUPREC = {}", h.angular_unit_precision), false))
                                }
                            },
                            "THICKNESS" => match &value {
                                Some(v) => v
                                    .parse::<f64>()
                                    .map(|x| {
                                        h.thickness = x;
                                        (format!("THICKNESS = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: numeric value required.".into()),
                                None => Ok((format!("THICKNESS = {}", h.thickness), false)),
                            },
                            "ELEVATION" => match &value {
                                Some(v) => v
                                    .parse::<f64>()
                                    .map(|x| {
                                        h.elevation = x;
                                        (format!("ELEVATION = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: numeric value required.".into()),
                                None => Ok((format!("ELEVATION = {}", h.elevation), false)),
                            },
                            "INSUNITS" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.insertion_units = x;
                                        (format!("INSUNITS = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("INSUNITS = {}", h.insertion_units), false)),
                            },
                            "SPLINETYPE" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.spline_type = x;
                                        (format!("SPLINETYPE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("SPLINETYPE = {}", h.spline_type), false)),
                            },
                            "ISOLINES" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        // Stored values stay inside the range
                                        // the Options slider can ask for; the
                                        // consumption sites clamp again for
                                        // file-sourced headers.
                                        let x = x.clamp(
                                            0,
                                            crate::entities::solid3d::MAX_HEADER_ISOLINES,
                                        );
                                        h.isolines = x;
                                        (format!("ISOLINES = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("ISOLINES = {}", h.isolines), false)),
                            },
                            "DIMASO" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.associate_dimensions = b;
                                        (format!("DIMASO = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((
                                    format!("DIMASO = {}", h.associate_dimensions as i32),
                                    false,
                                )),
                            },
                            "DIMSHO" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.update_dimensions_while_dragging = b;
                                        (format!("DIMSHO = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((
                                    format!(
                                        "DIMSHO = {}",
                                        h.update_dimensions_while_dragging as i32
                                    ),
                                    false,
                                )),
                            },
                            "QTEXTMODE" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.quick_text_mode = b;
                                        (format!("QTEXTMODE = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => {
                                    Ok((format!("QTEXTMODE = {}", h.quick_text_mode as i32), false))
                                }
                            },
                            "PLIMCHECK" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.paper_space_limit_check = b;
                                        (format!("PLIMCHECK = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((
                                    format!("PLIMCHECK = {}", h.paper_space_limit_check as i32),
                                    false,
                                )),
                            },
                            "VISRETAIN" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.retain_xref_visibility = b;
                                        (format!("VISRETAIN = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((
                                    format!("VISRETAIN = {}", h.retain_xref_visibility as i32),
                                    false,
                                )),
                            },
                            "USRTIMER" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.user_timer = b;
                                        (format!("USRTIMER = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => Ok((format!("USRTIMER = {}", h.user_timer as i32), false)),
                            },
                            "ATTMODE" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.attribute_visibility = x;
                                        (format!("ATTMODE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => {
                                    Ok((format!("ATTMODE = {}", h.attribute_visibility), false))
                                }
                            },
                            "COORDS" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.coords_mode = x;
                                        (format!("COORDS = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("COORDS = {}", h.coords_mode), false)),
                            },
                            "OSMODE" => match &value {
                                Some(v) => v
                                    .parse::<i32>()
                                    .map(|x| {
                                        h.object_snap_mode = x;
                                        (format!("OSMODE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("OSMODE = {}", h.object_snap_mode), false)),
                            },
                            "PICKSTYLE" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.pick_style = x;
                                        (format!("PICKSTYLE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("PICKSTYLE = {}", h.pick_style), false)),
                            },
                            "SPLINESEGS" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.spline_segments = x;
                                        (format!("SPLINESEGS = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("SPLINESEGS = {}", h.spline_segments), false)),
                            },
                            "SURFU" => match &value {
                                Some(v) => match v.parse::<i16>() {
                                    Ok(x @ 0..=200) => {
                                        h.surface_u_density = x;
                                        Ok((format!("SURFU = {x}"), true))
                                    }
                                    Ok(_) => Err("SETVAR: value must be between 0 and 200.".into()),
                                    Err(_) => Err("SETVAR: integer value required.".into()),
                                },
                                None => Ok((format!("SURFU = {}", h.surface_u_density), false)),
                            },
                            "SURFV" => match &value {
                                Some(v) => match v.parse::<i16>() {
                                    Ok(x @ 0..=200) => {
                                        h.surface_v_density = x;
                                        Ok((format!("SURFV = {x}"), true))
                                    }
                                    Ok(_) => Err("SETVAR: value must be between 0 and 200.".into()),
                                    Err(_) => Err("SETVAR: integer value required.".into()),
                                },
                                None => Ok((format!("SURFV = {}", h.surface_v_density), false)),
                            },
                            "SURFTYPE" => match &value {
                                Some(v) => match v.parse::<i16>() {
                                    Ok(x @ (5 | 6 | 8)) => {
                                        h.surface_type = x;
                                        Ok((format!("SURFTYPE = {x}"), true))
                                    }
                                    Ok(_) => Err("SETVAR: value must be 5, 6, or 8.".into()),
                                    Err(_) => Err("SETVAR: integer value required.".into()),
                                },
                                None => Ok((format!("SURFTYPE = {}", h.surface_type), false)),
                            },
                            "SHADEDGE" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.shade_edge = x;
                                        (format!("SHADEDGE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("SHADEDGE = {}", h.shade_edge), false)),
                            },
                            "MAXACTVP" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.max_active_viewports = x;
                                        (format!("MAXACTVP = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => {
                                    Ok((format!("MAXACTVP = {}", h.max_active_viewports), false))
                                }
                            },
                            "CMLJUST" => match &value {
                                Some(v) => match v.parse::<i16>() {
                                    Ok(x @ 0..=2) => {
                                        h.multiline_justification = x;
                                        Ok((format!("CMLJUST = {x}"), true))
                                    }
                                    _ => Err("SETVAR: integer value from 0 to 2 required.".into()),
                                },
                                None => {
                                    Ok((format!("CMLJUST = {}", h.multiline_justification), false))
                                }
                            },
                            "CMLSCALE" => match &value {
                                Some(v) => match v.parse::<f64>() {
                                    Ok(x) if x.is_finite() => {
                                        let changed = h.multiline_scale != x;
                                        h.multiline_scale = x;
                                        Ok((format!("CMLSCALE = {x}"), changed))
                                    }
                                    _ => Err("SETVAR: finite numeric value required.".into()),
                                },
                                None => Ok((format!("CMLSCALE = {}", h.multiline_scale), false)),
                            },
                            "CMLSTYLE" => match &value {
                                Some(_) => Err(
                                    "SETVAR: CMLSTYLE is read-only here — use the MLSTYLE command."
                                        .into(),
                                ),
                                None => Ok((format!("CMLSTYLE = {}", h.multiline_style), false)),
                            },
                            "TEXTQLTY" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.text_quality = x;
                                        (format!("TEXTQLTY = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("TEXTQLTY = {}", h.text_quality), false)),
                            },
                            "SORTENTS" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.sort_entities = x;
                                        (format!("SORTENTS = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("SORTENTS = {}", h.sort_entities), false)),
                            },
                            "HALOGAP" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.halo_gap = x;
                                        (format!("HALOGAP = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("HALOGAP = {}", h.halo_gap), false)),
                            },
                            "TRACEWID" => match &value {
                                Some(v) => v
                                    .parse::<f64>()
                                    .map(|x| {
                                        h.trace_width = x;
                                        (format!("TRACEWID = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: numeric value required.".into()),
                                None => Ok((format!("TRACEWID = {}", h.trace_width), false)),
                            },
                            "SKETCHINC" => match &value {
                                Some(v) => match v.parse::<f64>() {
                                    Ok(x) if x.is_finite() && x > 0.0 => {
                                        let changed = h.sketch_increment != x;
                                        h.sketch_increment = x;
                                        Ok((format!("SKETCHINC = {x}"), changed))
                                    }
                                    _ => Err("SETVAR: positive numeric value required.".into()),
                                },
                                None => Ok((format!("SKETCHINC = {}", h.sketch_increment), false)),
                            },
                            "SKPOLY" => match &value {
                                Some(v) => match v.parse::<i16>() {
                                    Ok(x @ 0..=2) => {
                                        let changed = h.sketch_type != x;
                                        h.sketch_type = x;
                                        Ok((format!("SKPOLY = {x}"), changed))
                                    }
                                    _ => Err("SETVAR: integer value from 0 to 2 required.".into()),
                                },
                                None => Ok((format!("SKPOLY = {}", h.sketch_type), false)),
                            },
                            "SKTOLERANCE" => match &value {
                                Some(v) => match v.parse::<f64>() {
                                    Ok(x) if x.is_finite() && (0.0..=1.0).contains(&x) => {
                                        let changed = h.sketch_tolerance != x;
                                        h.sketch_tolerance = x;
                                        Ok((format!("SKTOLERANCE = {x}"), changed))
                                    }
                                    _ => Err("SETVAR: numeric value from 0 to 1 required.".into()),
                                },
                                None => {
                                    Ok((format!("SKTOLERANCE = {}", h.sketch_tolerance), false))
                                }
                            },
                            "CLAYER" => match &value {
                                Some(_) => Err(
                                    "SETVAR: CLAYER is read-only here — use the CLAYER command."
                                        .into(),
                                ),
                                None => Ok((format!("CLAYER = {}", h.current_layer_name), false)),
                            },
                            "CELTYPE" => match &value {
                                Some(_) => {
                                    Err("SETVAR: CELTYPE is read-only here — use LINETYPE SET."
                                        .into())
                                }
                                None => {
                                    Ok((format!("CELTYPE = {}", h.current_linetype_name), false))
                                }
                            },
                            "TEXTSTYLE" => match &value {
                                Some(_) => Err(
                                    "SETVAR: TEXTSTYLE is read-only here — use the STYLE command."
                                        .into(),
                                ),
                                None => Ok((
                                    format!("TEXTSTYLE = {}", h.current_text_style_name),
                                    false,
                                )),
                            },
                            _ => {
                                Err(crate::tf!("SETVAR: unknown variable \"{name}\".").into_owned())
                            }
                        }
                    };
                    match outcome {
                        Ok((msg, changed)) => {
                            if let Some(value) = requested_delete_objects {
                                self.delete_objects = value;
                            }
                            if changed {
                                if name == "INSUNITS" {
                                    self.tabs[i].scene.sync_annotation_scale_from_header();
                                    self.tabs[i].scene.invalidate_annotation_dependencies();
                                }
                                if matches!(
                                    name.as_str(),
                                    "ZOOMWHEEL"
                                        | "ZOOMFACTOR"
                                        | "SHORTCUTMENU"
                                        | "SHORTCUTMENUDURATION"
                                        | "CURSORSIZE"
                                        | "PICKBOX"
                                        | "APERTURE"
                                        | "HPASSOC"
                                        | "HPGAPTOL"
                                        | "COMMANDLINEOPACITY"
                                        | "MENUBAR"
                                        | "CURSORTYPE"
                                        | "SNAPANG"
                                        | "CLIPROMPTLINES"
                                        | "COMMANDLINEFADETIME"
                                        | "COLORTHEME"
                                        | "SELECTIONAREA"
                                        | "SELECTIONAREAOPACITY"
                                        | "SELECTIONEFFECT"
                                        | "SELECTIONEFFECTCOLOR"
                                        | "VSEDGECOLOR"
                                        | "WINDOWSAREACOLOR"
                                        | "WINDOWAREACOLOR"
                                        | "CROSSINGAREACOLOR"
                                        | "SELECTIONPREVIEW"
                                        | "GRIPSIZE"
                                        | "GRIPCOLOR"
                                        | "GRIPHOT"
                                        | "GRIPHOVER"
                                        | "GRIPOBJLIMIT"
                                        | "DELOBJ"
                                        // App-level, not part of the drawing:
                                        // it lives in a process global and is
                                        // snapshotted into the settings file.
                                        // Without this it marked the drawing
                                        // modified instead of persisting.
                                        | "TEXTFILL"
                                        | "DYNMODE"
                                ) {
                                    self.persist_settings_if_changed();
                                } else {
                                    self.tabs[i].dirty = true;
                                }
                                if name == "FILLMODE" || name == "CAMERADISPLAY" {
                                    self.tabs[i].scene.bump_geometry();
                                }
                                if matches!(name.as_str(), "PSLTSCALE" | "PLIMCHECK") {
                                    self.tabs[i].scene.persist_current_layout_state();
                                }
                                self.command_line.push_output(&msg);
                            } else {
                                // Queried with no value (`changed == false`):
                                // prompt for a new value on the next line instead
                                // of only echoing the current one. Enter keeps it.
                                let current = msg.split('=').nth(1).map(str::trim).unwrap_or("");
                                self.command_line.push_output(
                                    crate::tf!("Enter new value for {name} <{current}>:").as_ref(),
                                );
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        Err(e) => self.command_line.push_error(&e),
                    }
                    // TEXTFILL reset the glyph atlas; re-tessellate so text picks
                    // up the re-baked filled / hollow tiles.
                    if name == "TEXTFILL" {
                        self.invalidate_text_everywhere();
                    }
                    // LTSCALE scales the dash pattern baked into every wire, and
                    // PDMODE / PDSIZE decide the point glyph built at tessellation
                    // time. Their own commands invalidate for exactly that reason;
                    // reaching the same variable through SETVAR has to do it too,
                    // or the drawing keeps rendering the old value until some
                    // unrelated edit happens to rebuild it.
                    if name == "LTSCALE" {
                        self.tabs[i].scene.bump_geometry();
                    }
                    if name == "PDMODE" || name == "PDSIZE" {
                        self.tabs[i].scene.invalidate_point_dependencies();
                    }
                    if matches!(name.as_str(), "LUNITS" | "LUPREC" | "AUNITS" | "AUPREC") {
                        crate::entities::common::set_unit_context(
                            crate::entities::common::UnitContext::from_header(
                                &self.tabs[i].scene.document.header,
                            ),
                        );
                    }
                    // ORTHOMODE / OSMODE set the header directly; mirror them into
                    // the live Ortho / running OSNAP so the constraint + status
                    // bar follow and the save-time stamp doesn't revert them.
                    if name == "ORTHOMODE" {
                        self.ortho_mode = self.tabs[i].scene.document.header.ortho_mode;
                        if self.ortho_mode {
                            self.polar_mode = false;
                        }
                    }
                    if name == "OSMODE" {
                        let (modes, en) = crate::app::settings::snaps_from_osmode(
                            self.tabs[i].scene.document.header.object_snap_mode,
                        );
                        self.snapper.enabled = modes.into_iter().collect();
                        self.snapper.snap_enabled = en;
                        // The OSNAP set is app-level (no modern-DWG file slot).
                        self.persist_settings_if_changed();
                    }
                }
            }

            // ── FINDNONPURGEABLE — list named objects still in use ───────
            // Reports the layers, linetypes, text styles and blocks that are
            // referenced by objects, and therefore cannot be purged — so it is
            // clear why PURGE leaves them behind. Read-only.
            "FINDNONPURGEABLE" => {
                use rustc_hash::FxHashSet;
                let mut layers: FxHashSet<String> = FxHashSet::default();
                let mut linetypes: FxHashSet<String> = FxHashSet::default();
                let mut styles: FxHashSet<String> = FxHashSet::default();
                let mut blocks: FxHashSet<String> = FxHashSet::default();
                {
                    let doc = &self.tabs[i].scene.document;
                    for e in doc.entities() {
                        let l = &e.common().layer;
                        if !l.is_empty() {
                            layers.insert(l.clone());
                        }
                        let lt = &e.common().linetype;
                        if !lt.is_empty() && lt != "ByLayer" && lt != "ByBlock" {
                            linetypes.insert(lt.clone());
                        }
                        match e {
                            codec::EntityType::Text(t) if !t.style.is_empty() => {
                                styles.insert(t.style.clone());
                            }
                            codec::EntityType::MText(t) if !t.style.is_empty() => {
                                styles.insert(t.style.clone());
                            }
                            codec::EntityType::Insert(ins) => {
                                blocks.insert(ins.block_name.clone());
                            }
                            _ => {}
                        }
                    }
                }
                let fmt = |set: FxHashSet<String>| -> String {
                    if set.is_empty() {
                        "(none)".to_string()
                    } else {
                        let mut v: Vec<_> = set.into_iter().collect();
                        v.sort();
                        v.join(", ")
                    }
                };
                self.command_line.push_output(
                    crate::t!("FINDNONPURGEABLE: named objects in use (not purgeable):").as_ref(),
                );
                self.command_line
                    .push_output(crate::tf!("  Layers: {}", fmt(layers)).as_ref());
                self.command_line
                    .push_output(crate::tf!("  Linetypes: {}", fmt(linetypes)).as_ref());
                self.command_line
                    .push_output(crate::tf!("  Text styles: {}", fmt(styles)).as_ref());
                self.command_line
                    .push_output(crate::tf!("  Blocks: {}", fmt(blocks)).as_ref());
            }

            // ── AUDIT — report drawing-database integrity issues ─────────
            // Read-only scan: flags entities on undefined layers and block
            // references to undefined block definitions. Reports only; it does
            // not auto-repair (so it can never make the drawing worse).
            // COMPARE — overlay what differs from another drawing. Bare picks
            // the drawing; with a path it compares at once.
            "COMPARE" => {
                return Some(Task::perform(
                    async {
                        crate::sys::file_dialog()
                            .set_title(crate::t!("Compare with drawing").as_ref())
                            .add_filter(crate::t!("Drawing").as_ref(), &["dwg", "dxf"])
                            .pick_file()
                            .await
                            .map(|handle| crate::sys::handle_path(&handle))
                    },
                    |path| match path {
                        Some(path) => Message::Command(format!("COMPARE {}", path.display())),
                        None => Message::Noop,
                    },
                ));
            }
            cmd if cmd.starts_with("COMPARE ") => {
                let path = std::path::PathBuf::from(cmd.trim_start_matches("COMPARE").trim());
                match crate::io::load_file(&path) {
                    Ok(other) => {
                        let comparison = crate::scene::compare::compare(&self.tabs[i].scene, other);
                        // The overlay shows at once, before the cursor moves.
                        self.tabs[i]
                            .scene
                            .set_preview_wires(comparison.overlay.clone());
                        let command =
                            crate::modules::manage::compare::CompareCommand::new(comparison);
                        self.command_line.push_info(&command.prompt());
                        self.tabs[i].active_cmd = Some(Box::new(command));
                    }
                    Err(error) => self
                        .command_line
                        .push_error(crate::tf!("COMPARE: {error}").as_ref()),
                }
            }
            // AUDIT runs the codec's audit; `AUDIT Yes` also applies its
            // repair, undoable as one step.
            "AUDIT" => {
                use crate::command::KeywordCommand;
                let c = KeywordCommand::new(
                    "AUDIT",
                    "AUDIT  Fix any errors detected? [Yes / No] <No>:",
                    vec![("Yes", "YES", None), ("No", "NO", None)],
                )
                .with_default("NO");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("AUDIT ") => {
                let fix = cmd[6..].trim().to_uppercase().starts_with('Y');
                let report = self.tabs[i].scene.document.audit();
                let total = {
                    let doc = &self.tabs[i].scene.document;
                    doc.entities().count() + doc.objects.len()
                };
                self.command_line
                    .push_output(crate::tf!("AUDIT: scanned {total} object(s).").as_ref());
                if report.is_clean() {
                    self.command_line
                        .push_output(crate::t!("AUDIT: no issues found.").as_ref());
                    return Some(Task::none());
                }
                self.command_line.push_error(
                    crate::tf!(
                        "AUDIT: {} error(s), {} warning(s) found.",
                        report.error_count(),
                        report.warning_count()
                    )
                    .as_ref(),
                );
                // ponytail: first 50 issues only; the counts above cover the rest.
                for issue in report.issues.iter().take(50) {
                    self.command_line
                        .push_info(&format!("  {:?}: {}", issue.code, issue.message));
                }
                if fix {
                    self.push_undo_snapshot(i, "AUDIT");
                    let repair = self.tabs[i].scene.document.repair();
                    self.command_line.push_output(
                        crate::tf!(
                            "AUDIT: {} repair(s) applied, {} issue(s) left unresolved.",
                            repair.actions.len(),
                            repair.unresolved.len()
                        )
                        .as_ref(),
                    );
                    if !repair.actions.is_empty() {
                        self.tabs[i].dirty = true;
                        self.tabs[i].scene.rebuild_derived_caches();
                        self.refresh_layer_panel();
                    }
                }
            }

            // ── RENAME table entries ──────────────────────────────────────
            // Bare RENAME prompts step by step (type → old → new); the argument
            // form below both handles a fully-typed line and is the target the
            // stepped front-end dispatches to.
            "RENAME" => self.open_rename_dialog(),
            "-RENAME" => {
                use crate::command::RenameCommand;
                let c = RenameCommand::new();
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("RENAME ") => {
                // Usage: RENAME <type> <old_name> <new_name>
                // Types: LAYER BLOCK STYLE DIMSTYLE LINETYPE UCS VIEW
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                let type_str = parts.get(1).map(|s| s.to_uppercase()).unwrap_or_default();
                let old_name = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();
                let new_name = parts.get(3).map(|s| s.trim()).unwrap_or("").to_string();

                if type_str.is_empty() || old_name.is_empty() || new_name.is_empty() {
                    self.command_line.push_info(
                        crate::t!("Usage: RENAME <type> <old> <new>  (types: LAYER BLOCK STYLE DIMSTYLE LINETYPE UCS VIEW)").as_ref()
                    );
                } else {
                    self.rename_named_object(i, &type_str, &old_name, &new_name);
                }
            }

            // ── System variable getters/setters ──────────────────────────────────
            // CLAYER [name]    — get or set current layer
            // TEXTSTYLE [name] — already handled above under STYLE SET
            // DIMSTYLE [name]  — get or set active dim style
            // LTSCALE [val]    — global linetype scale
            "CLAYER" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new("CLAYER", "CLAYER  new current layer name:");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("CLAYER ") => {
                let name_arg = cmd.trim_start_matches("CLAYER").trim();
                if name_arg.is_empty() {
                    let cur = &self.tabs[i].scene.document.header.current_layer_name;
                    self.command_line
                        .push_output(crate::tf!("CLAYER = \"{cur}\"").as_ref());
                } else {
                    if self.set_current_layer_name(i, name_arg).is_ok() {
                        self.command_line
                            .push_output(crate::tf!("CLAYER set to \"{name_arg}\"").as_ref());
                    } else {
                        self.command_line.push_error(
                            crate::tf!("CLAYER: layer '{}' not found.", name_arg).as_ref(),
                        );
                    }
                }
            }
            "CDIMSTY" | "DIMCURRENT" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "CDIMSTY",
                    "CDIMSTY  new current dimension style name:",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("CDIMSTY ") || cmd.starts_with("DIMCURRENT ") => {
                let name_arg = cmd.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
                if name_arg.is_empty() {
                    let cur = &self.tabs[i].scene.document.header.current_dimstyle_name;
                    self.command_line
                        .push_output(crate::tf!("CDIMSTY = \"{cur}\"").as_ref());
                } else {
                    if self.tabs[i].scene.document.dim_styles.contains(&name_arg) {
                        self.tabs[i].scene.document.header.current_dimstyle_name = name_arg.clone();
                        crate::scene::creation_style::sync_header_dim_vars(
                            &mut self.tabs[i].scene.document,
                        );
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(
                            crate::tf!("Active dim style set to \"{name_arg}\"").as_ref(),
                        );
                    } else {
                        self.command_line.push_error(
                            crate::tf!("CDIMSTY: dim style '{}' not found.", name_arg).as_ref(),
                        );
                    }
                }
            }
            "LTSCALE" => {
                use crate::command::ValuePromptCommand;

                let current = self.tabs[i].scene.document.header.linetype_scale;

                self.command_line
                    .push_output(crate::tf!("LTSCALE = {current:.4}").as_ref());

                let c = ValuePromptCommand::new("LTSCALE", "LTSCALE  new global line-type scale:");

                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("LTSCALE ") => {
                let val_str = cmd.trim_start_matches("LTSCALE").trim();
                if val_str.is_empty() {
                    let v = self.tabs[i].scene.document.header.linetype_scale;
                    self.command_line
                        .push_output(crate::tf!("LTSCALE = {v:.4}").as_ref());
                } else if let Ok(v) = val_str.parse::<f64>() {
                    if v > 0.0 {
                        self.push_undo_snapshot(i, "LTSCALE");
                        self.tabs[i].scene.document.header.linetype_scale = v;
                        // LTSCALE affects the cached linetype geometry of the whole drawing.
                        self.tabs[i].scene.bump_geometry();
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::tf!("LTSCALE set to {v:.4}").as_ref());
                    } else {
                        self.command_line
                            .push_error(crate::t!("LTSCALE: value must be positive.").as_ref());
                    }
                } else {
                    self.command_line
                        .push_error(crate::t!("Usage: LTSCALE [value]").as_ref());
                }
            }
            "PDMODE" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "PDMODE",
                    "PDMODE  new value [0=dot 1=none 2=+ 3=x 4=tick; +32 circle +64 square]:",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("PDMODE ") => {
                let val_str = cmd.trim_start_matches("PDMODE").trim();
                if val_str.is_empty() {
                    let v = self.tabs[i].scene.document.header.point_display_mode;
                    self.command_line
                        .push_output(crate::tf!("PDMODE = {v}").as_ref());
                } else if let Ok(v) = val_str.parse::<i16>() {
                    self.push_undo_snapshot(i, "PDMODE");
                    self.tabs[i].scene.document.header.point_display_mode = v;
                    // Point glyphs are built at tessellation time — rebuild them.
                    self.tabs[i].scene.invalidate_point_dependencies();
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("PDMODE set to {v}").as_ref());
                } else {
                    self.command_line.push_error(
                        crate::t!("Usage: PDMODE [value]  (0=dot 1=none 2=+ 3=x 4=tick; +32 circle, +64 square)").as_ref(),
                    );
                }
            }
            cmd if cmd.starts_with("TEXTEDITMODE ") => {
                let val_str = cmd.trim_start_matches("TEXTEDITMODE").trim().to_lowercase();
                if val_str.is_empty() {
                    let v = if self.texteditmode { 1 } else { 0 };
                    self.command_line
                        .push_output(crate::tf!("TEXTEDITMODE = {v}").as_ref());
                } else if let Some(v) =
                    crate::modules::annotate::textedit::parse_texteditmode(&val_str)
                {
                    self.texteditmode = v;
                    let n = if v { 1 } else { 0 };
                    self.command_line
                        .push_output(crate::tf!("TEXTEDITMODE set to {n}").as_ref());
                } else {
                    self.command_line
                        .push_error(crate::t!("Requires 0 OR 1 OR MULTIPLE OR SINGLE").as_ref());
                }
            }
            "ISAVEBAK" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "ISAVEBAK",
                    "ISAVEBAK  write a .bak on save?  [1 / 0]:",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("ISAVEBAK ") => {
                match cmd.trim_start_matches("ISAVEBAK").trim() {
                    "" => {
                        let v = if self.backup_on_save { 1 } else { 0 };
                        self.command_line
                            .push_output(crate::tf!("ISAVEBAK = {v}").as_ref());
                    }
                    "0" => {
                        self.backup_on_save = false;
                        self.persist_settings_if_changed();
                        self.command_line
                            .push_output(crate::t!("ISAVEBAK set to 0").as_ref());
                    }
                    "1" => {
                        self.backup_on_save = true;
                        self.persist_settings_if_changed();
                        self.command_line
                            .push_output(crate::t!("ISAVEBAK set to 1").as_ref());
                    }
                    _ => self
                        .command_line
                        .push_error(crate::t!("Requires 0 or 1").as_ref()),
                }
            }
            "FILEASSOC" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "FILEASSOC",
                    "FILEASSOC  register as a .dwg/.dxf handler?  [1 / 0]:",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("FILEASSOC ") => {
                match cmd.trim_start_matches("FILEASSOC").trim() {
                    "" => {
                        let v = if self.file_assoc_enabled { 1 } else { 0 };
                        self.command_line
                            .push_output(crate::tf!("FILEASSOC = {v}").as_ref());
                    }
                    "1" => {
                        self.file_assoc_enabled = true;
                        self.persist_settings_if_changed();
                        match crate::io::file_association::register_as_handler() {
                            Ok(()) => self.command_line.push_output(
                                crate::t!(
                                    "FILEASSOC set to 1 — registered as a .dwg/.dxf/.bak handler"
                                )
                                .as_ref(),
                            ),
                            Err(e) => self.command_line.push_error(
                                crate::tf!("FILEASSOC: registration failed: {e}").as_ref(),
                            ),
                        }
                    }
                    "0" => {
                        self.file_assoc_enabled = false;
                        self.persist_settings_if_changed();
                        match crate::io::file_association::unregister_handler() {
                            Ok(()) => self.command_line.push_output(
                                crate::t!("FILEASSOC set to 0 — unregistered as a file handler")
                                    .as_ref(),
                            ),
                            Err(e) => self.command_line.push_error(
                                crate::tf!("FILEASSOC: unregister failed: {e}").as_ref(),
                            ),
                        }
                    }
                    _ => self
                        .command_line
                        .push_error(crate::t!("Requires 0 or 1").as_ref()),
                }
            }
            cmd if cmd == "SCRIPTCOMMANDS" || cmd.starts_with("SCRIPTCOMMANDS ") => {
                // A user setting, deliberately outside the script's reach: it is refused by
                // the script command runner, so only the command line can change it.
                let arg = cmd.trim_start_matches("SCRIPTCOMMANDS").trim();
                match arg {
                    "" => {}
                    "1" | "ON" | "YES" => self.script_commands = true,
                    "0" | "OFF" | "NO" => self.script_commands = false,
                    _ => {
                        self.command_line.push_error(
                            crate::t!("SCRIPTCOMMANDS takes 1 (scripts may run commands) or 0 (they may not)")
                                .as_ref(),
                        );
                        return Some(Task::none());
                    }
                }
                if !arg.is_empty() {
                    self.persist_settings_if_changed();
                }
                let state = if self.script_commands { 1 } else { 0 };
                self.command_line
                    .push_output(crate::tf!("SCRIPTCOMMANDS = {state}").as_ref());
            }
            "SAVETIME" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "SAVETIME",
                    "SAVETIME  autosave interval in minutes (0 = off):",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("SAVETIME ") => {
                let arg = cmd.trim_start_matches("SAVETIME").trim();
                if arg.is_empty() {
                    self.command_line
                        .push_output(crate::tf!("SAVETIME = {}", self.savetime_min).as_ref());
                    return Some(Task::none());
                }
                match arg.parse::<i32>() {
                    Ok(v) if v >= 0 => {
                        self.savetime_min = v;
                        self.persist_settings_if_changed();
                        let msg = if v == 0 {
                            "SAVETIME set to 0 (autosave off)".to_string()
                        } else {
                            crate::tf!("SAVETIME set to {v} minute(s)").into_owned()
                        };
                        self.command_line.push_output(&msg);
                    }
                    _ => self.command_line.push_error(
                        crate::t!("Requires a non-negative number of minutes (0 = off)").as_ref(),
                    ),
                }
            }
            "PDSIZE" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "PDSIZE",
                    "PDSIZE  new point size (0 = 5% of viewport, >0 = absolute, <0 = viewport percentage):",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("PDSIZE ") => {
                let val_str = cmd.trim_start_matches("PDSIZE").trim();
                if val_str.is_empty() {
                    let v = self.tabs[i].scene.document.header.point_display_size;
                    self.command_line
                        .push_output(crate::tf!("PDSIZE = {v:.4}").as_ref());
                } else if let Ok(v) = val_str.parse::<f64>() {
                    self.push_undo_snapshot(i, "PDSIZE");
                    self.tabs[i].scene.document.header.point_display_size = v;
                    self.tabs[i].scene.invalidate_point_dependencies();
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("PDSIZE set to {v:.4}").as_ref());
                } else {
                    self.command_line.push_error(
                        crate::t!("Usage: PDSIZE [value]  (>0 absolute size, <0 percent of viewport, 0 default)").as_ref(),
                    );
                }
            }
            cmd if cmd == "DDPTYPE" || cmd == "PTYPE" => {
                self.begin_point_style_dialog();
            }
            "LWDISPLAY" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "LWDISPLAY",
                    "LWDISPLAY  show lineweights?  [ON / OFF]:",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("LWDISPLAY ") => {
                let val_str = cmd.trim_start_matches("LWDISPLAY").trim();
                let parsed: Result<Option<bool>, ()> = match val_str.to_ascii_uppercase().as_str() {
                    "" => Ok(None),
                    "ON" | "1" | "TRUE" => Ok(Some(true)),
                    "OFF" | "0" | "FALSE" => Ok(Some(false)),
                    _ => Err(()),
                };
                match parsed {
                    Err(_) => self
                        .command_line
                        .push_error(crate::t!("Usage: LWDISPLAY [ON|OFF]").as_ref()),
                    Ok(Some(v)) => {
                        self.push_undo_snapshot(i, "LWDISPLAY");
                        self.tabs[i].scene.document.header.lineweight_display = v;
                        // No retessellate — the wire shader honours the flag via uniforms.
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(
                            crate::tf!("LWDISPLAY {}", if v { "ON" } else { "OFF" }).as_ref(),
                        );
                    }
                    Ok(None) => {
                        let v = self.tabs[i].scene.document.header.lineweight_display;
                        self.command_line.push_output(
                            crate::tf!("LWDISPLAY = {}", if v { "ON" } else { "OFF" }).as_ref(),
                        );
                    }
                }
            }
            "CELTSCALE" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "CELTSCALE",
                    "CELTSCALE  new current-object line-type scale:",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("CELTSCALE ") => {
                let val_str = cmd.trim_start_matches("CELTSCALE").trim();
                if val_str.is_empty() {
                    let v = self.tabs[i]
                        .scene
                        .document
                        .header
                        .current_entity_linetype_scale;
                    self.command_line
                        .push_output(crate::tf!("CELTSCALE = {v:.4}").as_ref());
                } else if let Ok(v) = val_str.parse::<f64>() {
                    if v > 0.0 {
                        self.tabs[i]
                            .scene
                            .document
                            .header
                            .current_entity_linetype_scale = v;
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::tf!("CELTSCALE set to {v:.4}").as_ref());
                    } else {
                        self.command_line
                            .push_error(crate::t!("CELTSCALE: value must be positive.").as_ref());
                    }
                } else {
                    self.command_line
                        .push_error(crate::t!("Usage: CELTSCALE [value]").as_ref());
                }
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }
}

impl OpenCADStudio {
    /// Rename a named object of `type_str` (LAYER BLOCK STYLE DIMSTYLE
    /// LINETYPE UCS VIEW) as one undo step, reporting on the command line.
    pub(crate) fn rename_named_object(
        &mut self,
        i: usize,
        type_str: &str,
        old_name: &str,
        new_name: &str,
    ) -> bool {
        let (old_name, new_name) = (old_name.to_string(), new_name.to_string());
        // Snapshot BEFORE mutating (undo must restore the old
        // name); dropped again if nothing was renamed.
        let block = type_str == "BLOCK";
        let label = if block {
            format!("Rename block {old_name} to {new_name}")
        } else {
            "RENAME".to_string()
        };
        self.push_undo_snapshot(i, label);
        let known = matches!(
            type_str,
            "LAYER"
                | "BLOCK"
                | "STYLE"
                | "TEXTSTYLE"
                | "DIMSTYLE"
                | "LINETYPE"
                | "LT"
                | "UCS"
                | "VIEW"
        );
        // The block path's refusal text comes from the shared
        // service, so the command line, the palette, the plugin
        // API and the control op all word it the same way.
        let mut block_error: Option<String> = None;
        let ok = if block {
            let open_edits: Vec<codec::Handle> = self.tabs[i]
                .block_edits
                .iter()
                .map(|session| session.br_handle)
                .collect();
            // Re-keys the record, syncs the Block marker and every
            // INSERT reference; refuses anonymous/xref sources,
            // protected records and invalid/taken names.
            match crate::scene::block_service::rename(
                &mut self.tabs[i].scene,
                &old_name,
                &new_name,
                &open_edits,
            ) {
                Ok(_) => true,
                Err(error) => {
                    block_error = Some(error.to_string());
                    false
                }
            }
        } else if type_str == "LAYER" {
            self.tabs[i].rename_layer(&old_name, &new_name)
        } else if known {
            rename_symbol(
                &mut self.tabs[i].scene.document,
                &type_str,
                &old_name,
                &new_name,
            )
        } else {
            false
        };
        if ok {
            if type_str != "LAYER" {
                self.tabs[i].scene.bump_geometry_no_blocks();
            }
            if type_str == "UCS" {
                self.tabs[i].scene.bump_ucs_epoch();
                if let Some(active) = self.tabs[i].active_ucs.as_mut() {
                    if active.name.eq_ignore_ascii_case(&old_name) {
                        active.name = new_name.clone();
                        self.tabs[i].persist_active_ucs();
                    }
                }
            }
            self.tabs[i].dirty = true;
            if type_str == "LAYER" {
                self.refresh_layer_panel();
            }
            self.command_line.push_output(
                crate::tf!("RENAME: '{}' → '{}'.", old_name, new_name).as_ref(),
            );
        } else {
            self.discard_last_undo_entry(i);
            if let Some(message) = block_error {
                self.command_line.push_error(&message);
            } else if !known {
                self.command_line.push_error(crate::tf!("RENAME: unknown type '{}'. Use LAYER BLOCK STYLE DIMSTYLE LINETYPE UCS VIEW", type_str).as_ref());
            } else {
                self.command_line.push_error(crate::tf!(
                    "RENAME: could not rename '{}' in {} — not found or protected, or '{}' is invalid or taken.",
                    old_name, type_str, new_name
                ).as_ref());
            }
        }
        ok
    }
}

/// A VSEDGECOLOR value: an index, a colour name or R,G,B, with BLACK (or 0)
/// for the default black edges (`Some(None)`).
fn parse_edge_color(value: &str) -> Option<Option<[u8; 3]>> {
    let value = value.trim().trim_start_matches("RGB:");
    if value.eq_ignore_ascii_case("BLACK") || value == "0" {
        return Some(None);
    }
    let parts: Vec<Option<u8>> = value.split(',').map(|p| p.trim().parse().ok()).collect();
    if let [Some(r), Some(g), Some(b)] = parts[..] {
        return Some(Some([r, g, b]));
    }
    crate::ui::window::attman_dialog::parse_color(value)
        .and_then(crate::ui::color_select::math::color_rgb)
        .map(|(r, g, b)| Some([r, g, b]))
}

/// RENAME for the remaining name-keyed symbol tables.
fn rename_symbol(doc: &mut codec::Document, ty: &str, old: &str, new: &str) -> bool {
    use codec::{EntityType, Table, TableEntry};

    fn rekey<T: TableEntry>(table: &mut Table<T>, old: &str, new: &str) -> bool {
        if !crate::scene::valid_block_name(new) {
            return false;
        }
        table.rename(old, new.to_string()).is_ok()
    }

    match ty {
        "STYLE" | "TEXTSTYLE" => {
            if !rekey(&mut doc.text_styles, old, new) {
                return false;
            }
            for e in doc.entities_mut() {
                match e {
                    EntityType::Text(t) if t.style.eq_ignore_ascii_case(old) => {
                        t.style = new.to_string();
                    }
                    EntityType::MText(m) if m.style.eq_ignore_ascii_case(old) => {
                        m.style = new.to_string();
                    }
                    EntityType::AttributeDefinition(a)
                        if a.text_style.eq_ignore_ascii_case(old) =>
                    {
                        a.text_style = new.to_string();
                    }
                    EntityType::Insert(ins) => {
                        for a in &mut ins.attributes {
                            if a.text_style.eq_ignore_ascii_case(old) {
                                a.text_style = new.to_string();
                            }
                        }
                    }
                    _ => {}
                }
            }
            if doc.header.current_text_style_name.eq_ignore_ascii_case(old) {
                doc.header.current_text_style_name = new.to_string();
            }
            // Dimension styles name their text style; follow the rename (same
            // as rename_style_storage — see #42).
            for ds in doc.dim_styles.iter_mut() {
                if ds.dimtxsty.eq_ignore_ascii_case(old) {
                    ds.dimtxsty = new.to_string();
                }
            }
            true
        }
        "DIMSTYLE" => {
            if !rekey(&mut doc.dim_styles, old, new) {
                return false;
            }
            for e in doc.entities_mut() {
                match e {
                    EntityType::Dimension(d) if d.base().style_name.eq_ignore_ascii_case(old) => {
                        d.base_mut().style_name = new.to_string();
                    }
                    EntityType::Leader(l) if l.dimension_style.eq_ignore_ascii_case(old) => {
                        l.dimension_style = new.to_string();
                    }
                    EntityType::Tolerance(t)
                        if t.dimension_style_name.eq_ignore_ascii_case(old) =>
                    {
                        t.dimension_style_name = new.to_string();
                    }
                    _ => {}
                }
            }
            if doc.header.current_dimstyle_name.eq_ignore_ascii_case(old) {
                doc.header.current_dimstyle_name = new.to_string();
            }
            true
        }
        "LINETYPE" | "LT" => {
            // The three built-ins are fixed names every drawing relies on.
            if ["BYLAYER", "BYBLOCK", "CONTINUOUS"].contains(&old.to_uppercase().as_str()) {
                return false;
            }
            if !rekey(&mut doc.line_types, old, new) {
                return false;
            }
            for e in doc.entities_mut() {
                if e.common().linetype.eq_ignore_ascii_case(old) {
                    e.common_mut().linetype = new.to_string();
                }
            }
            for l in doc.layers.iter_mut() {
                if l.line_type.eq_ignore_ascii_case(old) {
                    l.line_type = new.to_string();
                }
            }
            if doc.header.current_linetype_name.eq_ignore_ascii_case(old) {
                doc.header.current_linetype_name = new.to_string();
            }
            true
        }
        "UCS" => {
            if !rekey(&mut doc.ucss, old, new) {
                return false;
            }
            if doc.header.model_space_ucs_name.eq_ignore_ascii_case(old) {
                doc.header.model_space_ucs_name = new.to_string();
            }
            true
        }
        "VIEW" => rekey(&mut doc.views, old, new),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;

    fn fresh_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// `SETVAR ZOOMFACTOR` is the system variable, so it keeps the range
    /// the variable has — the Options window is where a wheel faster than
    /// that is set, and a value set there is still what the variable
    /// reports.
    #[test]
    fn setvar_zoomfactor_keeps_the_range_of_the_system_variable() {
        let mut app = fresh_app();

        let _ = app.run_command_line("SETVAR ZOOMFACTOR 200");
        assert_eq!(app.zoom_factor, 200);

        let _ = app.run_command_line("SETVAR ZOOMFACTOR 201");
        assert_eq!(
            app.zoom_factor, 200,
            "201 is not a value the variable takes"
        );

        let _ = app.run_command_line("SETVAR ZOOMFACTOR 2");
        assert_eq!(app.zoom_factor, 200, "and neither is 2");

        // What the Options field reached is still reported here.
        let _ = app.update(crate::app::Message::ZoomFactorInputChanged("150".into()));
        assert_eq!(app.zoom_factor, 150);
        let _ = app.run_command_line("SETVAR ZOOMFACTOR 60");
        assert_eq!(app.zoom_factor, 60);
    }

    /// `SETVAR ISOLINES` stored any parseable `i16` verbatim. The header
    /// value is the per-face isolate count handed to the tessellation
    /// kernel (which loops `0..count`), so `ISOLINES 32767` made every
    /// rebuild and per-cursor preview allocate without bound. The stored
    /// value must stay inside the range the app's own UI can ask for
    /// (the Options slider is 0..=64).
    #[test]
    fn setvar_isolines_stays_within_the_renderable_range() {
        let mut app = fresh_app();
        let i = app.active_tab;

        let _ = app.run_command_line("SETVAR ISOLINES 32767");
        assert_eq!(app.tabs[i].scene.document.header.isolines, 64);

        let _ = app.run_command_line("SETVAR ISOLINES 8");
        assert_eq!(app.tabs[i].scene.document.header.isolines, 8);

        let _ = app.run_command_line("SETVAR ISOLINES -3");
        assert_eq!(app.tabs[i].scene.document.header.isolines, 0);
    }

    /// `SETVAR LUPREC` / `AUPREC` stored any parseable `i16` verbatim. The
    /// units dialog only ever offers 0-8 places, and the formatters read the
    /// header on every properties/annotation call, so an unclamped value
    /// (positive or negative) fed unbounded strings into those hot paths.
    #[test]
    fn setvar_precision_stays_within_the_range_the_units_dialog_offers() {
        let mut app = fresh_app();
        let i = app.active_tab;

        let _ = app.run_command_line("SETVAR LUPREC 30000");
        assert_eq!(app.tabs[i].scene.document.header.linear_unit_precision, 8);

        let _ = app.run_command_line("SETVAR AUPREC 30000");
        assert_eq!(app.tabs[i].scene.document.header.angular_unit_precision, 8);

        let _ = app.run_command_line("SETVAR LUPREC -3");
        assert_eq!(app.tabs[i].scene.document.header.linear_unit_precision, 0);

        let _ = app.run_command_line("SETVAR LUPREC 5");
        assert_eq!(app.tabs[i].scene.document.header.linear_unit_precision, 5);
    }

    #[test]
    fn selection_commands_gather_instead_of_using_the_whole_drawing() {
        for command in ["SETBYLAYER", "OVERKILL"] {
            let mut app = fresh_app();
            let _ = app.run_command_line(command);
            assert!(
                app.tabs[app.active_tab]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|active| active.is_selection_gathering()),
                "{command} should gather an explicit selection"
            );
        }
    }

    #[test]
    fn test_cad_selection_and_model_space_sysvars() {
        let mut app = fresh_app();

        // SELECTIONAREA toggle
        let _ = app.run_command_line("SETVAR SELECTIONAREA 0");
        assert!(!app.model_space.selection_area);
        let _ = app.run_command_line("SELECTIONAREA 1");
        assert!(app.model_space.selection_area);

        // SELECTIONAREAOPACITY
        let _ = app.run_command_line("SETVAR SELECTIONAREAOPACITY 45");
        assert_eq!(app.model_space.selection_opacity, 45);
        let _ = app.run_command_line("SELECTIONAREAOPACITY 80");
        assert_eq!(app.model_space.selection_opacity, 80);

        // SELECTIONEFFECT
        let _ = app.run_command_line("SETVAR SELECTIONEFFECT 0");
        assert!(!app.model_space.selection_effect);
        assert!(!app.tabs[app.active_tab].scene.selection_effect);
        let _ = app.run_command_line("SELECTIONEFFECT 1");
        assert!(app.model_space.selection_effect);
        assert!(app.tabs[app.active_tab].scene.selection_effect);

        // SELECTIONEFFECTCOLOR
        let _ = app.run_command_line("SETVAR SELECTIONEFFECTCOLOR 4");
        assert_eq!(app.model_space.selection_highlight_color, 4);
        assert_eq!(
            app.tabs[app.active_tab].scene.selection_color,
            app.model_space.resolve_selection_color()
        );

        // WINDOWSAREACOLOR & CROSSINGAREACOLOR
        let _ = app.run_command_line("SETVAR WINDOWSAREACOLOR 5");
        assert_eq!(app.model_space.selection_window_color, 5);
        let _ = app.run_command_line("WINDOWSAREACOLOR 1");
        assert_eq!(app.model_space.selection_window_color, 1);

        let _ = app.run_command_line("SETVAR CROSSINGAREACOLOR 3");
        assert_eq!(app.model_space.selection_crossing_color, 3);
        let _ = app.run_command_line("CROSSINGAREACOLOR 2");
        assert_eq!(app.model_space.selection_crossing_color, 2);

        // SELECTIONPREVIEW
        let _ = app.run_command_line("SETVAR SELECTIONPREVIEW 2");
        assert_eq!(app.model_space.selection_preview, 2);

        let _ = app.run_command_line("SETVAR GRIPOBJLIMIT 0");
        assert_eq!(app.grip_object_limit, 0);
        let _ = app.run_command_line("GRIPOBJLIMIT 32767");
        assert_eq!(app.grip_object_limit, 32767);
        let _ = app.run_command_line("GRIPOBJLIMIT 32768");
        assert_eq!(app.grip_object_limit, 32767);
    }

    #[test]
    fn test_background_command_modes() {
        let mut app = fresh_app();
        let i = app.active_tab;

        // BACKGROUND CLASSIC
        let _ = app.run_command_line("BACKGROUND CLASSIC");
        assert_eq!(
            app.model_space.mode,
            crate::app::config::ModelSpaceMode::ClassicDark
        );
        assert_eq!(
            app.tabs[i].scene.bg_color,
            [33.0 / 255.0, 40.0 / 255.0, 48.0 / 255.0, 1.0]
        );

        // BACKGROUND RGB custom
        let _ = app.run_command_line("BACKGROUND 50 60 70");
        assert_eq!(
            app.model_space.mode,
            crate::app::config::ModelSpaceMode::Custom
        );
        assert_eq!(app.model_space.custom_bg, Some([50, 60, 70]));
        assert!((app.tabs[i].scene.bg_color[0] - 50.0 / 255.0).abs() < 1e-4);
        assert!((app.tabs[i].scene.bg_color[1] - 60.0 / 255.0).abs() < 1e-4);
        assert!((app.tabs[i].scene.bg_color[2] - 70.0 / 255.0).abs() < 1e-4);

        // BACKGROUND DEFAULT restores MatchTheme
        let _ = app.run_command_line("BACKGROUND DEFAULT");
        assert_eq!(
            app.model_space.mode,
            crate::app::config::ModelSpaceMode::MatchTheme
        );
        assert_eq!(app.model_space.custom_bg, None);

        // BACKGROUND DESK
        let _ = app.run_command_line("BACKGROUND DESK 80 85 90");
        assert_eq!(app.model_space.custom_desk_bg, Some([80, 85, 90]));
        let _ = app.run_command_line("BACKGROUND DESK DEFAULT");
        assert_eq!(app.model_space.custom_desk_bg, None);
    }

    #[test]
    fn test_reviewer_feedback_fixes() {
        let mut app = fresh_app();

        // An empty background restores MatchTheme.
        let _ = app.update(crate::app::Message::ModelSpaceBgChanged("".to_string()));
        assert_eq!(
            app.model_space.mode,
            crate::app::config::ModelSpaceMode::MatchTheme
        );
        assert_eq!(app.model_space.custom_bg, None);

        // Display and selection defaults are restored separately.
        app.model_space.mode = crate::app::config::ModelSpaceMode::Custom;
        app.model_space.custom_bg = Some([10, 20, 30]);
        app.model_space.selection_opacity = 99;
        let _ = app.update(crate::app::Message::RestoreModelSpaceDisplayDefaults);
        assert_eq!(
            app.model_space.mode,
            crate::app::config::ModelSpaceMode::MatchTheme
        );
        assert_eq!(
            app.model_space.selection_opacity, 99,
            "Selection visual must not be wiped by display restore"
        );

        let _ = app.update(crate::app::Message::RestoreSelectionVisualDefaults);
        assert_eq!(
            app.model_space.selection_opacity, 12,
            "Selection restore resets selection visual defaults"
        );
    }

    #[test]
    fn test_grip_and_desk_surround_sysvars() {
        let mut app = fresh_app();

        // 1. Desk surround background message
        let _ = app.update(crate::app::Message::DeskSpaceBgChanged(
            "#1a2b3c".to_string(),
        ));
        assert_eq!(app.model_space.custom_desk_bg, Some([0x1a, 0x2b, 0x3c]));
        assert_eq!(app.desk_bg_input, "#1a2b3c");
        let _ = app.update(crate::app::Message::DeskSpaceBgChanged("".to_string()));
        assert_eq!(app.model_space.custom_desk_bg, None);

        // 2. GRIPSIZE, GRIPCOLOR, GRIPHOT, GRIPHOVER via command line & SETVAR
        let _ = app.run_command_line("GRIPSIZE 8");
        assert_eq!(app.model_space.grip_size, 8);
        let _ = app.run_command_line("GRIPCOLOR 4");
        assert_eq!(app.model_space.grip_color, 4);
        let _ = app.run_command_line("GRIPHOT 1");
        assert_eq!(app.model_space.grip_hot, 1);
        let _ = app.run_command_line("GRIPHOVER 2");
        assert_eq!(app.model_space.grip_hover, 2);

        // Direct messages
        let _ = app.update(crate::app::Message::GripSizeChanged(12));
        assert_eq!(app.model_space.grip_size, 12);
        let _ = app.update(crate::app::Message::GripColorChanged(5));
        assert_eq!(app.model_space.grip_color, 5);

        // Restore defaults resets grips
        let _ = app.update(crate::app::Message::RestoreSelectionVisualDefaults);
        assert_eq!(app.model_space.grip_size, 5);
        assert_eq!(app.model_space.grip_color, 0);
        assert_eq!(app.model_space.grip_hot, 0);
        assert_eq!(app.model_space.grip_hover, 0);
    }

    #[test]
    fn bare_commandlinefadetime_stays_active_until_value_or_cancel() {
        let mut app = fresh_app();
        let _ = app.run_command_line("COMMANDLINEFADETIME");
        // Like RECTANGLE and PICKADD: bare form installs an active command
        // whose pinned prompt stays until a value, Enter, or Esc.
        assert!(
            app.tabs[app.active_tab].active_cmd.is_some(),
            "bare COMMANDLINEFADETIME must stay active"
        );
        // A value sets the variable and ends the command.
        let _ = app.run_command_line("COMMANDLINEFADETIME 5000");
        assert_eq!(app.commandline_fade_ms, 5000);
        assert_eq!(app.command_line.commandline_fade_ms(), 5000);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        // Bare again, then Esc cancels without changing the value.
        let _ = app.run_command_line("COMMANDLINEFADETIME");
        assert!(app.tabs[app.active_tab].active_cmd.is_some());
        let _ = app.update(crate::app::Message::CommandEscape);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        assert_eq!(app.commandline_fade_ms, 5000);
    }

    #[test]
    fn delobj_accepts_values_zero_through_three() {
        let mut app = fresh_app();

        for value in 0..=3 {
            let _ = app.run_command_line(&format!("SETVAR DELOBJ {value}"));
            assert_eq!(app.delete_objects, value);
        }

        let _ = app.run_command_line("SETVAR DELOBJ 4");
        assert_eq!(app.delete_objects, 3);
    }

    #[test]
    fn dynmode_direct_and_setvar_toggles_dynamic_input() {
        let mut app = fresh_app();
        assert!(app.dyn_input);
        assert_eq!(app.dyn_mode, 3);

        // Turn off via direct command: DYNMODE 0
        let _ = app.run_command_line("DYNMODE 0");
        assert!(!app.dyn_input);
        assert_eq!(app.dyn_mode, 0);

        // Turn on via direct command: DYNMODE 3
        let _ = app.run_command_line("DYNMODE 3");
        assert!(app.dyn_input);
        assert_eq!(app.dyn_mode, 3);

        // Turn off via SETVAR: SETVAR DYNMODE 0
        let _ = app.run_command_line("SETVAR DYNMODE 0");
        assert!(!app.dyn_input);
        assert_eq!(app.dyn_mode, 0);

        // Turn on via SETVAR: SETVAR DYNMODE 1 (pointer input only)
        let _ = app.run_command_line("SETVAR DYNMODE 1");
        assert!(app.dyn_input);
        assert_eq!(app.dyn_mode, 1);

        // Out of range rejected
        let _ = app.run_command_line("DYNMODE 5");
        assert_eq!(app.dyn_mode, 1);
        assert!(app.dyn_input);

        let _ = app.run_command_line("DYNMODE -5");
        assert_eq!(app.dyn_mode, 1);
        assert!(app.dyn_input);

        // F12 turns it off temporarily (negative) and restores the mode.
        let _ = app.update(crate::app::Message::ToggleDynInput);
        assert!(!app.dyn_input);
        assert_eq!(app.dyn_mode, -1);

        let _ = app.update(crate::app::Message::ToggleDynInput);
        assert!(app.dyn_input);
        assert_eq!(app.dyn_mode, 1);
    }
}

#[cfg(test)]
mod scale_validation_tests {
    use crate::app::OpenCADStudio;

    /// A linetype scale of zero or less degenerates every dash pattern in the
    /// drawing, which is why the standalone commands reject it. Reaching the
    /// same variable through SETVAR used to accept it, so which door you came
    /// through decided whether the drawing could be broken.
    #[test]
    fn both_doors_to_ltscale_refuse_a_non_positive_value() {
        for name in ["LTSCALE", "CELTSCALE"] {
            for entry in [format!("{name} -5"), format!("SETVAR {name} -5")] {
                let mut app = OpenCADStudio::new_for_test();
                app.automation_op(r#"{"op":"new"}"#);
                let i = app.active_tab;
                let before = if name == "LTSCALE" {
                    app.tabs[i].scene.document.header.linetype_scale
                } else {
                    app.tabs[i]
                        .scene
                        .document
                        .header
                        .current_entity_linetype_scale
                };
                let _ = app.run_command_line(&entry);
                let after = if name == "LTSCALE" {
                    app.tabs[i].scene.document.header.linetype_scale
                } else {
                    app.tabs[i]
                        .scene
                        .document
                        .header
                        .current_entity_linetype_scale
                };
                assert_eq!(before, after, "{entry} must be refused");
            }
        }
    }
}

impl OpenCADStudio {
    /// XDWGFADECTL: store it in the profile, show it on the ribbon and redraw
    /// every tab's referenced drawings.
    pub(in crate::app) fn set_xref_fade(&mut self, fade: i32) {
        crate::scene::cache::block_cache::set_xref_fade_ctl(fade);
        self.ribbon.xref_fade = crate::scene::cache::block_cache::xref_fade_ctl();
        self.save_config();
        for tab in &mut self.tabs {
            if !tab.is_start {
                tab.scene.recolor_meshes();
                tab.scene.bump_geometry();
            }
        }
    }
}
