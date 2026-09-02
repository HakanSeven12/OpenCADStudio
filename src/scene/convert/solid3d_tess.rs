use acadrust::entities::acis::{SabReader, SatBody, SatDocument};
use acadrust::entities::{Body, Region, Solid3D, Surface};
use cadkernel::brep::{self, Body as KernelBody};

use crate::scene::model::mesh_model::{MeshLodSet, MeshModel};

fn tessellate_acis(
    sat: &SatDocument,
    name: String,
    color: [f32; 4],
    facet_res: f64,
    chordal_deflection: Option<f64>,
    isolines: usize,
) -> Option<MeshLodSet> {
    crate::scene::convert::acis_kernel::tessellate_sat(
        sat,
        name,
        color,
        facet_res,
        chordal_deflection,
        isolines,
    )
}

pub(crate) fn body_transform(
    sat: &SatDocument,
    body_record: usize,
) -> Result<Option<([f64; 9], [f64; 3], f64)>, ()> {
    let body = SatBody::from_record(sat.record(body_record).ok_or(())?).ok_or(())?;
    let Some(transform_record) = body.transform().index() else {
        return Ok(None);
    };
    let transform = sat.record(transform_record).ok_or(())?;
    if transform.entity_type != "transform" {
        return Err(());
    }
    let mut values = Vec::with_capacity(13);
    for token in &transform.tokens {
        if values.len() >= 13 {
            break;
        }
        if let Some((components, len)) = token.coordinate_components() {
            values.extend_from_slice(&components[..len]);
        } else if let Some(value) = token.as_float() {
            values.push(value);
        } else if let Some(value) = token.as_integer() {
            values.push(value as f64);
        } else if let Some(text) = token.as_string() {
            for word in text.split_ascii_whitespace() {
                let Ok(value) = word.parse::<f64>() else {
                    break;
                };
                values.push(value);
                if values.len() >= 13 {
                    break;
                }
            }
        }
    }
    (values.len() >= 13
        && values[..13].iter().all(|value| value.is_finite())
        && values[12] > 0.0)
        .then(|| {
            (
                [
                    values[0], values[1], values[2], values[3], values[4], values[5], values[6],
                    values[7], values[8],
                ],
                [values[9], values[10], values[11]],
                values[12],
            )
        })
        .map(Some)
        .ok_or(())
}

pub(crate) fn finalize_mesh(
    name: String,
    verts: Vec<[f64; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
    triangle_material_handles: Vec<Option<acadrust::Handle>>,
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

pub fn kernel_body(solid: &Solid3D) -> Option<KernelBody> {
    kernel_acis_body(&solid.acis_data)
}

pub fn kernel_surface_body(surface: &Surface) -> Option<KernelBody> {
    kernel_acis_body(&surface.acis_data)
}

fn kernel_acis_body(acis: &acadrust::entities::AcisData) -> Option<KernelBody> {
    let sat = parse_acis(
        || acis.parse(),
        acis.is_binary,
        &acis.sab_data,
    )?;
    let (mut bodies, loss) = cadkernel::acis::lift(&sat);
    if !loss.is_empty() || bodies.len() != 1 {
        return None;
    }
    let body = bodies.pop()?;
    let source = body.provenance.source()?;
    let Some((matrix, translation, scale)) = body_transform(&sat, source.index() as usize).ok()?
    else {
        return Some(body);
    };
    brep::transform(
        &body,
        &brep::Placement {
            x_axis: [scale * matrix[0], scale * matrix[1], scale * matrix[2]],
            y_axis: [scale * matrix[3], scale * matrix[4], scale * matrix[5]],
            z_axis: [scale * matrix[6], scale * matrix[7], scale * matrix[8]],
            origin: translation,
        },
    )
}

fn remap_acis_material_bindings(
    set: &mut MeshLodSet,
    acis: &acadrust::entities::AcisData,
) {
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
    isolines: usize,
    acis: &acadrust::entities::AcisData,
) -> Option<MeshLodSet> {
    let mut set = tessellate_acis(
        &sat,
        name,
        color,
        facet_res,
        chordal_deflection,
        isolines,
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
        isolines,
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
        isolines,
        &body.acis_data,
    )
}

pub fn tessellate_surface(
    surface: &acadrust::entities::Surface,
    color: [f32; 4],
    facet_res: f64,
    chordal_deflection: Option<f64>,
    isolines: usize,
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
        isolines,
        &solid.acis_data,
    )
}
