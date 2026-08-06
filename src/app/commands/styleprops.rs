use super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_styleprops(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
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
                use acadrust::types::Color;
                let describe = |c: &Color| match c {
                    Color::ByLayer => "ByLayer".to_string(),
                    Color::ByBlock => "ByBlock".to_string(),
                    Color::Index(n) => format!("index {n}"),
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
                        self.command_line
                            .push_output(crate::tf!("Current object colour: {}", describe(&c)).as_ref());
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
                        self.command_line
                            .push_output(crate::tf!("Object colour set to {}.", describe(&c)).as_ref());
                    }
                    None => {
                        self.command_line.push_error(
                            "Usage: COLOR <ByLayer|ByBlock|1-255|red|yellow|green|cyan|blue|magenta|white>",
                        );
                    }
                }
            }

            // ── LINETYPE management ───────────────────────────────────────
            "LINETYPE" | "LT" => {
                use crate::command::KeywordCommand;
                let c = KeywordCommand::new(
                    "LINETYPE",
                    "LINETYPE  [List / Set]:",
                    vec![
                        ("List", "LIST", None),
                        ("Set", "SET", Some("LINETYPE SET  linetype name (ByLayer / ByBlock / …):")),
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
                    "" | "LIST" | "?" => {
                        let ltypes: Vec<String> = self.tabs[i]
                            .scene
                            .document
                            .line_types
                            .iter()
                            .map(|lt| format!("{} ({})", lt.name, lt.description))
                            .collect();
                        if ltypes.is_empty() {
                            self.command_line.push_output(crate::t!("No linetypes defined.").as_ref());
                        } else {
                            self.command_line
                                .push_output(crate::tf!("Linetypes: {}", ltypes.join(", ")).as_ref());
                        }
                    }
                    // Set the current linetype applied to newly drawn entities.
                    "SET" | "CURRENT" | "S" => {
                        let name = parts.get(1).copied().unwrap_or("");
                        if name.is_empty() {
                            self.command_line
                                .push_info(crate::t!("Usage: LINETYPE SET <name | ByLayer | ByBlock>").as_ref());
                        } else {
                            let canon = if name.eq_ignore_ascii_case("BYLAYER") {
                                Some(("ByLayer".to_string(), acadrust::types::Handle::NULL))
                            } else if name.eq_ignore_ascii_case("BYBLOCK") {
                                Some(("ByBlock".to_string(), acadrust::types::Handle::NULL))
                            } else {
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
                                    self.command_line
                                        .push_output(crate::tf!("Current linetype set to {nm}.").as_ref());
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
            // Bare PURGE prompts with the category options as clickable
            // keywords; PURGE <sub> (typed, clicked or scripted) runs directly.
            "PURGE" => {
                use crate::command::KeywordCommand;
                let c = KeywordCommand::new(
                    "PURGE",
                    "PURGE  [All / Blocks / LAyers / LineTypes / STyles]:",
                    vec![
                        ("All", "ALL", None),
                        ("Blocks", "BLOCKS", None),
                        ("LAyers", "LAYERS", None),
                        ("LineTypes", "LINETYPES", None),
                        ("STyles", "STYLES", None),
                    ],
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("PURGE ") => {
                let sub = cmd
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("ALL")
                    .to_uppercase();
                let all = sub == "ALL" || sub.is_empty();
                let do_layers = all || matches!(sub.as_str(), "LAYERS" | "LAYER" | "LA");
                let do_styles = all || matches!(sub.as_str(), "TEXTSTYLES" | "STYLES" | "ST");
                let do_lts = all || matches!(sub.as_str(), "LINETYPES" | "LT");
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
                    let used_layers: rustc_hash::FxHashSet<String> = self.tabs[i]
                        .scene
                        .document
                        .entities()
                        .filter_map(|e| {
                            let name = &e.common().layer;
                            if name.is_empty() {
                                None
                            } else {
                                Some(name.clone())
                            }
                        })
                        .collect();
                    // Text styles: entity references plus every dimension
                    // style's DIMTXSTY — a style only a dimstyle uses is not
                    // purgeable.
                    let used_text_styles: rustc_hash::FxHashSet<String> = self.tabs[i]
                        .scene
                        .document
                        .entities()
                        .filter_map(|e| match e {
                            acadrust::EntityType::Text(t) => Some(t.style.clone()),
                            acadrust::EntityType::MText(t) => Some(t.style.clone()),
                            acadrust::EntityType::AttributeDefinition(a) => {
                                Some(a.text_style.clone())
                            }
                            acadrust::EntityType::AttributeEntity(a) => {
                                Some(a.text_style.clone())
                            }
                            _ => None,
                        })
                        .chain(
                            self.tabs[i]
                                .scene
                                .document
                                .dim_styles
                                .iter()
                                .map(|ds| ds.dimtxsty.clone()),
                        )
                        .filter(|s| !s.is_empty())
                        .collect();
                    // Linetypes: entity references plus every layer's own
                    // linetype — a ByLayer entity reaches it through the layer.
                    let used_linetypes: rustc_hash::FxHashSet<String> = self.tabs[i]
                        .scene
                        .document
                        .entities()
                        .filter_map(|e| {
                            let lt = &e.common().linetype;
                            if lt.is_empty() || lt == "ByLayer" || lt == "ByBlock" {
                                None
                            } else {
                                Some(lt.clone())
                            }
                        })
                        .chain(
                            self.tabs[i]
                                .scene
                                .document
                                .layers
                                .iter()
                                .map(|l| l.line_type.clone()),
                        )
                        .filter(|s| !s.is_empty())
                        .collect();

                    // Which block definitions are LIVE — reachable from a real
                    // layout container by following every block reference. A
                    // block nothing reachable inserts is dead and purges, even a
                    // whole mutually-referencing subgraph (e.g. the dependent
                    // blocks a detached xref leaves behind: they insert one
                    // another but hang off no layout, so a flat "is it named by
                    // any INSERT" scan keeps them alive forever). Roots:
                    //   • *Model_Space / *Paper_Space(N),
                    //   • any block whose layout handle resolves to a real Layout
                    //     object — a DANGLING layout handle, as orphaned xref
                    //     blocks carry, does NOT count, so is_layout() alone can't
                    //     shield the dead subgraph it heads,
                    //   • dimension-style arrowhead blocks (referenced by handle).
                    // A block's out-edges: its members' INSERT names, dimension
                    // *D blocks, table *T / cell blocks, and MultiLeader content.
                    let live_blocks: rustc_hash::FxHashSet<String> = {
                        let doc = &self.tabs[i].scene.document;
                        let by_handle: rustc_hash::FxHashMap<acadrust::Handle, String> = doc
                            .block_records
                            .iter()
                            .map(|br| (br.handle, br.name.clone()))
                            .collect();
                        let is_real_layout = |br: &acadrust::BlockRecord| -> bool {
                            let up = br.name.to_ascii_uppercase();
                            up.starts_with("*MODEL_SPACE")
                                || up.starts_with("*PAPER_SPACE")
                                || matches!(
                                    doc.objects.get(&br.layout),
                                    Some(acadrust::objects::ObjectType::Layout(_))
                                )
                        };
                        let children = |name: &str| -> Vec<String> {
                            let mut out = Vec::new();
                            let Some(br) = doc.block_records.get(name) else {
                                return out;
                            };
                            for &h in &br.entity_handles {
                                match doc.get_entity(h) {
                                    Some(acadrust::EntityType::Insert(ins))
                                        if !ins.block_name.is_empty() =>
                                    {
                                        out.push(ins.block_name.clone());
                                    }
                                    Some(acadrust::EntityType::Dimension(d))
                                        if !d.base().block_name.is_empty() =>
                                    {
                                        out.push(d.base().block_name.clone());
                                    }
                                    Some(acadrust::EntityType::Table(t)) => {
                                        if let Some(bh) = t.block_record_handle {
                                            if let Some(n) = by_handle.get(&bh) {
                                                out.push(n.clone());
                                            }
                                        }
                                        for row in &t.rows {
                                            for cell in &row.cells {
                                                for c in &cell.contents {
                                                    if let Some(bh) = c.block_handle {
                                                        if let Some(n) = by_handle.get(&bh) {
                                                            out.push(n.clone());
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Some(acadrust::EntityType::MultiLeader(ml)) => {
                                        if let Some(bh) = ml.block_content_handle {
                                            if let Some(n) = by_handle.get(&bh) {
                                                out.push(n.clone());
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            out
                        };
                        let mut live = rustc_hash::FxHashSet::default();
                        let mut stack: Vec<String> = doc
                            .block_records
                            .iter()
                            .filter(|&br| is_real_layout(br))
                            .map(|br| br.name.clone())
                            .collect();
                        for ds in doc.dim_styles.iter() {
                            for h in [ds.dimblk, ds.dimblk1, ds.dimblk2, ds.dimldrblk] {
                                if !h.is_null() {
                                    if let Some(n) = by_handle.get(&h) {
                                        stack.push(n.clone());
                                    }
                                }
                            }
                        }
                        while let Some(n) = stack.pop() {
                            if !live.insert(n.clone()) {
                                continue;
                            }
                            for c in children(&n) {
                                if !live.contains(&c) {
                                    stack.push(c);
                                }
                            }
                        }
                        live
                    };

                    // Build removal lists (still immutable)
                    let layer_remove: Vec<String> = if do_layers {
                        self.tabs[i]
                            .scene
                            .document
                            .layers
                            .iter()
                            .filter(|l| l.name != "0" && !used_layers.contains(&l.name))
                            .map(|l| l.name.clone())
                            .collect()
                    } else {
                        vec![]
                    };
                    let style_remove: Vec<String> = if do_styles {
                        self.tabs[i]
                            .scene
                            .document
                            .text_styles
                            .iter()
                            .filter(|s| {
                                s.name != "Standard" && !used_text_styles.contains(&s.name)
                            })
                            .map(|s| s.name.clone())
                            .collect()
                    } else {
                        vec![]
                    };
                    let lt_remove: Vec<String> = if do_lts {
                        let standard = ["Continuous", "ByLayer", "ByBlock"];
                        self.tabs[i]
                            .scene
                            .document
                            .line_types
                            .iter()
                            .filter(|lt| {
                                !standard.iter().any(|s| s.eq_ignore_ascii_case(&lt.name))
                                    && !used_linetypes.contains(&lt.name)
                            })
                            .map(|lt| lt.name.clone())
                            .collect()
                    } else {
                        vec![]
                    };

                    // Blocks: everything the layout-reachability walk above did
                    // NOT mark live. That covers unreferenced named blocks,
                    // leftover anonymous blocks (*U hatch/unnamed, orphaned *D
                    // dimension, *T table, dynamic-block variants — AutoCAD drops
                    // these too), AND whole dead subgraphs a detached xref leaves
                    // behind. Live xrefs stay (an attached xref's blocks are
                    // reached from the layout that inserts them); the *Model_Space
                    // / *Paper_Space containers are never removed.
                    let block_remove: Vec<String> = if do_blocks {
                        self.tabs[i]
                            .scene
                            .document
                            .block_records
                            .iter()
                            .filter(|br| {
                                let up = br.name.to_ascii_uppercase();
                                // Never PURGE an xref *definition* — that is a
                                // detach, not a purge. (Orphaned dependent blocks
                                // a past detach left behind are is_xref=false and
                                // still fall to the reachability test.)
                                !br.flags.is_xref
                                    && !br.flags.is_xref_overlay
                                    && !up.starts_with("*MODEL_SPACE")
                                    && !up.starts_with("*PAPER_SPACE")
                                    && !live_blocks.contains(&br.name)
                            })
                            .map(|br| br.name.clone())
                            .collect()
                    } else {
                        vec![]
                    };

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
                        self.push_undo_snapshot(i, "PURGE");
                        snapshot_pushed = true;
                    }

                    // Apply removals (mutable)
                    for name in &layer_remove {
                        self.tabs[i].scene.document.layers.remove(name);
                    }
                    for name in &style_remove {
                        self.tabs[i].scene.document.text_styles.remove(name);
                    }
                    for name in &lt_remove {
                        self.tabs[i].scene.document.line_types.remove(name);
                    }
                    for name in &block_remove {
                        // Drop the block definition's member entities (and the
                        // BLOCK/ENDBLK delimiters) before the record so no orphaned
                        // geometry survives in the document entity list.
                        let handles: Vec<_> = if let Some(br) =
                            self.tabs[i].scene.document.block_records.get(name)
                        {
                            let mut h = br.entity_handles.clone();
                            h.push(br.block_entity_handle);
                            h.push(br.block_end_handle);
                            h
                        } else {
                            Vec::new()
                        };
                        for h in handles {
                            self.tabs[i].scene.document.remove_entity(h);
                        }
                        self.tabs[i].scene.document.block_records.remove(name);
                    }
                    n_layers += layer_remove.len();
                    n_styles += style_remove.len();
                    n_lts += lt_remove.len();
                    n_blocks += block_remove.len();
                }

                // Draw-order cleanup. A SortEntitiesTable lists a block's
                // entities in draw order; purging the block drops the entities
                // and the block record, but this object lingers, still holding
                // the (now dangling) handle of every deleted entity. On a
                // detached-xref purge that is hundreds of thousands of stale
                // handles — megabytes of dead weight the save re-emits, which is
                // why an OCS-purged file stayed far larger than AutoCAD's.
                // Drop every SortEntitiesTable whose owning block is gone
                // (AutoCAD's "orphaned data").
                let mut n_sortents = 0usize;
                if do_blocks {
                    let live_blocks: rustc_hash::FxHashSet<acadrust::Handle> = self.tabs[i]
                        .scene
                        .document
                        .block_records
                        .iter()
                        .map(|br| br.handle)
                        .collect();
                    let orphans: Vec<acadrust::Handle> = self.tabs[i]
                        .scene
                        .document
                        .objects
                        .iter()
                        .filter_map(|(h, o)| match o {
                            acadrust::objects::ObjectType::SortEntitiesTable(s)
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
                            self.push_undo_snapshot(i, "PURGE");
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
                        parts.push(format!("{n_layers} layer(s)"));
                    }
                    if n_styles > 0 {
                        parts.push(format!("{n_styles} text style(s)"));
                    }
                    if n_lts > 0 {
                        parts.push(format!("{n_lts} linetype(s)"));
                    }
                    if n_blocks > 0 {
                        parts.push(format!("{n_blocks} block(s)"));
                    }
                    if n_sortents > 0 {
                        parts.push(format!("{n_sortents} stale draw-order table(s)"));
                    }
                    self.command_line.push_output(crate::tf!(
                        "PURGE: {} item(s) removed — {}.",
                        purged + n_sortents,
                        parts.join(", ")
                    ).as_ref());
                } else {
                    self.command_line.push_output(crate::t!("PURGE: nothing to purge.").as_ref());
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
                        ("Color", "COLOR", Some("CHPROP  new colour (name / 1-255 / ByLayer):")),
                        ("Linetype", "LINETYPE", Some("CHPROP  new linetype name:")),
                        ("LtScale", "LTSCALE", Some("CHPROP  new linetype scale:")),
                        ("Transparency", "TRANSPARENCY", Some("CHPROP  transparency 0-90:")),
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
                        "Usage: CHPROP <prop> <val>  (props: LAYER COLOR LINETYPE LTSCALE)",
                    );
                } else {
                    let handles: Vec<_> = self.tabs[i]
                        .scene
                        .selected_entities()
                        .into_iter()
                        .map(|(h, _)| h)
                        .collect();
                    if handles.is_empty() {
                        self.command_line
                            .push_error(crate::t!("CHPROP: no entities selected.").as_ref());
                    } else {
                        // Validate value early to give clear errors
                        let color_val: Option<acadrust::types::Color> = if prop == "COLOR" {
                            value
                                .parse::<i16>()
                                .ok()
                                .map(acadrust::types::Color::from_index)
                        } else {
                            None
                        };
                        let ltscale_val: Option<f64> = if prop == "LTSCALE" {
                            value.parse().ok()
                        } else {
                            None
                        };
                        let transparency_val: Option<acadrust::types::Transparency> =
                            if prop == "TRANSPARENCY" {
                                value
                                    .parse::<f64>()
                                    .ok()
                                    .map(acadrust::types::Transparency::from_percent)
                            } else {
                                None
                            };

                        if (prop == "COLOR" && color_val.is_none())
                            || (prop == "LTSCALE" && ltscale_val.is_none())
                            || (prop == "TRANSPARENCY" && transparency_val.is_none())
                        {
                            self.command_line.push_error(crate::tf!(
                                "CHPROP: invalid value '{}' for {}.",
                                value, prop
                            ).as_ref());
                        } else {
                            let mut changed = 0usize;
                            for handle in &handles {
                                if let Some(entity) =
                                    self.tabs[i].scene.document.get_entity_mut(*handle)
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
                                            changed += 1;
                                        }
                                        "TRANSPARENCY" => {
                                            common.transparency = transparency_val.unwrap();
                                            changed += 1;
                                        }
                                        _ => {
                                            self.command_line.push_error(crate::tf!(
                                                "CHPROP: unknown property '{}'. Use: LAYER COLOR LINETYPE LTSCALE TRANSPARENCY", prop
                                            ).as_ref());
                                            break;
                                        }
                                    }
                                }
                            }
                            if changed > 0 {
                                self.push_undo_snapshot(i, "CHPROP");
                                self.tabs[i].dirty = true;
                                // Colour / linetype / ltscale / transparency /
                                // layer are baked into the cached wire geometry —
                                // re-tessellate the changed entities so they
                                // repaint immediately (issue #231 class).
                                self.invalidate_property_targets(i, &handles);
                                self.command_line.push_output(crate::tf!(
                                    "CHPROP: {} entity/entities updated.",
                                    changed
                                ).as_ref());
                            }
                        }
                    }
                }
            }

            // ── SETBYLAYER — clear color/linetype/lineweight overrides ────
            // Resets the selected entities' direct property overrides back to
            // ByLayer so they follow their layer again.
            "SETBYLAYER" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("SETBYLAYER: select entities first.").as_ref());
                } else {
                    self.push_undo_snapshot(i, "SETBYLAYER");
                    let mut changed = 0usize;
                    for handle in &handles {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(*handle) {
                            let common = entity.common_mut();
                            common.color = acadrust::types::Color::ByLayer;
                            common.linetype = "ByLayer".to_string();
                            common.line_weight = acadrust::types::LineWeight::ByLayer;
                            changed += 1;
                        }
                    }
                    self.tabs[i].dirty = true;
                    let changes: Vec<_> = handles
                        .into_iter()
                        .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.command_line.push_output(crate::tf!(
                        "SETBYLAYER: reset {changed} entity/entities to ByLayer."
                    ).as_ref());
                }
            }

            // ── OVERKILL — delete duplicate (identical) objects ──────────
            // Removes objects that are identical in geometry AND properties to
            // another object (compared with the handle ignored). Operates on
            // the current selection, or the whole drawing when nothing is
            // selected. Conservative: only exact duplicates are removed.
            "OVERKILL" => {
                use acadrust::Handle;
                let selected: std::collections::HashSet<u64> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h.value())
                    .collect();
                // Capture (handle, type-name, handle-normalized clone) for each
                // candidate while the document is borrowed immutably.
                let candidates: Vec<(Handle, String, acadrust::EntityType)> = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter(|e| {
                        selected.is_empty() || selected.contains(&e.common().handle.value())
                    })
                    .map(|e| {
                        let key = crate::entities::names::dxf_name(e).to_string();
                        let mut norm = e.clone();
                        norm.common_mut().handle = Handle::NULL;
                        (e.common().handle, key, norm)
                    })
                    .collect();
                // Bucket by (type, layer) so only like objects are compared.
                let mut kept: Vec<(String, acadrust::EntityType)> = Vec::new();
                let mut dups: Vec<Handle> = Vec::new();
                for (h, key, norm) in &candidates {
                    let bucket = format!("{key}\u{0}{}", norm.common().layer);
                    if kept.iter().any(|(b, e)| b == &bucket && e == norm) {
                        dups.push(*h);
                    } else {
                        kept.push((bucket, norm.clone()));
                    }
                }
                if dups.is_empty() {
                    self.command_line
                        .push_output(crate::t!("OVERKILL: no duplicate objects found.").as_ref());
                } else {
                    let n = dups.len();
                    self.push_undo_snapshot(i, "OVERKILL");
                    self.tabs[i].scene.erase_entities(&dups);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                    self.command_line
                        .push_output(crate::tf!("OVERKILL: deleted {n} duplicate object(s).").as_ref());
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
                    self.command_line.push_output(crate::tf!(
                        "{} = {v}",
                        if is_add { "PICKADD" } else { "PICKDRAG" }
                    ).as_ref());
                } else {
                    match arg {
                        "0" | "1" => {
                            let on = arg == "1";
                            if is_add {
                                self.pick_add = on;
                                self.command_line.push_output(crate::tf!(
                                    "PICKADD = {} ({})",
                                    arg,
                                    if on {
                                        "click adds to selection"
                                    } else {
                                        "click replaces selection, Shift toggles"
                                    }
                                ).as_ref());
                            } else {
                                self.pick_drag_rect = on;
                                self.command_line.push_output(crate::tf!(
                                    "PICKDRAG = {} ({})",
                                    arg,
                                    if on {
                                        "press-drag draws a rectangle"
                                    } else {
                                        "press-drag lassoes"
                                    }
                                ).as_ref());
                            }
                            self.persist_settings_if_changed();
                        }
                        _ => self.command_line.push_error(crate::tf!(
                            "{}: expected 0 or 1.",
                            if is_add { "PICKADD" } else { "PICKDRAG" }
                        ).as_ref()),
                    }
                }
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
                    | "TEXTFILL"
                    | "ATTREQ"
                    | "ATTDIA"
                    | "DIMASSOC"
                    | "ANGBASE"
                    | "ANGDIR"
                    | "REGENMODE"
                    | "BLIPMODE"
                    | "SPLFRAME"
                    | "DELOBJ"
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
                    | "TEXTQLTY"
                    | "SORTENTS"
                    | "XCLIPFRAME"
                    | "HALOGAP"
                    | "TRACEWID"
                    | "SKETCHINC"
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
                    self.command_line.push_info(
                        "SETVAR: LTSCALE CELTSCALE PDMODE PDSIZE TEXTSIZE ORTHOMODE FILLMODE MIRRTEXT ATTREQ ATTDIA DIMASSOC ANGBASE ANGDIR | CLAYER CELTYPE TEXTSTYLE (read-only)",
                    );
                } else {
                    // Parse a boolean given as 0/1 or ON/OFF.
                    let parse_bool = |s: &str| match s.to_uppercase().as_str() {
                        "1" | "ON" | "TRUE" => Some(true),
                        "0" | "OFF" | "FALSE" => Some(false),
                        _ => None,
                    };
                    let outcome: Result<(String, bool), String> = {
                        let h = &mut self.tabs[i].scene.document.header;
                        match name.as_str() {
                            "LTSCALE" => match &value {
                                Some(v) => v
                                    .parse::<f64>()
                                    .map(|x| {
                                        h.linetype_scale = x;
                                        (format!("LTSCALE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: numeric value required.".into()),
                                None => Ok((format!("LTSCALE = {}", h.linetype_scale), false)),
                            },
                            "CELTSCALE" => match &value {
                                Some(v) => v
                                    .parse::<f64>()
                                    .map(|x| {
                                        h.current_entity_linetype_scale = x;
                                        (format!("CELTSCALE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: numeric value required.".into()),
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
                            "DELOBJ" => match &value {
                                Some(v) => parse_bool(v)
                                    .map(|b| {
                                        h.delete_objects = b;
                                        (format!("DELOBJ = {}", b as i32), true)
                                    })
                                    .ok_or_else(|| "SETVAR: 0 or 1 required.".into()),
                                None => {
                                    Ok((format!("DELOBJ = {}", h.delete_objects as i32), false))
                                }
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
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.surface_u_density = x;
                                        (format!("SURFU = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("SURFU = {}", h.surface_u_density), false)),
                            },
                            "SURFV" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.surface_v_density = x;
                                        (format!("SURFV = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("SURFV = {}", h.surface_v_density), false)),
                            },
                            "SURFTYPE" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.surface_type = x;
                                        (format!("SURFTYPE = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
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
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.multiline_justification = x;
                                        (format!("CMLJUST = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => {
                                    Ok((format!("CMLJUST = {}", h.multiline_justification), false))
                                }
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
                            "XCLIPFRAME" => match &value {
                                Some(v) => v
                                    .parse::<i16>()
                                    .map(|x| {
                                        h.xclip_frame = x;
                                        (format!("XCLIPFRAME = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: integer value required.".into()),
                                None => Ok((format!("XCLIPFRAME = {}", h.xclip_frame), false)),
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
                                Some(v) => v
                                    .parse::<f64>()
                                    .map(|x| {
                                        h.sketch_increment = x;
                                        (format!("SKETCHINC = {x}"), true)
                                    })
                                    .map_err(|_| "SETVAR: numeric value required.".into()),
                                None => Ok((format!("SKETCHINC = {}", h.sketch_increment), false)),
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
                            _ => Err(format!("SETVAR: unknown variable \"{name}\".")),
                        }
                    };
                    match outcome {
                        Ok((msg, changed)) => {
                            if changed {
                                self.tabs[i].dirty = true;
                                self.command_line.push_output(&msg);
                            } else {
                                // Queried with no value (`changed == false`):
                                // prompt for a new value on the next line instead
                                // of only echoing the current one. Enter keeps it.
                                let current = msg.split('=').nth(1).map(str::trim).unwrap_or("");
                                self.command_line.push_output(crate::tf!(
                                    "Enter new value for {name} <{current}>:"
                                ).as_ref());
                                self.pending_setvar = Some(name.clone());
                            }
                        }
                        Err(e) => self.command_line.push_error(&e),
                    }
                    // TEXTFILL reset the glyph atlas; re-tessellate so text picks
                    // up the re-baked filled / hollow tiles.
                    if name == "TEXTFILL" {
                        self.tabs[i]
                            .scene
                            .invalidate_text_geometry_dependencies();
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
                            acadrust::EntityType::Text(t) if !t.style.is_empty() => {
                                styles.insert(t.style.clone());
                            }
                            acadrust::EntityType::MText(t) if !t.style.is_empty() => {
                                styles.insert(t.style.clone());
                            }
                            acadrust::EntityType::Insert(ins) => {
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
                self.command_line
                    .push_output(crate::t!("FINDNONPURGEABLE: named objects in use (not purgeable):").as_ref());
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
            "AUDIT" => {
                use std::collections::BTreeSet;
                let mut undefined_layers: BTreeSet<String> = BTreeSet::new();
                let mut undefined_blocks: BTreeSet<String> = BTreeSet::new();
                let mut total = 0usize;
                {
                    let doc = &self.tabs[i].scene.document;
                    for e in doc.entities() {
                        total += 1;
                        let layer = &e.common().layer;
                        if !layer.is_empty() && doc.layers.get(layer).is_none() {
                            undefined_layers.insert(layer.clone());
                        }
                        if let acadrust::EntityType::Insert(ins) = e {
                            if doc.block_records.get(&ins.block_name).is_none() {
                                undefined_blocks.insert(ins.block_name.clone());
                            }
                        }
                    }
                }
                self.command_line
                    .push_output(crate::tf!("AUDIT: scanned {total} object(s).").as_ref());
                if undefined_layers.is_empty() && undefined_blocks.is_empty() {
                    self.command_line.push_output(crate::t!("AUDIT: no issues found.").as_ref());
                } else {
                    if !undefined_layers.is_empty() {
                        self.command_line.push_error(crate::tf!(
                            "AUDIT: reference(s) to undefined layer(s): {}",
                            undefined_layers.into_iter().collect::<Vec<_>>().join(", ")
                        ).as_ref());
                    }
                    if !undefined_blocks.is_empty() {
                        self.command_line.push_error(crate::tf!(
                            "AUDIT: reference(s) to undefined block(s): {}",
                            undefined_blocks.into_iter().collect::<Vec<_>>().join(", ")
                        ).as_ref());
                    }
                    self.command_line
                        .push_info(crate::t!("AUDIT: report only — no automatic repair performed.").as_ref());
                }
            }

            // ── RENAME table entries ──────────────────────────────────────
            // Bare RENAME prompts step by step (type → old → new); the argument
            // form below both handles a fully-typed line and is the target the
            // stepped front-end dispatches to.
            "RENAME" => {
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
                    // Snapshot BEFORE mutating (undo must restore the old
                    // name); dropped again if nothing was renamed.
                    self.push_undo_snapshot(i, "RENAME");
                    let known = matches!(
                        type_str.as_str(),
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
                    let ok = if type_str == "BLOCK" {
                        // Re-keys the record, syncs the Block marker and every
                        // INSERT reference; refuses anonymous/xref sources and
                        // invalid/taken names.
                        self.tabs[i].scene.rename_block(&old_name, &new_name)
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
                        // Renamed references resolve at tessellation time —
                        // rebuild so nothing renders off a stale name.
                        self.tabs[i].scene.bump_geometry_no_blocks();
                        // The per-tab active layer tracks the name.
                        if type_str == "LAYER"
                            && self.tabs[i].active_layer.eq_ignore_ascii_case(&old_name)
                        {
                            self.tabs[i].active_layer = new_name.clone();
                        }
                        if type_str == "UCS" {
                            if let Some(active) = self.tabs[i].active_ucs.as_mut() {
                                if active.name.eq_ignore_ascii_case(&old_name) {
                                    active.name = new_name.clone();
                                    self.tabs[i].persist_active_ucs();
                                }
                            }
                        }
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::tf!("RENAME: '{}' → '{}'.", old_name, new_name).as_ref());
                    } else {
                        self.discard_last_undo_entry(i);
                        if !known {
                            self.command_line.push_error(crate::tf!("RENAME: unknown type '{}'. Use LAYER BLOCK STYLE DIMSTYLE LINETYPE UCS VIEW", type_str).as_ref());
                        } else if type_str == "BLOCK" {
                            self.command_line.push_error(crate::tf!(
                                "RENAME: could not rename block '{}' — missing/anonymous/xref, or '{}' is invalid or taken.",
                                old_name, new_name
                            ).as_ref());
                        } else {
                            self.command_line.push_error(crate::tf!(
                                "RENAME: could not rename '{}' in {} — not found or protected, or '{}' is invalid or taken.",
                                old_name, type_str, new_name
                            ).as_ref());
                        }
                    }
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
                    if self.tabs[i].scene.document.layers.contains(name_arg) {
                        self.tabs[i].scene.document.header.current_layer_name =
                            name_arg.to_string();
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::tf!("CLAYER set to \"{name_arg}\"").as_ref());
                    } else {
                        self.command_line
                            .push_error(crate::tf!("CLAYER: layer '{}' not found.", name_arg).as_ref());
                    }
                }
            }
            "CDIMSTY" | "DIMCURRENT" => {
                use crate::command::ValuePromptCommand;
                let c =
                    ValuePromptCommand::new("CDIMSTY", "CDIMSTY  new current dimension style name:");
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
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::tf!("Active dim style set to \"{name_arg}\"").as_ref());
                    } else {
                        self.command_line
                            .push_error(crate::tf!("CDIMSTY: dim style '{}' not found.", name_arg).as_ref());
                    }
                }
            }
            "LTSCALE" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new("LTSCALE", "LTSCALE  new global line-type scale:");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("LTSCALE ") => {
                let val_str = cmd.trim_start_matches("LTSCALE").trim();
                if val_str.is_empty() {
                    let v = self.tabs[i].scene.document.header.linetype_scale;
                    self.command_line.push_output(crate::tf!("LTSCALE = {v:.4}").as_ref());
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
                    self.command_line.push_error(crate::t!("Usage: LTSCALE [value]").as_ref());
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
                    self.command_line.push_output(crate::tf!("PDMODE = {v}").as_ref());
                } else if let Ok(v) = val_str.parse::<i16>() {
                    self.push_undo_snapshot(i, "PDMODE");
                    self.tabs[i].scene.document.header.point_display_mode = v;
                    // Point glyphs are built at tessellation time — rebuild them.
                    self.tabs[i].scene.invalidate_point_dependencies();
                    self.tabs[i].dirty = true;
                    self.command_line.push_output(crate::tf!("PDMODE set to {v}").as_ref());
                } else {
                    self.command_line.push_error(
                        "Usage: PDMODE [value]  (0=dot 1=none 2=+ 3=x 4=tick; +32 circle, +64 square)",
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
                let c =
                    ValuePromptCommand::new("ISAVEBAK", "ISAVEBAK  write a .bak on save?  [1 / 0]:");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("ISAVEBAK ") => {
                match cmd.trim_start_matches("ISAVEBAK").trim() {
                    "" => {
                        let v = if self.backup_on_save { 1 } else { 0 };
                        self.command_line.push_output(crate::tf!("ISAVEBAK = {v}").as_ref());
                    }
                    "0" => {
                        self.backup_on_save = false;
                        self.persist_settings_if_changed();
                        self.command_line.push_output(crate::t!("ISAVEBAK set to 0").as_ref());
                    }
                    "1" => {
                        self.backup_on_save = true;
                        self.persist_settings_if_changed();
                        self.command_line.push_output(crate::t!("ISAVEBAK set to 1").as_ref());
                    }
                    _ => self.command_line.push_error(crate::t!("Requires 0 or 1").as_ref()),
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
                        self.command_line.push_output(crate::tf!("FILEASSOC = {v}").as_ref());
                    }
                    "1" => {
                        self.file_assoc_enabled = true;
                        self.persist_settings_if_changed();
                        match crate::io::file_association::register_as_handler() {
                            Ok(()) => self.command_line.push_output(
                                "FILEASSOC set to 1 — registered as a .dwg/.dxf/.bak handler",
                            ),
                            Err(e) => self
                                .command_line
                                .push_error(crate::tf!("FILEASSOC: registration failed: {e}").as_ref()),
                        }
                    }
                    "0" => {
                        self.file_assoc_enabled = false;
                        self.persist_settings_if_changed();
                        match crate::io::file_association::unregister_handler() {
                            Ok(()) => self
                                .command_line
                                .push_output(crate::t!("FILEASSOC set to 0 — unregistered as a file handler").as_ref()),
                            Err(e) => self
                                .command_line
                                .push_error(crate::tf!("FILEASSOC: unregister failed: {e}").as_ref()),
                        }
                    }
                    _ => self.command_line.push_error(crate::t!("Requires 0 or 1").as_ref()),
                }
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
                            format!("SAVETIME set to {v} minute(s)")
                        };
                        self.command_line.push_output(&msg);
                    }
                    _ => self
                        .command_line
                        .push_error(crate::t!("Requires a non-negative number of minutes (0 = off)").as_ref()),
                }
            }
            "PDSIZE" => {
                use crate::command::ValuePromptCommand;
                let c = ValuePromptCommand::new(
                    "PDSIZE",
                    "PDSIZE  new point size (0 = 5% of viewport, <0 = absolute):",
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("PDSIZE ") => {
                let val_str = cmd.trim_start_matches("PDSIZE").trim();
                if val_str.is_empty() {
                    let v = self.tabs[i].scene.document.header.point_display_size;
                    self.command_line.push_output(crate::tf!("PDSIZE = {v:.4}").as_ref());
                } else if let Ok(v) = val_str.parse::<f64>() {
                    self.push_undo_snapshot(i, "PDSIZE");
                    self.tabs[i].scene.document.header.point_display_size = v;
                    self.tabs[i].scene.invalidate_point_dependencies();
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("PDSIZE set to {v:.4}").as_ref());
                } else {
                    self.command_line.push_error(
                        "Usage: PDSIZE [value]  (>0 absolute size, <0 percent of viewport, 0 default)",
                    );
                }
            }
            cmd if cmd == "DDPTYPE" => {
                // The dialog shows the magnitude; the sign (relative/absolute)
                // is driven by the radio buttons. A positive PDSIZE is absolute;
                // zero or negative is relative.
                let pdsize = self.tabs[i].scene.document.header.point_display_size;
                self.point_size_relative = pdsize <= 0.0;
                self.point_size_buf = format!("{}", pdsize.abs());
                self.active_modal = Some(super::super::ModalKind::PointStyle);
            }
            "LWDISPLAY" => {
                use crate::command::ValuePromptCommand;
                let c =
                    ValuePromptCommand::new("LWDISPLAY", "LWDISPLAY  show lineweights?  [ON / OFF]:");
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
                    Err(_) => self.command_line.push_error(crate::t!("Usage: LWDISPLAY [ON|OFF]").as_ref()),
                    Ok(Some(v)) => {
                        self.push_undo_snapshot(i, "LWDISPLAY");
                        self.tabs[i].scene.document.header.lineweight_display = v;
                        // No retessellate — the wire shader honours the flag via uniforms.
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::tf!("LWDISPLAY {}", if v { "ON" } else { "OFF" }).as_ref());
                    }
                    Ok(None) => {
                        let v = self.tabs[i].scene.document.header.lineweight_display;
                        self.command_line
                            .push_output(crate::tf!("LWDISPLAY = {}", if v { "ON" } else { "OFF" }).as_ref());
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
                    self.command_line.push_error(crate::t!("Usage: CELTSCALE [value]").as_ref());
                }
            }

            // ── SCALETEXT — rescale selected Text/MText entities ─────────────────
            // Usage: SCALETEXT <factor>   e.g. SCALETEXT 2
            //        SCALETEXT H <height>  set absolute height
            "SCALETEXT" => {
                use crate::command::SelectThenValueCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenValueCommand::new(
                    "SCALETEXT",
                    "SCALETEXT  scale factor (or 'H <height>' for an absolute height):",
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("SCALETEXT ") => {
                let rest = cmd.trim_start_matches("SCALETEXT").trim();
                let parts: Vec<&str> = rest.split_whitespace().collect();
                let selected_handles: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .collect();
                if selected_handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("SCALETEXT: select Text/MText entities first.").as_ref());
                } else {
                    let (use_absolute, value) = match (
                        parts.first().map(|s| s.to_uppercase()).as_deref(),
                        parts.get(1),
                    ) {
                        (Some("H"), Some(v)) => (true, v.parse::<f64>().ok()),
                        (Some(v), None) => (false, v.parse::<f64>().ok()),
                        _ => (false, None),
                    };
                    if let Some(val) = value {
                        if val <= 0.0 {
                            self.command_line
                                .push_error(crate::t!("SCALETEXT: value must be positive.").as_ref());
                        } else {
                            self.push_undo_snapshot(i, "SCALETEXT");
                            let mut count = 0usize;
                            for sh in &selected_handles {
                                for entity in self.tabs[i].scene.document.entities_mut() {
                                    if entity.common().handle != *sh {
                                        continue;
                                    }
                                    match entity {
                                        acadrust::EntityType::Text(t) => {
                                            t.height =
                                                if use_absolute { val } else { t.height * val };
                                            count += 1;
                                        }
                                        acadrust::EntityType::MText(t) => {
                                            t.height =
                                                if use_absolute { val } else { t.height * val };
                                            count += 1;
                                        }
                                        _ => {}
                                    }
                                    break;
                                }
                            }
                            if count > 0 {
                                self.tabs[i].dirty = true;
                                self.command_line.push_output(crate::tf!(
                                    "SCALETEXT: scaled {count} text entity(ies)."
                                ).as_ref());
                            } else {
                                self.command_line
                                    .push_error(crate::t!("SCALETEXT: no Text/MText in selection.").as_ref());
                            }
                        }
                    } else {
                        self.command_line
                            .push_info(crate::t!("Usage: SCALETEXT <factor>  or  SCALETEXT H <height>").as_ref());
                    }
                }
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }
}

/// RENAME for the name-keyed symbol tables (layer / text style / dim style /
/// linetype / UCS / view). `Table<T>` stores entries in a map keyed by
/// UPPERCASE name, so a rename must remove + re-add — mutating `.name` in
/// place leaves the map keyed by the old name and every later
/// `get(new_name)` lookup silently fails. Name-based references (entity
/// layer/linetype/style fields, layer linetypes, header current-* names) are
/// chased case-insensitively, matching how the tables resolve names.
fn rename_symbol(doc: &mut acadrust::CadDocument, ty: &str, old: &str, new: &str) -> bool {
    use acadrust::{EntityType, Table, TableEntry};

    fn rekey<T: TableEntry>(table: &mut Table<T>, old: &str, new: &str) -> bool {
        if !crate::scene::valid_block_name(new) {
            return false;
        }
        // A case-only rename reuses its own key; any other target must be free.
        if !old.eq_ignore_ascii_case(new) && table.contains(new) {
            return false;
        }
        let Some(mut e) = table.remove(old) else {
            return false;
        };
        e.set_name(new.to_string());
        let _ = table.add(e);
        true
    }

    match ty {
        "LAYER" => {
            // Layer 0 and Defpoints are fixed names; xref-dependent ("|")
            // layers belong to the referenced file.
            if old == "0" || old.eq_ignore_ascii_case("Defpoints") || old.contains('|') {
                return false;
            }
            if !rekey(&mut doc.layers, old, new) {
                return false;
            }
            for e in doc.entities_mut() {
                if e.common().layer.eq_ignore_ascii_case(old) {
                    e.common_mut().layer = new.to_string();
                }
            }
            if doc.header.current_layer_name.eq_ignore_ascii_case(old) {
                doc.header.current_layer_name = new.to_string();
            }
            true
        }
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
            true
        }
        "DIMSTYLE" => {
            if !rekey(&mut doc.dim_styles, old, new) {
                return false;
            }
            for e in doc.entities_mut() {
                match e {
                    EntityType::Dimension(d)
                        if d.base().style_name.eq_ignore_ascii_case(old) =>
                    {
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
            if ["BYLAYER", "BYBLOCK", "CONTINUOUS"]
                .contains(&old.to_uppercase().as_str())
            {
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
