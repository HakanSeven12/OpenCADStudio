use super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_layerprops(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            // ── LAYER management ─────────────────────────────────────────
            cmd if cmd == "LAYER" || cmd.starts_with("LAYER ") || cmd.starts_with("LA ") => {
                use acadrust::tables::Layer;
                let raw_rest = if cmd.starts_with("LAYER ") {
                    cmd.trim_start_matches("LAYER ").trim()
                } else if cmd.starts_with("LA ") {
                    cmd.trim_start_matches("LA ").trim()
                } else {
                    ""
                };
                let parts: Vec<&str> = raw_rest.split_whitespace().collect();
                let sub = parts.get(0).map(|s| s.to_uppercase()).unwrap_or_default();
                match sub.as_str() {
                    "" | "LIST" | "?" => {
                        let info: Vec<String> = self.tabs[i]
                            .scene
                            .document
                            .layers
                            .iter()
                            .map(|l| {
                                let state = if l.flags.frozen {
                                    "frozen"
                                } else if l.flags.off {
                                    "off"
                                } else if l.flags.locked {
                                    "locked"
                                } else {
                                    "on"
                                };
                                format!("{}({})", l.name, state)
                            })
                            .collect();
                        self.command_line
                            .push_output(crate::tf!("Layers: {}", info.join(", ")).as_ref());
                    }
                    "NEW" | "N" => {
                        let name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: LAYER NEW <name>").as_ref());
                        } else if self.tabs[i].scene.document.layers.contains(&name) {
                            self.command_line
                                .push_error(crate::tf!("LAYER: '{}' already exists.", name).as_ref());
                        } else {
                            let mut layer = Layer::new(&name);
                            // Allocate a unique handle so the layer survives a
                            // DWG save (handle-based format; issue #67).
                            layer.handle = self.tabs[i].scene.document.allocate_handle();
                            let _ = self.tabs[i].scene.document.layers.add(layer);
                            self.push_undo_snapshot(i, "LAYER NEW");
                            self.tabs[i].dirty = true;
                            self.command_line
                                .push_output(crate::tf!("LAYER: '{}' created.", name).as_ref());
                        }
                    }
                    "ON" => {
                        for name in &parts[1..] {
                            if let Some(l) = self.tabs[i].scene.document.layers.get_mut(name) {
                                l.flags.off = false;
                                l.flags.frozen = false;
                            }
                        }
                        self.push_undo_snapshot(i, "LAYER ON");
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(crate::t!("LAYER: layers turned on.").as_ref());
                    }
                    "OFF" => {
                        for name in &parts[1..] {
                            if let Some(l) = self.tabs[i].scene.document.layers.get_mut(name) {
                                l.flags.off = true;
                            }
                        }
                        self.push_undo_snapshot(i, "LAYER OFF");
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(crate::t!("LAYER: layers turned off.").as_ref());
                    }
                    "FREEZE" | "FR" => {
                        for name in &parts[1..] {
                            if let Some(l) = self.tabs[i].scene.document.layers.get_mut(name) {
                                l.flags.frozen = true;
                            }
                        }
                        self.push_undo_snapshot(i, "LAYER FREEZE");
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(crate::t!("LAYER: layers frozen.").as_ref());
                    }
                    "THAW" | "TH" => {
                        for name in &parts[1..] {
                            if let Some(l) = self.tabs[i].scene.document.layers.get_mut(name) {
                                l.flags.frozen = false;
                            }
                        }
                        self.push_undo_snapshot(i, "LAYER THAW");
                        self.tabs[i].dirty = true;
                        self.command_line.push_output(crate::t!("LAYER: layers thawed.").as_ref());
                    }
                    "LOCK" | "LO" => {
                        for name in &parts[1..] {
                            if let Some(l) = self.tabs[i].scene.document.layers.get_mut(name) {
                                l.flags.locked = true;
                            }
                        }
                        self.push_undo_snapshot(i, "LAYER LOCK");
                        self.tabs[i].dirty = true;
                        self.refresh_properties();
                        self.command_line.push_output(crate::t!("LAYER: layers locked.").as_ref());
                    }
                    "UNLOCK" | "UL" => {
                        for name in &parts[1..] {
                            if let Some(l) = self.tabs[i].scene.document.layers.get_mut(name) {
                                l.flags.locked = false;
                            }
                        }
                        self.push_undo_snapshot(i, "LAYER UNLOCK");
                        self.tabs[i].dirty = true;
                        self.refresh_properties();
                        self.command_line.push_output(crate::t!("LAYER: layers unlocked.").as_ref());
                    }
                    "COLOR" | "C" => {
                        // LAYER COLOR <name> <aci_index>
                        let layer_name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        let color_str = parts.get(2).map(|s| s.trim()).unwrap_or("");
                        if let Ok(idx) = color_str.parse::<i16>() {
                            if let Some(l) = self.tabs[i].scene.document.layers.get_mut(&layer_name)
                            {
                                l.color = acadrust::types::Color::from_index(idx);
                                l.color_name = None;
                                l.book_name = None;
                                self.push_undo_snapshot(i, "LAYER COLOR");
                                self.tabs[i].dirty = true;
                                // By-layer colour is baked into every wire on
                                // this layer. The dependency index maps block
                                // children back to their top-level INSERTs, so
                                // unrelated entities stay warm.
                                self.tabs[i]
                                    .scene
                                    .invalidate_layer_dependencies(std::slice::from_ref(
                                        &layer_name,
                                    ));
                                self.command_line.push_output(crate::tf!(
                                    "LAYER: '{}' color set to ACI {}.",
                                    layer_name, idx
                                ).as_ref());
                            } else {
                                self.command_line
                                    .push_error(crate::tf!("LAYER: '{}' not found.", layer_name).as_ref());
                            }
                        } else {
                            self.command_line
                                .push_error(crate::t!("Usage: LAYER COLOR <name> <aci_index>").as_ref());
                        }
                    }
                    "SET" | "S" | "CURRENT" => {
                        let name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        if self.tabs[i].scene.document.layers.contains(&name) {
                            self.tabs[i].layers.current_layer = name.clone();
                            self.command_line
                                .push_output(crate::tf!("LAYER: current layer set to '{}'.", name).as_ref());
                        } else {
                            self.command_line
                                .push_error(crate::tf!("LAYER: '{}' not found.", name).as_ref());
                        }
                    }
                    _ => {
                        self.command_line.push_info(
                            crate::t!("Usage: LAYER LIST | NEW <name> | ON/OFF/FREEZE/THAW/LOCK/UNLOCK <name> | COLOR <name> <aci> | SET <name>").as_ref()
                        );
                    }
                }
            }

            // Bare UCS → interactive front-end (option then value as steps), so
            // `UCS Z 90` is typable in the command line and works headlessly.
            // The front-end delegates back to the inline handler below. (#169)
            "UCS" => {
                use crate::modules::view::ucs_cmd::UcsCommand;
                let cmd_obj = UcsCommand::new();
                self.command_line.push_info(&cmd_obj.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd_obj));
            }

            // ── UCS management (inline `UCS <option> …`) ─────────────────────
            cmd if cmd.starts_with("UCS ") => {
                use super::super::helpers::{ucs_rotated_z, ucs_to_wcs, ucs_z_axis};
                use acadrust::tables::Ucs;
                use acadrust::types::Vector3;
                let parts: Vec<&str> = cmd.splitn(4, ' ').collect();
                let sub = parts.get(1).map(|s| s.to_uppercase()).unwrap_or_default();
                let mut active_changed = false;
                match sub.as_str() {
                    "" | "LIST" | "?" => {
                        let active_name = self.tabs[i]
                            .active_ucs
                            .as_ref()
                            .map(|u| u.name.clone())
                            .unwrap_or_else(|| "WCS".into());
                        let names: Vec<String> = self.tabs[i]
                            .scene
                            .document
                            .ucss
                            .iter()
                            .map(|u| u.name.clone())
                            .collect();
                        if names.is_empty() {
                            self.command_line.push_output(crate::tf!(
                                "Active UCS: {}  |  No named UCSs defined.",
                                active_name
                            ).as_ref());
                        } else {
                            self.command_line.push_output(crate::tf!(
                                "Active UCS: {}  |  Named: {}",
                                active_name,
                                names.join(", ")
                            ).as_ref());
                        }
                    }
                    "SAVE" | "S" => {
                        let name = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: UCS SAVE <name>").as_ref());
                        } else {
                            // Save the current active UCS under this name.
                            let mut ucs = match &self.tabs[i].active_ucs {
                                Some(u) => {
                                    let mut saved = u.clone();
                                    saved.name = name.clone();
                                    saved
                                }
                                None => Ucs::new(&name), // save WCS (identity)
                            };
                            ucs.handle = self.tabs[i]
                                .scene
                                .document
                                .ucss
                                .get(&name)
                                .map(|existing| existing.handle)
                                .filter(|handle| !handle.is_null())
                                .unwrap_or_else(|| {
                                    self.tabs[i].scene.document.allocate_handle()
                                });
                            self.tabs[i]
                                .scene
                                .document
                                .ucss
                                .add_or_replace(ucs.clone());
                            self.tabs[i].active_ucs = Some(ucs);
                            active_changed = true;
                            self.tabs[i].dirty = true;
                            self.command_line
                                .push_output(crate::tf!("UCS '{}' saved.", name).as_ref());
                        }
                    }
                    "DELETE" | "DEL" | "D" => {
                        let name = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: UCS DELETE <name>").as_ref());
                        } else if let Some(removed) =
                            self.tabs[i].scene.document.ucss.remove(&name)
                        {
                            let removed_handle = removed.handle;
                            for entity in self.tabs[i].scene.document.entities_mut() {
                                if let acadrust::EntityType::Viewport(viewport) = entity {
                                    if viewport.ucs_handle == removed_handle {
                                        viewport.ucs_handle = acadrust::Handle::NULL;
                                    }
                                }
                            }
                            let active_matches = self.tabs[i].active_ucs.as_ref().is_some_and(|ucs| {
                                (!removed_handle.is_null() && ucs.handle == removed_handle)
                                    || ucs.name.eq_ignore_ascii_case(&name)
                            });
                            if active_matches {
                                if let Some(active) = self.tabs[i].active_ucs.as_mut() {
                                    active.name = "*ACTIVE*".to_string();
                                    active.handle = acadrust::Handle::NULL;
                                    active.named_ucs_handle = acadrust::Handle::NULL;
                                    active.base_ucs_handle = acadrust::Handle::NULL;
                                }
                                active_changed = true;
                            }
                            self.tabs[i].dirty = true;
                            self.command_line
                                .push_output(crate::tf!("UCS '{}' deleted.", name).as_ref());
                        } else {
                            self.command_line
                                .push_error(crate::tf!("UCS '{}' not found.", name).as_ref());
                        }
                    }
                    "W" | "WORLD" => {
                        self.tabs[i].active_ucs = None;
                        active_changed = true;
                        self.command_line
                            .push_output(crate::t!("UCS reset to World Coordinate System.").as_ref());
                    }
                    "VIEW" | "V" => {
                        let rotation = self.tabs[i].scene.active_camera_rotation();
                        let origin = self.tabs[i]
                            .active_ucs
                            .as_ref()
                            .map(|ucs| ucs.origin)
                            .unwrap_or(Vector3::ZERO);
                        let x = rotation * glam::Vec3::X;
                        let y = rotation * glam::Vec3::Y;
                        let mut ucs = Ucs::new("*VIEW*");
                        ucs.origin = origin;
                        ucs.x_axis = Vector3::new(x.x as f64, x.y as f64, x.z as f64);
                        ucs.y_axis = Vector3::new(y.x as f64, y.y as f64, y.z as f64);
                        self.tabs[i].active_ucs = Some(ucs);
                        active_changed = true;
                        self.command_line
                            .push_output(crate::t!("UCS aligned to the current view.").as_ref());
                    }
                    "3POINTW" => {
                        let raw = parts.get(2).copied().unwrap_or("");
                        let points: Vec<glam::DVec3> = raw
                            .split('|')
                            .filter_map(|value| {
                                super::super::helpers::parse_coord(value).map(|(point, _)| point)
                            })
                            .collect();
                        if points.len() == 3 {
                            let x = (points[1] - points[0]).normalize_or_zero();
                            let toward_y = points[2] - points[0];
                            let z = x.cross(toward_y).normalize_or_zero();
                            let y = z.cross(x).normalize_or_zero();
                            if x.length_squared() > 1e-12
                                && y.length_squared() > 1e-12
                                && z.length_squared() > 1e-12
                            {
                                let mut ucs = Ucs::new("*ACTIVE*");
                                ucs.origin = Vector3::new(points[0].x, points[0].y, points[0].z);
                                ucs.x_axis = Vector3::new(x.x, x.y, x.z);
                                ucs.y_axis = Vector3::new(y.x, y.y, y.z);
                                self.tabs[i].active_ucs = Some(ucs);
                                active_changed = true;
                                self.command_line.push_output(
                                    crate::t!("UCS defined from three points.").as_ref(),
                                );
                            } else {
                                self.command_line.push_error(
                                    crate::t!("UCS points must define two non-collinear axes.")
                                        .as_ref(),
                                );
                            }
                        } else {
                            self.command_line.push_error(
                                crate::t!("UCS requires origin, X-axis point and XY-plane point.")
                                    .as_ref(),
                            );
                        }
                    }
                    // UCS ORIGIN x,y,z  — shift the active UCS origin, keep axes
                    "ORIGIN" | "O" | "ORIGINW" => {
                        let coord_str = parts.get(2).copied().unwrap_or("");
                        if let Some((pt, _)) = super::super::helpers::parse_coord(coord_str) {
                            // `pt` is in current UCS space; convert to WCS.
                            // The @/# relative-coordinate prefix is ignored
                            // here — a UCS origin is always absolute.
                            let wcs_origin = if sub == "ORIGINW" {
                                pt
                            } else if let Some(ref ucs) = self.tabs[i].active_ucs {
                                ucs_to_wcs(pt, ucs)
                            } else {
                                pt
                            };
                            let ucs = self.tabs[i]
                                .active_ucs
                                .get_or_insert_with(|| Ucs::new("*ACTIVE*"));
                            ucs.origin = Vector3::new(
                                wcs_origin.x as f64,
                                wcs_origin.y as f64,
                                wcs_origin.z as f64,
                            );
                            active_changed = true;
                            self.command_line.push_output(crate::tf!(
                                "UCS origin set to ({:.4}, {:.4}, {:.4}).",
                                wcs_origin.x, wcs_origin.y, wcs_origin.z
                            ).as_ref());
                        } else {
                            self.command_line.push_error(crate::t!("Usage: UCS ORIGIN x,y,z").as_ref());
                        }
                    }
                    // UCS Z angle  — rotate active UCS around its Z axis by degrees
                    "Z" => {
                        let deg: Option<f32> = parts.get(2).and_then(|s| s.trim().parse().ok());
                        if let Some(angle_deg) = deg {
                            let rad = angle_deg.to_radians();
                            let current = self.tabs[i].active_ucs.as_ref();
                            let origin = current
                                .map(|u| {
                                    glam::DVec3::new(u.origin.x, u.origin.y, u.origin.z)
                                })
                                .unwrap_or(glam::DVec3::ZERO);
                            let mut new_ucs = ucs_rotated_z(origin, rad);
                            // If already had axes, compose rotation on top
                            if let Some(ref ucs) = self.tabs[i].active_ucs {
                                let old_x = glam::Vec3::new(
                                    ucs.x_axis.x as f32,
                                    ucs.x_axis.y as f32,
                                    ucs.x_axis.z as f32,
                                );
                                let old_y = glam::Vec3::new(
                                    ucs.y_axis.x as f32,
                                    ucs.y_axis.y as f32,
                                    ucs.y_axis.z as f32,
                                );
                                let z_ax = ucs_z_axis(ucs).as_vec3();
                                let rot = glam::Quat::from_axis_angle(z_ax, rad);
                                let nx = rot * old_x;
                                let ny = rot * old_y;
                                new_ucs.x_axis =
                                    Vector3::new(nx.x as f64, nx.y as f64, nx.z as f64);
                                new_ucs.y_axis =
                                    Vector3::new(ny.x as f64, ny.y as f64, ny.z as f64);
                            }
                            self.tabs[i].active_ucs = Some(new_ucs);
                            active_changed = true;
                            self.command_line
                                .push_output(crate::tf!("UCS rotated {:.2}° around Z.", angle_deg).as_ref());
                        } else {
                            self.command_line.push_error(crate::t!("Usage: UCS Z <angle_degrees>").as_ref());
                        }
                    }
                    // UCS X angle  — rotate around current UCS X axis
                    "X" => {
                        let deg: Option<f32> = parts.get(2).and_then(|s| s.trim().parse().ok());
                        if let Some(angle_deg) = deg {
                            let rad = angle_deg.to_radians();
                            let ucs = self.tabs[i]
                                .active_ucs
                                .get_or_insert_with(|| Ucs::new("*ACTIVE*"));
                            let x_ax = glam::Vec3::new(
                                ucs.x_axis.x as f32,
                                ucs.x_axis.y as f32,
                                ucs.x_axis.z as f32,
                            );
                            let old_y = glam::Vec3::new(
                                ucs.y_axis.x as f32,
                                ucs.y_axis.y as f32,
                                ucs.y_axis.z as f32,
                            );
                            let rot = glam::Quat::from_axis_angle(x_ax, rad);
                            let ny = rot * old_y;
                            ucs.y_axis = Vector3::new(ny.x as f64, ny.y as f64, ny.z as f64);
                            active_changed = true;
                            self.command_line
                                .push_output(crate::tf!("UCS rotated {:.2}° around X.", angle_deg).as_ref());
                        } else {
                            self.command_line.push_error(crate::t!("Usage: UCS X <angle_degrees>").as_ref());
                        }
                    }
                    // UCS Y angle  — rotate around current UCS Y axis
                    "Y" => {
                        let deg: Option<f32> = parts.get(2).and_then(|s| s.trim().parse().ok());
                        if let Some(angle_deg) = deg {
                            let rad = angle_deg.to_radians();
                            let ucs = self.tabs[i]
                                .active_ucs
                                .get_or_insert_with(|| Ucs::new("*ACTIVE*"));
                            let y_ax = glam::Vec3::new(
                                ucs.y_axis.x as f32,
                                ucs.y_axis.y as f32,
                                ucs.y_axis.z as f32,
                            );
                            let old_x = glam::Vec3::new(
                                ucs.x_axis.x as f32,
                                ucs.x_axis.y as f32,
                                ucs.x_axis.z as f32,
                            );
                            let rot = glam::Quat::from_axis_angle(y_ax, rad);
                            let nx = rot * old_x;
                            ucs.x_axis = Vector3::new(nx.x as f64, nx.y as f64, nx.z as f64);
                            active_changed = true;
                            self.command_line
                                .push_output(crate::tf!("UCS rotated {:.2}° around Y.", angle_deg).as_ref());
                        } else {
                            self.command_line.push_error(crate::t!("Usage: UCS Y <angle_degrees>").as_ref());
                        }
                    }
                    _ => {
                        // UCS <name> — activate a named UCS
                        let name = sub.clone();
                        if let Some(named) = self.tabs[i].scene.document.ucss.get(&name).cloned() {
                            self.tabs[i].active_ucs = Some(named);
                            active_changed = true;
                            self.command_line
                                .push_output(crate::tf!("UCS '{}' activated.", name).as_ref());
                        } else {
                            self.command_line.push_error(crate::tf!(
                                "UCS '{}' not found.  Usage: UCS LIST | SAVE <name> | DELETE <name> | W | ORIGIN x,y,z | X/Y/Z <angle>",
                                name
                            ).as_ref());
                        }
                    }
                }
                if active_changed {
                    self.commit_active_ucs_change(i, "UCS");
                }
            }

            // ── Named Views (VIEW command) ────────────────────────────────
            // PLAN — plan view of a UCS (#326). Shows its options right on
            // activation instead of flipping the view like a cube click; the
            // view only changes once an option (or the Current default) runs.
            "PLAN" => {
                use crate::command::KeywordCommand;
                let c = KeywordCommand::new(
                    "PLAN",
                    "PLAN  [Current ucs / Ucs / World] <Current>:",
                    vec![
                        ("Current ucs", "CURRENT", None),
                        ("Ucs", "UCS", Some("PLAN UCS  ucs name:")),
                        ("World", "WORLD", None),
                    ],
                )
                .with_default("CURRENT");
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("PLAN ") => {
                let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
                let sub = parts.get(1).map(|s| s.to_uppercase()).unwrap_or_default();
                match sub.as_str() {
                    "" | "CURRENT" | "C" => {
                        let r = self.tabs[i].scene.viewcube_ucs_mat();
                        self.plan_snap(i, r);
                        self.command_line.push_output(crate::t!("PLAN: current UCS plan view.").as_ref());
                    }
                    "WORLD" | "W" => {
                        self.plan_snap(i, glam::Mat4::IDENTITY);
                        self.command_line.push_output(crate::t!("PLAN: world plan view.").as_ref());
                    }
                    "UCS" | "U" => {
                        let name = parts.get(2).map(|s| s.trim()).unwrap_or("");
                        if name.is_empty() {
                            self.command_line.push_info(crate::t!("Usage: PLAN UCS <name>").as_ref());
                        } else {
                            let ucs = self.tabs[i]
                                .scene
                                .document
                                .ucss
                                .iter()
                                .find(|u| u.name.eq_ignore_ascii_case(name))
                                .cloned();
                            match ucs {
                                Some(u) => {
                                    let r = crate::app::helpers::UcsXform::from_ucs(&u)
                                        .rotation_mat();
                                    self.plan_snap(i, r);
                                    self.command_line.push_output(crate::tf!(
                                        "PLAN: plan view of UCS '{}'.",
                                        u.name
                                    ).as_ref());
                                }
                                None => self
                                    .command_line
                                    .push_error(crate::tf!("UCS '{name}' not found.").as_ref()),
                            }
                        }
                    }
                    _ => self
                        .command_line
                        .push_info(crate::t!("Usage: PLAN [CURRENT / UCS <name> / WORLD]").as_ref()),
                }
            }

            "VIEW" => {
                use crate::command::KeywordCommand;
                let c = KeywordCommand::new(
                    "VIEW",
                    "VIEW  [List / Save / Restore / Delete]:",
                    vec![
                        ("List", "LIST", None),
                        ("Save", "SAVE", Some("VIEW SAVE  new view name:")),
                        ("Restore", "RESTORE", Some("VIEW RESTORE  view name:")),
                        ("Delete", "DELETE", Some("VIEW DELETE  view name:")),
                    ],
                );
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
            }
            cmd if cmd.starts_with("VIEW ") => {
                let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
                let sub = parts.get(1).map(|s| s.to_uppercase()).unwrap_or_default();
                match sub.as_str() {
                    "" | "LIST" | "?" => {
                        let views: Vec<String> = self.tabs[i]
                            .scene
                            .document
                            .views
                            .iter()
                            .map(|v| v.name.clone())
                            .collect();
                        if views.is_empty() {
                            self.command_line.push_output(crate::t!("No named views saved.").as_ref());
                        } else {
                            self.command_line
                                .push_output(crate::tf!("Named views: {}", views.join(", ")).as_ref());
                        }
                    }
                    "SAVE" | "S" => {
                        let name = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: VIEW SAVE <name>").as_ref());
                        } else {
                            let new_view = self.tabs[i].scene.current_as_named_view(&name);
                            self.tabs[i].scene.document.views.add_or_replace(new_view);
                            self.command_line
                                .push_output(crate::tf!("View '{}' saved.", name).as_ref());
                        }
                    }
                    "DELETE" | "DEL" | "D" => {
                        let name = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: VIEW DELETE <name>").as_ref());
                        } else {
                            if self.tabs[i].scene.document.views.remove(&name).is_some() {
                                self.command_line
                                    .push_output(crate::tf!("View '{}' deleted.", name).as_ref());
                            } else {
                                self.command_line
                                    .push_error(crate::tf!("View '{}' not found.", name).as_ref());
                            }
                        }
                    }
                    "RESTORE" | "R" => {
                        let name = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: VIEW RESTORE <name>").as_ref());
                        } else {
                            let found = self.tabs[i].scene.document.views.get(&name).cloned();
                            if let Some(v) = found {
                                self.tabs[i].scene.restore_named_view(&v);
                                self.command_line
                                    .push_output(crate::tf!("View '{}' restored.", v.name).as_ref());
                            } else {
                                self.command_line
                                    .push_error(crate::tf!("View '{}' not found.", name).as_ref());
                            }
                        }
                    }
                    // Standard orientation presets — snap the camera to a world
                    // axis view (these names take precedence over a same-named
                    // saved view, matching the standard orientation behaviour).
                    "TOP" | "FRONT" | "BACK" | "LEFT" | "RIGHT" | "BOTTOM" => {
                        use crate::scene::pipeline::viewcube::{
                            FACE_BACK, FACE_BOTTOM, FACE_FRONT, FACE_LEFT, FACE_RIGHT, FACE_TOP,
                        };
                        let face = match sub.as_str() {
                            "TOP" => FACE_TOP,
                            "BOTTOM" => FACE_BOTTOM,
                            "FRONT" => FACE_FRONT,
                            "BACK" => FACE_BACK,
                            "RIGHT" => FACE_RIGHT,
                            _ => FACE_LEFT,
                        };
                        return Some(Task::done(Message::ViewCubeSnapWorld(
                            crate::scene::CubeRegion::Face(face),
                        )));
                    }
                    "ISO" | "ISOMETRIC" | "SWISO" => {
                        return Some(Task::done(Message::ViewCubeHome));
                    }
                    // VIEW <name> shortcut for restore
                    _ => {
                        let name = sub.clone();
                        let found = self.tabs[i].scene.document.views.get(&name).cloned();
                        if let Some(v) = found {
                            self.tabs[i].scene.restore_named_view(&v);
                            self.command_line
                                .push_output(crate::tf!("View '{}' restored.", v.name).as_ref());
                        } else {
                            self.command_line.push_error(
                                crate::t!("Usage: VIEW LIST | VIEW SAVE <name> | VIEW RESTORE <name> | VIEW DELETE <name>").as_ref()
                            );
                        }
                    }
                }
            }

            // ── DimStyle management ───────────────────────────────────────
            // TABLESTYLE — Table Style Manager.
            cmd if cmd == "TABLESTYLE" || cmd == "TS" || cmd.starts_with("TABLESTYLE ") => {
                use acadrust::objects::{ObjectType, TableStyle};
                let raw_rest = cmd.split_once(' ').map(|(_, r)| r.trim()).unwrap_or("");
                let parts: Vec<&str> = raw_rest.split_whitespace().collect();
                let sub = parts.first().map(|s| s.to_uppercase()).unwrap_or_default();
                match sub.as_str() {
                    "" | "DIALOG" | "UI" => {
                        return Some(Task::done(Message::TableStyleDialogOpen));
                    }
                    "LIST" | "?" => {
                        let doc = &self.tabs[i].scene.document;
                        let styles: Vec<String> = doc
                            .objects
                            .values()
                            .filter_map(|o| {
                                if let ObjectType::TableStyle(s) = o {
                                    Some(s)
                                } else {
                                    None
                                }
                            })
                            .map(|s| {
                                format!(
                                    "{}  (h_margin:{:.2} v_margin:{:.2})",
                                    s.name, s.horizontal_margin, s.vertical_margin
                                )
                            })
                            .collect();
                        if styles.is_empty() {
                            self.command_line.push_output(crate::t!("No table styles.").as_ref());
                        } else {
                            self.command_line
                                .push_output(crate::tf!("TableStyles:\n  {}", styles.join("\n  ")).as_ref());
                        }
                    }
                    "NEW" | "N" => {
                        let name = parts.get(1).copied().unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: TABLESTYLE NEW <name>").as_ref());
                        } else {
                            let doc = &self.tabs[i].scene.document;
                            let exists = doc.objects.values().any(|o| {
                                matches!(o, ObjectType::TableStyle(s) if s.name.eq_ignore_ascii_case(&name))
                            });
                            if exists {
                                self.command_line
                                    .push_error(crate::tf!("TABLESTYLE: '{}' already exists.", name).as_ref());
                            } else {
                                self.push_undo_snapshot(i, "TABLESTYLE NEW");
                                let mut style = TableStyle::standard();
                                style.name = name.clone();
                                let nh = acadrust::Handle::new(
                                    self.tabs[i].scene.document.next_handle(),
                                );
                                style.handle = nh;
                                self.tabs[i]
                                    .scene
                                    .document
                                    .objects
                                    .insert(nh, ObjectType::TableStyle(style));
                                self.tabs[i].dirty = true;
                                self.command_line
                                    .push_output(crate::tf!("TABLESTYLE: '{}' created.", name).as_ref());
                            }
                        }
                    }
                    _ => {
                        self.command_line
                            .push_error(crate::t!("Usage: TABLESTYLE [LIST|NEW <name>]").as_ref());
                    }
                }
            }

            // MLSTYLE — Multiline Style Manager.
            // Usage:
            //   MLSTYLE                — open dialog
            //   MLSTYLE LIST / ?       — list all multiline styles
            //   MLSTYLE NEW <name>     — create a new style
            //   MLSTYLE SET <name>     — set current multiline style
            //   MLSTYLE DEL <name>     — delete a style (not Standard)
            cmd if cmd == "MLSTYLE" || cmd.starts_with("MLSTYLE ") => {
                use acadrust::objects::{MLineStyle, ObjectType};
                let raw_rest = cmd.split_once(' ').map(|(_, r)| r.trim()).unwrap_or("");
                let parts: Vec<&str> = raw_rest.split_whitespace().collect();
                let sub = parts.first().map(|s| s.to_uppercase()).unwrap_or_default();
                match sub.as_str() {
                    "" | "DIALOG" | "UI" => {
                        return Some(Task::done(Message::MlStyleDialogOpen));
                    }
                    "LIST" | "?" => {
                        let doc = &self.tabs[i].scene.document;
                        let current = &doc.header.multiline_style;
                        let styles: Vec<String> = doc
                            .objects
                            .values()
                            .filter_map(|o| {
                                if let ObjectType::MLineStyle(s) = o {
                                    Some(s)
                                } else {
                                    None
                                }
                            })
                            .map(|s| {
                                let cur = if &s.name == current { " (current)" } else { "" };
                                format!("{}  [{}]{}", s.name, s.elements.len(), cur)
                            })
                            .collect();
                        if styles.is_empty() {
                            self.command_line.push_output(crate::t!("No multiline styles.").as_ref());
                        } else {
                            self.command_line
                                .push_output(crate::tf!("MLineStyles:\n  {}", styles.join("\n  ")).as_ref());
                        }
                    }
                    "NEW" | "N" => {
                        let name = parts.get(1).copied().unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: MLSTYLE NEW <name>").as_ref());
                        } else {
                            let doc = &self.tabs[i].scene.document;
                            let exists = doc.objects.values().any(|o| {
                                matches!(o, ObjectType::MLineStyle(s) if s.name.eq_ignore_ascii_case(&name))
                            });
                            if exists {
                                self.command_line
                                    .push_error(crate::tf!("MLSTYLE: '{}' already exists.", name).as_ref());
                            } else {
                                self.push_undo_snapshot(i, "MLSTYLE NEW");
                                let mut style = MLineStyle::standard();
                                style.name = name.clone();
                                let nh = acadrust::Handle::new(
                                    self.tabs[i].scene.document.next_handle(),
                                );
                                style.handle = nh;
                                self.tabs[i]
                                    .scene
                                    .document
                                    .objects
                                    .insert(nh, ObjectType::MLineStyle(style));
                                self.tabs[i].dirty = true;
                                self.command_line
                                    .push_output(crate::tf!("MLSTYLE: '{}' created.", name).as_ref());
                            }
                        }
                    }
                    "SET" | "S" => {
                        let name = parts.get(1).copied().unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: MLSTYLE SET <name>").as_ref());
                        } else {
                            let doc = &self.tabs[i].scene.document;
                            let exists = doc.objects.values().any(|o| {
                                matches!(o, ObjectType::MLineStyle(s) if s.name.eq_ignore_ascii_case(&name))
                            });
                            if exists {
                                self.push_undo_snapshot(i, "MLSTYLE SET");
                                self.tabs[i].scene.document.header.multiline_style = name.clone();
                                self.tabs[i].dirty = true;
                                self.command_line.push_output(crate::tf!(
                                    "MLSTYLE: current style set to '{}'.",
                                    name
                                ).as_ref());
                            } else {
                                self.command_line
                                    .push_error(crate::tf!("MLSTYLE: '{}' not found.", name).as_ref());
                            }
                        }
                    }
                    "DEL" | "DELETE" => {
                        let name = parts.get(1).copied().unwrap_or("").to_string();
                        if name.is_empty() || name.eq_ignore_ascii_case("Standard") {
                            self.command_line
                                .push_error(crate::t!("Cannot delete the Standard style.").as_ref());
                        } else {
                            let doc = &self.tabs[i].scene.document;
                            let handle = doc.objects.iter().find_map(|(&h, o)| {
                                if let ObjectType::MLineStyle(s) = o {
                                    if s.name.eq_ignore_ascii_case(&name) {
                                        Some(h)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            });
                            if let Some(h) = handle {
                                self.push_undo_snapshot(i, "MLSTYLE DEL");
                                self.tabs[i].scene.document.objects.remove(&h);
                                self.tabs[i].dirty = true;
                                self.command_line
                                    .push_output(crate::tf!("MLSTYLE: '{}' deleted.", name).as_ref());
                            } else {
                                self.command_line
                                    .push_error(crate::tf!("MLSTYLE: '{}' not found.", name).as_ref());
                            }
                        }
                    }
                    _ => {
                        self.command_line
                            .push_error(crate::t!("Usage: MLSTYLE [LIST|NEW <name>|SET <name>|DEL <name>]").as_ref());
                    }
                }
            }

            cmd if cmd == "DIMSTYLE"
                || cmd == "DDIM"
                || cmd.starts_with("DIMSTYLE ")
                || cmd.starts_with("DDIM ") =>
            {
                use acadrust::tables::DimStyle;
                let raw_rest = cmd.split_once(' ').map(|(_, r)| r.trim()).unwrap_or("");
                let parts: Vec<&str> = raw_rest.split_whitespace().collect();
                let sub = parts.get(0).map(|s| s.to_uppercase()).unwrap_or_default();
                match sub.as_str() {
                    // No sub-command or "DIALOG" → open the DimStyle Manager dialog
                    "" | "DIALOG" | "UI" => {
                        return Some(Task::done(Message::DimStyleDialogOpen));
                    }
                    "LIST" | "?" => {
                        let styles: Vec<String> = self.tabs[i]
                            .scene
                            .document
                            .dim_styles
                            .iter()
                            .map(|s| format!("{}(txt:{:.2} asz:{:.2})", s.name, s.dimtxt, s.dimasz))
                            .collect();
                        if styles.is_empty() {
                            self.command_line.push_output(crate::t!("No dim styles defined.").as_ref());
                        } else {
                            self.command_line
                                .push_output(crate::tf!("DimStyles: {}", styles.join(", ")).as_ref());
                        }
                    }
                    "NEW" | "N" => {
                        let name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line.push_error(crate::t!("Usage: DIMSTYLE NEW <name>").as_ref());
                        } else if self.tabs[i].scene.document.dim_styles.contains(&name) {
                            self.command_line
                                .push_error(crate::tf!("DIMSTYLE: '{}' already exists.", name).as_ref());
                        } else {
                            let undo = self.begin_dim_style_undo(
                                i,
                                "DIMSTYLE NEW",
                                std::slice::from_ref(&name),
                            );
                            let style = DimStyle::new(&name);
                            let _ = self.tabs[i].scene.document.dim_styles.add(style);
                            self.tabs[i].dirty = true;
                            self.commit_dim_style_undo(i, undo);
                            self.command_line
                                .push_output(crate::tf!("DIMSTYLE: '{}' created.", name).as_ref());
                        }
                    }
                    "SET" | "S" => {
                        // DIMSTYLE SET <name> <property> <value>
                        // e.g. DIMSTYLE SET Standard dimtxt 2.5
                        let style_name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        let prop = parts.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
                        let val_str = parts.get(3).map(|s| s.trim()).unwrap_or("");
                        if let Ok(val) = val_str.parse::<f64>() {
                            let undo = self.begin_dim_style_undo(
                                i,
                                "DIMSTYLE SET",
                                std::slice::from_ref(&style_name),
                            );
                            if let Some(ds) =
                                self.tabs[i].scene.document.dim_styles.get_mut(&style_name)
                            {
                                match prop.as_str() {
                                    "dimtxt" => {
                                        ds.dimtxt = val;
                                    }
                                    "dimasz" => {
                                        ds.dimasz = val;
                                    }
                                    "dimdli" => {
                                        ds.dimdli = val;
                                    }
                                    "dimexo" => {
                                        ds.dimexo = val;
                                    }
                                    "dimexe" => {
                                        ds.dimexe = val;
                                    }
                                    "dimgap" => {
                                        ds.dimgap = val;
                                    }
                                    "dimscale" => {
                                        ds.dimscale = val;
                                    }
                                    "dimlfac" => {
                                        ds.dimlfac = val;
                                    }
                                    "dimdle" => {
                                        ds.dimdle = val;
                                    }
                                    "dimtvp" => {
                                        ds.dimtvp = val;
                                    }
                                    "dimcen" => {
                                        ds.dimcen = val;
                                    }
                                    "dimtsz" => {
                                        ds.dimtsz = val;
                                    }
                                    "dimfxl" => {
                                        ds.dimfxl = val;
                                    }
                                    _ => {
                                        self.command_line.push_error(crate::tf!(
                                            "DIMSTYLE: unknown property '{}'. Try: dimtxt dimasz dimdli dimexo dimexe dimgap dimscale dimlfac dimdle dimcen dimtsz", prop
                                        ).as_ref());
                                        return Some(Task::none());
                                    }
                                }
                                self.tabs[i].dirty = true;
                                self.tabs[i].scene
                                    .invalidate_dim_style_dependencies(&style_name);
                                self.commit_dim_style_undo(i, undo);
                                self.command_line.push_output(crate::tf!(
                                    "DIMSTYLE: '{style_name}'.{prop} = {val:.3}"
                                ).as_ref());
                            } else {
                                self.command_line
                                    .push_error(crate::tf!("DIMSTYLE: '{}' not found.", style_name).as_ref());
                            }
                        } else {
                            self.command_line
                                .push_error(crate::t!("Usage: DIMSTYLE SET <name> <property> <value>").as_ref());
                        }
                    }
                    _ => {
                        self.command_line.push_info(
                            "Usage: DIMSTYLE LIST | NEW <name> | SET <name> <prop> <val>",
                        );
                    }
                }
            }

            // ── MLeader Style management ──────────────────────────────────
            cmd if cmd == "MLEADERSTYLE" || cmd.starts_with("MLEADERSTYLE ") => {
                use acadrust::objects::{MultiLeaderStyle, ObjectType};
                let raw_rest = cmd.trim_start_matches("MLEADERSTYLE").trim();
                let parts: Vec<&str> = raw_rest.split_whitespace().collect();
                let sub = parts.first().map(|s| s.to_uppercase()).unwrap_or_default();
                match sub.as_str() {
                    "" | "DIALOG" | "UI" => {
                        return Some(Task::done(Message::MLeaderStyleDialogOpen));
                    }
                    "LIST" | "?" => {
                        let styles: Vec<String> = self.tabs[i]
                            .scene
                            .document
                            .objects
                            .values()
                            .filter_map(|o| {
                                if let ObjectType::MultiLeaderStyle(s) = o {
                                    Some(format!(
                                        "{}(txt:{:.2} asz:{:.2})",
                                        s.name, s.text_height, s.arrowhead_size
                                    ))
                                } else {
                                    None
                                }
                            })
                            .collect();
                        let current = &self.tabs[i].active_mleader_style;
                        if styles.is_empty() {
                            self.command_line
                                .push_output(crate::tf!("MLeader styles: (none)  active: {current}").as_ref());
                        } else {
                            self.command_line.push_output(crate::tf!(
                                "MLeader styles: {}  active: {current}",
                                styles.join(", ")
                            ).as_ref());
                        }
                    }
                    "NEW" | "N" => {
                        let name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line
                                .push_error(crate::t!("Usage: MLEADERSTYLE NEW <name>").as_ref());
                        } else {
                            let already_exists = self.tabs[i].scene.document.objects.values().any(
                                |o| matches!(o, ObjectType::MultiLeaderStyle(s) if s.name == name),
                            );
                            if already_exists {
                                self.command_line.push_error(crate::tf!(
                                    "MLEADERSTYLE: '{}' already exists.",
                                    name
                                ).as_ref());
                            } else {
                                let handle = self.tabs[i].scene.document.allocate_handle();
                                let mut style = MultiLeaderStyle::new(&name);
                                style.handle = handle;
                                self.tabs[i]
                                    .scene
                                    .document
                                    .objects
                                    .insert(handle, ObjectType::MultiLeaderStyle(style));
                                self.push_undo_snapshot(i, "MLEADERSTYLE NEW");
                                self.tabs[i].dirty = true;
                                self.command_line
                                    .push_output(crate::tf!("MLEADERSTYLE: '{}' created.", name).as_ref());
                            }
                        }
                    }
                    "SET" | "S" => {
                        // MLEADERSTYLE SET <name> <property> <value>
                        // Properties: text_height arrowhead_size landing_distance landing_gap
                        let style_name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        let prop = parts.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
                        let val_str = parts.get(3).map(|s| s.trim()).unwrap_or("");
                        if let Ok(val) = val_str.parse::<f64>() {
                            let style_entry = self.tabs[i]
                                .scene
                                .document
                                .objects
                                .values_mut()
                                .find_map(|o| {
                                    if let ObjectType::MultiLeaderStyle(s) = o {
                                        if s.name == style_name {
                                            Some(s)
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                });
                            if let Some(s) = style_entry {
                                match prop.as_str() {
                                    "text_height" | "textheight" | "txth" => {
                                        s.text_height = val;
                                    }
                                    "arrowhead_size" | "arrowsize" | "asz" => {
                                        s.arrowhead_size = val;
                                    }
                                    "landing_distance" | "landing" | "dogleg" => {
                                        s.landing_distance = val;
                                    }
                                    "landing_gap" | "gap" => {
                                        s.landing_gap = val;
                                    }
                                    _ => {
                                        self.command_line.push_error(crate::tf!(
                                            "MLEADERSTYLE: unknown property '{}'. Try: text_height arrowhead_size landing_distance landing_gap", prop
                                        ).as_ref());
                                        return Some(Task::none());
                                    }
                                }
                                self.push_undo_snapshot(i, "MLEADERSTYLE SET");
                                self.tabs[i].dirty = true;
                                self.command_line.push_output(crate::tf!(
                                    "MLEADERSTYLE: '{style_name}'.{prop} = {val:.3}"
                                ).as_ref());
                            } else {
                                self.command_line.push_error(crate::tf!(
                                    "MLEADERSTYLE: '{}' not found.",
                                    style_name
                                ).as_ref());
                            }
                        } else {
                            self.command_line
                                .push_error(crate::t!("Usage: MLEADERSTYLE SET <name> <property> <value>").as_ref());
                        }
                    }
                    "CURRENT" | "C" | "ACTIVE" => {
                        let name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line.push_output(crate::tf!(
                                "Current MLeader style: {}",
                                self.tabs[i].active_mleader_style
                            ).as_ref());
                        } else {
                            let exists = name == "Standard" || self.tabs[i].scene.document.objects.values()
                                .any(|o| matches!(o, ObjectType::MultiLeaderStyle(s) if s.name == name));
                            if exists {
                                self.tabs[i].active_mleader_style = name.clone();
                                self.command_line.push_output(crate::tf!(
                                    "MLEADERSTYLE: current style set to '{name}'."
                                ).as_ref());
                            } else {
                                self.command_line
                                    .push_error(crate::tf!("MLEADERSTYLE: '{}' not found.", name).as_ref());
                            }
                        }
                    }
                    _ => {
                        self.command_line.push_info(
                            crate::t!("Usage: MLEADERSTYLE LIST | NEW <name> | SET <name> <prop> <val> | CURRENT [<name>]").as_ref()
                        );
                    }
                }
            }

            // ── TextStyle / Style management ──────────────────────────────
            cmd if cmd == "STYLE"
                || cmd == "TEXTSTYLE"
                || cmd.starts_with("STYLE ")
                || cmd.starts_with("TEXTSTYLE ") =>
            {
                let (prefix, rest) = if cmd.starts_with("TEXTSTYLE") {
                    ("TEXTSTYLE", cmd.trim_start_matches("TEXTSTYLE").trim())
                } else {
                    ("STYLE", cmd.trim_start_matches("STYLE").trim())
                };
                let parts: Vec<&str> = rest.splitn(3, ' ').collect();
                let sub = parts.get(0).map(|s| s.to_uppercase()).unwrap_or_default();
                match sub.as_str() {
                    "" | "DIALOG" | "UI" => {
                        return Some(Task::done(Message::TextStyleDialogOpen));
                    }
                    "LIST" | "?" => {
                        let styles: Vec<String> = self.tabs[i]
                            .scene
                            .document
                            .text_styles
                            .iter()
                            .map(|s| {
                                format!(
                                    "{} (font: {}, w: {:.2}, oblique: {:.1}°)",
                                    s.name,
                                    s.font_file,
                                    s.width_factor,
                                    s.oblique_angle.to_degrees()
                                )
                            })
                            .collect();
                        if styles.is_empty() {
                            self.command_line.push_output(crate::t!("No text styles defined.").as_ref());
                        } else {
                            self.command_line
                                .push_output(crate::tf!("Text styles: {}", styles.join(" | ")).as_ref());
                        }
                    }
                    "SET" | "S" => {
                        // STYLE SET <name> — set active text style (for future text commands)
                        let name = parts.get(1).map(|s| s.trim()).unwrap_or("");
                        if self.tabs[i].scene.document.text_styles.get(name).is_some() {
                            self.command_line
                                .push_output(crate::tf!("{prefix}: active style set to '{name}'.").as_ref());
                        } else {
                            self.command_line
                                .push_error(crate::tf!("{prefix}: style '{name}' not found.").as_ref());
                        }
                    }
                    "NEW" | "N" => {
                        let name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        if name.is_empty() {
                            self.command_line
                                .push_error(crate::tf!("Usage: {prefix} NEW <name>").as_ref());
                        } else if self.tabs[i].scene.document.text_styles.contains(&name) {
                            self.command_line
                                .push_error(crate::tf!("{prefix}: style '{name}' already exists.").as_ref());
                        } else {
                            let undo = self.begin_text_style_undo(
                                i,
                                "STYLE NEW",
                                std::slice::from_ref(&name),
                            );
                            let style = acadrust::tables::TextStyle::new(&name);
                            let _ = self.tabs[i].scene.document.text_styles.add(style);
                            self.tabs[i].dirty = true;
                            self.commit_text_style_undo(i, undo);
                            self.command_line
                                .push_output(crate::tf!("{prefix}: style '{name}' created.").as_ref());
                        }
                    }
                    "FONT" | "F" => {
                        // STYLE FONT <name> <font_file>
                        let style_name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        let font = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();
                        if style_name.is_empty() || font.is_empty() {
                            self.command_line
                                .push_error(crate::tf!("Usage: {prefix} FONT <style> <font_file>").as_ref());
                        } else {
                            let undo = self.begin_text_style_undo(
                                i,
                                "STYLE FONT",
                                std::slice::from_ref(&style_name),
                            );
                            if let Some(style) =
                            self.tabs[i].scene.document.text_styles.get_mut(&style_name)
                        {
                                style.font_file = font.clone();
                            self.tabs[i].dirty = true;
                                self.tabs[i]
                                    .scene
                                    .invalidate_text_style_dependencies(&style_name);
                                self.commit_text_style_undo(i, undo);
                            self.command_line.push_output(crate::tf!(
                                "{prefix}: '{style_name}' font set to '{font}'."
                            ).as_ref());
                        } else {
                            self.command_line
                                .push_error(crate::tf!("{prefix}: style '{style_name}' not found.").as_ref());
                            }
                        }
                    }
                    "WIDTH" | "W" => {
                        // STYLE WIDTH <name> <factor>
                        let style_name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        let factor_str = parts.get(2).map(|s| s.trim()).unwrap_or("");
                        if let Ok(factor) = factor_str.parse::<f64>() {
                            let undo = self.begin_text_style_undo(
                                i,
                                "STYLE WIDTH",
                                std::slice::from_ref(&style_name),
                            );
                            if let Some(style) =
                                self.tabs[i].scene.document.text_styles.get_mut(&style_name)
                            {
                                style.width_factor = factor;
                                self.tabs[i].dirty = true;
                                self.tabs[i]
                                    .scene
                                    .invalidate_text_style_dependencies(&style_name);
                                self.commit_text_style_undo(i, undo);
                                self.command_line.push_output(crate::tf!(
                                    "{prefix}: '{style_name}' width factor set to {factor:.3}."
                                ).as_ref());
                            } else {
                                self.command_line.push_error(crate::tf!(
                                    "{prefix}: style '{style_name}' not found."
                                ).as_ref());
                            }
                        } else {
                            self.command_line
                                .push_error(crate::tf!("Usage: {prefix} WIDTH <style> <factor>").as_ref());
                        }
                    }
                    "OBLIQUE" => {
                        // STYLE OBLIQUE <name> <angle_degrees>
                        let style_name = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                        let angle_str = parts.get(2).map(|s| s.trim()).unwrap_or("");
                        if let Ok(deg) = angle_str.parse::<f64>() {
                            let undo = self.begin_text_style_undo(
                                i,
                                "STYLE OBLIQUE",
                                std::slice::from_ref(&style_name),
                            );
                            if let Some(style) =
                                self.tabs[i].scene.document.text_styles.get_mut(&style_name)
                            {
                                style.oblique_angle = deg.to_radians();
                                self.tabs[i].dirty = true;
                                self.tabs[i]
                                    .scene
                                    .invalidate_text_style_dependencies(&style_name);
                                self.commit_text_style_undo(i, undo);
                                self.command_line.push_output(crate::tf!(
                                    "{prefix}: '{style_name}' oblique angle set to {deg:.1}°."
                                ).as_ref());
                            } else {
                                self.command_line.push_error(crate::tf!(
                                    "{prefix}: style '{style_name}' not found."
                                ).as_ref());
                            }
                        } else {
                            self.command_line.push_error(crate::tf!(
                                "Usage: {prefix} OBLIQUE <style> <angle_degrees>"
                            ).as_ref());
                        }
                    }
                    _ => {
                        self.command_line.push_info(crate::tf!(
                            "Usage: {prefix} LIST | NEW <name> | FONT <style> <file> | WIDTH <style> <factor> | OBLIQUE <style> <angle>"
                        ).as_ref());
                    }
                }
            }

            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }

    /// Snap to the plan view of the UCS whose rotation is `r_ucs`, straight —
    /// no "already there → flip to the opposite face" cube behaviour (#326).
    fn plan_snap(&mut self, i: usize, r_ucs: glam::Mat4) {
        let eye_dir = r_ucs.transform_vector3(
            crate::scene::CubeRegion::Face(crate::scene::pipeline::viewcube::FACE_TOP)
                .snap_direction(),
        );
        if self.tabs[i].scene.active_viewport.is_some() {
            self.tabs[i]
                .scene
                .mutate_active_viewport_camera(|c| c.snap_to_face(eye_dir, r_ucs));
        } else {
            self.tabs[i]
                .scene
                .camera
                .borrow_mut()
                .snap_to_face(eye_dir, r_ucs);
        }
        self.tabs[i].scene.camera_generation += 1;
    }
}
