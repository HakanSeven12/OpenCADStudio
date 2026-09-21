// Mirror tool — ribbon definition + interactive command.
//
// Command:  MIRROR (MI)
//   Requires at least one entity selected.
//   Step 1: pick first mirror-line point
//   Step 2: pick second mirror-line point
//   Step 3: "Erase source objects? [Yes/No] <No>"
//           No  → keep the original, add a mirrored copy
//           Yes → flip the original in place (no copy kept)

use acadrust::Handle;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, EntityTransform, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "MIRROR",
        label: "Mirror",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/mirror.svg")),
        event: ModuleEvent::Command("MIRROR".to_string()),
    }
}

enum Step {
    P1,
    P2(DVec3),
    /// Both mirror-line points fixed; waiting on the erase-source answer.
    AskErase { p1: DVec3, p2: DVec3 },
}

pub struct MirrorCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    /// Text ghosts paired with their bounding-box centre — mirrored per MIRRTEXT
    /// so the preview matches the commit (true glyph mirror on / symmetric
    /// right-reading off).
    text_ghosts: Vec<(WireModel, DVec3)>,
    mirror_text: bool,
    step: Step,
    plane: WorkingPlane,
}

impl MirrorCommand {
    pub fn new(
        handles: Vec<Handle>,
        wire_models: Vec<WireModel>,
        text_ghosts: Vec<(WireModel, DVec3)>,
        mirror_text: bool,
    ) -> Self {
        Self {
            handles,
            wire_models,
            text_ghosts,
            mirror_text,
            step: Step::P1,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for MirrorCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "MIRROR"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::P1 => t!(
                "MIRROR  Specify first mirror-line point  [%{count} objects]:",
                count = self.handles.len()
            )
            .into_owned(),
            Step::P2(p1) => {
                let px = format!("{:.2}", p1.x);
                let py = format!("{:.2}", p1.y);
                t!(
                    "MIRROR  Specify second point  [p1=%{px},%{py}]:",
                    px = px,
                    py = py
                )
                .into_owned()
            }
            Step::AskErase { .. } => {
                t!("MIRROR  Erase source objects? [Yes/No] <No>:").into_owned()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            Step::P1 => {
                self.step = Step::P2(pt);
                CmdResult::NeedPoint
            }
            Step::P2(p1) => {
                self.step = Step::AskErase { p1: *p1, p2: pt };
                CmdResult::NeedPoint
            }
            // Second point is fixed; further clicks ignored until the
            // erase-source question is answered via the command line.
            Step::AskErase { .. } => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        // Enter at the erase prompt accepts the default (No → keep source).
        match &self.step {
            Step::AskErase { p1, p2 } => self.finish(*p1, *p2, false),
            _ => CmdResult::Cancel,
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, Step::AskErase { .. })
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if matches!(self.step, Step::AskErase { .. }) {
            vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")]
        } else {
            Vec::new()
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let Step::AskErase { p1, p2 } = &self.step else {
            return None;
        };
        let (p1, p2) = (*p1, *p2);
        let t = text.trim().to_ascii_lowercase();
        let erase = match t.as_str() {
            "y" | "yes" => true,
            // Empty input (bare Enter) or an explicit No keeps the source.
            "" | "n" | "no" => false,
            // Unrecognised input: re-ask without committing.
            _ => return Some(CmdResult::NeedPoint),
        };
        Some(self.finish(p1, p2, erase))
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        // While picking the second point the ghost tracks the cursor; once it
        // is fixed (erase prompt) the ghost freezes at the chosen axis.
        let (p1, p2) = match &self.step {
            Step::P2(p1) => (*p1, pt),
            Step::AskErase { p1, p2 } => (*p1, *p2),
            _ => return vec![],
        };
        // Mirrored ghosts of all non-text objects (full geometric reflection).
        let mut out: Vec<WireModel> = self
            .wire_models
            .iter()
            .map(|w| {
                w.mirrored_in_plane(
                    p1.as_vec3(),
                    p2.as_vec3(),
                    self.plane.z.as_vec3(),
                )
            })
            .collect();
        // Text ghosts honour MIRRTEXT: on → true glyph mirror (full reflection);
        // off → keep glyphs readable, relocate to the mirror-symmetric position
        // by reflecting the box centre and translating.
        for (w, center) in &self.text_ghosts {
            if self.mirror_text {
                out.push(w.mirrored_in_plane(
                    p1.as_vec3(),
                    p2.as_vec3(),
                    self.plane.z.as_vec3(),
                ));
            } else {
                let reflected = crate::scene::view::transform::reflected_point(
                    *center,
                    p1,
                    p2,
                    self.plane.z,
                );
                let delta = (reflected - *center).as_vec3();
                out.push(w.translated(delta));
            }
        }
        // Mirror-axis line (rubber-band).
        out.push(WireModel::solid(
            "rubber_band".into(),
            vec![
                [p1.x as f32, p1.y as f32, p1.z as f32],
                [p2.x as f32, p2.y as f32, p2.z as f32],
            ],
            WireModel::CYAN,
            false,
        ));
        out
    }
}

impl MirrorCommand {
    /// Commit the mirror. `erase` true flips the originals in place; false
    /// keeps them and adds a mirrored copy. Either way the command ends.
    fn finish(&self, p1: DVec3, p2: DVec3, erase: bool) -> CmdResult {
        let xform = EntityTransform::Mirror {
            p1,
            p2,
            working_normal: self.plane.z,
        };
        if erase {
            CmdResult::TransformSelected(self.handles.clone(), xform)
        } else {
            CmdResult::BatchCopy(self.handles.clone(), vec![xform])
        }
    }
}
