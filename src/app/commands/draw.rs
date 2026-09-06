use super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_draw(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            // ── Draw commands ──────────────────────────────────────────────
            "LINE" => {
                use crate::modules::draw::draw::line::LineCommand;
                let new_cmd = LineCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "MLINE" => {
                use crate::modules::draw::draw::mline::MlineCommand;
                let header = &self.tabs[i].scene.document.header;
                let style_name = header.multiline_style.clone();
                let scale = header.multiline_scale;
                let justification = header.multiline_justification;
                let styles = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .iter()
                    .filter_map(|(handle, object)| match object {
                        acadrust::objects::ObjectType::MLineStyle(style) => {
                            Some((*handle, style.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                let cmd_obj = MlineCommand::with_styles(
                    styles,
                    style_name,
                    scale,
                    justification,
                );
                self.command_line.push_info(&cmd_obj.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd_obj));
            }

            cmd if cmd == "WIPEOUT" || cmd == "WO" || cmd.starts_with("WIPEOUT ") => {
                use crate::modules::draw::draw::wipeout::WipeoutCommand;
                let args = cmd
                    .split_once(' ')
                    .map(|(_, r)| r.trim().to_uppercase())
                    .unwrap_or_default();
                let wo_cmd = match args.as_str() {
                    "P" | "POLYLINE" => WipeoutCommand::new_polyline(),
                    "R" | "RECTANGULAR" => WipeoutCommand::new_rectangular(),
                    _ => WipeoutCommand::new_polygonal(
                        crate::modules::draw::draw::wipeout::wipeout_frame_mode(
                            &self.tabs[i].scene.document,
                        ),
                    ),
                };
                self.command_line.push_info(&wo_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(wo_cmd));
            }

            cmd if cmd == "IMAGE" || cmd == "IMAGEATTACH" || cmd == "IM" => {
                return Some(Task::done(Message::ImagePick));
            }

            "REVCLOUD" => {
                use crate::modules::draw::draw::revcloud::RevCloudCommand;
                let view_height = self.tabs[i].scene.camera.borrow().ortho_size() as f64 * 2.0;
                let default_arc_length = (view_height * 0.0125).max(1.0e-6);
                let sources = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .map(|entity| (entity.common().handle, entity.clone()))
                    .collect();
                let cmd = RevCloudCommand::new(default_arc_length, sources);
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "ATTDEF" => {
                use crate::modules::draw::draw::attdef::AttdefCommand;
                let defaults = crate::scene::creation_style::current_text_defaults(
                    &self.tabs[i].scene.document,
                );
                let cmd = AttdefCommand::with_text_defaults(
                    defaults.height,
                    defaults.style_name,
                    defaults.width_factor,
                    defaults.oblique_angle,
                );
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            // Command-line attribute editing on selected Insert entities. Bare
            // ATTEDIT and the ATE alias launch the interactive editor instead
            // (see the ATTEDIT arm in the inquiry family); the dash form is the
            // command-line entry point.
            // Usage:
            //   -ATTEDIT          — list all attributes on selected Insert(s)
            //   ATTEDIT <tag> <v> — quick-set attribute <tag> to <v>
            cmd if cmd.starts_with("ATTEDIT ")
                || cmd == "-ATTEDIT"
                || cmd.starts_with("-ATTEDIT ") =>
            {
                let rest = cmd
                    .trim_start_matches("-ATTEDIT")
                    .trim_start_matches("ATTEDIT")
                    .trim();
                let parts: Vec<&str> = rest.splitn(2, char::is_whitespace).collect();
                let selected_handles: Vec<acadrust::Handle> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(h, _)| *h)
                    .collect();
                if selected_handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("ATTEDIT: select an Insert entity first.").as_ref());
                } else {
                    let mut found_any = false;
                    for sh in &selected_handles {
                        if let Some(acadrust::EntityType::Insert(ins)) = self.tabs[i]
                            .scene
                            .document
                            .entities()
                            .find(|e| e.common().handle == *sh)
                        {
                            found_any = true;
                            if rest.is_empty() {
                                // List attributes.
                                if ins.attributes.is_empty() {
                                    self.command_line.push_output(crate::tf!(
                                        "  Insert {:x}: no attributes.",
                                        sh.value()
                                    ).as_ref());
                                } else {
                                    for attr in &ins.attributes {
                                        self.command_line.push_output(crate::tf!(
                                            "  [{tag}] = {val}",
                                            tag = attr.tag,
                                            val = attr.get_value()
                                        ).as_ref());
                                    }
                                }
                            }
                        }
                    }
                    if !found_any {
                        self.command_line
                            .push_error(crate::t!("ATTEDIT: no Insert entities in selection.").as_ref());
                    }
                    // If tag + value supplied, mutate attributes.
                    if parts.len() == 2 && !parts[0].is_empty() {
                        let tag_up = parts[0].to_uppercase();
                        let new_val = parts[1];
                        let mut changed = 0usize;
                        self.push_undo_snapshot(i, "ATTEDIT");
                        for sh in &selected_handles {
                            if self.tabs[i].scene.is_layer_locked(*sh) {
                                continue;
                            }
                            if let Some(acadrust::EntityType::Insert(ins)) = self.tabs[i]
                                .scene
                                .document
                                .entities_mut()
                                .find(|e| e.common().handle == *sh)
                            {
                                for attr in &mut ins.attributes {
                                    if attr.tag.to_uppercase() == tag_up {
                                        attr.set_value(new_val);
                                        changed += 1;
                                    }
                                }
                            }
                        }
                        if changed > 0 {
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(crate::tf!(
                                "ATTEDIT: updated {changed} attribute(s) [{tag_up}] = {new_val}."
                            ).as_ref());
                        } else {
                            self.command_line.push_error(crate::tf!(
                                "ATTEDIT: tag '{tag_up}' not found in selection."
                            ).as_ref());
                        }
                    }
                }
            }

            // ATTDISP — control attribute display visibility.
            // ATTDISP ON   — make all AttributeDefinitions visible
            // ATTDISP OFF  — make all AttributeDefinitions invisible
            // ATTDISP NORMAL — restore: show only those without the invisible flag
            "ATTDISP" => {
                use crate::command::KeywordCommand;
                let c = KeywordCommand::new(
                    "ATTDISP",
                    "ATTDISP  attribute display  [On / Off / Normal]:",
                    vec![
                        ("On", "ON", None),
                        ("Off", "OFF", None),
                        ("Normal", "NORMAL", None),
                    ],
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("ATTDISP ") => {
                let sub = cmd.split_whitespace().nth(1).unwrap_or("").to_uppercase();
                match sub.as_str() {
                    "ON" | "OFF" | "NORMAL" => {
                        let handles: Vec<_> = self.tabs[i]
                            .scene
                            .document
                            .entities()
                            .filter_map(|entity| {
                                matches!(entity, acadrust::EntityType::AttributeDefinition(_))
                                    .then_some(entity.common().handle)
                            })
                            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                            .collect();
                        self.push_undo_snapshot(i, "ATTDISP");
                        let mut count = 0usize;
                        for handle in handles {
                            if let Some(acadrust::EntityType::AttributeDefinition(ad)) =
                                self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                match sub.as_str() {
                                    "ON" => {
                                        ad.flags.invisible = false;
                                        count += 1;
                                    }
                                    "OFF" => {
                                        ad.flags.invisible = true;
                                        count += 1;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(crate::tf!(
                            "ATTDISP {sub}: {count} attribute definition(s) updated."
                        ).as_ref());
                    }
                    _ => {
                        self.command_line
                            .push_info(crate::t!("Usage: ATTDISP ON | OFF | NORMAL").as_ref());
                    }
                }
            }

            "DONUT" => {
                use crate::modules::draw::draw::donut::DonutCommand;
                let cmd = DonutCommand::new();
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            "CIRCLE" => {
                use crate::modules::draw::draw::circle::CircleCommand;
                let new_cmd = CircleCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "CIRCLE_CD" => {
                use crate::modules::draw::draw::circle::CircleCDCommand;
                let new_cmd = CircleCDCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "CIRCLE_2P" => {
                use crate::modules::draw::draw::circle::Circle2PCommand;
                let new_cmd = Circle2PCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "CIRCLE_3P" => {
                use crate::modules::draw::draw::circle::Circle3PCommand;
                let new_cmd = Circle3PCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "CIRCLE_TTR" => {
                use crate::modules::draw::draw::circle::CircleTTRCommand;
                let new_cmd = CircleTTRCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.pre_cmd_tangent = Some(self.snapper.is_on(crate::snap::SnapType::Tangent));
                self.snapper.enabled.insert(crate::snap::SnapType::Tangent);
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "CIRCLE_TTT" => {
                use crate::modules::draw::draw::circle::CircleTTTCommand;
                let new_cmd = CircleTTTCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.pre_cmd_tangent = Some(self.snapper.is_on(crate::snap::SnapType::Tangent));
                self.snapper.enabled.insert(crate::snap::SnapType::Tangent);
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "ARC" => {
                use crate::modules::draw::draw::arc::Arc3PCommand;
                let new_cmd = Arc3PCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "ARC_3P" => {
                use crate::modules::draw::draw::arc::Arc3PCommand;
                let new_cmd = Arc3PCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "ARC_CSE" => {
                use crate::modules::draw::draw::arc::ArcCommand;
                let new_cmd = ArcCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "ARC_SCE" => {
                use crate::modules::draw::draw::arc::ArcSCECommand;
                let new_cmd = ArcSCECommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "ARC_SCA" => {
                use crate::modules::draw::draw::arc::ArcSCACommand;
                let new_cmd = ArcSCACommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "ARC_SCL" => {
                use crate::modules::draw::draw::arc::ArcSCLCommand;
                let new_cmd = ArcSCLCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "ARC_SEA" => {
                use crate::modules::draw::draw::arc::ArcSEACommand;
                let new_cmd = ArcSEACommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "ARC_SER" => {
                use crate::modules::draw::draw::arc::ArcSERCommand;
                let new_cmd = ArcSERCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "ARC_SED" => {
                use crate::modules::draw::draw::arc::ArcSEDCommand;
                let new_cmd = ArcSEDCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "ARC_CSA" => {
                use crate::modules::draw::draw::arc::ArcCSACommand;
                let new_cmd = ArcCSACommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "ARC_CSL" => {
                use crate::modules::draw::draw::arc::ArcCSLCommand;
                let new_cmd = ArcCSLCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "ARC_CONT" => {
                use crate::modules::draw::draw::arc::{continue_anchor, ArcContCommand};
                // Prefer the anchor recorded when the last line/arc was drawn (it
                // knows the true drawing-end tangent); otherwise fall back to the
                // last line/arc found in the document (e.g. after a file load).
                let seed = self.cont_anchor.or_else(|| {
                    self.tabs[i]
                        .scene
                        .document
                        .entities()
                        .filter_map(|e| continue_anchor(e, None))
                        .last()
                });
                match seed {
                    Some((s, tangent)) => {
                        let new_cmd = ArcContCommand::new(s, tangent);
                        self.command_line.push_info(&new_cmd.prompt());
                        self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                    }
                    None => {
                        self.command_line
                            .push_info(crate::t!("ARC Continue  No previous line or arc to continue.").as_ref());
                    }
                }
            }

            "RECT" | "RECTANG" => {
                use crate::modules::draw::draw::shapes::RectCommand;
                let new_cmd = RectCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                if self.ortho_mode {
                    self.rect_suppressed_ortho = true;
                    self.ortho_mode = false;
                }
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "RECT_ROT" => {
                use crate::modules::draw::draw::shapes::RectRotCommand;
                let new_cmd = RectRotCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                if self.ortho_mode {
                    self.rect_suppressed_ortho = true;
                    self.ortho_mode = false;
                }
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "RECT_CEN" => {
                use crate::modules::draw::draw::shapes::RectCenCommand;
                let new_cmd = RectCenCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                if self.ortho_mode {
                    self.rect_suppressed_ortho = true;
                    self.ortho_mode = false;
                }
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "POLY" | "POLYGON" => {
                use crate::modules::draw::draw::shapes::PolyCommand;
                let new_cmd = PolyCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "POLY_C" => {
                use crate::modules::draw::draw::shapes::PolyCCommand;
                let new_cmd = PolyCCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "POLY_E" => {
                use crate::modules::draw::draw::shapes::PolyECommand;
                let new_cmd = PolyECommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "PLINE" => {
                use crate::modules::draw::draw::polyline::PlineCommand;
                let new_cmd = PlineCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "3DPOLY" => {
                use crate::modules::draw::draw::poly3d::Poly3dCommand;
                let new_cmd = Poly3dCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "3DMESH" => {
                use crate::modules::draw::draw::mesh3d::Mesh3dCommand;
                let new_cmd = Mesh3dCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "3DFACE" => {
                use crate::modules::draw::draw::face3d::Face3dCommand;
                let new_cmd = Face3dCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "EDGE" => {
                use crate::modules::draw::draw::face3d::FaceEdgeCommand;
                let faces = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter_map(|entity| match entity {
                        acadrust::EntityType::Face3D(face) => {
                            Some((face.common.handle, face.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                let new_cmd = FaceEdgeCommand::new(faces);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            // 2D filled solid. Reached via SO / SOLID2D — the bare SOLID verb is
            // currently the shaded-display toggle (token collision tracked).
            "SOLID" | "SOLID2D" => {
                use crate::modules::draw::draw::solid2d::Solid2dCommand;
                let new_cmd = Solid2dCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "HELIX" => {
                use crate::modules::draw::draw::helix::HelixCommand;
                let new_cmd = HelixCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "TRACE" => {
                use crate::modules::draw::draw::trace::TraceCommand;
                let new_cmd = TraceCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "CENTERLINE" => {
                use crate::modules::draw::draw::centerline::CenterLineCommand;
                let settings = self.tabs[i].scene.centerline_settings();
                let new_cmd = CenterLineCommand::new(settings);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "CENTERRESET" => {
                let handles = self.tabs[i].scene.selected_handles_in_order();
                self.push_undo_snapshot(i, "CENTERRESET");
                let count = self.tabs[i].scene.reset_centerlines(&handles)
                    + self.tabs[i].scene.reset_center_marks(&handles);
                if count > 0 {
                    self.tabs[i].dirty = true;
                }
                self.command_line
                    .push_output(&crate::tf!("CENTERRESET: {count} center object(s) updated."));
            }

            "CENTERREASSOCIATE" => {
                let handles = self.tabs[i].scene.selected_handles_in_order();
                let mark_targets: Vec<_> = handles.iter().copied().filter(|handle| {
                    let Some(acadrust::EntityType::Line(line)) = self.tabs[i].scene.document.get_entity(*handle) else { return false; };
                    acadrust::entities::CenterMarkAssociation::read(&line.common.extended_data).is_some()
                }).collect();
                if mark_targets.len() == 1 && handles.len() == 1 {
                    use crate::modules::draw::draw::dimcenter::CenterMarkReassociateCommand;
                    let new_cmd = CenterMarkReassociateCommand::new(mark_targets[0]);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                    return Some(self.finish_dispatch(cmd));
                }
                self.push_undo_snapshot(i, "CENTERREASSOCIATE");
                let count = self.tabs[i].scene.set_centerline_association(&handles, true)
                    + self.tabs[i].scene.set_center_mark_association(&handles, true);
                if count > 0 {
                    self.tabs[i].dirty = true;
                }
                self.command_line
                    .push_output(&crate::tf!("CENTERREASSOCIATE: {count} center object(s) associated."));
            }

            "CENTERDISASSOCIATE" => {
                let handles = self.tabs[i].scene.selected_handles_in_order();
                self.push_undo_snapshot(i, "CENTERDISASSOCIATE");
                let count = self.tabs[i].scene.set_centerline_association(&handles, false)
                    + self.tabs[i].scene.set_center_mark_association(&handles, false);
                if count > 0 {
                    self.tabs[i].dirty = true;
                }
                self.command_line
                    .push_output(&crate::tf!("CENTERDISASSOCIATE: {count} center object(s) detached."));
            }

            "DIMCENTER" => {
                use crate::modules::draw::draw::dimcenter::DimCenterCommand;
                let new_cmd = DimCenterCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "CENTERMARK" => {
                use crate::modules::draw::draw::dimcenter::CenterMarkCommand;
                let settings = self.tabs[i].scene.centerline_settings();
                let new_cmd = CenterMarkCommand::new(settings);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "SKETCH" => {
                use crate::modules::draw::draw::sketch::SketchCommand;
                let header = &self.tabs[i].scene.document.header;
                let new_cmd = SketchCommand::new(
                    header.sketch_type,
                    header.sketch_increment,
                    header.sketch_tolerance,
                );
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "REVERSE" => {
                use crate::modules::draw::modify::reverse::ReverseCommand;
                let new_cmd = ReverseCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "MEASUREGEOM" => {
                use crate::modules::draw::inquiry::measuregeom::MeasureGeomCommand;
                let new_cmd = MeasureGeomCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            // ── Modify commands ────────────────────────────────────────────
            // MOVE works from picked points, so it already relocates entities
            // in 3D; 3DMOVE is the same operation.
            "MOVE" | "3DMOVE" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("MOVE");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::translate::MoveCommand;
                    let wires = self.tabs[i].scene.wire_models_for(&handles);
                    let new_cmd = MoveCommand::new(handles, wires);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            "COPY" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("COPY");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::copy::CopyCommand;
                    let wires = self.tabs[i].scene.wire_models_for(&handles);
                    let new_cmd = CopyCommand::new(handles, wires);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            "ROTATE" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("ROTATE");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::rotate::RotateCommand;
                    let wires = self.tabs[i].scene.wire_models_for(&handles);
                    let new_cmd = RotateCommand::new(handles, wires);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            "TORIENT" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("TORIENT");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::torient::TorientCommand;
                    let entities: Vec<_> = handles
                        .iter()
                        .filter_map(|&h| {
                            self.tabs[i]
                                .scene
                                .document
                                .get_entity(h)
                                .cloned()
                                .map(|e| (h, e))
                        })
                        .collect();
                    let cam_rot = self.tabs[i].scene.camera.borrow().rotation;
                    let right = cam_rot * glam::Vec3::X;
                    let view_twist = right.y.atan2(right.x) as f64;
                    let new_cmd = TorientCommand::new(entities, view_twist);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            "POINT" | "MULTIPOINT" => {
                use crate::modules::draw::draw::point::PointCommand;
                let new_cmd = if cmd == "MULTIPOINT" {
                    PointCommand::multiple()
                } else {
                    PointCommand::new()
                };
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "RAY" => {
                use crate::modules::draw::draw::ray::RayCommand;
                let new_cmd = RayCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "XLINE" | "CONSTRUCTIONLINE" => {
                use crate::modules::draw::draw::ray::XLineCommand;
                let new_cmd = XLineCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "HATCH" => {
                use crate::modules::draw::draw::hatch::HatchCommand;
                let working_plane = if self.tabs[i].editing_model_space() {
                    self.tabs[i].ucs_xform().working_plane()
                } else {
                    crate::command::WorkingPlane::default()
                };
                let normal = working_plane.z.normalize_or(glam::DVec3::Z);
                let elevation = working_plane.origin.dot(normal);
                let storage = crate::entities::curve::ocs_plane(
                    acadrust::types::Vector3::new(normal.x, normal.y, normal.z),
                    elevation,
                );
                let plane = crate::command::WorkingPlane::new(
                    glam::DVec3::from_array(storage.origin),
                    glam::DVec3::from_array(storage.x_axis),
                    glam::DVec3::from_array(storage.y_axis),
                );
                let boundary_sources = self.tabs[i]
                    .scene
                    .boundary_sources_on_plane(plane, 1.0e-6);
                let outlines = crate::scene::boundary_faces(&boundary_sources, 1.0e-6);
                let selected = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(handle, _)| handle)
                    .collect::<Vec<_>>();
                let inherited = selected
                    .iter()
                    .find_map(|handle| {
                        let model = self.tabs[i].scene.hatches.get(handle)?.clone();
                        let common = self.tabs[i].scene.document.get_entity(*handle)?.common();
                        Some((model, common.color.clone(), common.transparency))
                    });
                let new_cmd = HatchCommand::new(
                    outlines,
                    boundary_sources,
                    selected,
                    inherited,
                    plane,
                );
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                self.refresh_area_preview(i);
            }

            "HATCHEDIT" => {
                use crate::modules::draw::draw::hatchedit::HatcheditCommand;
                // If a single hatch is already selected, skip the pick step.
                let sel = self.tabs[i].scene.selected_entities();
                if sel.len() == 1 {
                    let (h, _) = sel[0];
                    if let Some(model) = self.tabs[i].scene.hatches.get(&h).cloned() {
                        let entity = self.tabs[i].scene.document.get_entity(h);
                        let annotative = entity.is_some_and(|entity| {
                                crate::scene::annotative::is_annotative(
                                    &self.tabs[i].scene.document,
                                    entity,
                                )
                            });
                        let (scale, angle) = match entity {
                            Some(acadrust::EntityType::Hatch(hatch)) => (
                                hatch.pattern_scale as f32,
                                hatch.pattern_angle.to_degrees() as f32,
                            ),
                            _ => (model.scale, model.angle_offset.to_degrees()),
                        };
                        let cmd = HatcheditCommand::with_handle(
                            h,
                            model.name.clone(),
                            scale,
                            angle,
                            annotative,
                        );
                        self.command_line.push_info(&cmd.prompt());
                        self.tabs[i].active_cmd = Some(Box::new(cmd));
                    } else {
                        self.command_line
                            .push_error(crate::t!("HATCHEDIT: selected entity is not a hatch.").as_ref());
                    }
                } else {
                    let cmd = HatcheditCommand::new();
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            "GRADIENT" => {
                use crate::modules::draw::draw::hatch::GradientCommand;
                let boundary_sources = self.tabs[i]
                    .scene
                    .boundary_sources_on_plane(crate::command::WorkingPlane::default(), 1.0e-6);
                let outlines = crate::scene::boundary_faces(&boundary_sources, 1.0e-6);
                let new_cmd = GradientCommand::new(outlines, boundary_sources);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "BOUNDARY" => {
                use crate::modules::draw::draw::hatch::BoundaryCommand;
                let plane = if self.tabs[i].editing_model_space() {
                    self.tabs[i].ucs_xform().working_plane()
                } else {
                    crate::command::WorkingPlane::default()
                };
                let sources = self.tabs[i]
                    .scene
                    .boundary_sources_on_plane(plane, 1.0e-6);
                let selected = self.tabs[i]
                    .scene
                    .selected_entities()
                    .iter()
                    .map(|(handle, _)| *handle)
                    .collect();
                let new_cmd = BoundaryCommand::new(sources, selected, plane);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "ELLIPSE" => {
                use crate::modules::draw::draw::ellipse::EllipseCommand;
                let new_cmd = EllipseCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "ELLIPSE_AXIS" => {
                use crate::modules::draw::draw::ellipse::EllipseAxisCommand;
                let new_cmd = EllipseAxisCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "ELLIPSE_ARC" => {
                use crate::modules::draw::draw::ellipse::EllipseArcCommand;
                let new_cmd = EllipseArcCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "SPLINE" => {
                use crate::modules::draw::draw::spline::SplineCommand;
                let new_cmd = SplineCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "SCALE" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("SCALE");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::scale::ScaleCommand;
                    let wires = self.tabs[i].scene.wire_models_for(&handles);
                    let new_cmd = ScaleCommand::new(handles, wires);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            "MIRROR" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("MIRROR");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    use crate::modules::draw::modify::mirror::MirrorCommand;
                    let (wires, text_ghosts) =
                        self.tabs[i].scene.mirror_preview_parts(&handles);
                    let mirror_text = self.tabs[i].scene.document.header.mirror_text;
                    let new_cmd = MirrorCommand::new(handles, wires, text_ghosts, mirror_text);
                    self.command_line.push_info(&new_cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(new_cmd));
                }
            }

            "ERASE" => {
                let handles: Vec<_> = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .map(|(h, _)| h)
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    use crate::modules::draw::select::SelectObjectsCommand;
                    let cmd = SelectObjectsCommand::new("ERASE");
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                } else {
                    let n = handles.len();
                    let delta_safe = self.delta_erase_safe(i, &handles);
                    let pending = self.begin_undo(i, "ERASE", handles.len(), delta_safe);
                    // Stash the erased entities so OOPS can restore them.
                    self.oops_cache = handles
                        .iter()
                        .filter_map(|h| self.tabs[i].scene.document.get_entity_arc(*h))
                        .collect();
                    self.tabs[i].scene.erase_entities(&handles);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                    self.command_line
                        .push_output(crate::tf!("{n} object(s) erased.").as_ref());
                    if let Some(pd) = pending {
                        self.commit_undo_delta(i, pd);
                    }
                }
            }

            // ── Model commands (3D primitives) ─────────────────────────────
            "CYLINDER" => {
                use crate::modules::model::cylinder_cmd::CylinderCommand;
                let new_cmd = CylinderCommand::new();
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }
            "BOX" | "WEDGE" | "CONE" | "SPHERE" | "PYRAMID" | "PYR"
            | "TORUS" => {
                use crate::modules::model::primitive_cmd::PrimitiveCommand;
                let new_cmd = PrimitiveCommand::new(cmd);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            // ── Solid booleans ─────────────────────────────────────────────
            "UNION" | "INTERSECT" => {
                use crate::modules::model::boolean_cmd::BoolOp;
                if let Some(op) = BoolOp::from_id(cmd) {
                    let solid_count = {
                        let scene = &self.tabs[i].scene;
                        scene
                            .selected_handles_in_order()
                            .into_iter()
                            .filter(|handle| !scene.is_layer_locked(*handle))
                            .filter(|handle| {
                                matches!(
                                    scene.document.get_entity(*handle),
                                    Some(acadrust::EntityType::Solid3D(_))
                                )
                            })
                            .take(2)
                            .count()
                    };
                    if solid_count < 2 {
                        use crate::modules::draw::select::SelectObjectsCommand;
                        let selection = SelectObjectsCommand::new(cmd);
                        self.command_line.push_info(&selection.prompt());
                        self.tabs[i].active_cmd = Some(Box::new(selection));
                    } else {
                        return Some(self.solid_boolean(op));
                    }
                }
            }

            "SUBTRACT" => {
                use crate::modules::model::boolean_cmd::SubtractCommand;
                let bases = {
                    let scene = &self.tabs[i].scene;
                    scene
                        .selected_handles_in_order()
                        .into_iter()
                        .filter(|handle| !scene.is_layer_locked(*handle))
                        .filter(|handle| {
                            matches!(
                                scene.document.get_entity(*handle),
                                Some(acadrust::EntityType::Solid3D(_))
                            )
                        })
                        .collect()
                };
                self.tabs[i].scene.deselect_all();
                let subtract = SubtractCommand::new(bases);
                self.command_line.push_info(&subtract.prompt());
                self.tabs[i].active_cmd = Some(Box::new(subtract));
            }

            // INTERFERE — non-destructive intersect: solid from the overlap.
            "INTERFERE" => {
                return Some(self.solid_interfere());
            }

            // FLATSHOT — flatten the selected solid's edges to 2D lines at Z=0.
            "FLATSHOT" => {
                return Some(self.solid_flatshot());
            }

            // CONVTOSURFACE — convert the selected solid(s) to surface entities.
            "CONVTOSURFACE" => {
                return Some(self.solid_convtosurface());
            }

            "POLYSOLID" => {
                use crate::modules::model::polysolid_cmd::PolysolidCommand;
                let preselected = self.tabs[i]
                    .scene
                    .selected_entities()
                    .into_iter()
                    .find(|(_, entity)| {
                        matches!(
                            entity,
                            acadrust::EntityType::Line(_)
                                | acadrust::EntityType::Arc(_)
                                | acadrust::EntityType::Circle(_)
                                | acadrust::EntityType::Ellipse(_)
                                | acadrust::EntityType::LwPolyline(_)
                                | acadrust::EntityType::Spline(_)
                        )
                    })
                    .map(|(handle, entity)| (handle, entity.clone()));
                let c = PolysolidCommand::new(preselected);
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }

            // SPLINEFIT — fit a smooth spline through the selected polyline's points.
            "SPLINEFIT" | "FITSPLINE" => {
                return Some(self.fit_spline());
            }

            // REGION — convert selected closed boundaries (closed polylines /
            // circles) into Region entities (one wire loop each).
            "REGION" | "REG" => {
                use acadrust::entities::{Region, Wire};
                use acadrust::types::Vector3;
                let mut loops: Vec<Vec<Vector3>> = Vec::new();
                for (_, e) in self.tabs[i].scene.selected_entities().iter() {
                    let supported = matches!(
                        e,
                        acadrust::EntityType::LwPolyline(pl)
                            if pl.is_closed && pl.vertices.len() >= 3
                    ) || matches!(e, acadrust::EntityType::Circle(_));
                    if supported {
                        let Some(curve) = crate::entities::curve::entity_curve(e) else {
                            continue;
                        };
                        loops.push(
                            crate::entities::curve::curve_points(&curve)
                                .into_iter()
                                .map(|point| Vector3::new(point[0], point[1], point[2]))
                                .collect(),
                        );
                    }
                }
                if loops.is_empty() {
                    self.command_line
                        .push_error(crate::t!("REGION: select closed polylines or circles.").as_ref());
                } else {
                    self.push_undo_snapshot(i, "REGION");
                    let count = loops.len();
                    for pts in loops {
                        let mut w = Wire::new();
                        let first = pts.first().copied().unwrap_or(Vector3::new(0.0, 0.0, 0.0));
                        w.points = pts;
                        let mut r = Region::new();
                        r.point_of_reference = first;
                        r.wires = vec![w];
                        r.common.layer = self.tabs[i].active_layer.clone();
                        self.tabs[i].scene.add_entity(acadrust::EntityType::Region(r));
                    }
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("REGION: created {count} region(s).").as_ref());
                }
            }

            // PYRAMID <radius> <height> [sides] keeps the direct numeric form.
            cmd if cmd.starts_with("PYRAMID ") || cmd.starts_with("PYR ") => {
                let nums: Vec<f64> = cmd
                    .split_whitespace()
                    .skip(1)
                    .filter_map(|s| s.parse::<f64>().ok())
                    .collect();
                if nums.len() >= 2 && nums[0] > 0.0 && nums[1] > 0.0 {
                    let sides = nums.get(2).map(|s| *s as usize).unwrap_or(4);
                    return Some(self.solid_pyramid(nums[0], nums[1], sides));
                }
                self.command_line
                    .push_info(crate::t!("Usage: PYRAMID <radius> <height> [sides]   (default 4 sides)").as_ref());
            }

            // SECTION [X|Y|Z] <value> — draw the cross-section outline of the solid.
            "SECTION" => {
                use crate::command::SelectThenKeywordCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenKeywordCommand::new(
                    "SECTION",
                    "SECTION  cutting-plane axis  [X / Y / Z]:",
                    vec![
                        ("X", "X", Some("SECTION  offset along X:")),
                        ("Y", "Y", Some("SECTION  offset along Y:")),
                        ("Z", "Z", Some("SECTION  offset along Z:")),
                    ],
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("SECTION ") => {
                let parts: Vec<String> = cmd
                    .split_whitespace()
                    .skip(1)
                    .map(|s| s.to_uppercase())
                    .collect();
                let (axis, val_idx) = match parts.first().map(String::as_str) {
                    Some("X") => (0, 1),
                    Some("Y") => (1, 1),
                    Some("Z") => (2, 1),
                    _ => (2, 0),
                };
                match parts.get(val_idx).and_then(|s| s.parse::<f64>().ok()) {
                    Some(v) => return Some(self.solid_section(axis, v)),
                    None => self.command_line.push_info(
                        crate::t!("Usage: SECTION [X|Y|Z] <value>   (cross-sections the selected solid)").as_ref(),
                    ),
                }
            }

            // 3DALIGN <18 numbers> — align the selected solid by 3 source→3 dest points.
            cmd if cmd == "3DALIGN"
                || cmd == "ALIGN3D"
                || cmd.starts_with("3DALIGN ")
                || cmd.starts_with("ALIGN3D ") =>
            {
                let n: Vec<f64> = cmd
                    .split_whitespace()
                    .skip(1)
                    .filter_map(|s| s.parse::<f64>().ok())
                    .collect();
                if n.len() >= 18 {
                    let src = [[n[0], n[1], n[2]], [n[3], n[4], n[5]], [n[6], n[7], n[8]]];
                    let dst = [
                        [n[9], n[10], n[11]],
                        [n[12], n[13], n[14]],
                        [n[15], n[16], n[17]],
                    ];
                    return Some(self.solid_align3d(src, dst));
                }
                self.command_line.push_info(
                    crate::t!("Usage: 3DALIGN <sx1 sy1 sz1 … sx3 sy3 sz3  dx1 dy1 dz1 … dx3 dy3 dz3>  (18 numbers: 3 source then 3 destination points)").as_ref(),
                );
            }

            // 3DMIRROR [X|Y|Z] — add a mirror of the selected solid across a plane.
            "3DMIRROR" | "MIRROR3D" => {
                use crate::command::SelectThenKeywordCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenKeywordCommand::new(
                    "3DMIRROR",
                    "3DMIRROR  mirror plane  [X / Y / Z]:",
                    vec![("X", "X", None), ("Y", "Y", None), ("Z", "Z", None)],
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("3DMIRROR ") || cmd.starts_with("MIRROR3D ") => {
                let parts: Vec<String> = cmd
                    .split_whitespace()
                    .skip(1)
                    .map(|s| s.to_uppercase())
                    .collect();
                let axis = match parts.first().map(String::as_str) {
                    Some("X") => 0,
                    Some("Y") => 1,
                    Some("Z") => 2,
                    _ => {
                        self.command_line.push_info(
                            crate::t!("Usage: 3DMIRROR [X|Y|Z]   (mirrors the selected solid across that plane)").as_ref(),
                        );
                        return None;
                    }
                };
                return Some(self.solid_mirror3d(axis));
            }

            // 3DROTATE [X|Y|Z] <angle> — rotate the selected solid about an axis.
            "3DROTATE" | "ROTATE3D" => {
                use crate::command::SelectThenKeywordCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenKeywordCommand::new(
                    "3DROTATE",
                    "3DROTATE  rotation axis  [X / Y / Z]:",
                    vec![
                        ("X", "X", Some("3DROTATE  angle in degrees:")),
                        ("Y", "Y", Some("3DROTATE  angle in degrees:")),
                        ("Z", "Z", Some("3DROTATE  angle in degrees:")),
                    ],
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("3DROTATE ") || cmd.starts_with("ROTATE3D ") => {
                let parts: Vec<String> = cmd
                    .split_whitespace()
                    .skip(1)
                    .map(|s| s.to_uppercase())
                    .collect();
                let axis = match parts.first().map(String::as_str) {
                    Some("X") => 0,
                    Some("Y") => 1,
                    _ => 2,
                };
                let angle: Option<f64> = parts.iter().find_map(|s| s.parse::<f64>().ok());
                match angle {
                    Some(a) => return Some(self.solid_rotate3d(axis, a)),
                    None => self.command_line.push_info(
                        crate::t!("Usage: 3DROTATE [X|Y|Z] <angle>   (rotates the selected solid)").as_ref(),
                    ),
                }
            }

            // SLICE [X|Y|Z] <value> [TOP|BOTTOM] — cut the selected solid with an
            // axis-aligned plane, keeping the lower half by default.
            "SLICE" | "SL" => {
                use crate::command::SelectThenKeywordCommand;
                let has_sel = !self.tabs[i].scene.selected_entities().is_empty();
                let c = SelectThenKeywordCommand::new(
                    "SLICE",
                    "SLICE  cutting-plane axis  [X / Y / Z]  (add TOP/BOTTOM by typing):",
                    vec![
                        ("X", "X", Some("SLICE  offset along X:")),
                        ("Y", "Y", Some("SLICE  offset along Y:")),
                        ("Z", "Z", Some("SLICE  offset along Z:")),
                    ],
                    has_sel,
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("SLICE ") || cmd.starts_with("SL ") => {
                let parts: Vec<String> = cmd
                    .split_whitespace()
                    .skip(1)
                    .map(|s| s.to_uppercase())
                    .collect();
                let (axis, val_idx) = match parts.first().map(String::as_str) {
                    Some("X") => (0, 1),
                    Some("Y") => (1, 1),
                    Some("Z") => (2, 1),
                    _ => (2, 0), // default Z plane
                };
                let value: Option<f64> = parts.get(val_idx).and_then(|s| s.parse().ok());
                let keep_low = !parts.iter().any(|s| s == "TOP");
                match value {
                    Some(v) => return Some(self.solid_slice(axis, v, keep_low)),
                    None => self.command_line.push_info(
                        crate::t!("Usage: SLICE [X|Y|Z] <value> [TOP|BOTTOM]   (cuts the selected solid)").as_ref(),
                    ),
                }
            }

            // ── Annotate commands ──────────────────────────────────────────
            "TEXT" => {
                use crate::modules::annotate::text::TextCommand;
                let (defaults, styles, annotation_multiplier) = {
                    let scene = &self.tabs[i].scene;
                    let annotation_multiplier = scene.creation_annotation_multiplier();
                    let defaults =
                        crate::scene::creation_style::current_text_defaults(&scene.document);
                    let styles = scene.document.text_styles.iter().cloned().collect();
                    (defaults, styles, annotation_multiplier)
                };
                let new_cmd =
                    TextCommand::with_defaults(defaults, styles, annotation_multiplier);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "DDEDIT" => {
                use crate::modules::annotate::ddedit::DdeditCommand;
                // A single text entity already selected opens its in-place
                // editor directly; otherwise prompt for a pick.
                let sel = self.tabs[i].scene.selected_entities();
                let editable = (sel.len() == 1).then(|| sel[0].0).filter(|h| {
                    self.tabs[i].scene.document.get_entity(*h).is_some_and(|e| {
                        super::super::text_inline::read_text_field(e).is_some()
                            || matches!(e, acadrust::EntityType::Leader(_))
                    })
                });
                if let Some(h) = editable {
                    return Some(self.begin_text_edit(h));
                }
                if sel.len() == 1 {
                    self.command_line
                        .push_error(crate::t!("DDEDIT: selected entity is not text.").as_ref());
                } else {
                    let cmd = DdeditCommand::new();
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
            }

            "MTEXT" => {
                use crate::modules::annotate::mtext::MTextCommand;
                let height = crate::scene::creation_style::current_text_defaults(
                    &self.tabs[i].scene.document,
                )
                .height;
                let style = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_text_style_name
                    .clone();
                let new_cmd = MTextCommand::with_defaults(height, style);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "TEXTEDIT" | "TEDIT" => {
                use crate::modules::annotate::textedit::TexteditCommand;
                let mode_str = if self.texteditmode {
                    "Single"
                } else {
                    "Multiple"
                };
                self.command_line
                    .push_output(crate::tf!("Current settings: Edit mode = {}", mode_str).as_ref());
                let new_cmd = TexteditCommand::new(self.texteditmode);
                self.command_line.push_info(&new_cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(new_cmd));
            }

            "TEXTEDITMODE" => {
                use crate::modules::annotate::textedit::TexteditmodeCommand;
                let cmd = TexteditmodeCommand::new(self.texteditmode);
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }
}
