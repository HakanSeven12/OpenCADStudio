// IMAGE / IMAGEATTACH command — place a raster image in the drawing.
//
// Workflow:
//   1. File dialog opens (async, handled in update.rs).
//   2. User picks insertion point (first click).
//   3. User drags to pick width; height is computed from the image's aspect ratio.
//   4. Entity is committed.

use acadrust::entities::RasterImage;
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::scene::model::wire_model::WireModel;

pub struct ImageCommand {
    file_path: String,
    pixel_width: u32,
    pixel_height: u32,
    origin: Option<DVec3>,
    plane: WorkingPlane,
}

impl ImageCommand {
    pub fn new(file_path: String, pixel_width: u32, pixel_height: u32) -> Self {
        Self {
            file_path,
            pixel_width,
            pixel_height,
            origin: None,
            plane: WorkingPlane::default(),
        }
    }

    fn aspect(&self) -> f64 {
        if self.pixel_height == 0 {
            1.0
        } else {
            self.pixel_width as f64 / self.pixel_height as f64
        }
    }

    fn make_entity(&self, origin: DVec3, width_pt: DVec3) -> EntityType {
        let origin = self.plane.to_local(origin);
        let width_pt = self.plane.to_local(width_pt);
        let world_width = (width_pt.x - origin.x).abs().max(0.001);
        let world_height = world_width / self.aspect();

        let ins = Vector3::new(origin.x, origin.y, origin.z);

        let mut img = RasterImage::with_size(
            &self.file_path,
            ins,
            self.pixel_width as f64,
            self.pixel_height as f64,
            world_width,
            world_height,
        );
        img.flags = acadrust::entities::ImageDisplayFlags::SHOW_IMAGE
            | acadrust::entities::ImageDisplayFlags::USE_CLIPPING_BOUNDARY;
        self.plane.place_entity(EntityType::RasterImage(img))
    }
}

impl CadCommand for ImageCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "IMAGE"
    }

    fn prompt(&self) -> String {
        if self.origin.is_none() {
            t!(
                "IMAGE  Specify insertion point (%{name}):  ",
                name = short_name(&self.file_path)
            )
            .into_owned()
        } else {
            t!("IMAGE  Specify width (drag right):").into_owned()
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(origin) = self.origin {
            let entity = self.make_entity(origin, pt);
            CmdResult::CommitAndExit(entity)
        } else {
            self.origin = Some(pt);
            CmdResult::NeedPoint
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        // If origin is set, place with a default width of 1 unit * pixel count / 100
        if let Some(origin) = self.origin {
            let default_w = (self.pixel_width as f64 / 100.0).max(1.0);
            let width_pt = origin + self.plane.x * default_w;
            let entity = self.make_entity(origin, width_pt);
            CmdResult::CommitAndExit(entity)
        } else {
            CmdResult::Cancel
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let origin = self.origin?;
        let origin_local = self.plane.to_local(origin);
        let point_local = self.plane.to_local(pt);
        let width = (point_local.x - origin_local.x).abs().max(0.001);
        let height = width / self.aspect();
        let corners = [
            origin_local,
            origin_local + DVec3::X * width,
            origin_local + DVec3::new(width, height, 0.0),
            origin_local + DVec3::Y * height,
        ]
        .map(|point| self.plane.to_world(point).as_vec3().to_array());
        let [p0, p1, p2, p3] = corners;

        Some(WireModel {
            point_marker: None,
            taper_widths: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: "image_preview".into(),
            points: vec![p0, p1, p2, p3, p0],
            points_low: Vec::new(),
            color: WireModel::CYAN,
            selected: false,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: vec![],
            tangent_geoms: vec![],
            aci: 0,
            key_vertices: vec![],
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris: vec![],
            fill_tris_low: Vec::new(),
        })
    }
}

fn short_name(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
}
