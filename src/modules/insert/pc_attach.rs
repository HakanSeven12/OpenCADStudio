// -POINTCLOUDATTACH — attach a point cloud: a scan (.rcs) or a project of
// scans (.rcp).
//
//   Path to point cloud file to attach:
//   Specify insertion point <0,0>:      Current insertion point: X = 5.5, Y = 2, Z = 3
//   Specify scale factor <1>:           Current scale factor: 2.500000
//   Specify rotation angle <0>:         Current rotate angle: 30
//   1 point cloud attached
//
// The definition (one per file, in ACAD_POINTCLOUD_EX_DICT) is created with
// the cloud when it is placed, so a cancelled attach leaves nothing and undo
// removes both.

use std::path::PathBuf;

use glam::DVec3;

use crate::command::{CadCommand, CmdResult, InputKind, WorkingPlane};
use crate::modules::IconKind;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/pc_attach.svg"));

/// What the Attach Point Cloud dialog settles before the prompts: the
/// path to store, the switches, and the scale and rotation not left to
/// the command line.
#[derive(Clone, Debug)]
pub struct AttachOptions {
    /// The path stored as given (no path: the file name); None stores the
    /// full path.
    pub stored: Option<String>,
    /// Stored relative to the drawing (once it has a file).
    pub relative: bool,
    /// Locked on attach; None follows POINTCLOUDLOCK.
    pub locked: Option<bool>,
    /// Zoom to the cloud once attached.
    pub zoom: bool,
    pub scale: Option<f64>,
    pub rotation: Option<f64>,
}

impl Default for AttachOptions {
    fn default() -> Self {
        // The command line attaches with a relative path, as the dialog
        // does by default.
        Self { stored: None, relative: true, locked: None, zoom: false, scale: None, rotation: None }
    }
}

/// A point cloud to attach: the file, where and how it is placed.
#[derive(Clone, Debug)]
pub struct PointCloudPlacement {
    pub path: PathBuf,
    pub insertion: DVec3,
    /// The placement's axes in the drawing, each as long as the scale.
    pub axes: [DVec3; 3],
    pub options: AttachOptions,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Path,
    Insertion,
    Scale,
    Rotation,
}

pub struct PointCloudAttachCommand {
    step: Step,
    path: PathBuf,
    insertion: DVec3,
    scale: f64,
    plane: WorkingPlane,
    options: AttachOptions,
}

/// A number as the reference echoes a coordinate or an angle: no trailing
/// zeros.
fn plain(value: f64) -> String {
    crate::entities::underlay::plain_number(value)
}

impl PointCloudAttachCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Path,
            path: PathBuf::new(),
            insertion: DVec3::ZERO,
            scale: 1.0,
            plane: WorkingPlane::default(),
            options: AttachOptions::default(),
        }
    }

    /// From the Attach Point Cloud dialog: the file chosen, what it set.
    pub fn from_dialog(path: PathBuf, options: AttachOptions) -> Self {
        let mut command = Self::with_file(path);
        command.options = options;
        command
    }

    /// Starts at the insertion point, the file already chosen.
    pub fn with_file(path: PathBuf) -> Self {
        let mut command = Self::new();
        command.path = path;
        command.step = Step::Insertion;
        command
    }

    fn accept_path(&mut self, text: &str) -> CmdResult {
        let text = text.trim().trim_matches('"');
        if text.is_empty() {
            return CmdResult::ReportError("Filename cannot be blank".to_string());
        }
        let path = PathBuf::from(text);
        // A file that is not there is asked for again.
        if !path.is_file() {
            return CmdResult::NeedPoint;
        }
        if crate::scene::model::point_cloud::load(&path).is_none() {
            // Said, and the command ends.
            return CmdResult::Measurement(
                "All scans are not found or invalid.\nAttach point cloud failed".to_string(),
            );
        }
        self.path = path;
        self.step = Step::Insertion;
        CmdResult::NeedPoint
    }

    fn accept_insertion(&mut self, point: DVec3) -> CmdResult {
        self.insertion = point;
        self.step = Step::Scale;
        let local = self.plane.to_local(point);
        let message = format!(
            "Current insertion point: X = {}, Y = {}, Z = {}",
            plain(local.x),
            plain(local.y),
            plain(local.z)
        );
        // Scale and rotation the dialog set are not asked for.
        let Some(scale) = self.options.scale else {
            return CmdResult::ReportMeasurement(message);
        };
        self.scale = scale;
        self.step = Step::Rotation;
        match self.options.rotation {
            Some(degrees) => self.placed(degrees, message),
            None => CmdResult::ReportMeasurement(message),
        }
    }

    fn accept_scale(&mut self, text: &str) -> CmdResult {
        let scale = if text.trim().is_empty() {
            1.0
        } else {
            match crate::entities::common::parse_f64(text) {
                Some(scale) => scale,
                None => {
                    return CmdResult::ReportError(
                        "Requires numeric distance or second point.".to_string(),
                    )
                }
            }
        };
        if scale <= 0.0 {
            return CmdResult::ReportError("Value must be positive and nonzero.".to_string());
        }
        self.scale = scale;
        self.step = Step::Rotation;
        let message = format!("Current scale factor: {scale:.6}");
        match self.options.rotation {
            Some(degrees) => self.placed(degrees, message),
            None => CmdResult::ReportMeasurement(message),
        }
    }

    fn accept_rotation(&mut self, degrees: f64) -> CmdResult {
        self.placed(degrees, format!("Current rotate angle: {}", plain(degrees)))
    }

    fn placed(&mut self, degrees: f64, message: String) -> CmdResult {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let x = (self.plane.x * cos + self.plane.y * sin) * self.scale;
        let y = (self.plane.y * cos - self.plane.x * sin) * self.scale;
        CmdResult::AttachPointCloud {
            placement: PointCloudPlacement {
                path: self.path.clone(),
                insertion: self.insertion,
                axes: [x, y, self.plane.z * self.scale],
                options: self.options.clone(),
            },
            message,
        }
    }
}

impl CadCommand for PointCloudAttachCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "POINTCLOUDATTACH"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Path => "Path to point cloud file to attach:",
            Step::Insertion => "Specify insertion point <0,0>:",
            Step::Scale => "Specify scale factor <1>:",
            Step::Rotation => "Specify rotation angle <0>:",
        }
        .to_string()
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Path => InputKind::FreeText,
            Step::Insertion => InputKind::Point,
            _ => InputKind::SingleToken,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::Insertion => self.accept_insertion(pt),
            // A second point gives the angle from the insertion point.
            Step::Rotation => {
                let d = self.plane.vector_to_local(pt - self.insertion);
                self.accept_rotation(d.y.atan2(d.x).to_degrees())
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match self.step {
            Step::Path => self.accept_path(text),
            Step::Insertion => CmdResult::ReportError("Invalid point.".to_string()),
            Step::Scale => self.accept_scale(text),
            Step::Rotation => match crate::entities::common::parse_f64(text) {
                Some(degrees) => self.accept_rotation(degrees),
                None => CmdResult::ReportError(
                    "Requires valid numeric angle or second point.".to_string(),
                ),
            },
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Path => self.accept_path(""),
            Step::Insertion => self.accept_insertion(self.plane.origin),
            Step::Scale => self.accept_scale(""),
            Step::Rotation => self.accept_rotation(0.0),
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["POINTCLOUDATTACH", "-POINTCLOUDATTACH"]
});
