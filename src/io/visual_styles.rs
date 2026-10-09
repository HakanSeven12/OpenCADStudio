//! The visual styles a new drawing carries, as the reference's new drawing
//! has them: name, kind and settings, then each property value in file
//! order. Tokens: `L` long, `S` short, `D` real, `B0`/`B1` flag, `I` color
//! index, `R` true color, `CL`/`CB`/`CN` by layer / by block / none, `T` text;
//! a trailing `!` marks a property that is not enabled.

use codec::objects::{ObjectType, VisualStyle, VisualStyleProperty, VisualStylePropertyValue};
use codec::types::Color;

/// (name, type, face lighting model, face lighting quality, face color
/// mode, face modifier, edge model, edge style, internal only, extended
/// lighting model, properties)
type Row = (&'static str, i16, i16, i16, i16, i32, i32, i32, bool, i16, &'static str);

const STYLES: &[Row] = &[
    ("2dWireframe", 4, 0, 2, 0, 0, 1, 4, false, 3,
        "L0 L2 L0 L0 D0.6 D30.0 R255,255,255 L1 L4 I7 CN L1 L1 D1.0 L0 CN D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L1 D0.0 L0 B1 B1 B0 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Basic", 7, 1, 0, 1, 0, 0, 4, true, 3,
        "L1 L0 L1 L0 D0.6 D30.0 R255,255,255 L0 L4 I7 CN L1 L1 D1.0 L8 I7 D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L1 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Brighten", 12, 2, 2, 0, 0, 1, 4, true, 3,
        "L2 L2 L0 L0 D0.6 D30.0 R255,255,255 L1 L4 I7 CN L1 L1 D1.0 L8 I7 D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L1 D50.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("ColorChange", 16, 2, 2, 3, 0, 1, 4, true, 3,
        "L2 L2 L3 L0 D0.6 D30.0 R128,128,128 L1 L4 I7 CN L1 L1 D1.0 L8 R128,128,128 D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L1 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Conceptual", 9, 3, 2, 0, 0, 2, 2, false, 3,
        "L3 L2 L0 L0 D0.6 D30.0 R255,255,255 L2 L2 I7 CN L1 L1 D179.0 L8 I7 D1.0 L1 L6 L2 I7 L3 L0 L0 B0 L1 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Dim", 11, 2, 2, 0, 0, 1, 4, true, 3,
        "L2 L2 L0 L0 D0.6 D30.0 R255,255,255 L1 L4 I7 CN L1 L1 D1.0 L8 I7 D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L1 D-50.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("EdgeColorOff", 22, 2, 2, 0, 0, 1, 4, true, 3,
        "L2! L2! L0! L0! D0.6! D30.0! R255,255,255! L1! L4! I7! CN! L1! L1! D1.0! L8 I7! D1.0! L1! L6! L2! I7! L5! L0! L0! B0! L1! D0.0! L0! B0! B1! B1! B0! B0! B0! B0! B0! B0! L50! D0.0! D1.0! L0! R0,0,0! L50! L3! R0,0,255! B0! L50! L50! L50! B0! L50! CL! D1.0! L2! Tstrokes_ogs.tif! B0! D1.0! D1.0!"),
    ("Facepattern", 15, 2, 2, 0, 0, 1, 4, true, 3,
        "L2 L2 L0 L0 D0.6 D30.0 R255,255,255 L1 L4 I7 CN L1 L1 D1.0 L8 I7 D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L1 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Flat", 0, 2, 1, 1, 2, 0, 0, true, 3,
        "L2 L1 L1 L2 D0.6 D30.0 R255,255,255 L0 L0 I7 CN L1 L1 D1.0 L8 I7 D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L13 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("FlatWithEdges", 1, 2, 1, 1, 2, 1, 0, true, 3,
        "L2 L1 L1 L2 D0.6 D30.0 R255,255,255 L1 L0 I7 CN L1 L1 D1.0 L0 CN D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L13 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Gouraud", 2, 2, 2, 1, 2, 0, 0, true, 3,
        "L2 L2 L1 L2 D0.6 D30.0 R255,255,255 L0 L0 I7 CN L1 L1 D1.0 L0 I7 D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L13 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("GouraudWithEdges", 3, 2, 2, 1, 2, 1, 0, true, 3,
        "L2 L2 L1 L2 D0.6 D30.0 R255,255,255 L1 L0 I7 CN L1 L1 D1.0 L0 CN D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L13 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Hidden", 6, 1, 2, 2, 0, 2, 2, false, 3,
        "L1 L2 L2 L0 D0.6 D30.0 R255,255,255 L2 L2 I7 CN L2 L1 D40.0 L0 CN D1.0 L1 L6 L2 I7 L3 L0 L0 B0 L1 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("JitterOff", 20, 2, 2, 0, 0, 1, 4, true, 3,
        "L2! L2! L0! L0! D0.6! D30.0! R255,255,255! L1! L4! I7! CN! L1! L1! D1.0! L10 I7! D1.0! L1! L6! L2! I7! L5! L0! L0! B0! L1! D0.0! L0! B0! B1! B1! B0! B0! B0! B0! B0! B0! L50! D0.0! D1.0! L0! R0,0,0! L50! L3! R0,0,255! B0! L50! L50! L50! B0! L50! CL! D1.0! L2! Tstrokes_ogs.tif! B0! D1.0! D1.0!"),
    ("Linepattern", 14, 2, 2, 0, 0, 1, 4, true, 3,
        "L2 L2 L0 L0 D0.6 D30.0 R255,255,255 L1 L4 I7 CN L7 L7 D1.0 L8 I7 D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L1 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("OverhangOff", 21, 2, 2, 0, 0, 1, 4, true, 3,
        "L2! L2! L0! L0! D0.6! D30.0! R255,255,255! L1! L4! I7! CN! L1! L1! D1.0! L9 I7! D1.0! L1! L6! L2! I7! L5! L0! L0! B0! L1! D0.0! L0! B0! B1! B1! B0! B0! B0! B0! B0! B0! L50! D0.0! D1.0! L0! R0,0,0! L50! L3! R0,0,255! B0! L50! L50! L50! B0! L50! CL! D1.0! L2! Tstrokes_ogs.tif! B0! D1.0! D1.0!"),
    ("Realistic", 8, 2, 3, 0, 2, 0, 0, false, 3,
        "L2 L3 L0 L2 D0.6 D30.0 R255,255,255 L0 L0 I7 CN L1 L1 D1.0 L8 CN D1.0 L1 L6 L2 I7 L3 L0 L0 B0 L13 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Shaded", 27, 2, 2, 1, 2, 0, 4, false, 3,
        "L2 L2 L1 L2 D0.6 D30.0 R255,255,255 L0 L4 I7 CN L1 L1 D1.0 L8 CN D1.0 L1 L6 L2 R120,120,120 L3 L0 L0 B0 L5 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Shaded with edges", 26, 2, 2, 1, 2, 1, 2, false, 3,
        "L2 L2 L1 L2 D0.6 D30.0 R255,255,255 L1 L2 I7 CN L2 L1 D1.0 L8 CN D1.0 L1 L6 L2 I7 L3 L0 L0 B0 L5 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Shades of Gray", 23, 2, 2, 3, 0, 2, 2, false, 3,
        "L2 L2 L3 L0 D0.6 D30.0 R255,255,255 L2 L2 I7 I7 L1 L1 D40.0 L8 I7 D1.0 L1 L6 L2 I7 L3 L0 L0 B0 L1 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Sketchy", 24, 1, 2, 2, 0, 2, 2, false, 3,
        "L1 L2 L2 L0 D0.6 D30.0 R255,255,255 L2 L2 I7 I7 L1 L1 D40.0 L11 I7 D1.0 L1 L6 L2 I7 L6 L0 L0 B0 L1 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Thicken", 13, 2, 2, 0, 0, 1, 4, true, 3,
        "L2 L2 L0 L0 D0.6 D30.0 R255,255,255 L1 L4 I7 CN L1 L1 D1.0 L12 I7 D1.0 L1 L6 L2 I7 L5 L0 L0 B0 L1 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("Wireframe", 5, 0, 2, 0, 0, 1, 4, false, 3,
        "L0 L2 L0 L0 D0.6 D30.0 R255,255,255 L1 L4 I7 CN L1 L1 D1.0 L0 CN D1.0 L1 L6 L2 I7 L3 L0 L0 B0 L1 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
    ("X-Ray", 25, 2, 2, 1, 1, 1, 0, false, 3,
        "L2 L2 L1 L1 D0.5 D30.0 R255,255,255 L1 L0 I7 CN L1 L1 D1.0 L8 I7 D1.0 L1 L6 L2 I7 L3 L0 L0 B0 L13 D0.0 L0 B0 B1 B1 B0 B0 B0 B0 B0 B0 L50 D0.0 D1.0 L0 R0,0,0 L50 L3 R0,0,255 B0 L50 L50 L50 B0 L50 CL! D1.0! L2 Tstrokes_ogs.tif B0 D1.0 D1.0"),
];

fn property(token: &str) -> Option<VisualStyleProperty> {
    let (token, enabled) = match token.strip_suffix('!') {
        Some(token) => (token, 0),
        None => (token, 1),
    };
    let value = match token.split_at(1) {
        ("L", v) => VisualStylePropertyValue::Long(v.parse().ok()?),
        ("S", v) => VisualStylePropertyValue::Short(v.parse().ok()?),
        ("D", v) => VisualStylePropertyValue::Double(v.parse().ok()?),
        ("B", v) => VisualStylePropertyValue::Bool(v == "1"),
        ("I", v) => VisualStylePropertyValue::Color(Color::Index(v.parse().ok()?)),
        ("R", v) => {
            let mut parts = v.split(',').map(|part| part.parse::<u8>().ok());
            let (r, g, b) = (parts.next()??, parts.next()??, parts.next()??);
            VisualStylePropertyValue::Color(Color::Rgb { r, g, b })
        }
        ("C", "L") => VisualStylePropertyValue::Color(Color::ByLayer),
        ("C", "B") => VisualStylePropertyValue::Color(Color::ByBlock),
        ("C", "N") => VisualStylePropertyValue::Color(Color::None),
        ("T", v) => VisualStylePropertyValue::Text(v.to_string()),
        _ => return None,
    };
    Some(VisualStyleProperty { value, enabled })
}

/// Adds the reference's visual styles to the drawing's ACAD_VISUALSTYLE
/// dictionary, keeping a style of the same name that is there already.
pub fn populate_document(document: &mut codec::CadDocument) {
    let dictionary = document.header.acad_visualstyle_dict_handle;
    if dictionary.is_null() {
        return;
    }
    for &(name, style_type, flm, flq, fcm, fm, em, es, internal, elm, props) in STYLES {
        let present = match document.objects.get(&dictionary) {
            Some(ObjectType::Dictionary(entries)) => entries.get(name).is_some(),
            _ => return,
        };
        if present {
            continue;
        }
        let Some(properties) = props.split(' ').map(property).collect::<Option<Vec<_>>>() else { continue };
        let handle = document.allocate_handle();
        let mut style = VisualStyle::new();
        style.handle = handle;
        style.owner = dictionary;
        style.reactors = vec![dictionary];
        style.description = name.to_string();
        style.style_type = style_type;
        style.face_lighting_model = flm;
        style.face_lighting_quality = flq;
        style.face_color_mode = fcm;
        style.face_modifier = fm;
        style.edge_model = em;
        style.edge_style = es;
        style.internal_use_only = internal;
        style.extended_lighting_model = elm;
        style.properties = properties;
        document.objects.insert(handle, ObjectType::VisualStyle(style));
        if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&dictionary) {
            entries.add_entry(name, handle);
        }
    }
}

/// The visual styles offered by name (not the internal ones), as the
/// reference lists them: the 2D wireframe style shows as "2D Wireframe".
pub fn offered(document: &codec::CadDocument) -> Vec<(String, codec::types::Handle)> {
    let Some(ObjectType::Dictionary(entries)) = document.objects.get(&document.header.acad_visualstyle_dict_handle) else {
        return Vec::new();
    };
    entries
        .entries
        .iter()
        .filter(|(_, handle)| match document.objects.get(handle) {
            Some(ObjectType::VisualStyle(style)) => !style.internal_use_only,
            _ => false,
        })
        .map(|(name, handle)| {
            let shown = if name.eq_ignore_ascii_case("2dWireframe") { "2D Wireframe".to_string() } else { name.clone() };
            (shown, *handle)
        })
        .collect()
}
