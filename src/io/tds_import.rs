//! 3D Studio (.3ds) reading: the materials, meshes, lights and cameras of a
//! file's editor section, and their conversion to drawing objects — each
//! mesh a polyface mesh on a layer chosen by object, material, object colour
//! or a single layer, with its material; each camera a named view.

use std::collections::{HashMap, HashSet};

use document::entities::{PolyfaceFace, PolyfaceMesh, PolyfaceVertex};
use document::types::Vector3;

use crate::scene::sanitized_table_name;

#[derive(Debug)]
pub struct TdsMaterial {
    pub name: String,
    pub ambient: Option<[u8; 3]>,
    pub diffuse: Option<[u8; 3]>,
    pub specular: Option<[u8; 3]>,
    /// Shininess and transparency from 0 to 1 (single precision, as the
    /// file's percentages give them).
    pub shininess: Option<f32>,
    pub transparency: Option<f32>,
    /// The shading type (1 flat, 2 Gouraud, 3 Phong, 4 metal).
    pub shading: Option<u16>,
    /// The diffuse texture map: its file name and strength from 0 to 1.
    pub map: Option<(String, f32)>,
}

#[derive(Debug)]
pub struct TdsMesh {
    pub vertices: Vec<[f64; 3]>,
    pub faces: Vec<[usize; 3]>,
    /// Each material the mesh uses and the faces it covers, in file order.
    pub groups: Vec<(String, Vec<usize>)>,
    /// The object colour, a palette index, when the file gives one.
    pub color: Option<u8>,
}

#[derive(Debug)]
pub struct TdsCamera {
    pub position: [f64; 3],
    pub target: [f64; 3],
    pub lens: f64,
}

#[derive(Debug)]
pub struct TdsLight {
    pub position: [f64; 3],
    /// Red, green and blue from 0 to 1 (single precision, as the file
    /// holds them); white when the file gives none.
    pub color: [f64; 3],
    /// A spotlight: its target and its hotspot and falloff angles.
    pub spot: Option<([f64; 3], f64, f64)>,
}

#[derive(Debug)]
pub enum TdsObject {
    Mesh(TdsMesh),
    Light(TdsLight),
    Camera(TdsCamera),
}

impl TdsObject {
    /// The kind as the import window lists it.
    pub fn kind(&self) -> &'static str {
        match self {
            TdsObject::Mesh(_) => "Mesh",
            TdsObject::Light(_) => "Light",
            TdsObject::Camera(_) => "Camera",
        }
    }
}

#[derive(Debug)]
pub struct TdsScene {
    pub materials: Vec<TdsMaterial>,
    pub objects: Vec<(String, TdsObject)>,
}

impl TdsScene {
    /// The materials a mesh uses, in the order the file defines them.
    pub fn mesh_materials(&self, mesh: &TdsMesh) -> Vec<String> {
        // One pass over the groups instead of a full scan per scene material.
        let present: HashSet<&str> = mesh
            .groups
            .iter()
            .filter(|(_, faces)| !faces.is_empty())
            .map(|(name, _)| name.as_str())
            .collect();
        let mut used: Vec<String> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for m in &self.materials {
            if present.contains(m.name.as_str()) {
                used.push(m.name.clone());
                seen.insert(m.name.as_str());
            }
        }
        for (name, faces) in &mesh.groups {
            if !faces.is_empty() && seen.insert(name.as_str()) {
                used.push(name.clone());
            }
        }
        used
    }
}

struct Reader<'a> {
    b: &'a [u8],
}

impl Reader<'_> {
    fn u16(&self, at: usize) -> Option<u16> {
        Some(u16::from_le_bytes([*self.b.get(at)?, *self.b.get(at + 1)?]))
    }
    fn u32(&self, at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.b.get(at..at + 4)?.try_into().ok()?))
    }
    fn f32(&self, at: usize) -> Option<f64> {
        Some(f64::from(f32::from_le_bytes(
            self.b.get(at..at + 4)?.try_into().ok()?,
        )))
    }
    fn point(&self, at: usize) -> Option<[f64; 3]> {
        Some([self.f32(at)?, self.f32(at + 4)?, self.f32(at + 8)?])
    }
    /// A zero-terminated string and the offset after it.
    fn cstr(&self, at: usize, end: usize) -> (String, usize) {
        let stop = self.b[at.min(end)..end]
            .iter()
            .position(|c| *c == 0)
            .map_or(end, |k| at + k);
        (
            String::from_utf8_lossy(&self.b[at.min(stop)..stop]).into_owned(),
            (stop + 1).min(end),
        )
    }
    /// The chunks between `from` and `to`: id, body start, body end.
    fn chunks(&self, from: usize, to: usize) -> Vec<(u16, usize, usize)> {
        let mut out = Vec::new();
        let mut at = from;
        while at + 6 <= to {
            let (Some(id), Some(len)) = (self.u16(at), self.u32(at + 2)) else {
                break;
            };
            let len = len as usize;
            if len < 6 || len > to - at {
                break;
            }
            out.push((id, at + 6, at + len));
            at += len;
        }
        out
    }
    /// A percentage chunk's value from 0 to 1: its integer (0x0030) or
    /// float (0x0031) percentage.
    fn percent(&self, from: usize, to: usize) -> Option<f32> {
        self.chunks(from, to)
            .into_iter()
            .find_map(|(id, start, _)| self.percent_value(id, start))
    }
    fn percent_value(&self, id: u16, start: usize) -> Option<f32> {
        match id {
            0x0030 => Some(f32::from(self.u16(start)?) / 100.0),
            0x0031 => Some(self.f32(start)? as f32 / 100.0),
            _ => None,
        }
    }
    /// A colour chunk's 8-bit RGB: 24-bit or float, plain before linear.
    fn color(&self, from: usize, to: usize) -> Option<[u8; 3]> {
        let chunks = self.chunks(from, to);
        for want in [0x0011, 0x0010, 0x0012, 0x0013] {
            for &(id, start, _) in &chunks {
                if id != want {
                    continue;
                }
                return Some(match id {
                    0x0011 | 0x0012 => [
                        *self.b.get(start)?,
                        *self.b.get(start + 1)?,
                        *self.b.get(start + 2)?,
                    ],
                    _ => self
                        .point(start)?
                        .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8),
                });
            }
        }
        None
    }
}

/// `None` when the bytes are no 3D Studio file.
pub fn read(bytes: &[u8]) -> Option<TdsScene> {
    let r = Reader { b: bytes };
    if r.u16(0)? != 0x4D4D {
        return None;
    }
    let end = (r.u32(2)? as usize).min(bytes.len());
    let mut scene = TdsScene {
        materials: Vec::new(),
        objects: Vec::new(),
    };
    for (id, start, stop) in r.chunks(6, end) {
        if id != 0x3D3D {
            continue;
        }
        for (id, start, stop) in r.chunks(start, stop) {
            match id {
                0xAFFF => {
                    let mut material = TdsMaterial {
                        name: String::new(),
                        ambient: None,
                        diffuse: None,
                        specular: None,
                        shininess: None,
                        transparency: None,
                        shading: None,
                        map: None,
                    };
                    for (id, s, e) in r.chunks(start, stop) {
                        match id {
                            0xA000 => material.name = r.cstr(s, e).0,
                            0xA010 => material.ambient = r.color(s, e),
                            0xA020 => material.diffuse = r.color(s, e),
                            0xA030 => material.specular = r.color(s, e),
                            0xA040 => material.shininess = r.percent(s, e),
                            0xA050 => material.transparency = r.percent(s, e),
                            0xA100 => material.shading = r.u16(s),
                            0xA200 => {
                                let mut name = String::new();
                                let mut strength = None;
                                for (id, s, e) in r.chunks(s, e) {
                                    match id {
                                        0xA300 => name = r.cstr(s, e).0,
                                        0x0030 | 0x0031 => strength = r.percent_value(id, s),
                                        _ => {}
                                    }
                                }
                                if !name.is_empty() {
                                    material.map = Some((name, strength.unwrap_or(1.0)));
                                }
                            }
                            _ => {}
                        }
                    }
                    scene.materials.push(material);
                }
                0x4000 => {
                    let (name, body) = r.cstr(start, stop);
                    for (id, s, e) in r.chunks(body, stop) {
                        let object = match id {
                            0x4100 => Some(TdsObject::Mesh(read_mesh(&r, s, e))),
                            0x4600 => read_light(&r, s, e).map(TdsObject::Light),
                            0x4700 => r.point(s).zip(r.point(s + 12)).map(|(position, target)| {
                                TdsObject::Camera(TdsCamera {
                                    position,
                                    target,
                                    lens: r.f32(s + 28).filter(|l| *l > 0.0).unwrap_or(50.0),
                                })
                            }),
                            _ => continue,
                        };
                        // A short light or camera chunk drops that object,
                        // not the file.
                        scene
                            .objects
                            .extend(object.map(|object| (name.clone(), object)));
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    Some(scene)
}

fn read_light(r: &Reader<'_>, from: usize, to: usize) -> Option<TdsLight> {
    let position = r.point(from)?;
    let chunks = r.chunks(from + 12, to);
    let color = chunks
        .iter()
        .find_map(|&(id, s, _)| match id {
            0x0010 | 0x0013 => r.point(s).map(|c| c.map(|v| f64::from(v as f32))),
            0x0011 | 0x0012 => {
                let c = r.b.get(s..s + 3)?;
                Some([c[0], c[1], c[2]].map(|v| f64::from(f32::from(v) / 255.0)))
            }
            _ => None,
        })
        .unwrap_or([1.0; 3]);
    Some(TdsLight {
        position,
        color,
        spot: chunks
            .iter()
            .find(|&&(id, ..)| id == 0x4610)
            .and_then(|&(_, s, _)| Some((r.point(s)?, r.f32(s + 12)?, r.f32(s + 16)?))),
    })
}

fn read_mesh(r: &Reader<'_>, from: usize, to: usize) -> TdsMesh {
    let mut mesh = TdsMesh {
        vertices: Vec::new(),
        faces: Vec::new(),
        groups: Vec::new(),
        color: None,
    };
    for (id, s, e) in r.chunks(from, to) {
        match id {
            0x4110 => {
                let n = r.u16(s).unwrap_or(0) as usize;
                mesh.vertices = (0..n).filter_map(|k| r.point(s + 2 + 12 * k)).collect();
            }
            0x4120 => {
                let n = r.u16(s).unwrap_or(0) as usize;
                mesh.faces = (0..n)
                    .filter_map(|k| {
                        let at = s + 2 + 8 * k;
                        Some([
                            r.u16(at)? as usize,
                            r.u16(at + 2)? as usize,
                            r.u16(at + 4)? as usize,
                        ])
                    })
                    .collect();
                for (id, gs, ge) in r.chunks(s + 2 + 8 * n, e) {
                    if id == 0x4130 {
                        let (name, at) = r.cstr(gs, ge);
                        let count = r.u16(at).unwrap_or(0) as usize;
                        let faces = (0..count)
                            .filter_map(|k| r.u16(at + 2 + 2 * k).map(usize::from))
                            .collect();
                        mesh.groups.push((name, faces));
                    }
                }
            }
            0x4165 => mesh.color = r.b.get(s).copied(),
            _ => {}
        }
    }
    mesh
}

/// Where imported objects go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayerMode {
    #[default]
    ByObject,
    ByMaterial,
    ByObjectColor,
    Single,
}

/// What a mesh with several materials becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultiMaterial {
    /// One mesh per material.
    Split,
    /// One mesh with the first material the file defines.
    First,
    /// One mesh with this material, or none.
    Material(Option<String>),
}

pub struct TdsOptions {
    /// The objects to import, by name ("Global Settings" included).
    pub selected: Vec<String>,
    pub layers: LayerMode,
    /// The choice for each mesh with several materials, by name.
    pub multi: HashMap<String, MultiMaterial>,
    /// Whether meshes keep only the vertices their faces use, in the order
    /// they first use them — as they do when the import splits by
    /// material or asks — rather than the file's vertex list.
    pub compact: bool,
    /// The layer every mesh goes on with [`LayerMode::Single`].
    pub single_layer: String,
    /// The mesh whose material question was cancelled: the import stops
    /// there, keeping what came before it.
    pub stop_at: Option<String>,
}

/// One imported mesh.
#[derive(Debug, Clone)]
pub struct TdsPart {
    pub layer: String,
    pub material: Option<String>,
    /// The index colour the object's colour imports as.
    pub color: Option<u8>,
    pub mesh: PolyfaceMesh,
}

/// The index colour each 3D Studio object colour (0–63) imports as; a
/// larger number leaves the colour by layer.
const OBJECT_ACI: [u8; 64] = [
    7, 251, 1, 3, 5, 2, 6, 4, 110, 120, 4, 140, 150, 150, 160, 5, //
    180, 190, 190, 213, 6, 220, 223, 230, 242, 12, 22, 42, 52, 40, 52, 62, //
    254, 254, 254, 254, 254, 254, 21, 31, 51, 71, 81, 111, 131, 151, 254, 191, //
    211, 231, 254, 21, 31, 51, 71, 81, 91, 101, 111, 131, 151, 153, 171, 171,
];

/// What the import adds, in order.
#[derive(Debug, Clone)]
pub enum TdsStep {
    /// The part at this index of [`TdsImport::parts`].
    Part(usize),
    /// The legacy render record of a material, after the first part that
    /// uses it.
    Material(String),
    /// A light, as the legacy light block (a spotlight's with its target,
    /// hotspot and falloff).
    Light {
        name: String,
        position: [f64; 3],
        color: [f64; 3],
        spot: Option<([f64; 3], f64, f64)>,
    },
}

#[derive(Debug, Clone)]
pub struct TdsCameraView {
    pub name: String,
    pub target: [f64; 3],
    pub direction: [f64; 3],
    /// The view height at the target, from the lens.
    pub height: f64,
    pub lens: f64,
}

#[derive(Debug, Clone)]
pub struct TdsImport {
    pub parts: Vec<TdsPart>,
    /// The parts, materials and lights in the order they are added.
    pub steps: Vec<TdsStep>,
    /// The global settings were chosen: the legacy render settings blocks
    /// come first.
    pub globals: bool,
    /// The materials the parts use, in the order they were first used.
    pub materials: Vec<String>,
    pub views: Vec<TdsCameraView>,
    pub messages: Vec<String>,
    /// The texture maps not found, as (map file, material), in the order
    /// their materials were converted.
    pub missing_maps: Vec<(String, String)>,
}

/// Polyface meshes of `faces` of `mesh`: its vertices in file order, or
/// (`compact`) only those the faces use, in the order they first use them.
/// A polyface mesh numbers its vertices in 16 bits, so a mesh with more
/// vertices than that is always compact, and the faces that need further
/// vertices go on further meshes.
fn polyfaces(mesh: &TdsMesh, faces: &[usize], compact: bool) -> Vec<PolyfaceMesh> {
    const MAX_VERTICES: usize = i16::MAX as usize;
    let compact = compact || mesh.vertices.len() > MAX_VERTICES;
    let vertex = |v: usize| {
        let p = mesh.vertices[v];
        PolyfaceVertex::new(Vector3::new(p[0], p[1], p[2]))
    };
    let mut out = Vec::new();
    let mut current = PolyfaceMesh::new();
    let mut index: HashMap<usize, i16> = HashMap::new();
    if !compact {
        for k in 0..mesh.vertices.len() {
            index.insert(k, current.add_vertex(vertex(k)));
        }
    }
    for &f in faces {
        let Some(face) = mesh.faces.get(f) else {
            continue;
        };
        if face.iter().any(|v| *v >= mesh.vertices.len()) {
            continue;
        }
        let new = face.iter().filter(|v| !index.contains_key(v)).count();
        if current.vertices.len() + new > MAX_VERTICES {
            out.push(std::mem::replace(&mut current, PolyfaceMesh::new()));
            index.clear();
        }
        let ids = face.map(|v| {
            *index
                .entry(v)
                .or_insert_with(|| current.add_vertex(vertex(v)))
        });
        current.add_face(PolyfaceFace::triangle(ids[0], ids[1], ids[2]));
    }
    out.push(current);
    out
}

/// The view height a camera's lens sees at the target distance.
// ponytail: one measured camera (50 mm at 86.46 units gives 30.457); the
// height is taken as proportional to distance over lens.
fn view_height(distance: f64, lens: f64) -> f64 {
    distance * 17.6141 / lens
}

pub fn convert(scene: &TdsScene, options: &TdsOptions) -> TdsImport {
    let mut out = TdsImport {
        parts: Vec::new(),
        steps: Vec::new(),
        globals: options.selected.iter().any(|s| s == "Global Settings"),
        materials: Vec::new(),
        views: Vec::new(),
        messages: Vec::new(),
        missing_maps: Vec::new(),
    };
    let chosen = |name: &str| options.selected.iter().any(|s| s == name);
    // The objects are taken in the order the window lists them, by name.
    let mut objects: Vec<&(String, TdsObject)> = scene.objects.iter().collect();
    objects.sort_by_key(|(name, _)| name.to_lowercase());
    for (name, object) in objects {
        if !chosen(name) {
            continue;
        }
        if options.stop_at.as_ref() == Some(name) {
            // The mesh the cancelled material question stopped at.
            out.messages.push(format!("Processing object {name}"));
            out.messages.push(format!("Import stopped at {name}"));
            return out;
        }
        match object {
            TdsObject::Mesh(mesh) => {
                out.messages.push(format!("Processing object {name}"));
                let materials = scene.mesh_materials(mesh);
                let all: Vec<usize> = (0..mesh.faces.len()).collect();
                let pieces: Vec<(Option<String>, Vec<usize>)> = if materials.len() > 1 {
                    match options
                        .multi
                        .get(name)
                        .cloned()
                        .unwrap_or(MultiMaterial::Split)
                    {
                        MultiMaterial::Split => {
                            // Index the groups once; the old code scanned the
                            // whole list again for every material.
                            let mut by_material: HashMap<&str, Vec<usize>> = HashMap::new();
                            for (group_name, faces) in &mesh.groups {
                                if !faces.is_empty() {
                                    by_material
                                        .entry(group_name.as_str())
                                        .or_default()
                                        .extend_from_slice(faces);
                                }
                            }
                            materials
                                .iter()
                                .map(|m| {
                                    let faces =
                                        by_material.get(m.as_str()).cloned().unwrap_or_default();
                                    (Some(m.clone()), faces)
                                })
                                .collect()
                        }
                        MultiMaterial::First => vec![(materials.first().cloned(), all)],
                        MultiMaterial::Material(m) => vec![(m, all)],
                    }
                } else {
                    vec![(materials.first().cloned(), all)]
                };
                let compact = options.compact || pieces.len() > 1;
                for (material, faces) in pieces {
                    let mut first_use = None;
                    if let Some(m) = &material {
                        if !out.materials.contains(m) {
                            out.messages.push(format!("Converting material {m}"));
                            // A texture map is looked for in the search path
                            // (the current folder), not beside the file.
                            let map = scene
                                .materials
                                .iter()
                                .find(|x| x.name == *m)
                                .and_then(|x| x.map.as_ref());
                            if let Some((file, _)) = map {
                                let found = std::env::current_dir()
                                    .is_ok_and(|dir| dir.join(file).is_file());
                                if !found {
                                    out.messages
                                        .push(format!("Cannot find texture map file \"{file}\"."));
                                    out.missing_maps.push((file.clone(), m.clone()));
                                }
                            }
                            out.materials.push(m.clone());
                            first_use = Some(m.clone());
                        }
                    }
                    // Object and material names come from the file, so they
                    // are made fit to name a layer.
                    let layer = match options.layers {
                        LayerMode::ByObject => sanitized_table_name(&name.to_uppercase(), "0"),
                        LayerMode::ByMaterial => sanitized_table_name(
                            &material.as_deref().unwrap_or("0").to_uppercase(),
                            "0",
                        ),
                        LayerMode::ByObjectColor => match mesh.color {
                            Some(c) => format!("COLOR{c:02}"),
                            None => "COLORNONE".to_string(),
                        },
                        LayerMode::Single => options.single_layer.clone(),
                    };
                    for polyface in polyfaces(mesh, &faces, compact) {
                        out.steps.push(TdsStep::Part(out.parts.len()));
                        out.parts.push(TdsPart {
                            layer: layer.clone(),
                            material: material.clone(),
                            color: mesh
                                .color
                                .and_then(|c| OBJECT_ACI.get(usize::from(c)).copied()),
                            mesh: polyface,
                        });
                    }
                    out.steps.extend(first_use.map(TdsStep::Material));
                }
            }
            TdsObject::Camera(camera) => {
                let name = sanitized_table_name(&name.to_uppercase(), "CAMERA");
                out.messages.push(format!("Creating camera {name}"));
                let direction = [0, 1, 2].map(|k| camera.position[k] - camera.target[k]);
                let distance = direction.iter().map(|d| d * d).sum::<f64>().sqrt();
                out.views.push(TdsCameraView {
                    name,
                    target: camera.target,
                    direction,
                    height: view_height(distance, camera.lens),
                    lens: camera.lens,
                });
            }
            TdsObject::Light(light) => out.steps.push(TdsStep::Light {
                name: name.to_uppercase(),
                position: light.position,
                color: light.color,
                spot: light.spot,
            }),
        }
    }
    out.messages
        .push("3D Studio file import completed".to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: &[u8] = include_bytes!("../../tests/fixtures/tds/box.3ds");
    const TWO_MATERIALS: &[u8] = include_bytes!("../../tests/fixtures/tds/two_materials.3ds");
    const LIGHTS_CAMERA: &[u8] = include_bytes!("../../tests/fixtures/tds/lights_camera.3ds");
    const TEXTURED: &[u8] = include_bytes!("../../tests/fixtures/tds/textured.3ds");
    const OBJECTS: &[u8] = include_bytes!("../../tests/fixtures/tds/objects.3ds");

    /// The file holds single-precision numbers (Blender writes the camera's
    /// target z as -20.000023).
    fn assert_close(actual: [f64; 3], expected: [f64; 3], what: &str) {
        let near = (0..3).all(|k| (actual[k] - expected[k]).abs() < 1e-4);
        assert!(near, "{what}: {actual:?}, expected {expected:?}");
    }

    fn bounds(points: &[[f64; 3]]) -> ([f64; 3], [f64; 3]) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in points {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    fn options(selected: &[&str]) -> TdsOptions {
        TdsOptions {
            selected: selected.iter().map(|s| s.to_string()).collect(),
            layers: LayerMode::ByObject,
            multi: HashMap::new(),
            compact: false,
            single_layer: "IMPORTED".to_string(),
            stop_at: None,
        }
    }

    fn object<'a>(scene: &'a TdsScene, name: &str) -> &'a TdsObject {
        &scene
            .objects
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("no object {name}"))
            .1
    }

    fn layers(import: &TdsImport) -> Vec<&str> {
        import.parts.iter().map(|p| p.layer.as_str()).collect()
    }

    /// A chunk: its id, its length (header included), its body.
    fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
        let mut out = id.to_le_bytes().to_vec();
        out.extend((6 + body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    #[test]
    fn a_box_is_read_with_its_material() {
        let scene = read(BOX).expect("a 3D Studio file");
        let [red] = &scene.materials[..] else {
            panic!("one material");
        };
        assert_eq!(red.name, "Red");
        assert_eq!(red.ambient, Some([255, 255, 255]));
        assert_eq!(red.diffuse, Some([204, 25, 25]));
        assert_eq!(red.specular, Some([255, 255, 255]));
        assert_eq!(red.shininess, Some(0.6));
        assert_eq!(red.transparency, Some(0.25));
        assert_eq!(red.shading, Some(3));
        assert!(red.map.is_none());
        let [(name, TdsObject::Mesh(mesh))] = &scene.objects[..] else {
            panic!("one mesh");
        };
        assert_eq!(name, "Box");
        assert_eq!((mesh.vertices.len(), mesh.faces.len()), (14, 12));
        let (lo, hi) = bounds(&mesh.vertices);
        assert_close(lo, [9.0, 18.5, 28.0], "lowest corner");
        assert_close(hi, [11.0, 21.5, 32.0], "highest corner");
        assert!(mesh.faces.iter().flatten().all(|v| *v < 14));
        let [(material, faces)] = &mesh.groups[..] else {
            panic!("one material group");
        };
        assert_eq!(material, "Red");
        let mut faces = faces.clone();
        faces.sort_unstable();
        assert_eq!(faces, (0..12).collect::<Vec<_>>());
        assert_eq!(mesh.color, None);
    }

    #[test]
    fn lights_and_a_camera_are_read() {
        let scene = read(LIGHTS_CAMERA).expect("a 3D Studio file");
        let kinds: Vec<(&str, &str)> = scene
            .objects
            .iter()
            .map(|(name, o)| (name.as_str(), o.kind()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("Floor", "Mesh"),
                ("Lamp", "Light"),
                ("Spot", "Light"),
                ("Camera", "Camera")
            ]
        );
        // The mesh's own vertices, not the keyframer section that follows.
        let TdsObject::Mesh(floor) = object(&scene, "Floor") else {
            panic!("a mesh");
        };
        let (lo, hi) = bounds(&floor.vertices);
        assert_close(lo, [-5.0, -5.0, -0.2], "floor, lowest corner");
        assert_close(hi, [5.0, 5.0, 0.0], "floor, highest corner");
        let TdsObject::Light(lamp) = object(&scene, "Lamp") else {
            panic!("a light");
        };
        assert_close(lamp.position, [4.0, -3.0, 5.0], "lamp position");
        assert_close(lamp.color, [1.0, 0.5, 0.25], "lamp colour");
        assert!(lamp.spot.is_none());
        let TdsObject::Light(spot) = object(&scene, "Spot") else {
            panic!("a light");
        };
        assert_close(spot.position, [0.0, 0.0, 8.0], "spot position");
        assert_close(spot.color, [0.25, 0.5, 1.0], "spot colour");
        let (target, hotspot, falloff) = spot.spot.expect("a spotlight");
        assert_close(target, [0.0, 0.0, -8.0], "spot target");
        assert_eq!((hotspot, falloff), (45.0, 60.0));
        let TdsObject::Camera(camera) = object(&scene, "Camera") else {
            panic!("a camera");
        };
        assert_close(camera.position, [0.0, -20.0, 10.0], "camera position");
        assert_close(camera.target, [0.0, 20.0, -20.0], "camera target");
        assert_eq!(camera.lens, 35.0);
    }

    #[test]
    fn a_texture_map_is_read_and_a_missing_one_reported() {
        let scene = read(TEXTURED).expect("a 3D Studio file");
        assert_eq!(
            scene.materials[0].map,
            Some(("checker.png".to_string(), 1.0))
        );
        let import = convert(&scene, &options(&["Tile"]));
        assert_eq!(
            import.missing_maps,
            [("checker.png".to_string(), "Checker".to_string())]
        );
        assert!(import
            .messages
            .contains(&"Cannot find texture map file \"checker.png\".".to_string()));
    }

    #[test]
    fn a_mesh_with_two_materials_is_split_or_given_one() {
        let scene = read(TWO_MATERIALS).expect("a 3D Studio file");
        let TdsObject::Mesh(mesh) = object(&scene, "Crate") else {
            panic!("a mesh");
        };
        assert_eq!(scene.mesh_materials(mesh), ["Green", "Blue"]);
        let parts = |import: &TdsImport| -> Vec<(Option<String>, usize, usize)> {
            import
                .parts
                .iter()
                .map(|p| {
                    let m = &p.mesh;
                    (p.material.clone(), m.faces.len(), m.vertices.len())
                })
                .collect()
        };

        // Split, the default: one mesh per material, each with only the
        // vertices its faces use.
        let split = convert(&scene, &options(&["Crate"]));
        let got = parts(&split);
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].0.as_deref(), got[0].1), (Some("Green"), 8));
        assert_eq!((got[1].0.as_deref(), got[1].1), (Some("Blue"), 4));
        for part in &split.parts {
            let mut used: Vec<i16> = part
                .mesh
                .faces
                .iter()
                .flat_map(|f| [f.index1, f.index2, f.index3])
                .collect();
            used.sort_unstable();
            used.dedup();
            assert_eq!(
                used,
                (1..=part.mesh.vertices.len() as i16).collect::<Vec<_>>()
            );
        }
        assert_eq!(split.materials, ["Green", "Blue"]);
        let order: Vec<String> = split
            .steps
            .iter()
            .map(|s| match s {
                TdsStep::Part(k) => format!("part {k}"),
                TdsStep::Material(m) => format!("material {m}"),
                TdsStep::Light { name, .. } => format!("light {name}"),
            })
            .collect();
        assert_eq!(
            order,
            ["part 0", "material Green", "part 1", "material Blue"]
        );

        let mut by_material = options(&["Crate"]);
        by_material.layers = LayerMode::ByMaterial;
        assert_eq!(layers(&convert(&scene, &by_material)), ["GREEN", "BLUE"]);

        // One mesh: the first material the file defines, or a chosen one,
        // or none; all faces and the file's vertex list.
        for (choice, material) in [
            (MultiMaterial::First, Some("Green")),
            (
                MultiMaterial::Material(Some("Blue".to_string())),
                Some("Blue"),
            ),
            (MultiMaterial::Material(None), None),
        ] {
            let mut one = options(&["Crate"]);
            one.multi.insert("Crate".to_string(), choice);
            let import = convert(&scene, &one);
            assert_eq!(parts(&import), [(material.map(String::from), 12, 14)]);
        }
    }

    #[test]
    fn objects_are_taken_by_name_onto_layers() {
        let scene = read(OBJECTS).expect("a 3D Studio file");
        let all = ["zeta", "Alpha", "mid/part", "Beta"];
        // In name order, whatever order the file has; `/` cannot be in a
        // layer name.
        let import = convert(&scene, &options(&all));
        assert_eq!(layers(&import), ["ALPHA", "BETA", "MID_PART", "ZETA"]);
        let materials: Vec<Option<&str>> =
            import.parts.iter().map(|p| p.material.as_deref()).collect();
        assert_eq!(
            materials,
            [
                Some("M_Alpha"),
                Some("None"),
                Some("M_mid_part"),
                Some("M_zeta")
            ]
        );
        assert_eq!(
            import.messages.last().map(String::as_str),
            Some("3D Studio file import completed")
        );

        assert_eq!(
            layers(&convert(&scene, &options(&["zeta", "Beta"]))),
            ["BETA", "ZETA"]
        );

        let mut stopped = options(&all);
        stopped.stop_at = Some("mid/part".to_string());
        let import = convert(&scene, &stopped);
        assert_eq!(layers(&import), ["ALPHA", "BETA"]);
        assert_eq!(
            import.messages.last().map(String::as_str),
            Some("Import stopped at mid/part")
        );

        let mut single = options(&all);
        single.layers = LayerMode::Single;
        assert_eq!(layers(&convert(&scene, &single)), ["IMPORTED"; 4]);
        let mut by_colour = options(&all);
        by_colour.layers = LayerMode::ByObjectColor;
        assert_eq!(layers(&convert(&scene, &by_colour)), ["COLORNONE"; 4]);
    }

    #[test]
    fn a_camera_becomes_a_view_and_lights_become_steps() {
        let scene = read(LIGHTS_CAMERA).expect("a 3D Studio file");
        let import = convert(&scene, &options(&["Camera", "Lamp", "Spot"]));
        assert!(import.parts.is_empty());
        let [view] = &import.views[..] else {
            panic!("one view");
        };
        assert_eq!(view.name, "CAMERA");
        assert_close(view.target, [0.0, 20.0, -20.0], "view target");
        assert_close(view.direction, [0.0, -40.0, 30.0], "view direction");
        assert_eq!(view.lens, 35.0);
        // 50 units from the target with a 35 mm lens.
        assert!((view.height - 50.0 * 17.6141 / 35.0).abs() < 1e-4);
        let lights: Vec<(&str, bool)> = import
            .steps
            .iter()
            .filter_map(|s| match s {
                TdsStep::Light { name, spot, .. } => Some((name.as_str(), spot.is_some())),
                _ => None,
            })
            .collect();
        assert_eq!(lights, [("LAMP", false), ("SPOT", true)]);
    }

    /// A mesh with more vertices than a polyface mesh can number (32,767)
    /// goes on several, and an object colour picks the index colour; built
    /// here, since Blender writes neither.
    #[test]
    fn a_mesh_too_big_for_one_polyface_mesh_is_split() {
        const TRIANGLES: usize = 13_334;
        let vertex_count = 3 * TRIANGLES;
        let mut vertices = (vertex_count as u16).to_le_bytes().to_vec();
        for k in 0..vertex_count {
            for c in [k as f32, (k % 7) as f32, 1.0] {
                vertices.extend(c.to_le_bytes());
            }
        }
        // Each triangle has its own three vertices; one more face names a
        // vertex the mesh does not have and is left out.
        let mut faces = ((TRIANGLES + 1) as u16).to_le_bytes().to_vec();
        for t in 0..TRIANGLES {
            for v in [3 * t, 3 * t + 1, 3 * t + 2] {
                faces.extend((v as u16).to_le_bytes());
            }
            faces.extend(0u16.to_le_bytes());
        }
        for v in [0u16, 1, 65_000, 0] {
            faces.extend(v.to_le_bytes());
        }
        let mesh = [
            chunk(0x4110, &vertices),
            chunk(0x4120, &faces),
            chunk(0x4165, &[2]),
        ]
        .concat();
        let object = [b"Big\0".as_slice(), &chunk(0x4100, &mesh)].concat();
        let file = chunk(0x4D4D, &chunk(0x3D3D, &chunk(0x4000, &object)));

        let scene = read(&file).expect("a 3D Studio file");
        let mut colours = options(&["Big"]);
        colours.layers = LayerMode::ByObjectColor;
        let import = convert(&scene, &colours);
        assert_eq!(import.parts.len(), 2);
        let faces: usize = import.parts.iter().map(|p| p.mesh.faces.len()).sum();
        let vertices: usize = import.parts.iter().map(|p| p.mesh.vertices.len()).sum();
        assert_eq!((faces, vertices), (TRIANGLES, vertex_count));
        for part in &import.parts {
            assert!(part.mesh.vertices.len() <= 32_767);
            assert_eq!((part.layer.as_str(), part.color), ("COLOR02", Some(1)));
        }
        let last = import.parts[1].mesh.vertices.last().expect("vertices");
        let k = (vertex_count - 1) as f64;
        assert_close(
            [last.location.x, last.location.y, last.location.z],
            [k, k % 7.0, 1.0],
            "last vertex",
        );
    }

    /// Colours and percentages in the forms Blender does not write: float
    /// and linear colours, a 24-bit colour preferred to a float one, float
    /// percentages, a texture map without a strength (full strength), and a
    /// light's 24-bit colour; built here.
    #[test]
    fn colours_and_percentages_in_their_other_forms_are_read() {
        let floats = |v: [f32; 3]| -> Vec<u8> { v.iter().flat_map(|c| c.to_le_bytes()).collect() };
        let material = [
            chunk(0xA000, b"Other\0"),
            // Ambient: linear float only.
            chunk(0xA010, &chunk(0x0013, &floats([0.2, 0.4, 0.6]))),
            // Diffuse: a float colour and a 24-bit one; the 24-bit one wins.
            chunk(
                0xA020,
                &[
                    chunk(0x0010, &floats([0.5, 0.25, 1.0])),
                    chunk(0x0011, &[10, 20, 30]),
                ]
                .concat(),
            ),
            // Specular: a float colour, clamped to 0-1.
            chunk(0xA030, &chunk(0x0010, &floats([0.5, -1.0, 2.0]))),
            chunk(0xA040, &chunk(0x0031, &37.5f32.to_le_bytes())),
            chunk(0xA050, &chunk(0x0031, &12.5f32.to_le_bytes())),
        ]
        .concat();
        let light = [floats([1.0, 2.0, 3.0]), chunk(0x0011, &[255, 51, 0])].concat();
        let bulb = [b"Bulb\0".as_slice(), &chunk(0x4600, &light)].concat();
        let mapped = [
            chunk(0xA000, b"Mapped\0"),
            chunk(0xA200, &chunk(0xA300, b"wood.jpg\0")),
        ]
        .concat();
        let editor = [
            chunk(0xAFFF, &material),
            chunk(0xAFFF, &mapped),
            chunk(0x4000, &bulb),
        ]
        .concat();
        let scene = read(&chunk(0x4D4D, &chunk(0x3D3D, &editor))).expect("a 3D Studio file");

        let m = &scene.materials[0];
        assert_eq!(m.name, "Other");
        assert_eq!(m.ambient, Some([51, 102, 153]));
        assert_eq!(m.diffuse, Some([10, 20, 30]));
        assert_eq!(m.specular, Some([128, 0, 255]));
        assert_eq!((m.shininess, m.transparency), (Some(0.375), Some(0.125)));
        assert_eq!(scene.materials[1].map, Some(("wood.jpg".to_string(), 1.0)));
        let TdsObject::Light(bulb) = object(&scene, "Bulb") else {
            panic!("a light");
        };
        assert_close(bulb.position, [1.0, 2.0, 3.0], "light position");
        assert_close(bulb.color, [1.0, 0.2, 0.0], "light colour");
    }

    /// Every truncation of every fixture, and every byte of each set to 0,
    /// to 0xFF and with its top bit flipped, is read and converted without
    /// a panic.
    #[test]
    fn damaged_files_never_panic() {
        let mut cases = 0;
        let mut check = |bytes: &[u8]| {
            cases += 1;
            if let Some(scene) = read(bytes) {
                let names: Vec<&str> = scene.objects.iter().map(|(n, _)| n.as_str()).collect();
                let _ = convert(&scene, &options(&names));
            }
        };
        for fixture in [BOX, TWO_MATERIALS, LIGHTS_CAMERA, TEXTURED, OBJECTS] {
            for end in 0..fixture.len() {
                check(&fixture[..end]);
            }
            let mut damaged = fixture.to_vec();
            for at in 0..fixture.len() {
                for value in [0x00, 0xFF, fixture[at] ^ 0x80] {
                    damaged[at] = value;
                    check(&damaged);
                }
                damaged[at] = fixture[at];
            }
        }
        assert!(cases > 25_000, "{cases} damaged files");
    }
}
