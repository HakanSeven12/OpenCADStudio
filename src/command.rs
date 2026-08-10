// Command system — trait for interactive CAD commands.
//
// Each tool that requires user interaction (point picks, object selection,
// numeric input) implements `CadCommand`.  The active command receives
// viewport events from main.rs and returns `CmdResult` tokens that tell
// the host what to do next.

use crate::scene::model::hatch_model::HatchModel;
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use acadrust::{EntityType, Handle};
use glam::DVec3;

// ── Working plane ─────────────────────────────────────────────────────────

/// Full-precision coordinate frame used by interactive commands.
///
/// Points delivered to commands remain WCS points. Commands that construct
/// planar geometry use this frame for their local calculations, then convert
/// the result back to WCS for storage and preview.
#[derive(Clone, Copy, Debug)]
pub struct WorkingPlane {
    pub origin: DVec3,
    pub x: DVec3,
    pub y: DVec3,
    pub z: DVec3,
}

impl Default for WorkingPlane {
    fn default() -> Self {
        Self {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::Y,
            z: DVec3::Z,
        }
    }
}

impl WorkingPlane {
    pub fn is_identity(self) -> bool {
        self.origin.abs_diff_eq(DVec3::ZERO, 1e-12)
            && self.x.abs_diff_eq(DVec3::X, 1e-12)
            && self.y.abs_diff_eq(DVec3::Y, 1e-12)
            && self.z.abs_diff_eq(DVec3::Z, 1e-12)
    }

    pub fn new(origin: DVec3, x: DVec3, y: DVec3) -> Self {
        let x = x.normalize_or(DVec3::X);
        let raw_y = y.normalize_or(DVec3::Y);
        let fallback = if x.dot(DVec3::Z).abs() < 0.999 {
            DVec3::Z
        } else {
            DVec3::Y
        };
        let z = x.cross(raw_y).normalize_or(x.cross(fallback).normalize());
        let y = z.cross(x).normalize();
        Self { origin, x, y, z }
    }

    pub fn to_world(self, point: DVec3) -> DVec3 {
        self.origin + self.x * point.x + self.y * point.y + self.z * point.z
    }

    pub fn to_local(self, point: DVec3) -> DVec3 {
        let delta = point - self.origin;
        DVec3::new(delta.dot(self.x), delta.dot(self.y), delta.dot(self.z))
    }

    pub fn vector_to_world(self, vector: DVec3) -> DVec3 {
        self.x * vector.x + self.y * vector.y + self.z * vector.z
    }

    pub fn vector_to_local(self, vector: DVec3) -> DVec3 {
        DVec3::new(vector.dot(self.x), vector.dot(self.y), vector.dot(self.z))
    }

    pub fn angle(self, from: DVec3, to: DVec3) -> Option<f64> {
        let direction = self.vector_to_local(to - from);
        (direction.x.hypot(direction.y) > f64::EPSILON)
            .then(|| direction.y.atan2(direction.x))
    }

    pub fn to_world_transform(self) -> acadrust::types::Transform {
        use acadrust::types::{Matrix4, Transform};
        Transform::from_matrix(Matrix4 {
            m: [
                [self.x.x, self.y.x, self.z.x, self.origin.x],
                [self.x.y, self.y.y, self.z.y, self.origin.y],
                [self.x.z, self.y.z, self.z.z, self.origin.z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        })
    }

    pub fn to_local_transform(self) -> acadrust::types::Transform {
        use acadrust::types::{Matrix4, Transform};
        Transform::from_matrix(Matrix4 {
            m: [
                [self.x.x, self.x.y, self.x.z, -self.origin.dot(self.x)],
                [self.y.x, self.y.y, self.y.z, -self.origin.dot(self.y)],
                [self.z.x, self.z.y, self.z.z, -self.origin.dot(self.z)],
                [0.0, 0.0, 0.0, 1.0],
            ],
        })
    }

    pub fn place_entity(self, mut entity: EntityType) -> EntityType {
        crate::scene::view::dispatch::apply_transform(
            &mut entity,
            &EntityTransform::Affine(self.to_world_transform()),
        );
        entity
    }
}

#[cfg(test)]
mod working_plane_tests {
    use super::*;

    #[test]
    fn translated_and_rotated_frame_round_trips_points_and_vectors() {
        let plane = WorkingPlane::new(
            DVec3::new(125_000.25, -42_000.5, 810.75),
            DVec3::new(0.0, 1.0, 0.0),
            DVec3::new(0.0, 0.0, 1.0),
        );
        let local = DVec3::new(12.5, -3.25, 7.75);
        let vector = DVec3::new(-2.0, 5.0, 1.5);

        assert!(plane
            .to_local(plane.to_world(local))
            .abs_diff_eq(local, 1e-9));
        assert!(plane
            .vector_to_local(plane.vector_to_world(vector))
            .abs_diff_eq(vector, 1e-12));
    }
}

/// Domain object resolved under the cursor for ObjectPick snapping.
#[derive(Clone, Copy, Debug)]
pub struct ObjectPickHit {
    pub handle: Handle,
    pub x: f64,
    pub y: f64,
    pub label: &'static str,
}

#[derive(Clone)]
pub struct SelectionEntity {
    pub handle: Handle,
    pub entity: EntityType,
    pub surface_area: Option<f64>,
}

#[derive(Clone)]
pub enum AreaPreviewSource {
    Handles(Vec<Handle>),
    Boundary(Vec<[f64; 2]>),
}

#[derive(Clone)]
pub struct AreaPreviewRegion {
    pub source: AreaPreviewSource,
    pub subtract: bool,
}

// ── Transform ─────────────────────────────────────────────────────────────

/// A geometric transformation applied to existing entities.
#[derive(Clone)]
pub enum EntityTransform {
    /// Move every point by the given world-space delta.
    Translate(DVec3),
    /// Rotate around the axis through `center`.
    Rotate {
        center: DVec3,
        axis: DVec3,
        angle_rad: f64,
    },
    /// Uniform scale from `center` by `factor`.
    Scale { center: DVec3, factor: f64 },
    /// Mirror through the plane containing `p1`→`p2` and the working normal.
    Mirror {
        p1: DVec3,
        p2: DVec3,
        working_normal: DVec3,
    },
    /// General affine transform. Used when a complete UCS basis must be baked
    /// into block-local geometry instead of stored as drawing UCS state.
    Affine(acadrust::types::Transform),
}

// ── Tangent object ─────────────────────────────────────────────────────────

/// Geometric representation of a tangent-snap target.
#[derive(Clone, Copy, Debug)]
pub enum TangentObject {
    /// Infinite line through two world-space XZ-plane points.
    Line { p1: DVec3, p2: DVec3 },
    /// Circle in the world XY plane.
    Circle { center: DVec3, radius: f64 },
}

/// One unit of input to the active command's step machine.
///
/// Every input source — the GUI command line, the headless automation feeder,
/// dynamic input, the plugin API, and the viewport (clicks / picks / selection
/// / tangent) — translates its raw input into one of these and routes it
/// through `OpenCADStudio::feed_command`, so a single place drives the command
/// regardless of where the step came from.
///
/// Variants are wired up incrementally as each source is migrated onto
/// `feed_command`; `#[allow(dead_code)]` covers those not yet constructed.
#[allow(dead_code)]
pub enum StepInput {
    /// A coordinate (`on_point`).
    Point(DVec3),
    /// A typed token: keyword, option letter, distance, or value
    /// (`on_text_input`).
    Text(String),
    /// An object pick (`on_entity_pick`).
    EntityPick(Handle, DVec3),
    /// A sub-structure pick — vertex / edge / face (`on_structure_pick`).
    StructurePick(Handle, DVec3),
    /// A completed selection set (`on_selection_complete`).
    SelectionComplete(Vec<Handle>),
    /// A tangent-snap target (`on_tangent_point`).
    Tangent(TangentObject, DVec3),
    /// The text / MText editor closed, with its commit flag (`on_editor_closed`).
    EditorClosed(bool),
    /// Advance / finish the current step — Enter, or Space-as-Enter (`on_enter`).
    Enter,
    /// Cancel the command (`on_escape`).
    Escape,
}

/// Generic interactive front-end for a single-value setting command (PDMODE,
/// PDSIZE, LTSCALE, CELTSCALE, …). Bare `<name>` enters this command, which
/// prompts for one value and then delegates to the existing inline
/// `<name> <value>` handler via [`CmdResult::Dispatch`]. A bare Enter delegates
/// to `<name> ` (trailing space) so the inline handler reports the current
/// value. Keeps the value math + persistence in the one inline place while
/// making the command prompt for its argument step-by-step.
pub struct ValuePromptCommand {
    name: &'static str,
    prompt: &'static str,
}

impl ValuePromptCommand {
    pub fn new(name: &'static str, prompt: &'static str) -> Self {
        Self { name, prompt }
    }
}

impl CadCommand for ValuePromptCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        crate::t!(self.prompt).into_owned()
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        if t.is_empty() {
            Some(CmdResult::Dispatch(format!("{} ", self.name)))
        } else {
            Some(CmdResult::Dispatch(format!("{} {t}", self.name)))
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        // A value command takes no point; ignore stray clicks, keep prompting.
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        // Bare Enter → report the current value via the inline handler.
        CmdResult::Dispatch(format!("{} ", self.name))
    }
}

/// Interactive front-end for RENAME. Prompts for the object type (as clickable
/// buttons), then the current name, then the new name, and delegates to the
/// inline `RENAME <type> <old> <new>` handler via [`CmdResult::Dispatch`] — the
/// rename logic stays in one place while the command prompts step by step like
/// every other command instead of only working with all three arguments typed
/// on one line.
pub struct RenameCommand {
    step: RenameStep,
}

enum RenameStep {
    Type,
    Old { ty: String },
    New { ty: String, old: String },
}

impl RenameCommand {
    /// `(button label, keyword)` for the object types the inline handler can
    /// actually rename.
    const TYPES: [(&'static str, &'static str); 7] = [
        ("Layer", "LAYER"),
        ("Block", "BLOCK"),
        ("Text style", "STYLE"),
        ("Dim style", "DIMSTYLE"),
        ("Linetype", "LINETYPE"),
        ("UCS", "UCS"),
        ("View", "VIEW"),
    ];

    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            step: RenameStep::Type,
        }
    }
}

impl CadCommand for RenameCommand {
    fn name(&self) -> &'static str {
        "RENAME"
    }

    fn prompt(&self) -> String {
        match &self.step {
            RenameStep::Type => crate::t!("RENAME  Select the object type to rename:").into_owned(),
            RenameStep::Old { ty } => crate::t!(
                "RENAME %{type}  Enter the current name:",
                type = ty
            )
            .into_owned(),
            RenameStep::New { ty, old } => crate::t!(
                "RENAME %{type}  Rename \"%{old}\" to:",
                type = ty,
                old = old
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            RenameStep::Type => Self::TYPES
                .iter()
                .map(|(label, keyword)| CmdOption::new(label, keyword))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        if t.is_empty() {
            // Keep prompting for the current step.
            return None;
        }
        // NOTE: a consumed-but-still-prompting input must return
        // `Some(NeedPoint)` (reprint the prompt), NOT `None` — the driver
        // treats `None` as "not consumed" and offers the same text to the
        // command a second time, which would advance two steps at once.
        match &self.step {
            RenameStep::Type => {
                // Accept a known type (or a common alias); re-prompt otherwise so
                // a typo doesn't advance to a rename that can't happen.
                let canonical = match t.to_uppercase().as_str() {
                    "LAYER" | "LA" => "LAYER",
                    "BLOCK" | "B" => "BLOCK",
                    "STYLE" | "TEXTSTYLE" | "ST" => "STYLE",
                    "DIMSTYLE" | "D" => "DIMSTYLE",
                    "LINETYPE" | "LT" => "LINETYPE",
                    "UCS" => "UCS",
                    "VIEW" | "V" => "VIEW",
                    _ => return Some(CmdResult::NeedPoint),
                };
                self.step = RenameStep::Old {
                    ty: canonical.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            RenameStep::Old { ty } => {
                self.step = RenameStep::New {
                    ty: ty.clone(),
                    old: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            RenameStep::New { ty, old } => {
                Some(CmdResult::Dispatch(format!("RENAME {ty} {old} {t}")))
            }
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        // A rename takes no point; ignore stray clicks and keep prompting.
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        // Bare Enter with nothing typed cancels, like the other prompt commands.
        CmdResult::Cancel
    }
}

/// Interactive front-end for USERI / USERR — the five integer / real user
/// registers in the drawing header. They take two inputs (which register, then
/// the value), so they prompt for the register as clickable `1`–`5` buttons and
/// then the value, and delegate to the inline `USERI <n> <value>` handler.
pub struct UserRegCommand {
    /// `"USERI"` or `"USERR"`.
    name: &'static str,
    /// Chosen register 1–5 once the first step is answered.
    slot: Option<u8>,
}

impl UserRegCommand {
    pub fn new(name: &'static str) -> Self {
        Self { name, slot: None }
    }
}

impl CadCommand for UserRegCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        match self.slot {
            None => crate::t!("%{name}  which register?  [1-5]:", name = self.name)
                .into_owned(),
            Some(n) => crate::t!("%{name}%{slot}  new value:", name = self.name, slot = n)
                .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.slot {
            None => (1..=5).map(|n| CmdOption::new(&n.to_string(), &n.to_string())).collect(),
            Some(_) => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        match self.slot {
            None => {
                // Accept a register 1–5; re-prompt on anything else. Consumed
                // either way (`Some(NeedPoint)`) — `None` would feed the same
                // text to the command a second time.
                if let Ok(n @ 1..=5) = t.parse::<u8>() {
                    self.slot = Some(n);
                }
                Some(CmdResult::NeedPoint)
            }
            // The inline handler validates the value (int vs real) and reports
            // usage if it doesn't parse, so just hand the whole line over.
            Some(n) => Some(CmdResult::Dispatch(format!("{} {n} {t}", self.name))),
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// Generic interactive front-end for a keyword sub-verb command (ATTDISP,
/// LAYERSTATE, SCALELISTEDIT, VIEW…). Bare `<name>` enters this command, which
/// shows the sub-verbs as clickable buttons; picking one either dispatches
/// `<name> <keyword>` straight away (an action verb) or, when the verb needs an
/// argument, prompts for it and then dispatches `<name> <keyword> <value>`.
/// Delegates to the existing inline `<name> …` handler so the logic stays in
/// one place, the way [`ValuePromptCommand`] does for single values.
pub struct KeywordCommand {
    name: &'static str,
    prompt: &'static str,
    /// `(button label, keyword, value prompt)`. A `Some` value prompt means the
    /// verb takes one argument collected in a second step; `None` acts alone.
    options: Vec<(&'static str, &'static str, Option<&'static str>)>,
    /// Set once a value-taking verb is chosen: `(keyword, value prompt)`.
    pending: Option<(&'static str, &'static str)>,
    /// Keyword a bare Enter dispatches at the verb step (`<Default>` prompts,
    /// e.g. PLAN's `<Current>`). `None` = Enter cancels, as before.
    default: Option<&'static str>,
}

impl KeywordCommand {
    /// Set the verb a bare Enter dispatches (shown as `<Default>` in prompts).
    pub fn with_default(mut self, kw: &'static str) -> Self {
        self.default = Some(kw);
        self
    }

    pub fn new(
        name: &'static str,
        prompt: &'static str,
        options: Vec<(&'static str, &'static str, Option<&'static str>)>,
    ) -> Self {
        Self {
            name,
            prompt,
            options,
            pending: None,
            default: None,
        }
    }
}

impl CadCommand for KeywordCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        match self.pending {
            Some((_, value_prompt)) => crate::t!(value_prompt).into_owned(),
            None => crate::t!(self.prompt).into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.pending {
            Some(_) => Vec::new(),
            None => self
                .options
                .iter()
                .map(|(label, keyword, _)| CmdOption::new(label, keyword))
                .collect(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        match self.pending {
            // Second step: the argument for the already-chosen verb.
            Some((keyword, _)) => Some(CmdResult::Dispatch(format!("{} {keyword} {t}", self.name))),
            // First step: match the typed / clicked token to a sub-verb.
            // Consumed inputs that keep prompting return `Some(NeedPoint)` —
            // `None` would hand the same text to the command a second time.
            None => {
                let up = t.to_uppercase();
                let Some((_, keyword, value_prompt)) = self.options.iter().find(|(label, k, _)| {
                    k.eq_ignore_ascii_case(&up) || label.eq_ignore_ascii_case(t)
                })
                else {
                    // Unknown verb — keep prompting rather than dispatch garbage.
                    return Some(CmdResult::NeedPoint);
                };
                match value_prompt {
                    Some(vp) => {
                        self.pending = Some((keyword, vp));
                        Some(CmdResult::NeedPoint)
                    }
                    None => Some(CmdResult::Dispatch(format!("{} {keyword}", self.name))),
                }
            }
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        // Bare Enter at the verb step runs the default verb when one is set.
        if self.pending.is_none() {
            if let Some(kw) = self.default {
                return CmdResult::Dispatch(format!("{} {kw}", self.name));
            }
        }
        CmdResult::Cancel
    }
}

/// Generic interactive front-end for a two-argument command (LAYMRG source +
/// target, SETVAR variable + value…). Prompts for the two values in turn and
/// dispatches `<name> <first> <second>` to the existing inline handler. A bare
/// Enter on the second value dispatches `<name> <first>` (no second token) so a
/// getter-style command (SETVAR reading a variable) still works.
pub struct TwoValuePromptCommand {
    name: &'static str,
    prompt1: &'static str,
    prompt2: &'static str,
    first: Option<String>,
}

impl TwoValuePromptCommand {
    pub fn new(name: &'static str, prompt1: &'static str, prompt2: &'static str) -> Self {
        Self {
            name,
            prompt1,
            prompt2,
            first: None,
        }
    }
}

impl CadCommand for TwoValuePromptCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        match &self.first {
            None => crate::t!(self.prompt1).into_owned(),
            Some(_) => crate::t!(self.prompt2).into_owned(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        match &self.first {
            None => {
                if t.is_empty() {
                    return None;
                }
                self.first = Some(t.to_string());
                // Consumed; `None` here would feed the same text back as the
                // second value and dispatch `<name> <x> <x>` in one step.
                Some(CmdResult::NeedPoint)
            }
            Some(first) => Some(CmdResult::Dispatch(if t.is_empty() {
                format!("{} {first}", self.name)
            } else {
                format!("{} {first} {t}", self.name)
            })),
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match &self.first {
            // Enter on the second step with nothing typed = report / no-op form.
            Some(first) => CmdResult::Dispatch(format!("{} {first}", self.name)),
            None => CmdResult::Cancel,
        }
    }
}

/// Generic interactive front-end for a keyword command that operates on the
/// current selection (CHPROP, ADJUST, XDATA, UNDERLAY, DRAWORDER…). If nothing
/// is selected when it starts it first gathers a selection (Enter confirms),
/// then shows the sub-verbs as buttons exactly like [`KeywordCommand`] and
/// dispatches `<name> <verb> [value]` to the inline handler, which reads the
/// (still-selected) set. Verbs the generic form can't express — a second value
/// (XDATA SET) or a reference pick (DRAWORDER ABOVE) — stay available by typing
/// the full argument line.
pub struct SelectThenKeywordCommand {
    name: &'static str,
    prompt: &'static str,
    options: Vec<(&'static str, &'static str, Option<&'static str>)>,
    gathering: bool,
    selected: Vec<Handle>,
    pending: Option<(&'static str, &'static str)>,
}

impl SelectThenKeywordCommand {
    pub fn new(
        name: &'static str,
        prompt: &'static str,
        options: Vec<(&'static str, &'static str, Option<&'static str>)>,
        has_selection: bool,
    ) -> Self {
        Self {
            name,
            prompt,
            options,
            gathering: !has_selection,
            selected: Vec::new(),
            pending: None,
        }
    }
}

impl CadCommand for SelectThenKeywordCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        if self.gathering {
            return crate::t!(
                "%{name}  select objects, then press Enter:",
                name = self.name
            )
            .into_owned();
        }
        match self.pending {
            Some((_, value_prompt)) => crate::t!(value_prompt).into_owned(),
            None => crate::t!(self.prompt).into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.gathering || self.pending.is_some() {
            return Vec::new();
        }
        self.options
            .iter()
            .map(|(label, keyword, _)| CmdOption::new(label, keyword))
            .collect()
    }

    fn wants_text_input(&self) -> bool {
        !self.gathering
    }

    fn is_selection_gathering(&self) -> bool {
        self.gathering
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        // The normal selection system has set the scene selection; remember the
        // set so Enter knows whether anything was picked, and keep gathering.
        self.selected = handles;
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.gathering {
            if self.selected.is_empty() {
                return CmdResult::Cancel;
            }
            self.gathering = false;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        match self.pending {
            Some((keyword, _)) => Some(CmdResult::Dispatch(format!("{} {keyword} {t}", self.name))),
            None => {
                let up = t.to_uppercase();
                let Some((_, keyword, value_prompt)) =
                    self.options.iter().find(|(_, k, _)| k.eq_ignore_ascii_case(&up))
                else {
                    // Unknown verb — consumed, keep prompting (`None` would
                    // feed the same text to the command a second time).
                    return Some(CmdResult::NeedPoint);
                };
                match value_prompt {
                    Some(vp) => {
                        self.pending = Some((keyword, vp));
                        Some(CmdResult::NeedPoint)
                    }
                    None => Some(CmdResult::Dispatch(format!("{} {keyword}", self.name))),
                }
            }
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
}

/// Generic interactive front-end for a single-value command that operates on
/// the current selection (HYPERLINK url, ARCTEXT text, TEXTFIT width, TCASE
/// aside…). Gathers a selection first when none is set (Enter confirms), then
/// prompts for one value and dispatches `<name> <value>` to the inline handler,
/// which reads the still-selected set. A bare Enter on the value step dispatches
/// `<name>` alone — for commands whose value is optional (TCOUNT, TEXTMASK).
pub struct SelectThenValueCommand {
    name: &'static str,
    value_prompt: &'static str,
    gathering: bool,
    selected: Vec<Handle>,
}

impl SelectThenValueCommand {
    pub fn new(name: &'static str, value_prompt: &'static str, has_selection: bool) -> Self {
        Self {
            name,
            value_prompt,
            gathering: !has_selection,
            selected: Vec::new(),
        }
    }
}

impl CadCommand for SelectThenValueCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        if self.gathering {
            crate::t!(
                "%{name}  select objects, then press Enter:",
                name = self.name
            )
            .into_owned()
        } else {
            crate::t!(self.value_prompt).into_owned()
        }
    }

    fn wants_text_input(&self) -> bool {
        !self.gathering
    }

    fn is_selection_gathering(&self) -> bool {
        self.gathering
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.selected = handles;
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.gathering {
            return None;
        }
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        Some(CmdResult::Dispatch(format!("{} {t}", self.name)))
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.gathering {
            if self.selected.is_empty() {
                return CmdResult::Cancel;
            }
            self.gathering = false;
            return CmdResult::NeedPoint;
        }
        // Value step, nothing typed: the no-argument form (optional value).
        CmdResult::Dispatch(format!("{} ", self.name))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TCountStep {
    Start,
    Increment,
    Placement,
}

/// Interactive front-end for TCOUNT.
///
/// After gathering the text selection it asks for:
/// 1. starting number,
/// 2. increment,
/// 3. placement mode (Overwrite / Prefix / Suffix).
///
/// The actual entity modification remains in the inline TCOUNT handler.
pub struct TCountCommand {
    gathering: bool,
    selected: Vec<Handle>,
    step: TCountStep,
    start: i64,
    increment: i64,
}

impl TCountCommand {
    pub fn new(has_selection: bool) -> Self {
        Self {
            gathering: !has_selection,
            selected: Vec::new(),
            step: TCountStep::Start,
            start: 1,
            increment: 1,
        }
    }

    fn dispatch(&self, placement: &str) -> CmdResult {
        CmdResult::Dispatch(format!(
            "TCOUNT {} {} {}",
            self.start, self.increment, placement
        ))
    }
}

impl CadCommand for TCountCommand {
    fn name(&self) -> &'static str {
        "TCOUNT"
    }

    fn prompt(&self) -> String {
        if self.gathering {
            return crate::t!("TCOUNT  select text objects, then press Enter:").into_owned();
        }

        match self.step {
            TCountStep::Start => {
                let start = self.start;
                crate::tf!("TCOUNT  starting number <{start}>:").into_owned()
            }
            TCountStep::Increment => {
                let increment = self.increment;
                crate::tf!("TCOUNT  increment <{increment}>:").into_owned()
            }
            TCountStep::Placement => {
                crate::t!("TCOUNT  placement [Overwrite/Prefix/Suffix] <Overwrite>:").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.gathering || self.step != TCountStep::Placement {
            return Vec::new();
        }

        vec![
            CmdOption::new("Overwrite", "O"),
            CmdOption::new("Prefix", "P"),
            CmdOption::new("Suffix", "S"),
        ]
    }

    fn wants_text_input(&self) -> bool {
        !self.gathering
    }

    fn is_selection_gathering(&self) -> bool {
        self.gathering
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.selected = handles;
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.gathering {
            return None;
        }

        let t = text.trim();

        if t.is_empty() {
            return None;
        }

        match self.step {
            TCountStep::Start => {
                if let Ok(value) = t.parse::<i64>() {
                    self.start = value;
                    self.step = TCountStep::Increment;
                }
                Some(CmdResult::NeedPoint)
            }

            TCountStep::Increment => {
                if let Ok(value) = t.parse::<i64>() {
                    self.increment = value;
                    self.step = TCountStep::Placement;
                }
                Some(CmdResult::NeedPoint)
            }

            TCountStep::Placement => {
                let placement = match t.to_uppercase().as_str() {
                    "O" | "OVERWRITE" => "O",
                    "P" | "PREFIX" => "P",
                    "S" | "SUFFIX" => "S",
                    _ => return Some(CmdResult::NeedPoint),
                };

                Some(self.dispatch(placement))
            }
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.gathering {
            if self.selected.is_empty() {
                return CmdResult::Cancel;
            }

            self.gathering = false;
            return CmdResult::NeedPoint;
        }

        match self.step {
            TCountStep::Start => {
                self.start = 1;
                self.step = TCountStep::Increment;
                CmdResult::NeedPoint
            }

            TCountStep::Increment => {
                self.increment = 1;
                self.step = TCountStep::Placement;
                CmdResult::NeedPoint
            }

            TCountStep::Placement => self.dispatch("O"),
        }
    }
}
/// Generic front-end for a two-value command that operates on the current
/// selection (POLYSOLID width + height on a selected polyline). Gathers a
/// selection first when none is set, prompts for two values, and dispatches
/// `<name> <first> <second>` to the inline handler.
pub struct SelectThenTwoValueCommand {
    name: &'static str,
    prompt1: &'static str,
    prompt2: &'static str,
    gathering: bool,
    selected: Vec<Handle>,
    first: Option<String>,
}

impl SelectThenTwoValueCommand {
    pub fn new(
        name: &'static str,
        prompt1: &'static str,
        prompt2: &'static str,
        has_selection: bool,
    ) -> Self {
        Self {
            name,
            prompt1,
            prompt2,
            gathering: !has_selection,
            selected: Vec::new(),
            first: None,
        }
    }
}

impl CadCommand for SelectThenTwoValueCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        if self.gathering {
            crate::t!(
                "%{name}  select objects, then press Enter:",
                name = self.name
            )
            .into_owned()
        } else if self.first.is_none() {
            crate::t!(self.prompt1).into_owned()
        } else {
            crate::t!(self.prompt2).into_owned()
        }
    }

    fn wants_text_input(&self) -> bool {
        !self.gathering
    }

    fn is_selection_gathering(&self) -> bool {
        self.gathering
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.selected = handles;
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.gathering {
            return None;
        }
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        match &self.first {
            None => {
                self.first = Some(t.to_string());
                // Consumed; `None` would feed the same text back as the second
                // value and dispatch `<name> <x> <x>` in one step.
                Some(CmdResult::NeedPoint)
            }
            Some(first) => Some(CmdResult::Dispatch(format!("{} {first} {t}", self.name))),
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.gathering {
            if self.selected.is_empty() {
                return CmdResult::Cancel;
            }
            self.gathering = false;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }
}

// ── Result token ──────────────────────────────────────────────────────────

/// Returned by every `CadCommand` method to tell main.rs what to do.
#[allow(dead_code)]
pub enum CmdResult {
    /// Command is still waiting for the next point; show updated prompt.
    NeedPoint,
    /// Update the committed-segment wire (normal colour) and keep collecting points.
    InterimWire(WireModel),
    /// Update the in-progress (cyan) preview wire in the viewport.
    Preview(WireModel),
    /// Commit an acadrust entity to the document; keep the command active.
    CommitEntity(EntityType),
    /// Commit several acadrust entities in one undo step; keep the command active.
    CommitEntities(Vec<EntityType>),
    /// Commit an acadrust entity to the document and end the command.
    CommitAndExit(EntityType),
    /// Commit a Model-tab 3D solid: the acadrust entity (for selection /
    /// persistence) plus its B-rep (cached for boolean ops + shaded
    /// rendering). Ends the command.
    CommitSolid {
        entity: EntityType,
        solid: Box<acadrust::kernel::brep::Body>,
    },
    /// Commit an acadrust entity, end the command, and open the in-place text
    /// editor on it (used by MLEADER to type the annotation after placement).
    CommitAndEditText(EntityType),
    /// Commit several entities, end the command, and open the in-place text
    /// editor on the one at `edit_index` (used by LEADER to place the leader
    /// line plus an empty MText annotation, then type into the MText).
    CommitManyAndEditText {
        entities: Vec<EntityType>,
        edit_index: usize,
    },
    /// Create a block definition from existing entities and insert one reference.
    CreateBlock {
        handles: Vec<Handle>,
        name: String,
        base: DVec3,
    },
    /// Apply a transform to selected entities and end the command.
    TransformSelected(Vec<Handle>, EntityTransform),
    /// Copy selected entities with a transform; command stays active for more copies.
    CopySelected(Vec<Handle>, EntityTransform),
    /// Commit a hatch fill (stored in Scene::hatches, not the DXF document).
    CommitHatch(HatchModel),
    /// Copy selected entities with multiple transforms (e.g. rectangular array); end command.
    BatchCopy(Vec<Handle>, Vec<EntityTransform>),
    /// Erase `handle` and replace with new entities; command stays active.
    ReplaceEntity(Handle, Vec<EntityType>),
    /// Replace / delete multiple entities and add new ones; command ends.
    /// Each pair: (handle_to_erase, replacement_entities) — empty vec = delete only.
    ReplaceMany(Vec<(Handle, Vec<EntityType>)>, Vec<EntityType>),
    /// Replace several entities as one undo step while keeping the command active.
    ReplaceManyContinue(Vec<(Handle, Vec<EntityType>)>),
    /// Cancel: discard any preview and end the command.
    Cancel,
    /// Cancel because the active drawing space changed. Cleanup is identical
    /// to `Cancel`, but the host reports the context change explicitly.
    CancelForSpaceChange,
    /// End the selection-gather phase and re-dispatch the named command
    /// with the gathered handles installed as the active scene selection.
    /// Select by a path the user picked point by point. `closed` polygons take
    /// what they enclose, or merely touch when `crossing`; an open fence takes
    /// only what it actually cuts. The host owns the hit test, so the command
    /// hands over the geometry rather than the answer. (#596)
    SelectByPath {
        path: Vec<[f64; 2]>,
        closed: bool,
        crossing: bool,
    },
    Relaunch(String, Vec<Handle>),
    /// End the command and dispatch the given command string. Used by an
    /// interactive front-end that gathered its arguments step-by-step and
    /// delegates execution to an existing inline command handler (e.g. UCS
    /// collecting an option + value, then running `UCS Z 90`). Unlike
    /// `Relaunch` it does not touch the selection.
    Dispatch(String),
    /// Move `dest` entities to the layer of the `src` entity; end command.
    MatchEntityLayer { dest: Vec<Handle>, src: Handle },
    /// Copy all visual properties (layer/color/linetype/lineweight) from `src` to `dest`; end command.
    MatchProperties { dest: Vec<Handle>, src: Handle },
    /// Create a named group from the given entity handles; end command.
    CreateGroup { handles: Vec<Handle>, name: String },
    /// Dissolve all groups that contain any of the given handles; end command.
    DeleteGroups { handles: Vec<Handle> },
    /// Freeze or thaw layers by name in the given viewport; command stays active.
    VpLayerUpdate {
        vp_handle: Handle,
        freeze: Vec<String>,
        thaw: Vec<String>,
    },
    /// Paste clipboard entities translated so their centroid lands at `base_pt`; end command.
    PasteClipboard { base_pt: DVec3 },
    /// Zoom the model-space camera to fit the given corner points; end command.
    ZoomToWindow { p1: DVec3, p2: DVec3 },
    /// Print a measurement result to the command line and end the command.
    Measurement(String),
    /// Print a measurement result and keep the command active.
    ReportMeasurement(String),
    /// Print a measurement result, clear the current selection, and keep the command active.
    ReportMeasurementAndDeselect(String),
    /// Clear the current selection and keep the command active at its updated step.
    DeselectAndContinue,
    /// Break `handle` at points `p1` and `p2`; replace with computed fragments.
    BreakEntity { handle: Handle, p1: DVec3, p2: DVec3 },
    /// Attempt to join the given entities into fewer merged entities.
    JoinEntities(Vec<Handle>),
    /// Apply a polyline-edit operation to one entity; keep command active.
    PeditOp {
        handle: Handle,
        op: crate::modules::draw::modify::pedit::PeditOp,
    },
    /// Place Point entities at N equal intervals along the entity.
    DivideEntity { handle: Handle, n: usize },
    /// Place Point entities at `segment_length` intervals along the entity.
    MeasureEntity { handle: Handle, segment_length: f64 },
    /// Extend/trim a Line or Arc by the given mode; end command.
    LengthenEntity {
        handle: Handle,
        pick_pt: DVec3,
        mode: crate::modules::draw::modify::lengthen::LenMode,
    },
    /// Align selected entities: translate to dst1, rotate by angle_rad, optional scale.
    AlignSelected {
        handles: Vec<Handle>,
        src1: DVec3,
        dst1: DVec3,
        angle_rad: f64,
        scale: f64,
    },
    /// Set the plot window on the active layout's PlotSettings.
    SetPlotWindow { p1: DVec3, p2: DVec3 },
    /// Create a paper-space viewport. `preserve_view` keeps an explicitly
    /// selected/defined view instead of applying the normal model-extents fit.
    MviewCreate {
        viewport: acadrust::entities::Viewport,
        preserve_view: bool,
    },
    /// Create a viewport clipped by either a new polygon boundary or an
    /// existing closed paper-space entity.
    MviewCreateClipped {
        boundary: Option<EntityType>,
        boundary_handle: Handle,
    },
    /// Create a wipeout from an existing closed polyline in the active space.
    WipeoutFromPolyline(Handle),
    /// Temporarily switch between paper and Model while MVIEW defines a new
    /// model-space window, keeping the command active.
    MviewSwitchLayout(String),
    /// Cancel MVIEW's temporary Model-space step and return to its layout.
    MviewCancelToLayout(String),
    /// Quick-print the bounding box of the given selected entities to a PDF.
    QuickPrint(Vec<Handle>),
    /// Replace the text content of a Text/MText entity in-place.
    DdeditEntity { handle: Handle, new_text: String },
    /// Open the in-place editor (plain box or rich MText editor, per type) for
    /// a text-bearing entity picked by a command such as DDEDIT.
    EditTextEntity { handle: Handle },
    /// Open the in-place MText editor (formatting toolbar + multi-line text
    /// area with live viewport preview). `handle` is `Some` when editing an
    /// existing MText, `None` when creating a new one at `pos`.
    OpenMTextEditor {
        pos: DVec3,
        handle: Option<Handle>,
        initial: String,
        height: f64,
    },
    /// Open the in-place single-line TEXT editor (a plain text-entry box, no
    /// formatting toolbar). `handle` is `Some` when editing an existing Text,
    /// `None` when creating a new one at `pos`.
    OpenTextEditor {
        pos: DVec3,
        handle: Option<Handle>,
        initial: String,
        height: f64,
    },
    /// Apply new pattern/scale/angle to an existing hatch entity.
    HatcheditApply {
        handle: Handle,
        name: String,
        scale: f32,
        angle: f32,
    },
    /// Implicit STRETCH selection: the crossing window was drawn with no prior
    /// selection, so the host resolves which entities it touches and restarts
    /// the command at the base-point step with them. (#338)
    StretchWindow { win_min: DVec3, win_max: DVec3 },
    /// Stretch entities: move only vertices/endpoints inside the crossing window.
    StretchEntities {
        handles: Vec<Handle>,
        /// Min corner of the crossing window in world XZ (= DXF XY).
        win_min: DVec3,
        /// Max corner of the crossing window in world XZ (= DXF XY).
        win_max: DVec3,
        /// Translation vector to apply to vertices inside the window.
        delta: DVec3,
    },
    /// Create a Solid3D placeholder entity + associated MeshModel.
    /// `mesh_fn` is called with the entity's handle string to build the mesh.
    CommitSolid3D {
        mesh_fn: Box<dyn FnOnce(String) -> Option<crate::scene::model::mesh_model::MeshModel> + Send>,
    },
    /// Extrude the profile entity `handle` by `height` along Z.
    ExtrudeEntity {
        handle: Handle,
        height: f64,
        color: [f32; 4],
    },
    /// Revolve the profile entity `handle` around the given axis by `angle_deg`.
    RevolveEntity {
        handle: Handle,
        axis_start: glam::DVec3,
        axis_end: glam::DVec3,
        angle_deg: f32,
        color: [f32; 4],
    },
    /// Sweep the profile entity `profile_handle` along `path_handle`.
    SweepEntity {
        profile_handle: Handle,
        path_handle: Handle,
        color: [f32; 4],
    },
    /// Loft through a series of profile entities.
    LoftEntities {
        handles: Vec<Handle>,
        color: [f32; 4],
    },
    /// INSERT landed on a block that has AttributeDefinitions.
    /// The host should look up the attdefs for `block_name` from the document
    /// and call `attreq_set_attdefs()` on the command, then loop on text input.
    AttreqNeeded { block_name: String },
    /// Add a command-owned "live" entity to the document mid-command and hand
    /// its assigned handle back to the active command via `set_live_handle()`.
    /// One undo snapshot is pushed here, so the whole in-progress object reverts
    /// as a single unit. The command stays active. Used by PLINE so the partial
    /// polyline is a real, snappable entity while later vertices are placed.
    CommitLiveEntity(EntityType),
    /// Replace the geometry of the live entity `handle` in place — preserving
    /// its layer — without pushing a new undo snapshot. When `finish` is true
    /// the command also exits (the entity is already committed, so no separate
    /// commit is needed).
    UpdateLiveEntity {
        handle: Handle,
        entity: EntityType,
        finish: bool,
    },
    /// End a command-owned live entity without replacing its already-current
    /// document geometry. PLINE uses this for Enter/Escape after the latest
    /// vertex was published, avoiding one redundant geometry epoch/GPU patch.
    FinalizeLiveEntity(Handle),
    /// Remove the live entity from the document but keep the command running —
    /// PLINE's Undo popping back below the two vertices an entity needs.
    RemoveLiveEntity(Handle),
    /// Suspends command execution, moves it to suspended_cmd, and opens the text editor for the given handle.
    SuspendForTextEdit { handle: Handle },
    /// Requests a standard document-level undo while keeping the command active.
    UndoDocument,
    /// Sets the TEXTEDITMODE system variable and ends the command.
    SetTexteditMode(bool),
}

/// What kind of value the active command is currently asking for. Drives
/// the dynamic-input overlay so the tooltip shows the relevant quantity
/// (coordinates for a point pick, a single length for a radius/distance
/// prompt, degrees for an angle prompt).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DynField {
    /// A position — X/Y coordinates (or distance+angle relative to the
    /// last point). The default for every command step.
    #[default]
    Point,
    /// A single linear distance (radius, length, offset) measured from
    /// the last point.
    Distance,
    /// An angle, shown in degrees, measured from the last point.
    Angle,
    /// A typed scalar with no geometric meaning at the cursor — a count
    /// (number of sides / segments) or any value the command reads purely
    /// from the keyboard. Shown as a single typed box.
    Scalar,
}

// ── Per-step dynamic-input specification ───────────────────────────────────
//
// `DynField` only says "this step wants a point / distance / angle". `DynSpec`
// lets a command describe its step precisely: which value boxes to show (with
// roles + labels), what guide geometry to draw, and where it is measured from.
// A command returns `Some(DynSpec)` from `dyn_spec()` to take explicit control;
// returning `None` (the default) keeps the legacy `dyn_field()` behaviour.

/// Semantic role of a dynamic-input box. Resolution maps each role to a base
/// ordinate/distance/angle; the role additionally drives the label and any
/// value scaling (e.g. a diameter shows/accepts twice the geometric radius).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DynRole {
    X,
    Y,
    Z,
    /// Linear distance from the anchor.
    Distance,
    /// Angle from the anchor, degrees.
    Angle,
    /// Distance shown labelled `R` (circle/arc radius).
    Radius,
    /// Distance shown labelled `⌀`; displayed/typed value is twice the radius.
    Diameter,
    /// Cartesian X-delta shown labelled `W` (rectangle width).
    Width,
    /// Cartesian Y-delta shown labelled `H` (rectangle height).
    Height,
    /// Typed-only scale factor.
    Factor,
    /// Typed-only integer count. Reserved for upcoming command migrations.
    #[allow(dead_code)]
    Count,
}

/// Shared rule for how an angle reads in the dynamic-input box: the unsigned
/// magnitude of the short signed angle, so a clockwise angle (cursor below the
/// reference axis) shows as a positive value rather than a negative or a
/// CCW 300-something. Callers keep the *signed* radian for the actual
/// computation/commit; this is display-only. `signed_rad` is the angle from
/// the reference to the cursor.
pub fn dyn_display_angle_deg(signed_rad: f32) -> f32 {
    let mut a = signed_rad % std::f32::consts::TAU;
    if a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    if a <= -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a.to_degrees().abs()
}

impl DynRole {
    /// Default label shown before the value (empty = value only).
    pub fn label(self) -> &'static str {
        match self {
            DynRole::X => "X",
            DynRole::Y => "Y",
            DynRole::Z => "Z",
            DynRole::Distance | DynRole::Angle | DynRole::Factor => "",
            DynRole::Radius => "R",
            DynRole::Diameter => "\u{2300}",
            DynRole::Width => "W",
            DynRole::Height => "H",
            DynRole::Count => "#",
        }
    }

    /// Multiplier between the geometric value and the displayed/typed value.
    /// A diameter box shows and accepts twice the underlying radius.
    pub fn value_scale(self) -> f32 {
        match self {
            DynRole::Diameter => 2.0,
            _ => 1.0,
        }
    }
}

/// Guide geometry the overlay draws for a step, anchored at the step's base.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DynGuide {
    /// No guide lines.
    None,
    /// +X reference line and the angle arc (polar point entry).
    Polar,
    /// Dotted projections from the cursor down to the anchor's X and Y axes.
    AxisDelta,
    /// A line from the anchor to the cursor (radius / single distance).
    Radius,
    /// The two rectangle sides (width × height) from the anchor corner.
    RectSides,
    /// A line from the anchor, perpendicular to the reference line (anchor →
    /// `DynSpec::ref_point`), reaching the cursor's perpendicular offset — the
    /// measured semi-axis (ellipse minor). The value is that offset.
    Perp,
    /// Like `Perp` but drawn as a dimension: the measured segment is offset off
    /// the edge with extension lines back to its endpoints (rectangle height).
    PerpDim,
}

/// Where a step's values are measured from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DynAnchor {
    /// The previous committed point (`App::last_point`). Reserved — current
    /// specs pass the anchor explicitly via `Point`.
    #[allow(dead_code)]
    LastPoint,
    /// An explicit world point.
    Point(DVec3),
}

/// One value box in a [`DynSpec`].
#[derive(Clone, Debug)]
pub struct DynFieldSpec {
    pub role: DynRole,
    /// Label override; `None` uses the role's default label.
    #[allow(dead_code)] // dyn-spec framework field; not yet consumed
    pub label: Option<&'static str>,
}

impl DynFieldSpec {
    pub fn new(role: DynRole) -> Self {
        Self { role, label: None }
    }
}

/// A full per-step dynamic-input description.
#[derive(Clone, Debug)]
pub struct DynSpec {
    pub anchor: DynAnchor,
    pub fields: Vec<DynFieldSpec>,
    pub guide: DynGuide,
    /// Far end of a reference line through `anchor` (only used by
    /// [`DynGuide::Perp`]); `None` otherwise.
    pub ref_point: Option<DVec3>,
}

// ── Trait ─────────────────────────────────────────────────────────────────

/// An interactive CAD command that collects user input step-by-step.
/// One clickable option a command step offers. Rendered as a button next to
/// the command-line prompt; clicking it feeds `keyword` to the running command
/// exactly as if the user had typed it (routed through `on_text_input`). An
/// empty `keyword` submits the step like pressing Enter (the finish / default
/// action). Lets every bracketed `[A=arc L=line …]` prompt become buttons so
/// the option need not be typed. (#304)
#[derive(Clone, Debug)]
pub struct CmdOption {
    /// Text shown on the button, e.g. `"3P"` or `"Close"`.
    pub label: String,
    /// Token fed to the command when clicked, e.g. `"3P"`. Empty = Enter.
    pub keyword: String,
}

impl CmdOption {
    /// Button whose keyword is typed on click, e.g. `("Ttr", "TTR")`.
    pub fn new(label: &str, keyword: &str) -> Self {
        Self {
            label: crate::t!(label).into_owned(),
            keyword: keyword.to_string(),
        }
    }
    /// A "finish" button that submits the step like Enter.
    pub fn enter(label: &str) -> Self {
        Self {
            label: crate::t!(label).into_owned(),
            keyword: String::new(),
        }
    }
}

pub trait CadCommand: Send {
    /// Short name shown in the command line prompt, e.g. `"LINE"`.
    #[allow(dead_code)]
    fn name(&self) -> &'static str;
    /// Current prompt string to display in the command line.
    fn prompt(&self) -> String;

    /// Clickable keyword options for the current step, rendered as buttons in
    /// the command line next to the prompt. Default: none. A command returns
    /// different options per step; clicking a button feeds its keyword through
    /// the same path as typed command-line text (`on_text_input`). (#304)
    fn options(&self) -> Vec<CmdOption> {
        Vec::new()
    }

    /// Push the active coordinate frame in full precision. Geometry commands
    /// use it for plane-local construction; inquiry and modify commands use it
    /// for local deltas, angles and transformation axes.
    fn set_working_plane(&mut self, _plane: WorkingPlane) {}

    /// Push the live Ctrl-key state into the command before each preview/commit
    /// dispatch. Commands that offer a Ctrl toggle (e.g. arc-direction flip on
    /// `ARC_CONT`) store it; most commands ignore it. Default no-op.
    fn set_ctrl(&mut self, _ctrl: bool) {}

    /// Push the live Shift-key state into the command before each dispatch.
    /// TRIM/EXTEND use it for the shift-select swap (Shift+click extends
    /// during TRIM and trims during EXTEND, #336). Default no-op.
    fn set_shift(&mut self, _shift: bool) {}

    /// Mid-command Ctrl+Z: a multi-point drawing command can take the undo
    /// itself (PLINE pops its last vertex) instead of the document undo
    /// swallowing the whole in-progress object. `None` (default) lets the
    /// normal document undo run.
    fn on_undo_step(&mut self) -> Option<CmdResult> {
        None
    }

    /// Called when the user left-clicks in the viewport (point pick).
    fn on_point(&mut self, pt: DVec3) -> CmdResult;

    /// Called when the user presses Enter (finalize / next option).
    fn on_enter(&mut self) -> CmdResult;

    /// Whether a bare Enter should supply the drawing's continuation point as
    /// this command's first point instead of calling [`Self::on_enter`]. Draw
    /// commands opt in only while their first point is still unset; later
    /// Enter presses retain their normal finish/cancel meaning.
    fn enter_accepts_default_start(&self) -> bool {
        false
    }

    /// Called when the user presses Escape (cancel).
    #[allow(dead_code)]
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    /// Abort because the active drawing coordinate space is about to change.
    /// Unlike `on_escape`, the default never interprets cancellation as
    /// "finish with the points collected so far" (SPLINE does that for a
    /// deliberate Escape). Commands owning a live document entity can
    /// override this to close that entity's deferred history safely.
    fn on_space_change(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    /// Returns `true` when the command needs entity picking (hit-test) instead of point picking.
    fn needs_entity_pick(&self) -> bool {
        false
    }

    /// Include filled hatch / DXF SOLID regions in the entity hit-test.
    ///
    /// Most entity-pick commands operate on curve geometry and intentionally
    /// keep the cheaper wire-only path. Commands that accept fill entities
    /// override this so clicking inside a fill resolves its entity handle.
    fn entity_pick_includes_fills(&self) -> bool {
        false
    }

    /// Render the entity under the cursor through the normal rollover
    /// highlight while this command is waiting for an entity pick.
    fn entity_pick_highlights_hover(&self) -> bool {
        false
    }

    /// Called when the text editor closes, either because the user committed or cancelled the edit.
    fn on_editor_closed(&mut self, _committed: bool) -> CmdResult {
        CmdResult::Cancel
    }

    /// Called when the user clicks and `needs_entity_pick()` is true.
    /// `handle` is the nearest wire's entity handle (Handle::NULL if nothing found).
    fn on_entity_pick(&mut self, _handle: Handle, _pt: DVec3) -> CmdResult {
        CmdResult::Cancel
    }

    /// Host callback after `CmdResult::CommitLiveEntity`: records the handle the
    /// new live entity was assigned so later `UpdateLiveEntity` results can
    /// target it.
    fn set_live_handle(&mut self, _handle: Handle) {}

    /// Point-click pick of domain objects (wire hit-test often misses small markers).
    fn needs_structure_point_pick(&self) -> bool {
        false
    }

    /// Resolve a domain object near `(x, y)` while `needs_structure_point_pick()` is active.
    fn resolve_object_pick(&self, _scene: &Scene, _x: f64, _y: f64) -> Option<ObjectPickHit> {
        None
    }

    /// Preview wires while hovering during object-point pick.
    fn object_pick_hover_previews(&self, _scene: &Scene, _cursor: DVec3) -> Vec<WireModel> {
        vec![]
    }

    /// Message when `resolve_object_pick` returns none on click.
    fn object_pick_miss_message(&self) -> &'static str {
        "No object near click."
    }

    /// Called when `needs_structure_point_pick()` is true and a structure is found near the click.
    fn on_structure_pick(&mut self, _handle: Handle, _pt: DVec3) -> CmdResult {
        CmdResult::Cancel
    }

    /// Extra acquisition previews during entity pick (besides `on_hover_entity`).
    fn entity_pick_acquire_previews(&self, _scene: &Scene, _handle: Handle) -> Vec<WireModel> {
        vec![]
    }

    /// Acquisition hint label during entity pick hover.
    fn entity_pick_acquire_hint(&self, _handle: Handle) -> Option<&'static str> {
        None
    }

    /// Hover label for object acquisition (e.g. "Inlet" under cursor).
    fn set_acquisition_hint(&mut self, _hint: Option<&str>) {}

    /// Called after `CmdResult::ReplaceEntity` is applied to the document.
    /// `old` is the erased handle; `new_handles` are the handles assigned to the replacement entities.
    /// Commands that stay active across replaces should update their internal snapshots here.
    fn on_entity_replaced(&mut self, _old: Handle, _new_handles: &[Handle]) {}

    /// Consume a lasso or drag-box gesture while the command is active.
    /// `fence` is the gesture boundary in drawing coordinates; `window` is
    /// present for rectangular gestures. Returning `Some` prevents the normal
    /// selection system from selecting the crossed entities.
    fn on_drag_selection(
        &mut self,
        _fence: &[[f64; 2]],
        _window: Option<([f64; 2], [f64; 2])>,
    ) -> Option<CmdResult> {
        None
    }

    /// Whether an empty click may start a two-corner selection box for this
    /// command instead of being reported as a missed entity pick.
    fn accepts_drag_selection(&self) -> bool {
        false
    }

    /// Called on every mouse-move when `needs_entity_pick()` is true.
    /// Return preview wires showing the operation result under the cursor.
    /// Default: empty (no preview).
    fn on_hover_entity(&mut self, _handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }

    /// Called on every mouse-move in the viewport.
    /// Return `Some(WireModel)` to update the rubber-band preview, `None` to skip.
    fn on_mouse_move(&mut self, _pt: DVec3) -> Option<WireModel> {
        None
    }

    /// Called on every mouse-move; return all preview wires to show (object ghosts + rubber-band).
    /// Default: forwards to `on_mouse_move` for backwards compatibility.
    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        self.on_mouse_move(pt).into_iter().collect()
    }

    /// Source entities replaced by the current live preview. The host removes
    /// these from the resident render until the command commits or cancels.
    /// Commands such as COPY keep their sources visible and use the default.
    fn preview_hidden_handles(&self) -> &[Handle] {
        &[]
    }

    /// Returns `true` when the command is waiting for text typed in the command line.
    fn wants_text_input(&self) -> bool {
        false
    }

    /// Returns `true` when the current step is a point pick that *also* accepts
    /// optional keyword letters (e.g. PLINE's A/L/C/U). Such a step keeps the
    /// polar dynamic-input boxes: typed digits become coordinates while letters
    /// still reach the command line as keywords. Without this, a command that
    /// returns `wants_text_input() == true` for its keywords would suppress the
    /// dynamic-input distance/angle entirely. Default `false`.
    fn point_step_accepts_keywords(&self) -> bool {
        false
    }

    /// Returns `true` when the active text prompt expects free-form prose
    /// that can legitimately contain whitespace (the body of a TEXT /
    /// MTEXT / DDEDIT entity, an attribute default value, etc.). For
    /// these prompts the command-line input must let `Space` be typed as
    /// a literal character; for every other prompt `Space` submits the
    /// input the same way `Enter` does.
    ///
    /// Default `false` — single-token prompts (option letters, numeric
    /// radius, block name) do not embed spaces.
    fn wants_text_with_spaces(&self) -> bool {
        false
    }

    /// Called when the user submits text via the command line while `wants_text_input` is true.
    fn on_text_input(&mut self, _text: &str) -> Option<CmdResult> {
        None
    }

    /// Returns `true` when the command is in a selection-gathering phase.
    /// While true, viewport clicks are routed through the normal selection
    /// system (single / box / polygon) instead of the command's point-pick path.
    /// After each completed selection action the host calls `on_selection_complete`.
    fn is_selection_gathering(&self) -> bool {
        false
    }

    fn selection_forces_add(&self) -> bool {
        false
    }

    /// Called after a selection action completes while `is_selection_gathering` is true.
    /// `handles` is the full set of currently selected entities.
    /// Return `Relaunch` to fire the pending command, or `NeedPoint` to keep gathering.
    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        CmdResult::Cancel
    }

    fn inject_selection_entities(&mut self, _entities: Vec<SelectionEntity>) {}

    fn area_preview_regions(&self) -> Option<Vec<AreaPreviewRegion>> {
        None
    }

    /// Returns `true` when the current step picks a corner of a selection
    /// *window* by point (e.g. STRETCH's crossing window). Such a pick must be a
    /// free point: applying the Ortho/Polar lock would pin the opposite corner to
    /// an axis through the first corner, collapsing the rectangle to a line and
    /// making the window unusable. The host skips the ortho/polar constraint for
    /// these steps. Default `false`. (#291)
    fn window_corner_pick(&self) -> bool {
        false
    }

    /// The already-picked first corner of the selection window (world space)
    /// while `window_corner_pick()` is true and the opposite corner is being
    /// dragged. The host projects it and draws a filled selection marquee to the
    /// cursor, so a point-picked window (STRETCH) reads like a normal box
    /// selection. `None` before the first corner is set. (#291)
    fn window_first_corner(&self) -> Option<glam::DVec3> {
        None
    }

    /// Returns `true` when the command wants object picks via Tangent snap.
    fn needs_tangent_pick(&self) -> bool {
        false
    }

    /// If this command is XATTACH, returns the file path to attach.
    /// Default: None.
    fn xattach_path(&self) -> Option<String> {
        None
    }

    /// The block reference an ATTEDIT pick has resolved to, awaiting the
    /// attribute editor dialog; else None.
    fn attedit_pending_handle(&self) -> Option<acadrust::Handle> {
        None
    }

    /// Inject the block's attribute definitions for ATTREQ attr-filling after
    /// the INSERT point is picked. Carries the full definitions so the created
    /// attributes inherit their geometry, not just tag / prompt / default (#255).
    fn attreq_set_attdefs(&mut self, _attdefs: Vec<acadrust::entities::AttributeDefinition>) {}

    /// Returns the INSERT entity built so far (pending attr fill) if this is an
    /// ATTREQ-aware INSERT command waiting for attdef injection.
    /// Called by the host after `AttreqNeeded` to commit the completed Insert.
    fn attreq_take_insert(&mut self) -> Option<acadrust::EntityType> {
        None
    }

    /// Called instead of `on_point` when the command needs a tangent pick
    /// and the snap system found a tangent object.
    fn on_tangent_point(&mut self, obj: TangentObject, hit: DVec3) -> CmdResult {
        let _ = obj;
        self.on_point(hit)
    }

    /// Commit a picked point that may carry a running Tangent-snap reference
    /// (the object under the cursor when Tangent won). The default ignores the
    /// tangent, so only commands that opt in see it — LINE uses it to resolve a
    /// deferred tangent-to-tangent line between two circles, which needs both
    /// objects and so can't be computed at a single pick. Returning `None`
    /// means "not handled — fall back to `on_point`". (#274)
    fn on_point_with_tangent(
        &mut self,
        _pt: DVec3,
        _tangent: Option<TangentObject>,
    ) -> Option<CmdResult> {
        None
    }

    /// The command's current anchor (rubber-band origin) after a commit, so the
    /// host can sync `last_point` when the command replaced the picked
    /// coordinate — e.g. LINE resolving a deferred tangent to the true tangent
    /// point. `None` keeps the picked point. Only consulted after
    /// `on_point_with_tangent` handled the pick. (#274)
    fn resolved_anchor(&self) -> Option<DVec3> {
        None
    }

    /// When true, `update.rs` injects the picked entity before calling
    /// `on_entity_pick` (required when the pick handler reads injected state).
    fn inject_before_entity_pick(&self) -> bool {
        false
    }

    /// Called by update.rs to inject the cloned entity into commands
    /// that need to read/modify it (e.g. DIMTEDIT, MLEADERADD, MLEADERREMOVE).
    /// Default: no-op.
    fn inject_picked_entity(&mut self, _entity: acadrust::EntityType) {}

    /// Supply the tessellated surface area associated with the picked entity.
    /// Commands that measure mesh-backed objects can opt in without owning the
    /// scene's render cache.
    fn inject_picked_surface_area(&mut self, _area: f64) {}

    /// What the command is asking for at this step, used to label the
    /// dynamic-input overlay. Default is a point pick; commands waiting
    /// on a radius/length return `Distance` and angle prompts return
    /// `Angle`.
    fn dyn_field(&self) -> DynField {
        DynField::Point
    }

    /// When true, a value typed into the dynamic-input box for this step is
    /// committed via `on_text_input` (as a string the command parses) rather
    /// than resolved into a point. Used by steps whose typed value is a span /
    /// included angle / length the command interprets itself (e.g. ARC angle
    /// modes), while the box still previews a live value from the cursor.
    fn dyn_commit_as_text(&self) -> bool {
        false
    }

    /// Explicit per-step dynamic-input description. `Some(spec)` takes full
    /// control of the boxes, guide geometry and anchor for this step; `None`
    /// (the default) falls back to the legacy `dyn_field()` behaviour so
    /// commands that haven't migrated keep working unchanged.
    fn dyn_spec(&self) -> Option<DynSpec> {
        None
    }

    /// Live value for the dynamic-input scalar box, derived from the cursor
    /// world position. Lets a command drive a typed prompt by mouse — e.g.
    /// OFFSET returns the perpendicular distance from the cursor to the
    /// object being offset, so moving the cursor fills in the distance.
    /// Returns `None` when the value can only be typed (a count, or a
    /// distance with no reference yet). The string the host commits is this
    /// value formatted; the command's own `on_text_input` parses it back.
    fn dyn_live_value(&self, _cursor: DVec3) -> Option<f64> {
        None
    }
}

// ── Autocomplete registry ─────────────────────────────────────────────────
//
// Every `impl CadCommand for Foo` module submits the names it answers to
// at compile time via `inventory::submit!`. The command-line autocomplete
// then iterates the resulting collection at runtime — no central list to
// keep in sync.
//
// Non-interactive one-shot dispatch arms (NEW, OPEN, SAVE, …) live in
// `app/commands.rs` and don't have a `CadCommand` impl; they're absent
// from autocomplete by design. Add an explicit `inventory::submit!` next
// to their dispatch arm if you want them surfaced.

pub struct CommandRegistration {
    pub names: &'static [&'static str],
}

inventory::collect!(CommandRegistration);

/// All registered command names, including aliases.
pub fn all_registered_command_names() -> Vec<&'static str> {
    inventory::iter::<CommandRegistration>
        .into_iter()
        .flat_map(|r| r.names.iter().copied())
        .collect()
}
