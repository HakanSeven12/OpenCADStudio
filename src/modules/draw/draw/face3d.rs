use acadrust::entities::{face3d::InvisibleEdgeFlags, Face3D};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use cadkernel::geom2d::Tolerance;
use cadkernel::space::{curve::segments_overlap_collinearly, Vec3 as KernelVec3};
use glam::DVec3;
use std::collections::{HashMap, HashSet};

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::scene::model::wire_model::WireModel;
use crate::t;

fn vector(point: DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}

fn point(value: Vector3) -> DVec3 {
    DVec3::new(value.x, value.y, value.z)
}

fn set_edge_invisible(flags: &mut InvisibleEdgeFlags, edge: usize, invisible: bool) {
    match edge {
        0 => flags.set_first_invisible(invisible),
        1 => flags.set_second_invisible(invisible),
        2 => flags.set_third_invisible(invisible),
        3 => flags.set_fourth_invisible(invisible),
        _ => {}
    }
}

fn edge_is_invisible(flags: InvisibleEdgeFlags, edge: usize) -> bool {
    match edge {
        0 => flags.is_first_invisible(),
        1 => flags.is_second_invisible(),
        2 => flags.is_third_invisible(),
        3 => flags.is_fourth_invisible(),
        _ => false,
    }
}

fn append_segment(points: &mut Vec<[f64; 3]>, start: [f64; 3], end: [f64; 3]) {
    if !points.is_empty() {
        points.push([f64::NAN; 3]);
    }
    points.push(start);
    points.push(end);
}

pub struct Face3dCommand {
    points: Vec<DVec3>,
    invisible: [bool; 4],
    pending_invisible: bool,
}

impl Face3dCommand {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            invisible: [false; 4],
            pending_invisible: false,
        }
    }

    fn build(&self, fourth: DVec3) -> Option<EntityType> {
        if self.points.len() != 3 {
            return None;
        }
        let mut face = Face3D::new(
            vector(self.points[0]),
            vector(self.points[1]),
            vector(self.points[2]),
            vector(fourth),
        );
        for edge in 0..4 {
            set_edge_invisible(&mut face.invisible_edges, edge, self.invisible[edge]);
        }
        Some(EntityType::Face3D(face))
    }

    fn continue_from_last_edge(&mut self, fourth: DVec3) {
        let third = self.points[2];
        let shared_edge_invisible = self.invisible[2];
        self.points.clear();
        self.points.push(third);
        self.points.push(fourth);
        self.invisible = [shared_edge_invisible, false, false, false];
        self.pending_invisible = false;
    }

    fn continue_from_triangle(&mut self) {
        let second = self.points[1];
        let third = self.points[2];
        let shared_edge_invisible = self.invisible[1];
        self.points.clear();
        self.points.push(second);
        self.points.push(third);
        self.invisible = [shared_edge_invisible, false, false, false];
        self.pending_invisible = false;
    }
}

impl CadCommand for Face3dCommand {
    fn name(&self) -> &'static str {
        "3DFACE"
    }

    fn prompt(&self) -> String {
        match self.points.len() {
            0 => t!("3DFACE  Specify first point or [Invisible]:").into_owned(),
            1 => t!("3DFACE  Specify second point or [Invisible]:").into_owned(),
            2 => t!("3DFACE  Specify third point or [Invisible] <exit>:").into_owned(),
            _ => t!("3DFACE  Specify fourth point or [Invisible] <create three-sided face>:")
                .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        vec![CmdOption::new(t!("Invisible").as_ref(), "I")]
    }

    fn on_point(&mut self, picked: DVec3) -> CmdResult {
        let point_index = self.points.len();
        if self.pending_invisible && point_index < 4 {
            self.invisible[point_index] = true;
        }
        self.pending_invisible = false;
        if self.points.len() == 3 {
            let entity = self.build(picked);
            self.continue_from_last_edge(picked);
            return entity.map_or(CmdResult::Cancel, CmdResult::CommitEntity);
        }
        self.points.push(picked);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.points.len() != 3 {
            return CmdResult::Cancel;
        }
        if self.pending_invisible {
            self.invisible[3] = true;
        }
        let third = self.points[2];
        let entity = self.build(third);
        self.continue_from_triangle();
        entity.map_or(CmdResult::Cancel, CmdResult::CommitEntity)
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, value: &str) -> Option<CmdResult> {
        match value.trim().to_ascii_uppercase().as_str() {
            "I" | "INVISIBLE" => {
                self.pending_invisible = true;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_mouse_move(&mut self, cursor: DVec3) -> Option<WireModel> {
        if self.points.is_empty() {
            return None;
        }
        let mut preview = Vec::new();
        for edge in 0..self.points.len().saturating_sub(1) {
            if !self.invisible[edge] {
                append_segment(
                    &mut preview,
                    self.points[edge].to_array(),
                    self.points[edge + 1].to_array(),
                );
            }
        }
        let pending_edge = self.points.len() - 1;
        if !self.invisible[pending_edge] {
            append_segment(
                &mut preview,
                self.points.last()?.to_array(),
                cursor.to_array(),
            );
        }
        if self.points.len() == 3 && !(self.invisible[3] || self.pending_invisible) {
            append_segment(&mut preview, cursor.to_array(), self.points[0].to_array());
        }
        Some(WireModel::solid_f64(
            "face3d_preview".to_string(),
            preview,
            WireModel::CYAN,
            false,
        ))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeMode {
    Toggle,
    Display,
    DisplaySelect,
}

pub struct FaceEdgeCommand {
    faces: Vec<(Handle, Face3D)>,
    mode: EdgeMode,
    show_all_hidden: bool,
    displayed_faces: HashSet<Handle>,
    pending_replacements: HashMap<Handle, Face3D>,
}

impl FaceEdgeCommand {
    pub fn new(faces: Vec<(Handle, Face3D)>) -> Self {
        Self {
            faces,
            mode: EdgeMode::Toggle,
            show_all_hidden: false,
            displayed_faces: HashSet::new(),
            pending_replacements: HashMap::new(),
        }
    }

    fn corners(face: &Face3D) -> [KernelVec3; 4] {
        [
            KernelVec3::from(point(face.first_corner).to_array()),
            KernelVec3::from(point(face.second_corner).to_array()),
            KernelVec3::from(point(face.third_corner).to_array()),
            KernelVec3::from(point(face.fourth_corner).to_array()),
        ]
    }

    fn nearest_edge(face: &Face3D, picked: KernelVec3) -> Option<usize> {
        let corners = Self::corners(face);
        let tolerance = Tolerance::default().linear();
        let mut best = None;
        for edge in 0..4 {
            let start = corners[edge];
            let end = corners[(edge + 1) % 4];
            if start.distance(end) <= tolerance {
                continue;
            }
            let distance = picked.distance_to_segment(start, end);
            if best.is_none_or(|(_, best_distance)| distance < best_distance) {
                best = Some((edge, distance));
            }
        }
        best.map(|(edge, _)| edge)
    }

    fn edge_wire(face: &Face3D, edge: usize, name: &str) -> WireModel {
        let corners = Self::corners(face);
        WireModel::solid_f64(
            name.to_string(),
            vec![corners[edge].to_array(), corners[(edge + 1) % 4].to_array()],
            WireModel::CYAN,
            false,
        )
    }

    fn collinear_replacements(
        &mut self,
        selected_face: usize,
        selected_edge: usize,
    ) -> Vec<(Handle, Vec<EntityType>)> {
        let selected_corners = Self::corners(&self.faces[selected_face].1);
        let selected_start = selected_corners[selected_edge];
        let selected_end = selected_corners[(selected_edge + 1) % 4];
        let make_invisible = !edge_is_invisible(
            self.faces[selected_face].1.invisible_edges,
            selected_edge,
        );
        let mut replacements = Vec::new();
        self.pending_replacements.clear();
        for (handle, face) in &self.faces {
            let corners = Self::corners(face);
            let mut updated = face.clone();
            let mut changed = false;
            for edge in 0..4 {
                if segments_overlap_collinearly(
                    selected_start.to_array(),
                    selected_end.to_array(),
                    corners[edge].to_array(),
                    corners[(edge + 1) % 4].to_array(),
                    Tolerance::default().linear(),
                ) && edge_is_invisible(updated.invisible_edges, edge) != make_invisible
                {
                    set_edge_invisible(&mut updated.invisible_edges, edge, make_invisible);
                    changed = true;
                }
            }
            if changed {
                self.pending_replacements.insert(*handle, updated.clone());
                replacements.push((*handle, vec![EntityType::Face3D(updated)]));
            }
        }
        replacements
    }

    fn hidden_edges_wire(&self) -> WireModel {
        let mut points = Vec::new();
        for (handle, face) in &self.faces {
            if !self.show_all_hidden && !self.displayed_faces.contains(handle) {
                continue;
            }
            let corners = Self::corners(face);
            for edge in 0..4 {
                if edge_is_invisible(face.invisible_edges, edge)
                    && corners[edge].distance(corners[(edge + 1) % 4])
                        > Tolerance::default().linear()
                {
                    append_segment(
                        &mut points,
                        corners[edge].to_array(),
                        corners[(edge + 1) % 4].to_array(),
                    );
                }
            }
        }
        WireModel::solid_f64(
            "face3d_hidden_edges".to_string(),
            points,
            WireModel::CYAN,
            false,
        )
    }
}

impl CadCommand for FaceEdgeCommand {
    fn name(&self) -> &'static str {
        "EDGE"
    }

    fn prompt(&self) -> String {
        match self.mode {
            EdgeMode::Toggle => {
                t!("EDGE  Select edge of 3D face to toggle visibility or [Display]:").into_owned()
            }
            EdgeMode::Display => {
                t!("EDGE  Display invisible edges [All/Select] <Select>:").into_owned()
            }
            EdgeMode::DisplaySelect => {
                t!("EDGE  Select 3D faces to display invisible edges <done>:").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.mode {
            EdgeMode::Toggle => vec![CmdOption::new(t!("Display").as_ref(), "D")],
            EdgeMode::Display => vec![
                CmdOption::new(t!("All").as_ref(), "A"),
                CmdOption::new(t!("Select").as_ref(), "S"),
            ],
            EdgeMode::DisplaySelect => Vec::new(),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.mode, EdgeMode::Toggle | EdgeMode::DisplaySelect)
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

    fn on_entity_pick(&mut self, handle: Handle, picked: DVec3) -> CmdResult {
        let Some(index) = self.faces.iter().position(|(candidate, _)| *candidate == handle) else {
            return CmdResult::NeedPoint;
        };
        if self.mode == EdgeMode::DisplaySelect {
            self.displayed_faces.insert(handle);
            return CmdResult::Preview(self.hidden_edges_wire());
        }
        let picked = KernelVec3::from(picked.to_array());
        let Some(edge) = Self::nearest_edge(&self.faces[index].1, picked) else {
            return CmdResult::NeedPoint;
        };
        let replacements = self.collinear_replacements(index, edge);
        if replacements.is_empty() {
            CmdResult::NeedPoint
        } else {
            CmdResult::ReplaceManyContinue(replacements)
        }
    }

    fn on_hover_entity(&mut self, handle: Handle, picked: DVec3) -> Vec<WireModel> {
        let Some((_, face)) = self.faces.iter().find(|(candidate, _)| *candidate == handle) else {
            return Vec::new();
        };
        if self.mode == EdgeMode::DisplaySelect
            || self.show_all_hidden
            || self.displayed_faces.contains(&handle)
        {
            let mut points = Vec::new();
            let corners = Self::corners(face);
            for edge in 0..4 {
                if corners[edge].distance(corners[(edge + 1) % 4])
                    > Tolerance::default().linear()
                {
                    append_segment(
                        &mut points,
                        corners[edge].to_array(),
                        corners[(edge + 1) % 4].to_array(),
                    );
                }
            }
            return vec![WireModel::solid_f64(
                "face3d_edges".to_string(),
                points,
                WireModel::CYAN,
                false,
            )];
        }
        Self::nearest_edge(face, KernelVec3::from(picked.to_array()))
            .map(|edge| vec![Self::edge_wire(face, edge, "face3d_edge")])
            .unwrap_or_default()
    }

    fn on_text_input(&mut self, value: &str) -> Option<CmdResult> {
        let keyword = value.trim().to_ascii_uppercase();
        match self.mode {
            EdgeMode::Toggle if matches!(keyword.as_str(), "D" | "DISPLAY") => {
                self.mode = EdgeMode::Display;
                Some(CmdResult::NeedPoint)
            }
            EdgeMode::Display if matches!(keyword.as_str(), "A" | "ALL") => {
                self.mode = EdgeMode::Toggle;
                self.show_all_hidden = true;
                Some(CmdResult::Preview(self.hidden_edges_wire()))
            }
            EdgeMode::Display if matches!(keyword.as_str(), "S" | "SELECT") => {
                self.mode = EdgeMode::DisplaySelect;
                self.show_all_hidden = false;
                self.displayed_faces.clear();
                Some(CmdResult::NeedPoint)
            }
            _ => Some(CmdResult::NeedPoint),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.mode {
            EdgeMode::Display => {
                self.mode = EdgeMode::DisplaySelect;
                self.show_all_hidden = false;
                self.displayed_faces.clear();
                CmdResult::NeedPoint
            }
            EdgeMode::DisplaySelect => {
                self.mode = EdgeMode::Toggle;
                CmdResult::Preview(self.hidden_edges_wire())
            }
            EdgeMode::Toggle => CmdResult::Cancel,
        }
    }

    fn on_entity_replaced(&mut self, old: Handle, new_handles: &[Handle]) {
        if let Some(new_handle) = new_handles.first().copied() {
            if let Some((handle, face)) =
                self.faces.iter_mut().find(|(handle, _)| *handle == old)
            {
                if let Some(updated) = self.pending_replacements.remove(&old) {
                    *face = updated;
                }
                *handle = new_handle;
            }
            if self.displayed_faces.remove(&old) {
                self.displayed_faces.insert(new_handle);
            }
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["3DFACE", "EDGE"]
});
