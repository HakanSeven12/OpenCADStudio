//! AUTOCONSTRAIN's settings: which relations it infers, in which order,
//! and the tolerances it infers them within. Kept with the user's profile.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AutoConstraintKind {
    Coincident,
    Collinear,
    Parallel,
    Perpendicular,
    Tangent,
    Concentric,
    Horizontal,
    Vertical,
    Equal,
}

impl AutoConstraintKind {
    pub const ALL: [Self; 9] = [
        Self::Coincident,
        Self::Collinear,
        Self::Parallel,
        Self::Perpendicular,
        Self::Tangent,
        Self::Concentric,
        Self::Horizontal,
        Self::Vertical,
        Self::Equal,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Coincident => "Coincident",
            Self::Collinear => "Collinear",
            Self::Parallel => "Parallel",
            Self::Perpendicular => "Perpendicular",
            Self::Tangent => "Tangent",
            Self::Concentric => "Concentric",
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
            Self::Equal => "Equal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoConstrainSettings {
    pub priority: Vec<AutoConstraintKind>,
    pub enabled: Vec<AutoConstraintKind>,
    pub tangent_must_share_point: bool,
    pub perpendicular_must_intersect: bool,
    pub distance_tolerance: f64,
    pub angle_tolerance_deg: f64,
}

impl Default for AutoConstrainSettings {
    fn default() -> Self {
        Self {
            priority: AutoConstraintKind::ALL.to_vec(),
            // Keep Equal available without creating redundant relations
            // between equal-length segments by default.
            enabled: AutoConstraintKind::ALL
                .into_iter()
                .filter(|kind| *kind != AutoConstraintKind::Equal)
                .collect(),
            tangent_must_share_point: true,
            perpendicular_must_intersect: true,
            distance_tolerance: 0.05,
            angle_tolerance_deg: 1.0,
        }
    }
}

impl AutoConstrainSettings {
    pub fn sanitize(&mut self) {
        let mut priority = Vec::with_capacity(AutoConstraintKind::ALL.len());
        for kind in self.priority.iter().copied().chain(AutoConstraintKind::ALL) {
            if !priority.contains(&kind) {
                priority.push(kind);
            }
        }
        self.priority = priority;
        self.enabled
            .retain(|kind| AutoConstraintKind::ALL.contains(kind));
        self.enabled.sort_by_key(|kind| {
            self.priority
                .iter()
                .position(|candidate| candidate == kind)
                .unwrap_or(usize::MAX)
        });
        self.enabled.dedup();
        if !self.distance_tolerance.is_finite() || self.distance_tolerance < 0.0 {
            self.distance_tolerance = 0.05;
        }
        if !self.angle_tolerance_deg.is_finite() || self.angle_tolerance_deg < 0.0 {
            self.angle_tolerance_deg = 1.0;
        }
    }
}
