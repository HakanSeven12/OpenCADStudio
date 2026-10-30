use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, PointPickContext};
use crate::scene::block_actions::{self as actions, Kind, Parameter, XyKeyPoint};
use codec::{Document, Handle};
use glam::DVec3;

inventory::submit!(crate::command::CommandRegistration {
    names: &["BASSOCIATE"]
});

struct Candidate {
    handle: Handle,
    name: String,
    at: DVec3,
    kind: Kind,
    expected: codec::objects::ObjectType,
}

pub(crate) struct AssociateCommand {
    document: u64,
    definition: Handle,
    candidates: Vec<Candidate>,
    parameters: Vec<Parameter>,
    action: Option<usize>,
    parameter: Option<usize>,
    context: Option<PointPickContext>,
}

impl AssociateCommand {
    pub(crate) fn new(
        document: u64,
        definition: Handle,
        doc: &Document,
        selected: Option<Handle>,
    ) -> Self {
        let orphaned = actions::association::orphaned(doc, definition);
        let candidates = actions::actions(doc, definition)
            .into_iter()
            .filter(|(h, _, _, _)| orphaned.contains(h))
            .map(|(handle, action, kind, _)| Candidate {
                handle,
                name: action.element.name.clone(),
                at: actions::v(action.display_location),
                kind,
                expected: doc.objects[&handle].clone(),
            })
            .collect::<Vec<_>>();
        let action = candidates
            .iter()
            .position(|candidate| Some(candidate.handle) == selected);
        Self {
            document,
            definition,
            candidates,
            parameters: actions::all_parameters(doc, definition),
            action,
            parameter: None,
            context: None,
        }
    }
    fn finish(&self, key_point: Option<XyKeyPoint>) -> CmdResult {
        let action = &self.candidates[self.action.unwrap()];
        CmdResult::AssociateBlockAction {
            document: self.document,
            definition: self.definition,
            action: action.handle,
            expected: Box::new(action.expected.clone()),
            parameter: self.parameters[self.parameter.unwrap()].handle,
            key_point,
        }
    }
    fn choose_parameter(&mut self, index: usize, clicked_key: Option<XyKeyPoint>) -> CmdResult {
        self.parameter = Some(index);
        let kind = self.candidates[self.action.unwrap()].kind;
        if matches!(kind, Kind::Move | Kind::Stretch | Kind::PolarStretch) && !self.parameters[index].point {
            clicked_key.map_or(CmdResult::NeedPoint, |key| self.finish(Some(key)))
        } else {
            self.finish(None)
        }
    }
}

impl CadCommand for AssociateCommand {
    fn name(&self) -> &'static str {
        "BASSOCIATE"
    }
    fn input_kind(&self) -> InputKind {
        if self.parameter.is_some() {
            InputKind::SingleToken
        } else {
            InputKind::FreeText
        }
    }
    fn prompt(&self) -> String {
        crate::t!(if self.action.is_none() {
            "BASSOCIATE  Select an unassociated action marker or enter its name:"
        } else if self.parameter.is_none() {
            "BASSOCIATE  Select a compatible parameter key point or enter its name:"
        } else if self.parameters[self.parameter.unwrap()].xy {
            "BASSOCIATE  Parameter key point [Base/X/Y/End] <End>:"
        } else {
            "BASSOCIATE  Parameter key point [Base/End] <End>:"
        })
        .into_owned()
    }
    fn options(&self) -> Vec<CmdOption> {
        if let Some(index) = self.parameter {
            let mut options = vec![CmdOption::new("Base", "Base")];
            if self.parameters[index].xy {
                options.extend([CmdOption::new("X", "X"), CmdOption::new("Y", "Y")]);
            }
            options.push(CmdOption::new("End", "End"));
            options
        } else {
            vec![]
        }
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if self.action.is_none() {
            let matches = self
                .candidates
                .iter()
                .enumerate()
                .filter(|(_, candidate)| candidate.name.eq_ignore_ascii_case(text))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            return Some(if matches.len() == 1 {
                self.action = Some(matches[0]);
                CmdResult::NeedPoint
            } else {
                CmdResult::ReportError(
                    crate::t!("Enter a unique unassociated action name or pick its marker.")
                        .into_owned(),
                )
            });
        }
        if self.parameter.is_none() {
            let kind = self.candidates[self.action.unwrap()].kind;
            let matches = self
                .parameters
                .iter()
                .enumerate()
                .filter(|(_, p)| actions::compatible(p, kind) && p.name.eq_ignore_ascii_case(text))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            return Some(if matches.len() == 1 {
                self.choose_parameter(matches[0], None)
            } else {
                CmdResult::ReportError(
                    crate::t!("Enter a unique compatible parameter name or pick its key point.")
                        .into_owned(),
                )
            });
        }
        let key = match text.to_ascii_uppercase().as_str() {
            "BASE" | "B" => Some(XyKeyPoint::Base),
            "END" | "E" => Some(XyKeyPoint::End),
            "X" if self.parameters[self.parameter.unwrap()].xy => Some(XyKeyPoint::X),
            "Y" if self.parameters[self.parameter.unwrap()].xy => Some(XyKeyPoint::Y),
            _ => None,
        };
        Some(key.map(|key| self.finish(Some(key))).unwrap_or_else(|| {
            CmdResult::ReportError(
                crate::t!("Choose an available parameter key point.").into_owned(),
            )
        }))
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.parameter.is_some() {
            self.finish(Some(XyKeyPoint::End))
        } else {
            CmdResult::Cancel
        }
    }
    fn wants_point_pick_context(&self) -> bool {
        self.parameter.is_none()
    }
    fn set_point_pick_context(&mut self, context: Option<PointPickContext>) {
        self.context = context;
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let Some(context) = self.context else {
            return CmdResult::ReportError(
                crate::t!("Enter an action or parameter name.").into_owned(),
            );
        };
        let screen = |p| {
            crate::scene::pick::hit_test::world_to_screen(
                p,
                context.view,
                context.eye,
                context.bounds,
            )
        };
        let click = screen(point);
        let aperture = context.aperture_px.max(10.0);
        if self.action.is_none() {
            let chosen = self
                .candidates
                .iter()
                .enumerate()
                .map(|(index, c)| (screen(c.at).distance(click), index))
                .filter(|(distance, _)| distance.is_finite() && *distance <= aperture)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            return if let Some((_, index)) = chosen {
                self.action = Some(index);
                CmdResult::NeedPoint
            } else {
                CmdResult::ReportError(
                    crate::t!("Pick an unassociated action marker.").into_owned(),
                )
            };
        }
        let kind = self.candidates[self.action.unwrap()].kind;
        let chosen = self
            .parameters
            .iter()
            .enumerate()
            .filter(|(_, p)| actions::compatible(p, kind))
            .flat_map(|(index, p)| {
                let mut points = vec![(p.base, XyKeyPoint::Base), (p.end, XyKeyPoint::End)];
                if p.xy {
                    points.extend([
                        (DVec3::new(p.end.x, p.base.y, p.end.z), XyKeyPoint::X),
                        (DVec3::new(p.base.x, p.end.y, p.end.z), XyKeyPoint::Y),
                    ]);
                }
                points
                    .into_iter()
                    .map(move |(point, key)| (screen(point).distance(click), index, key))
            })
            .filter(|(distance, _, _)| distance.is_finite() && *distance <= aperture)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        chosen
            .map(|(_, index, key)| self.choose_parameter(index, Some(key)))
            .unwrap_or_else(|| {
                CmdResult::ReportError(
                    crate::t!("Pick a compatible parameter key point.").into_owned(),
                )
            })
    }
}
