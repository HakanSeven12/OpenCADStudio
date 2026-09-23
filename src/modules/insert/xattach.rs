// XATTACH placement — attach an external DWG/DXF file as an XREF block and
// insert it.
//
// The reference options (type, path type, fixed or on-screen scale,
// insertion point and rotation) come from the Attach External Reference
// dialog or from -XREF Attach / Overlay. Values left "on-screen" are asked
// here with the reference prompts:
//   Specify insertion point or [Scale/X/Y/Z/Rotate/PScale/PX/PY/PZ/PRotate]:
//   Enter X scale factor, specify opposite corner, or [Corner/XYZ] <1>:
//   Enter Y scale factor <use X scale factor>:
//   Specify rotation angle <0>:
// The definition is created when the INSERT is committed (see the command
// driver), so one undo removes both.

use acadrust::entities::Insert;
use acadrust::tables::block_record::{BlockFlags, BlockRecord};
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, WorkingPlane};
use crate::io::xref_model::{normalize_lexical, Pathtype};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "XATTACH",
        label: "Attach XREF",
        icon: IconKind::Svg(include_bytes!("../../../assets/icons/blocks/insert.svg")),
        event: ModuleEvent::Command("XATTACH".to_string()),
    }
}

/// What to attach and how the reference is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct XrefAttachRequest {
    /// The drawing file as chosen (absolute).
    pub path: String,
    pub overlay: bool,
    pub path_type: Pathtype,
}

/// Placement values fixed before the command starts; `None` = on-screen.
#[derive(Debug, Clone, Copy, Default)]
pub struct XrefPlacement {
    pub insert: Option<DVec3>,
    pub scale: Option<[f64; 3]>,
    /// Degrees.
    pub rotation: Option<f64>,
}

impl XrefPlacement {
    /// Everything on-screen, as -XREF asks it.
    pub fn on_screen() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preset {
    Scale,
    X,
    Y,
    Z,
    Rotate,
    PScale,
    PX,
    PY,
    PZ,
    PRotate,
}

impl Preset {
    fn from_keyword(text: &str) -> Option<Self> {
        Some(match text {
            "S" | "SCALE" => Preset::Scale,
            "X" => Preset::X,
            "Y" => Preset::Y,
            "Z" => Preset::Z,
            "R" | "ROTATE" => Preset::Rotate,
            "PS" | "PSCALE" => Preset::PScale,
            "PX" => Preset::PX,
            "PY" => Preset::PY,
            "PZ" => Preset::PZ,
            "PR" | "PROTATE" => Preset::PRotate,
            _ => return None,
        })
    }

    fn prompt(self) -> &'static str {
        match self {
            Preset::Scale => "Specify scale factor for XYZ axes:",
            Preset::X => "Specify X scale factor:",
            Preset::Y => "Specify Y scale factor:",
            Preset::Z => "Specify Z Scale factor:",
            Preset::Rotate => "Specify rotation angle:",
            Preset::PScale => "Specify preview scale factor for XYZ axes:",
            Preset::PX => "Specify preview X scale factor:",
            Preset::PY => "Specify preview Y scale factor:",
            Preset::PZ => "Specify preview Z scale factor:",
            Preset::PRotate => "Specify preview rotation angle:",
        }
    }

    fn is_angle(self) -> bool {
        matches!(self, Preset::Rotate | Preset::PRotate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    Insert,
    Preset(Preset),
    XScale,
    Corner,
    YScale,
    XyzX,
    XyzY,
    XyzZ,
    Rotation,
}

pub struct XAttachCommand {
    request: XrefAttachRequest,
    block_name: String,
    /// Command name shown before each prompt (ATTACH, XATTACH or -XREF).
    label: &'static str,
    plane: WorkingPlane,
    step: Step,
    point: Option<DVec3>,
    scale: [f64; 3],
    /// Scale fixed in the dialog or by the Scale/X/Y/Z options.
    scale_fixed: bool,
    /// Degrees.
    rotation: f64,
    rotation_fixed: bool,
}

const NONZERO: &str = "Value must be nonzero.";

impl XAttachCommand {
    pub fn new(request: XrefAttachRequest, placement: XrefPlacement, label: &'static str) -> Self {
        let block_name = path_to_block_name(&request.path);
        let mut command = Self {
            request,
            block_name,
            label,
            plane: WorkingPlane::default(),
            step: Step::Insert,
            point: placement.insert,
            scale: placement.scale.unwrap_or([1.0; 3]),
            scale_fixed: placement.scale.is_some(),
            rotation: placement.rotation.unwrap_or(0.0),
            rotation_fixed: placement.rotation.is_some(),
        };
        if command.point.is_some() {
            command.step = command.next_step().unwrap_or(Step::Rotation);
        }
        command
    }

    /// Attach with every value on-screen.
    #[cfg(test)]
    pub fn with_path(path: String) -> Self {
        Self::new(
            XrefAttachRequest {
                path,
                overlay: false,
                path_type: Pathtype::Full,
            },
            XrefPlacement::on_screen(),
            "XATTACH",
        )
    }

    /// The finished INSERT when nothing is left to ask (all values fixed in
    /// the dialog).
    pub fn immediate(&self) -> Option<EntityType> {
        (self.point.is_some() && self.next_step().is_none()).then(|| self.commit_insert())
    }

    /// The step after the insertion point, or `None` when it can commit.
    fn next_step(&self) -> Option<Step> {
        if !self.scale_fixed {
            Some(Step::XScale)
        } else if !self.rotation_fixed {
            Some(Step::Rotation)
        } else {
            None
        }
    }

    fn advance(&mut self) -> CmdResult {
        match self.next_step() {
            Some(step) => {
                self.step = step;
                CmdResult::NeedPoint
            }
            None => CmdResult::CommitAndExit(self.commit_insert()),
        }
    }

    /// Scale settled at this step: the rotation (if asked) comes next.
    fn scale_done(&mut self) -> CmdResult {
        self.scale_fixed = true;
        self.advance()
    }

    fn commit_insert(&self) -> EntityType {
        let point = self.point.unwrap_or(DVec3::ZERO);
        let mut ins = Insert::new(
            self.block_name.clone(),
            Vector3::new(point.x, point.y, point.z),
        );
        ins.set_x_scale(self.scale[0]);
        ins.set_y_scale(self.scale[1]);
        ins.set_z_scale(self.scale[2]);
        ins.rotation = self.rotation.to_radians();
        self.plane.place_entity(EntityType::Insert(ins))
    }

    /// Corner pick at the X scale prompt: X and Y from the rectangle,
    /// Z follows X.
    fn corner(&mut self, pt: DVec3) -> CmdResult {
        let base = self.point.unwrap_or(DVec3::ZERO);
        let corner = self.plane.to_local(pt);
        let (sx, sy) = (corner.x - base.x, corner.y - base.y);
        if sx.abs() < 1e-12 || sy.abs() < 1e-12 {
            return CmdResult::ReportError(NONZERO.to_string());
        }
        self.scale = [sx, sy, sx.abs()];
        self.scale_done()
    }
}

fn parse_number(text: &str) -> Option<f64> {
    text.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

impl CadCommand for XAttachCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "XATTACH"
    }

    fn prompt(&self) -> String {
        let text = match self.step {
            Step::Insert => {
                "Specify insertion point or [Scale/X/Y/Z/Rotate/PScale/PX/PY/PZ/PRotate]:"
            }
            Step::Preset(preset) => preset.prompt(),
            Step::XScale => {
                "Enter X scale factor, specify opposite corner, or [Corner/XYZ] <1>:"
            }
            Step::Corner => "Specify opposite corner:",
            Step::YScale => "Enter Y scale factor <use X scale factor>:",
            Step::XyzX => "Specify X scale factor or [Corner] <1>:",
            Step::XyzY => "Enter Y scale factor <use X scale factor>:",
            Step::XyzZ => "Specify Z scale factor or <use X scale factor>:",
            Step::Rotation => "Specify rotation angle <0>:",
        };
        format!("{}  {}", self.label, text)
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Insert => vec![
                CmdOption::new("Scale", "S"),
                CmdOption::new("X", "X"),
                CmdOption::new("Y", "Y"),
                CmdOption::new("Z", "Z"),
                CmdOption::new("Rotate", "R"),
                CmdOption::new("PScale", "PS"),
                CmdOption::new("PX", "PX"),
                CmdOption::new("PY", "PY"),
                CmdOption::new("PZ", "PZ"),
                CmdOption::new("PRotate", "PR"),
            ],
            Step::XScale => vec![CmdOption::new("Corner", "C"), CmdOption::new("XYZ", "XYZ")],
            Step::XyzX => vec![CmdOption::new("Corner", "C")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Insert | Step::Corner => InputKind::Point,
            _ => InputKind::SingleToken,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.step == Step::Insert
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::Insert => {
                self.point = Some(self.plane.to_local(pt));
                self.advance()
            }
            Step::XScale | Step::Corner | Step::XyzX => self.corner(pt),
            Step::Rotation => {
                let base = self.plane.to_world(self.point.unwrap_or(DVec3::ZERO));
                self.rotation = self.plane.angle(base, pt).unwrap_or(0.0).to_degrees();
                CmdResult::CommitAndExit(self.commit_insert())
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let token = text.trim().to_uppercase();
        match self.step {
            Step::Insert => {
                let preset = Preset::from_keyword(&token)?;
                self.step = Step::Preset(preset);
                Some(CmdResult::NeedPoint)
            }
            Step::Preset(preset) => {
                let value = if preset.is_angle() {
                    crate::entities::common::parse_typed_angle(text).map(f64::to_degrees)
                } else {
                    parse_number(text)
                };
                let Some(value) = value else {
                    return Some(CmdResult::NeedPoint);
                };
                if !preset.is_angle() && value == 0.0 {
                    return Some(CmdResult::ReportError(NONZERO.to_string()));
                }
                match preset {
                    Preset::Scale => {
                        self.scale = [value; 3];
                        self.scale_fixed = true;
                    }
                    Preset::X => {
                        self.scale[0] = value;
                        self.scale_fixed = true;
                    }
                    Preset::Y => {
                        self.scale[1] = value;
                        self.scale_fixed = true;
                    }
                    Preset::Z => {
                        self.scale[2] = value;
                        self.scale_fixed = true;
                    }
                    Preset::Rotate => {
                        self.rotation = value;
                        self.rotation_fixed = true;
                    }
                    // Preview-only values: the reference asks the real ones
                    // after the insertion point as usual.
                    Preset::PScale | Preset::PX | Preset::PY | Preset::PZ | Preset::PRotate => {}
                }
                self.step = Step::Insert;
                Some(CmdResult::NeedPoint)
            }
            Step::XScale | Step::XyzX if token == "C" || token == "CORNER" => {
                self.step = Step::Corner;
                Some(CmdResult::NeedPoint)
            }
            Step::XScale if token == "XYZ" => {
                self.step = Step::XyzX;
                Some(CmdResult::NeedPoint)
            }
            Step::XScale | Step::XyzX => {
                let value = parse_number(text)?;
                if value == 0.0 {
                    return Some(CmdResult::ReportError(NONZERO.to_string()));
                }
                self.scale = [value, value, value.abs()];
                self.step = if self.step == Step::XScale {
                    Step::YScale
                } else {
                    Step::XyzY
                };
                Some(CmdResult::NeedPoint)
            }
            Step::YScale | Step::XyzY => {
                let value = parse_number(text)?;
                if value == 0.0 {
                    return Some(CmdResult::ReportError(NONZERO.to_string()));
                }
                self.scale[1] = value;
                if self.step == Step::XyzY {
                    self.step = Step::XyzZ;
                    return Some(CmdResult::NeedPoint);
                }
                Some(self.scale_done())
            }
            Step::XyzZ => {
                let value = parse_number(text)?;
                if value == 0.0 {
                    return Some(CmdResult::ReportError(NONZERO.to_string()));
                }
                self.scale[2] = value;
                Some(self.scale_done())
            }
            Step::Rotation => {
                let rotation = crate::entities::common::parse_typed_angle(text)?;
                self.rotation = rotation.to_degrees();
                Some(CmdResult::CommitAndExit(self.commit_insert()))
            }
            Step::Corner => None,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Insert => {
                CmdResult::ReportError("Point or option keyword required.".to_string())
            }
            Step::Preset(_) => {
                self.step = Step::Insert;
                CmdResult::NeedPoint
            }
            Step::XScale | Step::XyzX => {
                self.scale = [1.0; 3];
                self.step = if self.step == Step::XScale {
                    Step::YScale
                } else {
                    Step::XyzY
                };
                CmdResult::NeedPoint
            }
            Step::YScale => {
                self.scale[1] = self.scale[0];
                self.scale_done()
            }
            Step::XyzY => {
                self.scale[1] = self.scale[0];
                self.step = Step::XyzZ;
                CmdResult::NeedPoint
            }
            Step::XyzZ => {
                self.scale[2] = self.scale[0];
                self.scale_done()
            }
            Step::Corner => CmdResult::NeedPoint,
            Step::Rotation => {
                self.rotation = 0.0;
                CmdResult::CommitAndExit(self.commit_insert())
            }
        }
    }

    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }

    fn xattach_request(&self) -> Option<XrefAttachRequest> {
        Some(self.request.clone())
    }
}

/// Reference name for a file: its name without the extension, case kept.
pub fn path_to_block_name(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    std::path::Path::new(&normalized)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "XREF".to_string())
}

/// Collision-free block name: `stem` unless taken (case-insensitive), else
/// `stem_1`, `stem_2`, ... (`$0$` numbering belongs to Bind, not Attach.)
pub fn unique_block_name(stem: &str, taken: &[impl AsRef<str>]) -> String {
    let upper_taken: Vec<String> = taken.iter().map(|t| t.as_ref().to_uppercase()).collect();
    if !upper_taken.iter().any(|t| t == &stem.to_uppercase()) {
        return stem.to_string();
    }
    let mut n = 1u32;
    loop {
        let candidate = format!("{stem}_{n}");
        if !upper_taken
            .iter()
            .any(|t| *t == candidate.to_uppercase())
        {
            return candidate;
        }
        n += 1;
    }
}

fn is_absolute_xref_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    std::path::Path::new(path).is_absolute()
        || path.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

/// Full store path: a relative incoming path is joined onto the host
/// drawing's base dir; absolute paths and missing base dirs pass through.
/// Separators are normalized to `/` for stable DWG storage.
pub fn resolve_xref_store_path(path: &str, host_base_dir: Option<&std::path::Path>) -> String {
    let p = std::path::Path::new(path);
    if is_absolute_xref_path(path) {
        return path.replace('\\', "/");
    }
    match host_base_dir {
        Some(base) => base.join(p).to_string_lossy().replace('\\', "/"),
        None => path.to_string(),
    }
}

/// True when attaching `ref_path` would attach the host drawing into itself.
/// Lexical comparison only (no filesystem touch — the target may be missing);
/// a relative ref is resolved against `host_base_dir` (falling back to the
/// host file's parent) before comparing.
pub fn is_self_attach(
    host_file: &std::path::Path,
    ref_path: &str,
    host_base_dir: Option<&std::path::Path>,
) -> bool {
    let joined = resolve_xref_store_path(ref_path, host_base_dir);
    let candidate = if is_absolute_xref_path(&joined) {
        joined
    } else if let Some(parent) = host_file.parent() {
        parent
            .join(&joined)
            .to_string_lossy()
            .replace('\\', "/")
    } else {
        joined
    };
    normalize_lexical(&candidate) == normalize_lexical(&host_file.to_string_lossy())
}

/// The drawing's existing reference for `name`, if any (case-insensitive).
pub fn existing_reference(scene: &Scene, name: &str) -> Option<String> {
    scene
        .document
        .block_records
        .iter()
        .find(|br| {
            (br.flags.is_xref || br.flags.is_xref_overlay) && br.name.eq_ignore_ascii_case(name)
        })
        .map(|br| br.name.clone())
}

/// The path to store for `request`: full, relative to the saved host, or
/// the file name alone. A relative path needs a saved host; until then the
/// full path is kept, as the reference does.
pub fn stored_path(request: &XrefAttachRequest, host_file: Option<&std::path::Path>) -> String {
    let full = resolve_xref_store_path(&request.path, None);
    match request.path_type {
        Pathtype::Full => full,
        Pathtype::Relative => host_file
            .and_then(|host| {
                crate::io::xref_model::to_pathtype_result(&full, host, Pathtype::Relative).ok()
            })
            .unwrap_or(full),
        Pathtype::None => std::path::Path::new(&full.replace('\\', "/"))
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(full),
    }
}

/// Create (or reuse) the reference definition for `request` and load its
/// content. Returns the block name the INSERT must use.
///
/// A reference of the same name already in the drawing is reused ("Using
/// existing definition."). The self-attach guard runs before this, where the
/// host path is known.
pub fn prepare_xref_definition(
    scene: &mut Scene,
    request: &XrefAttachRequest,
    host_file: Option<&std::path::Path>,
) -> String {
    let stem = path_to_block_name(&request.path);
    if let Some(existing) = existing_reference(scene, &stem) {
        return existing;
    }
    let taken: Vec<String> = scene
        .document
        .block_records
        .iter()
        .map(|b| b.name.clone())
        .collect();
    let block_name = unique_block_name(&stem, &taken);
    let store_path = stored_path(request, host_file);

    let mut br = BlockRecord::new(&block_name);
    br.handle = scene.document.allocate_handle();
    br.flags = BlockFlags {
        is_xref: !request.overlay,
        is_xref_overlay: request.overlay,
        anonymous: false,
        has_attributes: false,
        is_external: false,
        // A freshly attached reference is loaded; UNLOAD flips this later.
        is_xref_unloaded: false,
    };
    br.xref_path = store_path.clone();
    let key = br.handle;
    let _ = scene.document.block_records.add(br);

    // BLOCK / ENDBLK markers linked through the record.
    crate::io::xref::ensure_block_entities(&mut scene.document, &block_name);

    // Load this reference only. A stored relative path resolves against the
    // host folder; a full path or bare file name against the chosen file's.
    let file_dir = std::path::Path::new(&resolve_xref_store_path(&request.path, None))
        .parent()
        .map(|p| p.to_path_buf());
    let base_dir = match request.path_type {
        Pathtype::Relative => host_file.and_then(|h| h.parent()).map(|p| p.to_path_buf()),
        _ => None,
    }
    .or(file_dir);
    if let Some(base_dir) = base_dir {
        let keys: rustc_hash::FxHashSet<acadrust::types::Handle> = [key].into_iter().collect();
        let _ = crate::io::xref::resolve_xrefs_for_keys(&mut scene.document, &base_dir, &keys);
    }

    block_name
}

/// Full-path attach of `path` (kept for callers that only have a path).
#[cfg(test)]
pub fn prepare_xref_block(
    scene: &mut Scene,
    path: &str,
    host_base_dir: Option<&std::path::Path>,
) -> String {
    let request = XrefAttachRequest {
        path: resolve_xref_store_path(path, host_base_dir),
        overlay: false,
        path_type: Pathtype::Full,
    };
    prepare_xref_definition(scene, &request, None)
}

#[cfg(test)]
mod tests {
    use super::{
        is_self_attach, path_to_block_name, resolve_xref_store_path, unique_block_name,
        XAttachCommand,
    };
    use crate::command::{CadCommand, CmdResult};
    use acadrust::EntityType;
    use glam::DVec3;

    #[test]
    fn block_name_collision_gets_suffix() {
        assert_eq!(unique_block_name("PLAN", &["PLAN"]), "PLAN_1");
        assert_eq!(unique_block_name("PLAN", &["PLAN", "PLAN_1"]), "PLAN_2");
    }

    #[test]
    fn unique_block_name_is_case_insensitive() {
        assert_eq!(unique_block_name("PLAN", &["plan"]), "PLAN_1");
        assert_eq!(unique_block_name("plan", &["PLAN", "plan_1"]), "plan_2");
        assert_eq!(unique_block_name("NEW", &["PLAN"]), "NEW");
    }

    #[test]
    fn block_name_keeps_the_file_name_case() {
        assert_eq!(path_to_block_name("C:/refs/plan.dwg"), "plan");
        assert_eq!(path_to_block_name("C:\\refs\\Site Plan.dwg"), "Site Plan");
    }

    #[test]
    fn relative_ref_resolves_against_host_base() {
        let base = std::path::Path::new("C:/Drawings");
        assert_eq!(
            resolve_xref_store_path("refs/plan.dwg", Some(base)),
            "C:/Drawings/refs/plan.dwg"
        );
        assert_eq!(
            resolve_xref_store_path("C:/Lib/plan.dwg", Some(base)),
            "C:/Lib/plan.dwg"
        );
        assert_eq!(resolve_xref_store_path("refs/plan.dwg", None), "refs/plan.dwg");
    }

    #[test]
    fn self_attach_guard_matches_normalized_paths() {
        let host = std::path::Path::new("C:/Drawings/host.dwg");
        let base = std::path::Path::new("C:/Drawings");
        assert!(is_self_attach(host, "C:/Drawings/host.dwg", Some(base)));
        assert!(is_self_attach(host, "c:/Drawings/host.dwg", Some(base)));
        assert!(is_self_attach(host, "C:/Drawings/./host.dwg", Some(base)));
        assert!(is_self_attach(host, "host.dwg", Some(base)));
        assert!(is_self_attach(host, "C:/Drawings/host.dwg", None));
        assert!(!is_self_attach(host, "C:/Drawings/other.dwg", Some(base)));
        assert!(!is_self_attach(host, "other.dwg", Some(base)));
        assert!(!is_self_attach(host, "other.dwg", None));
    }

    #[test]
    fn enter_at_insertion_point_asks_again() {
        let mut cmd = XAttachCommand::with_path("C:/refs/plan.dwg".to_string());
        assert!(matches!(cmd.on_enter(), CmdResult::ReportError(_)));
        assert!(cmd.prompt().contains("Specify insertion point"));
    }

    fn committed(result: Option<CmdResult>) -> acadrust::entities::Insert {
        match result {
            Some(CmdResult::CommitAndExit(EntityType::Insert(ins))) => ins,
            _ => panic!("expected CommitAndExit(Insert)"),
        }
    }

    #[test]
    fn x_scale_then_default_y_and_rotation() {
        let mut cmd = XAttachCommand::with_path("C:/refs/plan.dwg".to_string());
        assert!(matches!(cmd.on_point(DVec3::new(10.0, 20.0, 0.0)), CmdResult::NeedPoint));
        assert!(cmd.prompt().contains("Enter X scale factor"));
        assert!(matches!(cmd.on_text_input("-2"), Some(CmdResult::NeedPoint)));
        assert!(cmd.prompt().contains("Enter Y scale factor"));
        assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint));
        assert!(cmd.prompt().contains("Specify rotation angle <0>"));
        let ins = committed(Some(cmd.on_enter()));
        assert_eq!(ins.block_name, "plan");
        assert!((ins.x_scale() + 2.0).abs() < 1e-9);
        assert!((ins.y_scale() + 2.0).abs() < 1e-9);
        // Z follows the size of X, not its sign.
        assert!((ins.z_scale() - 2.0).abs() < 1e-9);
        assert!((ins.insert_point.x - 10.0).abs() < 1e-9);
    }

    #[test]
    fn zero_scale_is_refused() {
        let mut cmd = XAttachCommand::with_path("C:/refs/plan.dwg".to_string());
        cmd.on_point(DVec3::ZERO);
        assert!(matches!(cmd.on_text_input("0"), Some(CmdResult::ReportError(_))));
        assert!(cmd.prompt().contains("Enter X scale factor"));
    }

    #[test]
    fn xyz_option_takes_three_factors() {
        let mut cmd = XAttachCommand::with_path("C:/refs/plan.dwg".to_string());
        cmd.on_point(DVec3::ZERO);
        cmd.on_text_input("XYZ");
        cmd.on_text_input("2");
        cmd.on_text_input("3");
        cmd.on_text_input("4");
        let ins = committed(cmd.on_text_input("90"));
        assert!((ins.x_scale() - 2.0).abs() < 1e-9);
        assert!((ins.y_scale() - 3.0).abs() < 1e-9);
        assert!((ins.z_scale() - 4.0).abs() < 1e-9);
        assert!((ins.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn corner_pick_sets_x_and_y() {
        let mut cmd = XAttachCommand::with_path("C:/refs/plan.dwg".to_string());
        cmd.on_point(DVec3::ZERO);
        assert!(matches!(cmd.on_point(DVec3::new(5.0, 7.0, 0.0)), CmdResult::NeedPoint));
        let ins = committed(Some(cmd.on_enter()));
        assert!((ins.x_scale() - 5.0).abs() < 1e-9);
        assert!((ins.y_scale() - 7.0).abs() < 1e-9);
        assert!((ins.z_scale() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn scale_preset_skips_the_scale_prompts() {
        let mut cmd = XAttachCommand::with_path("C:/refs/plan.dwg".to_string());
        cmd.on_text_input("S");
        assert!(cmd.prompt().contains("Specify scale factor for XYZ axes"));
        cmd.on_text_input("2");
        cmd.on_point(DVec3::ZERO);
        assert!(cmd.prompt().contains("Specify rotation angle <0>"));
        let ins = committed(Some(cmd.on_enter()));
        assert!((ins.z_scale() - 2.0).abs() < 1e-9);
    }
}
