//! Inspection dimensions (DIMINSPECT): a dimension whose text is shown with
//! an inspection label and an inspection rate inside a frame.
//!
//! The data is the dimension's `ACAD_DSTYLE_DIMINSPECT` XDATA record, a list
//! of (1070 code, value) pairs:
//! * 393 — 1 while the inspection is shown, 0 once it was removed (the other
//!   values stay so adding it again restores them);
//! * 394 — flags: 1 round frame, 2 angular frame, 4 no frame, 16 the label
//!   is shown, 32 the rate is shown;
//! * 395 — the label, 396 — the rate (only written while shown).

use codec::entities::EntityCommon;
use codec::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};

pub const APP: &str = "ACAD_DSTYLE_DIMINSPECT";

const ON: i16 = 393;
const FLAGS: i16 = 394;
const LABEL: i16 = 395;
const RATE: i16 = 396;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    #[default]
    Round,
    Angular,
    None,
}

impl Shape {
    fn flag(self) -> i16 {
        match self {
            Shape::Round => 1,
            Shape::Angular => 2,
            Shape::None => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inspection {
    pub shape: Shape,
    /// The label, when shown.
    pub label: Option<String>,
    /// The inspection rate, when shown.
    pub rate: Option<String>,
}

/// The (code, value) pairs of the record, in order.
fn pairs(record: &ExtendedDataRecord) -> Vec<(i16, XDataValue)> {
    let mut out = Vec::new();
    let mut values = record.values.iter();
    while let Some(value) = values.next() {
        if let XDataValue::Integer16(code) = value {
            if let Some(value) = values.next() {
                out.push((*code, value.clone()));
            }
        }
    }
    out
}

/// The inspection shown on a dimension, if any.
pub fn read(xd: &ExtendedData) -> Option<Inspection> {
    let pairs = pairs(xd.get_record(APP)?);
    let int = |code: i16| {
        pairs.iter().find_map(|(c, v)| match v {
            XDataValue::Integer16(n) if *c == code => Some(*n),
            _ => None,
        })
    };
    let string = |code: i16| {
        pairs.iter().find_map(|(c, v)| match v {
            XDataValue::String(s) if *c == code => Some(s.clone()),
            _ => None,
        })
    };
    if int(ON) != Some(1) {
        return None;
    }
    let flags = int(FLAGS).unwrap_or(1 | 32);
    let shape = if flags & 2 != 0 {
        Shape::Angular
    } else if flags & 4 != 0 {
        Shape::None
    } else {
        Shape::Round
    };
    Some(Inspection {
        shape,
        label: (flags & 16 != 0).then(|| string(LABEL).unwrap_or_default()),
        rate: (flags & 32 != 0).then(|| string(RATE).unwrap_or_default()),
    })
}

fn record(values: Vec<(i16, XDataValue)>) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(APP);
    for (code, value) in values {
        record.add_value(XDataValue::Integer16(code));
        record.add_value(value);
    }
    record
}

fn replace_record(common: &mut EntityCommon, record: Option<ExtendedDataRecord>) {
    let mut rebuilt = ExtendedData::new();
    for existing in common.extended_data.records() {
        if existing.application_name != APP {
            rebuilt.add_record(existing.clone());
        }
    }
    if let Some(record) = record {
        rebuilt.add_record(record);
    }
    rebuilt.raw_dwg_eed = common.extended_data.raw_dwg_eed.clone();
    common.extended_data = rebuilt;
}

/// Show `inspection` on the dimension.
pub fn set(common: &mut EntityCommon, inspection: &Inspection) {
    let mut flags = inspection.shape.flag();
    let mut values = Vec::new();
    if let Some(rate) = &inspection.rate {
        flags |= 32;
        values.push((RATE, XDataValue::String(rate.clone())));
    }
    if let Some(label) = &inspection.label {
        flags |= 16;
        values.push((LABEL, XDataValue::String(label.clone())));
    }
    values.push((FLAGS, XDataValue::Integer16(flags)));
    values.push((ON, XDataValue::Integer16(1)));
    replace_record(common, Some(record(values)));
}

/// Stop showing the inspection, keeping its values. Returns whether the
/// dimension had one.
pub fn remove(common: &mut EntityCommon) -> bool {
    if read(&common.extended_data).is_none() {
        return false;
    }
    let Some(existing) = common.extended_data.get_record(APP) else {
        return false;
    };
    let mut values = vec![(ON, XDataValue::Integer16(0))];
    values.extend(pairs(existing).into_iter().filter(|(code, _)| *code != ON));
    replace_record(common, Some(record(values)));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_remove() {
        let mut common = EntityCommon::default();
        let inspection = Inspection {
            shape: Shape::Angular,
            label: Some("LBL".into()),
            rate: Some("50%".into()),
        };
        set(&mut common, &inspection);
        let flags = pairs(common.extended_data.get_record(APP).unwrap())
            .into_iter()
            .find(|(c, _)| *c == FLAGS)
            .map(|(_, v)| v);
        assert_eq!(flags, Some(XDataValue::Integer16(50)));
        assert_eq!(read(&common.extended_data), Some(inspection));
        assert!(remove(&mut common));
        assert_eq!(read(&common.extended_data), None);
        assert!(!remove(&mut common));
    }
}

/// The value text moved for the inspection layout and the label, rate and
/// frame drawn around it, as entities in the value text's plane.
///
/// The parts sit side by side, centred where the value text was; each
/// separator has `gap` (DIMGAP) on either side and the frame runs `gap`
/// above and below the text. A round frame ends in half circles centred on
/// the text ends, an angular one in points half the frame height out.
pub(crate) fn layout(
    inspection: &Inspection,
    value: codec::EntityType,
    document: &codec::Document,
    gap: f64,
) -> Vec<codec::EntityType> {
    use codec::entities::text::{TextHorizontalAlignment as H, TextVerticalAlignment as V};
    use codec::entities::{Arc, Line};
    use codec::types::Vector3;
    use codec::EntityType;

    let (anchor, height, rotation, style, width_factor, plain) = match &value {
        EntityType::Text(text) => (
            text.insertion_point,
            text.height,
            text.rotation,
            text.style.clone(),
            text.width_factor,
            text.value.clone(),
        ),
        EntityType::MText(mtext) => (
            mtext.insertion_point,
            mtext.height,
            mtext.rotation,
            mtext.style.clone(),
            1.0,
            plain(&mtext.value),
        ),
        _ => return vec![value],
    };
    if height <= 0.0 {
        return vec![value];
    }
    let font = crate::entities::text_support::resolve_text_style(&style, document).font_name;
    let advance = |text: &str| {
        crate::entities::text_support::text_local_bounds(
            &font,
            text,
            height as f32,
            width_factor as f32,
            0.0,
        )
        .map_or(0.0, |bounds| f64::from(bounds.advance))
    };
    let (sin, cos) = rotation.sin_cos();
    let along = |origin: Vector3, x: f64, y: f64| {
        Vector3::new(
            origin.x + x * cos - y * sin,
            origin.y + x * sin + y * cos,
            origin.z,
        )
    };
    // Centre of the value text as it was placed.
    let value_width = advance(&plain);
    let center = match &value {
        EntityType::Text(text) => {
            let dx = match text.horizontal_alignment {
                H::Left => value_width / 2.0,
                H::Right => -value_width / 2.0,
                _ => 0.0,
            };
            let dy = match text.vertical_alignment {
                V::Baseline | V::Bottom => height / 2.0,
                V::Top => -height / 2.0,
                _ => 0.0,
            };
            along(anchor, dx, dy)
        }
        _ => anchor,
    };

    let mut parts: Vec<(Option<String>, f64)> = Vec::new();
    if let Some(label) = &inspection.label {
        parts.push((Some(label.clone()), advance(label)));
    }
    parts.push((None, value_width));
    if let Some(rate) = &inspection.rate {
        parts.push((Some(rate.clone()), advance(rate)));
    }
    let total: f64 =
        parts.iter().map(|(_, w)| w).sum::<f64>() + 2.0 * gap * (parts.len() - 1) as f64;
    let half_h = height / 2.0 + gap;
    let left = -total / 2.0;

    let place = |mut entity: EntityType, at: Vector3| -> EntityType {
        match &mut entity {
            EntityType::Text(text) => {
                text.insertion_point = at;
                text.alignment_point = None;
                text.horizontal_alignment = H::Center;
                text.vertical_alignment = V::Middle;
            }
            EntityType::MText(mtext) => {
                mtext.insertion_point = at;
                mtext.attachment_point = codec::entities::AttachmentPoint::MiddleCenter;
            }
            _ => {}
        }
        entity
    };
    let line = |a: Vector3, b: Vector3| {
        let mut line = Line::from_points(a, b);
        line.common = part_common(&value);
        EntityType::Line(line)
    };

    let mut out = Vec::new();
    let mut x = left;
    for (index, (text, width)) in parts.iter().enumerate() {
        let at = along(center, x + width / 2.0, 0.0);
        match text {
            Some(text) => {
                let mut part = codec::entities::Text::with_value(text.clone(), at)
                    .with_height(height)
                    .with_rotation(rotation);
                part.style = style.clone();
                part.width_factor = width_factor;
                part.normal = match &value {
                    EntityType::Text(t) => t.normal,
                    EntityType::MText(m) => m.normal,
                    _ => Vector3::UNIT_Z,
                };
                part.common = part_common(&value);
                out.push(place(EntityType::Text(part), at));
            }
            None => out.push(place(value.clone(), at)),
        }
        x += width;
        if index + 1 < parts.len() {
            x += gap;
            if inspection.shape != Shape::None {
                out.push(line(along(center, x, -half_h), along(center, x, half_h)));
            }
            x += gap;
        }
    }
    let right = left + total;
    match inspection.shape {
        Shape::None => {}
        Shape::Round => {
            out.push(line(along(center, left, half_h), along(center, right, half_h)));
            out.push(line(along(center, left, -half_h), along(center, right, -half_h)));
            for (end, start, sweep) in [
                (left, std::f64::consts::FRAC_PI_2, std::f64::consts::PI),
                (right, -std::f64::consts::FRAC_PI_2, std::f64::consts::PI),
            ] {
                let mut arc = Arc::new();
                arc.center = along(center, end, 0.0);
                arc.radius = half_h;
                arc.start_angle = rotation + start;
                arc.end_angle = rotation + start + sweep;
                arc.common = part_common(&value);
                out.push(EntityType::Arc(arc));
            }
        }
        Shape::Angular => {
            let tip_left = along(center, left - half_h, 0.0);
            let tip_right = along(center, right + half_h, 0.0);
            let corners = [
                along(center, left, half_h),
                along(center, right, half_h),
                tip_right,
                along(center, right, -half_h),
                along(center, left, -half_h),
                tip_left,
            ];
            for k in 0..corners.len() {
                out.push(line(corners[k], corners[(k + 1) % corners.len()]));
            }
        }
    }
    out
}

/// The value text's properties for an added part, without its handle and
/// XDATA.
fn part_common(value: &codec::EntityType) -> EntityCommon {
    let mut common = value.common().clone();
    common.handle = codec::Handle::NULL;
    common.extended_data = ExtendedData::new();
    common
}

/// MText value without its format codes, for measuring.
fn plain(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => match chars.next() {
                Some('P' | '~') => out.push(' '),
                Some(other @ ('\\' | '{' | '}')) => out.push(other),
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k') | None => {}
                Some(_) => {
                    chars.by_ref().find(|c| *c == ';');
                }
            },
            other => out.push(other),
        }
    }
    out
}
