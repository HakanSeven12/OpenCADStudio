//! Create Points > Surface: COGO points that take a TIN surface's elevation
//! at picked locations, on a grid, along a polyline and at its vertices.

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, PointPickContext};
use crate::entities::curve::entity_curve;
use document::entities::ExtendedEntityData;
use document::{EntityType, Handle};
use glam::{DVec2, DVec3};
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Random,
    Grid,
    Along,
    Vertices,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Random => "CREATEPOINTRANDOMPOINTS",
            Kind::Grid => "CREATEPOINTONGRID",
            Kind::Along => "CREATEPTALONGPOLYCONTOUR",
            Kind::Vertices => "CREATEPTPOLYCONTOURVERT",
        }
    }

    pub fn of(command: &str) -> Option<Kind> {
        Some(match command {
            "CREATEPOINTRANDOMPOINTS" => Kind::Random,
            "CREATEPOINTONGRID" => Kind::Grid,
            "CREATEPTALONGPOLYCONTOUR" => Kind::Along,
            "CREATEPTPOLYCONTOURVERT" => Kind::Vertices,
            _ => return None,
        })
    }
}

fn t(s: &'static str) -> String {
    crate::t!(s).into_owned()
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Whether `e` is a TIN surface.
fn is_surface(e: &EntityType) -> bool {
    matches!(e, EntityType::Extended(x) if matches!(&x.data, ExtendedEntityData::Arch(a) if a.dxf_name == "AECC_TIN_SURFACE"))
}

/// A TIN surface: its name, drawn triangles and the contours its style shows.
struct Surface {
    handle: Handle,
    name: String,
    triangles: Vec<[DVec3; 3]>,
    contours: Vec<Vec<DVec3>>,
}

/// The distance from `p` to the polyline through `run`, in plan.
fn run_distance(run: &[DVec3], p: DVec2) -> f64 {
    run.windows(2)
        .map(|w| {
            let (a, b) = (w[0].truncate(), w[1].truncate());
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-24)).clamp(0.0, 1.0);
            (a + ab * t).distance(p)
        })
        .fold(f64::INFINITY, f64::min)
}

/// The points every `distance` along `run` from its start, measured in 3D.
fn along(run: &[DVec3], distance: f64) -> Vec<DVec2> {
    let mut out = Vec::new();
    let mut walked = 0.0;
    let mut k = 0u32;
    for w in run.windows(2) {
        let len = w[0].distance(w[1]);
        while distance * f64::from(k) <= walked + len + 1e-9 {
            let at = distance * f64::from(k) - walked;
            out.push(w[0].lerp(w[1], if len > 0.0 { at / len } else { 0.0 }).truncate());
            k += 1;
        }
        walked += len;
    }
    out
}

impl Surface {
    fn of(e: &EntityType, document: &document::Document) -> Option<Surface> {
        let EntityType::Extended(x) = e else { return None };
        let ExtendedEntityData::Arch(a) = &x.data else { return None };
        if a.dxf_name != "AECC_TIN_SURFACE" {
            return None;
        }
        let strings = a.decode().strings;
        // ponytail: the name is read from the record's strings (after a data
        // shortcut's `.dwg:` reference, else the last one written); a codec
        // accessor replaces this when one exists.
        let name = strings
            .iter()
            .position(|s| s.contains(".dwg:"))
            .and_then(|i| strings.get(i + 1))
            .or_else(|| strings.iter().rev().find(|s| !s.is_empty()))
            .cloned()
            .unwrap_or_default();
        let tin = a.tin_surface().or_else(|| a.tin_snapshot())?;
        let triangles = tin
            .visible_triangles()
            .map(|tri| tri.map(|v| DVec3::new(v.x, v.y, v.z)))
            .collect();
        let contours = crate::entities::civil::surface_contours(a, document)
            .into_iter()
            .map(|run| run.into_iter().map(|[x, y, z]| DVec3::new(x, y, z)).collect())
            .collect();
        Some(Surface { handle: x.common.handle, name, triangles, contours })
    }

    /// The distance from `p` to the surface's border (the triangle edges
    /// only one triangle uses).
    fn border_distance(&self, p: DVec2) -> f64 {
        let key = |v: DVec3| ((v.x * 1e6).round() as i64, (v.y * 1e6).round() as i64);
        let mut edges: std::collections::HashMap<_, (DVec2, DVec2, usize)> = std::collections::HashMap::new();
        for t in &self.triangles {
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                let k = if key(a) < key(b) { (key(a), key(b)) } else { (key(b), key(a)) };
                edges.entry(k).or_insert((a.truncate(), b.truncate(), 0)).2 += 1;
            }
        }
        edges
            .values()
            .filter(|e| e.2 == 1)
            .map(|&(a, b, _)| {
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared().max(1e-24)).clamp(0.0, 1.0);
                (a + ab * t).distance(p)
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// The surface's elevation at `p`; none outside it.
    fn elevation(&self, p: DVec2) -> Option<f64> {
        self.triangles.iter().find_map(|[a, b, c]| {
            let (a2, b2, c2) = (a.truncate(), b.truncate(), c.truncate());
            let d = (b2 - a2).perp_dot(c2 - a2);
            if d.abs() < 1e-12 {
                return None;
            }
            let u = (b2 - p).perp_dot(c2 - p) / d;
            let v = (c2 - p).perp_dot(a2 - p) / d;
            let w = 1.0 - u - v;
            let eps = 1e-9;
            (u >= -eps && v >= -eps && w >= -eps).then(|| u * a.z + v * b.z + w * c.z)
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Step {
    Surface,
    SurfaceName,
    /// The list window's pick from the drawing.
    SurfacePick,
    Location,
    Base,
    Rotation,
    XSpacing,
    YSpacing,
    UpperRight,
    Change,
    Distance,
    Polyline,
    Description,
}

pub struct SurfacePointCommand {
    kind: Kind,
    step: Step,
    surfaces: Vec<Surface>,
    surface: Option<usize>,
    picked: Option<EntityType>,
    context: Option<PointPickContext>,
    queue: VecDeque<(DVec3, f64)>,
    description: Option<String>,
    base: DVec3,
    rotation: f64,
    x_spacing: f64,
    y_spacing: f64,
    distance: f64,
    /// A distance prompt answered with a point: the second point closes it.
    first_point: Option<DVec3>,
    /// After `Yes` at the change prompt the grid is asked again from its
    /// rotation, then its base point.
    again: bool,
    /// The grid asks its next base point without the rotation.
    repeat: bool,
    /// The extent of the last grid given, along and across its rotation.
    extent: Option<(f64, f64)>,
    /// The surface the list window shows picked.
    list_pick: usize,
}

/// What the list window's pick button answers the list prompt with.
pub const PICK_SURFACE: &str = "_PICKSURFACE";

impl SurfacePointCommand {
    pub fn new(kind: Kind, document: &document::Document) -> Self {
        let surfaces: Vec<Surface> = document.entities().filter_map(|e| Surface::of(e, document)).collect();
        let mut c = SurfacePointCommand {
            kind,
            step: Step::Surface,
            surface: None,
            surfaces,
            picked: None,
            context: None,
            queue: VecDeque::new(),
            description: None,
            base: DVec3::ZERO,
            rotation: 0.0,
            x_spacing: 5.0,
            y_spacing: 5.0,
            distance: 10.0,
            first_point: None,
            again: false,
            repeat: false,
            extent: None,
            list_pick: 0,
        };
        if c.surfaces.len() == 1 {
            c.surface = Some(0);
            c.step = c.first_step();
        }
        c
    }

    /// The line naming the surface the command uses (`Surface:  SG`); in a
    /// drawing without surfaces the message the command ends with.
    pub fn intro(&self) -> Option<String> {
        if self.surfaces.is_empty() {
            return Some(t("There are no surfaces available."));
        }
        let s = self.surfaces.get(self.surface?)?;
        Some(crate::tf!("Surface:  {}", s.name).into_owned())
    }

    /// Whether the command ends at once (no surfaces, measured).
    pub fn ends_at_once(&self) -> bool {
        self.surfaces.is_empty()
    }

    fn first_step(&self) -> Step {
        match self.kind {
            Kind::Random => Step::Location,
            Kind::Grid => Step::Base,
            Kind::Along => Step::Distance,
            Kind::Vertices => Step::Polyline,
        }
    }

    fn chosen(&self) -> Option<&Surface> {
        self.surfaces.get(self.surface?)
    }

    /// Takes surface `i`, writes its line and asks the command's first prompt.
    fn take_surface(&mut self, i: usize) -> CmdResult {
        self.surface = Some(i);
        self.step = self.first_step();
        CmdResult::ReportMessage(self.intro().unwrap_or_default())
    }

    fn ask_next(&mut self) -> CmdResult {
        if self.queue.is_empty() {
            match self.kind {
                // Measured: after its points the grid names the surface
                // again and asks a new base point (the rotation is kept).
                Kind::Grid => {
                    // The new round offers the first X spacing again.
                    self.step = Step::Base;
                    self.repeat = true;
                    self.x_spacing = 5.0;
                    return CmdResult::ReportMessage(self.intro().unwrap_or_default());
                }
                Kind::Random => self.step = Step::Location,
                _ => self.step = Step::Polyline,
            }
        } else {
            self.step = Step::Description;
        }
        CmdResult::NeedPoint
    }

    /// Queues the points at `xy` the surface covers, with its elevation there.
    fn queue_on_surface(&mut self, xy: impl IntoIterator<Item = DVec2>) {
        let Some(s) = self.chosen() else { return };
        let points: Vec<(DVec3, f64)> = xy.into_iter().filter_map(|p| s.elevation(p).map(|z| (p.extend(0.0), z))).collect();
        self.queue.extend(points);
    }

    /// The grid's points: rows of columns from the base point, along the
    /// rotation, as many whole spacings as fit in the extent (measured).
    fn grid_points(&self, du: f64, dv: f64) -> Vec<DVec2> {
        let (u, v) = (DVec2::from_angle(self.rotation), DVec2::from_angle(self.rotation).perp());
        let (nu, nv) = ((du / self.x_spacing + 1e-9).floor() as u32, (dv / self.y_spacing + 1e-9).floor() as u32);
        let base = self.base.truncate();
        (0..=nv).flat_map(|j| (0..=nu).map(move |i| (i, j))).map(|(i, j)| base + u * self.x_spacing * f64::from(i) + v * self.y_spacing * f64::from(j)).collect()
    }

    /// The pick box half size around `pick`, in drawing units.
    fn tolerance(&self, pick: DVec3) -> Option<f64> {
        let c = self.context.as_ref()?;
        use crate::scene::view::projection::project_screen;
        let a = project_screen(c.view, c.eye, c.bounds, pick)?;
        let b = project_screen(c.view, c.eye, c.bounds, pick + DVec3::X)?;
        let per_px = 1.0 / f64::from(((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()).max(1e-12);
        Some(f64::from(c.aperture_px) * per_px)
    }

    /// The chosen surface's shown contour nearest `pick` within the pick box.
    fn contour_hit(&self, pick: DVec3) -> Option<&[DVec3]> {
        let tolerance = self.tolerance(pick)?;
        self.chosen()?
            .contours
            .iter()
            .map(|run| (run, run_distance(run, pick.truncate())))
            .filter(|&(_, d)| d <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(run, _)| run.as_slice())
    }

    /// The surface whose border lies within the pick box of `pick`.
    fn border_hit(&self, pick: DVec3) -> Option<usize> {
        let tolerance = self.tolerance(pick)?;
        self.surfaces
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.border_distance(pick.truncate())))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|&(_, d)| d <= tolerance)
            .map(|(i, _)| i)
    }

    /// The vertices and the points every `distance` along a picked polyline
    /// (from its start; a 3D polyline is measured in 3D).
    fn polyline_points(&self, e: &EntityType) -> Option<Vec<DVec2>> {
        let vertices3d = |v: Vec<DVec3>, closed: bool| -> Vec<DVec3> {
            let mut v = v;
            if closed && v.len() > 1 && v[0] != v[v.len() - 1] && self.kind == Kind::Along {
                v.push(v[0]);
            }
            v
        };
        match e {
            EntityType::Polyline3D(p) => {
                let v = vertices3d(p.vertices.iter().map(|v| DVec3::new(v.position.x, v.position.y, v.position.z)).collect(), p.is_closed());
                Some(self.run_points(&v))
            }
            EntityType::LwPolyline(p) if self.kind == Kind::Vertices => Some(p.vertices.iter().map(|v| DVec2::new(v.location.x, v.location.y)).collect()),
            EntityType::Polyline2D(p) if self.kind == Kind::Vertices => Some(p.vertices.iter().map(|v| DVec2::new(v.location.x, v.location.y)).collect()),
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_) => {
                let path = entity_curve(e)?;
                let len = path.length();
                let n = (len / self.distance + 1e-9).floor() as u32;
                Some((0..=n).map(|k| {
                    let p = path.point_at_distance(self.distance * f64::from(k));
                    DVec2::new(p[0], p[1])
                }).collect())
            }
            _ => None,
        }
    }

    /// A run's vertices, or its points every distance (by the command).
    fn run_points(&self, run: &[DVec3]) -> Vec<DVec2> {
        match self.kind {
            Kind::Vertices => run.iter().map(|p| p.truncate()).collect(),
            _ => along(run, self.distance),
        }
    }

    fn distance_given(&mut self, value: f64) -> Option<CmdResult> {
        if !(f64::EPSILON..=1e24).contains(&value) {
            return Some(CmdResult::ReportError(t("Please enter a value greater than or equal to 2.220446E-16 and  less than or equal to 1.000000E+24")));
        }
        match self.step {
            Step::XSpacing => {
                self.x_spacing = value;
                self.y_spacing = value;
                self.step = Step::YSpacing;
            }
            Step::YSpacing => {
                self.y_spacing = value;
                self.step = Step::UpperRight;
            }
            _ => {
                self.distance = value;
                self.step = Step::Polyline;
            }
        }
        Some(CmdResult::NeedPoint)
    }

    fn is_distance(&self) -> bool {
        matches!(self.step, Step::XSpacing | Step::YSpacing | Step::Distance)
    }
}

impl CadCommand for SurfacePointCommand {
    fn name(&self) -> &'static str {
        self.kind.name()
    }

    fn prompt(&self) -> String {
        if self.first_point.is_some() {
            return t("Specify second point:");
        }
        match &self.step {
            Step::Surface => t("Select a surface <or press Enter key to select from a list>:"),
            Step::SurfaceName => crate::tf!("Select a surface from the list <{}>:", self.surfaces.get(self.list_pick).map_or("", |s| s.name.as_str())).into_owned(),
            Step::SurfacePick => t("Select Surface:"),
            Step::Location => t("Please specify a location for the new point:"),
            Step::Base => t("Specify a grid basepoint:"),
            Step::Rotation => crate::tf!("Grid rotation <{:.4} (d)>:", self.rotation.to_degrees()).into_owned(),
            Step::XSpacing => crate::tf!("Grid X spacing <{:.4}>:", self.x_spacing).into_owned(),
            Step::YSpacing => crate::tf!("Grid Y spacing <{:.4}>:", self.y_spacing).into_owned(),
            Step::UpperRight => t("Specify the upper right location for the grid:"),
            Step::Change => t("Change the size or rotation of the grid/grid squares [Yes/No] <No>:"),
            Step::Distance => crate::tf!("Distance between points <{:.4}>:", self.distance).into_owned(),
            Step::Polyline => t("Select a polyline or contour:"),
            Step::Description => crate::tf!("Enter a point description <{}>:", self.description.as_deref().unwrap_or(".")).into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Change => vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            Step::SurfaceName => self.surfaces.iter().map(|s| CmdOption::new(&s.name, &s.name)).collect(),
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Surface | Step::SurfacePick | Step::Location | Step::Base | Step::UpperRight | Step::Polyline | Step::XSpacing | Step::YSpacing | Step::Distance => InputKind::Point,
            _ => InputKind::FreeText,
        }
    }

    fn groups_undo(&self) -> bool {
        true
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Surface | Step::SurfacePick | Step::Polyline) && self.first_point.is_none()
    }

    fn inject_before_entity_pick(&self) -> bool {
        self.needs_entity_pick()
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn surface_list(&self) -> Option<(Vec<String>, usize)> {
        (self.step == Step::SurfaceName).then(|| (self.surfaces.iter().map(|s| s.name.clone()).collect(), self.list_pick))
    }

    fn wants_point_pick_context(&self) -> bool {
        matches!(self.step, Step::Surface | Step::SurfacePick | Step::Polyline)
    }

    fn set_point_pick_context(&mut self, context: Option<PointPickContext>) {
        self.context = context;
    }

    fn on_entity_pick(&mut self, handle: Handle, pick: DVec3) -> CmdResult {
        let picked = self.picked.take();
        match self.step {
            Step::Surface | Step::SurfacePick => {
                let found = self.surfaces.iter().position(|s| s.handle == handle).or_else(|| self.border_hit(pick));
                // Measured: a surface is taken by its border within the
                // pick box; inside it nothing is selected. Picked from the
                // list window, the window comes back with it picked.
                match (found, picked) {
                    (Some(i), _) if self.step == Step::SurfacePick => {
                        self.list_pick = i;
                        self.step = Step::SurfaceName;
                        CmdResult::NeedPoint
                    }
                    (Some(i), _) => self.take_surface(i),
                    (_, None) => CmdResult::ReportError(t("Nothing selected.")),
                    _ => CmdResult::ReportError(t("Selected entity must be of type: Surface")),
                }
            }
            _ => {
                // A pick on the surface's border ends the command; on one of
                // the contours its style shows, that contour is used from its
                // start (measured).
                if self.border_hit(pick).is_some() {
                    return CmdResult::Done;
                }
                if let Some(points) = self.contour_hit(pick).map(|run| self.run_points(run)) {
                    self.queue_on_surface(points);
                    return self.ask_next();
                }
                if picked.as_ref().is_some_and(is_surface) {
                    return CmdResult::Done;
                }
                let Some(e) = picked else { return CmdResult::ReportError(t("ERROR: Invalid selection set!")) };
                let Some(points) = self.polyline_points(&e) else { return CmdResult::ReportError(t("ERROR: Invalid selection set!")) };
                self.queue_on_surface(points);
                self.ask_next()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.is_distance() {
            return match self.first_point.take() {
                None => {
                    self.first_point = Some(pt);
                    CmdResult::NeedPoint
                }
                Some(first) => self.distance_given(first.truncate().distance(pt.truncate())).unwrap_or(CmdResult::NeedPoint),
            };
        }
        match self.step {
            Step::Surface | Step::SurfacePick | Step::Polyline => self.on_entity_pick(Handle::NULL, pt),
            // Measured: a location off the surface ends the command.
            Step::Location => match self.chosen().and_then(|s| s.elevation(pt.truncate())) {
                Some(z) => {
                    self.queue.push_back((pt.truncate().extend(0.0), z));
                    self.ask_next()
                }
                None => CmdResult::Done,
            },
            Step::Base => {
                self.base = pt;
                self.step = if self.again || self.repeat { Step::XSpacing } else { Step::Rotation };
                self.again = false;
                CmdResult::NeedPoint
            }
            Step::UpperRight => {
                let (u, v) = (DVec2::from_angle(self.rotation), DVec2::from_angle(self.rotation).perp());
                let d = (pt - self.base).truncate();
                let (du, dv) = (d.dot(u), d.dot(v));
                let valid = du > 1e-9 && dv > 1e-9;
                if valid {
                    self.extent = Some((du, dv));
                }
                // Measured: a corner not above the base row and right of the
                // base column is refused; once a grid was given, the grid keeps
                // that extent (along and across) and the change prompt follows.
                let Some((du, dv)) = self.extent else {
                    return CmdResult::ReportError(t("The specified point was not a valid upper right point."));
                };
                // The points wait for the answer to the change prompt.
                self.queue.clear();
                self.queue_on_surface(self.grid_points(du, dv));
                self.step = Step::Change;
                if valid {
                    CmdResult::NeedPoint
                } else {
                    CmdResult::ReportMessage(t("The specified point was not a valid upper right point."))
                }
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let typed = text.trim();
        let coordinate = !typed.is_empty() && typed.chars().any(|c| c.is_ascii_digit()) && typed.contains(',');
        if self.first_point.take().is_some() {
            if coordinate {
                return None;
            }
            return Some(CmdResult::ReportError(t("Requires numeric distance, second point, or option keyword.")));
        }
        match self.step {
            Step::Surface => {
                if typed.is_empty() {
                    self.step = Step::SurfaceName;
                    return Some(CmdResult::NeedPoint);
                }
                if coordinate {
                    return None;
                }
                Some(CmdResult::ReportError(format!("{}\n{}", t("*Invalid selection*"), t("Expects a point or Last"))))
            }
            Step::SurfacePick => {
                if typed.is_empty() {
                    self.step = Step::SurfaceName;
                    return Some(CmdResult::NeedPoint);
                }
                if coordinate {
                    return None;
                }
                Some(CmdResult::ReportError(format!("{}\n{}", t("*Invalid selection*"), t("Expects a point or Last"))))
            }
            Step::SurfaceName if typed == PICK_SURFACE => {
                self.step = Step::SurfacePick;
                Some(CmdResult::NeedPoint)
            }
            Step::SurfaceName => {
                let i = if typed.is_empty() { Some(self.list_pick) } else { self.surfaces.iter().position(|s| s.name.eq_ignore_ascii_case(typed)) };
                match i {
                    Some(i) => Some(self.take_surface(i)),
                    None => Some(CmdResult::ReportError(t("Invalid input"))),
                }
            }
            Step::Location | Step::Base | Step::UpperRight => {
                if typed.is_empty() {
                    return Some(CmdResult::Done);
                }
                if coordinate {
                    return None;
                }
                if self.step == Step::UpperRight && number(typed).is_some() {
                    return Some(CmdResult::ReportError(t("The specified point was not a valid upper right point.")));
                }
                Some(CmdResult::ReportError(t("Point or option keyword required.")))
            }
            Step::Rotation => {
                if !typed.is_empty() {
                    match number(typed) {
                        Some(a) => self.rotation = a.to_radians(),
                        None => return Some(CmdResult::ReportError(t("Invalid input"))),
                    }
                }
                self.step = Step::Base;
                if !self.again {
                    self.step = Step::XSpacing;
                }
                Some(CmdResult::NeedPoint)
            }
            Step::XSpacing | Step::YSpacing | Step::Distance => {
                if coordinate {
                    return None;
                }
                let value = if typed.is_empty() {
                    match self.step {
                        Step::XSpacing => self.x_spacing,
                        Step::YSpacing => self.y_spacing,
                        _ => self.distance,
                    }
                } else {
                    match number(typed) {
                        Some(v) => v,
                        None => return Some(CmdResult::ReportError(t("Invalid Input"))),
                    }
                };
                self.distance_given(value)
            }
            Step::Change => match typed.to_ascii_uppercase().as_str() {
                "" | "N" | "NO" => Some(self.ask_next()),
                "Y" | "YES" => {
                    self.queue.clear();
                    self.again = true;
                    self.step = Step::Rotation;
                    Some(CmdResult::NeedPoint)
                }
                _ => Some(CmdResult::ReportError(t("Invalid option keyword."))),
            },
            Step::Polyline => {
                if typed.is_empty() {
                    return Some(CmdResult::Done);
                }
                if coordinate {
                    return None;
                }
                Some(CmdResult::ReportError(format!("{}\n{}", t("*Invalid selection*"), t("Expects a single object."))))
            }
            Step::Description => {
                let d = match typed {
                    "" => self.description.clone(),
                    "." => None,
                    t => Some(t.to_string()),
                };
                self.description = d.clone();
                let (at, z) = self.queue.pop_front()?;
                let then = match self.ask_next() {
                    CmdResult::ReportMessage(line) => Some(line),
                    _ => None,
                };
                Some(CmdResult::AddCogoPoint { position: at, description: d, elevation: Some(z), echo: false, then })
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::NeedPoint)
    }

    fn on_escape(&mut self) -> CmdResult {
        // Measured: Esc at a description of Along goes back to the polyline
        // prompt; every other prompt ends the command.
        self.queue.clear();
        match (self.kind, &self.step) {
            (Kind::Along | Kind::Vertices, Step::Description) => {
                self.step = Step::Polyline;
                CmdResult::ReportMessage(t("*Cancel*"))
            }
            _ => CmdResult::Cancel,
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["CREATEPOINTRANDOMPOINTS", "CREATEPOINTONGRID", "CREATEPTALONGPOLYCONTOUR", "CREATEPTPOLYCONTOURVERT"]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_triangle_gives_its_plane_and_nothing_outside() {
        let s = Surface { handle: Handle::NULL, name: "S".into(), contours: Vec::new(), triangles: vec![[DVec3::new(0.0, 0.0, 10.0), DVec3::new(10.0, 0.0, 20.0), DVec3::new(0.0, 10.0, 30.0)]] };
        assert!((s.elevation(DVec2::new(2.0, 3.0)).unwrap() - (10.0 + 2.0 + 6.0)).abs() < 1e-9);
        assert!(s.elevation(DVec2::new(8.0, 8.0)).is_none());
    }

    #[test]
    fn a_contour_is_walked_from_its_start() {
        let run = [DVec3::new(0.0, 0.0, 5.0), DVec3::new(30.0, 0.0, 5.0), DVec3::new(30.0, 25.0, 5.0)];
        assert_eq!(along(&run, 20.0), vec![DVec2::new(0.0, 0.0), DVec2::new(20.0, 0.0), DVec2::new(30.0, 10.0)]);
        assert!((run_distance(&run, DVec2::new(31.0, 12.0)) - 1.0).abs() < 1e-12);
    }
}
