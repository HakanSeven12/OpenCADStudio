//! Lightweight, fixed-resolution preview of a huge *2-D* wire selection.
//!
//! Built once when a transform command starts, never on mouse movement. This
//! is a disposable visual snapshot, not an image entity in the DWG. The
//! drawing and all commits retain exact vector entities and snap geometry.

use std::sync::Arc;
use super::wire_model::WireModel;
use super::image_model::{ImageModel, ImageQuadVertex};

#[derive(Clone)]
pub struct RasterSelectionPreview {
    pixels: Arc<Vec<u8>>,
    width: u32,
    height: u32,
}

fn bounds_valid(b: [f64; 4]) -> bool {
    b.iter().all(|v| v.is_finite() && v.abs() < 1.0e13)
        && b[2] > b[0] && b[3] > b[1]
}

/// Draw only selected wire geometry into a transparent CPU RGBA snapshot.
/// The image is capped at 1536 pixels on its long side (~9 MiB), avoiding
/// CPU↔GPU readback or a full-scene screen grab (which would capture neighbors).
pub fn rasterize_selection(wires: &[WireModel], selected: usize) -> Option<RasterSelectionPreview> {
    if selected <= 512 || wires.len() < 2048 {
        return None;
    }
    let outlines = super::instance_model::mass_transform_group_preview(wires, 2048)?;
    let extent = outlines.iter().find(|w| w.name == "__mass_transform_group_extent__")?;
    let b = extent.aabb.map(f64::from);
    if !bounds_valid(b) {
        return None;
    }
    let (width_world, height_world) = (b[2] - b[0], b[3] - b[1]);
    const MAX_SIDE: f64 = 1536.0;
    let scale = MAX_SIDE / width_world.max(height_world);
    let width = (width_world * scale).ceil().max(16.0).min(MAX_SIDE) as u32;
    let height = (height_world * scale).ceil().max(16.0).min(MAX_SIDE) as u32;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let start = crate::perf::enabled().then(std::time::Instant::now());
    let mut segments = 0usize;

    // Full bitmap, not a recolored selection bounding box. The nested wire
    // instance structure is traversed without materializing millions of
    // independent WireModel copies.
    fn raster_wire(
        w: &WireModel,
        offset: [f64; 2],
        b: [f64; 4],
        size: (u32, u32),
        dst: &mut [u8],
        segments: &mut usize,
        depth: usize,
    ) {
        if !w.display_visible || w.snap_only || depth > 12 {
            return;
        }
        if let Some(stub) = &w.stub {
            let delta = [offset[0] + stub.delta[0], offset[1] + stub.delta[1]];
            for leaf in stub.prototype.iter() {
                raster_wire(leaf, delta, b, size, dst, segments, depth + 1);
            }
            return;
        }
        let (ww, hh) = (size.0 as f64, size.1 as f64);
        let point_xy = |i: usize| {
            let low = w.points_low.get(i).copied().unwrap_or([0.0; 3]);
            let p = w.points[i];
            (
                (p[0] as f64 + low[0] as f64 + offset[0] - b[0])
                    * ((ww-1.0)/(b[2]-b[0])),
                (b[3] - (p[1] as f64 + low[1] as f64 + offset[1]))
                    * ((hh-1.0)/(b[3]-b[1])),
            )
        };
        let a = [
            (w.color[0].clamp(0.0,1.0)*255.0) as u8,
            (w.color[1].clamp(0.0,1.0)*255.0) as u8,
            (w.color[2].clamp(0.0,1.0)*255.0) as u8,
            255u8,
        ];
        for i in 1..w.points.len() {
            let (x0,y0) = point_xy(i-1);
            let (x1,y1) = point_xy(i);
            if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
                continue;
            }
            draw_line(dst, size.0, size.1, (x0,y0), (x1,y1), a);
            *segments += 1;
        }
    }

    for wire in wires {
        raster_wire(wire, [0.0,0.0], b, (width,height), &mut pixels, &mut segments, 0);
    }
    if segments == 0 {
        return None;
    }
    if let Some(start) = start {
        crate::perf_record!(
            "[perf] raster-selection-capture {:.1}ms selected={} source_wires={} segments={} texture={}x{}",
            start.elapsed().as_secs_f64()*1000.0, selected, wires.len(),
            segments, width, height,
        );
    }
    Some(RasterSelectionPreview { pixels: Arc::new(pixels), width, height })
}

/// Clips before iterating pixels. The per-wire cost is bounded by the
/// selected preview texture's size, even when a line extends far offscreen.
fn draw_line(dst: &mut [u8], width: u32, height: u32,
             first: (f64,f64), last: (f64,f64), color: [u8;4]) {
    let (mut t0,mut t1)=(0.0f64,1.0f64);
    let (dx,dy)=(last.0-first.0,last.1-first.1);
    for (p,q) in [
        (-dx,first.0),(dx,width as f64-1.0-first.0),
        (-dy,first.1),(dy,height as f64-1.0-first.1)
    ] {
        if p.abs()<1e-12 {
            if q<0.0 { return; }
        } else {
            let r=q/p;
            if p<0.0 { t0=t0.max(r) } else { t1=t1.min(r) }
            if t0>t1 { return; }
        }
    }
    let x0=(first.0+dx*t0).round() as i32;
    let y0=(first.1+dy*t0).round() as i32;
    let x1=(first.0+dx*t1).round() as i32;
    let y1=(first.1+dy*t1).round() as i32;
    let (mut x,mut y)=(x0,y0);
    let dx=(x1-x0).abs();
    let sx=if x0<x1 {1} else {-1};
    let dy=-(y1-y0).abs();
    let sy=if y0<y1 {1} else {-1};
    let mut err=dx+dy;
    loop {
        if x>=0 && y>=0 && (x as u32)<width && (y as u32)<height {
            let at=((y as usize*width as usize+x as usize)*4) as usize;
            dst[at..at+4].copy_from_slice(&color);
        }
        if x==x1 && y==y1 { break; }
        let e=err*2;
        if e>=dy { err+=dy; x+=sx; }
        if e<=dx { err+=dx; y+=sy; }
    }
}

impl RasterSelectionPreview {
    /// The transformed 5-vertex outline supplies all four corners, so the
    /// exact same snapshot can move, rotate or scale without re-rasterizing.
    pub fn image_at(&self, outline: &WireModel) -> Option<ImageModel> {
        if outline.points.len()<4 {
            return None;
        }
        let point = |i:usize| {
            let p=outline.points[i];
            let low=outline.points_low.get(i).copied().unwrap_or([0.0;3]);
            let xyz=[0,1,2].map(|a| p[a] as f64+low[a] as f64);
            let high=xyz.map(|v|v as f32);
            let residual=[0,1,2].map(|a|(xyz[a]-high[a] as f64) as f32);
            (high,residual)
        };
        let pts=[point(0),point(1),point(2),point(3)];
        let corners=pts.map(|p|p.0);
        let corners_low=pts.map(|p|p.1);
        let verts=[(0,[0.0,1.0]),(1,[1.0,1.0]),(2,[1.0,0.0]),
                   (0,[0.0,1.0]),(2,[1.0,0.0]),(3,[0.0,0.0])]
            .map(|(i,uv)|ImageQuadVertex {
                pos:corners[i],pos_low:corners_low[i],uv
            }).to_vec();
        Some(ImageModel {
            render_instance: None,
            file_path: "__temporary_command_raster__".into(),
            pixels: Arc::clone(&self.pixels),
            width: self.width,
            height: self.height,
            opacity: 0.40,
            corners,
            corners_low,
            draw_depth: 0.0,
            verts,
            pixelated: false,
            use_alpha: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raster_is_transparent_off_geometry_and_builds_one_rgba_image() {
        let mut wires=Vec::new();
        for i in 0..600 {
            let x=i as f64;
            let mut wire=WireModel::solid_f64(
                i.to_string(),
                vec![[x,0.0,0.0],[x+0.5,1.0,0.0]],
                WireModel::WHITE,false,
            );
            wire.aabb=[x as f32,0.0,x as f32+0.5,1.0];
            for _ in 0..4 { wires.push(wire.clone()); }
        }
        let source=rasterize_selection(&wires,600).unwrap();
        assert!(source.pixels.iter().any(|&value|value!=0));
        assert_eq!(source.pixels.len(),source.width as usize*source.height as usize*4);
        let outline=super::super::instance_model::mass_transform_group_preview(&wires,2048).unwrap();
        let image=source.image_at(outline.iter().find(|w|w.name=="__mass_transform_group_extent__").unwrap()).unwrap();
        assert_eq!(image.opacity,0.4);
        assert_eq!(image.verts.len(),6);
    }
}
