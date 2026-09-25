// PDFCLIP / IMAGECLIP — clip a PDF underlay or a raster image to a boundary
// (CLIP continues here for either).
//
//   Select PDF to clip:                      (Select image to clip:)
//   Enter PDF clipping option [ON/OFF/Delete/New boundary] <New boundary>:
//   (Enter image clipping option [ON/OFF/Delete/New boundary] <New>:)
//   Delete old boundary? [Yes/No] <Yes>:     (when one exists; [No/Yes] for
//                                             an image)
//   Outside mode - Objects outside boundary will be hidden.
//   Specify clipping boundary or select invert option:
//   [Select polyline/Polygonal/Rectangular/Invert clip] <Rectangular>:
//
// The boundary is stored in underlay units (image: raster pixels, a
// rectangle as its two corners); the clip-inside bit follows the Invert
// choice.

use codec::entities::{
    ClipBoundary, ClipMode, ImageDisplayFlags, RasterImage, Underlay, UnderlayDisplayFlags,
    UnderlayType,
};
use codec::types::{Handle, Vector2};
use codec::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::wire_model::WireModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Select,
    Option,
    DeleteOld,
    Mode,
    RectFirst,
    RectSecond,
    PolyPoints,
    Polyline,
}

pub struct PdfClipCommand {
    step: Step,
    handle: Handle,
    underlay: Option<Underlay>,
    /// IMAGECLIP: the image being clipped (`underlay` stays empty).
    image: Option<RasterImage>,
    image_mode: bool,
    picked: Option<EntityType>,
    inverted: bool,
    points: Vec<DVec3>,
}

const OUTSIDE: &str = "Outside mode - Objects outside boundary will be hidden.";
const INSIDE: &str = "Inside mode - Objects inside boundary will be hidden.";
const BOUNDARY: &str = "Specify clipping boundary or select invert option:";

impl PdfClipCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Select,
            handle: Handle::NULL,
            underlay: None,
            image: None,
            image_mode: false,
            picked: None,
            inverted: false,
            points: Vec::new(),
        }
    }

    /// IMAGECLIP: asks for an image.
    pub fn image() -> Self {
        Self { image_mode: true, ..Self::new() }
    }

    /// An image CLIP picked: straight to the clipping option.
    pub fn for_image(handle: Handle, image: RasterImage) -> Self {
        let mut command = Self::image();
        command.handle = handle;
        command.image = Some(image);
        command.step = Step::Option;
        command
    }

    fn finish_image(&self, image: RasterImage) -> CmdResult {
        CmdResult::UpdateEntityAndFinish {
            handle: self.handle,
            entity: EntityType::RasterImage(image),
        }
    }

    /// Image options: ON / OFF switch only the display flag (the boundary
    /// stays), Delete drops the boundary back to the whole image, New asks
    /// to replace a boundary that exists.
    fn image_option(&mut self, token: &str) -> CmdResult {
        let Some(mut image) = self.image.clone() else {
            return CmdResult::Cancel;
        };
        match token {
            "ON" => image.flags |= ImageDisplayFlags::USE_CLIPPING_BOUNDARY,
            "OFF" => image.flags -= ImageDisplayFlags::USE_CLIPPING_BOUNDARY,
            "D" | "DELETE" => {
                let mode = image.clip_boundary.clip_mode;
                image.clip_boundary = ClipBoundary::full_image(image.size.x, image.size.y);
                image.clip_boundary.clip_mode = mode;
                image.clipping_enabled = false;
            }
            "" | "N" | "NEW" | "NEW BOUNDARY" if image.clipping_enabled => {
                self.step = Step::DeleteOld;
                return CmdResult::NeedPoint;
            }
            "" | "N" | "NEW" | "NEW BOUNDARY" => return self.enter_mode(),
            _ => return CmdResult::ReportError("Invalid option keyword.".to_string()),
        }
        self.finish_image(image)
    }

    /// A rectangle keeps its two corners (lowest and highest raster
    /// coordinates); a polygon is closed by repeating its first vertex.
    fn apply_image_boundary(&self, world: &[DVec3], rectangle: bool) -> CmdResult {
        let Some(mut image) = self.image.clone() else {
            return CmdResult::Cancel;
        };
        let raster: Vec<Vector2> = world
            .iter()
            .map(|p| crate::entities::raster_image::image_world_to_clip(&image, p.to_array()))
            .collect();
        let mut boundary = if rectangle {
            let (mut lo, mut hi) = (raster[0], raster[0]);
            for p in &raster {
                lo = Vector2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Vector2::new(hi.x.max(p.x), hi.y.max(p.y));
            }
            ClipBoundary::rectangular(lo, hi)
        } else {
            let mut ring = raster;
            ring.push(ring[0]);
            ClipBoundary::polygonal(ring)
        };
        boundary.clip_mode = if self.inverted { ClipMode::Inside } else { ClipMode::Outside };
        image.clip_boundary = boundary;
        image.clipping_enabled = true;
        image.flags |= ImageDisplayFlags::USE_CLIPPING_BOUNDARY;
        self.finish_image(image)
    }

    /// A new boundary for an underlay already chosen (the underlay tab's
    /// Create Clipping Boundary): the New boundary branch at once. Returns the
    /// command and the result of its first step.
    pub fn new_boundary(handle: Handle, underlay: Underlay) -> (Self, CmdResult) {
        let mut command = Self::new();
        command.handle = handle;
        command.underlay = Some(underlay);
        command.step = Step::Option;
        let first = command.option("N");
        (command, first)
    }

    /// An underlay CLIP picked: straight to the clipping option.
    pub fn for_underlay(handle: Handle, underlay: Underlay) -> Self {
        let mut command = Self::new();
        command.handle = handle;
        command.underlay = Some(underlay);
        command.step = Step::Option;
        command
    }

    fn finish(&self, underlay: Underlay) -> CmdResult {
        CmdResult::UpdateEntityAndFinish {
            handle: self.handle,
            entity: EntityType::Underlay(underlay),
        }
    }

    fn enter_mode(&mut self) -> CmdResult {
        self.step = Step::Mode;
        let mode = if self.inverted { INSIDE } else { OUTSIDE };
        CmdResult::ReportMeasurement(format!("{mode}\n{BOUNDARY}"))
    }

    fn to_local(u: &Underlay, p: DVec3) -> Vector2 {
        let (c, s) = (u.rotation.cos(), u.rotation.sin());
        let dx = p.x - u.insertion_point.x;
        let dy = p.y - u.insertion_point.y;
        let sx = if u.x_scale.abs() > 1e-12 { u.x_scale } else { 1.0 };
        let sy = if u.y_scale.abs() > 1e-12 { u.y_scale } else { 1.0 };
        Vector2::new((dx * c + dy * s) / sx, (-dx * s + dy * c) / sy)
    }

    fn apply_boundary(&self, world: &[DVec3], rectangle: bool) -> CmdResult {
        if self.image.is_some() {
            return self.apply_image_boundary(world, rectangle);
        }
        let Some(mut underlay) = self.underlay.clone() else {
            return CmdResult::Cancel;
        };
        underlay.clip_boundary_vertices = world.iter().map(|p| Self::to_local(&underlay, *p)).collect();
        underlay.flags |= UnderlayDisplayFlags::CLIPPING;
        underlay.clip_inverted = self.inverted;
        self.finish(underlay)
    }

    fn option(&mut self, text: &str) -> CmdResult {
        if self.image.is_some() {
            return self.image_option(&text.trim().to_ascii_uppercase());
        }
        let Some(mut underlay) = self.underlay.clone() else {
            return CmdResult::Cancel;
        };
        match text.trim().to_ascii_uppercase().as_str() {
            "ON" => {
                underlay.flags |= UnderlayDisplayFlags::CLIPPING;
                self.finish(underlay)
            }
            "OFF" => {
                underlay.flags -= UnderlayDisplayFlags::CLIPPING;
                self.finish(underlay)
            }
            "D" | "DELETE" => {
                underlay.clip_boundary_vertices.clear();
                underlay.flags -= UnderlayDisplayFlags::CLIPPING;
                self.finish(underlay)
            }
            "" | "N" | "NEW" | "NEW BOUNDARY" => {
                if underlay.clip_boundary_vertices.is_empty() {
                    self.enter_mode()
                } else {
                    self.step = Step::DeleteOld;
                    CmdResult::NeedPoint
                }
            }
            _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
        }
    }

    fn mode(&mut self, text: &str) -> CmdResult {
        match text.trim().to_ascii_uppercase().as_str() {
            "" | "R" | "RECTANGULAR" => {
                self.step = Step::RectFirst;
                CmdResult::NeedPoint
            }
            "P" | "POLYGONAL" => {
                self.points.clear();
                self.step = Step::PolyPoints;
                CmdResult::NeedPoint
            }
            "S" | "SELECT" | "SELECT POLYLINE" => {
                self.step = Step::Polyline;
                CmdResult::NeedPoint
            }
            "I" | "INVERT" | "INVERT CLIP" => {
                self.inverted = !self.inverted;
                self.enter_mode()
            }
            _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
        }
    }

    fn rectangle(a: DVec3, b: DVec3) -> Vec<DVec3> {
        vec![
            a,
            DVec3::new(b.x, a.y, a.z),
            b,
            DVec3::new(a.x, b.y, a.z),
        ]
    }
}

impl CadCommand for PdfClipCommand {
    fn name(&self) -> &'static str {
        if self.image_mode { "IMAGECLIP" } else { "PDFCLIP" }
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Select if self.image_mode => "Select image to clip:".to_string(),
            Step::Select => "Select PDF to clip:".to_string(),
            Step::Option if self.image_mode => {
                "Enter image clipping option [ON/OFF/Delete/New boundary] <New>:".to_string()
            }
            Step::Option => {
                "Enter PDF clipping option [ON/OFF/Delete/New boundary] <New boundary>:".to_string()
            }
            Step::DeleteOld if self.image_mode => "Delete old boundary? [No/Yes] <Yes>:".to_string(),
            Step::DeleteOld => "Delete old boundary? [Yes/No] <Yes>:".to_string(),
            Step::Mode => {
                "[Select polyline/Polygonal/Rectangular/Invert clip] <Rectangular>:".to_string()
            }
            Step::RectFirst => "Specify first corner point:".to_string(),
            Step::RectSecond => "Specify opposite corner point:".to_string(),
            Step::PolyPoints => match self.points.len() {
                0 => "Specify first point:".to_string(),
                1 | 2 => "Specify next point or [Undo]:".to_string(),
                _ => "Specify next point or [Close/Undo]:".to_string(),
            },
            Step::Polyline => "Select polyline:".to_string(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Option => vec![
                CmdOption::new("ON", "ON"),
                CmdOption::new("OFF", "OFF"),
                CmdOption::new("Delete", "D"),
                CmdOption::new("New boundary", "N"),
            ],
            Step::DeleteOld if self.image_mode => {
                vec![CmdOption::new("No", "N"), CmdOption::new("Yes", "Y")]
            }
            Step::DeleteOld => vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            Step::Mode => vec![
                CmdOption::new("Select polyline", "S"),
                CmdOption::new("Polygonal", "P"),
                CmdOption::new("Rectangular", "R"),
                CmdOption::new("Invert clip", "I"),
            ],
            Step::PolyPoints if self.points.len() >= 3 => {
                vec![CmdOption::new("Close", "C"), CmdOption::new("Undo", "U")]
            }
            Step::PolyPoints if !self.points.is_empty() => vec![CmdOption::new("Undo", "U")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Option | Step::DeleteOld | Step::Mode => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.step == Step::PolyPoints
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Select | Step::Polyline)
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        let picked = self.picked.take();
        match self.step {
            Step::Select if self.image_mode => match picked {
                Some(EntityType::RasterImage(image)) => {
                    self.handle = handle;
                    self.image = Some(image);
                    self.step = Step::Option;
                    CmdResult::NeedPoint
                }
                // Anything else: the prompt again, as the reference does.
                _ => CmdResult::NeedPoint,
            },
            Step::Select => match picked {
                Some(EntityType::Underlay(u)) if u.underlay_type == UnderlayType::Pdf => {
                    self.handle = handle;
                    self.underlay = Some(u);
                    self.step = Step::Option;
                    CmdResult::NeedPoint
                }
                _ => CmdResult::ReportError("Object selected was not a PDF underlay.".to_string()),
            },
            Step::Polyline => match picked {
                Some(EntityType::LwPolyline(pl)) if pl.vertices.len() >= 3 => {
                    let z = pl.elevation;
                    let world: Vec<DVec3> = pl
                        .vertices
                        .iter()
                        .map(|v| DVec3::new(v.location.x, v.location.y, z))
                        .collect();
                    self.apply_boundary(&world, false)
                }
                _ => CmdResult::ReportError("Invalid object selected.".to_string()),
            },
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::RectFirst => {
                self.points = vec![pt];
                self.step = Step::RectSecond;
                CmdResult::NeedPoint
            }
            Step::RectSecond => {
                let first = self.points[0];
                self.apply_boundary(&Self::rectangle(first, pt), true)
            }
            Step::PolyPoints => {
                self.points.push(pt);
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match self.step {
            Step::Option => self.option(text),
            Step::DeleteOld => match text.trim().to_ascii_uppercase().as_str() {
                "" | "Y" | "YES" => self.enter_mode(),
                "N" | "NO" => match (self.image.clone(), self.underlay.clone()) {
                    (Some(image), _) => self.finish_image(image),
                    (None, Some(underlay)) => self.finish(underlay),
                    _ => CmdResult::Cancel,
                },
                _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            Step::Mode => self.mode(text),
            Step::PolyPoints => match text.trim().to_ascii_uppercase().as_str() {
                "U" | "UNDO" => {
                    self.points.pop();
                    CmdResult::NeedPoint
                }
                "C" | "CLOSE" if self.points.len() >= 3 => {
                    let points = self.points.clone();
                    self.apply_boundary(&points, false)
                }
                _ => return None,
            },
            _ => return None,
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Option => self.option(""),
            Step::DeleteOld => self.enter_mode(),
            Step::Mode => self.mode(""),
            Step::PolyPoints if self.points.len() >= 3 => {
                let points = self.points.clone();
                self.apply_boundary(&points, false)
            }
            Step::PolyPoints => CmdResult::NeedPoint,
            _ => CmdResult::Cancel,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let ring: Vec<DVec3> = match self.step {
            Step::RectSecond => Self::rectangle(self.points[0], pt),
            Step::PolyPoints if !self.points.is_empty() => {
                let mut ring = self.points.clone();
                ring.push(pt);
                ring
            }
            _ => return None,
        };
        let mut points: Vec<[f64; 3]> = ring.iter().map(|p| p.to_array()).collect();
        points.push(points[0]);
        Some(WireModel::solid_f64("pdf_clip_boundary".into(), points, WireModel::CYAN, false))
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["PDFCLIP", "IMAGECLIP"]
});
