//! Coordinate-reference-system resolution for drawing GEODATA.
//!
//! The drawing-to-CRS affine is deliberately not inferred here. This module
//! only identifies the CRS and converts between projected coordinates and the
//! geographic CRS on which that projection is based. Angular values cross the
//! public API in degrees; proj4rs uses radians internally.

use document::objects::{GeoData, ObjectType};
use document::{Document, Handle};
use proj4rs::Proj;

pub const GEODATA_DICTIONARY_KEY: &str = "ACAD_GEOGRAPHICDATA";

#[derive(Clone, Debug)]
pub struct CrsDefinition {
    code: Option<u16>,
    name: String,
    proj4: String,
    wkt: Option<String>,
    proj: Proj,
}

impl CrsDefinition {
    pub fn from_epsg(code: u16) -> Result<Self, String> {
        let definition = crs_definitions::from_code(code)
            .ok_or_else(|| format!("EPSG:{code} is not in the embedded CRS registry"))?;
        Self::from_parts(
            Some(code),
            definition.proj4.to_string(),
            Some(definition.wkt.to_string()),
        )
    }

    pub fn from_wkt(wkt: &str) -> Result<Self, String> {
        let wkt = wkt.trim();
        if wkt.is_empty() {
            return Err("The CRS WKT is empty".to_string());
        }
        if let Some(code) = root_epsg_code(wkt) {
            if let Some(definition) = crs_definitions::from_code(code) {
                return Self::from_parts(
                    Some(code),
                    definition.proj4.to_string(),
                    Some(wkt.to_string()),
                );
            }
        }
        let proj4 = proj4wkt::wkt_to_projstring(wkt)
            .map_err(|error| format!("Cannot parse the CRS WKT: {error}"))?;
        Self::from_parts(None, proj4, Some(wkt.to_string()))
    }

    /// Parse the operation actually authored in WKT, without replacing its
    /// parameters by a same-named EPSG registry entry. GeoPDF uses this to
    /// detect an EPSG/WKT conflict instead of trusting the authority label.
    pub(crate) fn from_wkt_parameters(wkt: &str) -> Result<Self, String> {
        let wkt = wkt.trim();
        if wkt.is_empty() {
            return Err("The CRS WKT is empty".to_string());
        }
        let proj4 = proj4wkt::wkt_to_projstring(wkt)
            .map_err(|error| format!("Cannot parse the CRS WKT: {error}"))?;
        Self::from_parts(root_epsg_code(wkt), proj4, Some(wkt.to_string()))
    }

    /// Resolve an EPSG code, WKT, or PROJ string. MapGuide XML is rejected
    /// explicitly rather than mistaken for a coordinate operation.
    pub fn from_text(input: &str) -> Result<Self, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("Enter an EPSG code, WKT, or PROJ definition".to_string());
        }
        if input.starts_with('<') {
            return Err("MapGuide XML CRS definitions are not supported yet".to_string());
        }
        if input.starts_with('+') {
            return Self::from_parts(None, input.to_string(), None);
        }
        if input.contains('[') {
            return Self::from_wkt(input);
        }
        let code = input
            .strip_prefix("EPSG:")
            .or_else(|| input.strip_prefix("epsg:"))
            .unwrap_or(input)
            .parse::<u16>()
            .map_err(|_| format!("Unrecognized CRS definition: {input}"))?;
        Self::from_epsg(code)
    }

    /// Resolve the current definition first. The obsolete WKT field is a
    /// fallback, including when the current definition is unsupported XML.
    pub fn from_geodata(geo: &GeoData) -> Result<Self, String> {
        if !geo.coordinate_system_definition.trim().is_empty() {
            match Self::from_text(&geo.coordinate_system_definition) {
                Ok(crs) => return Ok(crs),
                Err(definition_error) => {
                    if geo.coordinate_system_wkt.trim().is_empty() {
                        return Err(definition_error);
                    }
                }
            }
        }
        if !geo.coordinate_system_wkt.trim().is_empty() {
            return Self::from_wkt(&geo.coordinate_system_wkt);
        }
        Err("The drawing GEODATA has no coordinate-system definition".to_string())
    }

    fn from_parts(code: Option<u16>, proj4: String, wkt: Option<String>) -> Result<Self, String> {
        let proj = Proj::from_proj_string(&proj4)
            .map_err(|error| format!("Cannot initialize the CRS projection: {error}"))?;
        if proj.is_geocent() {
            return Err(
                "Geocentric CRS definitions are not supported for drawing XY coordinates"
                    .to_string(),
            );
        }
        if !proj.has_forward() || !proj.has_inverse() {
            return Err("The CRS must support forward and inverse projection".to_string());
        }
        if !proj.to_meter().is_finite() || proj.to_meter() <= 0.0 {
            return Err("The CRS has an invalid linear-unit scale".to_string());
        }
        let name = wkt
            .as_deref()
            .and_then(crs_name)
            .or_else(|| code.map(|code| format!("EPSG:{code}")))
            .unwrap_or_else(|| proj.projname().to_string());
        Ok(Self {
            code,
            name,
            proj4,
            wkt,
            proj,
        })
    }

    /// Replace only the CRS fields. Design/reference points, north/up vectors,
    /// unit scales, meshes, and all other GEODATA state remain untouched.
    pub fn apply_to_geodata(&self, geo: &mut GeoData) -> Result<(), String> {
        let wkt = self
            .wkt
            .as_ref()
            .ok_or_else(|| "GEODATA requires an EPSG or WKT coordinate system".to_string())?;
        geo.coordinate_type = if self.is_geographic() { 3 } else { 2 };
        // Version 1 writes the WKT field; versions 2/3 serialize only the
        // definition field. Keep both views consistent in memory as well.
        geo.coordinate_system_definition = wkt.clone();
        geo.coordinate_system_wkt = wkt.clone();
        Ok(())
    }

    pub fn code(&self) -> Option<u16> {
        self.code
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn proj4(&self) -> &str {
        &self.proj4
    }

    pub fn wkt(&self) -> Option<&str> {
        self.wkt.as_deref()
    }

    pub fn is_geographic(&self) -> bool {
        self.proj.is_latlong()
    }

    /// Whether two definitions are proven to describe the same coordinate
    /// operation. Names alone never establish equivalence.
    pub fn equivalent_to(&self, other: &Self) -> bool {
        if let (Some(left), Some(right)) = (self.code, other.code) {
            if left != right {
                return false;
            }
        }
        canonical_proj4(&self.proj4, &self.proj) == canonical_proj4(&other.proj4, &other.proj)
    }

    /// The geographic CRS on which this definition is based. WKT1 preserves
    /// its nested GEOGCS metadata; PROJ-only input retains its datum terms.
    pub fn base_geographic_definition(&self) -> Result<Self, String> {
        if self.is_geographic() {
            return Ok(self.clone());
        }
        if let Some(wkt) = self.wkt.as_deref() {
            if let Some(geogcs) = nested_geogcs(wkt) {
                return Self::from_wkt(geogcs);
            }
        }
        Self::from_parts(None, self.base_geographic_proj4(), None)
    }

    pub fn is_normalized_axis(&self) -> bool {
        self.proj.is_normalized_axis()
    }

    pub fn axis(&self) -> [u8; 3] {
        *self.proj.axis()
    }

    pub fn units(&self) -> &str {
        self.proj.units()
    }

    pub fn to_meter(&self) -> f64 {
        self.proj.to_meter()
    }

    /// Convert projected easting/northing to longitude/latitude in the base
    /// geographic CRS. This does not transform to WGS84.
    pub fn to_base_geographic(&self, x: f64, y: f64) -> Result<(f64, f64), String> {
        let geographic = self.base_geographic_proj()?;
        let mut point = if self.is_geographic() {
            (x.to_radians(), y.to_radians(), 0.0)
        } else {
            (x, y, 0.0)
        };
        proj4rs::transform::transform(&self.proj, &geographic, &mut point)
            .map_err(|error| format!("Cannot inverse-project the drawing coordinate: {error}"))?;
        let (lon, lat) = (point.0.to_degrees(), point.1.to_degrees());
        finite_pair(lon, lat)
    }

    /// Convert longitude/latitude in the base geographic CRS to projected
    /// easting/northing. Inputs are degrees, not WGS84-normalised coordinates.
    pub fn from_base_geographic(&self, lon: f64, lat: f64) -> Result<(f64, f64), String> {
        if !lon.is_finite() || !lat.is_finite() {
            return Err("The geographic coordinate is not finite".to_string());
        }
        let geographic = self.base_geographic_proj()?;
        let mut point = (lon.to_radians(), lat.to_radians(), 0.0);
        proj4rs::transform::transform(&geographic, &self.proj, &mut point)
            .map_err(|error| format!("Cannot project the geographic coordinate: {error}"))?;
        if self.is_geographic() {
            finite_pair(point.0.to_degrees(), point.1.to_degrees())
        } else {
            finite_pair(point.0, point.1)
        }
    }

    fn base_geographic_proj4(&self) -> String {
        const DATUM_KEYS: &[&str] = &[
            "+ellps=",
            "+datum=",
            "+towgs84=",
            "+nadgrids=",
            "+R=",
            "+a=",
            "+b=",
            "+f=",
            "+rf=",
            "+pm=",
        ];
        let mut definition = vec!["+proj=longlat".to_string()];
        definition.extend(
            self.proj4
                .split_whitespace()
                .filter(|term| DATUM_KEYS.iter().any(|key| term.starts_with(key)))
                .map(str::to_string),
        );
        definition.push("+no_defs".to_string());
        definition.join(" ")
    }

    /// Build a geographic CRS with identical ellipsoid/datum parameters. Using
    /// the same datum definition makes proj4rs skip its WGS84-pivot transform.
    fn base_geographic_proj(&self) -> Result<Proj, String> {
        Proj::from_proj_string(&self.base_geographic_proj4())
            .map_err(|error| format!("Cannot initialize the CRS base geographic system: {error}"))
    }
}

fn canonical_proj4(definition: &str, proj: &Proj) -> Vec<String> {
    const ELLIPSOID_KEYS: &[&str] = &["+ellps=", "+R=", "+a=", "+b=", "+f=", "+rf="];
    let mut terms: Vec<String> = definition
        .split_whitespace()
        .filter(|term| {
            !matches!(*term, "+no_defs" | "+wktext" | "+type=crs")
                && !ELLIPSOID_KEYS.iter().any(|key| term.starts_with(key))
        })
        .map(|term| {
            let Some((key, value)) = term.split_once('=') else {
                return term.to_string();
            };
            let normalized = value
                .split(',')
                .map(|part| {
                    part.parse::<f64>().map_or_else(
                        |_| part.to_string(),
                        |number| {
                            if number == 0.0 {
                                "0".to_string()
                            } else {
                                format!("{number:?}")
                            }
                        },
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{key}={normalized}")
        })
        .collect();
    let (a, b) = proj.ellipse_parameters();
    terms.push(format!("+resolved_a={a:?}"));
    terms.push(format!("+resolved_b={b:?}"));
    terms.sort();
    terms
}

fn nested_geogcs(wkt: &str) -> Option<&str> {
    let uppercase = wkt.to_ascii_uppercase();
    let start = uppercase.find("GEOGCS[")?;
    let bytes = wkt.as_bytes();
    let mut quoted = false;
    let mut depth = 0usize;
    let mut index = start + "GEOGCS".len();
    while index < bytes.len() {
        match bytes[index] {
            b'"' if quoted && bytes.get(index + 1) == Some(&b'"') => index += 1,
            b'"' => quoted = !quoted,
            b'[' if !quoted => depth += 1,
            b']' if !quoted => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&wkt[start..=index]);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// The GEODATA object owned by the root model-space block record. Xref and
/// paper-space objects are deliberately not candidates for the document CRS.
pub fn model_space_geo_data_handle(document: &Document) -> Option<Handle> {
    let model_space = document.block_records.get("*Model_Space")?.handle;
    let dictionary = document.extension_dictionary_handle(model_space)?;
    let ObjectType::Dictionary(dictionary) = document.objects.get(&dictionary)? else {
        return None;
    };
    dictionary
        .entries
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(GEODATA_DICTIONARY_KEY))
        .map(|(_, handle)| *handle)
        .filter(|handle| matches!(document.objects.get(handle), Some(ObjectType::GeoData(_))))
}

pub fn model_space_geo_data(document: &Document) -> Option<&GeoData> {
    match document
        .objects
        .get(&model_space_geo_data_handle(document)?)?
    {
        ObjectType::GeoData(geo) => Some(geo),
        _ => None,
    }
}

pub fn document_crs(document: &Document) -> Result<Option<CrsDefinition>, String> {
    model_space_geo_data(document)
        .map(CrsDefinition::from_geodata)
        .transpose()
}

/// Create or update the root drawing's GEODATA while preserving every non-CRS
/// field. The extension-dictionary linkage is the native DWG/DXF owner graph;
/// inserting an unowned object would not declare a document CRS.
pub fn set_document_crs(document: &mut Document, crs: &CrsDefinition) -> Result<Handle, String> {
    let model_space = document
        .block_records
        .get("*Model_Space")
        .map(|record| record.handle)
        .ok_or_else(|| "The drawing has no model-space block record".to_string())?;
    if let Some(handle) = model_space_geo_data_handle(document) {
        let Some(ObjectType::GeoData(geo)) = document.objects.get_mut(&handle) else {
            return Err("The model-space GEODATA object is missing".to_string());
        };
        crs.apply_to_geodata(geo)?;
        return Ok(handle);
    }

    // Validate before allocating handles or changing the owner graph.
    let mut geo = GeoData::default();
    crs.apply_to_geodata(&mut geo)?;
    let dictionary = document.ensure_extension_dictionary(model_space);
    let handle = document.allocate_handle();
    geo.handle = handle;
    geo.owner = dictionary;
    geo.reactors = vec![dictionary];
    geo.version = 3;
    geo.host_block = model_space;
    document.objects.insert(handle, ObjectType::GeoData(geo));
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
        dictionary.add_entry(GEODATA_DICTIONARY_KEY, handle);
    }
    Ok(handle)
}

fn finite_pair(x: f64, y: f64) -> Result<(f64, f64), String> {
    if x.is_finite() && y.is_finite() {
        Ok((x, y))
    } else {
        Err("The coordinate transformation produced a non-finite result".to_string())
    }
}

fn crs_name(wkt: &str) -> Option<String> {
    let open = wkt.find('[')?;
    let rest = wkt[open + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].trim().to_string()).filter(|name| !name.is_empty())
}

/// The final root AUTHORITY/ID in WKT1/WKT2, when it names an EPSG code.
fn root_epsg_code(wkt: &str) -> Option<u16> {
    let compact = wkt
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let mut result = None;
    for prefix in ["AUTHORITY[\"EPSG\",\"", "ID[\"EPSG\","] {
        let mut offset = 0;
        while let Some(found) = compact[offset..].find(prefix) {
            let start = offset + found + prefix.len();
            let digits = compact[start..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            result = digits.parse::<u16>().ok().or(result);
            offset = start;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} differs from {expected} by more than {tolerance}"
        );
    }

    #[test]
    fn epsg_5514_has_canonical_geodata_metadata() {
        let crs = CrsDefinition::from_text("EPSG:5514").unwrap();
        assert_eq!(crs.code(), Some(5514));
        assert_eq!(crs.name(), "S-JTSK / Krovak East North");
        assert!(!crs.is_geographic());
        assert!(crs.is_normalized_axis());
        assert_eq!(crs.axis(), *b"enu");
        assert_eq!(crs.units(), "m");
        close(crs.to_meter(), 1.0, 1e-12);
        assert!(crs.wkt().unwrap().contains("AUTHORITY[\"EPSG\",\"5514\"]"));

        let mut geo = GeoData::default();
        geo.coordinate_system_definition = "stale XML".to_string();
        geo.north_direction.x = 0.5;
        crs.apply_to_geodata(&mut geo).unwrap();
        assert_eq!(geo.coordinate_type, 2);
        assert_eq!(geo.coordinate_system_definition, crs.wkt().unwrap());
        assert_eq!(geo.coordinate_system_wkt, crs.wkt().unwrap());
        assert_eq!(geo.north_direction.x, 0.5);
        assert_eq!(
            CrsDefinition::from_geodata(&geo).unwrap().code(),
            Some(5514)
        );
    }

    #[test]
    fn wkt_resolves_to_the_same_epsg_definition() {
        let expected = CrsDefinition::from_epsg(5514).unwrap();
        let custom = expected
            .wkt()
            .unwrap()
            .replace("S-JTSK / Krovak East North", "Local S-JTSK");
        let parsed = CrsDefinition::from_wkt(&custom).unwrap();
        assert_eq!(parsed.code(), Some(5514));
        assert_eq!(parsed.proj4(), expected.proj4());
        assert_eq!(parsed.wkt(), Some(custom.as_str()));
    }

    #[test]
    fn krovak_projection_matches_proj_reference_without_wgs84_pivot() {
        let crs = CrsDefinition::from_epsg(5514).unwrap();
        // PROJ 9.8.1, EPSG:4156 -> EPSG:5514, always_xy.
        let (x, y) = crs.from_base_geographic(14.42076, 50.08804).unwrap();
        close(x, -742_925.271_389_167_2, 1e-6);
        close(y, -1_043_020.310_123_933_5, 1e-6);

        let (lon, lat) = crs.to_base_geographic(x, y).unwrap();
        close(lon, 14.42076, 1e-9);
        close(lat, 50.08804, 1e-9);
    }

    #[test]
    fn krovak_origin_and_negative_coordinates_round_trip() {
        let crs = CrsDefinition::from_epsg(5514).unwrap();
        let (x, y) = crs
            .from_base_geographic(24.833_333_333_333_3, 49.5)
            .unwrap();
        close(x, 0.0, 1e-6);
        close(y, -1_159_048.159_527_503_2, 1e-6);
        let (lon, lat) = crs.to_base_geographic(x, y).unwrap();
        close(lon, 24.833_333_333_333_3, 1e-9);
        close(lat, 49.5, 1e-9);
    }

    #[test]
    fn geographic_crs_uses_degrees_at_the_public_boundary() {
        let crs = CrsDefinition::from_epsg(4326).unwrap();
        assert!(crs.is_geographic());
        let (lon, lat) = crs.from_base_geographic(14.5, 50.25).unwrap();
        close(lon, 14.5, 1e-12);
        close(lat, 50.25, 1e-12);
        let (lon, lat) = crs.to_base_geographic(lon, lat).unwrap();
        close(lon, 14.5, 1e-12);
        close(lat, 50.25, 1e-12);
    }

    #[test]
    fn base_geographic_identity_and_conservative_equivalence() {
        let projected = CrsDefinition::from_epsg(5514).unwrap();
        let geographic = projected.base_geographic_definition().unwrap();
        assert_eq!(geographic.code(), Some(4156));
        assert!(geographic.is_geographic());
        assert!(geographic.equivalent_to(&CrsDefinition::from_epsg(4156).unwrap()));
        assert!(!projected.equivalent_to(&CrsDefinition::from_epsg(4326).unwrap()));
        assert!(CrsDefinition::from_text(
            "+proj=krovak +lat_0=49.5 +lon_0=24.83333333333333 +alpha=30.28813975277778 +k=0.9999 +x_0=0 +y_0=0 +ellps=bessel +units=m +no_defs"
        )
        .unwrap()
        .equivalent_to(&CrsDefinition::from_text(
            "+type=crs +y_0=0.0 +x_0=0 +k=0.999900 +alpha=30.28813975277778 +lon_0=24.83333333333333 +lat_0=49.5 +proj=krovak +ellps=bessel +units=m"
        )
        .unwrap()));
        assert!(!projected.equivalent_to(&CrsDefinition::from_epsg(3857).unwrap()));
    }

    #[test]
    fn invalid_or_unsupported_definitions_fail_explicitly() {
        assert!(CrsDefinition::from_text("").is_err());
        assert!(CrsDefinition::from_text("not-a-crs").is_err());
        assert!(CrsDefinition::from_text("<MapGuide />").is_err());
        assert!(CrsDefinition::from_epsg(1).is_err());
        assert!(CrsDefinition::from_epsg(5514)
            .unwrap()
            .apply_to_geodata(&mut GeoData::default())
            .is_ok());
        assert!(CrsDefinition::from_text("+proj=longlat +datum=WGS84")
            .unwrap()
            .apply_to_geodata(&mut GeoData::default())
            .is_err());
    }
}
