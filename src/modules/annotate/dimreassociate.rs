// DIMREASSOCIATE — after the selection, each dimension's definition points
// are asked in turn. A point (or a picked object) moves that point onto the
// object and associates it; Enter (<next>) leaves the point as it is.

use codec::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, ReassociateTarget};
use crate::scene::model::wire_model::WireModel;

/// Which definition points a dimension asks for.
#[derive(Clone, Copy, PartialEq)]
pub enum ReassociateKind {
    /// Linear and aligned: two extension line origins, or one object.
    Linear,
    /// Three-point angular: vertex and two endpoints, or an arc or circle.
    Angular3,
    /// Two-line angular: the two lines.
    Angular2,
    /// Radius and diameter: the arc or circle.
    Radial,
    /// Ordinate: the feature location.
    Ordinate,
}

impl ReassociateKind {
    fn steps(self) -> usize {
        match self {
            Self::Linear | Self::Angular2 => 2,
            Self::Angular3 => 3,
            Self::Radial | Self::Ordinate => 1,
        }
    }
}

pub struct DimReassociateCommand {
    items: Vec<(Handle, ReassociateKind)>,
    /// Each dimension's definition points, for the X shown at the one asked.
    points: Vec<Vec<DVec3>>,
    item: usize,
    step: usize,
    /// "Select object" / "Select arc or circle" was chosen at the first prompt.
    select_object: bool,
    picked: Option<EntityType>,
    /// World units per screen pixel of the view, so the X keeps one screen
    /// size; 0 when unknown.
    world_per_pixel: f64,
}

/// Half the X across, in screen pixels.
const X_HALF_PX: f64 = 13.2;

impl DimReassociateCommand {
    pub fn new(items: Vec<(Handle, ReassociateKind)>, points: Vec<Vec<DVec3>>) -> Self {
        Self {
            items,
            points,
            item: 0,
            step: 0,
            select_object: false,
            picked: None,
            world_per_pixel: 0.0,
        }
    }

    fn kind(&self) -> Option<ReassociateKind> {
        self.items.get(self.item).map(|(_, kind)| *kind)
    }

    /// The next prompt, or the end once every dimension has been asked.
    fn advance(&mut self, whole_dimension: bool) -> bool {
        self.select_object = false;
        let steps = self.kind().map_or(0, ReassociateKind::steps);
        if whole_dimension || self.step + 1 >= steps {
            self.item += 1;
            self.step = 0;
        } else {
            self.step += 1;
        }
        self.item < self.items.len()
    }

    fn apply(&mut self, target: ReassociateTarget, whole_dimension: bool) -> CmdResult {
        let dimension = self.items[self.item].0;
        let more = self.advance(whole_dimension);
        CmdResult::ReassociateDimension {
            dimension,
            target,
            last: !more,
        }
    }

    /// Whether the current prompt picks an object rather than a point.
    fn picks_object(&self) -> bool {
        self.select_object
            || matches!(
                self.kind(),
                Some(ReassociateKind::Radial | ReassociateKind::Angular2)
            )
    }
}

impl CadCommand for DimReassociateCommand {
    fn name(&self) -> &'static str {
        "DIMREASSOCIATE"
    }

    fn groups_undo(&self) -> bool {
        true
    }

    fn set_world_per_pixel(&mut self, world_per_pixel: f64) {
        self.world_per_pixel = world_per_pixel;
    }

    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> {
        self.persistent_preview_wires()
    }

    /// An X on the definition point being asked.
    fn persistent_preview_wires(&self) -> Vec<WireModel> {
        if self.picks_object() {
            return Vec::new();
        }
        let Some(points) = self.points.get(self.item) else {
            return Vec::new();
        };
        let Some(&at) = points.get(self.step) else {
            return Vec::new();
        };
        // A fixed screen size; without a view scale, a twentieth of the
        // dimension's own extent.
        let half = if self.world_per_pixel > 0.0 {
            X_HALF_PX * self.world_per_pixel
        } else {
            let extent = points
                .iter()
                .flat_map(|a| points.iter().map(move |b| a.distance(*b)))
                .fold(0.0_f64, f64::max);
            (extent / 20.0).max(1.0e-6)
        };
        let corner = |dx: f64, dy: f64| [at.x + dx * half, at.y + dy * half, at.z];
        vec![WireModel::solid_f64(
            "dimreassociate_x".into(),
            vec![
                corner(-1.0, -1.0),
                corner(1.0, 1.0),
                [f64::NAN, f64::NAN, f64::NAN],
                corner(-1.0, 1.0),
                corner(1.0, -1.0),
            ],
            [4.0 / 255.0, 116.0 / 255.0, 230.0 / 255.0, 1.0],
            false,
        )]
    }

    fn prompt(&self) -> String {
        if self.select_object {
            return crate::t!("DIMREASSOCIATE  Select object:").into_owned();
        }
        let text = match (self.kind(), self.step) {
            (Some(ReassociateKind::Linear), 0) => {
                "DIMREASSOCIATE  Specify first extension line origin or [Select object] <next>:"
            }
            (Some(ReassociateKind::Linear), _) => {
                "DIMREASSOCIATE  Specify second extension line origin <next>:"
            }
            (Some(ReassociateKind::Angular3), 0) => {
                "DIMREASSOCIATE  Specify angle vertex or [Select arc or circle] <next>:"
            }
            (Some(ReassociateKind::Angular3), 1) => {
                "DIMREASSOCIATE  Specify first angle endpoint <next>:"
            }
            (Some(ReassociateKind::Angular3), _) => {
                "DIMREASSOCIATE  Specify second angle endpoint <next>:"
            }
            (Some(ReassociateKind::Angular2), 0) => "DIMREASSOCIATE  Select first line <next>:",
            (Some(ReassociateKind::Angular2), _) => "DIMREASSOCIATE  Select second line <next>:",
            (Some(ReassociateKind::Radial), _) => "DIMREASSOCIATE  Select arc or circle <next>:",
            (Some(ReassociateKind::Ordinate), _) => {
                "DIMREASSOCIATE  Specify feature location <next>:"
            }
            (None, _) => "",
        };
        crate::t!(text).into_owned()
    }

    fn options(&self) -> Vec<CmdOption> {
        match (self.kind(), self.step, self.select_object) {
            (Some(ReassociateKind::Linear), 0, false) => {
                vec![CmdOption::new("Select object", "S")]
            }
            (Some(ReassociateKind::Angular3), 0, false) => {
                vec![CmdOption::new("Select arc or circle", "S")]
            }
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn needs_entity_pick(&self) -> bool {
        self.picks_object()
    }

    fn inject_before_entity_pick(&self) -> bool {
        self.picks_object()
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _point: DVec3) -> CmdResult {
        if handle.is_null() || self.picked.take().is_none() {
            return CmdResult::NeedPoint;
        }
        let (kind, step, whole) = (self.kind(), self.step, self.select_object);
        match kind {
            Some(ReassociateKind::Linear) if whole => {
                self.apply(ReassociateTarget::Ends(handle), true)
            }
            Some(ReassociateKind::Angular3) if whole => {
                self.apply(ReassociateTarget::Arc(handle), true)
            }
            Some(ReassociateKind::Angular2) => {
                self.apply(ReassociateTarget::Line { index: step, object: handle }, false)
            }
            Some(ReassociateKind::Radial) => self.apply(ReassociateTarget::Radial(handle), true),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.picks_object() {
            return CmdResult::NeedPoint;
        }
        let index = self.step;
        self.apply(ReassociateTarget::Point { index, point }, false)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let word = text.trim().trim_start_matches('_').to_ascii_uppercase();
        let selectable = self.step == 0
            && !self.select_object
            && matches!(
                self.kind(),
                Some(ReassociateKind::Linear | ReassociateKind::Angular3)
            );
        if selectable && matches!(word.as_str(), "S" | "SELECT") {
            self.select_object = true;
            return Some(CmdResult::NeedPoint);
        }
        None
    }

    /// <next>: this point stays as it is.
    fn on_enter(&mut self) -> CmdResult {
        if self.select_object {
            self.select_object = false;
            return CmdResult::NeedPoint;
        }
        if self.advance(false) {
            CmdResult::NeedPoint
        } else {
            CmdResult::Measurement(String::new())
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
