//! OpenCADStudio TDS material group indexing benchmark suite.
//!
//! A 3D Studio file's meshes carry material groups: each group names a
//! material and the faces it covers. The importer collects the materials a
//! mesh uses, and splits a multi-material mesh into one part per material.
//! Both steps used to scan the mesh's whole group list once per material of
//! the scene, so a file with many materials and many groups paid
//! materials x groups work per mesh.
//!
//! Measures the REAL production import path: `tds_import::read` followed by
//! `tds_import::convert` on a synthesized file with `MATERIALS` scene
//! materials, `OBJECTS` mesh objects and `GROUPS_PER_OBJECT` material
//! groups each (groups x objects groups in total, every group covering one
//! face). The bytes are built once; each timed iteration parses and
//! converts them again, as an import does.
//!
//! The sink hashes the produced entity count, every part's material name,
//! layer and geometry counts, the ordered import steps, the first-use
//! material list and the import messages, so both sides of the
//! optimization must produce the identical import.
//!
//! Deterministic: fixed names, fixed geometry, no randomness.
//! Run: `cargo bench --bench tds_material_groups`

use std::collections::HashMap;
use std::hint::black_box;
use std::time::Instant;

use OpenCADStudio::io::tds_import::{
    self, LayerMode, TdsImport, TdsOptions, TdsStep,
};

const MATERIALS: usize = 100;
const OBJECTS: usize = 5;
const GROUPS_PER_OBJECT: usize = 1000;
const FACES_PER_OBJECT: usize = 1000;
const RUNS: usize = 3;
const ITERATIONS: usize = 5;
const WARMUP: usize = 1;

fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + body.len());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&((6 + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

fn cstr(name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + 1);
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    out
}

fn material_chunk(name: &str) -> Vec<u8> {
    let mut body = cstr(name);
    body.extend_from_slice(&chunk(0xA020, &chunk(0x0011, &[180, 180, 180])));
    chunk(0xAFFF, &body)
}

fn mesh_chunk(object: usize) -> Vec<u8> {
    let mut vertices = Vec::with_capacity(2 + 36);
    vertices.extend_from_slice(&3u16.to_le_bytes());
    for k in 0..3 {
        for value in [object as f32 + k as f32, k as f32, 0.0] {
            vertices.extend_from_slice(&value.to_le_bytes());
        }
    }

    let mut faces = Vec::with_capacity(2 + 8 * FACES_PER_OBJECT + 64 * GROUPS_PER_OBJECT);
    faces.extend_from_slice(&(FACES_PER_OBJECT as u16).to_le_bytes());
    for _ in 0..FACES_PER_OBJECT {
        faces.extend_from_slice(&0u16.to_le_bytes());
        faces.extend_from_slice(&1u16.to_le_bytes());
        faces.extend_from_slice(&2u16.to_le_bytes());
        faces.extend_from_slice(&0u16.to_le_bytes());
    }
    for group in 0..GROUPS_PER_OBJECT {
        let material = group % MATERIALS;
        let mut body = cstr(&format!("mat{material:03}"));
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&(group as u16).to_le_bytes());
        faces.extend_from_slice(&chunk(0x4130, &body));
    }

    let mut mesh = chunk(0x4110, &vertices);
    mesh.extend_from_slice(&chunk(0x4120, &faces));
    mesh.extend_from_slice(&chunk(0x4165, &[7]));
    chunk(0x4100, &mesh)
}

fn build_3ds() -> Vec<u8> {
    let mut editor = Vec::new();
    for material in 0..MATERIALS {
        editor.extend_from_slice(&material_chunk(&format!("mat{material:03}")));
    }
    for object in 0..OBJECTS {
        let mut body = cstr(&format!("object{object}"));
        body.extend_from_slice(&mesh_chunk(object));
        editor.extend_from_slice(&chunk(0x4000, &body));
    }
    chunk(0x4D4D, &chunk(0x3D3D, &editor))
}

fn options() -> TdsOptions {
    TdsOptions {
        selected: (0..OBJECTS).map(|o| format!("object{o}")).collect(),
        layers: LayerMode::ByObject,
        multi: HashMap::new(),
        compact: false,
        single_layer: String::new(),
        stop_at: None,
    }
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(u64::from(byte));
    }
}

fn hash_str(hash: &mut u64, value: &str) {
    *hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(value.len() as u64);
    hash_bytes(hash, value.as_bytes());
}

/// A hash of the produced import: entity counts, each part's material and
/// layer, its geometry counts, the ordered steps and the first-use material
/// list. Every importer side must agree bit for bit.
fn sink(import: &TdsImport) -> u64 {
    let mut hash: u64 = 0x9e37_79b9_7f4a_7c15;
    hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(import.parts.len() as u64);
    hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(import.steps.len() as u64);
    hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(import.materials.len() as u64);
    hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(import.messages.len() as u64);
    for part in &import.parts {
        hash_str(&mut hash, &part.layer);
        match &part.material {
            Some(name) => {
                hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(1);
                hash_str(&mut hash, name);
            }
            None => hash = hash.wrapping_mul(0x100_0000_01b3),
        }
        hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(part.mesh.vertices.len() as u64);
        hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(part.mesh.faces.len() as u64);
        hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(part.color.map_or(u64::MAX, u64::from));
    }
    for step in &import.steps {
        match step {
            TdsStep::Part(index) => {
                hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(*index as u64 + 1);
            }
            TdsStep::Material(name) => {
                hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(1);
                hash_str(&mut hash, name);
            }
            TdsStep::Light { name, .. } => {
                hash = hash.wrapping_mul(0x100_0000_01b3).wrapping_add(2);
                hash_str(&mut hash, name);
            }
        }
    }
    for name in &import.materials {
        hash_str(&mut hash, name);
    }
    for message in &import.messages {
        hash_str(&mut hash, message);
    }
    hash
}

fn main() {
    let bytes = build_3ds();
    let opts = options();
    let first_scene = tds_import::read(&bytes).expect("the synthesized 3D Studio file must import");
    let first = tds_import::convert(&first_scene, &opts);
    println!(
        "tds_material_groups: {} bytes, {} materials, {} objects, {} groups, {} parts, {} steps",
        bytes.len(),
        first_scene.materials.len(),
        first_scene.objects.len(),
        OBJECTS * GROUPS_PER_OBJECT,
        first.parts.len(),
        first.steps.len(),
    );
    let expected = sink(&first);

    for _ in 0..WARMUP {
        let scene = tds_import::read(black_box(&bytes)).expect("the synthesized 3D Studio file must import");
        let import = tds_import::convert(black_box(&scene), black_box(&opts));
        black_box(sink(black_box(&import)));
    }
    let mut best = f64::MAX;
    for _ in 0..RUNS {
        let started = Instant::now();
        let mut value = 0u64;
        for _ in 0..ITERATIONS {
            let scene =
                tds_import::read(black_box(&bytes)).expect("the synthesized 3D Studio file must import");
            let import = tds_import::convert(black_box(&scene), black_box(&opts));
            value = sink(black_box(&import));
        }
        let per_import = started.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64;
        best = best.min(per_import);
        assert_eq!(value, expected, "every import must produce the same result");
        black_box(value);
    }
    println!(
        "tds_material_groups: {ITERATIONS} imports x {RUNS} runs; best {best:.3} ms/import; sink {expected:#x}"
    );
}
