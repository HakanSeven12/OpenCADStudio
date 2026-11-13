//! Opt-in ISO 32000-2 geospatial metadata for model-space plot frames.
//!
//! Standard GPTS arrays contain PDF numbers, not strings. lopdf stores Real as
//! f32, so those arrays are serialized as placeholders and replaced here with
//! their original f64 decimal tokens. The classic xref table is rebuilt to keep
//! every indirect-object offset and startxref correct.

use crate::io::crs::CrsDefinition;
use crate::io::plot::PlotPage;
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub struct GeoPdfCoordinateSystem {
    projected: bool,
    epsg: Option<u16>,
    wkt: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeoPdfFrame {
    pub gcs: GeoPdfCoordinateSystem,
    pub display_cs: Option<GeoPdfCoordinateSystem>,
    /// Original model-space window in drawing units, before any f32 clip.
    pub model_window: [f64; 4],
    /// Latitude/longitude pairs in the base geographic CRS, one per model corner.
    pub gpts: [[f64; 2]; 4],
}

impl GeoPdfFrame {
    pub fn for_window(
        crs: &CrsDefinition,
        drawing_unit_mm: Option<f64>,
        window: (f64, f64, f64, f64),
    ) -> Result<Self, String> {
        let [x0, y0, x1, y1] = [window.0, window.1, window.2, window.3];
        if [x0, y0, x1, y1].iter().any(|value| !value.is_finite())
            || x1 - x0 <= 0.0
            || y1 - y0 <= 0.0
        {
            return Err("The GeoPDF model window is not finite and nondegenerate".to_string());
        }
        if crs.axis() != *b"enu" {
            return Err("GeoPDF export requires east/north drawing axes".to_string());
        }
        let projected = !crs.is_geographic();
        let unit_scale = if projected {
            let unit_mm = drawing_unit_mm
                .filter(|value| value.is_finite() && *value > 0.0)
                .ok_or_else(|| "GeoPDF export needs explicit drawing units".to_string())?;
            if !crs.to_meter().is_finite() || crs.to_meter() <= 0.0 {
                return Err("The drawing CRS has an invalid linear-unit scale".to_string());
            }
            unit_mm / (crs.to_meter() * 1000.0)
        } else {
            1.0
        };
        let model_corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        let mut gpts = [[0.0; 2]; 4];
        for (index, [x, y]) in model_corners.into_iter().enumerate() {
            let (lon, lat) = if projected {
                crs.to_base_geographic(x * unit_scale, y * unit_scale)?
            } else {
                (x, y)
            };
            if !lon.is_finite() || !lat.is_finite() {
                return Err("The GeoPDF geographic coordinates are not finite".to_string());
            }
            gpts[index] = [lat, lon];
        }
        let gcs = if projected {
            coordinate_system(&crs.base_geographic_definition()?, false)?
        } else {
            coordinate_system(crs, false)?
        };
        let display_cs = projected
            .then(|| coordinate_system(crs, true))
            .transpose()?;
        Ok(Self {
            gcs,
            display_cs,
            model_window: [x0, y0, x1, y1],
            gpts,
        })
    }
}

fn coordinate_system(
    crs: &CrsDefinition,
    projected: bool,
) -> Result<GeoPdfCoordinateSystem, String> {
    let epsg = crs.code();
    let wkt = epsg
        .is_none()
        .then(|| crs.wkt().map(str::to_string))
        .flatten();
    if epsg.is_none() && wkt.is_none() {
        return Err("GeoPDF needs an EPSG code or WKT coordinate system".to_string());
    }
    Ok(GeoPdfCoordinateSystem {
        projected,
        epsg,
        wkt,
    })
}

pub(crate) struct PreciseArray {
    id: ObjectId,
    values: Vec<f64>,
}

/// Add one ISO geospatial viewport to each page that explicitly carries a
/// frame. Other pages remain untouched, including when mixed in one document.
pub(crate) fn apply_metadata(
    document: &mut Document,
    pages: &[PlotPage],
) -> Result<Vec<PreciseArray>, String> {
    let page_ids: Vec<ObjectId> = document.get_pages().into_values().collect();
    if page_ids.len() != pages.len() {
        return Err("The rendered PDF page count does not match the plot input".to_string());
    }
    let mut precise = Vec::new();
    for (page, id) in pages.iter().zip(page_ids) {
        let Some(frame) = page.geopdf.as_ref() else {
            continue;
        };
        let (bbox, lpts) = viewport_geometry(page, frame)?;
        let bbox_id = document.add_object(Object::Array(vec![Object::Real(0.0); 4]));
        precise.push(PreciseArray {
            id: bbox_id,
            values: bbox.to_vec(),
        });
        let gpts_id = document.add_object(Object::Array(vec![Object::Real(0.0); 8]));
        precise.push(PreciseArray {
            id: gpts_id,
            values: frame.gpts.iter().flatten().copied().collect(),
        });

        let mut measure = Dictionary::new();
        measure.set("Type", Object::from("Measure"));
        measure.set("Subtype", Object::from("GEO"));
        measure.set("GCS", coordinate_object(&frame.gcs)?);
        if let Some(display) = frame.display_cs.as_ref() {
            measure.set("DCS", coordinate_object(display)?);
        }
        measure.set(
            "LPTS",
            Object::Array(lpts.into_iter().flatten().map(Object::Real).collect()),
        );
        measure.set("GPTS", Object::Reference(gpts_id));
        let mut viewport = Dictionary::new();
        viewport.set("Type", Object::from("Viewport"));
        viewport.set("BBox", Object::Reference(bbox_id));
        viewport.set("Measure", Object::Dictionary(measure));
        let viewport_id = document.add_object(Object::Dictionary(viewport));
        let page_dict = document
            .get_object_mut(id)
            .and_then(Object::as_dict_mut)
            .map_err(|error| format!("Cannot edit the GeoPDF page dictionary: {error}"))?;
        page_dict.set("VP", Object::Array(vec![Object::Reference(viewport_id)]));
        document.version = "2.0".to_string();
    }
    Ok(precise)
}

fn coordinate_object(system: &GeoPdfCoordinateSystem) -> Result<Object, String> {
    let mut dictionary = Dictionary::new();
    dictionary.set(
        "Type",
        Object::from(if system.projected { "PROJCS" } else { "GEOGCS" }),
    );
    if let Some(code) = system.epsg {
        dictionary.set("EPSG", Object::Integer(i64::from(code)));
    } else if let Some(wkt) = system.wkt.as_ref() {
        dictionary.set("WKT", Object::string_literal(wkt.as_str()));
    } else {
        return Err("The GeoPDF coordinate system has no definition".to_string());
    }
    Ok(Object::Dictionary(dictionary))
}

fn viewport_geometry(
    page: &PlotPage,
    frame: &GeoPdfFrame,
) -> Result<([f64; 4], [[f32; 2]; 4]), String> {
    let scale = f64::from(page.scale);
    if !scale.is_finite() || scale <= 0.0 {
        return Err("The GeoPDF plot scale is not positive and finite".to_string());
    }
    if !page.paper_w.is_finite()
        || !page.paper_h.is_finite()
        || page.paper_w <= 0.0
        || page.paper_h <= 0.0
    {
        return Err("The GeoPDF paper size is not positive and finite".to_string());
    }
    let [x0, y0, x1, y1] = frame.model_window;
    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let mut points = [[0.0; 2]; 4];
    for (index, [x, y]) in corners.into_iter().enumerate() {
        let x = (x + page.offset_x) * scale;
        let y = (y + page.offset_y) * scale;
        let (x, y) = match page.rotation_deg {
            0 => (x, y),
            90 => (page.paper_w - y, x),
            180 => (page.paper_w - x, page.paper_h - y),
            270 => (y, page.paper_h - x),
            _ => return Err("GeoPDF page rotation must be a right angle".to_string()),
        };
        points[index] = [x * 72.0 / 25.4, y * 72.0 / 25.4];
    }
    if points.iter().flatten().any(|value| !value.is_finite()) {
        return Err("The GeoPDF viewport is not finite".to_string());
    }
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for [x, y] in points {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    let (width, height) = (max_x - min_x, max_y - min_y);
    if width <= 0.0 || height <= 0.0 {
        return Err("The GeoPDF viewport is degenerate".to_string());
    }
    let lpts = points.map(|[x, y]| [((x - min_x) / width) as f32, ((y - min_y) / height) as f32]);
    Ok(([min_x, min_y, max_x, max_y], lpts))
}

pub(crate) fn save_document(
    document: &mut Document,
    precise: &[PreciseArray],
) -> Result<Vec<u8>, String> {
    document.compress();
    let mut bytes = Vec::new();
    document
        .save_to(&mut bytes)
        .map_err(|error| format!("PDF serialisation failed: {error}"))?;
    if precise.is_empty() {
        Ok(bytes)
    } else {
        replace_precise_arrays(bytes, precise)
    }
}

#[derive(Debug)]
struct XrefEntry {
    number: u32,
    offset: usize,
    generation: u16,
    in_use: bool,
}

#[derive(Debug)]
struct XrefSubsection {
    start: u32,
    entries: Vec<XrefEntry>,
}

#[derive(Debug)]
struct Patch {
    object_offset: usize,
    body_start: usize,
    body_end: usize,
    body: String,
}

fn replace_precise_arrays(mut bytes: Vec<u8>, precise: &[PreciseArray]) -> Result<Vec<u8>, String> {
    let xref_offset = rfind(&bytes, b"startxref")
        .and_then(|marker| {
            std::str::from_utf8(&bytes[marker + b"startxref".len()..])
                .ok()?
                .split_whitespace()
                .next()?
                .parse::<usize>()
                .ok()
        })
        .ok_or_else(|| "The PDF has no classic startxref offset".to_string())?;
    if !bytes
        .get(xref_offset..)
        .is_some_and(|tail| tail.starts_with(b"xref\n") || tail.starts_with(b"xref\r"))
    {
        return Err("GeoPDF precision requires a classic xref table".to_string());
    }
    let trailer_offset = find_from(&bytes, xref_offset + 4, b"trailer")
        .ok_or_else(|| "The PDF xref table has no trailer".to_string())?;
    let startxref_offset = find_from(&bytes, trailer_offset, b"startxref")
        .ok_or_else(|| "The PDF trailer has no startxref".to_string())?;
    let table = std::str::from_utf8(&bytes[xref_offset + 4..trailer_offset])
        .map_err(|_| "The PDF xref table is not ASCII".to_string())?;
    let mut tokens = table.split_whitespace();
    let mut subsections = Vec::new();
    while let Some(start) = tokens.next() {
        let start = start
            .parse::<u32>()
            .map_err(|_| "Invalid PDF xref subsection start".to_string())?;
        let count = tokens
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| "Invalid PDF xref subsection length".to_string())?;
        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let offset = tokens
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| "Invalid PDF xref object offset".to_string())?;
            let generation = tokens
                .next()
                .and_then(|value| value.parse::<u16>().ok())
                .ok_or_else(|| "Invalid PDF xref generation".to_string())?;
            let in_use = match tokens.next() {
                Some("n") => true,
                Some("f") => false,
                _ => return Err("Invalid PDF xref entry kind".to_string()),
            };
            entries.push(XrefEntry {
                number: start + index as u32,
                offset,
                generation,
                in_use,
            });
        }
        subsections.push(XrefSubsection { start, entries });
    }

    let original = bytes.clone();
    let mut patches = Vec::with_capacity(precise.len());
    for array in precise {
        let entry = subsections
            .iter()
            .flat_map(|section| section.entries.iter())
            .find(|entry| {
                entry.number == array.id.0 && entry.generation == array.id.1 && entry.in_use
            })
            .ok_or_else(|| "A precise GeoPDF object is missing from the xref".to_string())?;
        let header = format!("{} {} obj", array.id.0, array.id.1);
        if !original
            .get(entry.offset..)
            .is_some_and(|tail| tail.starts_with(header.as_bytes()))
        {
            return Err("A precise GeoPDF object offset is inconsistent".to_string());
        }
        let body_start = entry.offset + header.len();
        let body_end = find_from(&original, body_start, b"endobj")
            .ok_or_else(|| "A precise GeoPDF array has no endobj".to_string())?;
        let values = array
            .values
            .iter()
            .map(|value| pdf_decimal(*value))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
        patches.push(Patch {
            object_offset: entry.offset,
            body_start,
            body_end,
            body: format!("\n[{values}]\n"),
        });
    }
    patches.sort_by_key(|patch| std::cmp::Reverse(patch.body_start));
    for patch in &patches {
        bytes.splice(
            patch.body_start..patch.body_end,
            patch.body.as_bytes().iter().copied(),
        );
    }

    let shifted = |offset: usize| -> Result<usize, String> {
        let mut shifted = offset as i128;
        for patch in &patches {
            if patch.object_offset < offset {
                shifted += patch.body.len() as i128 - (patch.body_end - patch.body_start) as i128;
            }
        }
        usize::try_from(shifted).map_err(|_| "The rebuilt PDF offset overflowed".to_string())
    };
    let new_xref_offset = shifted(xref_offset)?;
    let mut output = bytes[..new_xref_offset].to_vec();
    let mut tail = String::from("xref\n");
    for section in &subsections {
        writeln!(tail, "{} {}", section.start, section.entries.len()).unwrap();
        for entry in &section.entries {
            let offset = if entry.in_use {
                shifted(entry.offset)?
            } else {
                entry.offset
            };
            if offset as u64 > 9_999_999_999_u64 {
                return Err("The rebuilt PDF exceeds classic xref offsets".to_string());
            }
            writeln!(
                tail,
                "{offset:010} {:05} {}",
                entry.generation,
                if entry.in_use { 'n' } else { 'f' }
            )
            .unwrap();
        }
    }
    tail.push_str("trailer");
    tail.push_str(
        std::str::from_utf8(&original[trailer_offset + b"trailer".len()..startxref_offset])
            .map_err(|_| "The PDF trailer is not ASCII".to_string())?,
    );
    writeln!(tail, "startxref\n{new_xref_offset}\n%%EOF").unwrap();
    output.extend(tail.as_bytes());
    Ok(output)
}

fn pdf_decimal(value: f64) -> Result<String, String> {
    if !value.is_finite() {
        return Err("A GeoPDF coordinate is not finite".to_string());
    }
    if value == 0.0 {
        return Ok("0".to_string());
    }
    let text = value.to_string();
    let Some(exponent_at) = text.find(['e', 'E']) else {
        return Ok(text);
    };
    // PDF numbers do not permit exponent notation. Expand Rust's shortest
    // round-trip decimal instead of formatting through a fixed precision.
    let (mantissa, exponent) = text.split_at(exponent_at);
    let exponent = exponent[1..]
        .parse::<i32>()
        .map_err(|_| "A GeoPDF coordinate has an invalid exponent".to_string())?;
    let (sign, unsigned) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |unsigned| ("-", unsigned));
    let integer_digits = unsigned.find('.').unwrap_or(unsigned.len());
    let digits: String = unsigned.chars().filter(char::is_ascii_digit).collect();
    let decimal_at = integer_digits as i32 + exponent;
    let mut fixed =
        String::with_capacity(sign.len() + digits.len() + exponent.unsigned_abs() as usize + 2);
    fixed.push_str(sign);
    if decimal_at <= 0 {
        fixed.push_str("0.");
        fixed.extend(std::iter::repeat_n('0', (-decimal_at) as usize));
        fixed.push_str(&digits);
    } else if decimal_at as usize >= digits.len() {
        fixed.push_str(&digits);
        fixed.extend(std::iter::repeat_n('0', decimal_at as usize - digits.len()));
    } else {
        let decimal_at = decimal_at as usize;
        fixed.push_str(&digits[..decimal_at]);
        fixed.push('.');
        fixed.push_str(&digits[decimal_at..]);
    }
    Ok(fixed)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

fn find_from(haystack: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    haystack
        .get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| start + position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epsg_5514_frame_uses_base_geographic_gcs_and_projected_dcs() {
        let crs = CrsDefinition::from_epsg(5514).unwrap();
        let frame = GeoPdfFrame::for_window(
            &crs,
            Some(1000.0),
            (
                -742_925.271_389_167_2,
                -1_043_020.310_123_933_5,
                -742_905.0,
                -1_043_000.0,
            ),
        )
        .unwrap();
        assert_eq!(frame.gcs.epsg, Some(4156));
        assert!(!frame.gcs.projected);
        assert_eq!(frame.display_cs.as_ref().unwrap().epsg, Some(5514));
        assert!(frame.display_cs.as_ref().unwrap().projected);
        let (expected_lon, expected_lat) = crs
            .to_base_geographic(frame.model_window[0], frame.model_window[1])
            .unwrap();
        assert_eq!(frame.gpts[0], [expected_lat, expected_lon]);
    }

    #[test]
    fn unknown_units_and_non_east_north_axes_are_rejected() {
        let crs = CrsDefinition::from_epsg(5514).unwrap();
        assert!(GeoPdfFrame::for_window(&crs, None, (0.0, 0.0, 1.0, 1.0)).is_err());
        let geographic = CrsDefinition::from_epsg(4326).unwrap();
        assert!(GeoPdfFrame::for_window(&geographic, None, (14.0, 50.0, 15.0, 51.0)).is_ok());
        assert!(GeoPdfFrame::for_window(&crs, Some(1000.0), (0.0, 0.0, 0.0, 1.0)).is_err());
    }

    #[test]
    fn decimal_fallback_never_emits_exponents_or_non_finite_values() {
        assert_eq!(pdf_decimal(-0.0).unwrap(), "0");
        assert_eq!(pdf_decimal(1e-20).unwrap(), "0.00000000000000000001");
        for value in [-1.234_567_890_123_456_7e25, f64::MIN_POSITIVE, f64::MAX] {
            let text = pdf_decimal(value).unwrap();
            assert!(!text.contains(['e', 'E']), "{text}");
            assert_eq!(text.parse::<f64>().unwrap(), value);
        }
        assert!(pdf_decimal(f64::NAN).is_err());
    }
}
