//! CREATEPTGEODETICDIR: a COGO point a geodesic distance along a geodetic
//! azimuth from a start point, on the drawing coordinate system's ellipsoid.
//! The start and the computed point are echoed with their grid coordinates,
//! meridian convergence, scale factor and latitude/longitude. Drawing
//! (local) coordinates turn into grid coordinates by the drawing's
//! transformation (`LocalGrid`), else they are the grid coordinates.

use crate::command::{CadCommand, CmdResult, InputKind};
use crate::io::crs::CrsDefinition;
use glam::{DVec2, DVec3};
use proj4rs_geodesic::Geodesic;

/// The drawing's local-to-grid transformation, as the drawing's GEODATA
/// keeps it: the local reference point (design point) sits at the grid
/// reference point; local axes turn by `rotation` (counter-clockwise, from
/// the north direction) and scale by the grid scale factor.
#[derive(Clone, Copy)]
pub struct LocalGrid {
    pub local: DVec2,
    pub grid: DVec2,
    pub rotation: f64,
    pub scale: Scale,
}

/// How the grid scale factor is found (GEODATA's scale estimation method).
#[derive(Clone, Copy)]
pub enum Scale {
    /// 1: unity.
    Unity,
    /// 2: the user's factor.
    User(f64),
    /// 3 and 4: the projection's point scale at the grid reference point
    /// (measured with 3; 4 gave the same near the reference point).
    ReferencePoint,
}

impl LocalGrid {
    /// From the drawing's GEODATA; none while its reference point is still
    /// a longitude/latitude (no transformation was set up).
    // ponytail: a set-up transformation that is switched off is not told
    // apart (the switch was not found in the drawing); use it once it is.
    pub fn of(geo: &document::objects::GeoData) -> Option<LocalGrid> {
        let r = geo.reference_point;
        if r.x.abs() <= 180.0 && r.y.abs() <= 90.0 {
            return None;
        }
        let n = geo.north_direction;
        Some(LocalGrid {
            local: DVec2::new(geo.design_point.x, geo.design_point.y),
            grid: DVec2::new(r.x, r.y),
            rotation: n.x.atan2(n.y),
            scale: match geo.scale_estimation_method {
                2 => Scale::User(geo.user_scale_factor),
                3 | 4 => Scale::ReferencePoint,
                _ => Scale::Unity,
            },
        })
    }
}

enum Step {
    Start,
    Azimuth(DVec3),
    Distance(DVec3, f64),
    /// A distance given as two points: the first one.
    SecondPoint(DVec3, f64, DVec3),
    Description(DVec3),
    Elevation(DVec3, Option<String>),
}

pub struct GeodeticDirectCommand {
    crs: CrsDefinition,
    geodesic: Geodesic,
    /// The local-to-grid transformation and its grid scale factor.
    transform: Option<(LocalGrid, f64)>,
    step: Step,
    /// The azimuth, distance and description the prompts offer: the last ones
    /// given in this run.
    azimuth: f64,
    distance: f64,
    description: Option<String>,
}

fn t(s: &'static str) -> String {
    crate::t!(s).into_owned()
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `deg° mm' ss.ss"` with `places` decimals on the seconds; the sign leads
/// even when the degrees are zero.
fn dms(value: f64, places: usize) -> String {
    let scale = 10f64.powi(places as i32);
    let total = (value.abs() * 3600.0 * scale).round() / scale;
    let d = (total / 3600.0).floor();
    let m = ((total - d * 3600.0) / 60.0).floor();
    let s = total - d * 3600.0 - m * 60.0;
    let sign = if value < 0.0 && total > 0.0 { "-" } else { "" };
    let width = if places == 0 { 2 } else { places + 3 };
    format!("{sign}{d}° {m:02}' {s:0width$.places$}\"")
}

impl GeodeticDirectCommand {
    pub fn new(crs: CrsDefinition, transform: Option<LocalGrid>) -> Self {
        let (a, b) = crs.ellipsoid();
        let mut c = Self { geodesic: Geodesic::new(a, (a - b) / a), crs, transform: None, step: Step::Start, azimuth: 0.0, distance: 1.0, description: None };
        c.transform = transform.map(|t| {
            let k = match t.scale {
                Scale::Unity => 1.0,
                Scale::User(k) => k,
                Scale::ReferencePoint => c.point_scale(t.grid.extend(0.0)).unwrap_or(1.0),
            };
            (t, k)
        });
        c
    }

    /// The grid coordinate of a local one.
    fn grid(&self, at: DVec3) -> DVec3 {
        match self.transform {
            Some((t, k)) => (t.grid + DVec2::from_angle(t.rotation).rotate(at.truncate() - t.local) * k).extend(0.0),
            None => DVec3::new(at.x, at.y, 0.0),
        }
    }

    /// The local coordinate of a grid one.
    fn local(&self, at: DVec3) -> DVec3 {
        match self.transform {
            Some((t, k)) => (t.local + DVec2::from_angle(-t.rotation).rotate(at.truncate() - t.grid) / k).extend(0.0),
            None => at,
        }
    }

    /// Latitude and longitude of a grid coordinate.
    fn geographic(&self, at: DVec3) -> Option<(f64, f64)> {
        self.crs.to_base_geographic(at.x, at.y).ok().map(|(lon, lat)| (lat, lon))
    }

    /// The projection's scale at a grid coordinate: the grid length of a
    /// 1 m geodesic step east.
    fn point_scale(&self, at: DVec3) -> Option<f64> {
        let (lat, lon) = self.geographic(at)?;
        let (lat2, lon2, _) = self.geodesic.direct(lat, lon, 90.0, 1.0);
        let (x, y) = self.crs.from_base_geographic(lon2, lat2).ok()?;
        Some((x - at.x).hypot(y - at.y) * self.crs.to_meter())
    }

    /// The four lines describing a local coordinate and its grid coordinate.
    fn describe(&self, local: DVec3) -> String {
        let at = self.grid(local);
        let Some((lat, lon)) = self.geographic(at) else { return String::new() };
        // Convergence: the grid direction of the meridian; scale: grid over
        // ellipsoid length of a short east step.
        let step = 1.0;
        let grid = |la: f64, lo: f64| self.crs.from_base_geographic(lo, la).unwrap_or((at.x, at.y));
        let (lat2, lon2, _) = self.geodesic.direct(lat, lon, 0.0, step);
        let (n_x, n_y) = grid(lat2, lon2);
        let convergence = -(n_x - at.x).atan2(n_y - at.y).to_degrees();
        let scale = self.point_scale(at).unwrap_or(1.0);
        let hemi = |v: f64, pos: char, neg: char| format!("{}{}", if v < 0.0 { neg } else { pos }, dms(v.abs(), 2));
        [
            crate::tf!("Northing: {:.4}m   Easting: {:.4}m", local.y, local.x).into_owned(),
            crate::tf!("Grid Northing: {:.4}m   Grid Easting: {:.4}m", at.y, at.x).into_owned(),
            crate::tf!("Convergence: {}  Scale Factor: {:.3}", dms(convergence, 2), scale).into_owned(),
            crate::tf!("Latitude: {}   Longitude: {}", hemi(lat, 'N', 'S'), hemi(lon, 'E', 'W')).into_owned(),
        ]
        .join("\n")
    }

    /// The local point `distance` along the geodesic leaving `from` at
    /// `azimuth`.
    fn travel(&self, from: DVec3, azimuth: f64, distance: f64) -> Option<DVec3> {
        let (lat, lon) = self.geographic(self.grid(from))?;
        let (lat2, lon2, _) = self.geodesic.direct(lat, lon, azimuth, distance * self.crs.to_meter());
        let (x, y) = self.crs.from_base_geographic(lon2, lat2).ok()?;
        Some(self.local(DVec3::new(x, y, 0.0)))
    }

    fn started(&mut self, at: DVec3) -> CmdResult {
        let at = DVec3::new(at.x, at.y, 0.0);
        self.step = Step::Azimuth(at);
        CmdResult::ReportMessage(self.describe(at))
    }

    fn distance_given(&mut self, from: DVec3, azimuth: f64, d: f64) -> CmdResult {
        if d < f64::EPSILON || d > 1e24 {
            self.step = Step::Distance(from, azimuth);
            return CmdResult::ReportError(t("Please enter a value greater than or equal to 2.220446E-16 and  less than or equal to 1.000000E+24"));
        }
        match self.travel(from, azimuth, d) {
            Some(at) => {
                self.azimuth = azimuth;
                self.distance = d;
                self.step = Step::Description(at);
                CmdResult::ReportMessage(self.describe(at))
            }
            None => {
                self.step = Step::Distance(from, azimuth);
                CmdResult::ReportError(t("Invalid input"))
            }
        }
    }
}

impl CadCommand for GeodeticDirectCommand {
    fn name(&self) -> &'static str {
        "CREATEPTGEODETICDIR"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Start => t("Please specify a location for the start point:"),
            Step::Azimuth(_) => crate::tf!("Geodetic azimuth <{}>:", dms(self.azimuth, 0)).into_owned(),
            Step::Distance(..) => crate::tf!("Geodesic distance <{:.3}>:", self.distance).into_owned(),
            Step::SecondPoint(..) => t("Specify second point:"),
            Step::Description(_) => crate::tf!("Enter a point description <{}>:", self.description.as_deref().unwrap_or(".")).into_owned(),
            Step::Elevation(..) => t("Specify a point elevation <.>:"),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            // The distance also takes two points (measured).
            Step::Start | Step::Distance(..) | Step::SecondPoint(..) => InputKind::Point,
            _ => InputKind::FreeText,
        }
    }

    fn groups_undo(&self) -> bool {
        true
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::Start => self.started(pt),
            Step::Distance(from, azimuth) => {
                self.step = Step::SecondPoint(from, azimuth, pt);
                CmdResult::NeedPoint
            }
            Step::SecondPoint(from, azimuth, first) => self.distance_given(from, azimuth, (pt - first).truncate().length()),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let typed = text.trim();
        Some(match std::mem::replace(&mut self.step, Step::Start) {
            Step::Start => CmdResult::ReportError(t("Point or option keyword required.")),
            second @ Step::SecondPoint(..) => {
                self.step = second;
                return None;
            }
            Step::Azimuth(from) => {
                let a = if typed.is_empty() { Some(self.azimuth) } else { number(typed) };
                match a {
                    Some(a) if (-360.0..=360.0).contains(&a) => {
                        self.azimuth = a;
                        self.step = Step::Distance(from, a);
                        CmdResult::NeedPoint
                    }
                    Some(a) => {
                        self.step = Step::Azimuth(from);
                        CmdResult::ReportError(format!(
                            "{}\n{}",
                            crate::tf!("'Transparent Commands Angle Property' validation error - value '{:.4} (d)' out of range.", a),
                            t("Please enter a value greater than or equal to -360.0000 (d) and less than or equal to 360.0000 (d)")
                        ))
                    }
                    None => {
                        self.step = Step::Azimuth(from);
                        CmdResult::ReportError(t("Invalid input"))
                    }
                }
            }
            Step::Distance(from, azimuth) => match if typed.is_empty() { Some(self.distance) } else { number(typed) } {
                Some(d) => self.distance_given(from, azimuth, d),
                None => {
                    self.step = Step::Distance(from, azimuth);
                    CmdResult::ReportError(t("Requires numeric distance, second point, or option keyword."))
                }
            },
            Step::Description(at) => {
                let d = match typed {
                    "" => self.description.clone(),
                    "." => None,
                    t => Some(t.to_string()),
                };
                self.description = d.clone();
                self.step = Step::Elevation(at, d);
                CmdResult::NeedPoint
            }
            Step::Elevation(at, d) => {
                let elevation = match typed {
                    "" | "." => None,
                    _ => match number(typed) {
                        Some(z) => Some(z),
                        None => {
                            self.step = Step::Elevation(at, d);
                            return Some(CmdResult::ReportError(t("Invalid input")));
                        }
                    },
                };
                CmdResult::AddCogoPoint { position: at, description: d, elevation, echo: false, then: None }
            }
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Start => CmdResult::Done,
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lands_where_measured() {
        // UTM zone 35N (EPSG:32635): 1000 m at 45° from (500000, 4500000).
        let mut c = GeodeticDirectCommand::new(CrsDefinition::from_epsg(32635).unwrap(), None);
        c.on_point(DVec3::new(500000.0, 4500000.0, 0.0));
        c.on_text_input("45");
        c.on_text_input("1000");
        let Step::Description(p) = c.step else { panic!("no point") };
        assert!((p.x - 500706.8239).abs() < 1e-3 && (p.y - 4500706.8239).abs() < 1e-3);
        assert_eq!(dms(-0.7462917, 2), "-0° 44' 46.65\"");
        assert_eq!(dms(0.0, 0), "0° 00' 00\"");
        // Local (123.5, 456.25) at grid (501234.5, 4501234.25), turned -25.5°,
        // user scale 0.9998: local (300, 200) is grid (501283.4780, 4500927.0388).
        let t = LocalGrid { local: DVec2::new(123.5, 456.25), grid: DVec2::new(501234.5, 4501234.25), rotation: (-25.5f64).to_radians(), scale: Scale::User(0.9998) };
        let c = GeodeticDirectCommand::new(CrsDefinition::from_epsg(32635).unwrap(), Some(t));
        let g = c.grid(DVec3::new(300.0, 200.0, 0.0));
        assert!((g.x - 501283.4780).abs() < 1e-3 && (g.y - 4500927.0388).abs() < 1e-3);
        let l = c.local(g);
        assert!((l.x - 300.0).abs() < 1e-9 && (l.y - 200.0).abs() < 1e-9);
    }
}
