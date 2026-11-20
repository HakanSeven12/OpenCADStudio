//! The surface modelling commands. Each is a list of steps — objects gathered, picks on
//! surfaces, points, values, and a closing prompt whose options set values — collected here
//! into one [`SurfaceRequest`] the app carries out with Solidcore.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use document::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, SelectionEntity};

/// What a gathering step keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// Lines, arcs, circles, ellipses, polylines and splines.
    Curves,
    /// Surfaces.
    Surfaces,
    /// Surfaces, solids and regions.
    Bodies,
    /// Civil 3D COGO points.
    CogoPoints,
    /// COGO points and points.
    SurfacePoints,
    /// Civil 3D alignments.
    Alignments,
    /// Civil 3D assemblies.
    Assemblies,
    /// Civil 3D sample line groups.
    SampleLineGroups,
    /// Civil 3D TIN surfaces; a step of them may be passed with none selected.
    TinSurfaces,
    /// Lines, 2D and 3D polylines.
    Paths,
    /// Lines, 2D and 3D polylines; a step of them may be passed with none selected.
    OptionalPaths,
}

/// The Civil 3D class of `entity`, if it is one.
pub fn civil_class(entity: &EntityType) -> Option<&str> {
    match entity {
        EntityType::Extended(x) => match &x.data {
            document::entities::ExtendedEntityData::Arch(a) => Some(a.dxf_name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

impl Filter {
    fn keeps(self) -> fn(&EntityType) -> bool {
        match self {
            Filter::Curves => |e| {
                matches!(
                    e,
                    EntityType::Line(_)
                        | EntityType::Arc(_)
                        | EntityType::Circle(_)
                        | EntityType::Ellipse(_)
                        | EntityType::Spline(_)
                        | EntityType::LwPolyline(_)
                        | EntityType::Polyline2D(_)
                        | EntityType::Polyline3D(_)
                        | EntityType::Helix(_)
                )
            },
            Filter::Surfaces => |e| matches!(e, EntityType::Surface(_)),
            Filter::Bodies => |e| {
                matches!(e, EntityType::Surface(_) | EntityType::Solid3D(_) | EntityType::Region(_))
            },
            Filter::CogoPoints => |e| civil_class(e) == Some("AECC_COGO_POINT"),
            Filter::SurfacePoints => |e| matches!(e, EntityType::Point(_)) || civil_class(e) == Some("AECC_COGO_POINT"),
            Filter::Alignments => |e| civil_class(e) == Some("AECC_ALIGNMENT"),
            Filter::Assemblies => |e| civil_class(e) == Some("AECC_ASSEMBLY"),
            Filter::SampleLineGroups => |e| civil_class(e) == Some("AECC_SAMPLE_LINE_GROUP"),
            Filter::TinSurfaces => |e| civil_class(e) == Some("AECC_TIN_SURFACE"),
            Filter::Paths | Filter::OptionalPaths => |e| {
                matches!(e, EntityType::Line(_) | EntityType::LwPolyline(_) | EntityType::Polyline2D(_) | EntityType::Polyline3D(_))
            },
        }
    }
}

/// An option of a closing prompt: typing its keyword asks for the value it sets.
#[derive(Clone, Copy, Debug)]
pub struct Opt {
    pub label: &'static str,
    pub keyword: &'static str,
    /// The value it sets.
    pub key: &'static str,
    /// What it asks.
    pub prompt: &'static str,
    pub kind: OptKind,
}

/// What an option's value is.
#[derive(Clone, Copy, Debug)]
pub enum OptKind {
    /// A number, kept in `values`.
    Number,
    /// Text, kept in `texts`.
    Text,
    /// One of a list (typed in part; `?` lists it), kept in `texts`.
    Choice(fn() -> Vec<&'static str>),
}

/// One step of a surface command.
#[derive(Clone, Copy, Debug)]
pub enum Step {
    /// Objects gathered by any selection, kept by `filter`, into `objects[slot]`.
    Objects { prompt: &'static str, filter: Filter, slot: usize },
    /// Points picked on objects (an edge, a face, a place on a surface), into `picks[slot]`;
    /// `once` moves on after one pick, otherwise Enter does.
    Picks { prompt: &'static str, slot: usize, once: bool },
    /// A point, into `points`.
    Point { prompt: &'static str },
    /// Points, into `points`, until Enter.
    Points { prompt: &'static str },
    /// A number, kept under `key`, offered at its last value.
    Value { prompt: &'static str, key: &'static str, default: f64 },
    /// The closing prompt: Enter carries the command out; an option sets its value first.
    Accept { prompt: &'static str, options: &'static [Opt] },
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

impl SurfaceRequest {
    /// The value under `key`, or `default`.
    pub fn value(&self, key: &str, default: f64) -> f64 {
        self.values.get(key).copied().unwrap_or(default)
    }

    /// The text under `key`, if one was given.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.texts.get(key).map(String::as_str).filter(|t| !t.is_empty())
    }
}

/// The texts commands took, offered next time like their values.
fn remembered_texts() -> &'static Mutex<HashMap<(&'static str, &'static str), String>> {
    static TEXTS: OnceLock<Mutex<HashMap<(&'static str, &'static str), String>>> = OnceLock::new();
    TEXTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The entry of `choices` that `typed` names: the one equal to it, else the first that
/// holds every word of it (case ignored).
fn choose(choices: &[&'static str], typed: &str) -> Option<&'static str> {
    let typed = typed.to_lowercase();
    choices.iter().find(|c| c.to_lowercase() == typed).or_else(|| {
        choices.iter().find(|c| {
            let c = c.to_lowercase();
            typed.split_whitespace().all(|word| c.contains(word))
        })
    }).copied()
}

/// The last value each command took for each key, offered next time.
fn remembered() -> &'static Mutex<HashMap<(&'static str, &'static str), f64>> {
    static VALUES: OnceLock<Mutex<HashMap<(&'static str, &'static str), f64>>> = OnceLock::new();
    VALUES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember(command: &'static str, key: &'static str, value: f64) {
    remembered()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert((command, key), value);
}

fn recall(command: &'static str, key: &'static str, default: f64) -> f64 {
    remembered()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&(command, key))
        .copied()
        .unwrap_or(default)
}

/// A surface command running its steps.
pub struct SurfaceCommand {
    request: SurfaceRequest,
    steps: Vec<Step>,
    at: usize,
    /// An option chosen at the closing prompt, waiting for its value.
    asking: Option<Opt>,
}

impl SurfaceCommand {
    /// The command `name` with its `steps`; objects already selected fill the first gathering
    /// step they fit, which is then passed.
    pub fn new(name: &'static str, steps: Vec<Step>, preselection: Vec<(Handle, EntityType)>) -> Self {
        let mut command = Self {
            request: SurfaceRequest {
                command: name,
                ..SurfaceRequest::default()
            },
            steps,
            at: 0,
            asking: None,
        };
        if let Some(Step::Objects { filter, slot, .. }) = command.steps.first().copied() {
            let keeps = filter.keeps();
            let kept: Vec<Handle> = preselection
                .into_iter()
                .filter(|(_, e)| keeps(e))
                .map(|(h, _)| h)
                .collect();
            if !kept.is_empty() {
                command.request.objects[slot] = kept;
                command.at = 1;
            }
        }
        command
    }

    /// A value set before the first step (a mode the command was started with).
    pub fn with_value(mut self, key: &'static str, value: f64) -> Self {
        self.request.values.insert(key, value);
        self
    }

    /// Whether every step is passed: the request is ready as the command starts.
    pub fn ready(&self) -> bool {
        self.at >= self.steps.len()
    }

    /// The request, taken.
    pub fn take_request(&mut self) -> SurfaceRequest {
        std::mem::take(&mut self.request)
    }

    fn step(&self) -> Option<Step> {
        self.steps.get(self.at).copied()
    }

    fn advance(&mut self) -> CmdResult {
        self.at += 1;
        if self.at >= self.steps.len() {
            return CmdResult::Surfacing(Box::new(std::mem::take(&mut self.request)));
        }
        CmdResult::NeedPoint
    }

    fn set(&mut self, key: &'static str, value: f64) {
        remember(self.request.command, key, value);
        self.request.values.insert(key, value);
    }

    fn set_text(&mut self, key: &'static str, text: String) {
        remembered_texts()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((self.request.command, key), text.clone());
        self.request.texts.insert(key, text);
    }

    /// The text an option holds: given, else remembered, else a choice's first entry.
    fn current_text(&self, opt: &Opt) -> String {
        self.request.texts.get(opt.key).cloned().or_else(|| {
            remembered_texts()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&(self.request.command, opt.key))
                .cloned()
        }).or_else(|| match opt.kind {
            OptKind::Choice(choices) => choices().first().map(|c| c.to_string()),
            _ => None,
        }).unwrap_or_default()
    }

    fn current(&self, key: &'static str, default: f64) -> f64 {
        self.request
            .values
            .get(key)
            .copied()
            .unwrap_or_else(|| recall(self.request.command, key, default))
    }
}

fn number(text: &str) -> Option<f64> {
    crate::entities::common::parse_typed_length(text.trim()).filter(|v| v.is_finite())
}

impl CadCommand for SurfaceCommand {
    fn name(&self) -> &'static str {
        self.request.command
    }

    fn prompt(&self) -> String {
        if let Some(opt) = self.asking {
            let current = match opt.kind {
                OptKind::Number => crate::entities::common::format_length(self.current(opt.key, 0.0)),
                OptKind::Text | OptKind::Choice(_) => self.current_text(&opt),
            };
            return format!("{} <{}>:", crate::t!(opt.prompt), current);
        }
        match self.step() {
            Some(
                Step::Objects { prompt, .. }
                | Step::Picks { prompt, .. }
                | Step::Point { prompt }
                | Step::Points { prompt }
                | Step::Accept { prompt, .. },
            ) => {
                crate::t!(prompt).into_owned()
            }
            Some(Step::Value { prompt, key, default }) => format!(
                "{} <{}>:",
                crate::t!(prompt),
                crate::entities::common::format_length(self.current(key, default))
            ),
            None => String::new(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match (self.asking, self.step()) {
            (None, Some(Step::Accept { options, .. })) => {
                options.iter().map(|o| CmdOption::new(o.label, o.keyword)).collect()
            }
            _ => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        self.asking.is_some() || matches!(self.step(), Some(Step::Value { .. } | Step::Accept { .. }))
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if let Some(opt) = self.asking {
            match opt.kind {
                OptKind::Number => {
                    let value = number(text)?;
                    self.set(opt.key, value);
                }
                OptKind::Text => self.set_text(opt.key, text.to_string()),
                OptKind::Choice(choices) => {
                    let choices = choices();
                    if text == "?" {
                        return Some(CmdResult::ReportMessage(choices.join("\n")));
                    }
                    match choose(&choices, text) {
                        Some(choice) => self.set_text(opt.key, choice.to_string()),
                        None => return Some(CmdResult::ReportError(crate::t!("No such entry; type ? to list them.").into_owned())),
                    }
                }
            }
            self.asking = None;
            return Some(CmdResult::NeedPoint);
        }
        match self.step()? {
            Step::Value { key, .. } => match number(text) {
                Some(value) => {
                    self.set(key, value);
                    Some(self.advance())
                }
                None => Some(CmdResult::ReportError(crate::t!("Requires a number.").into_owned())),
            },
            Step::Accept { options, .. } => {
                let typed = text.to_ascii_uppercase();
                match options.iter().find(|o| o.keyword.starts_with(&typed) || typed.starts_with(o.keyword)) {
                    Some(opt) => {
                        self.asking = Some(*opt);
                        Some(CmdResult::NeedPoint)
                    }
                    None => Some(CmdResult::ReportError(crate::t!("Invalid option keyword.").into_owned())),
                }
            }
            _ => None,
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step() {
            Some(Step::Point { .. }) => {
                self.request.points.push(point);
                self.advance()
            }
            Some(Step::Points { .. }) => {
                self.request.points.push(point);
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if let Some(opt) = self.asking.take() {
            match opt.kind {
                OptKind::Number => {
                    let value = self.current(opt.key, 0.0);
                    self.set(opt.key, value);
                }
                OptKind::Text | OptKind::Choice(_) => {
                    let text = self.current_text(&opt);
                    self.set_text(opt.key, text);
                }
            }
            return CmdResult::NeedPoint;
        }
        // PLANESURF: Enter at its first corner selects the objects to make planar surfaces of.
        if self.request.command == "PLANESURF" && self.at == 0 {
            self.steps = vec![Step::Objects { prompt: "Select objects:", filter: Filter::Curves, slot: 0 }];
            return CmdResult::NeedPoint;
        }
        match self.step() {
            // Enter with nothing selected ends the command.
            Some(Step::Objects { slot, filter, .. })
                if self.request.objects[slot].is_empty() && !matches!(filter, Filter::TinSurfaces | Filter::CogoPoints | Filter::OptionalPaths) =>
            {
                CmdResult::Done
            }
            Some(Step::Points { .. }) if self.request.points.is_empty() => CmdResult::Done,
            Some(Step::Picks { slot, .. }) if self.request.picks[slot].is_empty() => {
                CmdResult::Measurement(crate::t!("Nothing picked.").into_owned())
            }
            Some(Step::Value { key, default, .. }) => {
                let value = self.current(key, default);
                self.set(key, value);
                self.advance()
            }
            Some(_) => self.advance(),
            None => CmdResult::NeedPoint,
        }
    }

    fn is_selection_gathering(&self) -> bool {
        self.asking.is_none() && matches!(self.step(), Some(Step::Objects { .. }))
    }

    fn selection_forces_add(&self) -> bool {
        self.is_selection_gathering()
    }

    fn selection_entity_filter(&self) -> Option<fn(&EntityType) -> bool> {
        match self.step() {
            Some(Step::Objects { filter, .. }) => Some(filter.keeps()),
            _ => None,
        }
    }

    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) {
        if let Some(Step::Objects { filter, slot, .. }) = self.step() {
            let keeps = filter.keeps();
            self.request.objects[slot] = entities
                .into_iter()
                .filter(|e| keeps(&e.entity))
                .map(|e| e.handle)
                .collect();
        }
    }

    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn needs_entity_pick(&self) -> bool {
        self.asking.is_none() && matches!(self.step(), Some(Step::Picks { .. }))
    }

    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    fn entity_pick_uses_surface_point(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if let Some(Step::Picks { slot, once, .. }) = self.step() {
            self.request.picks[slot].push((handle, point));
            if once {
                return self.advance();
            }
        }
        CmdResult::NeedPoint
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.asking.take().is_some() {
            return Some(CmdResult::NeedPoint);
        }
        if let Some(Step::Picks { slot, .. }) = self.step() {
            if self.request.picks[slot].pop().is_some() {
                return Some(CmdResult::NeedPoint);
            }
        }
        if let Some(Step::Points { .. }) = self.step() {
            if self.request.points.pop().is_some() {
                return Some(CmdResult::NeedPoint);
            }
        }
        if self.at > 0 {
            self.at -= 1;
            return Some(CmdResult::NeedPoint);
        }
        None
    }
}
