use super::*;

fn selected_solid(app: &OpenCADStudio, tab: usize) -> Option<acadrust::Handle> {
    let scene = &app.tabs.get(tab)?.scene;
    let selected = scene.selected_handles_in_order();
    let [handle] = selected.as_slice() else {
        return None;
    };

    if matches!(
        scene.document.get_entity(*handle),
        Some(acadrust::EntityType::Solid3D(_))
    ) {
        Some(*handle)
    } else {
        None
    }
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

            // JUSTIFYTEXT <option> — change the justification of selected text and
            // multiline-text objects.
            "JUSTIFYTEXT" => {
                use crate::command::SelectThenKeywordCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenKeywordCommand::new(
                    "JUSTIFYTEXT",
                    "JUSTIFYTEXT  [Left / Center / Right / Middle / Aligned / Fit]:",
                    vec![
                        ("Left", "LEFT", None),
                        ("Center", "CENTER", None),
                        ("Right", "RIGHT", None),
                        ("Middle", "MIDDLE", None),
                        ("Aligned", "ALIGN", None),
                        ("Fit", "FIT", None),
                    ],
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("JUSTIFYTEXT ") => {
                let opt = cmd.split_whitespace().nth(1).unwrap_or("").to_uppercase();
                let handles: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("JUSTIFYTEXT: select text objects first.").as_ref());
                    return Some(Task::none());
                }
                if opt.is_empty() {
                    self.command_line.push_info(
                        crate::t!("Usage: JUSTIFYTEXT <Left|Center|Right|Middle|TL|TC|TR|ML|MC|MR|BL|BC|BR>").as_ref(),
                    );
                    return Some(Task::none());
                }
                use acadrust::entities::{
                    AttachmentPoint as AP, TextHorizontalAlignment as TH,
                    TextVerticalAlignment as TV,
                };
                let text_align = match opt.as_str() {
                    "L" | "LEFT" | "TL" | "ML" | "BL" => Some(TH::Left),
                    "C" | "CENTER" | "TC" | "BC" => Some(TH::Center),
                    "R" | "RIGHT" | "TR" | "MR" | "BR" => Some(TH::Right),
                    "M" | "MIDDLE" | "MC" => Some(TH::Middle),
                    "A" | "ALIGN" | "ALIGNED" => Some(TH::Aligned),
                    "F" | "FIT" => Some(TH::Fit),
                    _ => None,
                };
                // Vertical band for single-line TEXT. Bare L/C/R (and Aligned/Fit
                // and the special Middle) keep the baseline; the T*/M*/B* codes
                // pin the top/middle/bottom of the text box.
                let text_valign = match opt.as_str() {
                    "TL" | "TC" | "TR" => Some(TV::Top),
                    "ML" | "MC" | "MR" => Some(TV::Middle),
                    "BL" | "BC" | "BR" => Some(TV::Bottom),
                    _ => None,
                };
                let mtext_ap = match opt.as_str() {
                    "TL" => Some(AP::TopLeft),
                    "TC" => Some(AP::TopCenter),
                    "TR" => Some(AP::TopRight),
                    "ML" | "L" | "LEFT" => Some(AP::MiddleLeft),
                    "MC" | "M" | "MIDDLE" | "C" | "CENTER" => Some(AP::MiddleCenter),
                    "MR" | "R" | "RIGHT" => Some(AP::MiddleRight),
                    "BL" => Some(AP::BottomLeft),
                    "BC" => Some(AP::BottomCenter),
                    "BR" => Some(AP::BottomRight),
                    _ => None,
                };
                if text_align.is_none() && mtext_ap.is_none() {
                    self.command_line
                        .push_error(crate::t!("JUSTIFYTEXT: unknown justification option.").as_ref());
                    return Some(Task::none());
                }
                self.push_undo_snapshot(i, "JUSTIFYTEXT");
                let mut n = 0usize;
                for h in &handles {
                    if let Some(e) = self.tabs[i]
                        .scene
                        .document
                        .entities_mut()
                        .find(|e| e.common().handle == *h)
                    {
                        match e {
                            acadrust::EntityType::Text(t) => {
                                let mut changed = false;
                                if let Some(a) = text_align {
                                    t.horizontal_alignment = a;
                                    changed = true;
                                }
                                if let Some(v) = text_valign {
                                    t.vertical_alignment = v;
                                    changed = true;
                                }
                                if changed {
                                    crate::entities::text::sync_text_alignment_point(t);
                                    n += 1;
                                }
                            }
                            acadrust::EntityType::MText(m) => {
                                if let Some(a) = mtext_ap {
                                    m.attachment_point = a;
                                    n += 1;
                                }
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
                    .push_output(crate::tf!("JUSTIFYTEXT: updated {n} text object(s).").as_ref());
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
                let handles: Vec<acadrust::Handle> = self.tabs[i]
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
                            acadrust::EntityType::Text(t) => {
                                t.value = conv(&t.value);
                                n += 1;
                            }
                            acadrust::EntityType::MText(m) => {
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
                let handles: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .filter(|(_, e)| {
                        matches!(
                            e,
                            acadrust::EntityType::Text(_) | acadrust::EntityType::MText(_)
                        )
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
                    let c1 = acadrust::types::Vector3::new(
                        (min[0] - pad) as f64,
                        (min[1] - pad) as f64,
                        min[2] as f64,
                    );
                    let c2 = acadrust::types::Vector3::new(
                        (max[0] + pad) as f64,
                        (max[1] + pad) as f64,
                        min[2] as f64,
                    );
                    self.tabs[i]
                        .scene
                        .add_entity_clone(acadrust::EntityType::Wipeout(
                            acadrust::entities::Wipeout::from_corners(c1, c2),
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
                let c =
                    SelectThenValueCommand::new("TEXTFIT", "TEXTFIT  target width:", has_sel);
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
                let handles: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .filter(|(_, e)| matches!(e, acadrust::EntityType::Text(_)))
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
                    if let Some(acadrust::EntityType::Text(t)) = self.tabs[i]
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
                self.command_line.push_output(crate::tf!(
                    "TEXTFIT: fitted {n} text object(s) to width {target}."
                ).as_ref());
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

                let start: i64 = args
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);

                let increment: i64 = args
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);

                let placement = args
                    .next()
                    .unwrap_or("O")
                    .to_uppercase();

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

                let mut texts: Vec<(acadrust::Handle, f64, f64)> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .filter_map(|(h, e)| match e {
                        acadrust::EntityType::Text(t)
                            if !self.tabs[i].scene.is_layer_locked(*h) =>
                        {
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
                        .then(
                            a.1.partial_cmp(&b.1)
                                .unwrap_or(std::cmp::Ordering::Equal),
                        )
                });

                self.push_undo_snapshot(i, "TCOUNT");

                let mut num = start;

                for (h, _, _) in &texts {
                    if let Some(acadrust::EntityType::Text(t)) = self.tabs[i]
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
                    .map(|(handle, _, _)| {
                        (*handle, crate::scene::ChangeKind::Modified)
                    })
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
                let style = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .iter()
                    .find_map(|(handle, object)| match object {
                        acadrust::objects::ObjectType::MultiLeaderStyle(style)
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
                let new_cmd = style.map_or_else(MLeaderCommand::new, |style| {
                    MLeaderCommand::with_style(style, multiplier)
                }).with_drawing_resources(block_sources, layers, text_styles);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "TOLERANCE" => {
                self.open_tolerance_dialog(None);
            }

            "TABLE" => {
                use crate::modules::annotate::table_cmd::TableCommand;
                let name = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_table_style_name
                    .clone();
                let style = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .iter()
                    .find_map(|(handle, object)| match object {
                        acadrust::objects::ObjectType::TableStyle(style)
                            if style.name.eq_ignore_ascii_case(&name) =>
                        {
                            Some((*handle, style.clone()))
                        }
                        _ => None,
                    });
                let multiplier = self.tabs[i].scene.creation_annotation_multiplier();
                let cmd = style.as_ref().map_or_else(TableCommand::new, |(handle, style)| {
                    TableCommand::with_style(*handle, style, multiplier)
                });
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "TABLEDIT" => {
                use crate::modules::annotate::table_cmd::TableditCommand;
                let cmd = TableditCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMCONTINUE" => {
                use crate::modules::annotate::dim_continue::DimContinueCommand;
                let scene = &self.tabs[i].scene;
                let recent = scene
                    .last_created_dimension
                    .filter(|handle| scene.entity_belongs_to_active_space(*handle))
                    .and_then(|handle| scene.document.get_entity(handle))
                    .cloned();
                let cmd = DimContinueCommand::new(
                    recent,
                    self.dimension_continue_mode == 1,
                );
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMBASELINE" => {
                use crate::modules::annotate::dim_baseline::DimBaselineCommand;
                let scene = &self.tabs[i].scene;
                let recent = scene
                    .last_created_dimension
                    .filter(|handle| scene.entity_belongs_to_active_space(*handle))
                    .and_then(|handle| scene.document.get_entity(handle))
                    .cloned();
                let dimdli_by_style = scene
                    .document
                    .dim_styles
                    .iter()
                    .map(|style| (style.name.to_ascii_lowercase(), style.dimdli))
                    .collect();
                let fallback_dimdli = if scene.document.header.measurement == 1 {
                    3.75
                } else {
                    0.38
                };
                let current_style_name = scene.document.header.current_dimstyle_name.clone();
                let cmd = DimBaselineCommand::new(
                    recent,
                    dimdli_by_style,
                    current_style_name,
                    fallback_dimdli,
                    self.dimension_continue_mode == 1,
                );
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
                let cmd = QdimCommand::new(
                    selection,
                    dim_spacing,
                    self.quick_dimension_snap_priority,
                );
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "DIMEDIT" => {
                use crate::modules::annotate::dimedit::DimEditCommand;
                let cmd = DimEditCommand::new();
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

            "MLEADERADD" => {
                use crate::modules::annotate::mleader_edit::MLeaderAddCommand;
                let cmd = MLeaderAddCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "MLEADERREMOVE" => {
                use crate::modules::annotate::mleader_edit::MLeaderRemoveCommand;
                let cmd = MLeaderRemoveCommand::new();
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
                self.command_line.push_output(crate::t!("Zoom Extents — All Viewports").as_ref());
            }

            "ZOOM EXTENTS" | "ZOOM E" | "ZOOMEXTENTS" | "ZE" => {
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.fit_all();
                self.command_line.push_output(crate::t!("Zoom Extents").as_ref());
            }

            "ZOOM IN" | "ZOOM I" | "ZI" => {
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.zoom_camera(1.0 / 1.5);
                self.command_line.push_output(crate::t!("Zoom In").as_ref());
            }

            "ZOOM OUT" | "ZO" => {
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.zoom_camera(1.5);
                self.command_line.push_output(crate::t!("Zoom Out").as_ref());
            }

            // ZOOM ALL — fit the configured drawing limits.
            "ZOOM ALL" | "ZOOM A" | "ZA" => {
                self.tabs[i].scene.remember_current_view();
                self.tabs[i].scene.fit_all_with_limits();
                self.command_line.push_output(crate::t!("Zoom All").as_ref());
            }

            "ZOOM PREVIOUS" | "ZOOM P" | "ZP" => {
                if self.tabs[i].scene.restore_previous_view() {
                    self.command_line.push_output(crate::t!("Zoom Previous").as_ref());
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
                        self.command_line.push_output(crate::t!("Zoom Object").as_ref());
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
                if let Ok(factor) = rest.trim().parse::<f32>() {
                    if factor > 0.0 {
                        self.tabs[i].scene.remember_current_view();
                        self.tabs[i].scene.zoom_camera(1.0 / factor);
                        self.command_line
                            .push_output(crate::tf!("Zoom Scale ×{factor:.3}").as_ref());
                    }
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
                let new_cmd = StretchCommand::new(handles, wires);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "SOLIDFILLET" => {
                use crate::modules::model::edge_cmd::{EdgeOperation, SolidEdgeCommand};
                let command = SolidEdgeCommand::new(EdgeOperation::Fillet, selected_solid(self, i));
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            "FILLET" if selected_solid(self, i).is_some() => {
                use crate::modules::model::edge_cmd::{EdgeOperation, SolidEdgeCommand};
                let command = SolidEdgeCommand::new(EdgeOperation::Fillet, selected_solid(self, i));
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            "FILLET" => {
                use crate::modules::draw::modify::fillet::FilletCommand;
                let entities: Vec<_> = self.tabs[i]
                    .scene
                    .entity_wires()
                    .iter()
                    .filter_map(|w| {
                        let h = Scene::handle_from_wire_name(&w.name)?;
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(h)
                            .cloned()
                            .map(|e| (h, e))
                    })
                    .collect();
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
                    .entity_wires()
                    .iter()
                    .filter_map(|wire| {
                        let handle = Scene::handle_from_wire_name(&wire.name)?;
                        self.tabs[i].scene.document.get_entity(handle).cloned()
                    })
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
                        .entity_wires()
                        .iter()
                        .filter_map(|w| {
                            let h = Scene::handle_from_wire_name(&w.name)?;
                            self.tabs[i].scene.document.get_entity(h).cloned()
                        })
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

            "SOLIDCHAMFER" => {
                use crate::modules::model::edge_cmd::{EdgeOperation, SolidEdgeCommand};
                let command = SolidEdgeCommand::new(EdgeOperation::Chamfer, selected_solid(self, i));
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            "CHAMFER" if selected_solid(self, i).is_some() => {
                use crate::modules::model::edge_cmd::{EdgeOperation, SolidEdgeCommand};
                let command = SolidEdgeCommand::new(EdgeOperation::Chamfer, selected_solid(self, i));
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }

            "CHAMFER" => {
                use crate::modules::draw::modify::fillet::ChamferCommand;
                let entities: Vec<_> = self.tabs[i]
                    .scene
                    .entity_wires()
                    .iter()
                    .filter_map(|w| {
                        let h = Scene::handle_from_wire_name(&w.name)?;
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(h)
                            .cloned()
                            .map(|e| (h, e))
                    })
                    .collect();
                let all_entities: Vec<_> = entities.into_iter().map(|(_, e)| e).collect();
                let new_cmd = ChamferCommand::new(
                    crate::modules::draw::defaults::get_chamfer_dist1(),
                    all_entities,
                );
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "EXPLODE" => {
                use crate::modules::draw::modify::explode::explode_entity;
                let entities: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .filter(|(handle, _)| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if entities.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("EXPLODE");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let replacements: Vec<(acadrust::Handle, Vec<acadrust::EntityType>)> = entities
                        .iter()
                        .filter_map(|(h, e)| {
                            let pieces = explode_entity(e, &self.tabs[i].scene.document);
                            if pieces.is_empty() {
                                None
                            } else {
                                Some((*h, pieces))
                            }
                        })
                        .collect();
                    let exploded = replacements.len();
                    if exploded > 0 {
                        self.push_undo_snapshot(i, "EXPLODE");
                    }
                    for (handle, pieces) in replacements {
                        self.tabs[i].scene.erase_entities(&[handle]);
                        for piece in pieces {
                            self.tabs[i].scene.add_entity(piece);
                        }
                    }
                    if exploded > 0 {
                        self.tabs[i].dirty = true;
                        self.refresh_properties();
                        self.command_line
                            .push_output(crate::tf!("{exploded} object(s) exploded.").as_ref());
                    } else {
                        self.command_line
                            .push_info(crate::t!("EXPLODE: no explodable objects selected.").as_ref());
                    }
                }
            }

            "OFFSET" => {
                use crate::modules::draw::modify::offset::{is_offsettable, OffsetCommand};
                let all_entities: Vec<_> = self.tabs[i]
                    .scene
                    .entity_wires()
                    .iter()
                    .filter_map(|w| {
                        let h = Scene::handle_from_wire_name(&w.name)?;
                        self.tabs[i].scene.document.get_entity(h).cloned()
                    })
                    .collect();
                // Pick-first (#422): with offsettable objects already selected,
                // skip the pick step and go straight to distance / side.
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
                use crate::modules::draw::modify::trim::TrimCommand;
                let entities: Vec<_> = self.tabs[i]
                    .scene
                    .entity_wires()
                    .iter()
                    .filter_map(|w| {
                        let h = Scene::handle_from_wire_name(&w.name)?;
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(h)
                            .cloned()
                            .map(|e| (h, e))
                    })
                    .collect();
                let initial_edges: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .filter_map(|(handle, entity)| {
                        crate::modules::draw::modify::trim::is_trim_boundary_entity(&entity)
                            .then_some(handle)
                    })
                    .collect();
                let all_entities: Vec<_> = entities.into_iter().map(|(_, e)| e).collect();
                let new_cmd =
                    TrimCommand::with_cutting_edges(all_entities, initial_edges);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "EXTRIM" => {
                use crate::modules::draw::modify::trim::ExtrimCommand;
                let all: Vec<_> = self.tabs[i]
                    .scene
                    .entity_wires()
                    .iter()
                    .filter_map(|w| {
                        let h = Scene::handle_from_wire_name(&w.name)?;
                        self.tabs[i].scene.document.get_entity(h).cloned().map(|e| (h, e))
                    })
                    .collect();
                let new_cmd = ExtrimCommand::new(all);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "EXTEND" => {
                use crate::modules::draw::modify::trim::ExtendCommand;
                let entities: Vec<_> = self.tabs[i]
                    .scene
                    .entity_wires()
                    .iter()
                    .filter_map(|w| {
                        let h = Scene::handle_from_wire_name(&w.name)?;
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(h)
                            .cloned()
                            .map(|e| (h, e))
                    })
                    .collect();
                let all_entities: Vec<_> = entities.into_iter().map(|(_, e)| e).collect();
                let new_cmd = ExtendCommand::new(all_entities);
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
                use acadrust::entities::Text;
                use acadrust::types::Vector3;
                let text = cmd.strip_prefix("ARCTEXT").unwrap_or("").trim().to_string();
                if text.is_empty() {
                    self.command_line.push_info(
                        crate::t!("Usage: ARCTEXT <text>   (select an arc first; the text follows it)").as_ref(),
                    );
                    return None;
                }
                let arc =
                    self.tabs[i]
                        .scene
                        .selected_entities()
                        .iter()
                        .find_map(|(_, e)| match e {
                            acadrust::EntityType::Arc(a) => Some(a.clone()),
                            _ => None,
                        });
                let Some(arc) = arc else {
                    self.command_line
                        .push_error(crate::t!("ARCTEXT: select an arc first.").as_ref());
                    return None;
                };
                let chars: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
                if chars.is_empty() {
                    self.command_line.push_error(crate::t!("ARCTEXT: no printable text.").as_ref());
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
                    self.tabs[i].scene.add_entity(acadrust::EntityType::Text(t));
                }
                self.tabs[i].dirty = true;
                self.command_line
                    .push_output(crate::tf!("ARCTEXT: placed {n} character(s) along the arc.").as_ref());
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
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
