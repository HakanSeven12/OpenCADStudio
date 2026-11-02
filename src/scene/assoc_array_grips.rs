//! Grips of an associative array reference: arrows that change the array's
//! counts and spacings by dragging, as the reference product shows them.
//!
//! * Rectangular: column count (last column), column spacing (second
//!   column), row count (last row), row spacing (second row), and the
//!   opposite corner for both counts at once.
//! * Polar: item count (last item), item angle (second item), radius
//!   (first item), row count and row spacing outwards.
//! * Path: item count (last item), item spacing (second item, Measure
//!   only), row count and row spacing to the left of the path.
//!
//! A count arrow keeps the spacing and counts how many fit up to the
//! cursor; with Ctrl it keeps the count and stretches the spacing instead
//! (total spacing / fill angle). Levels are left to Properties.

use super::assoc_array::{self as array, ArrayKind, AssocArray};
use codec::{Document, EntityType};
use glam::{DMat4, DVec3};
use std::f64::consts::TAU;

/// Grip ids of array arrows, outside the entity, editor and dynamic-block
/// ranges.
pub const GRIP_START: usize = usize::MAX / 4 + 1024;
const GRIP_COUNT: usize = 16;

pub fn is_array_grip(id: usize) -> bool {
    (GRIP_START..GRIP_START + GRIP_COUNT).contains(&id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    ColumnCount,
    ColumnSpacing,
    RowCount,
    RowSpacing,
    RowAndColumnCount,
    ItemCount,
    ItemSpacing,
    ItemAngle,
    Radius,
}

const KINDS: [Kind; 9] = [
    Kind::ColumnCount,
    Kind::ColumnSpacing,
    Kind::RowCount,
    Kind::RowSpacing,
    Kind::RowAndColumnCount,
    Kind::ItemCount,
    Kind::ItemSpacing,
    Kind::ItemAngle,
    Kind::Radius,
];

impl Kind {
    pub fn id(self) -> usize {
        GRIP_START + KINDS.iter().position(|kind| *kind == self).unwrap_or(0)
    }

    pub fn from_id(id: usize) -> Option<Self> {
        KINDS.get(id.checked_sub(GRIP_START)?).copied()
    }

    /// What the grip changes, for its tooltip.
    pub fn label(self, array: ArrayKind) -> &'static str {
        match (self, array) {
            (Kind::ColumnCount, _) => "Column count",
            (Kind::ColumnSpacing, _) => "Column spacing",
            (Kind::RowCount, _) => "Row count",
            (Kind::RowSpacing, _) => "Row spacing",
            (Kind::RowAndColumnCount, _) => "Row and column count",
            (Kind::ItemCount, _) => "Item count",
            (Kind::ItemSpacing, _) => "Item spacing",
            (Kind::ItemAngle, _) => "Angle between items",
            (Kind::Radius, _) => "Radius",
        }
    }

    /// The command-line prompt while the grip is dragged.
    pub fn prompt(self) -> &'static str {
        match self {
            Kind::ColumnCount => "Specify number of columns (Ctrl: total column spacing):",
            Kind::RowCount => "Specify number of rows (Ctrl: total row spacing):",
            Kind::RowAndColumnCount => "Specify number of rows and columns:",
            Kind::ItemCount => "Specify number of items (Ctrl: fill angle or total spacing):",
            Kind::ColumnSpacing => "Specify the distance between columns:",
            Kind::RowSpacing => "Specify the distance between rows:",
            Kind::ItemSpacing => "Specify the distance between items:",
            Kind::ItemAngle => "Specify the angle between items:",
            Kind::Radius => "Specify radius:",
        }
    }
}

/// One arrow on an array: where it sits and which way it points (world).
#[derive(Clone, Copy, Debug)]
pub struct Grip {
    pub kind: Kind,
    pub world: DVec3,
    pub dir: DVec3,
}

/// How a drag turns the cursor into new parameter values, fixed when the
/// drag starts.
#[derive(Clone, Debug)]
pub enum Drag {
    /// Along one axis from `origin`: a count of `spacing` steps, or the
    /// spacing itself. `count`/`spacing` name the parameters.
    Linear {
        origin: DVec3,
        axis: DVec3,
        /// The other in-plane axis, for oblique (non-square) grids.
        across: Option<DVec3>,
        normal: DVec3,
        /// World length of one parameter unit along `axis`.
        unit: f64,
        spacing: f64,
        count: f64,
        count_param: &'static str,
        spacing_param: &'static str,
        /// The spacing it moves sits `offset` (world) out from `origin`
        /// (a polar ring's radius).
        offset: f64,
        sets_count: bool,
    },
    /// Both counts of a rectangular array from the corner arrow.
    Corner {
        origin: DVec3,
        columns: DVec3,
        rows: DVec3,
        normal: DVec3,
        column_step: f64,
        row_step: f64,
    },
    /// Polar items about `center`.
    Polar {
        center: DVec3,
        normal: DVec3,
        start: DVec3,
        sign: f64,
        spacing: f64,
        count: f64,
        kind: Kind,
        unit: f64,
    },
    /// Path items along a world polyline approximation of the path.
    Path {
        points: Vec<DVec3>,
        reversed: bool,
        closed: bool,
        spacing: f64,
        count: f64,
        divide: bool,
        kind: Kind,
    },
}

fn get(params: &[codec::objects::AssocValueParam], name: &str, default: f64) -> f64 {
    array::param_value(params, name).unwrap_or(default)
}

/// Item base points (world) by (column/item, row, level).
fn item_points(document: &Document, array: &AssocArray) -> Vec<([i32; 3], DVec3)> {
    let Ok((_, placements, _)) = array::plan(document, array, &[]) else {
        return Vec::new();
    };
    let to_world = array::world_from_array_block(document, array.insert);
    placements
        .into_iter()
        .map(|(location, placement)| {
            (
                location,
                to_world.transform_point3(placement.w_axis.truncate()),
            )
        })
        .collect()
}

fn at(points: &[([i32; 3], DVec3)], location: [i32; 3]) -> Option<DVec3> {
    points
        .iter()
        .find(|(item, _)| *item == location)
        .map(|(_, point)| *point)
}

/// World direction and length of one array-block unit along `local`.
fn world_axis(to_world: DMat4, local: DVec3) -> Option<(DVec3, f64)> {
    let world = to_world.transform_vector3(local);
    let length = world.length();
    (length > 1e-12).then(|| (world / length, length))
}

/// The array arrows of `array`, empty for an array that cannot be edited
/// (a path array whose path is gone).
pub fn grips(document: &Document, array: &AssocArray) -> Vec<Grip> {
    let Some(setup) = Setup::read(document, array) else {
        return Vec::new();
    };
    KINDS.iter().filter_map(|kind| setup.grip(*kind)).collect()
}

/// The drag of the arrow `kind` on `array`.
pub fn drag(document: &Document, array: &AssocArray, kind: Kind) -> Option<Drag> {
    Setup::read(document, array)?.drag(kind)
}

/// Everything the arrows of one array are placed and dragged from.
struct Setup {
    kind: ArrayKind,
    params: Vec<codec::objects::AssocValueParam>,
    points: Vec<([i32; 3], DVec3)>,
    normal: DVec3,
    /// Columns/items axis (rectangular) in world, and its unit length.
    u: Option<(DVec3, f64)>,
    /// Rows axis (rectangular) in world, and its unit length.
    v: Option<(DVec3, f64)>,
    center: Option<DVec3>,
    path: Option<(Vec<DVec3>, bool)>,
    unit: f64,
}

impl Setup {
    fn read(document: &Document, array: &AssocArray) -> Option<Self> {
        let params = array::value_params(document, array).to_vec();
        let points = item_points(document, array);
        if points.is_empty() {
            return None;
        }
        let to_world = array::world_from_array_block(document, array.insert);
        let local_normal = array::base_normal(document, array);
        let normal = to_world
            .transform_vector3(local_normal)
            .try_normalize()
            .unwrap_or(DVec3::Z);
        let unit = world_axis(to_world, DVec3::X).map_or(1.0, |(_, length)| length);
        let (mut u, mut v, mut center, mut path) = (None, None, None, None);
        match array.kind {
            ArrayKind::Rectangular => {
                let axis = array::column_axis(document, array);
                let angle = get(&params, "AxesAngle", std::f64::consts::FRAC_PI_2);
                let row_axis = glam::DQuat::from_axis_angle(local_normal, angle) * axis;
                u = world_axis(to_world, axis);
                v = world_axis(to_world, row_axis);
            }
            ArrayKind::Polar => {
                center = Some(to_world.transform_point3(array::polar_center(document, array)?));
            }
            ArrayKind::Path => {
                let (_, entity) = array::path_entity(document, array)?;
                let curve = crate::entities::curve::entity_curve(&entity)?;
                let tolerance = (curve.length() * 1e-4).max(1e-9);
                let polyline: Vec<DVec3> = curve
                    .tessellate_within(tolerance)
                    .into_iter()
                    .map(DVec3::from_array)
                    .collect();
                if polyline.len() < 2 {
                    return None;
                }
                path = Some((polyline, curve.is_closed()));
            }
        }
        Some(Self {
            kind: array.kind,
            params,
            points,
            normal,
            u,
            v,
            center,
            path,
            unit,
        })
    }

    fn get(&self, name: &str, default: f64) -> f64 {
        get(&self.params, name, default)
    }

    fn counts(&self) -> (i32, i32) {
        (
            self.get("Items", 1.0).max(1.0) as i32,
            self.get("Rows", 1.0).max(1.0) as i32,
        )
    }

    /// The direction from item (0,0,0) to item (0,1,0), or where a second
    /// row would go.
    fn row_direction(&self) -> Option<DVec3> {
        let first = at(&self.points, [0, 0, 0])?;
        // With a second row present, its items say where rows go.
        if self.kind != ArrayKind::Rectangular {
            let spacing = self.get("RowSpacing", 0.0);
            if let Some(second) = at(&self.points, [0, 1, 0]) {
                if let Some(dir) = (second - first).try_normalize() {
                    return Some(if spacing < 0.0 { -dir } else { dir });
                }
            }
        }
        match self.kind {
            ArrayKind::Rectangular => self.v.map(|(dir, _)| dir),
            ArrayKind::Polar => (first - self.center?).try_normalize(),
            ArrayKind::Path => {
                let (points, _) = self.path.as_ref()?;
                let next = at(&self.points, [1, 0, 0]).or_else(|| points.get(1).copied())?;
                let tangent = (next - first)
                    .try_normalize()
                    .or_else(|| (points[1] - points[0]).try_normalize())?;
                let tangent = if self.get("PathDirection", 1.0) == 0.0
                    && at(&self.points, [1, 0, 0]).is_none()
                {
                    -tangent
                } else {
                    tangent
                };
                Some(
                    self.normal
                        .cross(tangent)
                        .try_normalize()
                        .unwrap_or(DVec3::new(-tangent.y, tangent.x, 0.0)),
                )
            }
        }
    }

    fn grip(&self, kind: Kind) -> Option<Grip> {
        let (columns, rows) = self.counts();
        let first = at(&self.points, [0, 0, 0])?;
        let make =
            |world: DVec3, dir: DVec3| dir.try_normalize().map(|dir| Grip { kind, world, dir });
        match (self.kind, kind) {
            (ArrayKind::Rectangular, Kind::ColumnCount) => {
                let (u, unit) = self.u?;
                let step = self.get("ItemSpacing", 0.0) * unit;
                if step.abs() < 1e-12 {
                    return None;
                }
                let world = at(&self.points, [columns - 1, 0, 0])
                    .filter(|_| columns > 1)
                    .unwrap_or(first + u * step);
                make(world, u * step.signum())
            }
            (ArrayKind::Rectangular, Kind::ColumnSpacing) if columns > 2 => {
                let (u, _) = self.u?;
                let world = at(&self.points, [1, 0, 0])?;
                make(world, u * self.get("ItemSpacing", 0.0).signum())
            }
            (ArrayKind::Rectangular, Kind::RowCount) => {
                let (v, unit) = self.v?;
                let step = self.get("RowSpacing", 0.0) * unit;
                if step.abs() < 1e-12 {
                    return None;
                }
                let world = at(&self.points, [0, rows - 1, 0])
                    .filter(|_| rows > 1)
                    .unwrap_or(first + v * step);
                make(world, v * step.signum())
            }
            (ArrayKind::Rectangular, Kind::RowSpacing) if rows > 2 => {
                let (v, _) = self.v?;
                let world = at(&self.points, [0, 1, 0])?;
                make(world, v * self.get("RowSpacing", 0.0).signum())
            }
            (ArrayKind::Rectangular, Kind::RowAndColumnCount) if columns > 1 && rows > 1 => {
                let world = at(&self.points, [columns - 1, rows - 1, 0])?;
                make(world, world - first)
            }
            (ArrayKind::Polar, Kind::ItemCount) => {
                let center = self.center?;
                let spacing = self.get("ItemSpacing", 0.0);
                if spacing.abs() < 1e-12 {
                    return None;
                }
                let world = at(&self.points, [columns - 1, 0, 0])
                    .filter(|_| columns > 1)
                    .or_else(|| {
                        let turn = glam::DQuat::from_axis_angle(self.normal, self.sign() * spacing);
                        Some(center + turn * (first - center))
                    })?;
                make(world, self.normal.cross(world - center) * self.sign())
            }
            (ArrayKind::Polar, Kind::ItemAngle) if columns > 2 => {
                let center = self.center?;
                let world = at(&self.points, [1, 0, 0])?;
                make(world, self.normal.cross(world - center) * self.sign())
            }
            (ArrayKind::Polar, Kind::Radius) => {
                let center = self.center?;
                make(first, first - center)
            }
            (ArrayKind::Polar | ArrayKind::Path, Kind::RowCount) => {
                let direction = self.row_direction()?;
                let step = self.get("RowSpacing", 0.0) * self.unit;
                if step.abs() < 1e-12 {
                    return None;
                }
                let world = at(&self.points, [0, rows - 1, 0])
                    .filter(|_| rows > 1)
                    .unwrap_or(first + direction * step);
                make(world, direction * step.signum())
            }
            (ArrayKind::Polar | ArrayKind::Path, Kind::RowSpacing) if rows > 2 => {
                let direction = self.row_direction()?;
                let world = at(&self.points, [0, 1, 0])?;
                make(world, direction * self.get("RowSpacing", 0.0).signum())
            }
            (ArrayKind::Path, Kind::ItemCount) => {
                let spacing = self.get("ItemSpacing", 0.0);
                if spacing.abs() < 1e-12 {
                    return None;
                }
                let world = if columns > 1 {
                    at(&self.points, [columns - 1, 0, 0])?
                } else {
                    let along = self.path_point(spacing)?;
                    along
                };
                let before = if columns > 1 {
                    at(&self.points, [columns - 2, 0, 0])?
                } else {
                    first
                };
                make(world, world - before)
            }
            (ArrayKind::Path, Kind::ItemSpacing)
                if columns > 2 && self.get("Method", 1.0) != 0.0 =>
            {
                let world = at(&self.points, [1, 0, 0])?;
                make(world, world - first)
            }
            _ => None,
        }
    }

    fn sign(&self) -> f64 {
        if self.get("Direction", 1.0) == 0.0 {
            -1.0
        } else {
            1.0
        }
    }

    /// The world point `distance` along the path from where the items start.
    fn path_point(&self, distance: f64) -> Option<DVec3> {
        let (points, _) = self.path.as_ref()?;
        let reversed = self.get("PathDirection", 1.0) == 0.0;
        let total = polyline_length(points);
        let distance = if reversed { total - distance } else { distance };
        Some(point_at(points, distance.clamp(0.0, total)))
    }

    fn drag(&self, kind: Kind) -> Option<Drag> {
        self.grip(kind)?;
        let (columns, rows) = self.counts();
        let first = at(&self.points, [0, 0, 0])?;
        let linear = |axis: (DVec3, f64),
                      across: Option<DVec3>,
                      spacing_param: &'static str,
                      count_param: &'static str,
                      count: i32,
                      sets_count: bool,
                      offset: f64| Drag::Linear {
            origin: first,
            axis: axis.0,
            across,
            normal: self.normal,
            unit: axis.1,
            spacing: self.get(spacing_param, 0.0),
            count: f64::from(count),
            count_param,
            spacing_param,
            offset,
            sets_count,
        };
        Some(match (self.kind, kind) {
            (ArrayKind::Rectangular, Kind::ColumnCount | Kind::ColumnSpacing) => linear(
                self.u?,
                self.v.map(|(v, _)| v),
                "ItemSpacing",
                "Items",
                columns,
                kind == Kind::ColumnCount,
                0.0,
            ),
            (ArrayKind::Rectangular, Kind::RowCount | Kind::RowSpacing) => linear(
                self.v?,
                self.u.map(|(u, _)| u),
                "RowSpacing",
                "Rows",
                rows,
                kind == Kind::RowCount,
                0.0,
            ),
            (ArrayKind::Rectangular, Kind::RowAndColumnCount) => {
                let ((u, u_unit), (v, v_unit)) = (self.u?, self.v?);
                Drag::Corner {
                    origin: first,
                    columns: u,
                    rows: v,
                    normal: self.normal,
                    column_step: self.get("ItemSpacing", 0.0) * u_unit,
                    row_step: self.get("RowSpacing", 0.0) * v_unit,
                }
            }
            (ArrayKind::Polar | ArrayKind::Path, Kind::RowCount | Kind::RowSpacing) => {
                let direction = self.row_direction()?;
                linear(
                    (direction, self.unit),
                    None,
                    "RowSpacing",
                    "Rows",
                    rows,
                    kind == Kind::RowCount,
                    0.0,
                )
            }
            (ArrayKind::Polar, Kind::Radius) => {
                let center = self.center?;
                let direction = (first - center).try_normalize()?;
                Drag::Linear {
                    origin: center,
                    axis: direction,
                    across: None,
                    normal: self.normal,
                    unit: self.unit,
                    spacing: self.get("Radius", 0.0),
                    count: 1.0,
                    count_param: "Items",
                    spacing_param: "Radius",
                    offset: 0.0,
                    sets_count: false,
                }
            }
            (ArrayKind::Polar, Kind::ItemCount | Kind::ItemAngle) => {
                let center = self.center?;
                Drag::Polar {
                    center,
                    normal: self.normal,
                    start: (first - center).try_normalize()?,
                    sign: self.sign(),
                    spacing: self.get("ItemSpacing", 0.0).abs(),
                    count: f64::from(columns),
                    kind,
                    unit: self.unit,
                }
            }
            (ArrayKind::Path, Kind::ItemCount | Kind::ItemSpacing) => {
                let (points, closed) = self.path.clone()?;
                let divide = self.get("Method", 1.0) == 0.0;
                // Divide spreads the items over the whole path: step by
                // where they are, not by the stored (unused) spacing.
                let gaps = if closed { columns } else { columns - 1 };
                let spacing = if divide && gaps > 0 {
                    polyline_length(&points) / f64::from(gaps)
                } else {
                    self.get("ItemSpacing", 0.0)
                };
                Drag::Path {
                    points,
                    reversed: self.get("PathDirection", 1.0) == 0.0,
                    closed,
                    spacing,
                    count: f64::from(columns),
                    divide,
                    kind,
                }
            }
            _ => return None,
        })
    }
}

fn polyline_length(points: &[DVec3]) -> f64 {
    points
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum()
}

fn point_at(points: &[DVec3], distance: f64) -> DVec3 {
    let mut left = distance;
    for pair in points.windows(2) {
        let length = pair[0].distance(pair[1]);
        if left <= length && length > 0.0 {
            return pair[0].lerp(pair[1], left / length);
        }
        left -= length;
    }
    *points.last().unwrap_or(&DVec3::ZERO)
}

/// How far along the polyline the point nearest `cursor` lies. Past an
/// open end the distance runs on along the end's direction, so a count
/// arrow can be pulled beyond the last item.
fn distance_along(points: &[DVec3], cursor: DVec3, closed: bool) -> f64 {
    let (mut best, mut best_distance, mut walked) = (f64::INFINITY, 0.0, 0.0);
    let last = points.len().saturating_sub(2);
    for (index, pair) in points.windows(2).enumerate() {
        let segment = pair[1] - pair[0];
        let length = segment.length();
        if length <= 0.0 {
            continue;
        }
        let mut t = (cursor - pair[0]).dot(segment) / (length * length);
        if closed || (index != 0 || t > 0.0) && (index != last || t < 1.0) {
            t = t.clamp(0.0, 1.0);
        }
        let point = pair[0] + segment * t;
        let gap = point.distance_squared(cursor);
        if gap < best {
            best = gap;
            best_distance = walked + length * t;
        }
        walked += length;
    }
    best_distance
}

/// Signed angle from `from` to `to` about `normal`, in [0, 2π).
fn angle_about(from: DVec3, to: DVec3, normal: DVec3) -> f64 {
    let x = from;
    let y = normal.cross(from);
    let angle = to.dot(y).atan2(to.dot(x));
    if angle < 0.0 {
        angle + TAU
    } else {
        angle
    }
}

/// Components of `delta` along `axis` and `across` (oblique grid), in the
/// plane with `normal`.
fn oblique(delta: DVec3, axis: DVec3, across: Option<DVec3>, normal: DVec3) -> f64 {
    let Some(across) = across else {
        return delta.dot(axis);
    };
    let denominator = axis.cross(across).dot(normal);
    if denominator.abs() < 1e-9 {
        return delta.dot(axis);
    }
    delta.cross(across).dot(normal) / denominator
}

const MAX_ITEMS: f64 = 100_000.0;

fn count_for(distance: f64, step: f64) -> Option<f64> {
    (step.abs() > 1e-12)
        .then(|| ((distance / step).round() + 1.0).clamp(1.0, MAX_ITEMS))
        .filter(|count| count.is_finite())
}

impl Drag {
    /// The parameter changes with the cursor at `cursor`; `ctrl` swaps a
    /// count arrow to total spacing (fill angle on a polar array). Empty
    /// when the cursor gives no valid value.
    pub fn changes(&self, cursor: DVec3, ctrl: bool) -> Vec<(&'static str, f64)> {
        let changes = match self {
            Drag::Linear {
                origin,
                axis,
                across,
                normal,
                unit,
                spacing,
                count,
                count_param,
                spacing_param,
                offset,
                sets_count,
            } => {
                let world = oblique(cursor - *origin, *axis, *across, *normal) - offset;
                if *sets_count && !ctrl {
                    count_for(world, spacing * unit)
                        .map(|count| vec![(*count_param, count)])
                        .unwrap_or_default()
                } else if *sets_count {
                    if *count > 1.0 {
                        vec![(*spacing_param, world / unit / (count - 1.0))]
                    } else {
                        Vec::new()
                    }
                } else if *spacing_param == "Radius" {
                    (world > 1e-12)
                        .then(|| vec![(*spacing_param, world / unit)])
                        .unwrap_or_default()
                } else {
                    vec![(*spacing_param, world / unit)]
                }
            }
            Drag::Corner {
                origin,
                columns,
                rows,
                normal,
                column_step,
                row_step,
            } => {
                let delta = cursor - *origin;
                let along = oblique(delta, *columns, Some(*rows), *normal);
                let up = oblique(delta, *rows, Some(*columns), *normal);
                match (count_for(along, *column_step), count_for(up, *row_step)) {
                    (Some(columns), Some(rows)) => vec![("Items", columns), ("Rows", rows)],
                    _ => Vec::new(),
                }
            }
            Drag::Polar {
                center,
                normal,
                start,
                sign,
                spacing,
                count,
                kind,
                unit: _,
            } => {
                let angle = angle_about(*start, cursor - *center, *normal * *sign);
                match kind {
                    Kind::ItemCount if !ctrl => {
                        let Some(items) = count_for(angle, *spacing) else {
                            return Vec::new();
                        };
                        let full = (TAU / spacing).round();
                        if items >= full && (full * spacing - TAU).abs() < 1e-6 {
                            vec![("Items", full), ("FillAngle", TAU)]
                        } else {
                            vec![("Items", items), ("FillAngle", spacing * (items - 1.0))]
                        }
                    }
                    Kind::ItemCount => (angle > 1e-9)
                        .then(|| vec![("FillAngle", angle)])
                        .unwrap_or_default(),
                    _ => {
                        if angle < 1e-9 || *count < 2.0 {
                            return Vec::new();
                        }
                        if angle * count >= TAU - 1e-9 {
                            vec![("FillAngle", TAU)]
                        } else {
                            vec![("FillAngle", angle * (count - 1.0))]
                        }
                    }
                }
            }
            Drag::Path {
                points,
                reversed,
                closed,
                spacing,
                count,
                divide,
                kind,
            } => {
                let total = polyline_length(points);
                let mut along = distance_along(points, cursor, *closed);
                if *reversed {
                    along = total - along;
                }
                match kind {
                    Kind::ItemCount if !ctrl || *divide => {
                        let Some(mut items) = count_for(along, *spacing) else {
                            return Vec::new();
                        };
                        if !divide {
                            // Measure: only as many as fit on the path.
                            let room = if *closed { total - 1e-9 } else { total + 1e-9 };
                            items = items.min((room / spacing).floor() + 1.0).max(1.0);
                        }
                        vec![("Items", items)]
                    }
                    Kind::ItemCount => {
                        if *count > 1.0 && along > 1e-12 {
                            let gaps = count - 1.0;
                            vec![("ItemSpacing", (along / gaps).min(total / gaps))]
                        } else {
                            Vec::new()
                        }
                    }
                    _ => {
                        if along <= 1e-12 || *count < 2.0 {
                            return Vec::new();
                        }
                        vec![("ItemSpacing", along.min(total / (count - 1.0)))]
                    }
                }
            }
        };
        changes
            .into_iter()
            .filter(|(_, value)| value.is_finite())
            .collect()
    }

    /// Typed input: a whole number for a count arrow, a distance or angle
    /// otherwise.
    pub fn typed(&self, text: &str, ctrl: bool) -> Option<Vec<(&'static str, f64)>> {
        let text = text.trim();
        let count = || {
            text.parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && *value >= 1.0 && *value <= MAX_ITEMS)
                .map(f64::round)
        };
        let length =
            || crate::entities::common::parse_typed_length(text).filter(|value| value.is_finite());
        match self {
            Drag::Linear {
                count_param,
                spacing_param,
                sets_count,
                count: current,
                ..
            } => {
                if *sets_count && !ctrl {
                    Some(vec![(*count_param, count()?)])
                } else if *sets_count {
                    let total = length().filter(|_| *current > 1.0)?;
                    Some(vec![(*spacing_param, total / (current - 1.0))])
                } else {
                    let value = length()?;
                    (*spacing_param != "Radius" || value > 0.0)
                        .then(|| vec![(*spacing_param, value)])
                }
            }
            Drag::Corner { .. } => {
                let mut parts = text.split(',');
                let columns = parts.next()?.trim().parse::<f64>().ok()?;
                let rows = parts
                    .next()
                    .map_or(Some(columns), |rows| rows.trim().parse::<f64>().ok())?;
                let valid = |value: f64| (1.0..=MAX_ITEMS).contains(&value);
                (valid(columns) && valid(rows))
                    .then(|| vec![("Items", columns.round()), ("Rows", rows.round())])
            }
            Drag::Polar {
                kind,
                spacing,
                count: current,
                ..
            } => match kind {
                Kind::ItemCount if !ctrl => {
                    let items = count()?;
                    let full = (TAU / spacing).round();
                    Some(if items >= full && (full * spacing - TAU).abs() < 1e-6 {
                        vec![("Items", full), ("FillAngle", TAU)]
                    } else {
                        vec![("Items", items), ("FillAngle", spacing * (items - 1.0))]
                    })
                }
                Kind::ItemCount => crate::entities::common::parse_typed_angle(text)
                    .filter(|angle| *angle > 0.0 && *angle <= TAU)
                    .map(|angle| vec![("FillAngle", angle)]),
                _ => {
                    let angle = crate::entities::common::parse_typed_angle(text)
                        .filter(|angle| *angle > 0.0 && *angle < TAU && *current >= 2.0)?;
                    Some(if angle * current >= TAU - 1e-9 {
                        vec![("FillAngle", TAU)]
                    } else {
                        vec![("FillAngle", angle * (current - 1.0))]
                    })
                }
            },
            Drag::Path {
                kind,
                count: current,
                divide,
                ..
            } => match kind {
                Kind::ItemCount if !ctrl || *divide => Some(vec![("Items", count()?)]),
                _ => {
                    let value = length().filter(|value| *value > 0.0)?;
                    let gaps = if *kind == Kind::ItemCount {
                        (current - 1.0).max(1.0)
                    } else {
                        1.0
                    };
                    Some(vec![("ItemSpacing", value / gaps)])
                }
            },
        }
    }

    /// The point the drag starts from, for the rubber band.
    pub fn anchor(&self) -> DVec3 {
        match self {
            Drag::Linear { origin, .. } | Drag::Corner { origin, .. } => *origin,
            Drag::Polar { center, .. } => *center,
            Drag::Path {
                points, reversed, ..
            } => {
                if *reversed {
                    *points.last().unwrap_or(&DVec3::ZERO)
                } else {
                    points[0]
                }
            }
        }
    }
}

/// The world-space references the array would show after `changes`.
pub fn preview(
    document: &Document,
    array: &AssocArray,
    changes: &[(&str, f64)],
) -> Vec<EntityType> {
    array::preview_items(document, array, changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::assoc_array::{array_of_insert, create_from_spec, AssocArraySpec};
    use crate::scene::Scene;
    use codec::entities::{Circle, Line};
    use std::f64::consts::{FRAC_PI_2, PI};

    fn circle(scene: &mut Scene, x: f64, y: f64) -> codec::Handle {
        scene.add_entity(EntityType::Circle(Circle::from_coords(x, y, 0.0, 2.0)))
    }

    fn array(scene: &Scene, insert: codec::Handle) -> AssocArray {
        array_of_insert(&scene.document, insert).expect("array")
    }

    fn grip_at(grips: &[Grip], kind: Kind) -> Option<DVec3> {
        grips
            .iter()
            .find(|grip| grip.kind == kind)
            .map(|grip| grip.world)
    }

    fn close(a: DVec3, b: DVec3) -> bool {
        a.distance(b) < 1e-6
    }

    fn param(scene: &Scene, insert: codec::Handle, name: &str) -> f64 {
        let array = array(scene, insert);
        array::param_value(array::value_params(&scene.document, &array), name).unwrap()
    }

    fn item_count(scene: &Scene, insert: codec::Handle) -> usize {
        let array = array(scene, insert);
        scene
            .document
            .block_records
            .iter()
            .find(|record| record.handle == array.array_block)
            .unwrap()
            .entity_handles
            .iter()
            .filter(|handle| {
                matches!(
                    scene.document.get_entity(**handle),
                    Some(EntityType::Insert(_))
                )
            })
            .count()
    }

    fn rectangular(columns: u32, rows: u32) -> (Scene, codec::Handle) {
        let mut scene = Scene::new();
        let source = circle(&mut scene, 0.0, 0.0);
        let spec = AssocArraySpec::Rectangular {
            columns,
            rows,
            column_spacing: 20.0,
            row_spacing: 30.0,
        };
        let insert = create_from_spec(&mut scene, &[source], DVec3::ZERO, &spec).unwrap();
        (scene, insert)
    }

    #[test]
    fn rectangular_arrows_sit_on_the_last_and_second_items() {
        let (scene, insert) = rectangular(3, 2);
        let grips = grips(&scene.document, &array(&scene, insert));
        let origin = grip_at(&grips, Kind::ColumnCount).unwrap() - DVec3::new(40.0, 0.0, 0.0);
        assert!(close(
            grip_at(&grips, Kind::ColumnSpacing).unwrap(),
            origin + DVec3::new(20.0, 0.0, 0.0)
        ));
        assert!(close(
            grip_at(&grips, Kind::RowCount).unwrap(),
            origin + DVec3::new(0.0, 30.0, 0.0)
        ));
        assert!(close(
            grip_at(&grips, Kind::RowAndColumnCount).unwrap(),
            origin + DVec3::new(40.0, 30.0, 0.0)
        ));
        // Two rows: no second-row spacing arrow (it would sit on the count arrow).
        assert!(grip_at(&grips, Kind::RowSpacing).is_none());
        let column = grips
            .iter()
            .find(|grip| grip.kind == Kind::ColumnCount)
            .unwrap();
        assert!(close(column.dir, DVec3::X));
    }

    #[test]
    fn rectangular_count_arrow_counts_steps_or_stretches_with_ctrl() {
        let (scene, insert) = rectangular(3, 2);
        let array = array(&scene, insert);
        let origin = grip_at(&grips(&scene.document, &array), Kind::ColumnCount).unwrap()
            - DVec3::new(40.0, 0.0, 0.0);
        let count = drag(&scene.document, &array, Kind::ColumnCount).unwrap();
        // 101 along with spacing 20: 5 steps, 6 columns; off-axis movement ignored.
        assert_eq!(
            count.changes(origin + DVec3::new(101.0, 7.0, 0.0), false),
            vec![("Items", 6.0)]
        );
        // Behind the first column: still one column.
        assert_eq!(
            count.changes(origin - DVec3::new(50.0, 0.0, 0.0), false),
            vec![("Items", 1.0)]
        );
        // Ctrl: 3 columns stretched over 101.
        assert_eq!(
            count.changes(origin + DVec3::new(101.0, 0.0, 0.0), true),
            vec![("ItemSpacing", 50.5)]
        );
        let corner = drag(&scene.document, &array, Kind::RowAndColumnCount).unwrap();
        assert_eq!(
            corner.changes(origin + DVec3::new(61.0, 95.0, 0.0), false),
            vec![("Items", 4.0), ("Rows", 4.0)]
        );
        let spacing = drag(&scene.document, &array, Kind::ColumnSpacing).unwrap();
        assert_eq!(
            spacing.changes(origin + DVec3::new(25.0, 3.0, 0.0), false),
            vec![("ItemSpacing", 25.0)]
        );
        assert_eq!(count.typed("7", false), Some(vec![("Items", 7.0)]));
        assert_eq!(count.typed("0", false), None);
    }

    #[test]
    fn rectangular_count_arrow_rebuilds_the_items() {
        let (mut scene, insert) = rectangular(3, 2);
        let target = array(&scene, insert);
        let origin = grip_at(&grips(&scene.document, &target), Kind::ColumnCount).unwrap()
            - DVec3::new(40.0, 0.0, 0.0);
        let changes = drag(&scene.document, &target, Kind::ColumnCount)
            .unwrap()
            .changes(origin + DVec3::new(100.0, 0.0, 0.0), false);
        array::update(&mut scene, &target, &changes).unwrap();
        assert_eq!(param(&scene, insert, "Items"), 6.0);
        assert_eq!(item_count(&scene, insert), 12);
        // The arrow moved with the last column.
        let moved = grips(&scene.document, &array(&scene, insert));
        assert!(close(
            grip_at(&moved, Kind::ColumnCount).unwrap(),
            origin + DVec3::new(100.0, 0.0, 0.0)
        ));
    }

    #[test]
    fn single_item_array_still_has_count_arrows() {
        let (scene, insert) = rectangular(1, 1);
        let grips = grips(&scene.document, &array(&scene, insert));
        assert!(grip_at(&grips, Kind::ColumnCount).is_some());
        assert!(grip_at(&grips, Kind::RowCount).is_some());
        assert!(grip_at(&grips, Kind::RowAndColumnCount).is_none());
    }

    fn polar(items: u32, fill: f64) -> (Scene, codec::Handle, DVec3) {
        let mut scene = Scene::new();
        let source = circle(&mut scene, 50.0, 0.0);
        let spec = AssocArraySpec::Polar {
            center: DVec3::ZERO,
            items,
            fill_angle: fill,
        };
        let insert =
            create_from_spec(&mut scene, &[source], DVec3::new(50.0, 0.0, 0.0), &spec).unwrap();
        let center = drag(&scene.document, &array(&scene, insert), Kind::Radius)
            .unwrap()
            .anchor();
        (scene, insert, center)
    }

    fn on_circle(center: DVec3, radius: f64, degrees: f64) -> DVec3 {
        let angle = degrees.to_radians();
        center + DVec3::new(angle.cos(), angle.sin(), 0.0) * radius
    }

    #[test]
    fn polar_arrows_change_items_angle_and_radius() {
        let (scene, insert, center) = polar(4, TAU);
        let array = array(&scene, insert);
        let grips = grips(&scene.document, &array);
        let first = grip_at(&grips, Kind::Radius).unwrap();
        let radius = first.distance(center);
        assert!(
            (radius - 50.0).abs() < 1e-9,
            "radius {radius} center {center:?}"
        );
        assert!(close(
            grip_at(&grips, Kind::ItemCount).unwrap(),
            on_circle(center, radius, 270.0)
        ));
        assert!(close(
            grip_at(&grips, Kind::ItemAngle).unwrap(),
            on_circle(center, radius, 90.0)
        ));

        let count = drag(&scene.document, &array, Kind::ItemCount).unwrap();
        // 100 degrees at 90 degrees per item: 2 items over a quarter turn.
        assert_eq!(
            count.changes(on_circle(center, radius, 100.0), false),
            vec![("Items", 2.0), ("FillAngle", FRAC_PI_2)]
        );
        // Pulled round to the full circle: 4 items closing the circle.
        assert_eq!(
            count.changes(on_circle(center, radius, 300.0), false),
            vec![("Items", 4.0), ("FillAngle", TAU)]
        );
        let ctrl = count.changes(on_circle(center, radius, 120.0), true);
        assert_eq!(ctrl.len(), 1);
        assert_eq!(ctrl[0].0, "FillAngle");
        assert!((ctrl[0].1 - 120f64.to_radians()).abs() < 1e-9);

        let angle = drag(&scene.document, &array, Kind::ItemAngle).unwrap();
        let fill = angle.changes(on_circle(center, radius, 60.0), false);
        assert_eq!(fill[0].0, "FillAngle");
        assert!((fill[0].1 - PI).abs() < 1e-9, "{fill:?}");

        let radius_drag = drag(&scene.document, &array, Kind::Radius).unwrap();
        let changed = radius_drag.changes(on_circle(center, radius + 30.0, 0.0), false);
        assert_eq!(changed[0].0, "Radius");
        assert!((changed[0].1 - (radius + 30.0)).abs() < 1e-9, "{changed:?}");
        assert!(radius_drag
            .changes(center - DVec3::X * 10.0, false)
            .is_empty());
    }

    #[test]
    fn polar_count_arrow_rebuilds_the_items() {
        let (mut scene, insert, center) = polar(4, TAU);
        let target = array(&scene, insert);
        let radius = grip_at(&grips(&scene.document, &target), Kind::Radius)
            .unwrap()
            .distance(center);
        let changes = drag(&scene.document, &target, Kind::ItemCount)
            .unwrap()
            .changes(on_circle(center, radius, 100.0), false);
        array::update(&mut scene, &target, &changes).unwrap();
        assert_eq!(param(&scene, insert, "Items"), 2.0);
        assert_eq!(item_count(&scene, insert), 2);
    }

    #[test]
    fn polar_row_arrows_point_outwards_from_the_center() {
        let (mut scene, insert, center) = polar(4, TAU);
        let target = array(&scene, insert);
        array::update(&mut scene, &target, &[("Rows", 3.0), ("RowSpacing", 10.0)]).unwrap();
        let target = array(&scene, insert);
        let grips = grips(&scene.document, &target);
        let first = grip_at(&grips, Kind::Radius).unwrap();
        let outwards = (first - center).normalize();
        let rows = grips
            .iter()
            .find(|grip| grip.kind == Kind::RowCount)
            .unwrap();
        assert!(
            close(rows.world, first + outwards * 20.0),
            "{:?}",
            rows.world
        );
        assert!(close(rows.dir, outwards));
        assert!(close(
            grip_at(&grips, Kind::RowSpacing).unwrap(),
            first + outwards * 10.0
        ));
        let count = drag(&scene.document, &target, Kind::RowCount).unwrap();
        assert_eq!(
            count.changes(first + outwards * 41.0, false),
            vec![("Rows", 5.0)]
        );
        let spacing = drag(&scene.document, &target, Kind::RowSpacing).unwrap();
        assert_eq!(
            spacing.changes(first + outwards * 15.0, false),
            vec![("RowSpacing", 15.0)]
        );
    }

    fn path(divide: bool) -> (Scene, codec::Handle) {
        let mut scene = Scene::new();
        let source = circle(&mut scene, 0.0, 0.0);
        let path = scene.add_entity(EntityType::Line(Line::from_coords(
            0.0, 0.0, 0.0, 100.0, 0.0, 0.0,
        )));
        let spec = AssocArraySpec::Path {
            path,
            items: 3,
            spacing: 20.0,
            divide,
            reversed: false,
        };
        let insert = create_from_spec(&mut scene, &[source], DVec3::ZERO, &spec).unwrap();
        (scene, insert)
    }

    #[test]
    fn path_count_arrow_fits_items_on_the_path() {
        let (scene, insert) = path(false);
        let array = array(&scene, insert);
        let grips = grips(&scene.document, &array);
        assert!(close(
            grip_at(&grips, Kind::ItemCount).unwrap(),
            DVec3::new(40.0, 0.0, 0.0)
        ));
        assert!(close(
            grip_at(&grips, Kind::ItemSpacing).unwrap(),
            DVec3::new(20.0, 0.0, 0.0)
        ));
        let count = drag(&scene.document, &array, Kind::ItemCount).unwrap();
        assert_eq!(
            count.changes(DVec3::new(75.0, 3.0, 0.0), false),
            vec![("Items", 5.0)]
        );
        // Past the end of the path: as many as fit (100 / 20 + 1).
        assert_eq!(
            count.changes(DVec3::new(200.0, 0.0, 0.0), false),
            vec![("Items", 6.0)]
        );
        assert_eq!(
            count.changes(DVec3::new(60.0, 0.0, 0.0), true),
            vec![("ItemSpacing", 30.0)]
        );
        let spacing = drag(&scene.document, &array, Kind::ItemSpacing).unwrap();
        assert_eq!(
            spacing.changes(DVec3::new(35.0, -2.0, 0.0), false),
            vec![("ItemSpacing", 35.0)]
        );
        // No more than the path holds for the items there are.
        assert_eq!(
            spacing.changes(DVec3::new(90.0, 0.0, 0.0), false),
            vec![("ItemSpacing", 50.0)]
        );
    }

    #[test]
    fn divided_path_has_no_spacing_arrow_and_counts_freely() {
        let (mut scene, insert) = path(true);
        let target = array(&scene, insert);
        let grips = grips(&scene.document, &target);
        assert!(grip_at(&grips, Kind::ItemSpacing).is_none());
        let last = grip_at(&grips, Kind::ItemCount).unwrap();
        assert!(close(last, DVec3::new(100.0, 0.0, 0.0)), "{last:?}");
        let count = drag(&scene.document, &target, Kind::ItemCount).unwrap();
        // Divide: 3 items 50 apart; pulled to 150 along means 4 items.
        let changes = count.changes(DVec3::new(150.0, 0.0, 0.0), false);
        assert_eq!(changes, vec![("Items", 4.0)]);
        array::update(&mut scene, &target, &changes).unwrap();
        assert_eq!(item_count(&scene, insert), 4);
    }

    #[test]
    fn array_grip_ids_round_trip_and_stay_clear_of_other_grips() {
        for kind in KINDS {
            assert_eq!(Kind::from_id(kind.id()), Some(kind));
            assert!(is_array_grip(kind.id()));
        }
        assert!(!is_array_grip(0));
        assert!(!is_array_grip(usize::MAX));
        assert!(!is_array_grip(usize::MAX / 2));
        assert!(!is_array_grip(usize::MAX / 4));
    }
}
