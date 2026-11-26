//! The Geometry Cleanup settings: the error types to find and a tolerance
//! for each. They are kept in the user profile and read and written as
//! `.gcs` files:
//!
//! ```json
//! { "tolerances": [ { "name": "gap", "subName": "", "tolerance": 0.4 } ] }
//! ```
//!
//! A type that is not in the list is not scanned. The angle types
//! (`absolute-angle`, `relative-angle`) have a row for each standard angle,
//! with the angle in degrees in `subName`.

use ocs_geom::geom2d::cleanup::{
    AngleErrorKind, AngleTolerance, AngleTolerances, EndErrorKind, Tolerances,
};
use serde::{Deserialize, Serialize};

/// The smallest tolerance. A smaller value becomes this one.
pub const MIN_TOLERANCE: f64 = 0.0001;

/// The `.gcs` names of the angle types.
const ABSOLUTE_ANGLE: &str = "absolute-angle";
const RELATIVE_ANGLE: &str = "relative-angle";

/// The angle rows of a new profile: 0° and 90° for each angle
/// type, with a tolerance of 5°.
const DEFAULT_ANGLES: [(&str, &str); 4] = [
    (ABSOLUTE_ANGLE, "0"),
    (ABSOLUTE_ANGLE, "90"),
    (RELATIVE_ANGLE, "0"),
    (RELATIVE_ANGLE, "90"),
];
pub const DEFAULT_ANGLE_TOLERANCE: f64 = 5.0;

/// The length types, in the order of their rows.
const LENGTH_KINDS: [EndErrorKind; 3] = [
    EndErrorKind::Gap,
    EndErrorKind::Overshoot,
    EndErrorKind::Undershoot,
];

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CleanupSettings {
    pub tolerances: Vec<ToleranceRow>,
    /// The fields that OCS does not know, kept so that an export writes
    /// them back.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// One error type of a `.gcs` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToleranceRow {
    pub name: String,
    /// The angle in degrees of an angle type; empty for the others.
    #[serde(rename = "subName", default)]
    pub sub_name: String,
    pub tolerance: f64,
    /// The fields of the row that OCS does not know, kept as they are.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The name of a length type in a `.gcs` file.
pub fn kind_name(kind: EndErrorKind) -> &'static str {
    match kind {
        EndErrorKind::Gap => "gap",
        EndErrorKind::Overshoot => "overshoot",
        EndErrorKind::Undershoot => "undershoot",
    }
}

/// The name of an angle type in a `.gcs` file.
pub fn angle_kind_name(kind: AngleErrorKind) -> &'static str {
    match kind {
        AngleErrorKind::Absolute => ABSOLUTE_ANGLE,
        AngleErrorKind::Relative => RELATIVE_ANGLE,
    }
}

/// The angle type of a `.gcs` name; `None` for the other types.
pub fn angle_kind_of(name: &str) -> Option<AngleErrorKind> {
    [AngleErrorKind::Absolute, AngleErrorKind::Relative]
        .into_iter()
        .find(|&kind| angle_kind_name(kind) == name)
}

/// The length type of a `.gcs` name; `None` for the other types.
pub fn kind_of(name: &str) -> Option<EndErrorKind> {
    LENGTH_KINDS
        .into_iter()
        .find(|&kind| kind_name(kind) == name)
}

/// `tolerance` with at most 6 decimals, and not less than the minimum.
fn cleaned(tolerance: f64) -> f64 {
    ((tolerance * 1e6).round() / 1e6).max(MIN_TOLERANCE)
}

/// `tolerance` as the settings keep it: at most 6 decimals and not less
/// than the minimum. `None` when it cannot be kept: not a number, or too
/// large (the file would hold `null`).
pub fn clean_tolerance(tolerance: f64) -> Option<f64> {
    Some(cleaned(tolerance)).filter(|t| t.is_finite())
}

impl CleanupSettings {
    /// The three length types, each with `tolerance`, and the default
    /// angle rows.
    pub fn proposed(tolerance: f64) -> Self {
        let mut settings = Self {
            tolerances: DEFAULT_ANGLES
                .iter()
                .map(|&(name, angle)| ToleranceRow {
                    name: name.to_string(),
                    sub_name: angle.to_string(),
                    tolerance: DEFAULT_ANGLE_TOLERANCE,
                    extra: Default::default(),
                })
                .collect(),
            extra: Default::default(),
        };
        for kind in LENGTH_KINDS {
            settings.set(kind, Some(tolerance));
        }
        settings
    }

    /// Read a `.gcs` file; a byte order mark before it is skipped. Text
    /// that is not such a file, or a tolerance that is negative or not a
    /// number, is an error.
    pub fn parse_gcs(text: &str) -> Result<Self, String> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut settings: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        for row in &mut settings.tolerances {
            let bad = || {
                crate::tf!(
                    "The tolerance of {} is {}: it must be a number of 0 or more.",
                    row.name,
                    row.tolerance
                )
                .into_owned()
            };
            if !row.tolerance.is_finite() || row.tolerance < 0.0 {
                return Err(bad());
            }
            if kind_of(&row.name).is_some() {
                row.tolerance = clean_tolerance(row.tolerance).ok_or_else(bad)?;
            }
        }
        Ok(settings)
    }

    /// The settings as the text of a `.gcs` file.
    pub fn to_gcs(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// The tolerance of a length type, or `None` when it is not scanned.
    pub fn tolerance(&self, kind: EndErrorKind) -> Option<f64> {
        self.tolerances
            .iter()
            .find(|row| row.name == kind_name(kind))
            .map(|row| row.tolerance)
    }

    /// Set the tolerance of a length type, or remove the type (`None`).
    /// The length types stay before the other rows, in their order.
    /// A tolerance that cannot be kept (see [`clean_tolerance`]) changes
    /// nothing.
    pub fn set(&mut self, kind: EndErrorKind, tolerance: Option<f64>) {
        let tolerance = match tolerance.map(clean_tolerance) {
            Some(None) => return,
            cleaned => cleaned.flatten(),
        };
        // A changed row keeps the fields that OCS does not know.
        let extra = self
            .tolerances
            .iter()
            .find(|row| row.name == kind_name(kind))
            .map(|row| row.extra.clone())
            .unwrap_or_default();
        self.tolerances.retain(|row| row.name != kind_name(kind));
        let Some(tolerance) = tolerance else {
            return;
        };
        let at = self
            .tolerances
            .iter()
            .position(|row| kind_of(&row.name).is_none_or(|other| other > kind))
            .unwrap_or(self.tolerances.len());
        self.tolerances.insert(
            at,
            ToleranceRow {
                name: kind_name(kind).to_string(),
                sub_name: String::new(),
                tolerance,
                extra,
            },
        );
    }

    /// The tolerances that the scan uses.
    pub fn tolerances(&self) -> Tolerances {
        Tolerances {
            gap: self.tolerance(EndErrorKind::Gap),
            overshoot: self.tolerance(EndErrorKind::Overshoot),
            undershoot: self.tolerance(EndErrorKind::Undershoot),
        }
    }

    /// The standard angles that the scan uses: each angle modulo 180°, a
    /// tolerance not less than the minimum. A row whose angle is not a
    /// number is left out, and so is a second row with the same angle.
    pub fn angle_tolerances(&self) -> AngleTolerances {
        let read = |name: &str| {
            let mut standards: Vec<AngleTolerance> = Vec::new();
            for row in self.tolerances.iter().filter(|row| row.name == name) {
                let Ok(angle) = row.sub_name.trim().parse::<f64>() else {
                    continue;
                };
                let angle = angle.rem_euclid(180.0);
                if !angle.is_finite() || standards.iter().any(|s| s.angle == angle) {
                    continue;
                }
                standards.push(AngleTolerance {
                    angle,
                    tolerance: row.tolerance.max(MIN_TOLERANCE),
                });
            }
            standards
        };
        AngleTolerances {
            absolute: read(ABSOLUTE_ANGLE),
            relative: read(RELATIVE_ANGLE),
        }
    }
}

/// A `.gcs` file with angle rows as well as the length rows.
#[cfg(test)]
pub(crate) const SAMPLE_GCS: &str = r#"{
  "tolerances" :
  [
    {
      "name" : "gap",
      "subName" : "",
      "tolerance" : 0.4
    },
    {
      "name" : "overshoot",
      "subName" : "",
      "tolerance" : 0.4
    },
    {
      "name" : "undershoot",
      "subName" : "",
      "tolerance" : 0.4
    },
    {
      "name" : "absolute-angle",
      "subName" : "0",
      "tolerance" : 5.0
    },
    {
      "name" : "absolute-angle",
      "subName" : "90",
      "tolerance" : 5.0
    },
    {
      "name" : "relative-angle",
      "subName" : "0",
      "tolerance" : 5.0
    },
    {
      "name" : "relative-angle",
      "subName" : "90",
      "tolerance" : 5.0
    }
  ]
}"#;

#[cfg(test)]
mod tests;
