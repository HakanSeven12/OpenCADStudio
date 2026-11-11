//! Reuse the command host's point picking and object-set selection.
use crate::command::{CadCommand, CmdOption, CmdResult, PointPickContext};
use crate::scene::block_actions::{self as actions, Kind, Parameter, XyKeyPoint};
use document::{Document, Handle};
use glam::DVec3;

inventory::submit!(crate::command::CommandRegistration {
    names: &["BACTION", "BACTIONTOOL", "BARRAY"]
});

struct LookupChoice {
    handle: Handle,
    name: String,
    point: DVec3,
}

pub(crate) struct ActionCommand {
    document: u64,
    definition: Handle,
    kind: Option<Kind>,
    candidates: Vec<Parameter>,
    lookups: Vec<LookupChoice>,
    lookup: Option<String>,
    parameter: Option<Handle>,
    xy_key_point: Option<XyKeyPoint>,
    targets: Vec<Handle>,
    selecting: bool,
    choosing_rotation: bool,
    rotate_only: Vec<Handle>,
    frame: Vec<DVec3>,
    polygon_frame: bool,
    frame_done: bool,
    column_offset: Option<f64>,
    row_offset: Option<f64>,
    context: Option<PointPickContext>,
    multiplier: f64,
    angle_offset: f64,
    override_input: Option<bool>,
}
impl ActionCommand {
    pub(crate) fn new(
        document: u64,
        definition: Handle,
        kind: Option<Kind>,
        doc: &Document,
    ) -> Self {
        let candidates = match kind {
            None | Some(Kind::Move | Kind::Flip) => actions::all_parameters(doc, definition),
            Some(kind) => actions::parameters(doc, definition, kind),
        };
        let lookups = if kind.is_none() {
            crate::scene::block_lookup::parameters(doc, definition)
                .into_iter()
                .map(|(handle, parameter)| LookupChoice {
                    handle,
                    name: parameter.lookup_name.clone(),
                    point: actions::v(parameter.parameter.definition_point),
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            document,
            definition,
            kind,
            candidates,
            lookups,
            lookup: None,
            parameter: None,
            xy_key_point: None,
            targets: vec![],
            selecting: false,
            choosing_rotation: false,
            rotate_only: vec![],
            frame: vec![],
            polygon_frame: false,
            frame_done: false,
            column_offset: None,
            row_offset: None,
            context: None,
            multiplier: 1.,
            angle_offset: 0.,
            override_input: None,
        }
    }
    pub(crate) fn for_parameter(
        document: u64,
        definition: Handle,
        kind: Kind,
        doc: &Document,
        parameter: Handle,
        xy_key_point: Option<XyKeyPoint>,
    ) -> Option<Self> {
        let mut command = Self::new(document, definition, Some(kind), doc);
        if !command
            .candidates
            .iter()
            .any(|candidate| candidate.handle == parameter && actions::compatible(candidate, kind))
        {
            return None;
        }
        command.parameter = Some(parameter);
        command.xy_key_point = xy_key_point;
        command.selecting = !actions::needs_frame(kind);
        Some(command)
    }
    fn target_source(&self) -> Option<(Kind, i32)> {
        let kind = self.kind?;
        if !matches!(kind, Kind::Move | Kind::Flip) {
            return None;
        }
        let source = self
            .candidates
            .iter()
            .find(|p| Some(p.handle) == self.parameter)?;
        Some((kind, source.id))
    }
    fn point_choices(&self) -> Option<(Kind, i32, Vec<(Handle, i32, String)>)> {
        if !self.selecting || self.choosing_rotation {
            return None;
        }
        self.point_choices_snapshot()
    }
    fn point_choices_snapshot(&self) -> Option<(Kind, i32, Vec<(Handle, i32, String)>)> {
        let (kind, source) = self.target_source()?;
        Some((
            kind,
            source,
            self.candidates
                .iter()
                .filter(|p| p.is_action_target(kind, source))
                .map(|p| (p.handle, p.id, p.name.clone()))
                .collect(),
        ))
    }
    fn finish_frame(&mut self) -> CmdResult {
        if self.frame.len() < 3 {
            return CmdResult::ReportError(
                crate::t!("Specify at least three polygon vertices.").into_owned(),
            );
        }
        if let Err(error) = actions::validate_frame_points(&self.frame) {
            return CmdResult::ReportError(crate::t!(error).into_owned());
        }
        self.frame_done = true;
        self.selecting = true;
        CmdResult::DeselectAndContinue
    }
    fn choose(&mut self, p: Parameter) -> CmdResult {
        if self.kind.is_some_and(|kind| !actions::compatible(&p, kind)) {
            return CmdResult::ReportError(
                crate::t!("Select a compatible parameter for this action.").into_owned(),
            );
        }
        self.parameter = Some(p.handle);
        self.selecting = self.kind.is_some_and(|kind| !actions::needs_frame(kind));
        CmdResult::DeselectAndContinue
    }
    fn choose_lookup(&mut self, index: usize) -> CmdResult {
        let (handle, name) = {
            let lookup = &self.lookups[index];
            (lookup.handle, lookup.name.clone())
        };
        self.parameter = Some(handle);
        self.lookup = Some(name);
        self.selecting = false;
        CmdResult::DeselectAndContinue
    }
    /// The action types offered for the chosen parameter, in the order the
    /// list prints them.
    fn action_types(&self) -> Vec<Kind> {
        const ORDER: [Kind; 7] = [
            Kind::Array,
            Kind::Flip,
            Kind::Move,
            Kind::PolarStretch,
            Kind::Rotate,
            Kind::Scale,
            Kind::Stretch,
        ];
        let Some(parameter) = self.candidates.iter().find(|p| Some(p.handle) == self.parameter)
        else {
            return Vec::new();
        };
        ORDER
            .into_iter()
            .filter(|kind| actions::compatible(parameter, *kind))
            .collect()
    }
    fn choose_action_type(&mut self, s: &str) -> CmdResult {
        let refusal = || {
            CmdResult::ReportError(
                crate::t!("Choose one of the offered action types.").into_owned(),
            )
        };
        if let Some(name) = &self.lookup {
            return if matches!(s.trim().to_ascii_uppercase().as_str(), "LOOKUP" | "L") {
                CmdResult::OpenBlockLookupTable {
                    document: self.document,
                    definition: self.definition,
                    name: name.clone(),
                }
            } else {
                refusal()
            };
        }
        let Some(kind) = Kind::parse(s) else {
            return refusal();
        };
        let Some(parameter) = self.candidates.iter().find(|p| Some(p.handle) == self.parameter)
        else {
            return refusal();
        };
        if !actions::compatible(parameter, kind) {
            return refusal();
        }
        self.kind = Some(kind);
        self.selecting = !actions::needs_frame(kind);
        CmdResult::DeselectAndContinue
    }
    fn finish_action(&self, point: DVec3) -> CmdResult {
        if !point.is_finite() {
            return CmdResult::ReportError(
                crate::t!("Specify a finite action location.").into_owned(),
            );
        }
        let array_offsets = if self.kind == Some(Kind::Array) {
            let col = self.column_offset.unwrap_or(1.0);
            let row = self.row_offset.unwrap_or(1.0);
            Some((col, row))
        } else {
            None
        };
        CmdResult::PlaceBlockAction {
            document: self.document,
            definition: self.definition,
            kind: self.kind.unwrap(),
            parameter: self.parameter.unwrap(),
            xy_key_point: self.xy_key_point,
            point_choices: self.point_choices_snapshot(),
            targets: {
                let mut targets = self.targets.clone();
                targets.extend(&self.rotate_only);
                targets.sort();
                targets.dedup();
                targets
            },
            location: point,
            frame: self.frame_done.then(|| self.frame.clone()),
            array_offsets,
            displacement_overrides: matches!(
                self.kind,
                Some(Kind::Move | Kind::Stretch | Kind::PolarStretch)
            )
            .then_some((self.multiplier, self.angle_offset)),
            rotate_only: self.rotate_only.clone(),
        }
    }
}
impl CadCommand for ActionCommand {
    fn name(&self) -> &'static str {
        "BACTION"
    }
    fn input_kind(&self) -> crate::command::InputKind {
        if self.override_input.is_some() {
            return crate::command::InputKind::SingleToken;
        }
        if self.parameter.is_none() {
            return crate::command::InputKind::FreeText;
        }
        if self.kind.is_none() {
            return crate::command::InputKind::SingleToken;
        }
        if self
            .point_choices()
            .is_some_and(|(_, _, choices)| !choices.is_empty())
        {
            return crate::command::InputKind::FreeText;
        }
        if self.kind == Some(Kind::Array) && !self.selecting {
            let is_xy = self
                .candidates
                .iter()
                .find(|p| Some(p.handle) == self.parameter)
                .is_some_and(|p| p.xy);
            if self.column_offset.is_none() || (is_xy && self.row_offset.is_none()) {
                return crate::command::InputKind::SingleToken;
            }
        }
        crate::command::InputKind::Point
    }
    fn prompt(&self) -> String {
        if let Some(multiplier) = self.override_input {
            return if multiplier {
                crate::tf!("BACTION  Enter distance multiplier <{}>:", self.multiplier).into_owned()
            } else {
                crate::tf!(
                    "BACTION  Enter angle offset in degrees <{}>:",
                    self.angle_offset.to_degrees()
                )
                .into_owned()
            };
        }
        if self.parameter.is_none() {
            if self.kind.is_none() {
                return crate::t!("BACTION  Select parameter:").into_owned();
            }
            return crate::tf!(
                "BACTION {}  Select a parameter endpoint or enter its name:",
                crate::t!(self.kind.unwrap().name())
            )
            .into_owned();
        }
        if self.kind.is_none() {
            if self.lookup.is_some() {
                return crate::t!("BACTION  Enter action type [Lookup]:").into_owned();
            }
            let names = self
                .action_types()
                .into_iter()
                .map(action_type_label)
                .collect::<Vec<_>>()
                .join("/");
            return crate::tf!("BACTION  Enter action type [{}]:", names).into_owned();
        }
        if self.kind.is_some_and(actions::needs_frame) && !self.frame_done && self.polygon_frame {
            return crate::t!(
                "BACTION  Specify polygon vertex or [Undo/Close]; press Enter to close:"
            )
            .into_owned();
        }
        if self.kind.is_some_and(actions::needs_frame) && self.frame.is_empty() {
            if self.kind == Some(Kind::PolarStretch) {
                let point = if self.xy_key_point == Some(XyKeyPoint::Base) {
                    "Base"
                } else {
                    "End"
                };
                return crate::tf!("BACTION  Specify first corner of action frame or [CPolygon/Base/End] <{point}>:").into_owned();
            }
            if let Some(point) = self.xy_key_point {
                let point = match point {
                    XyKeyPoint::Base => "base",
                    XyKeyPoint::X => "X-axis",
                    XyKeyPoint::Y => "Y-axis",
                    XyKeyPoint::End => "endpoint",
                };
                return crate::tf!("BACTION  Specify first corner of action frame for the parameter's {point} point:").into_owned();
            }
            return crate::t!("BACTION  Specify first corner of action frame or [CPolygon]:")
                .into_owned();
        }
        if self.kind.is_some_and(actions::needs_frame) && self.frame.len() == 1 {
            return crate::t!("BACTION  Specify opposite corner of action frame:").into_owned();
        }
        if self.selecting && self.choosing_rotation {
            return crate::t!("BACTION  Select objects to rotate only, then press Enter <None>:")
                .into_owned();
        }
        if self.selecting {
            if self
                .point_choices()
                .is_some_and(|(_, _, choices)| !choices.is_empty())
            {
                return if self.kind == Some(Kind::Flip) {
                    crate::tr!("block-action-targets", "flip-create-prompt")
                } else {
                    crate::tr!("block-action-targets", "create-prompt")
                };
            }
            if let Some(point) = self.xy_key_point {
                let point = match point {
                    XyKeyPoint::Base => "base",
                    XyKeyPoint::X => "X-axis",
                    XyKeyPoint::Y => "Y-axis",
                    XyKeyPoint::End => "endpoint",
                };
                return crate::tf!("BACTION  Select objects associated with the parameter's {point} point, then press Enter:").into_owned();
            }
            return crate::t!("BACTION  Select objects for the action, then press Enter:")
                .into_owned();
        }
        if self.kind == Some(Kind::Array) {
            let chosen = self
                .candidates
                .iter()
                .find(|p| Some(p.handle) == self.parameter);
            let is_xy = chosen.is_some_and(|p| p.xy);
            let is_polar = chosen.is_some_and(|p| p.polar);
            if self.column_offset.is_none() {
                // One term per array shape: an angular pattern arrays items, a
                // grid arrays columns and rows, and the alternative named here
                // is the term the next prompt asks for.
                if is_polar {
                    return crate::t!("BACTION  Specify distance between items:").into_owned();
                }
                return if is_xy {
                    crate::t!(
                        "BACTION  Specify distance between columns (or distance between rows):"
                    )
                    .into_owned()
                } else {
                    crate::t!("BACTION  Enter the distance between columns (|||):").into_owned()
                };
            }
            if is_xy && self.row_offset.is_none() {
                return crate::t!("BACTION  Specify distance between rows:").into_owned();
            }
        }
        if matches!(
            self.kind,
            Some(Kind::Move | Kind::Stretch | Kind::PolarStretch)
        ) {
            crate::t!("BACTION  Specify action marker location or [Multiplier/Offset]:")
                .into_owned()
        } else {
            crate::t!("BACTION  Specify action marker location:").into_owned()
        }
    }
    fn options(&self) -> Vec<CmdOption> {
        if self.parameter.is_none() {
            vec![]
        } else if self.kind.is_none() {
            if self.lookup.is_some() {
                return vec![CmdOption::new("Lookup", "Lookup")];
            }
            self.action_types()
                .into_iter()
                .map(|kind| CmdOption::new(action_type_label(kind), kind.name()))
                .collect()
        } else if self.parameter.is_some()
            && self.kind.is_some_and(actions::needs_frame)
            && !self.frame_done
        {
            if self.polygon_frame {
                vec![
                    CmdOption::new("Undo", "Undo"),
                    CmdOption::new("Close", "Close"),
                ]
            } else if self.frame.is_empty() {
                let mut options = vec![CmdOption::new("CPolygon", "CPolygon")];
                if self.kind == Some(Kind::PolarStretch) {
                    options.extend([CmdOption::new("Base", "Base"), CmdOption::new("End", "End")]);
                }
                options
            } else {
                vec![]
            }
        } else if self.parameter.is_some()
            && !self.selecting
            && self.override_input.is_none()
            && matches!(
                self.kind,
                Some(Kind::Move | Kind::Stretch | Kind::PolarStretch)
            )
            && (!self.kind.is_some_and(actions::needs_frame) || self.frame_done)
        {
            vec![
                CmdOption::new("Multiplier", "Multiplier"),
                CmdOption::new("Offset", "Offset"),
            ]
        } else {
            vec![]
        }
    }
    fn on_text_input(&mut self, s: &str) -> Option<CmdResult> {
        if let Some((_, _, choices)) = self.point_choices() {
            if let Some(raw) = s
                .get(..10)
                .filter(|prefix| prefix.eq_ignore_ascii_case("parameter "))
                .and_then(|_| s.get(10..))
            {
                let name = if raw.starts_with('"') {
                    serde_json::from_str::<String>(raw).ok()
                } else {
                    Some(raw.trim().to_owned())
                };
                let matches = choices
                    .iter()
                    .filter(|(_, _, candidate)| {
                        name.as_ref()
                            .is_some_and(|name| candidate.eq_ignore_ascii_case(name))
                    })
                    .map(|(handle, _, _)| *handle)
                    .collect::<Vec<_>>();
                return Some(if matches.len() == 1 {
                    self.targets.push(matches[0]);
                    CmdResult::NeedPoint
                } else {
                    CmdResult::ReportError(crate::tr!("block-action-targets", "unique-point"))
                });
            }
        }
        if self.parameter.is_some()
            && self.kind == Some(Kind::PolarStretch)
            && self.frame.is_empty()
            && !self.frame_done
        {
            let key = match s.trim().to_ascii_uppercase().as_str() {
                "BASE" | "B" => Some(XyKeyPoint::Base),
                "END" | "E" => Some(XyKeyPoint::End),
                _ => None,
            };
            if let Some(key) = key {
                self.xy_key_point = Some(key);
                return Some(CmdResult::NeedPoint);
            }
        }
        if self.parameter.is_some()
            && self.kind.is_some_and(actions::needs_frame)
            && !self.frame_done
        {
            match s.trim().to_ascii_uppercase().as_str() {
                "CPOLYGON" | "CP" if self.frame.is_empty() => {
                    self.polygon_frame = true;
                    return Some(CmdResult::NeedPoint);
                }
                "UNDO" | "U" if self.polygon_frame => {
                    self.frame.pop();
                    return Some(CmdResult::NeedPoint);
                }
                "CLOSE" | "C" if self.polygon_frame => return Some(self.finish_frame()),
                _ => {}
            }
        }

        if let Some(multiplier) = self.override_input {
            let Ok(value) = s.trim().parse::<f64>() else {
                return Some(CmdResult::ReportError(
                    crate::t!("Enter a finite action override.").into_owned(),
                ));
            };
            if !value.is_finite() {
                return Some(CmdResult::ReportError(
                    crate::t!("Enter a finite action override.").into_owned(),
                ));
            }
            if multiplier {
                self.multiplier = value;
            } else {
                self.angle_offset = value.to_radians();
            }
            self.override_input = None;
            return Some(CmdResult::NeedPoint);
        }
        if self.parameter.is_some()
            && !self.selecting
            && matches!(
                self.kind,
                Some(Kind::Move | Kind::Stretch | Kind::PolarStretch)
            )
            && (!self.kind.is_some_and(actions::needs_frame) || self.frame_done)
        {
            match s.trim().to_ascii_uppercase().as_str() {
                "MULTIPLIER" | "M" => self.override_input = Some(true),
                "OFFSET" | "O" => self.override_input = Some(false),
                _ => {}
            }
            if self.override_input.is_some() {
                return Some(CmdResult::NeedPoint);
            }
        }
        if self.parameter.is_none() {
            let name = s.trim();
            if let Some(kind) = self.kind {
                let matches = self
                    .candidates
                    .iter()
                    .filter(|p| p.name.eq_ignore_ascii_case(name) && actions::compatible(p, kind))
                    .cloned()
                    .collect::<Vec<_>>();
                return Some(if matches.len() == 1 {
                    self.choose(matches[0].clone())
                } else {
                    CmdResult::ReportError(
                        crate::t!("Enter a unique compatible parameter name or pick its endpoint.")
                            .into_owned(),
                    )
                });
            }
            let geometry = self
                .candidates
                .iter()
                .filter(|p| p.name.eq_ignore_ascii_case(name))
                .cloned()
                .collect::<Vec<_>>();
            let lookups = self
                .lookups
                .iter()
                .enumerate()
                .filter(|(_, lookup)| lookup.name.eq_ignore_ascii_case(name))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            return Some(match (geometry.len(), lookups.len()) {
                (1, 0) => self.choose(geometry.into_iter().next().unwrap()),
                (0, 1) => self.choose_lookup(lookups[0]),
                _ => CmdResult::ReportError(
                    crate::t!("Enter a unique parameter name or pick its endpoint.").into_owned(),
                ),
            });
        }
        if self.kind.is_none() {
            return Some(self.choose_action_type(s));
        }
        if self.kind == Some(Kind::Array) && !self.selecting {
            let p = self
                .candidates
                .iter()
                .find(|p| Some(p.handle) == self.parameter)
                .unwrap();
            if self.column_offset.is_none() {
                let Ok(dist) = s.trim().parse::<f64>() else {
                    return Some(CmdResult::ReportError(
                        crate::t!("Specify a positive distance as a single value, not a point.")
                            .into_owned(),
                    ));
                };
                if dist <= 1e-6 || !dist.is_finite() {
                    return Some(CmdResult::ReportError(
                        crate::t!("Specify a positive distance.").into_owned(),
                    ));
                }
                self.column_offset = Some(dist);
                // An array has no location of its own: its last spacing answer
                // records it at the parameter's end. Only XY asks a second distance.
                if !p.xy {
                    return Some(self.finish_action(p.end));
                }
                return Some(CmdResult::NeedPoint);
            }
            if p.xy && self.row_offset.is_none() {
                let Ok(dist) = s.trim().parse::<f64>() else {
                    return Some(CmdResult::ReportError(
                        crate::t!("Specify a positive distance as a single value, not a point.")
                            .into_owned(),
                    ));
                };
                if dist <= 1e-6 || !dist.is_finite() {
                    return Some(CmdResult::ReportError(
                        crate::t!("Specify a positive distance.").into_owned(),
                    ));
                }
                self.row_offset = Some(dist);
                return Some(self.finish_action(p.end));
            }
        }
        None
    }
    fn wants_point_pick_context(&self) -> bool {
        self.parameter.is_none()
    }
    fn set_point_pick_context(&mut self, c: Option<PointPickContext>) {
        self.context = c;
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.override_input.is_some() {
            return CmdResult::ReportError(
                crate::t!("Enter a finite action override.").into_owned(),
            );
        }
        if self.parameter.is_none() {
            let Some(c) = self.context else {
                return CmdResult::ReportError(crate::t!("Enter the parameter name.").into_owned());
            };
            let candidate = crate::scene::view::projection::nearest_in_aperture(
                c.view,
                c.eye,
                c.bounds,
                point,
                c.aperture_px,
                self.candidates
                    .iter()
                    .filter(|p| self.kind.is_none_or(|kind| actions::compatible(p, kind)))
                    .cloned(),
                |p| p.end,
            );
            if let Some(p) = candidate {
                return self.choose(p);
            }
            if self.kind.is_none() {
                let lookup = crate::scene::view::projection::nearest_in_aperture(
                    c.view,
                    c.eye,
                    c.bounds,
                    point,
                    c.aperture_px,
                    self.lookups.iter().enumerate(),
                    |(_, lookup)| lookup.point,
                )
                .map(|(index, _)| index);
                if let Some(index) = lookup {
                    return self.choose_lookup(index);
                }
                return CmdResult::ReportError(
                    crate::t!("Pick a parameter endpoint or enter its name.").into_owned(),
                );
            }
            return CmdResult::ReportError(
                crate::t!("Pick a compatible parameter endpoint.").into_owned(),
            );
        }
        if self.kind.is_some_and(actions::needs_frame) && !self.frame_done {
            let plane = self
                .candidates
                .iter()
                .find(|p| Some(p.handle) == self.parameter)
                .unwrap()
                .end
                .z;
            if !point.is_finite() || (point.z - plane).abs() > 1e-8 {
                return CmdResult::ReportError(
                    crate::t!("Specify a finite corner in the parameter's XY plane.").into_owned(),
                );
            }
            if self.polygon_frame {
                if self.frame.len() >= 3 && self.frame[0].distance(point) < 1e-9 {
                    return self.finish_frame();
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
                .is_some_and(|a| (point.x - a.x).abs() < 1e-9 || (point.y - a.y).abs() < 1e-9)
            {
                return CmdResult::ReportError(
                    crate::t!("The stretch frame needs a nonzero width and height.").into_owned(),
                );
            }
            self.frame.push(point);
            if self.frame.len() == 2 {
                self.frame_done = true;
                self.selecting = true;
                return CmdResult::DeselectAndContinue;
            }
            return CmdResult::NeedPoint;
        }
        if self.kind == Some(Kind::Array) && !self.selecting {
            let p = self
                .candidates
                .iter()
                .find(|p| Some(p.handle) == self.parameter)
                .unwrap();
            if self.column_offset.is_none() {
                let dist = point.distance(p.base);
                if dist <= 1e-6 || !dist.is_finite() {
                    return CmdResult::ReportError(
                        crate::t!("Specify a positive distance.").into_owned(),
                    );
                }
                self.column_offset = Some(dist);
                if !p.xy {
                    return self.finish_action(p.end);
                }
                return CmdResult::NeedPoint;
            }
            if p.xy && self.row_offset.is_none() {
                let dist = (point.y - p.base.y).abs().max(point.distance(p.base));
                if dist <= 1e-6 || !dist.is_finite() {
                    return CmdResult::ReportError(
                        crate::t!("Specify a positive distance.").into_owned(),
                    );
                }
                self.row_offset = Some(dist);
                return self.finish_action(p.end);
            }
        }
        if self.selecting {
            return CmdResult::NeedPoint;
        }
        self.finish_action(point)
    }

    fn on_preview_wires(&mut self, point: DVec3) -> Vec<crate::scene::WireModel> {
        if self.polygon_frame {
            polygon_preview(&self.frame, (!self.frame_done).then_some(point))
        } else {
            frame_preview(&self.frame, point)
        }
    }

    fn on_selection_marker_click(
        &mut self,
        scene: &crate::scene::Scene,
        position: iced::Point,
        context: PointPickContext,
        remove: bool,
        replace: bool,
    ) -> Option<String> {
        if !self.selecting || self.choosing_rotation {
            return None;
        }
        let (kind, source) = self.target_source()?;
        let (handle, name) = super::action_set::pick_point_marker(
            scene,
            self.definition,
            |h| {
                self.candidates
                    .iter()
                    .any(|p| p.is_action_target(kind, source) && p.handle == h)
            },
            position,
            context,
        )?;
        if !remove && replace {
            self.targets.retain(|h| {
                !self
                    .candidates
                    .iter()
                    .any(|p| p.is_action_target(kind, source) && p.handle == *h)
            });
        }
        if remove {
            self.targets.retain(|h| *h != handle);
        } else if !self.targets.contains(&handle) {
            self.targets.push(handle);
        }
        Some(name)
    }
    fn selection_marker_selected(&self, handle: Handle) -> bool {
        self.target_source().is_some_and(|(kind, source)| {
            self.targets.contains(&handle)
                && self
                    .candidates
                    .iter()
                    .any(|p| p.handle == handle && p.is_action_target(kind, source))
        })
    }
    fn clear_selection_markers(&mut self) {
        let Some((kind, source)) = self.target_source() else {
            return;
        };
        self.targets.retain(|h| {
            !self
                .candidates
                .iter()
                .any(|p| p.is_action_target(kind, source) && p.handle == *h)
        });
    }
    fn on_selection_marker_region(
        &mut self,
        scene: &crate::scene::Scene,
        polygon: &[iced::Point],
        context: PointPickContext,
        crossing: bool,
        remove: bool,
        replace: bool,
        geometry_hit: bool,
    ) -> Vec<String> {
        if !self.selecting || self.choosing_rotation {
            return Vec::new();
        }
        let Some((kind, source)) = self.target_source() else {
            return Vec::new();
        };
        let hits = super::action_set::point_markers_in_region(
            scene,
            self.definition,
            |h| {
                self.candidates
                    .iter()
                    .any(|p| p.is_action_target(kind, source) && p.handle == h)
            },
            polygon,
            context,
            crossing,
        );
        if !remove && replace && (geometry_hit || !hits.is_empty()) {
            self.targets.retain(|h| {
                !self
                    .candidates
                    .iter()
                    .any(|p| p.is_action_target(kind, source) && p.handle == *h)
            });
        }
        for (handle, _) in &hits {
            if remove {
                self.targets.retain(|h| h != handle);
            } else if !self.targets.contains(handle) {
                self.targets.push(*handle);
            }
        }
        hits.into_iter().map(|(_, name)| name).collect()
    }
    fn is_selection_gathering(&self) -> bool {
        self.selecting
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        // The host calls this after every pick/window/removal, not just Enter.
        // Retain the current set while the user gathers additional objects.
        if self.choosing_rotation {
            self.rotate_only = handles;
        } else if let Some((_, _, choices)) = self.point_choices() {
            self.targets
                .retain(|h| choices.iter().any(|(point, _, _)| point == h));
            self.targets.extend(handles);
        } else {
            self.targets = handles;
        }
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.polygon_frame && !self.frame_done {
            return self.finish_frame();
        }

        if self.override_input.take().is_some() {
            return CmdResult::NeedPoint;
        }
        if self.selecting {
            if self.kind == Some(Kind::PolarStretch) && !self.choosing_rotation {
                self.choosing_rotation = true;
                return CmdResult::DeselectAndContinue;
            }
            if self.targets.is_empty() && self.rotate_only.is_empty() {
                return CmdResult::ReportError(
                    crate::t!("Select at least one object for the action.").into_owned(),
                );
            }
            self.selecting = false;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }
}

/// The label the action-type list prints for a kind. The stretch name leads
/// with a lower-case letter so it is not mistaken for Scale's initial.
fn action_type_label(kind: Kind) -> &'static str {
    if kind == Kind::Stretch {
        "sTretch"
    } else {
        kind.name()
    }
}

pub(crate) struct TestParameterCommand {
    pub grip_input: Option<crate::scene::block_grips::runtime::Input>,
    pub display_offset: DVec3,
    pub insert: Option<Handle>,
    pub document: u64,
    pub definition: Handle,
    pub parameter: Handle,
}
impl TestParameterCommand {
    fn parameter_result(&self, position: DVec3) -> CmdResult {
        CmdResult::EvaluateBlockParameter {
            grip: self.grip_input.as_ref().map(|input| input.id),
            insert: self.insert,
            document: self.document,
            definition: self.definition,
            parameter: self.parameter,
            value: self
                .grip_input
                .as_ref()
                .map(|input| input.value(position))
                .unwrap_or(actions::Value::Point(position - self.display_offset)),
        }
    }
}
impl CadCommand for TestParameterCommand {
    fn block_parameter_preview(&self, cursor: DVec3) -> Option<CmdResult> {
        Some(self.parameter_result(cursor))
    }
    fn cursor_plane(&self) -> Option<(DVec3, DVec3)> {
        let input = self.grip_input.as_ref()?;
        let normal =
            actions::v(input.to_world.apply_rotation(actions::native(DVec3::Z))).try_normalize()?;
        Some((
            normal,
            actions::v(input.to_world.apply(actions::native(input.frame.base))),
        ))
    }
    fn block_parameter_grip_id(&self) -> Option<usize> {
        self.grip_input.as_ref().map(|input| input.id)
    }
    fn block_parameter_target(&self) -> Option<(Option<Handle>, Handle)> {
        Some((self.insert, self.parameter))
    }
    fn name(&self) -> &'static str {
        if self.insert.is_some() {
            "BLOCKPARAMETERGRIP"
        } else {
            "BTESTBLOCK"
        }
    }
    fn prompt(&self) -> String {
        crate::t!("Specify the new block parameter grip position (Escape cancels):").into_owned()
    }
    fn on_point(&mut self, position: DVec3) -> CmdResult {
        self.parameter_result(position)
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// Shared rectangular frame preview for authoring and reauthoring.
pub(super) fn frame_preview(frame: &[DVec3], point: DVec3) -> Vec<crate::scene::WireModel> {
    let Some(a) = frame.first().filter(|_| point.is_finite()) else {
        return vec![];
    };
    let b = frame.get(1).copied().unwrap_or(point);
    vec![crate::scene::WireModel::solid_f64(
        "preview_stretch_frame".into(),
        vec![
            a.to_array(),
            [b.x, a.y, a.z],
            [b.x, b.y, a.z],
            [a.x, b.y, a.z],
        ],
        crate::scene::WireModel::CYAN,
        true,
    )]
}

pub(super) fn polygon_preview(
    frame: &[DVec3],
    cursor: Option<DVec3>,
) -> Vec<crate::scene::WireModel> {
    let mut points = frame
        .iter()
        .map(|point| point.to_array())
        .collect::<Vec<_>>();
    if let Some(point) = cursor.filter(|point| point.is_finite()) {
        points.push(point.to_array());
    }
    if points.len() < 2 {
        return vec![];
    }
    vec![crate::scene::WireModel::solid_f64(
        "preview_stretch_polygon".into(),
        points,
        crate::scene::WireModel::CYAN,
        true,
    )]
}
