use acadrust::entities::polygon_mesh::{PolygonMesh, PolygonMeshVertex};
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, DynField};
use crate::scene::model::wire_model::WireModel;
use crate::t;

const MIN_SIZE: usize = 2;
const MAX_SIZE: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    MSize,
    NSize,
    Vertices,
}

/// Creates an M by N polygon mesh from row-major 3D input.
pub struct Mesh3dCommand {
    step: Step,
    m: usize,
    n: usize,
    points: Vec<DVec3>,
}

impl Mesh3dCommand {
    pub fn new() -> Self {
        Self {
            step: Step::MSize,
            m: 0,
            n: 0,
            points: Vec::new(),
        }
    }

    fn parse_size(text: &str) -> Option<usize> {
        text.trim()
            .parse::<usize>()
            .ok()
            .filter(|value| (MIN_SIZE..=MAX_SIZE).contains(value))
    }

    fn total_vertices(&self) -> usize {
        self.m.saturating_mul(self.n)
    }

    fn current_indices(&self) -> (usize, usize) {
        let index = self.points.len().min(self.total_vertices().saturating_sub(1));
        (index / self.n.max(1), index % self.n.max(1))
    }

    fn build(&self) -> Option<EntityType> {
        if self.m < MIN_SIZE
            || self.n < MIN_SIZE
            || self.points.len() != self.total_vertices()
        {
            return None;
        }
        let mut mesh = PolygonMesh::new();
        mesh.m_vertex_count = self.m as i16;
        mesh.n_vertex_count = self.n as i16;
        mesh.vertices = self
            .points
            .iter()
            .map(|point| PolygonMeshVertex::at(Vector3::new(point.x, point.y, point.z)))
            .collect();
        Some(EntityType::PolygonMesh(mesh))
    }

    fn preview(&self, cursor: DVec3) -> Option<WireModel> {
        if self.step != Step::Vertices {
            return None;
        }
        let mut points = self.points.clone();
        if points.len() < self.total_vertices() {
            points.push(cursor);
        }
        let mut lines = Vec::new();
        let mut add_segment = |a: DVec3, b: DVec3| {
            if !lines.is_empty() {
                lines.push([f64::NAN; 3]);
            }
            lines.push([a.x, a.y, a.z]);
            lines.push([b.x, b.y, b.z]);
        };
        for index in 0..points.len() {
            let row = index / self.n;
            let column = index % self.n;
            if column > 0 {
                add_segment(points[index - 1], points[index]);
            }
            if row > 0 {
                add_segment(points[index - self.n], points[index]);
            }
        }
        (!lines.is_empty()).then(|| {
            WireModel::solid_f64("mesh3d_preview".to_string(), lines, WireModel::CYAN, false)
        })
    }
}

impl CadCommand for Mesh3dCommand {
    fn name(&self) -> &'static str {
        "3DMESH"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::MSize => t!("3DMESH  Enter size of mesh in M direction (2-256):").into_owned(),
            Step::NSize => t!("3DMESH  Enter size of mesh in N direction (2-256):").into_owned(),
            Step::Vertices => {
                let (m, n) = self.current_indices();
                t!(
                    "3DMESH  Specify location for vertex (%{m}, %{n}):",
                    m = m,
                    n = n
                )
                .into_owned()
            }
        }
    }

    fn wants_text_input(&self) -> bool {
        self.step != Step::Vertices
    }

    fn dyn_field(&self) -> DynField {
        if self.step == Step::Vertices {
            DynField::Point
        } else {
            DynField::Scalar
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.step {
            Step::MSize => {
                if let Some(value) = Self::parse_size(text) {
                    self.m = value;
                    self.step = Step::NSize;
                }
                Some(CmdResult::NeedPoint)
            }
            Step::NSize => {
                if let Some(value) = Self::parse_size(text) {
                    self.n = value;
                    self.points.reserve(self.total_vertices());
                    self.step = Step::Vertices;
                }
                Some(CmdResult::NeedPoint)
            }
            Step::Vertices => None,
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.step != Step::Vertices {
            return CmdResult::NeedPoint;
        }
        if !point.is_finite() {
            return CmdResult::NeedPoint;
        }
        self.points.push(point);
        if self.points.len() == self.total_vertices() {
            self.build()
                .map(CmdResult::CommitAndExit)
                .unwrap_or(CmdResult::Cancel)
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        self.preview(point)
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["3DMESH"] });
