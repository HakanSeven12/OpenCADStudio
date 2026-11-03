//! Edit native action selections without replacing their graph nodes.
use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, PointPickContext};
use crate::scene::block_actions as actions;
use codec::objects::ObjectType;
use codec::{Document, Handle};
use glam::DVec3;

inventory::submit!(crate::command::CommandRegistration {
    names: &["BACTIONSET"]
});

/// Pick the visible point marker or its leader in the active viewport.
pub(crate) fn pick_point_marker(
    scene: &crate::scene::Scene,
    definition: Handle,
    eligible: impl Fn(Handle) -> bool,
    click: iced::Point,
    context: PointPickContext,
) -> Option<(Handle, String)> {
    let project = |p: codec::types::Vector3| {
        crate::scene::view::projection::project_screen(
            context.view,
            context.eye,
            context.bounds,
            DVec3::new(p.x, p.y, p.z),
        )
    };
    let distance = |a: iced::Point, b: iced::Point| {
        let ab = glam::Vec2::new(b.x - a.x, b.y - a.y);
        let ap = glam::Vec2::new(click.x - a.x, click.y - a.y);
        let t = (ap.dot(ab) / ab.length_squared().max(1e-12)).clamp(0., 1.);
        (ap - ab * t).length()
    };
    crate::scene::block_authoring::edit::items(&scene.document, definition)
        .into_iter()
        .filter(|item| eligible(item.handle))
        .filter_map(|item| {
            let d = item
                .grips
                .iter()
                .filter_map(|(_, p)| project(*p).map(|at| distance(at, at)))
                .chain(
                    item.graphic
                        .lines
                        .iter()
                        .filter_map(|[a, b]| Some(distance(project(*a)?, project(*b)?))),
                )
                .fold(f32::INFINITY, f32::min);
            (d.is_finite() && d <= context.aperture_px.max(10.)).then_some((
                d,
                item.handle,
                item.name,
            ))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, handle, name)| (handle, name))
}

/// Select a point marker by its projected leader and endpoints.
pub(crate) fn point_markers_in_region(
    scene: &crate::scene::Scene,
    definition: Handle,
    eligible: impl Fn(Handle) -> bool,
    polygon: &[iced::Point],
    context: PointPickContext,
    crossing: bool,
) -> Vec<(Handle, String)> {
    use crate::scene::pick::hit_test::{point_in_polygon, segment_crosses_polygon};
    if polygon.len() < 3 {
        return Vec::new();
    }
    let project = |p: codec::types::Vector3| {
        crate::scene::view::projection::project_screen(
            context.view,
            context.eye,
            context.bounds,
            DVec3::new(p.x, p.y, p.z),
        )
        .filter(|p| {
            p.x >= 0. && p.x <= context.bounds.width && p.y >= 0. && p.y <= context.bounds.height
        })
    };
    crate::scene::block_authoring::edit::items(&scene.document, definition)
        .into_iter()
        .filter(|item| eligible(item.handle))
        .filter_map(|item| {
            let points = item
                .grips
                .iter()
                .map(|(_, p)| project(*p))
                .collect::<Vec<_>>();
            let lines = item
                .graphic
                .lines
                .iter()
                .map(|[a, b]| (project(*a), project(*b)))
                .collect::<Vec<_>>();
            let hit = if crossing {
                points
                    .iter()
                    .flatten()
                    .any(|p| point_in_polygon(*p, polygon))
                    || lines.iter().any(|(a, b)| match (a, b) {
                        (Some(a), Some(b)) => {
                            point_in_polygon(*a, polygon)
                                || point_in_polygon(*b, polygon)
                                || segment_crosses_polygon(*a, *b, polygon)
                        }
                        _ => false,
                    })
            } else {
                !points.is_empty()
                    && points
                        .iter()
                        .all(|p| p.is_some_and(|p| point_in_polygon(p, polygon)))
                    && lines.iter().all(|(a, b)| {
                        a.is_some_and(|p| point_in_polygon(p, polygon))
                            && b.is_some_and(|p| point_in_polygon(p, polygon))
                    })
            };
            hit.then_some((item.handle, item.name))
        })
        .collect()
}

struct Candidate {
    handle: Handle,
    name: String,
    at: DVec3,
    targets: Vec<Handle>,
    point_source: Option<i32>,
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
    points: Vec<(String, Handle, i32)>,
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
        let points = actions::all_parameters(doc, definition)
            .into_iter()
            .filter(|p| p.point)
            .map(|p| (p.name, p.handle, p.id))
            .collect::<Vec<_>>();
        let candidates = actions::actions(doc, definition)
            .into_iter()
            .map(|(handle, a, kind, id)| Candidate {
                handle,
                name: a.element.name.clone(),
                at: actions::v(a.display_location),
                targets: actions::editable_action_targets(
                    doc,
                    definition,
                    kind,
                    id,
                    &a.dependencies,
                ),
                point_source: (kind == actions::Kind::Move).then_some(id),
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
            points,
            chosen,
            mode: None,
            picked: vec![],
            frame: vec![],
            polygon_frame: false,
            context: None,
        }
    }
    fn has_point_choices(&self) -> bool {
        self.chosen
            .and_then(|i| self.candidates[i].point_source)
            .is_some_and(|source| self.points.iter().any(|(_, _, id)| *id != source))
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
            point_choices: c.point_source.map(|source| {
                (
                    source,
                    self.points
                        .iter()
                        .filter(|(_, _, id)| *id != source)
                        .map(|(name, handle, id)| (*handle, *id, name.clone()))
                        .collect(),
                )
            }),
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
        } else if matches!(self.mode, Some(Mode::New | Mode::Add | Mode::Remove))
            && self.has_point_choices()
        {
            InputKind::FreeText
        } else {
            InputKind::Point
        }
    }
    fn prompt(&self) -> String {
        if matches!(self.mode, Some(Mode::New | Mode::Add | Mode::Remove))
            && self.has_point_choices()
        {
            return crate::tr!("block-action-targets", "prompt");
        }
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
        if matches!(self.mode, Some(Mode::New | Mode::Add | Mode::Remove))
            && self
                .chosen
                .is_some_and(|i| self.candidates[i].point_source.is_some())
        {
            if let Some(name) = text
                .get(..10)
                .filter(|prefix| prefix.eq_ignore_ascii_case("parameter "))
                .and_then(|_| text.get(10..))
            {
                let name = if name.starts_with('"') {
                    match serde_json::from_str::<String>(name) {
                        Ok(name) => name,
                        Err(_) => {
                            return Some(CmdResult::ReportError(crate::tr!(
                                "block-action-targets",
                                "unique-point"
                            )))
                        }
                    }
                } else {
                    name.trim().to_owned()
                };
                let source = self.candidates[self.chosen.unwrap()].point_source;
                let matches = self
                    .points
                    .iter()
                    .filter(|(candidate, _, id)| {
                        source.is_some_and(|source| *id != source)
                            && candidate.eq_ignore_ascii_case(&name)
                    })
                    .map(|(_, handle, _)| *handle)
                    .collect::<Vec<_>>();
                return Some(if matches.len() == 1 {
                    self.picked.push(matches[0]);
                    CmdResult::NeedPoint
                } else {
                    CmdResult::ReportError(crate::tr!("block-action-targets", "unique-point"))
                });
            }
        }
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
    fn on_selection_marker_click(
        &mut self,
        scene: &crate::scene::Scene,
        position: iced::Point,
        context: PointPickContext,
        remove: bool,
        replace: bool,
    ) -> Option<String> {
        if !matches!(self.mode, Some(Mode::New | Mode::Add | Mode::Remove)) {
            return None;
        }
        let source = self.chosen.and_then(|i| self.candidates[i].point_source)?;
        let (handle, name) = pick_point_marker(
            scene,
            self.definition,
            |h| {
                self.points
                    .iter()
                    .any(|(_, point, id)| *point == h && *id != source)
            },
            position,
            context,
        )?;
        if !remove && replace {
            self.picked.retain(|h| {
                !self
                    .points
                    .iter()
                    .any(|(_, point, id)| point == h && *id != source)
            });
        }
        if remove {
            self.picked.retain(|h| *h != handle);
        } else if !self.picked.contains(&handle) {
            self.picked.push(handle);
        }
        Some(name)
    }
    fn selection_marker_selected(&self, handle: Handle) -> bool {
        self.chosen
            .and_then(|i| self.candidates[i].point_source)
            .is_some()
            && self.picked.contains(&handle)
            && self.points.iter().any(|(_, point, _)| *point == handle)
    }
    fn clear_selection_markers(&mut self) {
        self.picked
            .retain(|h| !self.points.iter().any(|(_, point, _)| point == h));
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
        if !matches!(self.mode, Some(Mode::New | Mode::Add | Mode::Remove)) {
            return Vec::new();
        }
        let Some(source) = self.chosen.and_then(|i| self.candidates[i].point_source) else {
            return Vec::new();
        };
        let hits = point_markers_in_region(
            scene,
            self.definition,
            |h| {
                self.points
                    .iter()
                    .any(|(_, point, id)| *point == h && *id != source)
            },
            polygon,
            context,
            crossing,
        );
        if !remove && replace && (geometry_hit || !hits.is_empty()) {
            self.picked.retain(|h| {
                !self
                    .points
                    .iter()
                    .any(|(_, point, id)| point == h && *id != source)
            });
        }
        for (handle, _) in &hits {
            if remove {
                self.picked.retain(|h| h != handle);
            } else if !self.picked.contains(handle) {
                self.picked.push(*handle);
            }
        }
        hits.into_iter().map(|(_, name)| name).collect()
    }
    fn is_selection_gathering(&self) -> bool {
        matches!(
            self.mode,
            Some(Mode::New | Mode::Add | Mode::Remove | Mode::RotateOnly)
        )
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        let source = self.candidates[self.chosen.unwrap()].point_source;
        self.picked.retain(|handle| {
            self.points
                .iter()
                .any(|(_, point, id)| point == handle && source.is_some_and(|source| *id != source))
        });
        self.picked.extend(handles);
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
