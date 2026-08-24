//! Per-object dimension-variable overrides.
//!
//! A leader (or dimension) that departs from its dimension style stores the
//! changed variables in the standard `ACAD` XDATA record, identified by a
//! leading `DSTYLE` string, as a list of (dimvar group code, value) pairs wrapped
//! in `{ }` control strings. Both the renderer and the properties panel prefer
//! an override over the style default, so editing one of these rows writes here
//! and the change round-trips to file.

use acadrust::types::Color;
use acadrust::xdata::{ExtendedData, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};

// DXF group codes of the dimension variables surfaced on the leader panel.
pub const DIMSCALE: i16 = 40; // overall scale       (real)
pub const DIMPOST: i16 = 3;
pub const DIMAPOST: i16 = 4;
pub const DIMASZ: i16 = 41; // arrow size          (real)
pub const DIMTAD: i16 = 77; // text vertical pos   (int16)
pub const DIMCLRD: i16 = 176; // dim line colour     (int16 = ACI index)
pub const DIMGAP: i16 = 147; // text offset / gap   (real)
pub const DIMLWD: i16 = 371; // dim line lineweight (int16)
pub const DIMLDRBLK: i16 = 341; // leader arrow block  (handle)
pub const DIMEXO: i16 = 42;
pub const DIMDLI: i16 = 43;
pub const DIMEXE: i16 = 44;
pub const DIMRND: i16 = 45;
pub const DIMDLE: i16 = 46;
pub const DIMTP: i16 = 47;
pub const DIMTM: i16 = 48;
pub const DIMFXL: i16 = 49;
pub const DIMJOGANG: i16 = 50;
pub const DIMTOL: i16 = 71;
pub const DIMLIM: i16 = 72;
pub const DIMTIH: i16 = 73;
pub const DIMTOH: i16 = 74;
pub const DIMSE1: i16 = 75;
pub const DIMSE2: i16 = 76;
pub const DIMZIN: i16 = 78;
pub const DIMAZIN: i16 = 79;
pub const DIMARCSYM: i16 = 90;
pub const DIMTXT: i16 = 140;
pub const DIMCEN: i16 = 141;
pub const DIMTSZ: i16 = 142;
pub const DIMALTF: i16 = 143;
pub const DIMLFAC: i16 = 144;
pub const DIMTVP: i16 = 145;
pub const DIMTFAC: i16 = 146;
pub const DIMALTRND: i16 = 148;
pub const DIMALT: i16 = 170;
pub const DIMALTD: i16 = 171;
pub const DIMTOFL: i16 = 172;
pub const DIMSAH: i16 = 173;
pub const DIMTIX: i16 = 174;
pub const DIMSOXD: i16 = 175;
pub const DIMCLRE: i16 = 177;
pub const DIMCLRT: i16 = 178;
pub const DIMADEC: i16 = 179;
pub const DIMDEC: i16 = 271;
pub const DIMTDEC: i16 = 272;
pub const DIMALTU: i16 = 273;
pub const DIMALTTD: i16 = 274;
pub const DIMAUNIT: i16 = 275;
pub const DIMFRAC: i16 = 276;
pub const DIMLUNIT: i16 = 277;
pub const DIMDSEP: i16 = 278;
pub const DIMTMOVE: i16 = 279;
pub const DIMJUST: i16 = 280;
pub const DIMSD1: i16 = 281;
pub const DIMSD2: i16 = 282;
pub const DIMTOLJ: i16 = 283;
pub const DIMTZIN: i16 = 284;
pub const DIMALTZ: i16 = 285;
pub const DIMALTTZ: i16 = 286;
pub const DIMUPT: i16 = 288;
pub const DIMATFIT: i16 = 289;
pub const DIMFXLON: i16 = 290;
pub const DIMTFILL: i16 = 69;
pub const DIMTFILLCLR: i16 = 70;
pub const DIMTXTDIRECTION: i16 = 295;
pub const DIMTXSTY: i16 = 340;
pub const DIMBLK: i16 = 342;
pub const DIMBLK1: i16 = 343;
pub const DIMBLK2: i16 = 344;
pub const DIMLTYPE: i16 = 345;
pub const DIMLTEX1: i16 = 346;
pub const DIMLTEX2: i16 = 347;
pub const DIMLWE: i16 = 372;

/// Every (code, value) override present in the `ACAD`/`DSTYLE` record.
pub fn pairs(xd: &ExtendedData) -> Vec<(i16, XDataValue)> {
    let values = xd
        .get_record("ACAD")
        .and_then(|rec| match rec.values.first() {
            Some(XDataValue::String(name)) if name == "DSTYLE" => Some(&rec.values[1..]),
            _ => None,
        })
        // Retain compatibility with records produced by older OCS versions.
        .or_else(|| {
            xd.get_record("ACAD_DSTYLE")
                .map(|rec| rec.values.as_slice())
        });
    let Some(values) = values else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut it = values.iter();
    // The record is a flat stream: a 1070 code marker followed by its typed
    // value, bracketed by 1002 "{" / "}" control strings (which are skipped).
    while let Some(v) = it.next() {
        if let XDataValue::Integer16(code) = v {
            if let Some(val) = it.next() {
                out.push((*code, val.clone()));
            }
        }
    }
    out
}

/// The real-valued override for `code`, if present.
pub fn real(xd: &ExtendedData, code: i16) -> Option<f64> {
    pairs(xd)
        .into_iter()
        .find(|(c, _)| *c == code)
        .and_then(|(_, v)| match v {
            XDataValue::Real(r) | XDataValue::Distance(r) | XDataValue::ScaleFactor(r) => Some(r),
            _ => None,
        })
}

/// The 16-bit-integer override for `code`, if present.
pub fn int(xd: &ExtendedData, code: i16) -> Option<i16> {
    pairs(xd)
        .into_iter()
        .find(|(c, _)| *c == code)
        .and_then(|(_, v)| match v {
            XDataValue::Integer16(n) => Some(n),
            _ => None,
        })
}

/// The colour override for `code`, if present. Dim-colour overrides are stored
/// as an ACI index (the same `int16` slot the dimension style uses), so this
/// decodes it back into a `Color` (0 = ByBlock, 256 = ByLayer, else indexed).
pub fn color(xd: &ExtendedData, code: i16) -> Option<Color> {
    int(xd, code).map(Color::from_index)
}

/// The handle-valued override for `code`, if present.
pub fn handle(xd: &ExtendedData, code: i16) -> Option<Handle> {
    pairs(xd)
        .into_iter()
        .find(|(c, _)| *c == code)
        .and_then(|(_, v)| match v {
            XDataValue::Handle(h) => Some(h),
            _ => None,
        })
}

pub fn string(xd: &ExtendedData, code: i16) -> Option<String> {
    pairs(xd)
        .into_iter()
        .find(|(c, _)| *c == code)
        .and_then(|(_, value)| match value {
            XDataValue::String(text) => Some(text),
            _ => None,
        })
}

fn write_pairs(doc: &mut CadDocument, handle: Handle, pairs: Vec<(i16, XDataValue)>) {
    let Some(entity) = doc.get_entity(handle) else {
        return;
    };
    let use_canonical_record = entity
        .common()
        .extended_data
        .get_record("ACAD")
        .map(|rec| {
            matches!(
                rec.values.first(),
                Some(XDataValue::String(name)) if name == "DSTYLE"
            )
        })
        .unwrap_or(true);

    let mut values = if pairs.is_empty() {
        None
    } else {
        let mut vals = vec![XDataValue::ControlString("{".to_string())];
        for (code, value) in pairs {
            vals.push(XDataValue::Integer16(code));
            vals.push(value);
        }
        vals.push(XDataValue::ControlString("}".to_string()));
        Some(vals)
    };

    if use_canonical_record {
        if let Some(vals) = &mut values {
            vals.insert(0, XDataValue::String("DSTYLE".to_string()));
        }
        crate::scene::view::dispatch::set_entity_xdata(doc, handle, "ACAD_DSTYLE", None);
        crate::scene::view::dispatch::set_entity_xdata(doc, handle, "ACAD", values);
    } else {
        // Preserve unrelated Autodesk XDATA already occupying the ACAD record.
        crate::scene::view::dispatch::set_entity_xdata(doc, handle, "ACAD_DSTYLE", values);
    }
}

/// Replace every dimension-variable override on entity `handle`.
pub fn replace(doc: &mut CadDocument, handle: Handle, values: Vec<(i16, XDataValue)>) {
    write_pairs(doc, handle, values);
}

/// Set — or, with `value: None`, clear — a single override on entity `handle`,
/// leaving the other overrides in the record untouched. Clearing the last one
/// drops the whole `ACAD`/`DSTYLE` record. Legacy `ACAD_DSTYLE` records written
/// by older OCS versions are migrated when the canonical `ACAD` slot is free.
pub fn set(doc: &mut CadDocument, handle: Handle, code: i16, value: Option<XDataValue>) {
    let Some(entity) = doc.get_entity(handle) else {
        return;
    };
    let mut kept: Vec<(i16, XDataValue)> = pairs(&entity.common().extended_data)
        .into_iter()
        .filter(|(c, _)| *c != code)
        .collect();
    if let Some(v) = value {
        kept.push((code, v));
    }
    write_pairs(doc, handle, kept);
}

pub fn property_real_code(field: &str) -> Option<i16> {
    Some(match field {
        "dim_arrow_size" => DIMASZ,
        "dim_line_ext" => DIMDLE,
        "dim_ext_line_ext" => DIMEXE,
        "dim_ext_line_offset" => DIMEXO,
        "dim_ext_line_fixed_length" => DIMFXL,
        "dim_text_height" => DIMTXT,
        "dim_text_offset" => DIMGAP,
        "dim_scale_overall" => DIMSCALE,
        "dim_roundoff" => DIMRND,
        "dim_scale_linear" => DIMLFAC,
        "dim_alt_scale_factor" => DIMALTF,
        "dim_alt_roundoff" => DIMALTRND,
        "dim_tolerance_limit_lower" => DIMTM,
        "dim_tolerance_limit_upper" => DIMTP,
        "dim_tolerance_text_height" => DIMTFAC,
        _ => return None,
    })
}

pub fn property_int_code(field: &str) -> Option<i16> {
    Some(match field {
        "dim_line_lineweight" => DIMLWD,
        "dim_ext_line_lineweight" => DIMLWE,
        "dim_ext_line_fixed" => DIMFXLON,
        "dim_text_pos_vert" => DIMTAD,
        "dim_text_pos_hor" => DIMJUST,
        "dim_text_outside_align" => DIMTOH,
        "dim_text_inside_align" => DIMTIH,
        "dim_fit" => DIMATFIT,
        "dim_text_inside" => DIMTIX,
        "dim_text_movement" => DIMTMOVE,
        "dim_line_forced" => DIMTOFL,
        "dim_line_inside" => DIMSOXD,
        "dim_units" => DIMLUNIT,
        "dim_precision" => DIMDEC,
        "dim_decimal_separator" => DIMDSEP,
        "dim_angle_format" => DIMAUNIT,
        "dim_angle_precision" => DIMADEC,
        "dim_alt_enabled" => DIMALT,
        "dim_alt_format" => DIMALTU,
        "dim_alt_precision" => DIMALTD,
        "dim_tolerance_precision" => DIMTDEC,
        "dim_tolerance_pos_vert" => DIMTOLJ,
        _ => return None,
    })
}

pub fn property_string_code(field: &str) -> Option<i16> {
    Some(match field {
        "dim_prefix_suffix" => DIMPOST,
        "dim_alt_prefix_suffix" => DIMAPOST,
        _ => return None,
    })
}

fn yes(value: &str) -> Option<i16> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" | "on" | "true" | "1" => Some(1),
        "no" | "off" | "false" | "0" => Some(0),
        _ => None,
    }
}

fn inherited_int(doc: &CadDocument, handle: Handle, code: i16) -> i16 {
    let Some(EntityType::Dimension(dimension)) = doc.get_entity(handle) else {
        return 0;
    };
    if let Some(value) = int(&dimension.base().common.extended_data, code) {
        return value;
    }
    let Some(style) = doc.dim_styles.iter().find(|style| {
        style.name.eq_ignore_ascii_case(&dimension.base().style_name)
            || (dimension.base().style_name.trim().is_empty()
                && style.name.eq_ignore_ascii_case("Standard"))
    }) else {
        return 0;
    };
    match code {
        DIMSD1 => style.dimsd1 as i16,
        DIMSD2 => style.dimsd2 as i16,
        DIMSE1 => style.dimse1 as i16,
        DIMSE2 => style.dimse2 as i16,
        DIMZIN => style.dimzin,
        DIMALTZ => style.dimaltz,
        DIMTZIN => style.dimtzin,
        DIMTOL => style.dimtol as i16,
        DIMLIM => style.dimlim as i16,
        _ => 0,
    }
}

pub fn set_property(
    doc: &mut CadDocument,
    handle: Handle,
    field: &str,
    value: &str,
) -> bool {
    let trimmed = value.trim();
    let handle_field = match field {
        "dim_arrowhead_1" => Some((DIMBLK1, true)),
        "dim_arrowhead_2" => Some((DIMBLK2, true)),
        "dim_linetype" => Some((DIMLTYPE, false)),
        "dim_ext_linetype_1" => Some((DIMLTEX1, false)),
        "dim_ext_linetype_2" => Some((DIMLTEX2, false)),
        _ => None,
    };
    if let Some((code, block)) = handle_field {
        let resolved = if block {
            (trimmed != "Closed filled")
                .then(|| {
                    doc.block_records
                        .iter()
                        .find(|record| record.name == trimmed)
                        .map(|record| record.handle)
                })
                .flatten()
        } else {
            doc.line_types
                .iter()
                .find(|line_type| line_type.name == trimmed)
                .map(|line_type| line_type.handle)
        };
        set(doc, handle, code, resolved.map(XDataValue::Handle));
        return true;
    }
    let inverse_boolean_code = match field {
        "dim_line_1" => Some(DIMSD1),
        "dim_line_2" => Some(DIMSD2),
        "dim_ext_line_1" => Some(DIMSE1),
        "dim_ext_line_2" => Some(DIMSE2),
        _ => None,
    };
    if let Some(code) = inverse_boolean_code {
        let Some(visible) = yes(trimmed) else {
            return false;
        };
        set(doc, handle, code, Some(XDataValue::Integer16((visible == 0) as i16)));
        return true;
    }
    let bit_field = match field {
        "dim_suppress_leading_zeros" => Some((DIMZIN, 4)),
        "dim_suppress_trailing_zeros" => Some((DIMZIN, 8)),
        "dim_alt_suppress_leading_zeros" => Some((DIMALTZ, 4)),
        "dim_alt_suppress_trailing_zeros" => Some((DIMALTZ, 8)),
        "dim_tolerance_suppress_leading_zeros" => Some((DIMTZIN, 4)),
        "dim_tolerance_suppress_trailing_zeros" => Some((DIMTZIN, 8)),
        _ => None,
    };
    if let Some((code, bit)) = bit_field {
        let Some(enabled) = yes(trimmed) else {
            return false;
        };
        let current = inherited_int(doc, handle, code);
        let value = if enabled != 0 {
            current | bit
        } else {
            current & !bit
        };
        set(doc, handle, code, Some(XDataValue::Integer16(value)));
        return true;
    }
    if field == "dim_tolerance_display" {
        let (tolerance, limits) = match trimmed.to_ascii_lowercase().as_str() {
            "deviation" => (1, 0),
            "limits" => (0, 1),
            "none" => (0, 0),
            _ => return false,
        };
        set(doc, handle, DIMTOL, Some(XDataValue::Integer16(tolerance)));
        set(doc, handle, DIMLIM, Some(XDataValue::Integer16(limits)));
        return true;
    }
    if let Some(code) = property_real_code(field) {
        if trimmed.is_empty() {
            set(doc, handle, code, None);
        } else if let Ok(number) = trimmed.parse::<f64>() {
            set(doc, handle, code, Some(XDataValue::Real(number)));
        } else {
            return false;
        }
        return true;
    }
    if let Some(code) = property_string_code(field) {
        set(
            doc,
            handle,
            code,
            (!trimmed.is_empty()).then(|| XDataValue::String(value.to_string())),
        );
        return true;
    }
    if let Some(code) = property_int_code(field) {
        let parsed = match field {
            "dim_ext_line_fixed"
            | "dim_text_outside_align"
            | "dim_text_inside_align"
            | "dim_text_inside"
            | "dim_line_forced"
            | "dim_line_inside"
            | "dim_alt_enabled" => yes(trimmed),
            "dim_decimal_separator" => trimmed
                .chars()
                .next()
                .map(|character| character as i16),
            "dim_units" => match trimmed.to_ascii_lowercase().as_str() {
                "scientific" => Some(1),
                "decimal" => Some(2),
                "engineering" => Some(3),
                "architectural" => Some(4),
                "fractional" => Some(5),
                "desktop" => Some(6),
                _ => trimmed.parse().ok(),
            },
            "dim_angle_format" => match trimmed.to_ascii_lowercase().as_str() {
                "decimal degrees" => Some(0),
                "degrees/minutes/seconds" => Some(1),
                "gradians" => Some(2),
                "radians" => Some(3),
                "surveyors units" => Some(4),
                _ => trimmed.parse().ok(),
            },
            "dim_text_pos_vert" => match trimmed.to_ascii_lowercase().as_str() {
                "centered" => Some(0),
                "above" => Some(1),
                "outside" => Some(2),
                "jis" => Some(3),
                "below" => Some(4),
                _ => trimmed.parse().ok(),
            },
            "dim_text_pos_hor" => match trimmed.to_ascii_lowercase().as_str() {
                "centered" => Some(0),
                "at extension line 1" => Some(1),
                "at extension line 2" => Some(2),
                "over extension line 1" => Some(3),
                "over extension line 2" => Some(4),
                _ => trimmed.parse().ok(),
            },
            "dim_fit" => match trimmed.to_ascii_lowercase().as_str() {
                "either text or arrows" => Some(0),
                "arrows" => Some(1),
                "text" => Some(2),
                "both text and arrows" => Some(3),
                "always keep text" => Some(4),
                _ => trimmed.parse().ok(),
            },
            "dim_text_movement" => match trimmed.to_ascii_lowercase().as_str() {
                "move dimension line" => Some(0),
                "add leader" => Some(1),
                "move text freely" => Some(2),
                _ => trimmed.parse().ok(),
            },
            "dim_tolerance_pos_vert" => match trimmed.to_ascii_lowercase().as_str() {
                "bottom" => Some(0),
                "middle" => Some(1),
                "top" => Some(2),
                _ => trimmed.parse().ok(),
            },
            _ => trimmed.parse().ok(),
        };
        let Some(number) = parsed else {
            return false;
        };
        set(doc, handle, code, Some(XDataValue::Integer16(number)));
        return true;
    }
    false
}
