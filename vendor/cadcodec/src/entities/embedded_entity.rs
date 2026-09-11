//! Typed entities embedded inside sweep, loft, extrusion, and revolve data.
//!
//! These values are not database-resident entities: their DWG payload contains
//! only the type-specific entity body, without common entity data or handles.

use super::{Arc, Circle, Ellipse, Line, LwPolyline, Point, Ray, Region, Spline, XLine};

/// Curve or point geometry embedded in a 3D construction-history record.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EmbeddedEntity {
    Point(Point),
    Line(Line),
    Arc(Arc),
    Circle(Circle),
    Ellipse(Ellipse),
    Spline(Spline),
    LwPolyline(LwPolyline),
    /// Planar modeler profile, including its inner boundary loops.
    Region(Region),
    Ray(Ray),
    XLine(XLine),
    /// Unsupported entity body preserved losslessly for round-trip output.
    Unknown {
        type_code: i32,
        bit_count: usize,
        bytes: Vec<u8>,
    },
}
