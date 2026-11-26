//! The picks the Create Points tool windows step aside for: a corridor when
//! the drawing holds several, and the window of Create Blocks from COGO
//! Points' "Define window".

use crate::command::{CadCommand, CmdResult, InputKind};
use document::{EntityType, Handle};
use glam::DVec3;

/// Hands the picked corridor's handle (hex) to the corridor points window.
pub const CORRIDOR_PICKED: &str = "_CORRIDORPOINTS_PICKED";
/// Hands the window's corners (`x1 y1 x2 y2`) back to the blocks window;
/// alone, the window comes back unchanged.
pub const WINDOW_PICKED: &str = "_POINTBLOCKS_WINDOW";

/// "Select a corridor:" of Create COGO Points from Corridor.
#[derive(Default)]
pub struct CorridorPickCommand {
    picked: Option<bool>,
}

impl CadCommand for CorridorPickCommand {
    fn name(&self) -> &'static str {
        "CREATEPOINTSFROMCORRIDOR"
    }

    fn prompt(&self) -> String {
        crate::t!("Select a corridor:").into_owned()
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(matches!(&entity, EntityType::Extended(x) if matches!(&x.data, document::entities::ExtendedEntityData::Arch(a) if a.dxf_name == "AECC_CORRIDOR")));
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        match self.picked.take() {
            Some(true) => CmdResult::Dispatch(format!("{CORRIDOR_PICKED} {:X}", handle.value())),
            _ => CmdResult::ReportError(crate::t!("Select a corridor.").into_owned()),
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// The two corners of "Define window".
#[derive(Default)]
pub struct WindowPickCommand {
    first: Option<DVec3>,
}

impl CadCommand for WindowPickCommand {
    fn name(&self) -> &'static str {
        "CREATEBLOCKSFROMPOINTS"
    }

    fn prompt(&self) -> String {
        match self.first {
            None => crate::t!("Specify first corner:"),
            Some(_) => crate::t!("Specify opposite corner:"),
        }
        .into_owned()
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.first.take() {
            None => {
                self.first = Some(pt);
                CmdResult::NeedPoint
            }
            Some(a) => CmdResult::Dispatch(format!("{WINDOW_PICKED} {} {} {} {}", a.x, a.y, pt.x, pt.y)),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch(WINDOW_PICKED.into())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch(WINDOW_PICKED.into())
    }
}

/// Hands the picked alignment (hex), or the station given (`start|end s`),
/// back to the corridor points window; `cancel` writes `*Cancel*` first;
/// alone, the window comes back unchanged.
pub const CORRIDOR_RANGE_PICKED: &str = "_CORRIDORPOINTS_RANGE";

/// The baseline or a station of Create COGO Points from Corridor, picked in
/// the drawing.
pub struct CorridorRangePickCommand {
    /// None: the baseline's alignment; Some(false/true): the start/end station.
    pub station: Option<bool>,
    picked: Option<bool>,
    /// The baseline's alignment and its station range (station picks).
    alignment: Option<EntityType>,
    range: (f64, f64),
}

impl CorridorRangePickCommand {
    pub fn new(station: Option<bool>, alignment: Option<EntityType>, range: (f64, f64)) -> Self {
        CorridorRangePickCommand { station, picked: None, alignment, range }
    }

    /// A station inside the baseline's range goes back to the window; any
    /// other is ignored and the prompt asks again (measured).
    fn station_given(&self, end: bool, s: f64) -> CmdResult {
        let (lo, hi) = self.range;
        if s < lo - 1e-9 || s > hi + 1e-9 {
            return CmdResult::NeedPoint;
        }
        CmdResult::Dispatch(format!("{CORRIDOR_RANGE_PICKED} {} {s}", if end { "end" } else { "start" }))
    }

    /// The station of `pt` on the alignment, when it lies across from it
    /// (not past either end).
    fn station_of(&self, pt: DVec3) -> Option<f64> {
        let e = self.alignment.as_ref()?;
        let curve = crate::modules::civil::point_intersect::alignment_curve(e)?;
        let ocs_geom::geom2d::Curve::Polyline(pl) = &curve else { return None };
        let v: Vec<glam::DVec2> = pl.vertices.iter().map(|v| glam::DVec2::from(v.position)).collect();
        let p = pt.truncate();
        // ponytail: the ends' directions are their chords (an end arc's
        // tangent differs); use the tangent if picks past a curved end matter.
        let past = |a: glam::DVec2, b: glam::DVec2| (p - a).dot(a - b) > 1e-9;
        if v.len() < 2 || past(v[0], v[1]) || past(v[v.len() - 1], v[v.len() - 2]) {
            return None;
        }
        crate::modules::civil::point_alignment::station_on(e, [pt.x, pt.y])
    }
}

impl CadCommand for CorridorRangePickCommand {
    fn name(&self) -> &'static str {
        "CREATEPOINTSFROMCORRIDOR"
    }

    fn prompt(&self) -> String {
        match self.station {
            None => crate::t!("Select baseline.:"),
            Some(_) => crate::t!("Specify station along baseline:"),
        }
        .into_owned()
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn needs_entity_pick(&self) -> bool {
        self.station.is_none()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let typed = text.trim();
        // A typed coordinate is a pick, handled as a point.
        let coordinate = matches!(typed.split(',').count(), 2 | 3) && typed.split(',').all(|v| v.trim().parse::<f64>().is_ok());
        if typed.is_empty() || coordinate {
            return None;
        }
        Some(match self.station {
            None => CmdResult::ReportError(format!("{}\n{}", crate::t!("*Invalid selection*"), crate::t!("Expects a point or Last"))),
            Some(end) => match typed.parse::<f64>().ok().filter(|v| v.is_finite()) {
                Some(s) => self.station_given(end, s),
                None => CmdResult::ReportError(crate::t!("Invalid input").into_owned()),
            },
        })
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(matches!(&entity, EntityType::Extended(x) if matches!(&x.data, document::entities::ExtendedEntityData::Arch(a) if a.dxf_name == "AECC_ALIGNMENT")));
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        // Measured: another object brings the window back unchanged.
        match self.picked.take() {
            Some(true) => CmdResult::Dispatch(format!("{CORRIDOR_RANGE_PICKED} {:X}", handle.value())),
            Some(false) => CmdResult::Dispatch(CORRIDOR_RANGE_PICKED.into()),
            None => CmdResult::ReportError(crate::t!("Nothing selected.").into_owned()),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.station {
            Some(end) => match self.station_of(pt) {
                Some(s) => self.station_given(end, s),
                None => CmdResult::NeedPoint,
            },
            None => CmdResult::ReportError(crate::t!("Nothing selected.").into_owned()),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch(CORRIDOR_RANGE_PICKED.into())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch(format!("{CORRIDOR_RANGE_PICKED} cancel"))
    }
}
