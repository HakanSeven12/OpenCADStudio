use document::entities::acis::{SabReader, SatDocument};
use document::entities::{Body, Region, Solid3D, Surface};
use ocs_solid::model::Brep;

use crate::scene::model::mesh_model::{MeshLodSet, MeshModel};

fn tessellate_acis(
    sat: &SatDocument,
    name: String,
    color: [f32; 4],
    facet_res: f64,
    chordal_deflection: Option<f64>,
    isolines: [usize; 2],
    planar_isolines: Option<bool>,
) -> Option<MeshLodSet> {
    crate::scene::convert::acis_kernel::tessellate_sat(
        sat,
        name,
        color,
        facet_res,
        chordal_deflection,
        isolines,
        planar_isolines,
    )
}

pub(crate) fn finalize_mesh(
    name: String,
    verts: Vec<[f64; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
    triangle_material_handles: Vec<Option<document::Handle>>,
    triangle_colors: Vec<Option<[f32; 4]>>,
    color: [f32; 4],
    xform: Option<([f64; 9], [f64; 3], f64)>,
) -> MeshModel {
    let mut verts_high = Vec::with_capacity(verts.len());
    let mut verts_low = Vec::with_capacity(verts.len());
    for [x, y, z] in verts {
        let [x, y, z] = match xform {
            Some((matrix, translation, scale)) => [
                scale * (x * matrix[0] + y * matrix[3] + z * matrix[6]) + translation[0],
                scale * (x * matrix[1] + y * matrix[4] + z * matrix[7]) + translation[1],
                scale * (x * matrix[2] + y * matrix[5] + z * matrix[8]) + translation[2],
            ],
            None => [x, y, z],
        };
        let high = [x as f32, y as f32, z as f32];
        verts_high.push(high);
        verts_low.push([
            (x - high[0] as f64) as f32,
            (y - high[1] as f64) as f32,
            (z - high[2] as f64) as f32,
        ]);
    }
    let normals = match xform {
        Some((matrix, _, _)) => normals
            .iter()
            .map(|normal| {
                let [x, y, z] = [normal[0] as f64, normal[1] as f64, normal[2] as f64];
                let transformed = [
                    x * matrix[0] + y * matrix[3] + z * matrix[6],
                    x * matrix[1] + y * matrix[4] + z * matrix[7],
                    x * matrix[2] + y * matrix[5] + z * matrix[8],
                ];
                let length = (transformed[0] * transformed[0]
                    + transformed[1] * transformed[1]
                    + transformed[2] * transformed[2])
                    .sqrt();
                if length > 1e-9 {
                    [
                        (transformed[0] / length) as f32,
                        (transformed[1] / length) as f32,
                        (transformed[2] / length) as f32,
                    ]
                } else {
                    *normal
                }
            })
            .collect(),
        None => normals,
    };
    MeshModel {
        name,
        verts: verts_high,
        verts_low,
        normals,
        indices,
        triangle_material_handles,
        triangle_colors,
        color,
        selected: false,
    }
}

fn parse_acis(
    sat_fn: impl FnOnce() -> Option<SatDocument>,
    is_binary: bool,
    sab_data: &[u8],
) -> Option<SatDocument> {
    sat_fn().or_else(|| {
        (is_binary && !sab_data.is_empty())
            .then(|| SabReader::read(sab_data).ok())
            .flatten()
    })
}

/// A 3DSOLID's ACIS data as a Solidcore record.
pub fn solid_record(solid: &Solid3D) -> Option<Brep> {
    acis_record(&solid.acis_data)
}

/// A REGION's ACIS data as a Solidcore record.
pub fn region_record(region: &Region) -> Option<Brep> {
    acis_record(&region.acis_data)
}

/// A SURFACE's ACIS data as a Solidcore record.
pub fn surface_record(surface: &Surface) -> Option<Brep> {
    acis_record(&surface.acis_data)
}

/// An object's ACIS data as a document, from its text or its binary form.
pub(crate) fn acis_document(acis: &document::entities::AcisData) -> Option<SatDocument> {
    parse_acis(|| acis.parse(), acis.is_binary, &acis.sab_data)
}

/// ACIS data holding one body, read whole, as a Solidcore record (placed by its transform).
pub(crate) fn acis_record(acis: &document::entities::AcisData) -> Option<Brep> {
    let sat = acis_document(acis)?;
    let (mut bodies, loss) = ocs_solid::lift(&sat);
    if !loss.is_empty() || bodies.len() != 1 {
        return None;
    }
    bodies.pop().map(|body| body.brep)
}

fn remap_acis_material_bindings(set: &mut MeshLodSet, acis: &document::entities::AcisData) {
    for lod in &mut set.lods {
        for handle in lod.triangle_material_handles.iter_mut().flatten() {
            let reference = handle.value() as i32;
            if let Some(material_handle) = acis.materials.iter().find_map(|binding| {
                (binding.absolute_reference == reference || binding.array_index == reference)
                    .then_some(binding.material_handle)
                    .flatten()
            }) {
                *handle = material_handle;
            }
        }
    }
}

fn finish(
    sat: SatDocument,
    name: String,
    color: [f32; 4],
    facet_res: f64,
    chordal_deflection: Option<f64>,
    isolines: [usize; 2],
    planar_isolines: Option<bool>,
    acis: &document::entities::AcisData,
) -> Option<MeshLodSet> {
    let mut set = tessellate_acis(
        &sat,
        name,
        color,
        facet_res,
        chordal_deflection,
        isolines,
        planar_isolines,
    )?;
    remap_acis_material_bindings(&mut set, acis);
    Some(set)
}

pub fn tessellate_region(
    region: &Region,
    color: [f32; 4],
    facet_res: f64,
    chordal_deflection: Option<f64>,
    isolines: usize,
) -> Option<MeshLodSet> {
    let sat = parse_acis(
        || region.parse_sat(),
        region.acis_data.is_binary,
        &region.acis_data.sab_data,
    )?;
    finish(
        sat,
        region.common.handle.value().to_string(),
        color,
        facet_res,
        chordal_deflection,
        [isolines; 2],
        None,
        &region.acis_data,
    )
}

pub fn tessellate_body(
    body: &Body,
    color: [f32; 4],
    facet_res: f64,
    chordal_deflection: Option<f64>,
    isolines: usize,
) -> Option<MeshLodSet> {
    let sat = parse_acis(
        || body.parse_sat(),
        body.acis_data.is_binary,
        &body.acis_data.sab_data,
    )?;
    finish(
        sat,
        body.common.handle.value().to_string(),
        color,
        facet_res,
        chordal_deflection,
        [isolines; 2],
        None,
        &body.acis_data,
    )
}

pub fn tessellate_surface(
    surface: &document::entities::Surface,
    color: [f32; 4],
    facet_res: f64,
    chordal_deflection: Option<f64>,
    isolines: [usize; 2],
    planar_isolines: bool,
) -> Option<MeshLodSet> {
    let sat = parse_acis(
        || surface.parse_sat(),
        surface.acis_data.is_binary,
        &surface.acis_data.sab_data,
    )?;
    finish(
        sat,
        surface.common.handle.value().to_string(),
        color,
        facet_res,
        chordal_deflection,
        isolines,
        Some(planar_isolines),
        &surface.acis_data,
    )
}

pub fn tessellate_solid3d(
    solid: &Solid3D,
    color: [f32; 4],
    facet_res: f64,
    chordal_deflection: Option<f64>,
    isolines: usize,
) -> Option<MeshLodSet> {
    let sat = parse_acis(
        || solid.parse_sat(),
        solid.acis_data.is_binary,
        &solid.acis_data.sab_data,
    )?;
    finish(
        sat,
        solid.common.handle.value().to_string(),
        color,
        facet_res,
        chordal_deflection,
        [isolines; 2],
        None,
        &solid.acis_data,
    )
}
