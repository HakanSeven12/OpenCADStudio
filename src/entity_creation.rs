//! Discoverable creation contracts, shared by MCP preflight and the entity builder.
use serde_json::{json, Value};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_examples_build_with_the_real_factory() {
        let document = document::Document::new();
        for kind in TYPES {
            let contract = schema(kind).unwrap();
            for example in contract["examples"].as_array().unwrap() {
                validate_entity(example, "/entities/0").unwrap();
                let built = build_entity(example, Some(&document));
                assert!(built.is_ok(), "{kind}: {built:?}");
            }
        }
        for name in ["line", "LINE", "polyline", "Polyline", "LWPOLYLINE"] {
            let mut example = schema(name).unwrap()["examples"][0].clone();
            example["type"] = json!(name);
            validate_entity(&example, "").unwrap();
            assert!(build_entity(&example, Some(&document)).is_ok());
        }
        // JSON Schema integers include numerically integral float notation.
        let circle = json!({"type":"Circle","center":[0,0],"radius":2,"color":2.0});
        validate_entity(&circle, "").unwrap();
        assert_eq!(
            build_entity(&circle, Some(&document))
                .unwrap()
                .common()
                .color,
            document::types::Color::from_index(2)
        );
        let spline = json!({"type":"Spline","degree":2.0,"control_points":[[0,0],[5,10],[10,0]]});
        validate_entity(&spline, "").unwrap();
        let document::EntityType::Spline(built) = build_entity(&spline, Some(&document)).unwrap()
        else {
            panic!("expected spline")
        };
        assert_eq!(built.degree, 2);
    }

    /// ATTDEF, REGION and the lineweight, linetype and justification that
    /// MCP callers set at creation instead of patching afterwards.
    #[test]
    fn builds_attdef_region_and_creation_properties() {
        let document = document::Document::new();
        let spec = json!({"type":"AttDef","tag":"circuit","prompt":"Circuit","default":"7",
            "position":[10,20],"height":3,"justify":"MC","invisible":true,"preset":true,
            "lineweight":35,"layer":"TAGS"});
        validate_entity(&spec, "").unwrap();
        let built = build_entity(&spec, Some(&document)).unwrap();
        assert_eq!(built.common().line_weight.value(), 35);
        assert_eq!(built.common().layer, "TAGS");
        let document::EntityType::AttributeDefinition(definition) = built else {
            panic!("expected an attribute definition")
        };
        assert_eq!(definition.tag, "CIRCUIT");
        assert_eq!((definition.prompt.as_str(), definition.default_value.as_str()), ("Circuit", "7"));
        assert!(definition.flags.invisible && definition.flags.preset && !definition.flags.constant);
        assert_eq!((definition.alignment_point.x, definition.alignment_point.y), (10.0, 20.0));
        assert!(definition.insertion_point.x < 10.0, "start sits left of the centre");

        let region = json!({"type":"Region","loops":[[[0,0],[10,0],[10,10],[0,10]],[[3,3],[7,3],[7,7],[3,7]]]});
        let document::EntityType::Region(region) = build_entity(&region, Some(&document)).unwrap()
        else {
            panic!("expected a region")
        };
        assert!(region.acis_data.has_data());
        assert_eq!(region.wires.len(), 8, "outer square and hole edges");
        let crossing = json!({"type":"Region","loops":[[[0,0],[10,10],[10,0],[0,10]]]});
        assert!(build_entity(&crossing, None).is_err());

        let text = json!({"type":"Text","value":"A","position":[5,5],"justify":"TR"});
        let document::EntityType::Text(text) = build_entity(&text, Some(&document)).unwrap() else {
            panic!("expected text")
        };
        assert_eq!(text.alignment_point.map(|p| (p.x, p.y)), Some((5.0, 5.0)));
        let fit = json!({"type":"Text","value":"A","position":[0,0],"justify":"F"});
        assert_eq!(build_entity(&fit, None).unwrap_err()["code"], "second_point_required");

        let dashed = json!({"type":"Line","start":[0,0],"end":[1,0],"linetype":"NOSUCH"});
        assert_eq!(build_entity(&dashed, Some(&document)).unwrap_err()["code"], "invalid_linetype");
        let heavy = json!({"type":"Line","start":[0,0],"end":[1,0],"lineweight":33});
        assert!(validate_entity(&heavy, "").is_err());
    }

    #[test]
    fn rejects_bad_nested_fields_and_matches_geometry_constraints() {
        let cases = [
            (
                json!({"type":"Line","start":[0,0],"end":[1,"bad"]}),
                "/end/1",
            ),
            (json!({"type":"Circle","center":[0,0]}), "/radius"),
            (
                json!({"type":"Circle","center":[0,0],"radius":-1}),
                "/radius",
            ),
            (
                json!({"type":"Solid","corners":[[0,0],[1,0],[0,1]]}),
                "/corners/0",
            ),
            (
                json!({"type":"Point","location":[0,0],"color":32768}),
                "/color",
            ),
            (
                json!({"type":"Line","start":[0,0],"end":[1,1],"unsupported":true}),
                "/unsupported",
            ),
        ];
        for (spec, path) in cases {
            let error = validate_entity(&spec, "/entities/0").unwrap_err();
            assert!(error.contains(&format!("/entities/0{path}")), "{error}");
            assert!(build_entity(&spec, None).is_err());
        }
        for spec in [
            json!({"type":"Arc","center":[0,0],"radius":5,"start_angle_deg":45,"end_angle_deg":45}),
            json!({"type":"Spline","degree":3,"control_points":[[0,0],[1,1]]}),
            json!({"type":"Spline","degree":1,"control_points":[[0,0],[1,1]],"weights":[1]}),
            json!({"type":"Insert","block":"Example","position":[0,0],"scale":0}),
        ] {
            assert!(validate_entity(&spec, "").is_err());
            assert!(build_entity(&spec, None).is_err());
        }
        // The static path deliberately omits document-dependent style lookup.
        let spec = json!({"type":"Text","value":"Label","position":[0,0],"style":"missing_style"});
        assert!(validate_entity(&spec, "").is_ok());
        assert_eq!(
            build_entity(&spec, Some(&document::Document::new())).unwrap_err()["code"],
            "invalid_text_style"
        );
    }
}

pub const TYPES: &[&str] = &[
    "Line",
    "Circle",
    "Arc",
    "Spline",
    "LwPolyline",
    "Point",
    "Text",
    "MText",
    "Insert",
    "Solid",
    "Hatch",
    "AttDef",
    "Region",
];

/// Lineweights a drawing can store, in hundredths of a millimetre, with
/// ByLayer (-1), ByBlock (-2) and Default (-3).
const LINEWEIGHTS: [i16; 27] = [
    -3, -2, -1, 0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120,
    140, 158, 200, 211,
];

/// Justification keywords as TEXT and ATTDEF take them.
const JUSTIFY: [&str; 15] = [
    "L", "A", "F", "C", "M", "R", "TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR",
];

fn point(min: usize, max: usize) -> Value {
    json!({"type":"array","items":{"type":"number"},"minItems":min,"maxItems":max})
}

/// Contracts are built once and shared by discovery, preflight and execution.
pub fn schema(name: &str) -> Result<Value, String> {
    use std::sync::OnceLock;
    static CONTRACTS: OnceLock<Vec<Value>> = OnceLock::new();
    let name = if name.eq_ignore_ascii_case("Polyline") {
        "LwPolyline"
    } else {
        name
    };
    let index = TYPES.iter().position(|kind| kind.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("/type: unknown entity type '{name}'; request entity_schema without type to list supported types"))?;
    let contracts = CONTRACTS.get_or_init(|| {
        TYPES
            .iter()
            .map(|kind| build_schema(kind).expect("known type"))
            .collect()
    });
    Ok(contracts[index].clone())
}

fn build_schema(name: &str) -> Result<Value, String> {
    let name = if name.eq_ignore_ascii_case("Polyline") {
        "LwPolyline"
    } else {
        name
    };
    let canonical = TYPES.iter().find(|t| t.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("/type: unknown entity type '{name}'; request entity_schema without type to list supported types"))?;
    let case_pattern = |name: &str| {
        name.chars()
            .map(|c| format!("[{}{}]", c.to_ascii_uppercase(), c.to_ascii_lowercase()))
            .collect::<String>()
    };
    let pattern = if *canonical == "LwPolyline" {
        format!(
            "^({}|{})$",
            case_pattern(canonical),
            case_pattern("Polyline")
        )
    } else {
        format!("^{}$", case_pattern(canonical))
    };
    let number = json!({"type":"number"});
    let string = json!({"type":"string"});
    let boolean = json!({"type":"boolean"});
    let mut properties = json!({"type":{"type":"string","pattern":pattern},"layer":{"type":"string","description":"Missing layers are created; empty uses the default layer."},"color":{"type":"integer","minimum":-32768,"maximum":32767,"description":"DXF indexed color, including ByLayer/ByBlock sentinels."},"lineweight":{"type":"integer","enum":LINEWEIGHTS,"description":"Hundredths of a millimetre (25 = 0.25 mm); -1 ByLayer, -2 ByBlock, -3 Default."},"linetype":{"type":"string","description":"Nonempty linetype names must exist in the target document; empty or ByLayer keeps ByLayer."}});
    let justify = json!({"type":"string","enum":JUSTIFY,"description":"L (default), A(lign), F(it), C, M, R, TL…BR. Except L, position is the justification point; A and F also take second_point."});
    let (required, example) = match *canonical {
        "Line" => {
            properties["start"] = point(2, 3);
            properties["end"] = point(2, 3);
            properties["thickness"] = number.clone();
            (
                vec!["start", "end"],
                json!({"type":"Line","start":[0,0],"end":[10,10]}),
            )
        }
        "Circle" | "Arc" => {
            properties["center"] = point(2, 3);
            properties["radius"] = if *canonical == "Arc" {
                json!({"type":"number","exclusiveMinimum":0})
            } else {
                json!({"type":"number","minimum":0})
            };
            if *canonical == "Arc" {
                properties["start_angle_deg"] = number.clone();
                properties["end_angle_deg"] = number.clone();
                properties["end_angle_deg"]["description"] = json!("Together with start_angle_deg, must define a finite, nonzero sweep using the CAD arc constructor.");
                (
                    vec!["center", "radius", "start_angle_deg", "end_angle_deg"],
                    json!({"type":"Arc","center":[0,0],"radius":5,"start_angle_deg":0,"end_angle_deg":90}),
                )
            } else {
                (
                    vec!["center", "radius"],
                    json!({"type":"Circle","center":[0,0],"radius":5}),
                )
            }
        }
        "Point" => {
            properties["location"] = point(2, 3);
            (vec!["location"], json!({"type":"Point","location":[0,0]}))
        }
        "LwPolyline" => {
            properties["vertices"] = json!({"type":"array","minItems":2,"items":point(2,3),"description":"XY coordinates; optional Z is ignored."});
            properties["closed"] = boolean.clone();
            properties["constant_width"] = number.clone();
            (
                vec!["vertices"],
                json!({"type":"LwPolyline","vertices":[[0,0],[10,0]],"closed":false}),
            )
        }
        "Text" | "MText" => {
            properties["value"] = json!({"type":"string","minLength":1});
            properties["position"] = point(2, 3);
            properties["height"] = json!({"type":"number","default":2.5,"description":"Absolute value, clamped to at least 0.000001."});
            properties["rotation_deg"] = number.clone();
            properties["style"] = json!({"type":"string","description":"Nonempty style names must exist in the target document."});
            if *canonical == "Text" {
                properties["justify"] = justify.clone();
                properties["second_point"] = point(2, 3);
            }
            if *canonical == "MText" {
                properties["width"] = json!({"type":"number","description":"Positive values set rectangle width; otherwise default."});
            }
            (
                vec!["value", "position"],
                json!({"type":canonical,"value":"Label","position":[0,0]}),
            )
        }
        "Insert" => {
            properties["block"] = json!({"type":"string","minLength":1});
            properties["position"] = point(2, 3);
            properties["rotation_deg"] = number.clone();
            properties["scale"] = json!({"anyOf":[{"type":"number","not":{"const":0}},{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","not":{"const":0}}}],"default":1,"description":"Nonzero uniform factor or three nonzero axis factors."});
            properties["attributes"] = json!({"type":"object","additionalProperties":{"type":"string"},"description":"Attribute values by tag (case-insensitive), e.g. {\"CIRCUIT\":\"3\"}. Every attribute the block defines is created; tags left out keep their default value. A tag the block does not define, or a constant one, is refused."});
            (
                vec!["block", "position"],
                json!({"type":"Insert","block":"DOOR_90","position":[0,0],"scale":1}),
            )
        }
        "Solid" => {
            properties["corners"] =
                json!({"type":"array","minItems":3,"maxItems":4,"items":point(3,3)});
            (
                vec!["corners"],
                json!({"type":"Solid","corners":[[0,0,0],[10,0,0],[0,10,0]]}),
            )
        }
        "Hatch" => {
            properties["boundary"] = json!({"type":"array","minItems":3,"items":point(2,3),"description":"Closed XY boundary; optional Z is ignored."});
            properties["solid"] = boolean.clone();
            properties["pattern"] = string.clone();
            properties["pattern_scale"] = json!({"type":"number","default":1,"description":"Absolute value, clamped to at least 0.000001."});
            properties["pattern_angle_deg"] = number.clone();
            (
                vec!["boundary"],
                json!({"type":"Hatch","boundary":[[0,0],[10,0],[0,10]],"solid":true}),
            )
        }
        "AttDef" => {
            properties["tag"] = json!({"type":"string","minLength":1,"pattern":"^\\S+$","description":"No spaces; stored upper case."});
            properties["prompt"] = string.clone();
            properties["default"] = json!({"type":"string","description":"Default value (the value of a constant attribute)."});
            properties["position"] = point(2, 3);
            properties["second_point"] = point(2, 3);
            properties["height"] = json!({"type":"number","default":2.5,"description":"Absolute value, clamped to at least 0.000001."});
            properties["rotation_deg"] = number.clone();
            properties["style"] = json!({"type":"string","description":"Nonempty style names must exist in the target document."});
            properties["justify"] = justify.clone();
            for key in ["invisible", "constant", "verify", "preset", "lock_position"] {
                properties[key] = boolean.clone();
            }
            (
                vec!["tag", "position"],
                json!({"type":"AttDef","tag":"CIRCUIT","prompt":"Circuit number","default":"1","position":[0,0],"height":2.5,"justify":"MC"}),
            )
        }
        "Region" => {
            properties["loops"] = json!({"type":"array","minItems":1,"items":{"type":"array","minItems":3,"items":point(2,3)},"description":"Closed coplanar boundaries as vertex lists: the first is the outer boundary, any others are holes."});
            (
                vec!["loops"],
                json!({"type":"Region","loops":[[[0,0],[10,0],[10,10],[0,10]],[[3,3],[7,3],[7,7],[3,7]]]}),
            )
        }
        "Spline" => {
            properties["degree"] = json!({"type":"integer","minimum":1,"maximum":10,"default":3});
            for key in ["control_points", "fit_points"] {
                properties[key] = json!({"type":"array","minItems":2,"items":point(2,3)});
            }
            for key in ["knots", "weights"] {
                properties[key] = json!({"type":"array","minItems":1,"items":{"type":"number"}});
            }
            properties["control_points"]["description"] =
                json!("Requires more points than degree (default degree 3).");
            properties["knots"]["description"] = json!("Requires control_points; exactly control_points count + degree + 1 non-decreasing values.");
            properties["weights"]["description"] = json!("Requires control_points; one positive weight per control point. Sets rational=true unless overridden explicitly.");
            properties["weights"]["items"]["exclusiveMinimum"] = json!(0);
            for key in ["closed", "periodic", "rational", "planar", "linear"] {
                properties[key] = boolean.clone();
            }
            (
                vec![],
                json!({"type":"Spline","degree":2,"control_points":[[0,0],[5,10],[10,0]]}),
            )
        }
        _ => unreachable!(),
    };
    let mut required = required;
    required.insert(0, "type");
    let mut result = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":properties,"required":required,"additionalProperties":false,"examples":[example],"description":"Creation input, not serialized record properties. Type names are case-insensitive; Polyline aliases LwPolyline. Angles are degrees; coordinates and lengths use drawing units. Additional geometric constraints are checked by preflight and the builder."});
    if *canonical == "Spline" {
        result["anyOf"] = json!([{"required":["control_points"]},{"required":["fit_points"]}]);
    }
    Ok(result)
}

pub fn validate_entity(spec: &Value, path: &str) -> Result<(), String> {
    let kind = spec["type"]
        .as_str()
        .ok_or_else(|| format!("{path}/type: expected entity type string"))?;
    let contract = schema(kind).map_err(|e| format!("{path}{e}"))?;
    crate::schema_validation::validate_shape(&contract, spec, path)?;
    build_entity(spec, None).map(|_| ()).map_err(|error| {
        format!(
            "{path}{}: {}",
            error["path"].as_str().unwrap_or(""),
            error["error"].as_str().unwrap_or("invalid entity")
        )
    })
}

type Parsed<T> = Result<T, Value>;

fn failure_at(code: &str, path: &str, error: impl ToString) -> Value {
    let mut result = failure(code, error);
    result["path"] = json!(path);
    result
}

fn failure(code: &str, error: impl ToString) -> Value {
    json!({"ok":false,"status":"failed","code":code,"error":error.to_string()})
}

/// Parse `[x,y]` or `[x,y,z]` without silently turning a malformed
/// coordinate into zero.
fn point_value(value: Option<&Value>, label: &str, code: &str) -> Parsed<document::types::Vector3> {
    let values = value
        .and_then(Value::as_array)
        .filter(|v| (2..=3).contains(&v.len()))
        .ok_or_else(|| failure(code, format!("{label} must be [x,y] or [x,y,z]")))?;
    let coord = |i: usize| {
        values[i]
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| failure(code, format!("{label} needs finite coordinates")))
    };
    let z = values
        .get(2)
        .map(|value| {
            value
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(|| failure(code, format!("{label} needs finite coordinates")))
        })
        .transpose()?
        .unwrap_or(0.0);
    Ok(document::types::Vector3::new(coord(0)?, coord(1)?, z))
}

/// `[x,y]` or `[x,y,z]` (missing z = 0) from `req[key]`.
pub(crate) fn point_field(req: &Value, key: &str) -> Parsed<document::types::Vector3> {
    point_value(req.get(key), key, "invalid_point")
}

fn point_list_field(
    spec: &Value,
    key: &str,
    minimum: usize,
) -> Parsed<Vec<document::types::Vector3>> {
    let points = spec[key]
        .as_array()
        .filter(|points| points.len() >= minimum)
        .ok_or_else(|| {
            failure(
                "invalid_spline_points",
                format!("{key} needs at least {minimum} points"),
            )
        })?;
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            point_value(
                Some(point),
                &format!("{key}[{index}]"),
                "invalid_spline_points",
            )
        })
        .collect()
}

fn finite_number_list_field(spec: &Value, key: &str) -> Parsed<Vec<f64>> {
    let values = spec[key]
        .as_array()
        .filter(|values| !values.is_empty())
        .ok_or_else(|| {
            failure(
                "invalid_spline_values",
                format!("{key} needs a non-empty array"),
            )
        })?;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| {
                    failure(
                        "invalid_spline_values",
                        format!("{key}[{index}] must be finite"),
                    )
                })
        })
        .collect()
}

pub(crate) fn deg_field(req: &Value, key: &str) -> f64 {
    req[key].as_f64().unwrap_or(0.0).to_radians()
}

/// Required finite angle in degrees, converted to radians without a default.
fn finite_angle_field(spec: &Value, key: &str) -> Parsed<f64> {
    let degrees = spec[key]
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| failure("invalid_angle", format!("arc needs a finite {key}")))?;
    let radians = degrees.to_radians();
    radians
        .is_finite()
        .then_some(radians)
        .ok_or_else(|| failure("invalid_angle", format!("arc needs a finite {key}")))
}

fn apply_common_properties(
    spec: &Value,
    entity: &mut document::EntityType,
    document: Option<&document::Document>,
) -> Parsed<()> {
    if let Some(layer) = spec["layer"].as_str() {
        if !layer.is_empty()
            && !matches!(
                entity,
                document::EntityType::Block(_) | document::EntityType::BlockEnd(_)
            )
        {
            entity.common_mut().layer = layer.to_owned();
        }
    }
    if let Some(index) = spec["color"]
        .as_f64()
        .filter(|n| n.fract() == 0.0 && (-32768.0..=32767.0).contains(n))
    {
        entity.common_mut().color = document::types::Color::from_index(index as i16);
    }
    if let Some(weight) = spec["lineweight"].as_f64() {
        let weight = Some(weight)
            .filter(|w| w.fract() == 0.0 && LINEWEIGHTS.contains(&(*w as i16)))
            .ok_or_else(|| {
                failure_at(
                    "invalid_lineweight",
                    "/lineweight",
                    "lineweight must be a standard value in hundredths of a millimetre, or -1/-2/-3",
                )
            })?;
        entity.common_mut().line_weight = document::types::LineWeight::from_value(weight as i16);
    }
    if let Some(linetype) = spec["linetype"].as_str().filter(|name| !name.is_empty()) {
        if !linetype.eq_ignore_ascii_case("ByLayer")
            && document.is_some_and(|document| !document.line_types.contains(linetype))
        {
            return Err(failure_at(
                "invalid_linetype",
                "/linetype",
                format!("Linetype '{linetype}' does not exist in the document"),
            ));
        }
        entity.common_mut().linetype = linetype.to_owned();
    }
    Ok(())
}

/// `justify` as the attribute-definition builder takes it; `None` is Left.
fn justify_field(spec: &Value) -> Parsed<crate::modules::draw::draw::attdef::Justify> {
    use crate::modules::draw::draw::attdef::Justify;
    match spec.get("justify") {
        None | Some(Value::Null) => Ok(Justify::Left),
        Some(value) => value.as_str().and_then(Justify::from_keyword).ok_or_else(|| {
            failure_at(
                "invalid_justify",
                "/justify",
                "justify must be one of L, A, F, C, M, R, TL, TC, TR, ML, MC, MR, BL, BC, BR",
            )
        }),
    }
}

/// Align and Fit run the baseline from `position` to `second_point`.
fn second_point_field(
    spec: &Value,
    justify: crate::modules::draw::draw::attdef::Justify,
) -> Parsed<Option<document::types::Vector3>> {
    let point = spec
        .get("second_point")
        .map(|_| point_field(spec, "second_point"))
        .transpose()?;
    if justify.two_point() && point.is_none() {
        return Err(failure_at(
            "second_point_required",
            "/second_point",
            "justify A or F needs second_point, the other end of the baseline",
        ));
    }
    Ok(point.filter(|_| justify.two_point()))
}

/// An explicit text style must exist in the current document; otherwise a
/// typo would silently fall back to the default style.
fn text_style_field(spec: &Value, document: Option<&document::Document>) -> Parsed<Option<String>> {
    let Some(style) = spec["style"].as_str().filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if document.is_some_and(|document| !document.text_styles.contains(style)) {
        return Err(failure(
            "invalid_text_style",
            format!("Text style '{style}' does not exist in the document"),
        ));
    }
    Ok(Some(style.to_owned()))
}

/// The attributes of a new Insert, built from its block's definitions the
/// way `-INSERT` builds them, with the values given by tag. Without a
/// document only the shape is checked, as for text styles; the scene then
/// fills a bare reference with the defaults.
fn insert_attributes(
    insert: &document::entities::Insert,
    values: &Value,
    document: Option<&document::Document>,
) -> Parsed<Vec<document::entities::AttributeEntity>> {
    let at = |tag: &str| format!("/attributes/{}", tag.replace('~', "~0").replace('/', "~1"));
    let object = values.as_object().ok_or_else(|| {
        failure_at(
            "invalid_attributes",
            "/attributes",
            "insert attributes map tags to text values",
        )
    })?;
    let mut given: Vec<(&str, &str)> = Vec::with_capacity(object.len());
    for (tag, value) in object {
        let value = value.as_str().ok_or_else(|| {
            failure_at("invalid_attributes", &at(tag), "attribute values are text")
        })?;
        if given.iter().any(|(seen, _)| seen.eq_ignore_ascii_case(tag)) {
            return Err(failure_at(
                "invalid_attributes",
                &at(tag),
                format!("attribute tag '{tag}' is given twice"),
            ));
        }
        given.push((tag, value));
    }
    let Some(document) = document else {
        return Ok(Vec::new());
    };
    let block = &insert.block_name;
    for (tag, _) in &given {
        let definition = document
            .entities_in_block(block)
            .find_map(|entity| match entity {
                document::EntityType::AttributeDefinition(definition)
                    if definition.tag.eq_ignore_ascii_case(tag) =>
                {
                    Some(definition)
                }
                _ => None,
            })
            .ok_or_else(|| {
                failure_at(
                    "unknown_attribute_tag",
                    &at(tag),
                    format!("block '{block}' defines no attribute tagged '{tag}'"),
                )
            })?;
        if definition.is_constant() {
            return Err(failure_at(
                "constant_attribute",
                &at(tag),
                format!("attribute '{tag}' of block '{block}' is constant"),
            ));
        }
    }
    Ok(crate::scene::Scene::attributes_from_definitions(
        document,
        insert,
        |definition| {
            given
                .iter()
                .find(|(tag, _)| tag.eq_ignore_ascii_case(&definition.tag))
                .map(|(_, value)| (*value).to_owned())
        },
    ))
}

/// One `entities` array entry → a real entity. Geometry validation happens
/// here so a bad definition aborts the whole batch before anything commits.
pub(crate) fn build_entity(
    spec: &Value,
    document: Option<&document::Document>,
) -> Parsed<document::EntityType> {
    use document::entities::*;
    use document::types::Vector3;
    let kind = spec["type"].as_str().unwrap_or("").to_ascii_uppercase();
    // Preserve the public unknown-field error while deriving allowed keys from
    // the published contract, rather than maintaining another field list.
    if let (Some(object), Ok(schema)) = (spec.as_object(), schema(&kind)) {
        for field in object.keys() {
            if schema["properties"].get(field).is_none() {
                return Err(failure(
                    "unknown_entity_field",
                    format!("Unknown field '{field}' for {kind} entity"),
                ));
            }
        }
    }
    let entity = match kind.as_str() {
        "LINE" => {
            let mut line = Line::from_points(point_field(spec, "start")?, point_field(spec, "end")?);
            if let Some(t) = spec["thickness"].as_f64() {
                line.thickness = t;
            }
            EntityType::Line(line)
        }
        "CIRCLE" => {
            let radius = spec["radius"]
                .as_f64()
                .filter(|r| r.is_finite() && *r >= 0.0)
                .ok_or_else(|| failure("invalid_radius", "circle needs a finite radius >= 0"))?;
            EntityType::Circle(Circle::from_center_radius(point_field(spec, "center")?, radius))
        }
        "ARC" => {
            let radius = spec["radius"]
                .as_f64()
                .filter(|r| r.is_finite() && *r > 0.0)
                .ok_or_else(|| failure("invalid_radius", "arc needs a finite radius > 0"))?;
            let arc = Arc::from_center_radius_angles(
                point_field(spec, "center")?,
                radius,
                finite_angle_field(spec, "start_angle_deg")?,
                finite_angle_field(spec, "end_angle_deg")?,
            );
            if !arc.sweep_angle().is_finite() || arc.sweep_angle() <= 0.0 {
                return Err(failure_at(
                    "invalid_sweep", "/end_angle_deg",
                    "arc start_angle_deg and end_angle_deg must define a finite non-zero sweep",
                ));
            }
            EntityType::Arc(arc)
        }
        "SPLINE" => {
            let control_points = if spec.get("control_points").is_some() {
                point_list_field(spec, "control_points", 2)?
            } else {
                Vec::new()
            };
            let fit_points = if spec.get("fit_points").is_some() {
                point_list_field(spec, "fit_points", 2)?
            } else {
                Vec::new()
            };
            if control_points.is_empty() && fit_points.is_empty() {
                return Err(failure(
                    "invalid_spline_points",
                    "spline needs control_points or fit_points (at least 2 points)",
                ));
            }
            let degree = match spec.get("degree") {
                None | Some(Value::Null) => 3,
                Some(value) => value
                    .as_f64()
                    .filter(|degree| degree.fract() == 0.0 && (1.0..=10.0).contains(degree))
                    .ok_or_else(|| {
                        failure("invalid_spline_values", "degree must be an integer 1..10")
                    })? as i32,
            };
            let count = control_points.len();
            if count > 0 && count <= degree as usize {
                return Err(failure_at(
                    "invalid_spline_values", "/control_points",
                    format!("a degree {degree} spline needs more than {degree} control_points"),
                ));
            }
            let mut spline = if control_points.is_empty() {
                let mut spline = Spline::from_fit_points(fit_points.clone());
                spline.degree = degree;
                spline
            } else {
                Spline::from_control_points(degree, control_points)
            };
            if !fit_points.is_empty() {
                spline.fit_points = fit_points;
            }
            if spec.get("knots").is_some() {
                if spline.control_points.is_empty() {
                    return Err(failure(
                        "invalid_spline_values",
                        "spline knots require control_points",
                    ));
                }
                let knots = finite_number_list_field(spec, "knots")?;
                if knots.len() != count + degree as usize + 1
                    || knots.windows(2).any(|pair| pair[1] < pair[0])
                {
                    return Err(failure_at(
                        "invalid_spline_values", "/knots",
                        format!(
                            "knots must be {} non-decreasing values (control_points + degree + 1)",
                            count + degree as usize + 1
                        ),
                    ));
                }
                spline.knots = knots;
            }
            if spec.get("weights").is_some() {
                if spline.control_points.is_empty() {
                    return Err(failure(
                        "invalid_spline_values",
                        "spline weights require control_points",
                    ));
                }
                let weights = finite_number_list_field(spec, "weights")?;
                if weights.len() != count || weights.iter().any(|weight| *weight <= 0.0) {
                    return Err(failure_at(
                        "invalid_spline_values", "/weights",
                        format!("weights must be {count} positive values, one per control point"),
                    ));
                }
                spline.weights = weights;
                spline.flags.rational = true;
            }
            if let Some(value) = spec["closed"].as_bool() {
                spline.flags.closed = value;
            }
            if let Some(value) = spec["periodic"].as_bool() {
                spline.flags.periodic = value;
            }
            if let Some(value) = spec["rational"].as_bool() {
                spline.flags.rational = value;
            }
            if let Some(value) = spec["planar"].as_bool() {
                spline.flags.planar = value;
            }
            if let Some(value) = spec["linear"].as_bool() {
                spline.flags.linear = value;
            }
            EntityType::Spline(spline)
        }
        "LWPOLYLINE" | "POLYLINE" => {
            let vertices = spec["vertices"]
                .as_array()
                .filter(|v| v.len() >= 2)
                .ok_or_else(|| failure("invalid_vertices", "polyline needs vertices:[[x,y],…]"))?;
            let mut pline = LwPolyline::new();
            for point in vertices {
                let x = point[0].as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                    failure("invalid_vertices", "polyline vertices must be finite [x,y]")
                })?;
                let y = point[1].as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                    failure("invalid_vertices", "polyline vertices must be finite [x,y]")
                })?;
                pline.add_point(document::types::Vector2::new(x, y));
            }
            if spec["closed"].as_bool().unwrap_or(false) {
                pline.close();
            }
            if let Some(width) = spec["constant_width"].as_f64() {
                pline.constant_width = width;
            }
            EntityType::LwPolyline(pline)
        }
        "POINT" => EntityType::Point(Point::at(point_field(spec, "location")?)),
        "TEXT" => {
            let value = spec["value"].as_str().unwrap_or("");
            if value.is_empty() {
                return Err(failure("invalid_text", "text needs a non-empty value"));
            }
            let mut text = Text::with_value(value, point_field(spec, "position")?)
                .with_height(spec["height"].as_f64().unwrap_or(2.5).abs().max(1e-6))
                .with_rotation(deg_field(spec, "rotation_deg"));
            if let Some(style) = text_style_field(spec, document)? {
                text.style = style;
            }
            let justify = justify_field(spec)?;
            let second = second_point_field(spec, justify)?;
            let (h, v) = justify.alignment();
            use document::entities::attribute_definition::{
                HorizontalAlignment as AH, VerticalAlignment as AV,
            };
            text.horizontal_alignment = match h {
                AH::Left => TextHorizontalAlignment::Left,
                AH::Center => TextHorizontalAlignment::Center,
                AH::Right => TextHorizontalAlignment::Right,
                AH::Aligned => TextHorizontalAlignment::Aligned,
                AH::Middle => TextHorizontalAlignment::Middle,
                AH::Fit => TextHorizontalAlignment::Fit,
            };
            text.vertical_alignment = match v {
                AV::Baseline => TextVerticalAlignment::Baseline,
                AV::Bottom => TextVerticalAlignment::Bottom,
                AV::Middle => TextVerticalAlignment::Middle,
                AV::Top => TextVerticalAlignment::Top,
            };
            // Except Left, the given point is where the justification puts
            // the text; Align and Fit run from it to second_point.
            text.alignment_point = second;
            if let Some(end) = second {
                text.rotation =
                    (end.y - text.insertion_point.y).atan2(end.x - text.insertion_point.x);
            }
            crate::entities::text::sync_text_alignment_point(&mut text);
            EntityType::Text(text)
        }
        "MTEXT" => {
            let value = spec["value"].as_str().unwrap_or("");
            if value.is_empty() {
                return Err(failure("invalid_text", "mtext needs a non-empty value"));
            }
            let mut mtext = MText::with_value(value, point_field(spec, "position")?)
                .with_height(spec["height"].as_f64().unwrap_or(2.5).abs().max(1e-6));
            if let Some(width) = spec["width"].as_f64().filter(|w| *w > 0.0) {
                mtext.rectangle_width = width;
            }
            mtext.rotation = deg_field(spec, "rotation_deg");
            if let Some(style) = text_style_field(spec, document)? {
                mtext.style = style;
            }
            EntityType::MText(Box::new(mtext))
        }
        "INSERT" => {
            let block = spec["block"].as_str().unwrap_or("");
            if block.is_empty() {
                return Err(failure("block_required", "insert needs a block name"));
            }
            let mut insert = Insert::new(block, point_field(spec, "position")?)
                .with_rotation(deg_field(spec, "rotation_deg"));
            match &spec["scale"] {
                Value::Number(factor) => {
                    let factor = factor.as_f64().filter(|f| *f != 0.0).ok_or_else(|| {
                        failure("invalid_scale", "insert scale must be nonzero")
                    })?;
                    insert = insert.with_uniform_scale(factor);
                }
                Value::Array(factors) if factors.len() == 3 => {
                    let axis = |i: usize| {
                        factors[i].as_f64().filter(|f| *f != 0.0).ok_or_else(|| {
                            failure("invalid_scale", "insert scale components must be nonzero")
                        })
                    };
                    insert = insert.with_scale(axis(0)?, axis(1)?, axis(2)?);
                }
                _ => {}
            }
            if let Some(values) = spec.get("attributes") {
                insert.attributes = insert_attributes(&insert, values, document)?;
            }
            EntityType::Insert(insert)
        }
        "SOLID" => {
            let raw_corners = spec["corners"]
                .as_array()
                .filter(|v| (3..=4).contains(&v.len()))
                .ok_or_else(|| failure("invalid_corners", "solid needs 3 or 4 corners"))?;
            let corners: Vec<Vector3> = raw_corners
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    if value.as_array().is_none_or(|values| values.len() != 3) {
                        return Err(failure(
                            "invalid_corners",
                            format!("corners[{index}] must be [x,y,z]"),
                        ));
                    }
                    point_value(
                        Some(value),
                        &format!("corners[{index}]"),
                        "invalid_corners",
                    )
                })
                .collect::<Parsed<_>>()?;
            let solid = match corners.as_slice() {
                [first, second, third] => Solid::triangle(*first, *second, *third),
                [first, second, third, fourth] => Solid::new(*first, *second, *third, *fourth),
                _ => unreachable!("corner count was validated above"),
            };
            EntityType::Solid(solid)
        }
        "HATCH" => {
            let boundary = spec["boundary"]
                .as_array()
                .filter(|v| v.len() >= 3)
                .ok_or_else(|| failure("invalid_boundary", "hatch needs boundary:[[x,y],…] (>= 3 points)"))?;
            let mut vertices = Vec::with_capacity(boundary.len());
            for point in boundary {
                let x = point[0].as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                    failure("invalid_boundary", "hatch boundary must be finite [x,y]")
                })?;
                let y = point[1].as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                    failure("invalid_boundary", "hatch boundary must be finite [x,y]")
                })?;
                vertices.push(document::types::Vector2::new(x, y));
            }
            let mut hatch = Hatch::new();
            if spec["solid"].as_bool().unwrap_or(false) {
                hatch.is_solid = true;
            } else {
                hatch.is_solid = false;
                hatch.pattern = HatchPattern::new(
                    spec["pattern"].as_str().unwrap_or("ANSI31"),
                );
                hatch.pattern_scale = spec["pattern_scale"].as_f64().unwrap_or(1.0).abs().max(1e-6);
                hatch.pattern_angle = deg_field(spec, "pattern_angle_deg");
            }
            let mut path = BoundaryPath::external();
            path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(vertices, true)));
            hatch.paths.push(path);
            EntityType::Hatch(Box::new(hatch))
        }
        "ATTDEF" => {
            use crate::modules::draw::draw::attdef::{self as attdef, AttdefSpec};
            let tag = spec["tag"].as_str().unwrap_or("").trim();
            if tag.is_empty() || tag.contains(char::is_whitespace) {
                return Err(failure_at(
                    "invalid_tag",
                    "/tag",
                    "attribute tag needs a nonempty value without spaces",
                ));
            }
            let flag = |key: &str, bit: u8| if spec[key].as_bool() == Some(true) { bit } else { 0 };
            let justify = justify_field(spec)?;
            let second = second_point_field(spec, justify)?;
            let to_dvec = |p: Vector3| glam::DVec3::new(p.x, p.y, p.z);
            let attdef_spec = AttdefSpec {
                aflags: flag("invisible", attdef::AFLAG_INVISIBLE)
                    | flag("constant", attdef::AFLAG_CONSTANT)
                    | flag("verify", attdef::AFLAG_VERIFY)
                    | flag("preset", attdef::AFLAG_PRESET)
                    | flag("lock_position", attdef::AFLAG_LOCK),
                annotative: false,
                tag: tag.to_owned(),
                prompt: spec["prompt"].as_str().unwrap_or("").to_owned(),
                value: spec["default"].as_str().unwrap_or("").to_owned(),
                justify,
                style: text_style_field(spec, document)?.unwrap_or_else(|| "Standard".to_owned()),
                height: spec["height"].as_f64().unwrap_or(2.5).abs().max(1e-6),
                rotation: deg_field(spec, "rotation_deg"),
                width_factor: 1.0,
                oblique_angle: 0.0,
                boundary_width: 0.0,
                line_spacing: 1.0,
                field: None,
            };
            attdef::build(
                &attdef_spec,
                crate::command::WorkingPlane::default(),
                to_dvec(point_field(spec, "position")?),
                second.map(to_dvec),
            )
        }
        "REGION" => region_from_loops(spec)?,
        other => {
            return Err(failure(
                "unknown_entity_type",
                format!(
                    "Unknown entity type '{other}'. Supported: Line, Circle, Arc, Spline, LwPolyline, Point, Text, MText, Insert, Solid, Hatch, AttDef, Region"
                ),
            ))
        }
    };
    let mut entity = entity;
    apply_common_properties(spec, &mut entity, document)?;
    let contract = schema(&kind).expect("supported factory type");
    crate::schema_validation::validate_shape(&contract, spec, "")
        .map_err(|error| failure("invalid_entity_definition", error))?;
    Ok(entity)
}

/// A REGION bounded by closed vertex loops, built by the geometry kernel the
/// way the REGION command builds one from closed polylines: the first loop
/// is the outer boundary, the others holes.
fn region_from_loops(spec: &Value) -> Parsed<document::EntityType> {
    let invalid = |error: &str| failure_at("invalid_region", "/loops", error);
    let loops = spec["loops"]
        .as_array()
        .filter(|loops| !loops.is_empty())
        .ok_or_else(|| invalid("region needs loops:[[[x,y],…],…]"))?;
    let mut plane = None;
    let mut curves = Vec::with_capacity(loops.len());
    for (index, vertices) in loops.iter().enumerate() {
        let points = point_list_field(&json!({ "loop": vertices }), "loop", 3)
            .map_err(|_| invalid(&format!("loops[{index}] needs at least 3 finite points")))?;
        let mut pline = document::entities::LwPolyline::new();
        pline.elevation = points[0].z;
        for point in &points {
            pline.add_point(document::types::Vector2::new(point.x, point.y));
        }
        pline.close();
        let Some((loop_plane, mut pieces, true)) =
            crate::scene::model::presspull_model::profile_geometry(
                &document::EntityType::LwPolyline(pline),
            )
        else {
            return Err(invalid(&format!(
                "loops[{index}] is not a closed planar boundary"
            )));
        };
        plane.get_or_insert(loop_plane);
        curves.append(&mut pieces);
    }
    let plane = plane.expect("at least one loop");
    let body = kernel::brep::planar_region(plane, &curves).ok_or_else(|| {
        invalid("the loops do not bound a region (self-intersecting, or a hole outside the boundary)")
    })?;
    let sat = crate::scene::convert::acis_export::solid_to_sat(&body)
        .ok_or_else(|| invalid("the region could not be encoded as ACIS"))?;
    let mut region = document::entities::Region::new();
    region.point_of_reference =
        document::types::Vector3::new(plane.origin[0], plane.origin[1], plane.origin[2]);
    region.wires = crate::scene::model::solid_model::edge_wires(&body);
    region.set_sat_document(&sat);
    Ok(document::EntityType::Region(region))
}
