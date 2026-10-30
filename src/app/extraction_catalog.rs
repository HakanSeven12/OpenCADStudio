//! What the data extraction wizard can extract: the object name and type each
//! drawing object is listed under, and the properties (with their display
//! names and categories) and formatted values it offers for it.

use codec::types::Color;
use codec::{Document, EntityType};

/// One extractable property: its key, display name and category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Property {
    pub key: &'static str,
    pub name: &'static str,
    pub category: &'static str,
}

const fn p(key: &'static str, name: &'static str, category: &'static str) -> Property {
    Property {
        key,
        name,
        category,
    }
}

const GENERAL: &str = "General";
const GEOMETRY: &str = "Geometry";
const DRAWING: &str = "Drawing";
const TEXT: &str = "Text";
const VISUALIZATION: &str = "3D Visualization";
const MISC: &str = "Misc";

/// The drawing-level properties every object offers.
const DRAWING_PROPERTIES: [Property; 15] = [
    p("AcDxDwgSummaryDwgAuthor", "Author", DRAWING),
    p("AcDxDwgSummaryDwgComments", "Comments", DRAWING),
    p("AcDxDwgSummaryDwgRevisionNumber", "Drawing Revision Number", DRAWING),
    p("AcDxDwgSummaryDwgAccessed", "File Accessed", DRAWING),
    p("AcDxDwgSummaryDwgCreated", "File Created", DRAWING),
    p("AcDxDwgSummaryDwgLastSavedBy", "File Last Saved By", DRAWING),
    p("AcDxDwgSummaryDwgLocation", "File Location", DRAWING),
    p("AcDxDwgSummaryDwgModified", "File Modified", DRAWING),
    p("AcDxDwgSummaryDwgName", "File Name", DRAWING),
    p("AcDxDwgSummaryDwgSize", "File Size", DRAWING),
    p("AcDxDwgSummaryDwgHyperLinkBase", "Hyperlink Base", DRAWING),
    p("AcDxDwgSummaryDwgKeywords", "Keywords", DRAWING),
    p("AcDxDwgSummaryDwgSubject", "Subject", DRAWING),
    p("AcDxDwgSummaryDwgTitle", "Title", DRAWING),
    p("AcDxDwgSummaryDwgTotalEditingTime", "Total Editing Time", DRAWING),
];

const ENTITY_PROPERTIES: [Property; 10] = [
    p("Color", "Color", GENERAL),
    p("AcDxEntityHyperlink", "Hyperlink", GENERAL),
    p("Layer", "Layer", GENERAL),
    p("Linetype", "Linetype", GENERAL),
    p("LinetypeScale", "Linetype Scale", GENERAL),
    p("LineWeight", "Lineweight", GENERAL),
    p("PlotStyleName", "Plot Style", GENERAL),
    p("EdgeStyleId", "EdgeStyleId", VISUALIZATION),
    p("FaceStyleId", "FaceStyleId", VISUALIZATION),
    p("VisualStyleId", "VisualStyleId", VISUALIZATION),
];

const THICKNESS: Property = p("Thickness", "Thickness", GENERAL);
const MATERIAL: Property = p("Material", "Material", VISUALIZATION);

const LINE: [Property; 11] = [
    p("Angle", "Angle", GEOMETRY),
    p("Delta.X", "Delta X", GEOMETRY),
    p("Delta.Y", "Delta Y", GEOMETRY),
    p("Delta.Z", "Delta Z", GEOMETRY),
    p("EndPoint.X", "End X", GEOMETRY),
    p("EndPoint.Y", "End Y", GEOMETRY),
    p("EndPoint.Z", "End Z", GEOMETRY),
    p("Length", "Length", GEOMETRY),
    p("StartPoint.X", "Start X", GEOMETRY),
    p("StartPoint.Y", "Start Y", GEOMETRY),
    p("StartPoint.Z", "Start Z", GEOMETRY),
];

const CIRCLE: [Property; 7] = [
    p("Area", "Area", GEOMETRY),
    p("Center.X", "Center X", GEOMETRY),
    p("Center.Y", "Center Y", GEOMETRY),
    p("Center.Z", "Center Z", GEOMETRY),
    p("Circumference", "Circumference", GEOMETRY),
    p("Diameter", "Diameter", GEOMETRY),
    p("Radius", "Radius", GEOMETRY),
];

const ARC: [Property; 9] = [
    p("ArcLength", "Arc Length", GEOMETRY),
    p("Area", "Area", GEOMETRY),
    p("Center.X", "Center X", GEOMETRY),
    p("Center.Y", "Center Y", GEOMETRY),
    p("Center.Z", "Center Z", GEOMETRY),
    p("EndAngle", "End Angle", GEOMETRY),
    p("Radius", "Radius", GEOMETRY),
    p("StartAngle", "Start Angle", GEOMETRY),
    p("TotalAngle", "Total Angle", GEOMETRY),
];

const TEXT_GEOMETRY: [Property; 3] = [
    p("Position.X", "Position X", GEOMETRY),
    p("Position.Y", "Position Y", GEOMETRY),
    p("Position.Z", "Position Z", GEOMETRY),
];

const SINGLE_LINE_TEXT: [Property; 6] = [
    p("Height", "Height", TEXT),
    p("Oblique", "Obliquing", TEXT),
    p("Rotation", "Rotation", TEXT),
    p("TextStyleName", "Style", TEXT),
    p("TextString", "Value", TEXT),
    p("WidthFactor", "Width Factor", TEXT),
];

const MULTILINE_TEXT: [Property; 4] = [
    p("Height", "Height", TEXT),
    p("Rotation", "Rotation", TEXT),
    p("TextStyleName", "Style", TEXT),
    p("TextString", "Value", TEXT),
];

const POLYLINE: [Property; 4] = [
    p("Area", "Area", GEOMETRY),
    p("Closed", "Closed", MISC),
    p("Elevation", "Elevation", GEOMETRY),
    p("Length", "Length", GEOMETRY),
];

const BLOCK: [Property; 7] = [
    p("Position.X", "Position X", GEOMETRY),
    p("Position.Y", "Position Y", GEOMETRY),
    p("Position.Z", "Position Z", GEOMETRY),
    p("Rotation", "Rotation", GEOMETRY),
    p("ScaleFactors.X", "Scale X", GEOMETRY),
    p("ScaleFactors.Y", "Scale Y", GEOMETRY),
    p("ScaleFactors.Z", "Scale Z", GEOMETRY),
];

/// The name an object is listed and extracted under, and whether it is a
/// block reference (listed by block name).
pub(crate) fn object_name(entity: &EntityType) -> (String, bool) {
    let name = match entity {
        EntityType::Insert(insert) => return (insert.block_name.clone(), true),
        EntityType::Line(_) => "Line",
        EntityType::Circle(_) => "Circle",
        EntityType::Arc(_) => "Arc",
        EntityType::Text(_) => "Text",
        EntityType::MText(_) => "MText",
        EntityType::LwPolyline(_) => "Polyline",
        EntityType::Point(_) => "Point",
        EntityType::Ellipse(_) => "Ellipse",
        EntityType::Spline(_) => "Spline",
        EntityType::Hatch(_) => "Hatch",
        EntityType::Table(_) => "Table",
        EntityType::Solid(_) => "2D Solid",
        EntityType::MultiLeader(_) => "Multileader",
        _ => {
            let dxf = crate::entities::names::dxf_name(entity);
            let mut name = dxf.to_ascii_lowercase();
            if let Some(first) = name.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            return (name, false);
        }
    };
    (name.to_string(), false)
}

/// The properties an object offers, in no particular order.
pub(crate) fn properties(entity: &EntityType) -> Vec<Property> {
    let mut list: Vec<Property> = ENTITY_PROPERTIES.to_vec();
    list.extend(DRAWING_PROPERTIES);
    let specific: &[Property] = match entity {
        EntityType::Line(_) => &LINE,
        EntityType::Circle(_) => &CIRCLE,
        EntityType::Arc(_) => &ARC,
        EntityType::Text(_) => &SINGLE_LINE_TEXT,
        EntityType::MText(_) => &MULTILINE_TEXT,
        EntityType::LwPolyline(_) => &POLYLINE,
        EntityType::Insert(_) => &BLOCK,
        _ => &[],
    };
    list.extend_from_slice(specific);
    if matches!(entity, EntityType::Text(_) | EntityType::MText(_)) {
        list.extend(TEXT_GEOMETRY);
    }
    if matches!(
        entity,
        EntityType::Line(_)
            | EntityType::Circle(_)
            | EntityType::Arc(_)
            | EntityType::Text(_)
            | EntityType::LwPolyline(_)
            | EntityType::Point(_)
    ) {
        list.push(THICKNESS);
    }
    if !matches!(entity, EntityType::Text(_) | EntityType::MText(_)) {
        list.push(MATERIAL);
    }
    list
}

/// Drawing-wide values: file facts and the summary information.
pub(crate) struct DrawingFacts {
    pub location: String,
    pub file_name: String,
    pub size: String,
    pub created: String,
    pub modified: String,
    pub accessed: String,
    pub editing_time: String,
    pub author: String,
    pub comments: String,
    pub revision: String,
    pub last_saved_by: String,
    pub hyperlink_base: String,
    pub keywords: String,
    pub subject: String,
    pub title: String,
    pub linear_precision: usize,
    pub angular_precision: usize,
}

impl DrawingFacts {
    pub(crate) fn of(doc: &Document, path: Option<&std::path::Path>) -> Self {
        let info = &doc.summary_info;
        let mut facts = Self {
            location: String::new(),
            file_name: String::new(),
            size: String::new(),
            created: String::new(),
            modified: String::new(),
            accessed: String::new(),
            // Minutes in the drawing, at least one.
            editing_time: ((doc.header.total_editing_time * 1440.0).round().max(1.0) as i64)
                .to_string(),
            author: info.author.clone(),
            comments: info.comments.clone(),
            revision: info.revision_number.clone(),
            last_saved_by: info.last_saved_by.clone(),
            hyperlink_base: info.hyperlink_base.clone(),
            keywords: info.keywords.clone(),
            subject: info.subject.clone(),
            title: info.title.clone(),
            linear_precision: doc.header.linear_unit_precision.clamp(0, 8) as usize,
            angular_precision: doc.header.angular_unit_precision.clamp(0, 8) as usize,
        };
        if let Some(path) = path {
            facts.location = path
                .parent()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();
            facts.file_name = path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Ok(meta) = std::fs::metadata(path) {
                facts.size = meta.len().to_string();
                let stamp = |time: std::io::Result<std::time::SystemTime>| {
                    time.ok().map(local_date_time).unwrap_or_default()
                };
                facts.created = stamp(meta.created());
                facts.modified = stamp(meta.modified());
                facts.accessed = stamp(meta.accessed());
            }
        }
        facts
    }
}

/// A file time as the operating system's short date and long time.
fn local_date_time(time: std::time::SystemTime) -> String {
    let seconds = time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs_f64())
        .unwrap_or_default();
    let julian =
        seconds / 86_400.0 + 2_440_587.5 + crate::entities::field::utc_offset_days();
    codec::fields::format_dt_in(
        codec::fields::julian_parts(julian),
        "%c",
        &crate::entities::field::os_date_locale(),
    )
}

fn color_text(color: &Color) -> String {
    match color {
        Color::ByLayer => "ByLayer".into(),
        Color::ByBlock => "ByBlock".into(),
        Color::None => "None".into(),
        Color::Index(index) => match index {
            1 => "Red".into(),
            2 => "Yellow".into(),
            3 => "Green".into(),
            4 => "Cyan".into(),
            5 => "Blue".into(),
            6 => "Magenta".into(),
            7 => "White".into(),
            other => other.to_string(),
        },
        Color::Rgb { r, g, b } => format!("{r},{g},{b}"),
    }
}

fn lineweight_text(weight: &codec::types::LineWeight) -> String {
    use codec::types::LineWeight;
    match weight {
        LineWeight::ByLayer => "ByLayer".into(),
        LineWeight::ByBlock => "ByBlock".into(),
        LineWeight::Default => "Default".into(),
        LineWeight::Value(hundredths) => format!("{:.2} mm", *hundredths as f64 / 100.0),
    }
}

/// The value of property `key` for an object, as the extraction shows it.
pub(crate) fn value(entity: &EntityType, key: &str, facts: &DrawingFacts) -> String {
    let lin = |v: f64| format!("{v:.prec$}", prec = facts.linear_precision);
    let ang = |radians: f64| {
        // Degrees at the angular precision; a full turn reads as zero.
        let step = 10f64.powi(facts.angular_precision as i32);
        let degrees = (radians.to_degrees().rem_euclid(360.0) * step).round() / step;
        format!(
            "{:.prec$}",
            if degrees >= 360.0 { 0.0 } else { degrees },
            prec = facts.angular_precision
        )
    };
    let common = entity.common();
    match key {
        "Color" => return color_text(&common.color),
        "Layer" => return common.layer.clone(),
        "Linetype" => {
            return if common.linetype.is_empty() {
                "ByLayer".into()
            } else {
                common.linetype.clone()
            }
        }
        "LinetypeScale" => return lin(common.linetype_scale),
        "LineWeight" => return lineweight_text(&common.line_weight),
        "PlotStyleName" | "Material" => return "ByLayer".into(),
        "EdgeStyleId" | "FaceStyleId" | "VisualStyleId" => return "(0)".into(),
        "AcDxEntityHyperlink" => {
            return common
                .extended_data
                .get_record("PE_URL")
                .and_then(|record| {
                    record.values.iter().find_map(|value| match value {
                        codec::xdata::XDataValue::String(text) => Some(text.clone()),
                        _ => None,
                    })
                })
                .unwrap_or_default()
        }
        "AcDxDwgSummaryDwgAuthor" => return facts.author.clone(),
        "AcDxDwgSummaryDwgComments" => return facts.comments.clone(),
        "AcDxDwgSummaryDwgRevisionNumber" => return facts.revision.clone(),
        "AcDxDwgSummaryDwgAccessed" => return facts.accessed.clone(),
        "AcDxDwgSummaryDwgCreated" => return facts.created.clone(),
        "AcDxDwgSummaryDwgLastSavedBy" => return facts.last_saved_by.clone(),
        "AcDxDwgSummaryDwgLocation" => return facts.location.clone(),
        "AcDxDwgSummaryDwgModified" => return facts.modified.clone(),
        "AcDxDwgSummaryDwgName" => return facts.file_name.clone(),
        "AcDxDwgSummaryDwgSize" => return facts.size.clone(),
        "AcDxDwgSummaryDwgHyperLinkBase" => return facts.hyperlink_base.clone(),
        "AcDxDwgSummaryDwgKeywords" => return facts.keywords.clone(),
        "AcDxDwgSummaryDwgSubject" => return facts.subject.clone(),
        "AcDxDwgSummaryDwgTitle" => return facts.title.clone(),
        "AcDxDwgSummaryDwgTotalEditingTime" => return facts.editing_time.clone(),
        _ => {}
    }
    match entity {
        EntityType::Line(line) => {
            let d = [
                line.end.x - line.start.x,
                line.end.y - line.start.y,
                line.end.z - line.start.z,
            ];
            match key {
                "Angle" => ang(d[1].atan2(d[0])),
                "Delta.X" => lin(d[0]),
                "Delta.Y" => lin(d[1]),
                "Delta.Z" => lin(d[2]),
                "EndPoint.X" => lin(line.end.x),
                "EndPoint.Y" => lin(line.end.y),
                "EndPoint.Z" => lin(line.end.z),
                "Length" => lin((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()),
                "StartPoint.X" => lin(line.start.x),
                "StartPoint.Y" => lin(line.start.y),
                "StartPoint.Z" => lin(line.start.z),
                "Thickness" => lin(line.thickness),
                _ => String::new(),
            }
        }
        EntityType::Circle(circle) => match key {
            "Area" => lin(std::f64::consts::PI * circle.radius * circle.radius),
            "Center.X" => lin(circle.center.x),
            "Center.Y" => lin(circle.center.y),
            "Center.Z" => lin(circle.center.z),
            "Circumference" => lin(std::f64::consts::TAU * circle.radius),
            "Diameter" => lin(2.0 * circle.radius),
            "Radius" => lin(circle.radius),
            "Thickness" => lin(circle.thickness),
            _ => String::new(),
        },
        EntityType::Arc(arc) => {
            let sweep = (arc.end_angle - arc.start_angle).rem_euclid(std::f64::consts::TAU);
            match key {
                "ArcLength" => lin(arc.radius * sweep),
                "Area" => lin(0.5 * arc.radius * arc.radius * (sweep - sweep.sin())),
                "Center.X" => lin(arc.center.x),
                "Center.Y" => lin(arc.center.y),
                "Center.Z" => lin(arc.center.z),
                "EndAngle" => ang(arc.end_angle),
                "Radius" => lin(arc.radius),
                "StartAngle" => ang(arc.start_angle),
                "TotalAngle" => ang(sweep),
                "Thickness" => lin(arc.thickness),
                _ => String::new(),
            }
        }
        EntityType::Text(text) => {
            let position = crate::entities::text::position(text);
            match key {
                "Height" => lin(text.height),
                "Oblique" => ang(text.oblique_angle),
                "Rotation" => ang(text.rotation),
                "TextStyleName" => text.style.clone(),
                "TextString" => text.value.clone(),
                "WidthFactor" => lin(text.width_factor),
                "Position.X" => lin(position.x),
                "Position.Y" => lin(position.y),
                "Position.Z" => lin(position.z),
                "Thickness" => lin(text.thickness),
                _ => String::new(),
            }
        }
        EntityType::MText(text) => match key {
            "Height" => lin(text.height),
            "Rotation" => ang(text.rotation),
            "TextStyleName" => text.style.clone(),
            "TextString" => text.value.clone(),
            "Position.X" => lin(text.insertion_point.x),
            "Position.Y" => lin(text.insertion_point.y),
            "Position.Z" => lin(text.insertion_point.z),
            _ => String::new(),
        },
        EntityType::LwPolyline(polyline) => match key {
            "Closed" => if polyline.is_closed { "Yes" } else { "No" }.into(),
            "Elevation" => lin(polyline.elevation),
            "Thickness" => lin(polyline.thickness),
            "Length" | "Area" => {
                use crate::entities::traits::MassPropsCalc;
                let props = polyline.mass_props();
                lin(if key == "Area" { props.area } else { props.perimeter })
            }
            _ => String::new(),
        },
        EntityType::Insert(insert) => match key {
            "Position.X" => lin(insert.insert_point.x),
            "Position.Y" => lin(insert.insert_point.y),
            "Position.Z" => lin(insert.insert_point.z),
            "Rotation" => ang(insert.rotation),
            "ScaleFactors.X" => lin(insert.x_scale()),
            "ScaleFactors.Y" => lin(insert.y_scale()),
            "ScaleFactors.Z" => lin(insert.z_scale()),
            _ => String::new(),
        },
        EntityType::Point(point) => match key {
            "Thickness" => lin(point.thickness),
            _ => String::new(),
        },
        _ => String::new(),
    }
}
