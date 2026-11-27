//! What a command asks the host for: the requests `CmdResult` carries and
//! the options it hands over. Their behaviour stays with the commands that
//! build them.

use std::collections::HashMap;
use std::path::PathBuf;

use document::Handle;
use glam::DVec3;
use ocs_geom::space::endpoint_join::JoinType;

use crate::io::xref_model::Pathtype;
use crate::scene::block_distance_parameter::{BaseLocation, ValueSet};

#[derive(Clone, Copy, Debug)]
pub enum ActionBarMode {
    Show,
    Hide,
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructionOperation {
    Convert,
    Revert,
    Show,
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SingleParameterKind {
    Basepoint,
    Lookup,
}

/// Values answered at the placement prompt before the parameter exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SingleParameterOptions {
    pub name: Option<String>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub palette: Option<bool>,
}

/// Values answered at the base-point prompt before the parameter exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrientationParameterOptions {
    pub name: Option<String>,
    pub label: Option<String>,
    pub chain: Option<bool>,
    pub description: Option<String>,
    pub palette: Option<bool>,
    pub value_set: Option<ValueSet>,
}

/// Values answered at the base-point prompt before the parameter exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DistanceParameterOptions {
    pub name: Option<String>,
    pub chain: Option<bool>,
    pub description: Option<String>,
    pub palette: Option<bool>,
    pub distance_label: Option<String>,
    pub angle_label: Option<String>,
    pub horizontal_label: Option<String>,
    pub vertical_label: Option<String>,
    pub base_location: Option<BaseLocation>,
    pub value_set: Option<ValueSet>,
    pub angle_value_set: Option<ValueSet>,
    pub vertical_value_set: Option<ValueSet>,
}

/// Values answered at the placement prompt before the parameter exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointParameterOptions {
    pub name: Option<String>,
    pub label: Option<String>,
    pub chain: Option<bool>,
    pub description: Option<String>,
    pub palette: Option<bool>,
}

/// How MLEADERALIGN spaces the multileaders it lines up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MLeaderAlignMode {
    /// Keep each content where it falls along the line (its projection).
    UseCurrent,
    /// Spread the contents evenly between two points.
    Distribute,
    /// Turn each leader's last segment parallel to the one aligned to.
    Parallel,
    /// Stack the contents along the line this far apart.
    Spacing,
}

/// How MLEADERCOLLECT lays the collected blocks out.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MLeaderCollectLayout {
    Horizontal,
    Vertical,
    /// Rows no wider than the width (0: one row).
    Wrap(f64),
    /// Rows of this many blocks.
    WrapNumber(u32),
}

/// What XCLIP does to the chosen block references.
#[derive(Clone, Debug, PartialEq)]
pub enum XclipAction {
    Enable(bool),
    Delete,
    Polyline,
    /// Boundary in WCS (two points = rectangle corners).
    New {
        boundary: Vec<[f64; 2]>,
        inverted: bool,
    },
    /// Front and back clipping planes: `Some(None)` removes one, `None`
    /// leaves it. `rejected` prints that the front plane was not applied.
    Depth {
        front: Option<Option<f64>>,
        back: Option<Option<f64>>,
        rejected: bool,
    },
}

/// A page of a file imported without an underlay (the File option): placed
/// at `insertion` with a scale and rotation.
#[derive(Clone, Debug)]
pub struct PdfFileImport {
    pub path: String,
    pub page: String,
    pub scale: f64,
    pub rotation: f64,
    pub insertion: DVec3,
}

#[derive(Clone, Debug)]
pub struct PdfImportRequest {
    pub underlay: Handle,
    pub area: ImportArea,
    pub mode: UnderlayMode,
}

/// What the read of a file named at the -PDFIMPORT file name prompt found.
#[derive(Debug, Clone)]
pub enum PdfPromptRead {
    /// The file cannot be read: the error to report.
    Error(String),
    /// The file needs a password first.
    Password {
        stage: crate::scene::model::pdf_password::Stage,
        remembered: bool,
    },
    /// The number of pages, and the notice when a password supplied
    /// earlier opened the file.
    Pages {
        pages: usize,
        notice: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportArea {
    All,
    Polygon(Vec<DVec3>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnderlayMode {
    Keep,
    Detach,
    Unload,
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

#[derive(Clone)]
pub enum PeditOp {
    Multiple(Vec<Handle>, Box<PeditOp>),
    JoinSelection(Vec<Handle>, f64, JoinType),
    SetClosed(bool),
    SetWidth(f64),
    SetVertexWidth {
        index: usize,
        start: f64,
        end: f64,
    },
    VertexRange {
        first: usize,
        last: usize,
        split: bool,
    },
    SetLinetypeGeneration(bool),
    EditVertex {
        index: usize,
        point: DVec3,
        insert: bool,
    },
    Reverse,
    /// Replace the picked Line/Arc with an equivalent LwPolyline (#263).
    ConvertToPolyline,
    Fit,
    FitWithTangents(Vec<(usize, f64)>),
    Spline,
    Decurve,
    SetMeshClosedM(bool),
    SetMeshClosedN(bool),
    SetMeshSmooth {
        smooth: document::entities::polygon_mesh::SurfaceSmoothType,
        m_density: i16,
        n_density: i16,
    },
    MoveMeshVertex {
        index: usize,
        point: DVec3,
    },
}

#[derive(Clone)]
pub enum LenMode {
    Delta(f64),
    Total(f64),
    Percent(f64),
    DeltaAngle(f64),
    TotalAngle(f64),
    Dynamic(DVec3),
}

/// Everything a surface command gathered.
#[derive(Clone, Debug, Default)]
pub struct SurfaceRequest {
    pub command: &'static str,
    pub objects: [Vec<Handle>; 3],
    pub picks: [Vec<(Handle, DVec3)>; 3],
    pub points: Vec<DVec3>,
    pub values: HashMap<&'static str, f64>,
    pub texts: HashMap<&'static str, String>,
}

/// What one import reads and where it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct DgnImportRequest {
    pub path: String,
    pub model: String,
    /// Sub units (else master units) become drawing units.
    pub sub_units: bool,
    /// Text nodes become one TEXT per line (else one MTEXT).
    pub explode_text_nodes: bool,
    /// Into the current drawing (else a new one).
    pub current_drawing: bool,
    /// In the current drawing: every imported layer, block and text style
    /// name gets "<file> - <model>$0$" as prefix (else names the drawing
    /// already has keep the existing definitions).
    pub prefix_duplicates: bool,
    /// What becomes of the model's reference attachments.
    pub references: ReferenceMode,
    /// What an earlier translation of a reference already on disk gets.
    pub existing_reference: ExistingReference,
    /// The text code page of a V7 file (e.g. 1250 for Czech/Slovak files).
    /// V7 names no page; `None` keeps the historical 1252 decoding.
    pub code_page: Option<u16>,
}

/// A translated reference drawing that already exists: asked about,
/// overwritten, or kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExistingReference {
    #[default]
    Prompt,
    Overwrite,
    Keep,
}

/// The External DGN references choice: translated into drawings attached
/// as external references, left out, or attached as DGN underlays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReferenceMode {
    Translate,
    #[default]
    Ignore,
    Underlay,
}

/// What to attach and how the reference is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct XrefAttachRequest {
    /// The drawing file as chosen (absolute).
    pub path: String,
    pub overlay: bool,
    pub path_type: Pathtype,
}

/// Which window the pick opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtteditKind {
    /// EATTEDIT: the enhanced attribute editor.
    Enhanced,
    /// ATTEDIT: the attribute value form.
    Classic,
}
