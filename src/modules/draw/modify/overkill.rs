use crate::command::{CadCommand, CmdOption, CmdResult};
use document::{EntityType, Handle};
use glam::DVec3;

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub tolerance: f64,
    pub ignore: u16,
    pub optimize: bool,
    pub overlap: bool,
    pub end_to_end: bool,
    pub associativity: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            tolerance: 1e-6,
            ignore: 0,
            optimize: true,
            overlap: true,
            end_to_end: true,
            associativity: true,
        }
    }
}
impl Settings {
    /// Remembers these settings for the next run and returns the internal
    /// command that applies them to the current selection.
    pub fn apply_command(self) -> String {
        *remembered().lock().unwrap_or_else(|e| e.into_inner()) = self;
        format!(
            "OVERKILL_APPLY {} {} {} {} {} {}",
            self.tolerance,
            self.ignore,
            u8::from(self.optimize),
            u8::from(self.overlap),
            u8::from(self.end_to_end),
            u8::from(self.associativity)
        )
    }
}
pub fn remembered() -> &'static std::sync::Mutex<Settings> {
    static SETTINGS: std::sync::OnceLock<std::sync::Mutex<Settings>> = std::sync::OnceLock::new();
    SETTINGS.get_or_init(|| std::sync::Mutex::new(Settings::default()))
}
enum Step {
    Selection,
    Options,
    Ignore,
    Tolerance,
    Optimize,
    Overlap,
    EndToEnd,
    Associativity,
}
pub struct OverkillCommand {
    selected: Vec<Handle>,
    step: Step,
    settings: Settings,
    /// OVERKILL hands the selection to the settings dialog; -OVERKILL asks
    /// for the settings on the command line.
    dialog: bool,
}
impl OverkillCommand {
    pub fn new(selected: Vec<Handle>, dialog: bool) -> Self {
        let step = if selected.is_empty() {
            Step::Selection
        } else {
            Step::Options
        };
        Self {
            selected,
            step,
            settings: *remembered().lock().unwrap_or_else(|e| e.into_inner()),
            dialog,
        }
    }
    fn finish(&self) -> CmdResult {
        CmdResult::Relaunch(self.settings.apply_command(), self.selected.clone())
    }
}
impl CadCommand for OverkillCommand {
    fn name(&self) -> &'static str {
        "OVERKILL"
    }
    fn prompt(&self) -> String {
        match self.step {
            Step::Selection => "Select objects:".into(),
            Step::Options => format!("Tolerance={}  Ignore={}  Optimize={}  Overlap={}  End-to-end={}\nEnter an option [Done/Ignore/tOlerance/optimize Plines/combine parTial overlap/combine Endtoend/Associativity] <Done>:", self.settings.tolerance, self.settings.ignore, self.settings.optimize, self.settings.overlap, self.settings.end_to_end),
            Step::Ignore => "Properties to ignore [None/All/Color/LAyer/Ltype/ltScale/LWeight/Thickness/TRansparency/plotSTyle/Material]:".into(),
            Step::Tolerance => format!("Specify tolerance <{}>:", self.settings.tolerance),
            Step::Optimize => format!("Optimize segments within polylines [Yes/No] <{}>:",if self.settings.optimize{"Yes"}else{"No"}),
            Step::Overlap => format!("Combine partially overlapping objects [Yes/No] <{}>:",if self.settings.overlap{"Yes"}else{"No"}),
            Step::EndToEnd => format!("Combine end-to-end objects [Yes/No] <{}>:",if self.settings.end_to_end{"Yes"}else{"No"}),
            Step::Associativity => format!("Maintain associative objects [Yes/No] <{}>:",if self.settings.associativity{"Yes"}else{"No"}),
        }
    }
    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Options => [
                ("Done", "D"),
                ("Ignore", "I"),
                ("Tolerance", "O"),
                ("Optimize polylines", "P"),
                ("Combine overlap", "T"),
                ("Combine end-to-end", "E"),
                ("Associativity", "A"),
            ]
            .into_iter()
            .map(|(l, k)| CmdOption::new(l, k))
            .collect(),
            Step::Optimize | Step::Overlap | Step::EndToEnd | Step::Associativity => {
                vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")]
            }
            _ => Vec::new(),
        }
    }
    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, Step::Selection)
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            return CmdResult::Cancel;
        }
        if self.dialog {
            // Re-entering OVERKILL with a selection opens the settings dialog.
            return CmdResult::Relaunch("OVERKILL".into(), handles);
        }
        self.selected = handles;
        self.step = Step::Options;
        CmdResult::NeedPoint
    }
    fn wants_text_input(&self) -> bool {
        !self.is_selection_gathering()
    }
    fn on_point(&mut self, _: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Selection if self.selected.is_empty() => CmdResult::Cancel,
            Step::Options => self.finish(),
            _ => {
                self.step = Step::Options;
                CmdResult::NeedPoint
            }
        }
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let k = text.trim().to_ascii_uppercase();
        match self.step {
            Step::Options => match k.as_str() {
                "D" | "DONE" => return Some(self.finish()),
                "I" | "IGNORE" => self.step = Step::Ignore,
                "O" | "TOLERANCE" => self.step = Step::Tolerance,
                "P" | "PLINES" => self.step = Step::Optimize,
                "T" | "PARTIAL" => self.step = Step::Overlap,
                "E" | "ENDTOEND" => self.step = Step::EndToEnd,
                "A" | "ASSOCIATIVITY" => self.step = Step::Associativity,
                _ => {}
            },
            Step::Ignore => {
                let flag = match k.as_str() {
                    "N" | "NONE" => {
                        self.settings.ignore = 0;
                        0
                    }
                    "A" | "ALL" => 511,
                    "C" | "COLOR" => 1,
                    "LA" | "LAYER" => 2,
                    "L" | "LTYPE" => 4,
                    "S" | "LTSCALE" => 8,
                    "LW" | "LWEIGHT" => 16,
                    "T" | "THICKNESS" => 32,
                    "TR" | "TRANSPARENCY" => 64,
                    "ST" | "PLOTSTYLE" => 128,
                    "M" | "MATERIAL" => 256,
                    _ => return Some(CmdResult::NeedPoint),
                };
                self.settings.ignore |= flag;
                self.step = Step::Options;
            }
            Step::Tolerance => {
                if let Ok(v) = k.parse::<f64>() {
                    if v.is_finite() && v >= 0.0 {
                        self.settings.tolerance = v;
                        self.step = Step::Options;
                    }
                }
            }
            Step::Optimize | Step::Overlap | Step::EndToEnd | Step::Associativity => {
                let yes = match k.as_str() {
                    "Y" | "YES" => true,
                    "N" | "NO" => false,
                    _ => return Some(CmdResult::NeedPoint),
                };
                match self.step {
                    Step::Optimize => self.settings.optimize = yes,
                    Step::Overlap => self.settings.overlap = yes,
                    Step::EndToEnd => self.settings.end_to_end = yes,
                    Step::Associativity => self.settings.associativity = yes,
                    _ => {}
                }
                self.step = Step::Options;
            }
            _ => {}
        }
        Some(CmdResult::NeedPoint)
    }
}

/// The entity without its identity and storage fields (handle, owner, reactors,
/// extension dictionary, raw DWG bytes) and without the ignored properties, so
/// two copies of the same geometry compare equal however they were created.
pub fn normalized(entity: &EntityType, ignore: u16) -> EntityType {
    let mut e = entity.clone();
    let common = e.common_mut();
    let default = document::entities::EntityCommon::new();
    common.handle = Handle::NULL;
    common.owner_handle = Handle::NULL;
    common.reactors.clear();
    common.xdictionary_handle = None;
    common.layer_handle = None;
    common.entity_mode = None;
    common.r12_metadata = None;
    common.graphic_data = None;
    common.raw_record = None;
    common.has_ds_data = false;
    if !common.linetype.is_empty() {
        common.linetype_handle = None;
    }
    if ignore & 1 != 0 {
        common.color = default.color;
        common.color_name = None;
        common.color_book_handle = None;
    }
    if ignore & 2 != 0 {
        common.layer.clear();
    }
    if ignore & 4 != 0 {
        common.linetype.clear();
        common.linetype_handle = None;
    }
    if ignore & 8 != 0 {
        common.linetype_scale = 1.0;
    }
    if ignore & 16 != 0 {
        common.line_weight = default.line_weight;
    }
    if ignore & 64 != 0 {
        common.transparency = default.transparency;
    }
    if ignore & 128 != 0 {
        common.plotstyle_flags = 0;
        common.plotstyle_handle = None;
    }
    if ignore & 256 != 0 {
        common.material_flags = 0;
        common.material_handle = None;
    }
    if ignore & 32 != 0 {
        match &mut e {
            EntityType::Line(v) => v.thickness = 0.0,
            EntityType::Circle(v) => v.thickness = 0.0,
            EntityType::Arc(v) => v.thickness = 0.0,
            EntityType::LwPolyline(v) => v.thickness = 0.0,
            _ => {}
        }
    }
    e
}

pub fn optimize(entity: &mut EntityType, tolerance: f64) {
    if let EntityType::LwPolyline(p) = entity {
        // Width changes and bulged spans carry geometry that must be retained.
        if p.vertices
            .iter()
            .any(|v| v.bulge != 0.0 || v.start_width != 0.0 || v.end_width != 0.0)
        {
            return;
        }
        let points: Vec<_> = p
            .vertices
            .iter()
            .map(|v| [v.location.x, v.location.y, p.elevation])
            .collect();
        let indices = kernel::space::simplify_linear_chain(&points, tolerance);
        if indices.len() >= if p.is_closed { 3 } else { 2 } {
            p.vertices = indices.into_iter().map(|i| p.vertices[i].clone()).collect();
        }
    }
}

/// Comparison data for one candidate: the normalized entity as JSON (so every
/// entity type can be compared with floats inside the tolerance), the same for
/// its supporting geometry (a line without its end points, an arc as its full
/// circle), and its X extent for the sweep that limits the pairs compared.
/// The full form is built on first use: large bodies (solids, meshes) that
/// never meet a same-typed neighbour are not serialized.
struct Entry {
    full: std::cell::OnceCell<Option<serde_json::Value>>,
    support: Option<serde_json::Value>,
    lo: f64,
    hi: f64,
}

impl Entry {
    fn full(&self, entity: &EntityType, ignore: u16) -> &Option<serde_json::Value> {
        self.full
            .get_or_init(|| serde_json::to_value(normalized(entity, ignore)).ok())
    }
}

fn entry(entity: &EntityType, ignore: u16) -> Entry {
    let support = match entity {
        EntityType::Line(line) => {
            let mut line = line.clone();
            line.start = document::types::Vector3::new(0.0, 0.0, 0.0);
            line.end = line.start;
            Some(EntityType::Line(line))
        }
        EntityType::Arc(arc) => Some(EntityType::Circle(circle_of(arc))),
        EntityType::Circle(_) => Some(entity.clone()),
        _ => None,
    };
    let bounds = entity.as_entity().bounding_box();
    let (lo, hi) = if bounds.min.x.is_finite() && bounds.max.x.is_finite() {
        (bounds.min.x, bounds.max.x)
    } else {
        (f64::NEG_INFINITY, f64::INFINITY)
    };
    Entry {
        full: std::cell::OnceCell::new(),
        support: support.and_then(|s| serde_json::to_value(normalized(&s, ignore)).ok()),
        lo,
        hi,
    }
}

fn circle_of(arc: &document::entities::Arc) -> document::entities::Circle {
    let mut circle = document::entities::Circle::new();
    circle.common = arc.common.clone();
    circle.center = arc.center;
    circle.normal = arc.normal;
    circle.radius = arc.radius;
    circle.thickness = arc.thickness;
    circle
}

/// Structural equality where floating-point values may differ by `tolerance`.
fn close(a: &serde_json::Value, b: &serde_json::Value, tolerance: f64) -> bool {
    use serde_json::Value as V;
    match (a, b) {
        (V::Number(x), V::Number(y)) if x.is_f64() || y.is_f64() => {
            matches!((x.as_f64(), y.as_f64()), (Some(x), Some(y)) if (x - y).abs() <= tolerance)
        }
        (V::Array(x), V::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| close(x, y, tolerance))
        }
        (V::Object(x), V::Object(y)) => {
            x.len() == y.len()
                && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| close(v, w, tolerance)))
        }
        _ => a == b,
    }
}

fn same(a: &Option<serde_json::Value>, b: &Option<serde_json::Value>, tolerance: f64) -> bool {
    matches!((a, b), (Some(a), Some(b)) if close(a, b, tolerance))
}

enum Outcome {
    /// `second` is redundant.
    DropSecond,
    /// `first` is redundant (an arc lying on a circle).
    DropFirst,
    /// `first` becomes the union and `second` is redundant.
    Merge(EntityType),
}

fn compare(
    first: &EntityType,
    second: &EntityType,
    a: &Entry,
    b: &Entry,
    s: &Settings,
) -> Option<Outcome> {
    use kernel::space::arc_union::{circular_arc_union, ArcUnionKind, CircularArc};
    use kernel::space::LineUnionKind;
    let tolerance = s.tolerance;
    let circular = |e: &EntityType| matches!(e, EntityType::Arc(_) | EntityType::Circle(_));
    if std::mem::discriminant(first) != std::mem::discriminant(second)
        && !(circular(first) && circular(second))
    {
        return None;
    }
    if same(a.full(first, s.ignore), b.full(second, s.ignore), tolerance) {
        return Some(Outcome::DropSecond);
    }
    if !same(&a.support, &b.support, tolerance) {
        return None;
    }
    let allowed = |duplicate: bool, overlap: bool| {
        duplicate || if overlap { s.overlap } else { s.end_to_end }
    };
    match (first, second) {
        (EntityType::Circle(_), EntityType::Arc(_)) => Some(Outcome::DropSecond),
        (EntityType::Arc(_), EntityType::Circle(_)) => Some(Outcome::DropFirst),
        (EntityType::Line(l), EntityType::Line(r)) => {
            let point = |p: document::types::Vector3| [p.x, p.y, p.z];
            let union = kernel::space::line_union(
                [point(l.start), point(l.end)],
                [point(r.start), point(r.end)],
                tolerance,
            )?;
            if !allowed(
                union.kind == LineUnionKind::Duplicate,
                union.kind == LineUnionKind::Overlap,
            ) {
                return None;
            }
            let mut line = l.clone();
            line.start = document::types::Vector3::new(union.start[0], union.start[1], union.start[2]);
            line.end = document::types::Vector3::new(union.end[0], union.end[1], union.end[2]);
            Some(Outcome::Merge(EntityType::Line(line)))
        }
        (EntityType::Arc(l), EntityType::Arc(r)) => {
            // The supports already match within the tolerance; give both the
            // first arc's circle so the union sees one identical frame.
            let arc = |v: &document::entities::Arc| CircularArc {
                center: [l.center.x, l.center.y, l.center.z],
                normal: [l.normal.x, l.normal.y, l.normal.z],
                radius: l.radius,
                start: v.start_angle,
                end: v.end_angle,
            };
            let union = circular_arc_union(arc(l), arc(r), tolerance)?;
            if !allowed(
                union.kind == ArcUnionKind::Duplicate,
                union.kind == ArcUnionKind::Overlap,
            ) {
                return None;
            }
            Some(Outcome::Merge(if union.full_circle {
                EntityType::Circle(circle_of(l))
            } else {
                let mut arc = l.clone();
                arc.start_angle = union.start;
                arc.end_angle = union.end;
                EntityType::Arc(arc)
            }))
        }
        _ => None,
    }
}

/// Finds redundant objects among `items` and rewrites the survivors in place.
/// Returns the handles whose entity changed and the handles to erase. Of two
/// matching objects the older one (lower handle) is kept.
pub fn run(
    items: &mut [(Handle, EntityType)],
    s: &Settings,
) -> (std::collections::HashSet<Handle>, std::collections::HashSet<Handle>) {
    let mut changed = std::collections::HashSet::new();
    let mut removed = std::collections::HashSet::new();
    if s.optimize {
        for (handle, entity) in items.iter_mut() {
            let before = entity.clone();
            optimize(entity, s.tolerance);
            if *entity != before {
                changed.insert(*handle);
            }
        }
    }
    // Repeat until stable: a merged line can reach objects it did not touch before.
    let mut entries: Vec<Entry> = items.iter().map(|(_, e)| entry(e, s.ignore)).collect();
    let mut progress = true;
    while progress {
        progress = false;
        let mut order: Vec<usize> = (0..items.len())
            .filter(|&i| !removed.contains(&items[i].0))
            .collect();
        order.sort_by(|&a, &b| entries[a].lo.total_cmp(&entries[b].lo));
        for (n, &a) in order.iter().enumerate() {
            for &b in &order[n + 1..] {
                if removed.contains(&items[a].0) {
                    break;
                }
                if entries[b].lo > entries[a].hi + s.tolerance {
                    break;
                }
                if removed.contains(&items[b].0) {
                    continue;
                }
                let (keep, drop) = if items[a].0.value() <= items[b].0.value() {
                    (a, b)
                } else {
                    (b, a)
                };
                let Some(outcome) =
                    compare(&items[keep].1, &items[drop].1, &entries[keep], &entries[drop], s)
                else {
                    continue;
                };
                progress = true;
                match outcome {
                    Outcome::DropSecond => {
                        removed.insert(items[drop].0);
                    }
                    Outcome::DropFirst => {
                        removed.insert(items[keep].0);
                    }
                    Outcome::Merge(entity) => {
                        entries[keep] = entry(&entity, s.ignore);
                        items[keep].1 = entity;
                        changed.insert(items[keep].0);
                        removed.insert(items[drop].0);
                    }
                }
            }
        }
    }
    changed.retain(|h| !removed.contains(h));
    (changed, removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::entities::{Line, LwPolyline};
    use document::types::{Color, Vector2};

    #[test]
    fn normalization_ignores_only_the_requested_properties() {
        let mut first = Line::from_coords(0.0, 0.0, 0.0, 2.0, 0.0, 0.0);
        first.common.handle = Handle::new(1);
        first.common.layer = "FIRST".into();
        first.common.color = Color::from_index(1);
        let mut second = first.clone();
        second.common.handle = Handle::new(2);
        second.common.layer = "SECOND".into();
        second.common.color = Color::from_index(2);

        assert_ne!(
            normalized(&EntityType::Line(first.clone()), 0),
            normalized(&EntityType::Line(second.clone()), 0)
        );
        assert_eq!(
            normalized(&EntityType::Line(first), 1 | 2),
            normalized(&EntityType::Line(second), 1 | 2)
        );
    }

    #[test]
    fn polyline_optimization_removes_only_plain_collinear_vertices() {
        let mut plain = EntityType::LwPolyline(LwPolyline::from_points(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(2.0, 0.0),
        ]));
        optimize(&mut plain, 1e-9);
        let EntityType::LwPolyline(plain) = plain else {
            unreachable!()
        };
        assert_eq!(plain.vertices.len(), 2);

        let mut styled = LwPolyline::from_points(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(2.0, 0.0),
        ]);
        styled.vertices[1].start_width = 1.0;
        let mut styled = EntityType::LwPolyline(styled);
        optimize(&mut styled, 1e-9);
        let EntityType::LwPolyline(styled) = styled else {
            unreachable!()
        };
        assert_eq!(styled.vertices.len(), 3);
    }

    #[test]
    fn read_back_copies_reversed_and_overlapping_lines_are_redundant() {
        let line = |handle, x0, x1, raw: u8| {
            let mut line = Line::from_coords(x0, 0.0, 0.0, x1, 0.0, 0.0);
            line.common.handle = Handle::new(handle);
            line.common.reactors = vec![Handle::new(100 + handle)];
            line.common.raw_record = Some(std::sync::Arc::new(document::entities::RawRecord {
                data: vec![raw],
                handle_bits: 0,
                version: document::types::DxfVersion::Unknown,
            }));
            (Handle::new(handle), EntityType::Line(line))
        };
        let mut items = vec![
            line(1, 0.0, 10.0, 1),
            line(2, 0.0, 10.0 + 1e-9, 2),
            line(3, 10.0, 0.0, 3),
            line(4, 5.0, 15.0, 4),
            line(5, 30.0, 40.0, 5),
        ];
        let (changed, removed) = run(&mut items, &Settings::default());
        assert_eq!(removed.len(), 3);
        assert!(changed.contains(&Handle::new(1)) && !removed.contains(&Handle::new(5)));
        let EntityType::Line(merged) = &items[0].1 else {
            unreachable!()
        };
        assert_eq!((merged.start.x, merged.end.x), (0.0, 15.0));
    }
}
