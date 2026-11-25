//! The mesh commands carried out: mesh primitives, meshes through curves, smoothing, creases,
//! face edits and the conversion of meshes to solids and surfaces, each one undo step.

use document::entities::{Mesh, Surface, SurfaceKind};
use document::{EntityType, Handle};
use glam::DVec3;
use iced::Task;
use ocs_solid::model::Brep;

use crate::app::{Message, OpenCADStudio};
use crate::entities::mesh_edit;
use crate::modules::mesh::{self as module, var};
use crate::modules::surface::command::{SurfaceCommand, SurfaceRequest};

/// The system variables the Mesh and Surface tabs switch from their buttons.
const TOGGLES: &[&str] = &["SURFACEASSOCIATIVITY", "SURFACEMODELINGMODE", "SURFACEAUTOTRIM"];

/// Whether `name` is a command this file carries out.
pub(super) fn is_mesh_command(name: &str) -> bool {
    module::COMMANDS.contains(&name) || matches!(name, "MESH" | "MESHPRIMITIVEOPTIONS")
}

fn point(p: [f64; 3]) -> DVec3 {
    DVec3::from_array(p)
}

/// The two ends of a line or polyline, the one nearer `pick` first.
fn ends_from(entity: &EntityType, pick: DVec3) -> Option<(DVec3, DVec3)> {
    let (points, closed) = crate::modules::draw::inquiry::divide::points_along(entity, 1)?;
    if closed {
        return None;
    }
    let (a, b) = (point(*points.first()?), point(*points.last()?));
    Some(if a.distance(pick) <= b.distance(pick) { (a, b) } else { (b, a) })
}

/// `steps` equal steps along a curve: its points (the closing point left out on a closed
/// curve) and whether it is closed.
fn sampled(entity: &EntityType, steps: usize) -> Option<(Vec<DVec3>, bool)> {
    let (points, closed) = crate::modules::draw::inquiry::divide::points_along(entity, steps.max(1))?;
    let mut points: Vec<DVec3> = points.into_iter().map(point).collect();
    if closed {
        points.pop();
    }
    Some((points, closed))
}

impl OpenCADStudio {
    fn surftab(&self, i: usize, second: bool) -> usize {
        let header = &self.tabs[i].scene.document.header;
        let value = if second { header.surface_tab2 } else { header.surface_tab1 };
        value.clamp(2, 32766) as usize
    }

    /// Shows `name`'s value and waits for a new one.
    fn ask_mesh_var(&mut self, name: &str, current: String) {
        self.command_line.push_output(&crate::ui::command_line::value_prompt(name, &current));
        self.pending_setvar = Some(name.to_string());
    }

    /// Reads or sets one of the Mesh tab's system variables; `None` for another name.
    fn mesh_setvar(&mut self, i: usize, name: &str, value: Option<&str>) -> Option<Task<Message>> {
        let value = value.map(str::trim).filter(|v| !v.is_empty());
        if name == "SURFTAB1" || name == "SURFTAB2" {
            let second = name == "SURFTAB2";
            match value.map(|v| v.parse::<i16>().ok().filter(|v| (2..=32766).contains(v))) {
                Some(Some(v)) => {
                    let header = &mut self.tabs[i].scene.document.header;
                    if second {
                        header.surface_tab2 = v;
                    } else {
                        header.surface_tab1 = v;
                    }
                    self.tabs[i].dirty = true;
                }
                Some(None) => {
                    self.command_line.push_rejection(crate::t!("Requires an integer between 2 and 32766.").as_ref());
                    self.ask_mesh_var(name, self.surftab(i, second).to_string());
                }
                None => self.ask_mesh_var(name, self.surftab(i, second).to_string()),
            }
            return Some(Task::none());
        }
        let known = module::VARS.iter().find(|v| v.name == name)?;
        match value {
            Some(text) => match text.parse::<f64>().map_err(|_| String::new()).and_then(|v| module::set_var(known.name, v)) {
                Ok(()) => {}
                Err(e) => {
                    let e = if e.is_empty() { crate::t!("Requires a number.").into_owned() } else { e };
                    self.command_line.push_rejection(&e);
                    self.ask_mesh_var(known.name, format!("{}", var(known.name)));
                }
            },
            None => self.ask_mesh_var(known.name, format!("{}", var(known.name))),
        }
        Some(Task::none())
    }

    /// Flips one of the on/off settings a ribbon button switches.
    fn toggle_setting(&mut self, name: &str) {
        match name {
            "SURFACEASSOCIATIVITY" => {
                use crate::scene::model::sweep_model;
                sweep_model::set_surface_associativity(!sweep_model::surface_associativity());
                self.save_config();
            }
            "SURFACEMODELINGMODE" => {
                let mode = super::surfacing::modeling_mode();
                super::surfacing::set_modeling_mode(1 - mode);
            }
            _ => {
                let on = var(name) != 0.0;
                let _ = module::set_var(name, if on { 0.0 } else { 1.0 });
            }
        }
    }

    /// Starts a mesh command, sets one of its variables, or switches a setting; `None` for any
    /// other command.
    pub(super) fn dispatch_mesh(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        let verb = verb.trim_start_matches(['_', '.', '\'']).to_ascii_uppercase();
        let rest = rest.trim();
        if TOGGLES.contains(&verb.as_str()) && rest.eq_ignore_ascii_case("TOGGLE") {
            self.toggle_setting(&verb);
            return Some(Task::none());
        }
        if verb == "SETVAR" {
            let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            return self.mesh_setvar(i, &name.to_ascii_uppercase(), Some(value));
        }
        if let Some(task) = self.mesh_setvar(i, &verb, Some(rest)) {
            return Some(task);
        }
        let name: &'static str = module::COMMANDS
            .iter()
            .copied()
            .chain(["MESH", "MESHPRIMITIVEOPTIONS"])
            .find(|c| *c == verb)?;
        use crate::command::CadCommand;
        if name == "MESH" {
            if let Some(shape) = module::primitive_named(rest) {
                return Some(self.start_mesh_primitive(i, shape));
            }
        }
        if (name == "MESH" || name == "MESHPRIMITIVEOPTIONS") && module::steps(name, rest).is_none() {
            let command = if name == "MESH" {
                module::ChooseCommand::mesh()
            } else {
                module::ChooseCommand::primitive_options()
            };
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return Some(Task::none());
        }
        let steps = module::steps(name, rest)?;
        let selected = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(handle, entity)| (handle, entity.clone()))
            .collect();
        let mut command = SurfaceCommand::new(name, steps.clone(), selected);
        // Settings are offered at their current values.
        for step in &steps {
            if let crate::modules::surface::command::Step::Value { key, .. } = step {
                if module::VARS.iter().any(|v| v.name == *key) {
                    command = command.with_value(key, var(key));
                }
            }
            if let crate::modules::surface::command::Step::Accept { options, .. } = step {
                for opt in options.iter() {
                    command = command.with_value(opt.key, var(opt.key));
                }
            }
        }
        if name == "MESH" {
            command = command.with_value("level", module::primitive_level() as f64);
        }
        if command.ready() {
            return Some(self.handle_surfacing(command.take_request()));
        }
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Some(Task::none())
    }

    /// MESH BOX (and the others): the solid primitive's prompts, a mesh made of its size.
    fn start_mesh_primitive(&mut self, i: usize, shape: &str) -> Task<Message> {
        use crate::command::CadCommand;
        let command: Box<dyn CadCommand> = if shape == "CYLINDER" {
            Box::new(crate::modules::model::cylinder_cmd::CylinderCommand::new().as_mesh())
        } else {
            Box::new(crate::modules::model::primitive_cmd::PrimitiveCommand::new(shape).as_mesh())
        };
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(command);
        Task::none()
    }

    /// Writes `mesh` over the mesh `handle`.
    fn put_mesh(&mut self, i: usize, handle: Handle, mut mesh: Mesh) {
        mesh.common.handle = handle;
        self.tabs[i].scene.update_entity(EntityType::Mesh(mesh));
    }

    /// The subdivision mesh `handle`, cloned.
    fn mesh_at(&self, i: usize, handle: Handle) -> Option<Mesh> {
        match self.tabs[i].scene.document.get_entity(handle)? {
            EntityType::Mesh(mesh) => Some(mesh.clone()),
            _ => None,
        }
    }

    /// The picks of `slot` on subdivision meshes, by mesh.
    fn mesh_picks(&self, i: usize, req: &SurfaceRequest, slot: usize) -> Vec<(Handle, Mesh, Vec<DVec3>)> {
        module::by_entity(&req.picks[slot])
            .into_iter()
            .filter_map(|(h, points)| Some((h, self.mesh_at(i, h)?, points)))
            .collect()
    }

    /// A curve picked in `slot`: the entity and the pick point.
    fn picked_curve(&self, i: usize, req: &SurfaceRequest, slot: usize, index: usize) -> Result<(EntityType, DVec3), String> {
        let (h, p) = *req.picks[slot].get(index).ok_or_else(|| crate::t!("Nothing picked.").into_owned())?;
        let entity = self.tabs[i].scene.document.get_entity(h).cloned().ok_or("no object")?;
        Ok((entity, p))
    }

    /// The faces of `handle` drawn as a mesh, for MESHSMOOTH: its own faces for a mesh or face,
    /// a tessellation of a solid, surface or region.
    fn mesh_of(&mut self, i: usize, handle: Handle) -> Option<Mesh> {
        let entity = self.tabs[i].scene.document.get_entity(handle)?.clone();
        let mut mesh = match &entity {
            EntityType::Face3D(face) => {
                let corners = [face.first_corner, face.second_corner, face.third_corner, face.fourth_corner];
                let mut points: Vec<[f64; 3]> = corners.iter().map(|c| [c.x, c.y, c.z]).collect();
                points.dedup();
                if points.len() == 4 && points[3] == points[0] {
                    points.pop();
                }
                mesh_edit::mesh_from(&points, vec![(0..points.len()).collect()])
            }
            EntityType::PolygonMesh(_) | EntityType::PolyfaceMesh(_) => {
                let (points, faces) = crate::entities::mesh::mesh_polygons(&entity)?;
                mesh_edit::mesh_from(&points, faces)
            }
            _ => {
                let body = self.surfacing_body(i, handle)?;
                let size = ocs_solid::model::size(&body);
                let deviation = var("FACETERDEVSURFACE");
                let tolerance = crate::scene::model::solid_model::DisplayTolerance {
                    angle: var("FACETERDEVNORMAL").clamp(1.0, 90.0).to_radians(),
                    chord: (deviation > 0.0).then_some(deviation),
                    isolines: [0, 0],
                    planar_isolines: Some(false),
                };
                let tess = crate::scene::model::solid_model::tessellate(&body, tolerance)?;
                let (points, map) = mesh_edit::weld(&tess.positions, size * 1e-9);
                let triangles: Vec<[usize; 3]> = tess.triangles.iter().map(|t| t.map(|v| map[v])).collect();
                let faces = match var("FACETERMESHTYPE") as i32 {
                    2 => triangles.iter().map(|t| t.to_vec()).collect(),
                    1 => mesh_edit::paired_quads(&triangles, &tess.triangle_faces),
                    _ => mesh_edit::merge_coplanar(&points, triangles.iter().map(|t| t.to_vec()).collect(), size * 1e-9),
                };
                mesh_edit::mesh_from(&points, faces)
            }
        };
        mesh.common = entity.common().clone();
        mesh.common.handle = Handle::NULL;
        Some(mesh)
    }

    /// The faces of a mesh object as SMOOTHMESHCONVERT converts them: smoothed or the base
    /// faces, flat, and those in one plane merged when optimized.
    fn converted_faces(&self, entity: &EntityType) -> Option<(Vec<[f64; 3]>, Vec<Vec<usize>>)> {
        let mode = var("SMOOTHMESHCONVERT") as i32;
        let (points, faces) = match entity {
            EntityType::Mesh(mesh) if mode <= 1 => {
                let (points, faces, _) = crate::entities::mesh::smoothed_cage(mesh);
                (points, faces)
            }
            _ => crate::entities::mesh::mesh_polygons(entity)?,
        };
        let size = points
            .iter()
            .fold((DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)), |(lo, hi), p| (lo.min(point(*p)), hi.max(point(*p))));
        let tolerance = (size.1 - size.0).length().max(1e-9) * 1e-9;
        let faces = mesh_edit::flat_faces(&points, faces, tolerance);
        let faces = if mode % 2 == 0 { mesh_edit::merge_coplanar(&points, faces, tolerance) } else { faces };
        Some((points, faces))
    }

    /// A solid entity of `body` styled like `source`.
    fn add_converted_solid(&mut self, body: Brep, source: &EntityType) -> Handle {
        let Some(sat) = crate::scene::convert::acis_export::solid_to_sat(&body) else {
            return Handle::NULL;
        };
        let mut solid = document::entities::Solid3D::new();
        solid.set_sat_document(&sat);
        solid.wires = crate::scene::model::solid_model::edge_wires(&body);
        solid.common = source.common().clone();
        solid.common.handle = Handle::NULL;
        let history = crate::scene::model::solid_history::brep_op(&body);
        self.add_solid_model_preserving_style(EntityType::Solid3D(solid), body, history)
    }

    /// A surface entity of `body` styled like `source`.
    fn add_converted_surface(&mut self, body: Brep, source: &EntityType) -> Handle {
        let mut surface = Surface::new(SurfaceKind::Generic);
        surface.common = source.common().clone();
        surface.common.handle = Handle::NULL;
        self.add_surface_model_inner(EntityType::Surface(Box::new(surface)), body, false)
    }

    /// Erases the converted objects when DELOBJ asks.
    fn erase_converted(&mut self, i: usize, handles: &[Handle]) {
        if self.delete_objects != 0 && !handles.is_empty() {
            self.tabs[i].scene.erase_entities(handles);
        }
    }

    /// Carries out a mesh command.
    pub(super) fn meshing(&mut self, i: usize, req: &SurfaceRequest) -> Result<String, String> {
        let name = req.command;
        match name {
            "MESH" => {
                module::set_primitive_level(req.value("level", 0.0) as i32);
                Ok(String::new())
            }
            "MESHPRIMITIVEOPTIONS" | "MESHOPTIONS" => {
                for (key, value) in &req.values {
                    if module::VARS.iter().any(|v| v.name == *key) {
                        module::set_var(key, *value)?;
                    }
                }
                Ok(String::new())
            }
            "MESHSMOOTH" => {
                let level = var("FACETERSMOOTHLEV") as i32;
                let mut made = Vec::new();
                for &h in &req.objects[0] {
                    if let Some(mut mesh) = self.mesh_of(i, h) {
                        mesh.subdivision_level = level;
                        made.push((h, mesh));
                    }
                }
                if made.is_empty() {
                    return Err(crate::t!("No objects could be converted to a mesh.").into_owned());
                }
                self.push_undo_snapshot(i, name);
                let sources: Vec<Handle> = made.iter().map(|(h, _)| *h).collect();
                self.tabs[i].scene.erase_entities(&sources);
                let count = made.len();
                for (_, mesh) in made {
                    self.tabs[i].scene.add_entity(EntityType::Mesh(mesh));
                }
                self.tabs[i].dirty = true;
                Ok(crate::tf!("{count} object(s) converted to mesh.").into_owned())
            }
            "MESHSMOOTHMORE" | "MESHSMOOTHLESS" => {
                let max = var("SMOOTHMESHMAXLEV") as i32;
                let step = if name == "MESHSMOOTHMORE" { 1 } else { -1 };
                let changed: Vec<(Handle, Mesh)> = req.objects[0]
                    .iter()
                    .filter_map(|&h| {
                        let mut mesh = self.mesh_at(i, h)?;
                        let level = (mesh.subdivision_level + step).clamp(0, max.min(4));
                        (level != mesh.subdivision_level).then(|| {
                            mesh.subdivision_level = level;
                            (h, mesh)
                        })
                    })
                    .collect();
                if changed.is_empty() {
                    return Err(if step > 0 {
                        crate::t!("The meshes are already at the highest smoothness level.").into_owned()
                    } else {
                        crate::t!("The meshes are not smoothed.").into_owned()
                    });
                }
                self.push_undo_snapshot(i, name);
                for (h, mesh) in changed {
                    self.put_mesh(i, h, mesh);
                }
                self.tabs[i].dirty = true;
                Ok(String::new())
            }
            "MESHREFINE" => {
                let refined: Vec<(Handle, Mesh)> = req.objects[0]
                    .iter()
                    .filter_map(|&h| {
                        let mut mesh = self.mesh_at(i, h)?;
                        mesh_edit::refine(&mut mesh).then_some((h, mesh))
                    })
                    .collect();
                if refined.is_empty() {
                    return Err(crate::t!("Only a mesh smoothed to level 1 or higher can be refined.").into_owned());
                }
                self.push_undo_snapshot(i, name);
                for (h, mesh) in refined {
                    self.put_mesh(i, h, mesh);
                }
                self.tabs[i].dirty = true;
                Ok(String::new())
            }
            "MESHCREASE" | "MESHUNCREASE" => {
                let value = (name == "MESHCREASE").then(|| {
                    let v = req.value("crease", mesh_edit::ALWAYS);
                    if v < 0.0 {
                        mesh_edit::ALWAYS
                    } else {
                        v
                    }
                });
                let mut changed = Vec::new();
                for (h, mut mesh, points) in self.mesh_picks(i, req, 0) {
                    let edges: Vec<(usize, usize)> = points
                        .iter()
                        .filter_map(|&p| match value {
                            Some(_) => mesh_edit::pick_part(&mesh, p).map(|part| mesh_edit::part_edges(&mesh, part)),
                            None => mesh_edit::pick_edge(&mesh, p).map(|e| vec![e]),
                        })
                        .flatten()
                        .collect();
                    if mesh_edit::set_creases(&mut mesh, &edges, value) > 0 {
                        changed.push((h, mesh));
                    }
                }
                if changed.is_empty() {
                    return Err(crate::t!("No mesh edges changed.").into_owned());
                }
                self.push_undo_snapshot(i, name);
                for (h, mesh) in changed {
                    self.put_mesh(i, h, mesh);
                }
                self.tabs[i].dirty = true;
                Ok(String::new())
            }
            "MESHEXTRUDE" => {
                let height = req.value("height", 1.0);
                let mut changed = Vec::new();
                for (h, mut mesh, points) in self.mesh_picks(i, req, 0) {
                    let mut faces: Vec<usize> = points.iter().filter_map(|&p| mesh_edit::pick_face(&mesh, p)).collect();
                    faces.sort_unstable();
                    faces.dedup();
                    if mesh_edit::extrude(&mut mesh, &faces, height, None) {
                        changed.push((h, mesh));
                    }
                }
                if changed.is_empty() {
                    return Err(crate::t!("Select faces of a mesh.").into_owned());
                }
                self.push_undo_snapshot(i, name);
                for (h, mesh) in changed {
                    self.put_mesh(i, h, mesh);
                }
                self.tabs[i].dirty = true;
                Ok(String::new())
            }
            "MESHSPLIT" | "MESHMERGE" | "MESHCOLLAPSE" | "MESHCAP" => {
                let picks = self.mesh_picks(i, req, 0);
                let [(h, mut mesh, points)] = <[_; 1]>::try_from(picks)
                    .map_err(|_| crate::t!("Select faces or edges of one mesh.").into_owned())?;
                match name {
                    "MESHSPLIT" => {
                        let face = mesh_edit::pick_face(&mesh, points[0]).ok_or("no face")?;
                        let [from, to] = req.points[..] else {
                            return Err(crate::t!("Specify two split points.").into_owned());
                        };
                        mesh_edit::split(&mut mesh, face, from, to).map_err(|e| crate::t!(e).into_owned())?;
                    }
                    "MESHMERGE" => {
                        let mut faces: Vec<usize> = points.iter().filter_map(|&p| mesh_edit::pick_face(&mesh, p)).collect();
                        faces.sort_unstable();
                        faces.dedup();
                        mesh_edit::merge(&mut mesh, &faces).map_err(|e| crate::t!(e).into_owned())?;
                    }
                    "MESHCOLLAPSE" => {
                        let part = mesh_edit::pick_part(&mesh, points[0]).ok_or("no face")?;
                        mesh_edit::collapse(&mut mesh, part);
                    }
                    _ => {
                        if mesh_edit::cap(&mut mesh, &points) == 0 {
                            return Err(crate::t!("The edges bound no hole in the mesh.").into_owned());
                        }
                    }
                }
                self.push_undo_snapshot(i, name);
                self.put_mesh(i, h, mesh);
                self.tabs[i].dirty = true;
                Ok(String::new())
            }
            "MESHSPIN" => {
                let (Some(&(h1, p1)), Some(&(h2, p2))) = (req.picks[0].first(), req.picks[1].first()) else {
                    return Err(crate::t!("Nothing picked.").into_owned());
                };
                if h1 != h2 {
                    return Err(crate::t!("Select two adjacent triangular faces.").into_owned());
                }
                let mut mesh = self.mesh_at(i, h1).ok_or("no mesh")?;
                let (f1, f2) = (mesh_edit::pick_face(&mesh, p1).ok_or("no face")?, mesh_edit::pick_face(&mesh, p2).ok_or("no face")?);
                mesh_edit::spin(&mut mesh, f1, f2).map_err(|e| crate::t!(e).into_owned())?;
                self.push_undo_snapshot(i, name);
                self.put_mesh(i, h1, mesh);
                self.tabs[i].dirty = true;
                Ok(String::new())
            }
            "CONVTOSOLID" | "CONVTOSURFACE" => {
                let solid = name == "CONVTOSOLID";
                let mut made: Vec<(Handle, EntityType, Brep)> = Vec::new();
                let mut open = 0;
                for &h in &req.objects[0] {
                    let Some(entity) = self.tabs[i].scene.document.get_entity(h).cloned() else { continue };
                    let body = match &entity {
                        EntityType::Solid3D(_) | EntityType::Region(_) => self
                            .surfacing_body(i, h)
                            .and_then(|b| {
                                let faces: Vec<u32> = (0..b.faces.len() as u32).collect();
                                ocs_solid::model::face_sheet(&b, &faces)
                            }),
                        _ => {
                            let Some((points, faces)) = self.converted_faces(&entity) else { continue };
                            if solid && !mesh_edit::closed(&faces) {
                                open += 1;
                                continue;
                            }
                            let faces = if mesh_edit::closed(&faces) { mesh_edit::outward(&points, faces) } else { faces };
                            if solid {
                                ocs_solid::model::faceted_solid(&points, &faces)
                            } else {
                                ocs_solid::model::faceted_sheet(&points, &faces)
                            }
                        }
                    };
                    if let Some(body) = body {
                        made.push((h, entity, body));
                    }
                }
                if made.is_empty() {
                    return Err(if open > 0 {
                        crate::t!("The mesh is not closed: close its holes (MESHCAP) to make a solid.").into_owned()
                    } else {
                        crate::t!("No objects could be converted.").into_owned()
                    });
                }
                self.push_undo_snapshot(i, name);
                let sources: Vec<Handle> = made.iter().map(|(h, _, _)| *h).collect();
                let count = made.len();
                for (_, entity, body) in made {
                    if solid {
                        self.add_converted_solid(body, &entity);
                    } else {
                        self.add_converted_surface(body, &entity);
                    }
                }
                self.erase_converted(i, &sources);
                self.tabs[i].dirty = true;
                let mut message = if solid {
                    crate::tf!("{count} object(s) converted to solids.").into_owned()
                } else {
                    crate::tf!("{count} object(s) converted to surfaces.").into_owned()
                };
                if open > 0 {
                    message.push('\n');
                    message.push_str(&crate::tf!("{open} open mesh(es) were left as they are."));
                }
                Ok(message)
            }
            "REVSURF" | "RULESURF" | "TABSURF" | "EDGESURF" => {
                let mesh = self.curve_mesh(i, req)?;
                self.push_undo_snapshot(i, name);
                self.commit_entity_handle(EntityType::PolygonMesh(mesh));
                self.tabs[i].dirty = true;
                Ok(String::new())
            }
            _ => Err(format!("{name}?")),
        }
    }

    /// The polygon mesh REVSURF, RULESURF, TABSURF or EDGESURF makes of the picked curves.
    fn curve_mesh(&self, i: usize, req: &SurfaceRequest) -> Result<document::entities::polygon_mesh::PolygonMesh, String> {
        let (m, n) = (self.surftab(i, false), self.surftab(i, true));
        let not_curve = || crate::t!("The object is not a curve that can define a mesh.").into_owned();
        let mesh = match req.command {
            "REVSURF" => {
                let (path, _) = self.picked_curve(i, req, 0, 0)?;
                let (axis, pick) = self.picked_curve(i, req, 1, 0)?;
                let (origin, tip) = ends_from(&axis, pick).ok_or_else(|| crate::t!("The axis must be a line or an open polyline.").into_owned())?;
                let (points, closed) = sampled(&path, n).ok_or_else(not_curve)?;
                let sweep = req.value("angle", 360.0).clamp(-360.0, 360.0).to_radians();
                if sweep == 0.0 {
                    return Err(crate::t!("The included angle must not be zero.").into_owned());
                }
                let rows = mesh_edit::revolved(&points, origin, tip - origin, req.value("start", 0.0).to_radians(), sweep, m);
                let whole = (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9;
                mesh_edit::polygon_mesh(&rows, whole, closed)
            }
            "RULESURF" => {
                let ((a, pa), (b, pb)) = (self.picked_curve(i, req, 0, 0)?, self.picked_curve(i, req, 1, 0)?);
                let ((mut ra, ca), (mut rb, cb)) = (sampled(&a, m).ok_or_else(not_curve)?, sampled(&b, m).ok_or_else(not_curve)?);
                if ca != cb {
                    return Err(crate::t!("Both curves must be open or both closed.").into_owned());
                }
                // An open curve runs from its end nearer the pick.
                for (row, pick) in [(&mut ra, pa), (&mut rb, pb)] {
                    if !ca && row.last().is_some_and(|l| l.distance(pick) < row[0].distance(pick)) {
                        row.reverse();
                    }
                }
                mesh_edit::polygon_mesh(&[ra, rb], false, ca)
            }
            "TABSURF" => {
                let (path, _) = self.picked_curve(i, req, 0, 0)?;
                let (direction, pick) = self.picked_curve(i, req, 1, 0)?;
                let (from, to) = ends_from(&direction, pick).ok_or_else(|| crate::t!("The direction vector must be a line or an open polyline.").into_owned())?;
                let (points, closed) = sampled(&path, m).ok_or_else(not_curve)?;
                let shifted = points.iter().map(|&p| p + (to - from)).collect();
                mesh_edit::polygon_mesh(&[points, shifted], false, closed)
            }
            _ => {
                let mut edges = Vec::new();
                for (slot, index) in [(0, 0), (1, 0), (2, 0), (0, 1)] {
                    edges.push(self.picked_curve(i, req, slot, index)?.0);
                }
                let ends: Vec<(DVec3, DVec3)> = edges
                    .iter()
                    .map(|e| ends_from(e, DVec3::ZERO).ok_or_else(|| crate::t!("Each edge must be an open curve.").into_owned()))
                    .collect::<Result<_, _>>()?;
                let size = ends.iter().map(|(a, b)| a.distance(*b)).fold(0.0, f64::max).max(1e-9);
                let touches = |p: DVec3, q: DVec3| p.distance(q) <= size * 1e-6;
                // The first edge runs u; the edges at its two ends run v; the last is opposite.
                let (a0, a1) = ends[0];
                let find = |at: DVec3, skip: &[usize]| {
                    (1..4).find(|k| !skip.contains(k) && (touches(ends[*k].0, at) || touches(ends[*k].1, at)))
                };
                let d0 = find(a0, &[]).ok_or_else(|| crate::t!("The edges must touch end to end, forming a closed loop.").into_owned())?;
                let d1 = find(a1, &[d0]).ok_or_else(|| crate::t!("The edges must touch end to end, forming a closed loop.").into_owned())?;
                let c = (1..4).find(|k| *k != d0 && *k != d1).ok_or("no edge")?;
                let along = |k: usize, steps: usize, start: DVec3| -> Result<Vec<DVec3>, String> {
                    let (mut points, _) = crate::modules::draw::inquiry::divide::points_along(&edges[k], steps)
                        .map(|(p, c)| (p.into_iter().map(point).collect::<Vec<_>>(), c))
                        .ok_or_else(not_curve)?;
                    if !touches(points[0], start) {
                        points.reverse();
                    }
                    Ok(points)
                };
                let a = along(0, m, a0)?;
                let v0 = along(d0, n, a0)?;
                let v1 = along(d1, n, a1)?;
                let opposite = along(c, m, *v0.last().ok_or("no edge")?)?;
                if !touches(*opposite.last().ok_or("no edge")?, *v1.last().ok_or("no edge")?) {
                    return Err(crate::t!("The edges must touch end to end, forming a closed loop.").into_owned());
                }
                mesh_edit::polygon_mesh(&mesh_edit::coons(&a, &opposite, &v0, &v1), false, false)
            }
        };
        mesh.ok_or_else(|| crate::t!("The mesh would be too large.").into_owned())
    }
}
