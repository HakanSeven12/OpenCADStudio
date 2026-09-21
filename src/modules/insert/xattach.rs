// XATTACH command — attach an external DWG/DXF file as an XREF block
// and insert it at a picked point.
//
// Workflow:
//   Step 1 (point pick): user clicks the insertion point.
//   Step 2 (text input): scale factor (default 1.0).
//   Step 3 (text input): rotation angle (default 0.0).
//   Result: BlockRecord + Block entities are created with is_xref=true,
//           then an INSERT entity is committed with the chosen scale/rotation.

use acadrust::entities::{Block, BlockEnd, Insert};
use acadrust::tables::block_record::{BlockFlags, BlockRecord};
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;

use crate::t;

use crate::command::{CadCommand, CmdResult, InputKind, WorkingPlane};
use crate::io::xref_model::normalize_lexical;
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

/// Multi-step placement state: insertion point, then scale, then rotation.
/// Mirrors the INSERT command's scale/rotation value handling (single value
/// typed after the point pick, bare Enter keeps the default).
enum XAttachStep {
    Point,
    Scale { point: DVec3 },
    Rotate { point: DVec3, sx: f64, sy: f64, sz: f64 },
}

pub struct XAttachCommand {
    path: String,
    block_name: String,
    plane: WorkingPlane,
    step: XAttachStep,
}

impl XAttachCommand {
    /// Create an XATTACH command with a path already filled in (from file-picker).
    pub fn with_path(path: String) -> Self {
        let block_name = path_to_block_name(&path);
        Self {
            path,
            block_name,
            plane: WorkingPlane::default(),
            step: XAttachStep::Point,
        }
    }

    fn commit_insert(&self, point: DVec3, sx: f64, sy: f64, sz: f64, rotation: f64) -> EntityType {
        let mut ins = Insert::new(
            self.block_name.clone(),
            Vector3::new(point.x, point.y, point.z),
        );
        ins.set_x_scale(sx);
        ins.set_y_scale(sy);
        ins.set_z_scale(sz);
        ins.rotation = rotation;
        self.plane.place_entity(EntityType::Insert(ins))
    }
}

impl CadCommand for XAttachCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "XATTACH"
    }

    fn prompt(&self) -> String {
        match &self.step {
            XAttachStep::Point => t!(
                "XATTACH  Specify insertion point for \"%{name}\":",
                name = self.block_name
            )
            .into_owned(),
            XAttachStep::Scale { .. } => {
                t!("XATTACH  Specify scale factor <1.0> (X,Y,Z or pick Corner):").into_owned()
            }
            XAttachStep::Rotate { .. } => {
                t!("XATTACH  Specify rotation angle <0>:").into_owned()
            }
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            XAttachStep::Point => InputKind::Point,
            XAttachStep::Scale { .. } | XAttachStep::Rotate { .. } => InputKind::SingleToken,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            XAttachStep::Point => {
                let point = self.plane.to_local(pt);
                self.step = XAttachStep::Scale { point };
                CmdResult::NeedPoint
            }
            XAttachStep::Scale { point } => {
                // Corner: derive X/Y scale from the picked opposite corner.
                let corner = self.plane.to_local(pt);
                let sx = (corner.x - point.x).abs();
                let sy = (corner.y - point.y).abs();
                self.step = XAttachStep::Rotate {
                    point,
                    sx: if sx < 1e-9 { 1.0 } else { sx },
                    sy: if sy < 1e-9 { 1.0 } else { sy },
                    sz: 1.0,
                };
                CmdResult::NeedPoint
            }
            XAttachStep::Rotate { point, sx, sy, sz } => {
                // Second point defines the rotation direction from the base.
                let base_world = self.plane.to_world(point);
                let rotation = self.plane.angle(base_world, pt).unwrap_or(0.0);
                CmdResult::CommitAndExit(self.commit_insert(point, sx, sy, sz, rotation))
            }
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.step {
            XAttachStep::Point => None,
            XAttachStep::Scale { point } => {
                // Single value (uniform) or X/Y/Z triplet; invalid input is
                // consumed and re-prompts (mirrors INSERT's scale handling).
                if let Some((sx, sy, sz)) = parse_scale_spec(text) {
                    self.step = XAttachStep::Rotate { point, sx, sy, sz };
                }
                Some(CmdResult::NeedPoint)
            }
            XAttachStep::Rotate { point, sx, sy, sz } => {
                if let Some(rotation) = crate::entities::common::parse_typed_angle(text) {
                    Some(CmdResult::CommitAndExit(
                        self.commit_insert(point, sx, sy, sz, rotation),
                    ))
                } else {
                    Some(CmdResult::NeedPoint)
                }
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            // Bare Enter at the insertion-point step cancels (unchanged).
            XAttachStep::Point => CmdResult::Cancel,
            // Bare Enter accepts the default scale / rotation.
            XAttachStep::Scale { point } => {
                self.step = XAttachStep::Rotate {
                    point,
                    sx: 1.0,
                    sy: 1.0,
                    sz: 1.0,
                };
                CmdResult::NeedPoint
            }
            XAttachStep::Rotate { point, sx, sy, sz } => {
                CmdResult::CommitAndExit(self.commit_insert(point, sx, sy, sz, 0.0))
            }
        }
    }

    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> {
        // XREF-Task6: PScale drag preview — no framework hook; revisit
        vec![]
    }

    fn xattach_path(&self) -> Option<String> {
        Some(self.path.clone())
    }
}

/// Derive a block name from the file path: take the file stem, uppercase it.
pub fn path_to_block_name(path: &str) -> String {
    let p = std::path::Path::new(path);
    p.file_stem()
        .map(|s| s.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| "XREF".to_string())
}

/// Collision-free block name: `stem` unless taken (case-insensitive — block
/// names are uppercase by convention), else `stem_1`, `stem_2`, ...
/// (`$0$` numbering belongs to Bind, not Attach.)
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

/// Attach-default-Full store path: a relative incoming path is joined onto
/// the host drawing's base dir (yielding an absolute path when the base dir
/// is absolute); absolute paths and missing base dirs pass through verbatim.
/// Separators are normalized to `/` for stable DWG storage.
fn is_absolute_xref_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    std::path::Path::new(path).is_absolute()
        || path.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

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

// Host-path note (XREF-Task6): `Scene` exposes no host file path — it owns
// the document plus view state only (verified in scene/mod.rs: no file path
// field). The host drawing path lives one layer up, in `Tab::current_path`
// (app/document.rs). The self-attach guard therefore runs at the command
// driver layer, which owns both the tab path and the pending XATTACH path,
// via this pure, unit-tested helper. `prepare_xref_block` below stays
// side-effect-only and keeps its `-> String` return.

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

/// Parse a typed scale: a single uniform value, or an X/Y/Z triplet separated
/// by commas, semicolons or whitespace. Zero never scales (rejected, like
/// INSERT); unparseable input returns None and the caller re-prompts.
fn parse_scale_spec(text: &str) -> Option<(f64, f64, f64)> {
    let norm = text.trim().replace([',', ';'], " ");
    let parts: Vec<&str> = norm.split_whitespace().collect();
    match parts.as_slice() {
        [single] => single
            .parse::<f64>()
            .ok()
            .filter(|v| *v != 0.0)
            .map(|v| (v, v, 1.0)),
        [x, y, z] => {
            let parsed: Option<Vec<f64>> =
                [x, y, z].iter().map(|s| s.parse::<f64>().ok()).collect();
            match parsed {
                Some(v) if v.iter().all(|n| *n != 0.0) => Some((v[0], v[1], v[2])),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Create the XREF BlockRecord + Block/EndBlock entities in the scene document
/// for a given file path.  Returns the block name.
///
/// This must be called before committing the INSERT so that the block
/// definition exists when the renderer looks it up.
///
/// Name collisions get a `_1`, `_2`... suffix via [`unique_block_name`]; the
/// stored path follows Attach-default-Full via [`resolve_xref_store_path`].
/// Self-attach blocking is NOT done here — see the note on [`is_self_attach`];
/// the driver checks it before calling.
pub fn prepare_xref_block(
    scene: &mut Scene,
    path: &str,
    host_base_dir: Option<&std::path::Path>,
) -> String {
    let stem = path_to_block_name(path);
    let taken: Vec<String> = scene
        .document
        .block_records
        .iter()
        .map(|b| b.name.clone())
        .collect();
    let block_name = unique_block_name(&stem, &taken);

    // If a BlockRecord already exists with this name, skip creation.
    if scene.document.block_records.get(&block_name).is_some() {
        return block_name;
    }

    let store_path = resolve_xref_store_path(path, host_base_dir);

    // Create the BlockRecord.
    let mut br = BlockRecord::new(&block_name);
    br.handle = scene.document.allocate_handle();
    br.flags = BlockFlags {
        is_xref: true,
        is_xref_overlay: false,
        anonymous: false,
        has_attributes: false,
        is_external: false,
        // A freshly attached reference is loaded; UNLOAD flips this later.
        is_xref_unloaded: false,
    };
    br.xref_path = store_path.clone();
    let _ = scene.document.block_records.add(br);

    // Create BLOCK entity.
    let b = Block::new(&block_name, Vector3::zero()).with_xref_path(&store_path);
    let _ = scene.document.add_entity(EntityType::Block(b));
    let _ = scene
        .document
        .add_entity(EntityType::BlockEnd(BlockEnd::new()));

    // Resolve the XREF content immediately.
    // Every reference in this host resolves relative to the host drawing, not
    // relative to the newly-attached file. Using `store_path.parent()` here
    // made an attach unexpectedly redirect existing relative xrefs.
    if let Some(base_dir) = host_base_dir {
        let _ = crate::io::xref::resolve_xrefs(&mut scene.document, base_dir);
    } else if let Some(base_dir) = std::path::Path::new(&store_path).parent() {
        let _ = crate::io::xref::resolve_xrefs(&mut scene.document, base_dir);
    }

    block_name
}

#[cfg(test)]
mod tests {
    use super::{
        is_self_attach, parse_scale_spec, path_to_block_name, resolve_xref_store_path,
        unique_block_name, XAttachCommand,
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
    fn block_name_derives_from_stem() {
        assert_eq!(path_to_block_name("C:/refs/plan.dwg"), "PLAN");
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
        // Verbatim, drive-letter case, dot segments, and base-relative forms.
        assert!(is_self_attach(host, "C:/Drawings/host.dwg", Some(base)));
        assert!(is_self_attach(host, "c:/Drawings/host.dwg", Some(base)));
        assert!(is_self_attach(host, "C:/Drawings/./host.dwg", Some(base)));
        assert!(is_self_attach(host, "host.dwg", Some(base)));
        assert!(is_self_attach(host, "C:/Drawings/host.dwg", None));
        // Anything else attaches freely.
        assert!(!is_self_attach(host, "C:/Drawings/other.dwg", Some(base)));
        assert!(!is_self_attach(host, "other.dwg", Some(base)));
        assert!(!is_self_attach(host, "other.dwg", None));
    }

    #[test]
    fn scale_spec_parses_single_and_triplet() {
        assert_eq!(parse_scale_spec("2"), Some((2.0, 2.0, 1.0)));
        assert_eq!(parse_scale_spec("2,3,4"), Some((2.0, 3.0, 4.0)));
        assert_eq!(parse_scale_spec("2 3 4"), Some((2.0, 3.0, 4.0)));
        assert_eq!(parse_scale_spec("0"), None);
        assert_eq!(parse_scale_spec("abc"), None);
        assert_eq!(parse_scale_spec("1,2"), None);
    }

    #[test]
    fn enter_at_insertion_point_cancels() {
        let mut cmd = XAttachCommand::with_path("C:/refs/plan.dwg".to_string());
        assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
    }

    #[test]
    fn placement_flow_scale_then_rotate_commits() {
        let mut cmd = XAttachCommand::with_path("C:/refs/plan.dwg".to_string());
        assert!(matches!(
            cmd.on_point(DVec3::new(10.0, 20.0, 0.0)),
            CmdResult::NeedPoint
        ));
        assert!(cmd.prompt().contains("scale"));
        assert!(matches!(cmd.on_text_input("2"), Some(CmdResult::NeedPoint)));
        assert!(cmd.prompt().contains("rotation"));
        match cmd.on_enter() {
            CmdResult::CommitAndExit(EntityType::Insert(ins)) => {
                assert_eq!(ins.block_name, "PLAN");
                assert!((ins.x_scale() - 2.0).abs() < 1e-9);
                assert!((ins.y_scale() - 2.0).abs() < 1e-9);
                assert!(ins.rotation.abs() < 1e-9);
                assert!((ins.insert_point.x - 10.0).abs() < 1e-9);
                assert!((ins.insert_point.y - 20.0).abs() < 1e-9);
            }
            _ => panic!("expected CommitAndExit(Insert)"),
        }
    }

    #[test]
    fn placement_flow_triplet_and_typed_rotation() {
        let mut cmd = XAttachCommand::with_path("C:/refs/plan.dwg".to_string());
        assert!(matches!(
            cmd.on_point(DVec3::ZERO),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            cmd.on_text_input("2,3,4"),
            Some(CmdResult::NeedPoint)
        ));
        match cmd.on_text_input("90") {
            Some(CmdResult::CommitAndExit(EntityType::Insert(ins))) => {
                assert!((ins.x_scale() - 2.0).abs() < 1e-9);
                assert!((ins.y_scale() - 3.0).abs() < 1e-9);
                assert!((ins.z_scale() - 4.0).abs() < 1e-9);
                assert!((ins.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
            }
            _ => panic!("expected CommitAndExit(Insert)"),
        }
    }

    #[test]
    fn placement_flow_corner_pick_sets_xy_scale() {
        let mut cmd = XAttachCommand::with_path("C:/refs/plan.dwg".to_string());
        assert!(matches!(
            cmd.on_point(DVec3::ZERO),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            cmd.on_point(DVec3::new(3.0, 4.0, 0.0)),
            CmdResult::NeedPoint
        ));
        match cmd.on_enter() {
            CmdResult::CommitAndExit(EntityType::Insert(ins)) => {
                assert!((ins.x_scale() - 3.0).abs() < 1e-9);
                assert!((ins.y_scale() - 4.0).abs() < 1e-9);
            }
            _ => panic!("expected CommitAndExit(Insert)"),
        }
    }
}
