//! Persistent association metadata for centre-line entities.
//!
//! A centre line is rendered and exchanged as an ordinary line while this
//! compact XDATA payload retains the source geometry needed to regenerate it.

use crate::types::{Handle, Vector3};
use crate::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};

/// Registered application name used by centre-line association records.
pub const CENTERLINE_XDATA_APPLICATION: &str = "OCS_CENTERLINE";
/// Registered application name used by centre-mark association records.
pub const CENTERMARK_XDATA_APPLICATION: &str = "OCS_CENTERMARK";
const SIGNATURE: &str = "CENTERLINE_ASSOCIATION";
const MARK_SIGNATURE: &str = "CENTERMARK_ASSOCIATION";
const VERSION: i16 = 1;

/// Kind of source geometry referenced by a centre line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CenterLineSourceKind {
    Line,
    LwPolylineSegment,
    Polyline2DSegment,
}

impl CenterLineSourceKind {
    fn code(self) -> i16 {
        match self {
            Self::Line => 0,
            Self::LwPolylineSegment => 1,
            Self::Polyline2DSegment => 2,
        }
    }

    fn from_code(code: i16) -> Option<Self> {
        match code {
            0 => Some(Self::Line),
            1 => Some(Self::LwPolylineSegment),
            2 => Some(Self::Polyline2DSegment),
            _ => None,
        }
    }
}

/// One selected source line or linear polyline segment.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CenterLineSource {
    pub handle: Handle,
    pub kind: CenterLineSourceKind,
    pub segment_index: i32,
    pub pick_point: Vector3,
}

/// Complete, versioned metadata required to regenerate a centre line.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CenterLineAssociation {
    pub first: CenterLineSource,
    pub second: CenterLineSource,
    pub plane_origin: Vector3,
    pub plane_x: Vector3,
    pub plane_y: Vector3,
    pub start_extension: f64,
    pub end_extension: f64,
    pub start_length_adjustment: f64,
    pub end_length_adjustment: f64,
    pub associated: bool,
}

impl CenterLineAssociation {
    /// Decode association metadata, rejecting incomplete or future payloads.
    pub fn read(data: &ExtendedData) -> Option<Self> {
        let values = &data.get_record(CENTERLINE_XDATA_APPLICATION)?.values;
        let [XDataValue::String(signature), XDataValue::Integer16(version), XDataValue::Handle(first_handle), XDataValue::Integer16(first_kind), XDataValue::Integer32(first_segment), XDataValue::Point3D(first_pick), XDataValue::Handle(second_handle), XDataValue::Integer16(second_kind), XDataValue::Integer32(second_segment), XDataValue::Point3D(second_pick), XDataValue::Point3D(plane_origin), XDataValue::Direction3D(plane_x), XDataValue::Direction3D(plane_y), XDataValue::Distance(start_extension), XDataValue::Distance(end_extension), XDataValue::Distance(start_length_adjustment), XDataValue::Distance(end_length_adjustment), XDataValue::Integer16(flags)] =
            values.as_slice()
        else {
            return None;
        };
        if signature != SIGNATURE || *version != VERSION {
            return None;
        }
        Some(Self {
            first: CenterLineSource {
                handle: *first_handle,
                kind: CenterLineSourceKind::from_code(*first_kind)?,
                segment_index: *first_segment,
                pick_point: *first_pick,
            },
            second: CenterLineSource {
                handle: *second_handle,
                kind: CenterLineSourceKind::from_code(*second_kind)?,
                segment_index: *second_segment,
                pick_point: *second_pick,
            },
            plane_origin: *plane_origin,
            plane_x: *plane_x,
            plane_y: *plane_y,
            start_extension: *start_extension,
            end_extension: *end_extension,
            start_length_adjustment: *start_length_adjustment,
            end_length_adjustment: *end_length_adjustment,
            associated: flags & 1 != 0,
        })
    }

    /// Replace the association payload without disturbing unrelated XDATA.
    pub fn write(&self, data: &mut ExtendedData) {
        let mut record = ExtendedDataRecord::new(CENTERLINE_XDATA_APPLICATION);
        record.values = vec![
            XDataValue::String(SIGNATURE.to_owned()),
            XDataValue::Integer16(VERSION),
            XDataValue::Handle(self.first.handle),
            XDataValue::Integer16(self.first.kind.code()),
            XDataValue::Integer32(self.first.segment_index),
            XDataValue::Point3D(self.first.pick_point),
            XDataValue::Handle(self.second.handle),
            XDataValue::Integer16(self.second.kind.code()),
            XDataValue::Integer32(self.second.segment_index),
            XDataValue::Point3D(self.second.pick_point),
            XDataValue::Point3D(self.plane_origin),
            XDataValue::Direction3D(self.plane_x),
            XDataValue::Direction3D(self.plane_y),
            XDataValue::Distance(self.start_extension),
            XDataValue::Distance(self.end_extension),
            XDataValue::Distance(self.start_length_adjustment),
            XDataValue::Distance(self.end_length_adjustment),
            XDataValue::Integer16(i16::from(self.associated)),
        ];
        data.upsert_record(record);
    }

    /// Remove only centre-line metadata, leaving other applications intact.
    pub fn remove(data: &mut ExtendedData) {
        data.remove_record(CENTERLINE_XDATA_APPLICATION);
    }
}

/// Kind of circular source referenced by a centre mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CenterMarkSourceKind {
    Circle,
    Arc,
    LwPolylineArcSegment,
    Polyline2DArcSegment,
}

impl CenterMarkSourceKind {
    fn code(self) -> i16 {
        match self {
            Self::Circle => 0,
            Self::Arc => 1,
            Self::LwPolylineArcSegment => 2,
            Self::Polyline2DArcSegment => 3,
        }
    }

    fn from_code(code: i16) -> Option<Self> {
        match code {
            0 => Some(Self::Circle),
            1 => Some(Self::Arc),
            2 => Some(Self::LwPolylineArcSegment),
            3 => Some(Self::Polyline2DArcSegment),
            _ => None,
        }
    }
}

/// One selected circle, arc, or circular polyline segment.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CenterMarkSource {
    pub handle: Handle,
    pub kind: CenterMarkSourceKind,
    pub segment_index: i32,
    pub pick_point: Vector3,
}

/// Complete, versioned metadata required to regenerate a smart centre mark.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CenterMarkAssociation {
    pub source: CenterMarkSource,
    pub plane_origin: Vector3,
    pub plane_x: Vector3,
    pub plane_y: Vector3,
    pub center: Vector3,
    pub radius: f64,
    pub cross_size: f64,
    pub cross_gap: f64,
    pub cross_size_relative: bool,
    pub cross_gap_relative: bool,
    pub extension_length: f64,
    pub length_adjustments: [f64; 4],
    pub overshoots: [f64; 4],
    pub show_extensions: bool,
    pub associated: bool,
}

impl CenterMarkAssociation {
    /// Decode association metadata, rejecting incomplete or future payloads.
    pub fn read(data: &ExtendedData) -> Option<Self> {
        let values = &data.get_record(CENTERMARK_XDATA_APPLICATION)?.values;
        let [XDataValue::String(signature), XDataValue::Integer16(version), XDataValue::Handle(source_handle), XDataValue::Integer16(source_kind), XDataValue::Integer32(segment_index), XDataValue::Point3D(pick_point), XDataValue::Point3D(plane_origin), XDataValue::Direction3D(plane_x), XDataValue::Direction3D(plane_y), XDataValue::Point3D(center), XDataValue::Distance(radius), XDataValue::Distance(cross_size), XDataValue::Distance(cross_gap), XDataValue::Distance(extension_length), XDataValue::Distance(length_0), XDataValue::Distance(length_1), XDataValue::Distance(length_2), XDataValue::Distance(length_3), XDataValue::Distance(overshoot_0), XDataValue::Distance(overshoot_1), XDataValue::Distance(overshoot_2), XDataValue::Distance(overshoot_3), XDataValue::Integer16(flags)] =
            values.as_slice()
        else {
            return None;
        };
        if signature != MARK_SIGNATURE || *version != VERSION {
            return None;
        }
        Some(Self {
            source: CenterMarkSource {
                handle: *source_handle,
                kind: CenterMarkSourceKind::from_code(*source_kind)?,
                segment_index: *segment_index,
                pick_point: *pick_point,
            },
            plane_origin: *plane_origin,
            plane_x: *plane_x,
            plane_y: *plane_y,
            center: *center,
            radius: *radius,
            cross_size: *cross_size,
            cross_gap: *cross_gap,
            extension_length: *extension_length,
            length_adjustments: [*length_0, *length_1, *length_2, *length_3],
            overshoots: [*overshoot_0, *overshoot_1, *overshoot_2, *overshoot_3],
            show_extensions: flags & 1 != 0,
            associated: flags & 2 != 0,
            cross_size_relative: flags & 4 != 0,
            cross_gap_relative: flags & 8 != 0,
        })
    }

    /// Replace the association payload without disturbing unrelated XDATA.
    pub fn write(&self, data: &mut ExtendedData) {
        let mut record = ExtendedDataRecord::new(CENTERMARK_XDATA_APPLICATION);
        record.values = vec![
            XDataValue::String(MARK_SIGNATURE.to_owned()),
            XDataValue::Integer16(VERSION),
            XDataValue::Handle(self.source.handle),
            XDataValue::Integer16(self.source.kind.code()),
            XDataValue::Integer32(self.source.segment_index),
            XDataValue::Point3D(self.source.pick_point),
            XDataValue::Point3D(self.plane_origin),
            XDataValue::Direction3D(self.plane_x),
            XDataValue::Direction3D(self.plane_y),
            XDataValue::Point3D(self.center),
            XDataValue::Distance(self.radius),
            XDataValue::Distance(self.cross_size),
            XDataValue::Distance(self.cross_gap),
            XDataValue::Distance(self.extension_length),
            XDataValue::Distance(self.length_adjustments[0]),
            XDataValue::Distance(self.length_adjustments[1]),
            XDataValue::Distance(self.length_adjustments[2]),
            XDataValue::Distance(self.length_adjustments[3]),
            XDataValue::Distance(self.overshoots[0]),
            XDataValue::Distance(self.overshoots[1]),
            XDataValue::Distance(self.overshoots[2]),
            XDataValue::Distance(self.overshoots[3]),
            XDataValue::Integer16(
                i16::from(self.show_extensions)
                    | (i16::from(self.associated) << 1)
                    | (i16::from(self.cross_size_relative) << 2)
                    | (i16::from(self.cross_gap_relative) << 3),
            ),
        ];
        data.upsert_record(record);
    }

    /// Remove only centre-mark metadata, leaving other applications intact.
    pub fn remove(data: &mut ExtendedData) {
        data.remove_record(CENTERMARK_XDATA_APPLICATION);
    }
}
