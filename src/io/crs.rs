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
            let code = mapguide_xml_epsg_code(input)
                .ok_or_else(|| "The GEODATA XML has no EPSG CoordinateSystem alias".to_string())?;
            return Self::from_epsg(code);
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

    /// Preserve the authored operation in WKT while putting the standard
    /// MapGuide XML expected by GEODATA v2 into `coordinate_system_definition`
    /// when an EPSG authority is known. `from_geodata` reads the XML alias.
    fn geodata_definition(&self) -> String {
        match self.code {
            Some(code) => format_geodata_xml(
                code,
                self.name(),
                self.is_geographic(),
                &self.axis(),
                self.units(),
            ),
            None => self
                .wkt
                .clone()
                .expect("from_parts requires WKT metadata for a non-EPSG definition"),
        }
    }

    /// Replace only the CRS fields. Design/reference points, north/up vectors,
    /// unit scales, meshes, and all other GEODATA state remain untouched.
    pub fn apply_to_geodata(&self, geo: &mut GeoData) -> Result<(), String> {
        let wkt = self
            .wkt
            .as_ref()
            .ok_or_else(|| "GEODATA requires an EPSG or WKT coordinate system".to_string())?;
        geo.coordinate_type = if self.is_geographic() { 3 } else { 2 };
        // Version 1 persists its separate WKT field; versions 2/3 persist the
        // XML/id definition instead. Keep both views consistent in memory.
        geo.coordinate_system_definition = self.geodata_definition();
        geo.coordinate_system_wkt = wkt.clone();
        Ok(())
    }

    /// Valid initial GEODATA fields for a newly created document CRS. A
    /// unitless drawing still uses unit scale 1 rather than invalid zeros.
    pub fn apply_to_geodata_for_new(
        &self,
        geo: &mut GeoData,
        insertion_units: i16,
    ) -> Result<(), String> {
        self.apply_to_geodata(geo)?;
        let valid = crate::app::properties::insunits_to_mm(insertion_units).is_some()
            && (1..=24).contains(&insertion_units);
        geo.horizontal_unit_scale = crate::app::properties::insunits_to_mm(insertion_units)
            .map(|millimetres| millimetres / 1_000.0)
            .unwrap_or(1.0);
        geo.vertical_unit_scale = geo.horizontal_unit_scale;
        geo.horizontal_units = if valid { i32::from(insertion_units) } else { 0 };
        geo.vertical_units = geo.horizontal_units;
        geo.north_direction = document::types::Vector2::new(0.0, 1.0);
        geo.up_direction = document::types::Vector3::UNIT_Z;
        geo.scale_estimation_method = 1;
        geo.user_scale_factor = 1.0;
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
    // A deleted GEODATA can leave a stale entry behind (undo or an external
    // writer); skip it instead of letting it shadow a re-created object.
    dictionary
        .entries
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(GEODATA_DICTIONARY_KEY))
        .map(|(_, handle)| *handle)
        .find(|handle| matches!(document.objects.get(handle), Some(ObjectType::GeoData(_))))
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
    let insertion_units = document.header.insertion_units;
    let mut geo = GeoData::default();
    crs.apply_to_geodata_for_new(&mut geo, insertion_units)?;
    let dictionary = document.ensure_extension_dictionary(model_space);
    let handle = document.allocate_handle();
    geo.handle = handle;
    geo.owner = dictionary;
    geo.reactors = vec![dictionary];
    geo.version = 3;
    geo.host_block = model_space;
    document.objects.insert(handle, ObjectType::GeoData(geo));
    // Replace a stale entry in place instead of appending a shadowing
    // duplicate under the same key.
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
        dictionary.insert(GEODATA_DICTIONARY_KEY, handle);
    }
    Ok(handle)
}

fn xml_attribute(input: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}=\"");
    let lower = input.to_ascii_lowercase();
    let start = lower.find(&prefix.to_ascii_lowercase())? + prefix.len();
    let end = input[start..].find('"')?;
    Some(input[start..start + end].to_string())
}

/// Read only the documented EPSG `CoordinateSystem` alias. A malformed or
/// unrelated XML definition must not silently become an EPSG code.
fn mapguide_xml_epsg_code(input: &str) -> Option<u16> {
    let lower = input.to_ascii_lowercase();
    let alias_start = lower.find("<alias")?;
    let _ = lower.find("</alias")?;
    let close_offset = lower[alias_start..].find("</alias>")? + alias_start + "</alias>".len() - 1;
    let alias = &input[alias_start..=close_offset];
    if xml_attribute(alias, "type")?.to_ascii_lowercase() != "coordinatesystem" {
        return None;
    }
    let lower_alias = alias.to_ascii_lowercase();
    if !lower_alias.contains("<namespace>epsg code</namespace>") {
        return None;
    }
    xml_attribute(alias, "id")?.parse::<u16>().ok()
}

fn xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A minimal MapGuide XML document with exactly what readers such as ezdxf
/// query: the coordinate-system alias carrying its EPSG code, and the first two
/// axes so their CAD-visualized ordering is explicit.
fn format_geodata_xml(
    code: u16,
    name: &str,
    geographic: bool,
    axes: &[u8; 3],
    unit: &str,
) -> String {
    let axis_description = |abbrev: u8| match abbrev {
        b'e' => ("East", "Easting", "East"),
        b'w' => ("West", "Westing", "West"),
        b'n' => ("North", "Northing", "North"),
        b's' => ("South", "Southing", "South"),
        _ => ("Unknown", "Unknown", "unknown"),
    };
    let (first_direction, first_name, _) = axis_description(axes[0]);
    let (second_direction, second_name, _) = axis_description(axes[1]);
    let root = if geographic {
        "GeographicCoordinateSystem"
    } else {
        "ProjectedCoordinateSystem"
    };
    let id = format!("EPSG:{code}");
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-16\" standalone=\"no\" ?>\n",
            "<Dictionary version=\"1.0\" xmlns=\"http://www.osgeo.org/mapguide/coordinatesystem\">\n",
            "<{root} id=\"{id}\">\n",
            "<Name>{name}</Name>\n",
            "<Axis uom=\"{unit}\">\n",
            "<CoordinateSystemAxis>\n<AxisOrder>1</AxisOrder>\n",
            "<AxisName>{first_name}</AxisName>\n",
            "<AxisAbbreviation>{first_abbrev}</AxisAbbreviation>\n",
            "<AxisDirection>{first_direction}</AxisDirection>\n",
            "</CoordinateSystemAxis>\n",
            "<CoordinateSystemAxis>\n<AxisOrder>2</AxisOrder>\n",
            "<AxisName>{second_name}</AxisName>\n",
            "<AxisAbbreviation>{second_abbrev}</AxisAbbreviation>\n",
            "<AxisDirection>{second_direction}</AxisDirection>\n",
            "</CoordinateSystemAxis>\n",
            "</Axis>\n",
            "</{root}>\n",
            "<Alias id=\"{code}\" type=\"CoordinateSystem\">\n",
            "<ObjectId>{id}</ObjectId>\n",
            "<Namespace>EPSG Code</Namespace>\n",
            "</Alias>\n",
            "</Dictionary>"
        ),
        root = root,
        id = xml_text(&id),
        name = xml_text(name),
        unit = xml_text(unit),
        first_name = first_name,
        first_direction = first_direction,
        first_abbrev = axes[0].to_ascii_uppercase() as char,
        second_name = second_name,
        second_direction = second_direction,
        second_abbrev = axes[1].to_ascii_uppercase() as char,
        code = code,
    )
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

/// The EPSG AUTHORITY/ID directly on the root WKT node. A nested GEOGCS
/// authority (even the common EPSG:4326 fallback) is not the root CRS code.
fn root_epsg_code(wkt: &str) -> Option<u16> {
    let compact = wkt
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let root_open = compact.find('[')?;
    let bytes = compact.as_bytes();
    let mut quoted = false;
    let mut depth = 0usize;
    let mut result = None;
    let mut index = root_open;

    while index < bytes.len() {
        match bytes[index] {
            b'"' if quoted && bytes.get(index + 1) == Some(&b'"') => index += 1,
            b'"' => quoted = !quoted,
            b'[' if !quoted => depth += 1,
            b']' if !quoted => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return result;
                }
            }
            _ if quoted || depth != 1 => {}
            _ => {
                for prefix in [r#"AUTHORITY["EPSG",""#, r#"ID["EPSG","#] {
                    let rest = &compact[index..];
                    if rest.starts_with(prefix) {
                        let digits = rest[prefix.len()..]
                            .chars()
                            .take_while(char::is_ascii_digit)
                            .collect::<String>();
                        result = digits.parse::<u16>().ok().or(result);
                        break;
                    }
                }
            }
        }
        index += 1;
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
    fn only_the_root_wkt_node_supplies_an_epsg_code() {
        let nested_only = r#"PROJCS["Custom TM",GEOGCS["WGS 84",DATUM["WGS_1984"],AUTHORITY["EPSG","4326"]],PROJECTION["Transverse_Mercator"]]"#;
        assert_eq!(root_epsg_code(nested_only), None);
        let nested_and_root = format!(
            "{},AUTHORITY[\"EPSG\",\"5514\"]]",
            nested_only
                .strip_suffix(']')
                .expect("one closing root bracket")
        );
        assert_eq!(root_epsg_code(&nested_and_root), Some(5514));
        let wkt2_root = r#"PROJCRS["Custom",GEOGCRS["S-JTSK",ID["EPSG",4156]],ID["EPSG",5514]]"#;
        assert_eq!(root_epsg_code(wkt2_root), Some(5514));
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
        let definition = geo.coordinate_system_definition.clone();
        assert!(definition.starts_with(r#"<?xml version="1.0""#));
        assert!(definition.contains(r#"<ProjectedCoordinateSystem id="EPSG:5514">"#));
        assert!(definition.contains("<AxisAbbreviation>E</AxisAbbreviation>"));
        assert!(definition.contains(r#"<Alias id="5514" type="CoordinateSystem""#));
        assert!(definition.contains("<ObjectId>EPSG:5514</ObjectId>"));
        assert!(definition.contains("<Namespace>EPSG Code</Namespace>"));
        assert_eq!(geo.coordinate_system_wkt, crs.wkt().unwrap());
        assert_eq!(geo.north_direction.x, 0.5);
        assert_eq!(
            CrsDefinition::from_geodata(&geo).unwrap().code(),
            Some(5514)
        );
        assert_eq!(
            CrsDefinition::from_text(&definition).unwrap().code(),
            Some(5514)
        );
        let unaliased = definition
            .replace(
                "<Namespace>EPSG Code</Namespace>",
                "<Namespace>Other</Namespace>",
            )
            .replace(r#"type="CoordinateSystem""#, r#"type="Datum""#);
    }

    #[test]
    fn geodata_xml_escapes_object_ids_and_names() {
        let xml = format_geodata_xml(5514, "Odd <CRS> & Name", false, b"enu", "m");
        assert!(xml.contains("<Name>Odd &lt;CRS&gt; &amp; Name</Name>"));
        assert!(xml.contains("<ObjectId>EPSG:5514</ObjectId>"));
        mapguide_xml_epsg_code(&xml).expect("aliased escaped XML");
    }

    #[test]
    fn a_new_geodata_has_usable_direction_and_units() {
        let crs = CrsDefinition::from_epsg(5514).unwrap();
        for (code, unit_code, scale) in [(4, 4, 0.001), (6, 6, 1.0), (0, 0, 1.0)] {
            let mut geo = GeoData::default();
            crs.apply_to_geodata_for_new(&mut geo, code).unwrap();
            assert_eq!(geo.north_direction, document::types::Vector2::new(0.0, 1.0));
            assert_eq!(geo.up_direction, document::types::Vector3::UNIT_Z);
            assert_eq!(geo.horizontal_units, unit_code, "code {code}");
            assert_eq!(geo.vertical_units, unit_code, "code {code}");
            assert!(
                (geo.horizontal_unit_scale - scale).abs() < 1e-14,
                "code {code}"
            );
            assert!(
                (geo.vertical_unit_scale - scale).abs() < 1e-14,
                "code {code}"
            );
            assert_eq!(geo.scale_estimation_method, 1);
            assert!((geo.user_scale_factor - 1.0).abs() < 1e-14);
        }
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

    #[test]
    fn a_deleted_geodata_does_not_shadow_a_recreated_crs() {
        let mut document = document::Document::new();
        let crs = CrsDefinition::from_epsg(5514).unwrap();
        let first = set_document_crs(&mut document, &crs).unwrap();

        // Undo-style removal: the object goes away while the extension
        // dictionary keeps pointing at it.
        assert!(document.objects.remove(&first).is_some());
        assert_eq!(model_space_geo_data_handle(&document), None);

        // Recreating the CRS must replace the stale entry instead of letting
        // it shadow the new GEODATA under the same dictionary key.
        let second = set_document_crs(&mut document, &crs).unwrap();
        assert_eq!(model_space_geo_data_handle(&document), Some(second));
        assert_eq!(document_crs(&document).unwrap().unwrap().code(), Some(5514));
        let model_space = document
            .block_records
            .get("*Model_Space")
            .expect("model space")
            .handle;
        let dictionary = document
            .extension_dictionary_handle(model_space)
            .expect("extension dictionary");
        let Some(document::objects::ObjectType::Dictionary(dictionary)) =
            document.objects.get(&dictionary)
        else {
            unreachable!("the extension dictionary is a dictionary object")
        };
        assert_eq!(
            dictionary
                .entries
                .iter()
                .filter(|(key, _)| key.eq_ignore_ascii_case(GEODATA_DICTIONARY_KEY))
                .count(),
            1,
            "one GEODATA dictionary entry"
        );
    }
}
