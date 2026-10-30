use super::*;

fn selected_edge_body(app: &OpenCADStudio, tab: usize) -> Option<codec::Handle> {
    let scene = &app.tabs.get(tab)?.scene;
    let selected = scene.selected_handles_in_order();
    let [handle] = selected.as_slice() else {
        return None;
    };

    if matches!(
        scene.document.get_entity(*handle),
        Some(codec::EntityType::Solid3D(_) | codec::EntityType::Surface(_))
    ) {
        Some(*handle)
    } else {
        None
    }
}

fn solid_edge_sources(
    app: &mut OpenCADStudio,
    tab: usize,
) -> Vec<(codec::Handle, kernel::brep::Body)> {
    let handles = app.tabs[tab]
        .scene
        .document
        .entities()
        .filter_map(|entity| {
            matches!(
                entity,
                codec::EntityType::Solid3D(_) | codec::EntityType::Surface(_)
            )
            .then_some(entity.common().handle)
        })
        .collect::<Vec<_>>();
    app.tabs[tab].scene.restore_solid_models(&handles);
    handles
        .into_iter()
        .filter_map(|handle| {
            app.tabs[tab]
                .scene
                .solid_models
                .get(&handle)
                .cloned()
                .map(|body| (handle, body))
        })
        .collect()
}

impl OpenCADStudio {
    pub(super) fn dispatch_dim(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            "DIMALIGNED" => {
                use crate::modules::annotate::aligned_dim::AlignedDimensionCommand;
                let cmd = AlignedDimensionCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMDIAMETER" => {
                use crate::modules::annotate::diameter_dim::DiameterDimensionCommand;
                let cmd = DiameterDimensionCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMLINEAR" => {
                use crate::modules::annotate::linear_dim::LinearDimensionCommand;
                let new_cmd = LinearDimensionCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "DIMRADIUS" => {
                use crate::modules::annotate::radius_dim::RadiusDimensionCommand;
                let new_cmd = RadiusDimensionCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "DIMJOGGED" | "DIMJOG" => {
                use crate::modules::annotate::jogged_radius_dim::JoggedRadiusDimensionCommand;
                let defaults = crate::scene::creation_style::current_dimension_defaults(
                    &self.tabs[i].scene.document,
                );
                let multiplier = self.tabs[i].scene.creation_annotation_multiplier();
                let new_cmd = JoggedRadiusDimensionCommand::new(defaults, multiplier);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "DIMANGULAR" => {
                use crate::modules::annotate::angular_dim::AngularDimensionCommand;
                let new_cmd = AngularDimensionCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "DIMARC" => {
                use crate::modules::annotate::arc_length_dim::ArcLengthDimensionCommand;
                let new_cmd = ArcLengthDimensionCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "DIMORDINATE" => {
                use crate::modules::annotate::ordinate_dim::OrdinateDimCommand;
                let new_cmd = OrdinateDimCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            // LEADER draws a leader line with an annotation; QLEADER is the same
            // quick leader-plus-annotation operation.
            "LEADER" | "QLEADER" => {
                use crate::modules::annotate::leader_cmd::LeaderCommand;
                let defaults = crate::scene::creation_style::current_dimension_defaults(
                    &self.tabs[i].scene.document,
                );
                let multiplier = self.tabs[i].scene.creation_annotation_multiplier();
                let new_cmd = LeaderCommand::with_defaults(defaults, multiplier);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            // TCASE <Upper|Lower|Sentence|Title> — change the case of the text in
            // selected text and multiline-text objects.
            "TCASE" => {
                use crate::command::SelectThenKeywordCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenKeywordCommand::new(
                    "TCASE",
                    "TCASE  [Upper / Lower / Sentence / Title]:",
                    vec![
                        ("Upper", "UPPER", None),
                        ("Lower", "LOWER", None),
                        ("Sentence", "SENTENCE", None),
                        ("Title", "TITLE", None),
                    ],
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("TCASE ") => {
                let opt = cmd.split_whitespace().nth(1).unwrap_or("").to_uppercase();
                let handles: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("TCASE: select text objects first.").as_ref());
                    return Some(Task::none());
                }
                if !matches!(
                    opt.as_str(),
                    "U" | "UPPER"
                        | "UPPERCASE"
                        | "L"
                        | "LOWER"
                        | "LOWERCASE"
                        | "S"
                        | "SENTENCE"
                        | "T"
                        | "TITLE"
                ) {
                    self.command_line
                        .push_info(crate::t!("Usage: TCASE <Upper|Lower|Sentence|Title>").as_ref());
                    return Some(Task::none());
                }
                let conv = move |s: &str| -> String {
                    match opt.as_str() {
                        "U" | "UPPER" | "UPPERCASE" => s.to_uppercase(),
                        "L" | "LOWER" | "LOWERCASE" => s.to_lowercase(),
                        "S" | "SENTENCE" => sentence_case(s),
                        _ => title_case(s),
                    }
                };
                self.push_undo_snapshot(i, "TCASE");
                let mut n = 0usize;
                for h in &handles {
                    if let Some(e) = self.tabs[i]
                        .scene
                        .document
                        .entities_mut()
                        .find(|e| e.common().handle == *h)
                    {
                        match e {
                            codec::EntityType::Text(t) => {
                                t.value = conv(&t.value);
                                n += 1;
                            }
                            codec::EntityType::MText(m) => {
                                m.value = conv(&m.value);
                                n += 1;
                            }
                            _ => {}
                        }
                    }
                }
                self.tabs[i].dirty = true;
                let changes: Vec<_> = handles
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                self.tabs[i].scene.bump_entities(&changes);
                self.command_line
                    .push_output(crate::tf!("TCASE: updated {n} text object(s).").as_ref());
            }

            // TEXTMASK — place a wipeout mask sized to each selected text object's
            // extent and bring the text in front, so the mask hides whatever is
            // underneath while the text stays readable. (Same world-XY coordinate
            // space the WIPEOUT command uses.)
            "TEXTMASK" => {
                use crate::command::SelectThenValueCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenValueCommand::new(
                    "TEXTMASK",
                    "TEXTMASK  press Enter to mask the selected text:",
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("TEXTMASK ") => {
                let handles: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .filter(|(_, e)| {
                        matches!(e, codec::EntityType::Text(_) | codec::EntityType::MText(_))
                    })
                    .map(|(h, _)| *h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("TEXTMASK: select text objects first.").as_ref());
                    return Some(Task::none());
                }
                self.push_undo_snapshot(i, "TEXTMASK");
                let mut n = 0usize;
                for h in &handles {
                    let wires = self.tabs[i].scene.wire_models_for(&[*h]);
                    let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
                    for w in &wires {
                        for p in &w.points {
                            for k in 0..3 {
                                min[k] = min[k].min(p[k]);
                                max[k] = max[k].max(p[k]);
                            }
                        }
                    }
                    if min[0] > max[0] {
                        continue; // no geometry for this object
                    }
                    let pad = ((max[1] - min[1]) * 0.15).max(0.0);
                    let c1 = codec::types::Vector3::new(
                        (min[0] - pad) as f64,
                        (min[1] - pad) as f64,
                        min[2] as f64,
                    );
                    let c2 = codec::types::Vector3::new(
                        (max[0] + pad) as f64,
                        (max[1] + pad) as f64,
                        min[2] as f64,
                    );
                    self.tabs[i]
                        .scene
                        .add_entity_clone(codec::EntityType::Wipeout(
                            codec::entities::Wipeout::from_corners(c1, c2),
                        ));
                    n += 1;
                }
                // Draw the text in front of its newly-added masks.
                self.tabs[i]
                    .scene
                    .replace_selection(handles.iter().cloned().collect());
                self.tabs[i].dirty = true;
                self.command_line
                    .push_output(crate::tf!("TEXTMASK: masked {n} text object(s).").as_ref());
                return self.dispatch_view("DRAWORDER FRONT", i);
            }

            // (text width fit)
            // TEXTFIT <width> — adjust the width factor of selected single-line
            // text so its rendered width matches the target.
            "TEXTFIT" => {
                use crate::command::SelectThenValueCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenValueCommand::new("TEXTFIT", "TEXTFIT  target width:", has_sel);
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("TEXTFIT ") => {
                let target: f64 = match cmd.split_whitespace().nth(1).and_then(|s| s.parse().ok()) {
                    Some(v) if v > 0.0 => v,
                    _ => {
                        self.command_line.push_info(
                            crate::t!("Usage: TEXTFIT <target width>   (fits selected text to that width)").as_ref(),
                        );
                        return Some(Task::none());
                    }
                };
                let handles: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .filter(|(_, e)| matches!(e, codec::EntityType::Text(_)))
                    .map(|(h, _)| *h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("TEXTFIT: select single-line text first.").as_ref());
                    return Some(Task::none());
                }
                self.push_undo_snapshot(i, "TEXTFIT");
                let mut n = 0usize;
                for h in &handles {
                    let wires = self.tabs[i].scene.wire_models_for(&[*h]);
                    let (mut minx, mut maxx) = (f32::MAX, f32::MIN);
                    for w in &wires {
                        for p in &w.points {
                            minx = minx.min(p[0]);
                            maxx = maxx.max(p[0]);
                        }
                    }
                    let cur_w = (maxx - minx) as f64;
                    if cur_w <= 1e-9 {
                        continue;
                    }
                    if let Some(codec::EntityType::Text(t)) = self.tabs[i]
                        .scene
                        .document
                        .entities_mut()
                        .find(|e| e.common().handle == *h)
                    {
                        let old = if t.width_factor.abs() > 1e-9 {
                            t.width_factor
                        } else {
                            1.0
                        };
                        t.width_factor = old * target / cur_w;
                        n += 1;
                    }
                }
                self.tabs[i].dirty = true;
                let changes: Vec<_> = handles
                    .into_iter()
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                self.tabs[i].scene.bump_entities(&changes);
                self.command_line.push_output(
                    crate::tf!("TEXTFIT: fitted {n} text object(s) to width {target}.").as_ref(),
                );
            }

            // (text sequential numbering)
            // TCOUNT [start] — prefix selected single-line text with sequential
            // numbers in reading order (top-to-bottom, then left-to-right).
            "TCOUNT" => {
                use crate::command::TCountCommand;

                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = TCountCommand::new(has_sel);

                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("TCOUNT ") => {
                let mut args = cmd.split_whitespace().skip(1);

                let start: i64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);

                let increment: i64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);

                let placement = args.next().unwrap_or("O").to_uppercase();

                let placement = match placement.as_str() {
                    "O" | "OVERWRITE" => "O",
                    "P" | "PREFIX" => "P",
                    "S" | "SUFFIX" => "S",
                    _ => {
                        self.command_line.push_error(
                            crate::t!("TCOUNT: placement must be Overwrite, Prefix, or Suffix.")
                                .as_ref(),
                        );
                        return Some(Task::none());
                    }
                };

                let mut texts: Vec<(codec::Handle, f64, f64)> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .filter_map(|(h, e)| match e {
                        codec::EntityType::Text(t) if !self.tabs[i].scene.is_layer_locked(*h) => {
                            Some((*h, t.insertion_point.x, t.insertion_point.y))
                        }
                        _ => None,
                    })
                    .collect();

                if texts.is_empty() {
                    self.command_line
                        .push_error(crate::t!("TCOUNT: select single-line text first.").as_ref());
                    return Some(Task::none());
                }

                // Keep the existing reading order:
                // higher Y first, then smaller X.
                texts.sort_by(|a, b| {
                    b.2.partial_cmp(&a.2)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                });

                self.push_undo_snapshot(i, "TCOUNT");

                let mut num = start;

                for (h, _, _) in &texts {
                    if let Some(codec::EntityType::Text(t)) = self.tabs[i]
                        .scene
                        .document
                        .entities_mut()
                        .find(|e| e.common().handle == *h)
                    {
                        t.value = match placement {
                            "P" => format!("{num} {}", t.value),
                            "S" => format!("{} {num}", t.value),
                            _ => num.to_string(),
                        };

                        num += increment;
                    }
                }

                self.tabs[i].dirty = true;

                let changes: Vec<_> = texts
                    .iter()
                    .map(|(handle, _, _)| (*handle, crate::scene::ChangeKind::Modified))
                    .collect();

                self.tabs[i].scene.bump_entities(&changes);

                let placement_name = crate::t!(match placement {
                    "P" => "Prefix",
                    "S" => "Suffix",
                    _ => "Overwrite",
                });
                let n = texts.len();
                self.command_line.push_output(
                    crate::tf!(
                        "TCOUNT: numbered {n} text object(s) from {start} by {increment} ({placement_name})."
                    )
                    .as_ref(),
                );
            }

            "MLEADER" => {
                use crate::modules::annotate::mleader_cmd::MLeaderCommand;
                let name = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_mleader_style_name
                    .clone();
                let style =
                    self.tabs[i]
                        .scene
                        .document
                        .objects
                        .iter()
                        .find_map(|(handle, object)| match object {
                            codec::objects::ObjectType::MultiLeaderStyle(style)
                                if style.name.eq_ignore_ascii_case(&name) =>
                            {
                                let mut style = style.clone();
                                style.handle = *handle;
                                Some(style)
                            }
                            _ => None,
                        });
                let multiplier = self.tabs[i].scene.creation_annotation_multiplier();
                let block_sources = self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .iter()
                    .filter(|block| !block.name.is_empty() && !block.name.starts_with('*'))
                    .map(|block| (block.name.clone(), block.handle))
                    .collect();
                let layers = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
                    .map(|layer| layer.name.clone())
                    .collect();
                let text_styles = self.tabs[i]
                    .scene
                    .document
                    .text_styles
                    .iter()
                    .map(|style| (style.name.clone(), style.handle))
                    .collect();
                let new_cmd = style
                    .map_or_else(MLeaderCommand::new, |style| {
                        MLeaderCommand::with_style(style, multiplier)
                    })
                    .with_drawing_resources(block_sources, layers, text_styles)
                    .with_layer(self.mleader_layer(i));
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "TOLERANCE" => {
                self.open_tolerance_dialog(None);
            }

            "TABLE" => {
                self.open_table_insert();
            }

            "-TABLE" => {
                self.start_command_line_table();
            }

            cmd if cmd.starts_with("-TABLE _LINK ") => {
                self.place_command_line_link_table(&cmd["-TABLE _LINK ".len()..]);
            }

            "TABLEDIT" => {
                use crate::modules::annotate::table_cmd::TableditCommand;
                let cmd = TableditCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMCONTINUE" => {
                let cmd = self.dim_continue_command(i, false);
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMBASELINE" => {
                let cmd = self.dim_baseline_command(i, false);
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "QDIM" => {
                use crate::modules::annotate::qdim::QdimCommand;
                let document = &self.tabs[i].scene.document;
                let dim_spacing = document
                    .dim_styles
                    .iter()
                    .find(|style| {
                        style
                            .name
                            .eq_ignore_ascii_case(&document.header.current_dimstyle_name)
                    })
                    .map(|style| style.dimdli)
                    .unwrap_or_else(|| {
                        if document.header.measurement == 1 {
                            3.75
                        } else {
                            0.38
                        }
                    });
                let selection = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(handle, entity)| crate::command::SelectionEntity {
                        handle,
                        entity: entity.clone(),
                        surface_area: None,
                    })
                    .collect();
                let cmd =
                    QdimCommand::new(selection, dim_spacing, self.quick_dimension_snap_priority);
                let priority = if self.quick_dimension_snap_priority == 1 {
                    crate::t!("Associative dimension priority = Intersection")
                } else {
                    crate::t!("Associative dimension priority = Endpoint")
                };
                self.command_line.push_output(&priority);
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMEDIT" => {
                use crate::modules::annotate::dimedit::DimEditCommand;
                let cmd = DimEditCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            // The Dimensions panel's text buttons: pick, then the option.
            "DIMTEDIT_ANGLE" | "DIMTEDIT_LEFT" | "DIMTEDIT_CENTER" | "DIMTEDIT_RIGHT" => {
                use crate::modules::annotate::dimtedit::DimTeditCommand;
                let option = match cmd {
                    "DIMTEDIT_ANGLE" => "A",
                    "DIMTEDIT_LEFT" => "L",
                    "DIMTEDIT_CENTER" => "C",
                    _ => "R",
                };
                let cmd = DimTeditCommand::with_option(option);
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMTEDIT" | "DIMTED" => {
                use crate::modules::annotate::dimtedit::DimTeditCommand;
                let cmd = DimTeditCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMBREAK" => {
                use crate::modules::annotate::dimbreak::DimBreakCommand;
                let cmd = DimBreakCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMSPACE" | "DSPACE" => {
                use crate::modules::annotate::dimspace::DimSpaceCommand;
                let cmd = DimSpaceCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMJOGLINE" => {
                use crate::modules::annotate::dimjogline::DimJogLineCommand;
                let cmd = DimJogLineCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "MLEADEREDIT" | "AIMLEADEREDITADD" | "AIMLEADEREDITREMOVE" => {
                use crate::modules::annotate::mleader_edit::MLeaderEditCommand;
                let name = match cmd {
                    "AIMLEADEREDITADD" => "AIMLEADEREDITADD",
                    "AIMLEADEREDITREMOVE" => "AIMLEADEREDITREMOVE",
                    _ => "MLEADEREDIT",
                };
                let mut cmd = MLeaderEditCommand::new(name);
                // A single preselected multileader is the one edited.
                let selected = self.tabs[i].scene.selected_entities();
                if let [(handle, codec::EntityType::MultiLeader(ml))] = selected.as_slice() {
                    cmd = cmd.with_target(*handle, (**ml).clone());
                    self.tabs[i].scene.replace_selection(Default::default());
                }
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "MLEADERALIGN" => {
                use crate::modules::annotate::mleader_edit::MLeaderAlignCommand;
                let cmd = MLeaderAlignCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "MLEADERCOLLECT" => {
                use crate::modules::annotate::mleader_edit::MLeaderCollectCommand;
                let cmd = MLeaderCollectCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "ZOOM EXTENTS ALL" | "ZOOM EXTENTS ALL VIEWPORTS" | "ZOOM EA" | "ZEA" => {
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.fit_all_model_viewports();
                self.command_line
                    .push_output(crate::t!("Zoom Extents — All Viewports").as_ref());
            }

            "ZOOM EXTENTS" | "ZOOM E" | "ZOOMEXTENTS" | "ZE" => {
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.fit_all();
                self.command_line
                    .push_output(crate::t!("Zoom Extents").as_ref());
            }

            "ZOOM IN" | "ZOOM I" | "ZI" => {
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.zoom_camera(1.0 / 1.5);
                self.command_line.push_output(crate::t!("Zoom In").as_ref());
            }

            "ZOOM OUT" | "ZO" => {
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.zoom_camera(1.5);
                self.command_line
                    .push_output(crate::t!("Zoom Out").as_ref());
            }

            // ZOOM ALL — fit the configured drawing limits.
            "ZOOM ALL" | "ZOOM A" | "ZA" => {
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.fit_all_with_limits();
                self.command_line
                    .push_output(crate::t!("Zoom All").as_ref());
            }

            "ZOOM PREVIOUS" | "ZOOM P" | "ZP" => {
                if self.tabs[i].scene.restore_previous_view() {
                    self.command_line
                        .push_output(crate::t!("Zoom Previous").as_ref());
                } else {
                    self.command_line
                        .push_error(crate::t!("ZOOM: no previous view.").as_ref());
                }
            }

            "ZOOM OBJECT" | "ZOOM O" | "ZOBJ" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(handle, _)| handle)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let command = SelectObjectsCommand::new("ZOOM OBJECT");
                    self.command_line.push_info(&command.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(command));
                } else {
                    self.tabs[i].scene.remember_current_view();
                    if self.tabs[i].scene.zoom_to_entities(&handles) {
                        self.command_line
                            .push_output(crate::t!("Zoom Object").as_ref());
                    } else {
                        self.command_line.push_error(
                            crate::t!("ZOOM: selected objects have no visible bounds.").as_ref(),
                        );
                    }
                }
            }

            "ZOOM DYNAMIC" | "ZOOM D" | "ZD" => {
                self.tabs[i].zoom_dynamic_mode = true;
                self.clear_navigation_hover(i);
                self.command_line.push_output(
                    crate::t!(
                        "ZOOM Dynamic: drag horizontally to pan and vertically to zoom. Press Esc to exit."
                    )
                    .as_ref(),
                );
            }

            // ZOOM SCALE — set zoom factor (e.g. "ZOOM SCALE 2" or "ZS 0.5")
            cmd if cmd.starts_with("ZOOM SCALE ")
                || cmd.starts_with("ZOOM S ")
                || cmd.starts_with("ZS ") =>
            {
                let rest = cmd
                    .strip_prefix("ZOOM SCALE ")
                    .or_else(|| cmd.strip_prefix("ZOOM S "))
                    .or_else(|| cmd.strip_prefix("ZS "))
                    .unwrap_or("1");
                // nXP scales the active viewport against paper space, nX (or
                // a bare n) the current view (#1465). n may be an expression
                // such as 1/50.
                let rest = rest.trim().to_ascii_uppercase();
                let (number, xp) = match rest.strip_suffix("XP") {
                    Some(n) => (n, true),
                    None => (rest.strip_suffix('X').unwrap_or(&rest), false),
                };
                let factor = crate::app::expr_eval::eval_number(number.trim())
                    .filter(|f| f.is_finite() && *f > 0.0);
                match factor {
                    Some(factor) if xp && self.tabs[i].scene.set_viewport_scale_factor(factor) => {
                        self.tabs[i].dirty = true;
                        self.command_line
                            .push_output(crate::tf!("Zoom Scale ×{factor:.3}").as_ref());
                    }
                    Some(factor) => {
                        self.tabs[i].scene.remember_current_view();
                        self.tabs[i].scene.zoom_camera(1.0 / factor as f32);
                        self.command_line
                            .push_output(crate::tf!("Zoom Scale ×{factor:.3}").as_ref());
                    }
                    None => {}
                }
            }

            "PLOTWINDOW" => {
                use crate::modules::view::plot_window::PlotWindowCommand;
                let cmd = PlotWindowCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "QUICKPRINT" => {
                use crate::modules::view::quick_print::QuickPrintCommand;
                let cmd = QuickPrintCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DVIEW" => {
                use crate::modules::view::dview::DviewCommand;
                let twist = {
                    let scene = &self.tabs[i].scene;
                    let roll = match scene.active_viewport {
                        Some(_) => scene.navigation_camera().roll(),
                        None => scene.camera.borrow().roll(),
                    };
                    -(roll as f64)
                };
                let cmd = DviewCommand::new(twist.rem_euclid(std::f64::consts::TAU));
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "ZOOM" | "ZOOM WINDOW" | "ZOOM W" | "ZW" => {
                use crate::modules::view::zoom_window::ZoomWindowCommand;
                let new_cmd = ZoomWindowCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "STRETCH" => {
                // With a prior selection the window only marks which points
                // move. With none, the crossing window itself selects the
                // objects — one round instead of a separate object-selection
                // step (#338); the command emits StretchWindow for the host
                // to resolve.
                use crate::modules::draw::modify::stretch::StretchCommand;
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                let wires = if handles.is_empty() {
                    Vec::new()
                } else {
                    self.tabs[i].scene.wire_models_for(&handles)
                };
                // Selected by a crossing window just before: stretch through
                // that window, no second one asked for (#1068). Every picked
                // object must touch it, or the selection was changed since.
                let window = self.tabs[i].last_crossing_window.and_then(|(lo, hi)| {
                    let touches = |wire: &crate::scene::WireModel| {
                        let [x0, y0, x1, y1] = wire.aabb.map(f64::from);
                        x0 <= hi[0] && x1 >= lo[0] && y0 <= hi[1] && y1 >= lo[1]
                    };
                    (!handles.is_empty()
                        && handles.iter().all(|h| {
                            self.tabs[i]
                                .scene
                                .wire_models_for(&[*h])
                                .iter()
                                .any(touches)
                        }))
                    .then(|| {
                        (
                            glam::DVec3::new(lo[0], lo[1], f64::NEG_INFINITY),
                            glam::DVec3::new(hi[0], hi[1], f64::INFINITY),
                        )
                    })
                });
                let new_cmd = match window {
                    Some(window) => StretchCommand::preselected(handles, wires, window),
                    None => StretchCommand::new(handles, wires),
                };
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "FILLETEDGE" | "SOLIDFILLET" => {
                use crate::modules::model::edge_cmd::{EdgeOperation, SolidEdgeCommand};
                let target = selected_edge_body(self, i);
                let bodies = solid_edge_sources(self, i);
                let command = SolidEdgeCommand::new(
                    EdgeOperation::Fillet,
                    target,
                    bodies,
                    crate::scene::WireModel::SELECTED,
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            "FILLET" if selected_edge_body(self, i).is_some() => {
                use crate::modules::model::edge_cmd::{EdgeOperation, SolidEdgeCommand};
                let target = selected_edge_body(self, i);
                let bodies = solid_edge_sources(self, i);
                let command = SolidEdgeCommand::new(
                    EdgeOperation::Fillet,
                    target,
                    bodies,
                    crate::scene::WireModel::SELECTED,
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            "FILLET" => {
                use crate::modules::draw::modify::fillet::FilletCommand;
                let entities: Vec<_> = self.tabs[i].scene.visible_entities_for_edit(|_| true);
                let all_entities: Vec<_> = entities.into_iter().map(|(_, e)| e).collect();
                let new_cmd = FilletCommand::new(
                    crate::modules::draw::defaults::get_fillet_radius(),
                    all_entities,
                );
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "BLEND" | "BLE" => {
                use crate::modules::draw::modify::blend::BlendCommand;
                let all_entities: Vec<_> = self.tabs[i]
                    .scene
                    .visible_entities_for_edit(|_| true)
                    .into_iter()
                    .map(|(_, e)| e)
                    .collect();
                let new_cmd = BlendCommand::new(all_entities);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "ARRAY" | "ARRAYRECT" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("ARRAYRECT");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::array::ArrayRectCommand;
                    let wires = self.tabs[i].scene.wire_models_for(&handles);
                    let new_cmd = ArrayRectCommand::new(handles, wires);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            "ARRAYPOLAR" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("ARRAYPOLAR");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::array::ArrayPolarCommand;
                    let wires = self.tabs[i].scene.wire_models_for(&handles);
                    let new_cmd = ArrayPolarCommand::new(handles, wires);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            "ARRAYPATH" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("ARRAYPATH");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::array::ArrayPathCommand;
                    let wires = self.tabs[i].scene.wire_models_for(&handles);
                    let all_entities: Vec<_> = self.tabs[i]
                        .scene
                        .visible_entities_for_edit(|_| true)
                        .into_iter()
                        .map(|(_, e)| e)
                        .collect();
                    let new_cmd = ArrayPathCommand::new(handles, wires, all_entities);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            "ARRAY3D" | "3DARRAY" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("ARRAY3D");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::array::Array3DCommand;
                    let new_cmd = Array3DCommand::new(handles);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            "CHAMFEREDGE" | "SOLIDCHAMFER" => {
                use crate::modules::model::edge_cmd::SolidEdgeCommand;
                let target = selected_edge_body(self, i);
                let header = &self.tabs[i].scene.document.header;
                let distances = (header.chamfer_distance_a, header.chamfer_distance_b);
                let bodies = solid_edge_sources(self, i);
                let command = SolidEdgeCommand::new_chamfer(
                    target,
                    bodies,
                    crate::scene::WireModel::SELECTED,
                    distances,
                );
                let distances = command.chamfer_distances();
                self.command_line.push_output(
                    crate::tf!(
                        "Distance1 = {:.4}, Distance2 = {:.4}",
                        distances.0,
                        distances.1
                    )
                    .as_ref(),
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            "CHAMFER" if selected_edge_body(self, i).is_some() => {
                use crate::modules::model::edge_cmd::SolidEdgeCommand;
                let target = selected_edge_body(self, i);
                let header = &self.tabs[i].scene.document.header;
                let distances = (header.chamfer_distance_a, header.chamfer_distance_b);
                let bodies = solid_edge_sources(self, i);
                let command = SolidEdgeCommand::new_chamfer(
                    target,
                    bodies,
                    crate::scene::WireModel::SELECTED,
                    distances,
                );
                let distances = command.chamfer_distances();
                self.command_line.push_output(
                    crate::tf!(
                        "Distance1 = {:.4}, Distance2 = {:.4}",
                        distances.0,
                        distances.1
                    )
                    .as_ref(),
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            "CHAMFER" => {
                use crate::modules::draw::modify::fillet::ChamferCommand;
                let entities: Vec<_> = self.tabs[i].scene.visible_entities_for_edit(|_| true);
                let all_entities: Vec<_> = entities.into_iter().map(|(_, e)| e).collect();
                let new_cmd = ChamferCommand::new(
                    crate::modules::draw::defaults::get_chamfer_dist1(),
                    all_entities,
                );
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "EXPLODE" => {
                use crate::modules::draw::modify::explode::{
                    apply_explode_replacements, plan_explode_in_context,
                };
                let selected: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .filter(|(handle, _)| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if selected.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("EXPLODE");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    // EXPLMODE 0 keeps references that are not uniformly and
                    // positively scaled whole.
                    let selected: Vec<_> =
                        if super::blockvars::explmode(&self.tabs[i].scene.document) == 0 {
                            selected
                                .into_iter()
                                .filter(|(_, entity)| match entity {
                                    codec::EntityType::Insert(ins) => {
                                        let (x, y, z) =
                                            (ins.x_scale(), ins.y_scale(), ins.z_scale());
                                        let uniform = x > 0.0
                                            && (x - y).abs() <= 1e-10
                                            && (x - z).abs() <= 1e-10;
                                        if !uniform {
                                            self.command_line.push_output(
                                            "The object is not uniformly and positively scaled.",
                                        );
                                        }
                                        uniform
                                    }
                                    _ => true,
                                })
                                .collect()
                        } else {
                            selected
                        };
                    let replacements = plan_explode_in_context(
                        &selected,
                        &self.tabs[i].scene.document,
                        self.tabs[i].scene.displayed_annotation_scale_handle(),
                    );
                    let exploded = replacements.len();
                    if exploded > 0 {
                        self.push_undo_snapshot(i, "EXPLODE");
                    }
                    apply_explode_replacements(&mut self.tabs[i].scene, replacements);
                    if exploded > 0 {
                        self.tabs[i].dirty = true;
                        self.refresh_properties();
                        self.command_line
                            .push_output(crate::tf!("{exploded} object(s) exploded.").as_ref());
                    } else {
                        self.command_line.push_info(
                            crate::t!("EXPLODE: no explodable objects selected.").as_ref(),
                        );
                    }
                }
            }

            // TXTEXP — text becomes the polylines of its character outlines.
            "TXTEXP" => {
                let selected: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .filter(|(handle, _)| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if selected.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("TXTEXP");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::explode::apply_explode_replacements;
                    use crate::modules::draw::modify::txtexp::plan_text_explode;
                    let replacements = plan_text_explode(&selected, &self.tabs[i].scene.document);
                    let exploded = replacements.len();
                    if exploded > 0 {
                        self.push_undo_snapshot(i, "TXTEXP");
                        apply_explode_replacements(&mut self.tabs[i].scene, replacements);
                        self.tabs[i].dirty = true;
                        self.refresh_properties();
                        self.command_line
                            .push_output(crate::tf!("{exploded} object(s) exploded.").as_ref());
                    } else {
                        self.command_line
                            .push_info(crate::t!("TXTEXP: no text selected.").as_ref());
                    }
                }
            }

            // TXT2MTXT — TEXT / MTEXT combined into MTEXT. A preselection is
            // used at once; otherwise the selection prompt with SEttings.
            "TXT2MTXT" => {
                if !self.tabs[i].scene.selected.is_empty() {
                    return Some(self.dispatch_command("_TXT2MTXTSEL"));
                }
                use crate::modules::draw::select::SelectObjectsCommand;
                self.command_line
                    .push_output(crate::t!("Select text objects to combine ...").as_ref());
                let command = SelectObjectsCommand::with_settings(
                    "TXT2MTXT",
                    "_TXT2MTXTSEL",
                    "Select objects or [SEttings]:",
                    || crate::command::CmdResult::OpenTxt2MtxtSettings,
                );
                self.command_line.push_info(&command.prompt());
                self.command_line.set_step_options(command.options());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            "_TXT2MTXTSEL" => {
                self.apply_txt2mtxt(i);
                return Some(self.finish_dispatch("TXT2MTXT"));
            }

            "OFFSET" => {
                use crate::modules::draw::modify::offset::{is_offsettable, OffsetCommand};
                let all_entities: Vec<_> = self.tabs[i]
                    .scene
                    .visible_entities_for_edit(|_| true)
                    .into_iter()
                    .map(|(_, e)| e)
                    .collect();
                // Pick-first (#422): with offsettable objects already selected,
                // skip the distance and pick steps and open on the live side
                // preview.
                let preselected: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(_, e)| e.clone())
                    .filter(is_offsettable)
                    .collect();
                let new_cmd = if preselected.is_empty() {
                    OffsetCommand::new(all_entities)
                } else {
                    OffsetCommand::with_selection(all_entities, preselected)
                };
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "TRIM" => {
                use crate::modules::draw::modify::trim::{is_trim_boundary_entity, TrimCommand};
                // Only curves cut or get cut: text and the like stay out of
                // the working copy (#1978), and a block reference cuts only
                // through its contents, added below (#2192). HATCH fills have
                // their own renderer, so visible top-level hatches are added
                // for the hatch trim solver. A multiline is trimmed but never
                // cuts (#2068).
                let mut entities = self.tabs[i].scene.visible_entities_for_edit(|entity| {
                    is_trim_boundary_entity(entity) || matches!(entity, codec::EntityType::MLine(_))
                });
                let mut seen: std::collections::HashSet<codec::Handle> =
                    entities.iter().map(|(handle, _)| *handle).collect();
                for handle in self.tabs[i].scene.visible_hatch_handles_for_edit() {
                    if seen.insert(handle) {
                        if let Some(entity) = self.tabs[i].scene.document.get_entity(handle).cloned() {
                            entities.push((handle, entity));
                        }
                    }
                }
                let initial_edges: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .filter(|(_, entity)| {
                        is_trim_boundary_entity(entity)
                            || matches!(entity, codec::EntityType::Insert(_))
                    })
                    .map(|(handle, _)| handle)
                    .collect();
                let all_entities: Vec<_> = entities.into_iter().map(|(_, e)| e).collect();
                let visible_inserts = self.tabs[i]
                    .scene
                    .visible_entities_for_edit(|entity| {
                        matches!(entity, codec::EntityType::Insert(_))
                    });
                let document = &self.tabs[i].scene.document;
                let block_boundaries = visible_inserts
                    .into_iter()
                    .filter_map(|entity| {
                        let (handle, entity) = entity;
                        let codec::EntityType::Insert(insert) = entity else {
                            return None;
                        };
                        let children = insert.block_entities_from_document(document)?;
                        let pieces = insert
                            .explode(&children)
                            .into_iter()
                            .filter(|child| {
                                !crate::scene::block_construction::is_construction(
                                    child.common(),
                                )
                            })
                            .collect();
                        Some((handle, pieces))
                    })
                    .collect();
                let new_cmd = TrimCommand::with_block_boundaries(
                    all_entities,
                    block_boundaries,
                    initial_edges,
                );
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "EXTRIM" => {
                use crate::modules::draw::modify::trim::{is_trim_boundary_entity, ExtrimCommand};
                let all = self.tabs[i].scene.visible_entities_for_edit(is_trim_boundary_entity);
                let new_cmd = ExtrimCommand::new(all);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "EXTEND" => {
                use crate::modules::draw::modify::trim::{is_trim_boundary_entity, ExtendCommand};
                let entities =
                    self.tabs[i].scene.visible_entities_for_edit(is_trim_boundary_entity);
                let initial_edges: Vec<codec::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .filter(|(_, entity)| is_trim_boundary_entity(entity))
                    .map(|(handle, _)| handle)
                    .collect();
                let all_entities: Vec<_> = entities.into_iter().map(|(_, e)| e).collect();
                let new_cmd = ExtendCommand::with_boundary_edges(all_entities, initial_edges);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            // ARCTEXT <text> — lay the text out as one Text entity per character
            // along the selected arc, each rotated to follow the tangent.
            "ARCTEXT" => {
                use crate::command::SelectThenValueCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenValueCommand::new(
                    "ARCTEXT",
                    "ARCTEXT  text to place along the arc:",
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("ARCTEXT ") => {
                use codec::entities::Text;
                use codec::types::Vector3;
                let text = cmd.strip_prefix("ARCTEXT").unwrap_or("").trim().to_string();
                if text.is_empty() {
                    self.command_line.push_info(
                        crate::t!(
                            "Usage: ARCTEXT <text>   (select an arc first; the text follows it)"
                        )
                        .as_ref(),
                    );
                    return None;
                }
                let arc =
                    self.tabs[i]
                        .scene
                        .selected_entities()
                        .iter()
                        .find_map(|(_, e)| match e {
                            codec::EntityType::Arc(a) => Some(a.clone()),
                            _ => None,
                        });
                let Some(arc) = arc else {
                    self.command_line
                        .push_error(crate::t!("ARCTEXT: select an arc first.").as_ref());
                    return None;
                };
                let chars: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
                if chars.is_empty() {
                    self.command_line
                        .push_error(crate::t!("ARCTEXT: no printable text.").as_ref());
                    return None;
                }
                let n = chars.len();
                let mut span = arc.end_angle - arc.start_angle;
                if span <= 0.0 {
                    span += std::f64::consts::TAU;
                }
                let height = (arc.radius * 0.12).max(0.1);
                self.push_undo_snapshot(i, "ARCTEXT");
                for (k, ch) in chars.iter().enumerate() {
                    let ang = arc.start_angle + span * (k as f64 + 0.5) / n as f64;
                    let pos = Vector3::new(
                        arc.center.x + arc.radius * ang.cos(),
                        arc.center.y + arc.radius * ang.sin(),
                        arc.center.z,
                    );
                    let t = Text::with_value(ch.to_string(), pos)
                        .with_height(height)
                        .with_rotation(ang + std::f64::consts::FRAC_PI_2);
                    self.tabs[i].scene.add_entity(codec::EntityType::Text(t));
                }
                self.tabs[i].dirty = true;
                self.command_line.push_output(
                    crate::tf!("ARCTEXT: placed {n} character(s) along the arc.").as_ref(),
                );
            }

            "DIMOVERRIDE" => {
                use crate::modules::annotate::dimoverride::DimOverrideCommand;
                let document = &self.tabs[i].scene.document;
                let style = document
                    .dim_styles
                    .iter()
                    .find(|style| {
                        style
                            .name
                            .eq_ignore_ascii_case(&document.header.current_dimstyle_name)
                    })
                    .cloned();
                let named = crate::modules::annotate::dimoverride::NamedValues {
                    defaults: style
                        .as_ref()
                        .map(|style| {
                            let block = |handle: codec::Handle| {
                                document
                                    .block_records
                                    .iter()
                                    .find(|record| !handle.is_null() && record.handle == handle)
                                    .map_or_else(
                                        || "ClosedFilled".to_string(),
                                        |record| record.name.clone(),
                                    )
                            };
                            vec![
                                ("DIMBLK".to_string(), block(style.dimblk)),
                                ("DIMBLK1".to_string(), block(style.dimblk1)),
                                ("DIMBLK2".to_string(), block(style.dimblk2)),
                                ("DIMLDRBLK".to_string(), block(style.dimldrblk)),
                                ("DIMTXSTY".to_string(), style.dimtxsty.clone()),
                            ]
                        })
                        .unwrap_or_default(),
                    blocks: document
                        .block_records
                        .iter()
                        .map(|record| record.name.clone())
                        .collect(),
                    text_styles: document
                        .text_styles
                        .iter()
                        .map(|style| style.name.clone())
                        .collect(),
                };
                let cmd = DimOverrideCommand::new(style, named);
                self.command_line.push_info(&cmd.prompt());
                self.command_line.set_step_options(cmd.options());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            cmd if cmd.starts_with("DIMOVERRIDE_APPLY ") => {
                let spec = cmd.trim_start_matches("DIMOVERRIDE_APPLY ").trim();
                // Arrowhead blocks and the text style are written as handles,
                // resolved (a standard arrowhead's block made) once here.
                use crate::modules::annotate::dimoverride::NAMED_VALUE_VARS;
                let mut handle_values = Vec::new();
                let spec: String = spec
                    .split(';')
                    .filter(|entry| {
                        let (name, value) = entry.split_once('=').unwrap_or((entry, ""));
                        let name = name.trim().to_ascii_uppercase();
                        if !NAMED_VALUE_VARS.contains(&name.as_str()) {
                            return true;
                        }
                        handle_values.push((name, value.trim().to_string()));
                        false
                    })
                    .collect::<Vec<_>>()
                    .join(";");
                let spec = spec.as_str();
                let mut resolved_handles = Vec::new();
                for (name, value) in handle_values {
                    use crate::entities::dim_override as dov;
                    let doc = &mut self.tabs[i].scene.document;
                    let (code, handle) = if name == "DIMTXSTY" {
                        let found = doc
                            .text_styles
                            .iter()
                            .find(|style| style.name.eq_ignore_ascii_case(&value))
                            .map(|style| style.handle);
                        (dov::DIMTXSTY, found)
                    } else {
                        let code = match name.as_str() {
                            "DIMBLK" => dov::DIMBLK,
                            "DIMBLK1" => dov::DIMBLK1,
                            "DIMBLK2" => dov::DIMBLK2,
                            _ => dov::DIMLDRBLK,
                        };
                        let block = crate::entities::arrowheads::typed_standard_block(&value)
                            .map(str::to_string)
                            .unwrap_or(value.clone());
                        (code, crate::entities::arrowheads::resolve(doc, &block))
                    };
                    if let Some(handle) = handle {
                        resolved_handles.push((code, codec::xdata::XDataValue::Handle(handle)));
                    }
                }
                let values = if spec.eq_ignore_ascii_case("CLEAR") {
                    Vec::new()
                } else if spec.trim().is_empty() {
                    resolved_handles.clone()
                } else {
                    match crate::modules::annotate::dimoverride::parse_spec(spec) {
                        Ok(mut values) if !values.is_empty() || !resolved_handles.is_empty() => {
                            values.extend(resolved_handles.iter().cloned());
                            values
                        }
                        Ok(_) => return Some(self.finish_dispatch(cmd)),
                        Err(entry) => {
                            self.command_line.push_error(
                                crate::tf!("DIMOVERRIDE: invalid override \"{entry}\".").as_ref(),
                            );
                            return Some(self.finish_dispatch(cmd));
                        }
                    }
                };
                let handles = self.selected_editable_dimensions(i);
                if self.tabs[i].scene.selected_handles_in_order().is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let sel = SelectObjectsCommand::routed("DIMOVERRIDE", cmd);
                    self.command_line.push_info(&sel.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(sel));
                } else if handles.is_empty() {
                    self.command_line.push_error(
                        crate::t!("DIMOVERRIDE: no editable dimensions were selected.").as_ref(),
                    );
                } else {
                    use crate::entities::dim_override as dov;
                    self.push_undo_snapshot(i, "DIMOVERRIDE");
                    for &handle in &handles {
                        let doc = &mut self.tabs[i].scene.document;
                        if values.is_empty() {
                            dov::replace(doc, handle, Vec::new());
                        }
                        for (code, value) in &values {
                            dov::set(doc, handle, *code, Some(value.clone()));
                        }
                    }
                    self.invalidate_property_targets(i, &handles);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                    self.command_line.push_output(
                        crate::tf!("DIMOVERRIDE: updated {} dimension(s).", handles.len()).as_ref(),
                    );
                }
            }

            // DIMREASSOCIATE's Disassociated: every dimension of the space
            // without associations.
            "_DIMREASSOCIATE_DISASSOCIATED" => {
                let scene = &self.tabs[i].scene;
                let handles: Vec<codec::Handle> = scene
                    .document
                    .entities()
                    .filter_map(|entity| match entity {
                        codec::EntityType::Dimension(dimension) => {
                            Some(dimension.base().common.handle)
                        }
                        _ => None,
                    })
                    .filter(|&handle| {
                        scene.document.get_entity(handle).is_some_and(|entity| {
                            entity.common().owner_handle == scene.current_layout_block_handle_pub()
                        }) && scene.dimension_association(handle).is_none()
                            && !scene.is_layer_locked(handle)
                    })
                    .collect();
                self.tabs[i].scene.deselect_all();
                if handles.is_empty() {
                    return Some(self.finish_dispatch(cmd));
                }
                let found = handles.len();
                self.command_line.push_output(&crate::tf!("{found} found"));
                self.tabs[i].scene.select_entities(&handles);
                return Some(self.dispatch_command("DIMREASSOCIATE"));
            }

            // DIMREGEN: every dimension draws its picture afresh, and the
            // next save writes the new pictures.
            "DIMREGEN" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter(|entity| {
                        matches!(entity, codec::EntityType::Dimension(d)
                            if !d.base().block_name.is_empty())
                    })
                    .map(|entity| entity.common().handle)
                    .collect();
                if !handles.is_empty() {
                    self.push_undo_snapshot(i, "DIMREGEN");
                    for &handle in &handles {
                        self.tabs[i].scene.invalidate_dim_block_recorded(handle);
                    }
                    let changes: Vec<_> = handles
                        .iter()
                        .map(|&handle| (handle, crate::scene::ChangeKind::Modified))
                        .collect();
                    self.tabs[i].scene.bump_entities(&changes);
                    self.tabs[i].dirty = true;
                }
            }

            "DIMDISASSOCIATE" | "DIMREASSOCIATE" => {
                let handles = self.selected_editable_dimensions(i);
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    self.tabs[i].scene.deselect_all();
                    let sel = if cmd == "DIMDISASSOCIATE" {
                        self.command_line
                            .push_info(&crate::t!("Select dimensions to disassociate ..."));
                        SelectObjectsCommand::associative_dimensions(cmd, cmd, "Select objects:")
                    } else {
                        self.command_line
                            .push_info(&crate::t!("Select dimensions to reassociate ..."));
                        SelectObjectsCommand::with_keyword(
                            cmd,
                            cmd,
                            "Select objects or [Disassociated]:",
                            "Disassociated",
                            "D",
                            || crate::command::CmdResult::Dispatch("_DIMREASSOCIATE_DISASSOCIATED".into()),
                        )
                    };
                    self.command_line.push_info(&sel.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(sel));
                } else if cmd == "DIMDISASSOCIATE" {
                    self.push_undo_snapshot(i, "DIMDISASSOCIATE");
                    let count = handles
                        .iter()
                        .filter(|&&handle| self.tabs[i].scene.detach_dimension_association(handle))
                        .count();
                    self.invalidate_property_targets(i, &handles);
                    self.tabs[i].dirty |= count > 0;
                    self.command_line
                        .push_output(crate::tf!("{count} disassociated.").as_ref());
                } else {
                    // Each dimension's definition points are asked in turn.
                    use crate::modules::annotate::dimreassociate::{
                        DimReassociateCommand, ReassociateKind,
                    };
                    let scene = &self.tabs[i].scene;
                    let v = |p: codec::types::Vector3| glam::DVec3::new(p.x, p.y, p.z);
                    let (items, points): (Vec<_>, Vec<_>) = handles
                        .iter()
                        .filter_map(|&handle| {
                            let dimension = scene
                                .document
                                .get_entity(handle)
                                .and_then(crate::entities::dimension::as_dimension)?;
                            let points = match dimension {
                                codec::entities::Dimension::Linear(d) => {
                                    vec![v(d.first_point), v(d.second_point)]
                                }
                                codec::entities::Dimension::Aligned(d) => {
                                    vec![v(d.first_point), v(d.second_point)]
                                }
                                codec::entities::Dimension::Angular3Pt(d) => {
                                    vec![v(d.angle_vertex), v(d.first_point), v(d.second_point)]
                                }
                                codec::entities::Dimension::Ordinate(d) => {
                                    vec![v(d.feature_location)]
                                }
                                _ => Vec::new(),
                            };
                            let kind = match dimension {
                                codec::entities::Dimension::Linear(_)
                                | codec::entities::Dimension::Aligned(_) => ReassociateKind::Linear,
                                codec::entities::Dimension::Angular3Pt(_) => {
                                    ReassociateKind::Angular3
                                }
                                codec::entities::Dimension::Angular2Ln(_) => {
                                    ReassociateKind::Angular2
                                }
                                codec::entities::Dimension::Radius(_)
                                | codec::entities::Dimension::Diameter(_) => {
                                    ReassociateKind::Radial
                                }
                                codec::entities::Dimension::Ordinate(_) => {
                                    ReassociateKind::Ordinate
                                }
                                _ => return None,
                            };
                            Some(((handle, kind), points))
                        })
                        .unzip();
                    self.tabs[i].scene.deselect_all();
                    if !items.is_empty() {
                        let command = DimReassociateCommand::new(items, points);
                        self.command_line.push_info(&command.prompt());
                        self.command_line.set_step_options(command.options());
                        self.tabs[i].active_cmd = Some(Box::new(command));
                    }
                }
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }

    /// The selected dimensions that are not on a locked layer.
    fn selected_editable_dimensions(&self, i: usize) -> Vec<codec::Handle> {
        let scene = &self.tabs[i].scene;
        scene
            .selected_entities()
            .into_iter()
            .filter(|(handle, entity)| {
                matches!(entity, codec::EntityType::Dimension(_)) && !scene.is_layer_locked(*handle)
            })
            .map(|(handle, _)| handle)
            .collect()
    }
}

// ── TCASE helpers ──────────────────────────────────────────────────────────
fn sentence_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl OpenCADStudio {
    /// TXT2MTXT on the selection (in selection order), as one undo step.
    fn apply_txt2mtxt(&mut self, i: usize) {
        let scene = &self.tabs[i].scene;
        let handles: Vec<codec::Handle> = scene
            .selected_handles_in_order()
            .into_iter()
            .filter(|handle| !scene.is_layer_locked(*handle))
            .collect();
        let plan = {
            let selected: Vec<(codec::Handle, &codec::EntityType)> = handles
                .iter()
                .filter_map(|h| scene.document.get_entity(*h).map(|e| (*h, e)))
                .collect();
            crate::modules::annotate::txt2mtxt::plan(
                &selected,
                &scene.document,
                self.txt2mtxt_settings,
            )
        };
        self.tabs[i].scene.deselect_all();
        // Enter on an empty selection ends quietly.
        if handles.is_empty() {
            return;
        }
        let Some(plan) = plan else {
            self.command_line
                .push_output("  0 Text objects removed, 0 MText objects added.");
            return;
        };
        self.push_undo_snapshot(i, "TXT2MTXT");
        self.tabs[i].scene.erase_entities(&plan.erase);
        let added = plan.add.len();
        for entity in plan.add {
            self.tabs[i].scene.add_entity(entity);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        // Every selected object counts as removed text, the way the
        // reference reports it.
        let removed = handles.len();
        self.command_line.push_output(&if added == 1 {
            format!("  {removed} Text objects removed, 1 MText object added.")
        } else {
            format!("  {removed} Text objects removed, {added} MText objects added.")
        });
    }
}
