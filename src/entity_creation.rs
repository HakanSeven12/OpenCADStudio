//! Discoverable creation contracts, shared by MCP preflight and the entity builder.
use serde_json::{json, Value};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_examples_build_with_the_real_factory() {
        let document = codec::Document::new();
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
            codec::types::Color::from_index(2)
        );
        let spline = json!({"type":"Spline","degree":2.0,"control_points":[[0,0],[5,10],[10,0]]});
        validate_entity(&spline, "").unwrap();
        let codec::EntityType::Spline(built) = build_entity(&spline, Some(&document)).unwrap()
        else {
            panic!("expected spline")
        };
        assert_eq!(built.degree, 2);
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
            build_entity(&spec, Some(&codec::Document::new())).unwrap_err()["code"],
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
    let mut properties = json!({"type":{"type":"string","pattern":pattern},"layer":{"type":"string","description":"Missing layers are created; empty uses the default layer."},"color":{"type":"integer","minimum":-32768,"maximum":32767,"description":"DXF indexed color, including ByLayer/ByBlock sentinels."}});
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
            properties["pattern"] = string;
            properties["pattern_scale"] = json!({"type":"number","default":1,"description":"Absolute value, clamped to at least 0.000001."});
            properties["pattern_angle_deg"] = number.clone();
            (
                vec!["boundary"],
                json!({"type":"Hatch","boundary":[[0,0],[10,0],[0,10]],"solid":true}),
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
fn point_value(value: Option<&Value>, label: &str, code: &str) -> Parsed<codec::types::Vector3> {
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
    Ok(codec::types::Vector3::new(coord(0)?, coord(1)?, z))
}

/// `[x,y]` or `[x,y,z]` (missing z = 0) from `req[key]`.
pub(crate) fn point_field(req: &Value, key: &str) -> Parsed<codec::types::Vector3> {
    point_value(req.get(key), key, "invalid_point")
}

fn point_list_field(spec: &Value, key: &str, minimum: usize) -> Parsed<Vec<codec::types::Vector3>> {
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

fn apply_common_properties(spec: &Value, entity: &mut codec::EntityType) -> Parsed<()> {
    if let Some(layer) = spec["layer"].as_str() {
        if !layer.is_empty()
            && !matches!(
                entity,
                codec::EntityType::Block(_) | codec::EntityType::BlockEnd(_)
            )
        {
            entity.common_mut().layer = layer.to_owned();
        }
    }
    if let Some(index) = spec["color"]
        .as_f64()
        .filter(|n| n.fract() == 0.0 && (-32768.0..=32767.0).contains(n))
    {
        entity.common_mut().color = codec::types::Color::from_index(index as i16);
    }
    Ok(())
}

/// An explicit text style must exist in the current document; otherwise a
/// typo would silently fall back to the default style.
fn text_style_field(spec: &Value, document: Option<&codec::Document>) -> Parsed<Option<String>> {
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

/// One `entities` array entry → a real entity. Geometry validation happens
/// here so a bad definition aborts the whole batch before anything commits.
pub(crate) fn build_entity(
    spec: &Value,
    document: Option<&codec::Document>,
) -> Parsed<codec::EntityType> {
    use codec::entities::*;
    use codec::types::Vector3;
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
                pline.add_point(codec::types::Vector2::new(x, y));
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
                vertices.push(codec::types::Vector2::new(x, y));
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
        other => {
            return Err(failure(
                "unknown_entity_type",
                format!(
                    "Unknown entity type '{other}'. Supported: Line, Circle, Arc, Spline, LwPolyline, Point, Text, MText, Insert, Solid, Hatch"
                ),
            ))
        }
    };
    let mut entity = entity;
    apply_common_properties(spec, &mut entity)?;
    let contract = schema(&kind).expect("supported factory type");
    crate::schema_validation::validate_shape(&contract, spec, "")
        .map_err(|error| failure("invalid_entity_definition", error))?;
    Ok(entity)
}
