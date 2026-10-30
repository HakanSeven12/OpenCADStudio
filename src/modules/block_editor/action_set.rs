//! Edit native action selections without replacing their graph nodes.
use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, PointPickContext};
use crate::scene::block_actions as actions;
use codec::objects::ObjectType;
use codec::{Document, Handle};
use glam::DVec3;

inventory::submit!(crate::command::CommandRegistration {
    names: &["BACTIONSET"]
});

struct Candidate {
    handle: Handle,
    name: String,
    at: DVec3,
    targets: Vec<Handle>,
    stretch: bool,
    polar: bool,
    expected: ObjectType,
}
#[derive(Clone, Copy)]
enum Mode {
    New,
    Add,
    Remove,
    Frame,
    RotateOnly,
}
pub(crate) struct ActionSetCommand {
    document: u64,
    definition: Handle,
    candidates: Vec<Candidate>,
    chosen: Option<usize>,
    mode: Option<Mode>,
    picked: Vec<Handle>,
    frame: Vec<DVec3>,
    polygon_frame: bool,
    context: Option<PointPickContext>,
}
impl ActionSetCommand {
    pub(crate) fn new(
        document: u64,
        definition: Handle,
        doc: &Document,
        selected: Option<Handle>,
    ) -> Self {
        let candidates = actions::actions(doc, definition)
            .into_iter()
            .map(|(handle, a, kind, _)| Candidate {
                handle,
                name: a.element.name.clone(),
                at: actions::v(a.display_location),
                // Native grip links are retained by retargeting, while this
                // command edits the geometric selection only.
                targets: a.dependencies.iter().copied().filter(|handle| doc.get_entity(*handle).is_some()).collect(),
                stretch: actions::needs_frame(kind),
                polar: kind == actions::Kind::PolarStretch,
                expected: doc.objects[&handle].clone(),
            })
            .collect::<Vec<_>>();
        let chosen = candidates.iter().position(|c| Some(c.handle) == selected);
        Self {
            document,
            definition,
            candidates,
            chosen,
            mode: None,
            picked: vec![],
            frame: vec![],
            polygon_frame: false,
            context: None,
        }
    }
    fn finish_polygon(&self) -> CmdResult {
        if self.frame.len() < 3 {
            return CmdResult::ReportError(
                crate::t!("Specify at least three polygon vertices.").into_owned(),
            );
        }
        match actions::validate_frame_points(&self.frame) {
            Ok(()) => self.finish(),
            Err(error) => CmdResult::ReportError(crate::t!(error).into_owned()),
        }
    }
    fn finish(&self) -> CmdResult {
        let c = &self.candidates[self.chosen.unwrap()];
        let targets = match self.mode.unwrap() {
            Mode::New => self.picked.clone(),
            Mode::Add => c.targets.iter().chain(&self.picked).copied().collect(),
            Mode::Remove => c
                .targets
                .iter()
                .copied()
                .filter(|h| !self.picked.contains(h))
                .collect(),
            Mode::Frame | Mode::RotateOnly => c.targets.clone(),
        };
        CmdResult::EditBlockActionSet {
            document: self.document,
            definition: self.definition,
            action: c.handle,
            expected: Box::new(c.expected.clone()),
            targets,
            frame: matches!(self.mode, Some(Mode::Frame)).then(|| self.frame.clone()),
            rotate_only: matches!(self.mode, Some(Mode::RotateOnly)).then(|| self.picked.clone()),
        }
    }
}
impl CadCommand for ActionSetCommand {
    fn name(&self) -> &'static str {
        "BACTIONSET"
    }
    fn input_kind(&self) -> InputKind {
        if self.chosen.is_none() {
            InputKind::FreeText
        } else if self.mode.is_none() {
            InputKind::SingleToken
        } else {
            InputKind::Point
        }
    }
    fn prompt(&self) -> String {
        crate::t!(if self.chosen.is_none() {
            "BACTIONSET  Pick an action marker or enter its name:"
        } else if self.mode.is_none() && self.candidates[self.chosen.unwrap()].polar {
            "BACTIONSET  [New/Add/Remove/Frame/RotateOnly] <New>:"
        } else if matches!(self.mode, Some(Mode::RotateOnly)) {
            "BACTIONSET  Select objects from this action to rotate only, then press Enter <None>:"
        } else if self.mode.is_none() && self.candidates[self.chosen.unwrap()].stretch {
            "BACTIONSET  [New/Add/Remove/Frame] <New>:"
        } else if self.mode.is_none() {
            "BACTIONSET  [New/Add/Remove] <New>:"
        } else if matches!(self.mode, Some(Mode::Frame)) && self.polygon_frame {
            "BACTIONSET  Specify polygon vertex or [Undo/Close]; press Enter to close:"
        } else if matches!(self.mode, Some(Mode::Frame)) && self.frame.is_empty() {
            "BACTIONSET  Specify first corner of replacement action frame or [CPolygon]:"
        } else if matches!(self.mode, Some(Mode::Frame)) {
            "BACTIONSET  Specify opposite corner of replacement action frame:"
        } else {
            "BACTIONSET  Select objects, then press Enter:"
        })
        .into_owned()
    }
    fn options(&self) -> Vec<CmdOption> {
        if self.chosen.is_some() && self.mode.is_none() {
            let mut options = vec![
                CmdOption::new("New", "New"),
                CmdOption::new("Add", "Add"),
                CmdOption::new("Remove", "Remove"),
            ];
            if self.candidates[self.chosen.unwrap()].stretch {
                options.push(CmdOption::new("Frame", "Frame"));
            }
            if self.candidates[self.chosen.unwrap()].polar {
                options.push(CmdOption::new("RotateOnly", "RotateOnly"));
            }
            options
        } else if matches!(self.mode, Some(Mode::Frame)) {
            if self.polygon_frame {
                vec![
                    CmdOption::new("Undo", "Undo"),
                    CmdOption::new("Close", "Close"),
                ]
            } else if self.frame.is_empty() {
                vec![CmdOption::new("CPolygon", "CPolygon")]
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if matches!(self.mode, Some(Mode::Frame)) {
            match text.trim().to_ascii_uppercase().as_str() {
                "CPOLYGON" | "CP" if self.frame.is_empty() => {
                    self.polygon_frame = true;
                    return Some(CmdResult::NeedPoint);
                }
                "UNDO" | "U" if self.polygon_frame => {
                    self.frame.pop();
                    return Some(CmdResult::NeedPoint);
                }
                "CLOSE" | "C" if self.polygon_frame => return Some(self.finish_polygon()),
                _ => {}
            }
        }

        if self.chosen.is_none() {
            let matches = self
                .candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| c.name.eq_ignore_ascii_case(text))
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            return Some(if matches.len() == 1 {
                self.chosen = Some(matches[0]);
                CmdResult::NeedPoint
            } else {
                CmdResult::ReportError(
                    crate::t!("Enter a unique action name or pick its marker.").into_owned(),
                )
            });
        }
        if self.mode.is_none() {
            let mode = match text.to_ascii_uppercase().as_str() {
                "NEW" | "N" => Mode::New,
                "ADD" | "A" => Mode::Add,
                "REMOVE" | "R" => Mode::Remove,
                "FRAME" | "F" if self.candidates[self.chosen.unwrap()].stretch => Mode::Frame,
                "ROTATEONLY" | "RO" if self.candidates[self.chosen.unwrap()].polar => {
                    Mode::RotateOnly
                }
                _ => {
                    return Some(CmdResult::ReportError(
                        crate::t!("Choose an available selection-editing option.").into_owned(),
                    ))
                }
            };
            self.mode = Some(mode);
            return Some(CmdResult::DeselectAndContinue);
        }
        None
    }
    fn wants_point_pick_context(&self) -> bool {
        self.chosen.is_none()
    }
    fn set_point_pick_context(&mut self, context: Option<PointPickContext>) {
        self.context = context;
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.chosen.is_none() {
            let Some(c) = self.context else {
                return CmdResult::ReportError(crate::t!("Enter the action name.").into_owned());
            };
            let project =
                |p| crate::scene::pick::hit_test::world_to_screen(p, c.view, c.eye, c.bounds);
            let at = project(point);
            if let Some((i, _)) = self
                .candidates
                .iter()
                .enumerate()
                .map(|(i, a)| (i, project(a.at).distance(at)))
                .filter(|(_, d)| d.is_finite() && *d <= c.aperture_px.max(10.))
                .min_by(|(_, a), (_, b)| a.total_cmp(b))
            {
                self.chosen = Some(i);
                return CmdResult::NeedPoint;
            }
            return CmdResult::ReportError(crate::t!("Pick an action marker.").into_owned());
        }
        if matches!(self.mode, Some(Mode::Frame)) {
            let plane = self.candidates[self.chosen.unwrap()].at.z;
            if !point.is_finite() || (point.z - plane).abs() > 1e-8 {
                return CmdResult::ReportError(
                    crate::t!("Specify a finite corner in the action's XY plane.").into_owned(),
                );
            }
            if self.polygon_frame {
                if self.frame.len() >= 3 && self.frame[0].distance(point) < 1e-9 {
                    return self.finish_polygon();
                }
                if self
                    .frame
                    .last()
                    .is_some_and(|last| last.distance(point) < 1e-9)
                {
                    return CmdResult::ReportError(
                        crate::t!("Specify a distinct polygon vertex.").into_owned(),
                    );
                }
                self.frame.push(point);
                return CmdResult::NeedPoint;
            }
            if self
                .frame
                .first()
                .is_some_and(|a| (a.x - point.x).abs() < 1e-9 || (a.y - point.y).abs() < 1e-9)
            {
                return CmdResult::ReportError(
                    crate::t!("The stretch frame needs a nonzero width and height.").into_owned(),
                );
            }
            self.frame.truncate(1);
            self.frame.push(point);
            if self.frame.len() == 2 {
                return self.finish();
            }
        }
        CmdResult::NeedPoint
    }
    fn is_selection_gathering(&self) -> bool {
        matches!(
            self.mode,
            Some(Mode::New | Mode::Add | Mode::Remove | Mode::RotateOnly)
        )
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.picked = handles;
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        if matches!(self.mode, Some(Mode::Frame)) && self.polygon_frame {
            return self.finish_polygon();
        }

        if self.chosen.is_none() {
            return CmdResult::Cancel;
        }
        if self.mode.is_none() {
            return self.on_text_input("New").unwrap();
        }
        if self.is_selection_gathering() {
            self.finish()
        } else {
            CmdResult::Cancel
        }
    }
    fn on_preview_wires(&mut self, point: DVec3) -> Vec<crate::scene::WireModel> {
        if self.polygon_frame {
            super::action::polygon_preview(&self.frame, Some(point))
        } else {
            super::action::frame_preview(&self.frame, point)
        }
    }
}
