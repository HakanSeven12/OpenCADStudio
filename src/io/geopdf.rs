//! GeoPDF georeferencing for underlay placement.
//!
//! Only a single, unambiguous ISO or legacy frame is accepted. The drawing CRS
//! is never inferred or changed, and no cross-datum operation is performed.

mod affine;

pub use affine::GeoPlacement;
use affine::{decompose, fit_affine};
use hayro::hayro_interpret::TransformExt;
use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Name, Object, String as PdfString};
use hayro::hayro_syntax::{Pdf, PdfVersion};
use kurbo::Affine;

use crate::io::crs::CrsDefinition;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GcsKind {
    Geographic,
    Projected,
}

/// Compute one 0-based page's underlay-local-inches → drawing transform.
/// `drawing_unit_mm` is the explicit INSUNITS conversion, not a CRS guess.
pub fn placement_for_pdf_page(
    pdf: &Pdf,
    page_index: usize,
    drawing_crs: &CrsDefinition,
    drawing_unit_mm: f64,
) -> Result<GeoPlacement, String> {
    if !drawing_unit_mm.is_finite() || drawing_unit_mm <= 0.0 {
        return Err("The drawing has unknown units".to_string());
    }
    let page = pdf
        .pages()
        .get(page_index)
        .ok_or_else(|| "The GeoPDF page does not exist".to_string())?;
    let map = if page.raw().contains_key(b"VP") && page.raw().contains_key(b"LGIDict") {
        return Err("Both ISO and legacy GeoPDF metadata are present".to_string());
    } else if page.raw().contains_key(b"VP") {
        iso_affine(pdf, page.raw(), drawing_crs, drawing_unit_mm)?
    } else if page.raw().contains_key(b"LGIDict") {
        legacy_affine(page.raw(), drawing_crs, drawing_unit_mm)?
    } else {
        return Err("The page has no GeoPDF metadata".to_string());
    };

    // Underlay local coordinates are rendered, crop/rotation-aware y-up points
    // divided by 72. Do not apply /UserUnit again: the fitted affine already
    // describes map units per PDF user-space unit.
    let page_to_render = page.initial_transform(false);
    let page_to_render = page_to_render.to_kurbo();
    if page_to_render.as_coeffs().iter().any(|v| !v.is_finite()) {
        return Err("The PDF page transform is non-finite".to_string());
    }
    let determinant = page_to_render.determinant();
    if !determinant.is_finite() || determinant.abs() <= 1e-15 {
        return Err("The PDF page transform is singular".to_string());
    }
    decompose(map * page_to_render.inverse() * Affine::scale(72.0))
}

/// Password-aware source adapter shared with the existing underlay stack.
pub(crate) fn placement_for_source(
    path: &str,
    page: &str,
    drawing_crs: &CrsDefinition,
    drawing_unit_mm: f64,
) -> Result<GeoPlacement, String> {
    let bytes = crate::scene::model::pdf_raster::source_bytes(path)
        .ok_or_else(|| "The PDF source bytes are unavailable".to_string())?;
    let pdf = crate::scene::model::pdf_password::open(path, bytes)
        .map_err(|error| format!("Cannot open the GeoPDF: {error:?}"))?;
    let page_number = page
        .trim()
        .parse::<usize>()
        .map_err(|_| "The PDF page number is invalid".to_string())?;
    let index = page_number
        .checked_sub(1)
        .ok_or_else(|| "PDF pages are numbered from 1".to_string())?;
    placement_for_pdf_page(&pdf, index, drawing_crs, drawing_unit_mm)
}

fn iso_affine(
    pdf: &Pdf,
    page: &Dict<'_>,
    drawing_crs: &CrsDefinition,
    drawing_unit_mm: f64,
) -> Result<Affine, String> {
    let viewports = dictionaries(page, b"VP")?;
    let mut geospatial = Vec::new();
    for viewport in viewports {
        let Some(measure) = viewport.get::<Dict<'_>>(b"Measure") else {
            continue;
        };
        if name(&measure, b"Subtype").as_deref() == Some(b"GEO".as_slice()) {
            geospatial.push((viewport, measure));
        }
    }
    if geospatial.len() != 1 {
        return Err("The page needs exactly one geospatial viewport".to_string());
    }
    let (viewport, measure) = &geospatial[0];
    let gcs = measure
        .get::<Dict<'_>>(b"GCS")
        .ok_or_else(|| "The geospatial viewport has no GCS".to_string())?;
    let (pdf_crs, kind) = parse_gcs(&gcs)?;
    ensure_same_crs(&pdf_crs, kind, drawing_crs)?;
    let iso = pdf.version() >= PdfVersion::Pdf20;
    if iso
        && kind == GcsKind::Projected
        && (measure.contains_key(b"PCSM") || gcs.contains_key(b"PCSM"))
    {
        return Err("Projected PCSM is not supported; GPTS cannot take priority".to_string());
    }

    let bbox = required_numbers(viewport, b"BBox", b"BBox")?;
    if bbox.len() != 4 || bbox.iter().any(|value| !value.is_finite()) {
        return Err("The GeoPDF BBox must contain four finite numbers".to_string());
    }
    let width = bbox[2] - bbox[0];
    let height = bbox[3] - bbox[1];
    if !width.is_finite() || !height.is_finite() || width == 0.0 || height == 0.0 {
        return Err("The GeoPDF BBox is degenerate".to_string());
    }
    let lpts = preferred_numbers(measure, b"LPTS", b"lgit:LPTS")?;
    let gpts = preferred_numbers(measure, b"GPTS", b"lgit:GPTS")?;
    if lpts.len() != gpts.len() || lpts.len() < 6 || lpts.len() % 2 != 0 {
        return Err("GeoPDF LPTS and GPTS need equal, even lengths of at least six".to_string());
    }
    let unit_scale = source_to_drawing_units(drawing_crs, drawing_unit_mm);
    let mut points = Vec::with_capacity(lpts.len() / 2);
    for index in 0..lpts.len() / 2 {
        let pdf_x = bbox[0] + lpts[index * 2] * width;
        let pdf_y = bbox[1] + lpts[index * 2 + 1] * height;
        let first = gpts[index * 2];
        let second = gpts[index * 2 + 1];
        let (x, y) = if iso && kind == GcsKind::Projected {
            (first * unit_scale, second * unit_scale)
        } else {
            // Adobe EL3 uses latitude/longitude for projected and geographic
            // GCS alike; ISO retains that ordering for a geographic GCS.
            let (x, y) = drawing_crs.from_base_geographic(second, first)?;
            (x * unit_scale, y * unit_scale)
        };
        points.push(([pdf_x, pdf_y], [x, y]));
    }
    fit_affine(&points)
}

fn legacy_affine(
    page: &Dict<'_>,
    drawing_crs: &CrsDefinition,
    drawing_unit_mm: f64,
) -> Result<Affine, String> {
    let dictionaries = dictionaries(page, b"LGIDict")?;
    if dictionaries.len() != 1 {
        return Err("The page needs exactly one legacy GeoPDF dictionary".to_string());
    }
    let lgi = &dictionaries[0];
    if name(lgi, b"Type").as_deref() != Some(b"LGIDict".as_slice()) {
        return Err("The legacy dictionary is not an LGIDict".to_string());
    }
    let projection = lgi
        .get::<Dict<'_>>(b"Projection")
        .ok_or_else(|| "The legacy GeoPDF has no projection".to_string())?;
    let pdf_crs = parse_legacy_projection(&projection)?;
    let kind = if pdf_crs.is_geographic() {
        GcsKind::Geographic
    } else {
        GcsKind::Projected
    };
    ensure_same_crs(&pdf_crs, kind, drawing_crs)?;
    if kind == GcsKind::Projected && (pdf_crs.to_meter() - 1.0).abs() > 1e-12 {
        return Err("Legacy GeoPDF is currently limited to metre-based projected CRSs".to_string());
    }
    let units = lgi
        .get::<Name<'_>>(b"Units")
        .map(|value| value.as_ref().to_vec())
        .or_else(|| {
            lgi.get::<PdfString<'_>>(b"Units")
                .map(|s| s.as_bytes().to_vec())
        })
        .or_else(|| {
            projection
                .get::<Name<'_>>(b"Units")
                .map(|value| value.as_ref().to_vec())
        })
        .or_else(|| {
            projection
                .get::<PdfString<'_>>(b"Units")
                .map(|s| s.as_bytes().to_vec())
        });
    if units.as_deref().is_some_and(|units| units != b"M") {
        return Err("Legacy GeoPDF units other than M are not supported".to_string());
    }
    let scale = source_to_drawing_units(drawing_crs, drawing_unit_mm);
    if lgi.contains_key(b"CTM") {
        // A present but malformed CTM must never fall through to Registration.
        // A geographic → projected operation is generally not affine, so it
        // cannot be represented by transforming these six coefficients.
        if kind == GcsKind::Geographic && !drawing_crs.is_geographic() {
            return Err("A geographic legacy CTM cannot place a projected drawing".to_string());
        }
        let ctm = required_numbers(lgi, b"CTM", b"CTM")?;
        if ctm.len() != 6 {
            return Err("Legacy GeoPDF CTM needs six numbers".to_string());
        }
        let coefficients: Vec<f64> = ctm.into_iter().map(|value| value * scale).collect();
        return Ok(Affine::new(
            coefficients
                .try_into()
                .map_err(|_| "Legacy GeoPDF CTM needs six numbers".to_string())?,
        ));
    }
    let registration = lgi
        .get::<Array<'_>>(b"Registration")
        .ok_or_else(|| "Legacy Registration is missing or not an array".to_string())?;
    let rows = registration_rows(&registration)?;
    if rows.len() < 3 {
        return Err("Legacy Registration needs at least three quadruples".to_string());
    }
    let mut points = Vec::with_capacity(rows.len());
    for values in rows {
        let (x, y) = if kind == GcsKind::Geographic {
            drawing_crs.from_base_geographic(values[2], values[3])?
        } else {
            (values[2], values[3])
        };
        points.push(([values[0], values[1]], [x * scale, y * scale]));
    }
    fit_affine(&points)
}

fn registration_rows(array: &Array<'_>) -> Result<Vec<Vec<f64>>, String> {
    let count = array.raw_iter().count();
    let entries: Vec<Object<'_>> = array.iter::<Object<'_>>().collect();
    if entries.len() != count {
        return Err("Legacy Registration contains an unresolved entry".to_string());
    }
    if entries
        .first()
        .is_some_and(|entry| matches!(entry, Object::Array(_)))
    {
        return entries
            .into_iter()
            .map(|entry| {
                let Object::Array(row) = entry else {
                    return Err("Legacy Registration mixes nested arrays and numbers".to_string());
                };
                let row = numbers(&row, b"Registration")?;
                if row.len() != 4 {
                    return Err("Each legacy Registration entry needs four numbers".to_string());
                }
                Ok(row)
            })
            .collect();
    }
    let flat = numbers(array, b"Registration")?;
    if flat.len() % 4 != 0 {
        return Err("Legacy Registration needs complete quadruples".to_string());
    }
    Ok(flat
        .as_chunks::<4>()
        .0
        .iter()
        .map(|values| values.to_vec())
        .collect())
}

fn parse_gcs(gcs: &Dict<'_>) -> Result<(CrsDefinition, GcsKind), String> {
    let kind = match name(gcs, b"Type").as_deref() {
        Some(b"GEOGCS") => GcsKind::Geographic,
        Some(b"PROJCS") => GcsKind::Projected,
        _ => return Err("The GCS type must be GEOGCS or PROJCS".to_string()),
    };
    let crs = parse_crs_dictionary(gcs)?;
    if (kind == GcsKind::Geographic) != crs.is_geographic() {
        return Err("The GCS type and coordinate operation disagree".to_string());
    }
    Ok((crs, kind))
}

fn parse_crs_dictionary(dictionary: &Dict<'_>) -> Result<CrsDefinition, String> {
    let epsg = optional_integer(dictionary, b"EPSG")?;
    let wkt = dictionary.get::<PdfString<'_>>(b"WKT");
    if epsg.is_none() && wkt.is_none() {
        return Err("The coordinate system has neither EPSG nor WKT".to_string());
    }
    let coded = epsg
        .map(|code| {
            u16::try_from(code)
                .ok()
                .filter(|code| *code > 0)
                .ok_or_else(|| "The EPSG code is outside the supported registry".to_string())
                .and_then(CrsDefinition::from_epsg)
        })
        .transpose()?;
    let authored = wkt
        .as_ref()
        .map(|wkt| crate::scene::model::pdf_layers::text_string(wkt.as_bytes()))
        .map(|wkt| CrsDefinition::from_wkt_parameters(&wkt))
        .transpose()?;
    if let (Some(coded), Some(authored)) = (&coded, &authored) {
        if !coded.equivalent_to(authored) {
            return Err("The GeoPDF EPSG and WKT definitions conflict".to_string());
        }
    }
    authored
        .or(coded)
        .ok_or_else(|| "The coordinate system cannot be resolved".to_string())
}

fn parse_legacy_projection(projection: &Dict<'_>) -> Result<CrsDefinition, String> {
    if projection.contains_key(b"WKT") || projection.contains_key(b"EPSG") {
        return parse_crs_dictionary(projection);
    }
    let object_type = name(projection, b"Type");
    let kind = if let Some(kind) = atom(projection, b"ProjectionType") {
        if object_type.as_deref() != Some(b"Projection".as_slice()) {
            return Err("The legacy projection type must be /Projection".to_string());
        }
        kind
    } else {
        // Also accept the compact extension form used by some writers.
        object_type.ok_or_else(|| "The legacy projection has no type".to_string())?
    };
    let kind = kind.to_ascii_uppercase();
    let datum = atom(projection, b"Datum")
        .map(|value| {
            value
                .into_iter()
                .filter(|byte| !matches!(byte, b' ' | b'_' | b'-'))
                .map(|byte| byte.to_ascii_uppercase())
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| "The legacy projection has no datum".to_string())?;
    if !matches!(datum.as_slice(), b"WE" | b"WGE" | b"WGS84") {
        return Err("The legacy datum is not an explicitly supported WGS84 mapping".to_string());
    }
    match kind.as_slice() {
        b"GEOGRAPHIC" => CrsDefinition::from_epsg(4326),
        b"UT" => {
            let zone = optional_integer(projection, b"Zone")?
                .filter(|zone| (1..=60).contains(zone))
                .ok_or_else(|| "The UTM zone must be an integer from 1 to 60".to_string())?;
            let north = match atom(projection, b"Hemisphere").as_deref() {
                Some(b"N") => true,
                Some(b"S") => false,
                _ => return Err("The UTM hemisphere must be N or S".to_string()),
            };
            CrsDefinition::from_epsg(if north {
                32600 + zone as u16
            } else {
                32700 + zone as u16
            })
        }
        b"UP" => match atom(projection, b"Hemisphere").as_deref() {
            Some(b"N") => CrsDefinition::from_epsg(32661),
            Some(b"S") => CrsDefinition::from_epsg(32761),
            _ => Err("The UPS hemisphere must be N or S".to_string()),
        },
        _ => Err("The legacy projection type is not supported".to_string()),
    }
}

fn ensure_same_crs(
    pdf_crs: &CrsDefinition,
    kind: GcsKind,
    drawing_crs: &CrsDefinition,
) -> Result<(), String> {
    let equivalent = match kind {
        GcsKind::Projected => !drawing_crs.is_geographic() && pdf_crs.equivalent_to(drawing_crs),
        GcsKind::Geographic => drawing_crs
            .base_geographic_definition()
            .map(|base| pdf_crs.equivalent_to(&base))
            .unwrap_or(false),
    };
    if equivalent {
        Ok(())
    } else {
        Err("The GeoPDF and drawing CRSs are not proven equal".to_string())
    }
}

fn source_to_drawing_units(drawing_crs: &CrsDefinition, drawing_unit_mm: f64) -> f64 {
    if drawing_crs.is_geographic() {
        1.0
    } else {
        drawing_crs.to_meter() * 1000.0 / drawing_unit_mm
    }
}

fn dictionaries<'a>(dict: &Dict<'a>, key: &[u8]) -> Result<Vec<Dict<'a>>, String> {
    match dict.get::<Object<'_>>(key) {
        Some(Object::Dict(dictionary)) => Ok(vec![dictionary]),
        Some(Object::Array(array)) => {
            let count = array.raw_iter().count();
            let dictionaries: Vec<Dict<'_>> = array.iter::<Dict<'_>>().collect();
            if dictionaries.len() != count {
                return Err(format!(
                    "{} contains a non-dictionary or unresolved entry",
                    String::from_utf8_lossy(key)
                ));
            }
            Ok(dictionaries)
        }
        _ => Err(format!(
            "{} must be a dictionary or an array of dictionaries",
            String::from_utf8_lossy(key)
        )),
    }
}

fn name(dict: &Dict<'_>, key: &[u8]) -> Option<Vec<u8>> {
    dict.get::<Name<'_>>(key).map(|name| name.as_ref().to_vec())
}

fn atom(dict: &Dict<'_>, key: &[u8]) -> Option<Vec<u8>> {
    name(dict, key).or_else(|| {
        dict.get::<PdfString<'_>>(key)
            .map(|string| string.as_bytes().to_vec())
    })
}

fn optional_integer(dict: &Dict<'_>, key: &[u8]) -> Result<Option<i64>, String> {
    match dict.get::<Object<'_>>(key) {
        None => Ok(None),
        Some(Object::Number(number)) => Ok(Some(number.as_i64())),
        Some(Object::String(string)) => std::str::from_utf8(string.as_bytes())
            .ok()
            .and_then(|text| text.trim().parse::<i64>().ok())
            .map(Some)
            .ok_or_else(|| format!("{} is not an integer", String::from_utf8_lossy(key))),
        _ => Err(format!(
            "{} is not an integer",
            String::from_utf8_lossy(key)
        )),
    }
}

fn required_numbers(dict: &Dict<'_>, key: &[u8], description: &[u8]) -> Result<Vec<f64>, String> {
    let array = dict.get::<Array<'_>>(key).ok_or_else(|| {
        format!(
            "{} is missing or not an array",
            String::from_utf8_lossy(description)
        )
    })?;
    numbers(&array, description)
}

fn preferred_numbers(dict: &Dict<'_>, standard: &[u8], precise: &[u8]) -> Result<Vec<f64>, String> {
    let key = if dict.contains_key(precise) {
        precise
    } else {
        standard
    };
    required_numbers(dict, key, standard)
}

fn numbers(array: &Array<'_>, description: &[u8]) -> Result<Vec<f64>, String> {
    // Number::as_f64 is suitable for ordinary PDF graphics, but hayro's
    // tokenizer stores all digits in a u64 before converting to f64. Read the
    // original token here so long, valid registration coordinates cannot wrap.
    let entries: Vec<MaybeRef<Object<'_>>> = array.raw_iter().collect();
    let data = array.data();
    let mut cursor = 0usize;
    entries
        .into_iter()
        .map(|entry| {
            skip_array_trivia(data, &mut cursor);
            let value = match entry {
                MaybeRef::NotRef(Object::Number(_)) => {
                    let start = cursor;
                    while data.get(cursor).is_some_and(|byte| {
                        byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')
                    }) {
                        cursor += 1;
                    }
                    if start == cursor
                        || data
                            .get(cursor)
                            .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'%')
                    {
                        return Err(format!(
                            "{} contains an invalid numeric token",
                            String::from_utf8_lossy(description)
                        ));
                    }
                    parse_decimal(&data[start..cursor], description)?
                }
                MaybeRef::NotRef(Object::String(string)) => {
                    skip_raw_string(data, &mut cursor).ok_or_else(|| {
                        format!(
                            "{} contains an invalid string",
                            String::from_utf8_lossy(description)
                        )
                    })?;
                    parse_decimal(string.as_bytes(), description)?
                }
                MaybeRef::Ref(_) => {
                    return Err(format!(
                        "{} contains an indirect numeric entry",
                        String::from_utf8_lossy(description)
                    ))
                }
                _ => {
                    return Err(format!(
                        "{} contains a non-numeric entry",
                        String::from_utf8_lossy(description)
                    ))
                }
            };
            if value.is_finite() {
                Ok(value)
            } else {
                Err(format!(
                    "{} contains a non-finite number",
                    String::from_utf8_lossy(description)
                ))
            }
        })
        .collect()
}

fn parse_decimal(bytes: &[u8], description: &[u8]) -> Result<f64, String> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.trim().parse::<f64>().ok())
        .ok_or_else(|| {
            format!(
                "{} contains a non-numeric value",
                String::from_utf8_lossy(description)
            )
        })
}

fn skip_array_trivia(data: &[u8], cursor: &mut usize) {
    loop {
        while data
            .get(*cursor)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            *cursor += 1;
        }
        if data.get(*cursor) != Some(&b'%') {
            break;
        }
        while data
            .get(*cursor)
            .is_some_and(|byte| !matches!(byte, b'\r' | b'\n'))
        {
            *cursor += 1;
        }
    }
}

fn skip_raw_string(data: &[u8], cursor: &mut usize) -> Option<()> {
    match data.get(*cursor)? {
        b'<' if data.get(*cursor + 1) != Some(&b'<') => {
            *cursor += 1;
            while data.get(*cursor).is_some_and(|byte| *byte != b'>') {
                *cursor += 1;
            }
            *cursor += 1;
            Some(())
        }
        b'(' => {
            *cursor += 1;
            let mut depth = 1usize;
            while let Some(byte) = data.get(*cursor) {
                match byte {
                    b'\\' => *cursor += 2,
                    b'(' => {
                        depth += 1;
                        *cursor += 1;
                    }
                    b')' => {
                        depth -= 1;
                        *cursor += 1;
                        if depth == 0 {
                            return Some(());
                        }
                    }
                    _ => *cursor += 1,
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
