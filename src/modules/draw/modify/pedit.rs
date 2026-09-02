// PEDIT edits polylines and polygon meshes.

use acadrust::entities::LwVertex;
use acadrust::types::Vector2;
use acadrust::{EntityType, Handle};
use cadkernel::geom2d::nurbs::clamped_uniform_knots;
use cadkernel::geom2d::NurbsCurve;
use glam::{DVec2, DVec3};
use rustc_hash::FxHashMap as HashMap;

use crate::command::{CadCommand, CmdResult};
use crate::t;

const TAU: f64 = std::f64::consts::TAU;

/// What PEDIT knows about a pickable entity, captured at dispatch.
#[derive(Clone, Copy)]
pub struct PeditTarget {
    /// LwPolyline / Polyline2D — a valid edit target.
    pub is_poly: bool,
    /// Line / Arc — offered for conversion on pick.
    pub convertible: bool,
    /// M/N size for a legacy polygon mesh; absent for ordinary polylines.
    pub mesh_size: Option<(usize, usize)>,
    /// Current M/N closure state for a polygon mesh. The option list exposes
    /// only the operation applicable to each direction.
    pub mesh_closed: Option<(bool, bool)>,
}

enum Mode {
    PickTarget,
    Options,
    /// Picked a Line/Arc: asking "Turn it into one? [Yes/No]".
    ConvertPrompt(Handle),
    AwaitWidth,
    /// Join: gathering additional segments; Enter merges.
    JoinGather(Vec<Handle>),
    /// Polygon-mesh vertex navigation uses the mesh's row-major control net.
    MeshVertex(usize),
    /// Waiting for the replacement location of the selected mesh vertex.
    MeshVertexMove(usize),
}

pub struct PeditCommand {
    target: Option<Handle>,
    info: HashMap<u64, PeditTarget>,
    mode: Mode,
    undo_count: usize,
    mesh_smooth_type: acadrust::entities::polygon_mesh::SurfaceSmoothType,
    mesh_smooth_density: (i16, i16),
    mesh_vertex_default: isize,
    pending_mesh_closed: Option<(bool, bool)>,
    mesh_closed_history: Vec<Option<(bool, bool)>>,
}

impl PeditCommand {
    pub fn new(
        info: HashMap<u64, PeditTarget>,
        surface_type: i16,
        surface_u_density: i16,
        surface_v_density: i16,
    ) -> Self {
        use acadrust::entities::polygon_mesh::SurfaceSmoothType;

        let mesh_smooth_type = match surface_type {
            5 => SurfaceSmoothType::Quadratic,
            8 => SurfaceSmoothType::Bezier,
            _ => SurfaceSmoothType::Cubic,
        };
        Self {
            target: None,
            info,
            mode: Mode::PickTarget,
            undo_count: 0,
            mesh_smooth_type,
            mesh_smooth_density: (
                surface_u_density.clamp(2, 200),
                surface_v_density.clamp(2, 200),
            ),
            mesh_vertex_default: 1,
            pending_mesh_closed: None,
            mesh_closed_history: Vec::new(),
        }
    }

    /// Adopt a pre-selected entity (pickfirst): a selected polyline skips the
    /// pick step, a selected line/arc goes straight to the convert prompt.
    pub fn with_preselection(mut self, handles: &[Handle]) -> Self {
        for &h in handles {
            let Some(info) = self.info.get(&h.value()).copied() else {
                continue;
            };
            if info.is_poly {
                self.target = Some(h);
                self.mode = Mode::Options;
                break;
            }
            if info.convertible {
                self.mode = Mode::ConvertPrompt(h);
                break;
            }
        }
        self
    }

    fn mesh_size(&self) -> Option<(usize, usize)> {
        let handle = self.target?;
        self.info.get(&handle.value())?.mesh_size
    }

    fn mesh_closed(&self) -> Option<(bool, bool)> {
        let handle = self.target?;
        self.info.get(&handle.value())?.mesh_closed
    }

    fn set_mesh_closed(&mut self, m_direction: bool, closed: bool) {
        let Some(handle) = self.target else {
            return;
        };
        let Some(target) = self.info.get_mut(&handle.value()) else {
            return;
        };
        let Some((closed_m, closed_n)) = target.mesh_closed.as_mut() else {
            return;
        };
        if m_direction {
            *closed_m = closed;
        } else {
            *closed_n = closed;
        }
    }

    fn replace_mesh_closed(&mut self, closed: (bool, bool)) {
        let Some(handle) = self.target else {
            return;
        };
        if let Some(target) = self.info.get_mut(&handle.value()) {
            target.mesh_closed = Some(closed);
        }
    }
}

impl CadCommand for PeditCommand {
    fn name(&self) -> &'static str {
        "PEDIT"
    }

    fn prompt(&self) -> String {
        match &self.mode {
            Mode::PickTarget => {
                t!("PEDIT  Select polyline (or a line/arc to convert):").into_owned()
            }
            Mode::ConvertPrompt(_) => t!(
                "PEDIT  Object is not a polyline. Turn it into one?  [Yes/No] <Y>:"
            )
            .into_owned(),
            Mode::AwaitWidth => t!("PEDIT  Specify new width:").into_owned(),
            Mode::JoinGather(list) => t!(
                "PEDIT Join  Select objects to join (%{count} picked), Enter to merge:",
                count = list.len().saturating_sub(1)
            )
            .into_owned(),
            Mode::MeshVertex(index) => t!(
                "PEDIT  Edit mesh vertex %{vertex}  Enter option:",
                vertex = index + 1
            )
            .into_owned(),
            Mode::MeshVertexMove(index) => t!(
                "PEDIT  Specify new location for mesh vertex %{vertex}:",
                vertex = index + 1
            )
            .into_owned(),
            Mode::Options => t!("PEDIT  Enter option:").into_owned(),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match &self.mode {
            Mode::Options if self.mesh_size().is_some() => {
                let (closed_m, closed_n) = self.mesh_closed().unwrap_or((false, false));
                vec![
                    CmdOption::new(t!("Edit vertex").as_ref(), "E"),
                    CmdOption::new(t!("Smooth surface").as_ref(), "S"),
                    CmdOption::new(t!("Desmooth").as_ref(), "D"),
                    if closed_m {
                        CmdOption::new(t!("M open").as_ref(), "MO")
                    } else {
                        CmdOption::new(t!("M close").as_ref(), "MC")
                    },
                    if closed_n {
                        CmdOption::new(t!("N open").as_ref(), "NO")
                    } else {
                        CmdOption::new(t!("N close").as_ref(), "NC")
                    },
                    CmdOption::new(t!("Undo").as_ref(), "U"),
                    CmdOption::new(t!("eXit").as_ref(), "X"),
                ]
            }
            Mode::Options => vec![
                CmdOption::new(t!("Close").as_ref(), "C"),
                CmdOption::new(t!("Open").as_ref(), "O"),
                CmdOption::new(t!("Join").as_ref(), "J"),
                CmdOption::new(t!("Width").as_ref(), "W"),
                CmdOption::new(t!("Fit").as_ref(), "F"),
                CmdOption::new(t!("Spline").as_ref(), "S"),
                CmdOption::new(t!("Decurve").as_ref(), "D"),
                CmdOption::new(t!("eXit").as_ref(), "X"),
            ],
            Mode::MeshVertex(_) => vec![
                CmdOption::new(t!("Next").as_ref(), "N"),
                CmdOption::new(t!("Previous").as_ref(), "P"),
                CmdOption::new(t!("Left").as_ref(), "L"),
                CmdOption::new(t!("Right").as_ref(), "R"),
                CmdOption::new(t!("Up").as_ref(), "U"),
                CmdOption::new(t!("Down").as_ref(), "D"),
                CmdOption::new(t!("Move").as_ref(), "M"),
                CmdOption::new(t!("Regen").as_ref(), "G"),
                CmdOption::new(t!("eXit").as_ref(), "X"),
            ],
            Mode::ConvertPrompt(_) => {
                vec![CmdOption::new(t!("Yes").as_ref(), "Y"), CmdOption::new(t!("No").as_ref(), "N")]
            }
            Mode::JoinGather(_) => vec![CmdOption::enter(t!("Join").as_ref())],
            Mode::MeshVertexMove(_) => vec![],
            _ => vec![],
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.mode, Mode::PickTarget)
    }

    fn is_selection_gathering(&self) -> bool {
        // Join uses the normal selection system, so single picks AND
        // window/crossing boxes both gather objects.
        matches!(self.mode, Mode::JoinGather(_))
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if let (Some(target), Mode::JoinGather(list)) = (self.target, &mut self.mode) {
            list.clear();
            list.push(target);
            for h in handles {
                if h != target && self.info.contains_key(&h.value()) && !list.contains(&h) {
                    list.push(h);
                }
            }
        }
        CmdResult::NeedPoint
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match &mut self.mode {
            Mode::PickTarget => {
                let Some(info) = self.info.get(&handle.value()).copied() else {
                    return CmdResult::NeedPoint;
                };
                if info.is_poly {
                    self.target = Some(handle);
                    self.mode = Mode::Options;
                } else if info.convertible {
                    self.mode = Mode::ConvertPrompt(handle);
                }
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_entity_replaced(&mut self, old: Handle, new_handles: &[Handle]) {
        // A Yes-conversion (or Break) replaced the entity — adopt the first
        // piece as the live target and carry its bookkeeping over.
        if let Some(&nh) = new_handles.first() {
            self.info.remove(&old.value());
            self.info.insert(
                nh.value(),
                PeditTarget {
                    is_poly: true,
                    convertible: false,
                    mesh_size: None,
                    mesh_closed: None,
                },
            );
            self.target = Some(nh);
            self.mode = Mode::Options;
        }
    }

    fn on_pedit_applied(&mut self) {
        self.undo_count = self.undo_count.saturating_add(1);
        if let Some((m_direction, closed)) = self.pending_mesh_closed.take() {
            let previous = self.mesh_closed();
            self.mesh_closed_history.push(previous);
            self.set_mesh_closed(m_direction, closed);
        } else {
            self.mesh_closed_history.push(None);
        }
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.mode, Mode::PickTarget | Mode::MeshVertexMove(_))
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        self.pending_mesh_closed = None;
        let up = text.trim().to_uppercase();
        let mesh_size = self.mesh_size();
        match &mut self.mode {
            Mode::PickTarget => None,
            Mode::ConvertPrompt(handle) => {
                let handle = *handle;
                match up.as_str() {
                    "Y" | "YES" | "" => Some(CmdResult::PeditOp {
                        handle,
                        op: PeditOp::ConvertToPolyline,
                    }),
                    "N" | "NO" => {
                        self.mode = Mode::PickTarget;
                        Some(CmdResult::NeedPoint)
                    }
                    _ => Some(CmdResult::NeedPoint),
                }
            }
            Mode::AwaitWidth => {
                let handle = self.target?;
                let w: f64 = up
                    .replace(',', ".")
                    .parse()
                    .ok()
                    .filter(|&v: &f64| v >= 0.0)?;
                self.mode = Mode::Options;
                Some(CmdResult::PeditOp {
                    handle,
                    op: PeditOp::SetWidth(w),
                })
            }
            Mode::JoinGather(_) => None,
            Mode::MeshVertexMove(_) => None,
            Mode::MeshVertex(index) => {
                let (m, n) = mesh_size?;
                let count = m.saturating_mul(n);
                if count == 0 {
                    self.mode = Mode::Options;
                    return Some(CmdResult::NeedPoint);
                }
                let row = *index / n;
                let column = *index % n;
                match up.as_str() {
                    "N" | "NEXT" => {
                        self.mesh_vertex_default = 1;
                        if *index + 1 < count {
                            *index += 1;
                        }
                    }
                    "P" | "PREVIOUS" => {
                        self.mesh_vertex_default = -1;
                        *index = index.saturating_sub(1);
                    }
                    "L" | "LEFT" if column > 0 => *index -= 1,
                    "R" | "RIGHT" if column + 1 < n => *index += 1,
                    "U" | "UP" if row + 1 < m => *index += n,
                    "D" | "DOWN" if row > 0 => *index -= n,
                    "L" | "LEFT" | "R" | "RIGHT" | "U" | "UP" | "D" | "DOWN" => {}
                    "M" | "MOVE" => self.mode = Mode::MeshVertexMove(*index),
                    "G" | "REGEN" => {
                        // Mesh display is regenerated after every edit; keep the
                        // command active without creating a false undo record.
                        return Some(CmdResult::NeedPoint);
                    }
                    "X" | "EXIT" => self.mode = Mode::Options,
                    _ => return None,
                }
                Some(CmdResult::NeedPoint)
            }
            Mode::Options => {
                let handle = self.target?;
                if mesh_size.is_some() {
                    return match up.as_str() {
                        "E" | "EDIT" | "EDIT VERTEX" => {
                            self.mode = Mode::MeshVertex(0);
                            Some(CmdResult::NeedPoint)
                        }
                        "S" | "SMOOTH" | "SMOOTH SURFACE" => Some(CmdResult::PeditOp {
                            handle,
                            op: PeditOp::SetMeshSmooth {
                                smooth: self.mesh_smooth_type,
                                m_density: self.mesh_smooth_density.0,
                                n_density: self.mesh_smooth_density.1,
                            },
                        }),
                        "D" | "DESMOOTH" => Some(CmdResult::PeditOp {
                            handle,
                            op: PeditOp::SetMeshSmooth {
                                smooth:
                                    acadrust::entities::polygon_mesh::SurfaceSmoothType::NoSmooth,
                                m_density: self.mesh_smooth_density.0,
                                n_density: self.mesh_smooth_density.1,
                            },
                        }),
                        "MC" | "MCLOSE" | "M CLOSE" => {
                            self.pending_mesh_closed = Some((true, true));
                            Some(CmdResult::PeditOp {
                                handle,
                                op: PeditOp::SetMeshClosedM(true),
                            })
                        }
                        "MO" | "MOPEN" | "M OPEN" => {
                            self.pending_mesh_closed = Some((true, false));
                            Some(CmdResult::PeditOp {
                                handle,
                                op: PeditOp::SetMeshClosedM(false),
                            })
                        }
                        "NC" | "NCLOSE" | "N CLOSE" => {
                            self.pending_mesh_closed = Some((false, true));
                            Some(CmdResult::PeditOp {
                                handle,
                                op: PeditOp::SetMeshClosedN(true),
                            })
                        }
                        "NO" | "NOPEN" | "N OPEN" => {
                            self.pending_mesh_closed = Some((false, false));
                            Some(CmdResult::PeditOp {
                                handle,
                                op: PeditOp::SetMeshClosedN(false),
                            })
                        }
                        "U" | "UNDO" if self.undo_count > 0 => {
                            self.undo_count -= 1;
                            if let Some(Some(closed)) = self.mesh_closed_history.pop() {
                                self.replace_mesh_closed(closed);
                            }
                            Some(CmdResult::UndoDocument)
                        }
                        "U" | "UNDO" => Some(CmdResult::NeedPoint),
                        "X" | "EXIT" => Some(CmdResult::Cancel),
                        _ => None,
                    };
                }
                match up.as_str() {
                    "X" | "EXIT" => Some(CmdResult::Cancel),
                    "C" | "CLOSE" => Some(CmdResult::PeditOp {
                        handle,
                        op: PeditOp::SetClosed(true),
                    }),
                    "O" | "OPEN" => Some(CmdResult::PeditOp {
                        handle,
                        op: PeditOp::SetClosed(false),
                    }),
                    "W" | "WIDTH" => {
                        self.mode = Mode::AwaitWidth;
                        Some(CmdResult::NeedPoint)
                    }
                    "J" | "JOIN" => {
                        self.mode = Mode::JoinGather(vec![handle]);
                        Some(CmdResult::NeedPoint)
                    }
                    "F" | "FIT" => Some(CmdResult::PeditOp {
                        handle,
                        op: PeditOp::Fit,
                    }),
                    "S" | "SPLINE" => Some(CmdResult::PeditOp {
                        handle,
                        op: PeditOp::Spline,
                    }),
                    "D" | "DECURVE" => Some(CmdResult::PeditOp {
                        handle,
                        op: PeditOp::Decurve,
                    }),
                    _ => {
                        // Inline shorthand `W <value>`.
                        if let Some(rest) = up.strip_prefix("W ") {
                            let w: f64 = rest.trim().replace(',', ".").parse().ok()?;
                            if w >= 0.0 {
                                return Some(CmdResult::PeditOp {
                                    handle,
                                    op: PeditOp::SetWidth(w),
                                });
                            }
                        }
                        None
                    }
                }
            }
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if let Mode::MeshVertexMove(index) = self.mode {
            let Some(handle) = self.target else {
                return CmdResult::Cancel;
            };
            self.mode = Mode::MeshVertex(index);
            return CmdResult::PeditOp {
                handle,
                op: PeditOp::MoveMeshVertex { index, point },
            };
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        let mesh_size = self.mesh_size();
        let vertex_default = self.mesh_vertex_default;
        match &mut self.mode {
            Mode::JoinGather(list) if list.len() >= 2 => CmdResult::JoinEntities(list.clone()),
            Mode::JoinGather(_) => {
                self.mode = Mode::Options;
                CmdResult::NeedPoint
            }
            Mode::ConvertPrompt(h) => CmdResult::PeditOp {
                handle: *h,
                op: PeditOp::ConvertToPolyline,
            },
            Mode::MeshVertex(index) => {
                let Some((m, n)) = mesh_size else {
                    return CmdResult::NeedPoint;
                };
                let count = m.saturating_mul(n);
                if vertex_default >= 0 {
                    if *index + 1 < count {
                        *index += 1;
                    }
                } else {
                    *index = index.saturating_sub(1);
                }
                CmdResult::NeedPoint
            }
            _ => CmdResult::Cancel,
        }
    }
}

// ── Op enum (used in CmdResult) ────────────────────────────────────────────

#[derive(Clone)]
pub enum PeditOp {
    SetClosed(bool),
    SetWidth(f64),
    /// Replace the picked Line/Arc with an equivalent LwPolyline (#263).
    ConvertToPolyline,
    Fit,
    Spline,
    Decurve,
    SetMeshClosedM(bool),
    SetMeshClosedN(bool),
    SetMeshSmooth {
        smooth: acadrust::entities::polygon_mesh::SurfaceSmoothType,
        m_density: i16,
        n_density: i16,
    },
    MoveMeshVertex { index: usize, point: DVec3 },
}

// ── Apply logic (pure entity edits; driver handles convert/break/marker) ──

pub fn apply_pedit(entity: &mut EntityType, op: &PeditOp) -> bool {
    match op {
        PeditOp::SetClosed(closed) => match entity {
            EntityType::LwPolyline(p) => {
                p.is_closed = *closed;
                true
            }
            EntityType::Polyline2D(p) => {
                if *closed {
                    p.close();
                } else {
                    p.flags.set_closed(false);
                }
                true
            }
            _ => false,
        },
        PeditOp::SetWidth(w) => match entity {
            EntityType::LwPolyline(p) => {
                p.constant_width = *w;
                for v in &mut p.vertices {
                    v.start_width = *w;
                    v.end_width = *w;
                }
                true
            }
            _ => false,
        },
        PeditOp::Fit => match entity {
            EntityType::LwPolyline(p) => fit_curve(p),
            _ => false,
        },
        PeditOp::Spline => match entity {
            EntityType::LwPolyline(p) => spline_smooth(p),
            _ => false,
        },
        PeditOp::Decurve => match entity {
            EntityType::LwPolyline(p) => {
                for v in &mut p.vertices {
                    v.bulge = 0.0;
                }
                true
            }
            _ => false,
        },
        PeditOp::SetMeshClosedM(closed) => match entity {
            EntityType::PolygonMesh(mesh) => {
                if mesh.is_closed_m() == *closed {
                    return false;
                }
                mesh.flags.set(
                    acadrust::entities::polygon_mesh::PolygonMeshFlags::CLOSED_M,
                    *closed,
                );
                true
            }
            _ => false,
        },
        PeditOp::SetMeshClosedN(closed) => match entity {
            EntityType::PolygonMesh(mesh) => {
                if mesh.is_closed_n() == *closed {
                    return false;
                }
                mesh.flags.set(
                    acadrust::entities::polygon_mesh::PolygonMeshFlags::CLOSED_N,
                    *closed,
                );
                true
            }
            _ => false,
        },
        PeditOp::SetMeshSmooth {
            smooth,
            m_density,
            n_density,
        } => match entity {
            EntityType::PolygonMesh(mesh) => {
                let mut changed = mesh.smooth_type != *smooth;
                mesh.smooth_type = *smooth;
                if mesh.smooth_type
                    != acadrust::entities::polygon_mesh::SurfaceSmoothType::NoSmooth
                {
                    let m_density = (*m_density).clamp(2, 200);
                    let n_density = (*n_density).clamp(2, 200);
                    if mesh.m_smooth_density != m_density {
                        mesh.m_smooth_density = m_density;
                        changed = true;
                    }
                    if mesh.n_smooth_density != n_density {
                        mesh.n_smooth_density = n_density;
                        changed = true;
                    }
                }
                changed
            }
            _ => false,
        },
        PeditOp::MoveMeshVertex { index, point } => match entity {
            EntityType::PolygonMesh(mesh) => {
                if !point.is_finite() {
                    return false;
                }
                let Some(vertex) = mesh.vertices.get_mut(*index) else {
                    return false;
                };
                if vertex.location.x == point.x
                    && vertex.location.y == point.y
                    && vertex.location.z == point.z
                {
                    return false;
                }
                vertex.location = acadrust::types::Vector3::new(point.x, point.y, point.z);
                true
            }
            _ => false,
        },
        // Handled by the driver (it replaces the entity, not edits in place).
        PeditOp::ConvertToPolyline => false,
    }
}

/// A Line or Arc as an equivalent 2-vertex LwPolyline (common carried over,
/// handle NULL for the replace flow). `None` for anything else.
pub fn convert_to_polyline(entity: &EntityType) -> Option<EntityType> {
    let mut pl = acadrust::LwPolyline::new();
    match entity {
        EntityType::Line(l) => {
            pl.common = l.common.clone();
            pl.thickness = l.thickness;
            pl.vertices = vec![
                LwVertex::new(Vector2::new(l.start.x, l.start.y)),
                LwVertex::new(Vector2::new(l.end.x, l.end.y)),
            ];
        }
        EntityType::Arc(a) => {
            pl.common = a.common.clone();
            pl.thickness = a.thickness;
            let (sa, ea) = (a.start_angle, a.end_angle);
            let sweep = {
                let s = (ea - sa).rem_euclid(TAU);
                if s.abs() < 1e-12 {
                    TAU
                } else {
                    s
                }
            };
            let p0 = (
                a.center.x + a.radius * sa.cos(),
                a.center.y + a.radius * sa.sin(),
            );
            let p1 = (
                a.center.x + a.radius * ea.cos(),
                a.center.y + a.radius * ea.sin(),
            );
            let mut v0 = LwVertex::new(Vector2::new(p0.0, p0.1));
            v0.bulge = (sweep / 4.0).tan();
            pl.vertices = vec![v0, LwVertex::new(Vector2::new(p1.0, p1.1))];
        }
        _ => return None,
    }
    pl.common.handle = Handle::NULL;
    Some(EntityType::LwPolyline(pl))
}

#[cfg(test)]
mod convert_tests {
    use super::*;

    // PEDIT convert rebuilds the entity as a polyline; thickness must move
    // onto the new entity instead of resetting to 0 (#916).
    #[test]
    fn convert_keeps_line_thickness() {
        let mut l = acadrust::entities::Line::new();
        l.start = acadrust::types::Vector3::new(0.0, 0.0, 0.0);
        l.end = acadrust::types::Vector3::new(10.0, 0.0, 0.0);
        l.thickness = 3.5;
        let Some(EntityType::LwPolyline(pl)) = convert_to_polyline(&EntityType::Line(l)) else {
            panic!("line must convert");
        };
        assert!(
            (pl.thickness - 3.5).abs() < 1e-12,
            "converted polyline must keep source thickness, got {}",
            pl.thickness
        );
    }

    #[test]
    fn convert_keeps_arc_thickness() {
        let mut a = acadrust::entities::Arc::new();
        a.center = acadrust::types::Vector3::new(0.0, 0.0, 0.0);
        a.radius = 5.0;
        a.start_angle = 0.0;
        a.end_angle = std::f64::consts::FRAC_PI_2;
        a.thickness = -2.0;
        let Some(EntityType::LwPolyline(pl)) = convert_to_polyline(&EntityType::Arc(a)) else {
            panic!("arc must convert");
        };
        assert!(
            (pl.thickness - (-2.0)).abs() < 1e-12,
            "converted polyline must keep negative thickness, got {}",
            pl.thickness
        );
    }
}

// ── Curve fitting ─────────────────────────────────────────────────────────

fn vert_xy(v: &LwVertex) -> DVec2 {
    DVec2::new(v.location.x, v.location.y)
}

/// Wrap an angle into (-pi, pi].
fn wrap_angle(mut a: f64) -> f64 {
    while a > std::f64::consts::PI {
        a -= TAU;
    }
    while a <= -std::f64::consts::PI {
        a += TAU;
    }
    a
}

/// Bulge of the arc `a` -> `b` whose tangent AT `a` is `t` (entry form).
fn bulge_entry(a: DVec2, t: DVec2, b: DVec2) -> f64 {
    let d = b - a;
    if d.length_squared() < 1e-12 {
        return 0.0;
    }
    (wrap_angle(d.y.atan2(d.x) - t.y.atan2(t.x)) / 2.0).tan()
}

/// Bulge of the arc `a` -> `b` whose tangent AT `b` is `t` (exit form).
fn bulge_exit(a: DVec2, b: DVec2, t: DVec2) -> f64 {
    let d = b - a;
    if d.length_squared() < 1e-12 {
        return 0.0;
    }
    (wrap_angle(t.y.atan2(t.x) - d.y.atan2(d.x)) / 2.0).tan()
}

/// PEDIT Fit: replace every segment with a BIARC — two arcs that leave the
/// start vertex along its tangent, meet each other tangentially at an
/// inserted knee vertex, and arrive at the end vertex along ITS tangent.
/// Both segments at a vertex share that vertex's tangent (the average of the
/// neighbouring chords), so the whole run is tangent-continuous — arcs
/// mutually tangent everywhere, which a single arc per segment cannot do.
fn fit_curve(p: &mut acadrust::LwPolyline) -> bool {
    let n = p.vertices.len();
    if n < 3 {
        return false;
    }
    let pts: Vec<DVec2> = p.vertices.iter().map(vert_xy).collect();
    let chord = |i: usize| -> DVec2 {
        let a = pts[i % n];
        let b = pts[(i + 1) % n];
        (b - a).normalize_or_zero()
    };
    // Per-vertex tangents.
    let tangent = |i: usize| -> DVec2 {
        if !p.is_closed && i == 0 {
            return chord(0);
        }
        if !p.is_closed && i == n - 1 {
            return chord(n - 2);
        }
        let prev = chord((i + n - 1) % n);
        let cur = chord(i % n);
        let sum = prev + cur;
        if sum.length_squared() < 1e-12 {
            cur
        } else {
            sum.normalize()
        }
    };
    let seg_count = if p.is_closed { n } else { n - 1 };
    let mut out: Vec<LwVertex> = Vec::with_capacity(seg_count * 2 + 1);
    for i in 0..seg_count {
        let p0 = pts[i % n];
        let p1 = pts[(i + 1) % n];
        let t0 = tangent(i % n);
        let t1 = tangent((i + 1) % n);
        let d = p1 - p0;
        let src = &p.vertices[i % n];
        let mut push = |q: DVec2, bulge: f64| {
            let mut v = LwVertex::new(Vector2::new(q.x, q.y));
            v.bulge = bulge;
            v.start_width = src.start_width;
            v.end_width = src.end_width;
            out.push(v);
        };
        if d.length_squared() < 1e-12 {
            push(p0, 0.0);
            continue;
        }
        // Both tangents already aligned with the chord: keep it straight.
        let dn = d.normalize();
        if (dn - t0).length_squared() < 1e-12 && (dn - t1).length_squared() < 1e-12 {
            push(p0, 0.0);
            continue;
        }
        // Classic equal-parameter biarc: apexes A = p0 + k*t0, B = p1 - k*t1
        // and the knee M at their midpoint. Tangency at M needs |AB| = 2k,
        // i.e. 2(1 - t0.t1) k^2 + 2 (d.(t0+t1)) k - |d|^2 = 0 — A and B are
        // then the tangent-line apexes of their arcs, so both arcs' tangents
        // at M run along AB and the pair is tangent-continuous.
        let dot_tt = t0.dot(t1).clamp(-1.0, 1.0);
        let b_lin = d.dot(t0 + t1);
        let a_quad = 2.0 * (1.0 - dot_tt);
        let k = if a_quad.abs() > 1e-9 {
            let disc = b_lin * b_lin + a_quad * d.length_squared();
            if disc < 0.0 {
                push(p0, bulge_entry(p0, t0, p1));
                continue;
            }
            (-b_lin + disc.sqrt()) / a_quad
        } else if b_lin.abs() > 1e-9 {
            // Parallel tangents: the quadratic degenerates to one root.
            d.length_squared() / (2.0 * b_lin)
        } else {
            // Anti-parallel S with no along-tangent reach: symmetric split.
            (d.length_squared() / 4.0).sqrt()
        };
        if !k.is_finite() || k <= 1e-9 {
            push(p0, bulge_entry(p0, t0, p1));
            continue;
        }
        let m = (p0 + t0 * k + p1 - t1 * k) * 0.5;
        push(p0, bulge_entry(p0, t0, m));
        push(m, bulge_exit(m, p1, t1));
    }
    if !p.is_closed {
        let mut last = p.vertices[n - 1].clone();
        last.bulge = 0.0;
        out.push(last);
    }
    if out.len() < 2 {
        return false;
    }
    p.vertices = out;
    true
}

/// PEDIT Spline: replace the shape with a sampled uniform cubic B-spline of
/// the vertex frame (8 samples per span; a closed frame wraps around).
/// Replace the polyline's vertices with a sampling of the cubic B-spline its
/// vertices control.
///
/// Evaluated by the kernel rather than from a hand-written basis. The four
/// blending polynomials that were here are the uniform cubic ones spelled
/// out, with the open case clamped by repeating end points and then the two
/// ends pinned back afterwards to undo the drift that leaves — all of which
/// a clamped knot vector expresses directly and exactly.
fn spline_smooth(p: &mut acadrust::LwPolyline) -> bool {
    const DEGREE: usize = 3;
    const PER_SPAN: usize = 8;

    let control: Vec<[f64; 2]> = p.vertices.iter().map(|v| [v.location.x, v.location.y]).collect();
    if control.len() < 3 {
        return false;
    }
    let curve = if p.is_closed {
        // A periodic curve is written by wrapping the first `degree` control
        // points onto the end, over a uniform knot vector — which is what
        // makes the seam as smooth as everywhere else.
        let mut wrapped = control.clone();
        wrapped.extend(control.iter().take(DEGREE).copied());
        let count = wrapped.len();
        let knots: Vec<f64> = (0..count + DEGREE + 1).map(|i| i as f64).collect();
        NurbsCurve::new(DEGREE, wrapped, knots, None)
    } else {
        // Clamped: the curve starts and finishes on its outer control points
        // without them needing to be repeated.
        let knots = clamped_uniform_knots(DEGREE, control.len());
        NurbsCurve::new(DEGREE, control.clone(), knots, None)
    };
    let Some(curve) = curve else {
        return false;
    };

    let steps = PER_SPAN * control.len();
    let sampled: Vec<[f64; 2]> = (0..=steps)
        .map(|step| curve.point_at(step as f64 / steps as f64))
        .collect();
    if sampled.len() < 2 {
        return false;
    }

    let width = p.constant_width;
    p.vertices = sampled
        .into_iter()
        .map(|q| {
            let mut v = LwVertex::new(Vector2::new(q[0], q[1]));
            v.start_width = width;
            v.end_width = width;
            v
        })
        .collect();
    true
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["PEDIT"] });  // PeditCommand
