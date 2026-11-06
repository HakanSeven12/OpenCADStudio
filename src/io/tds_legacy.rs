//! The legacy render records a 3D Studio import adds beside the meshes: the
//! render settings (`AVE_RENDER`) and global material (`AVE_GLOBAL`) blocks
//! on the locked `ASHADE` layer, a material block (`RM_SDB`) with the
//! material's attributes for each converted material, and a light block
//! (`overhead`) at each point light — the records and their extended data
//! as the reference importer writes them.

use document::entities::{Arc, AttributeDefinition, EntityType, HorizontalAlignment, Line};
use document::types::{Color, Vector3};
use document::xdata::{ExtendedDataRecord, XDataValue};

/// The layer and text style the records use.
pub const LAYER: &str = "ASHADE";
/// The font of the text style of the same name.
pub const FONT: &str = "simplex.shx";

/// The render settings records, by insert colour.
pub const RENDER_SETTINGS: [(u8, &str); 8] = [
    (1, "1002={|1070=7|1000=Pref|1070=50|1070=20|1070=1|1070=0|1000=|1070=10|1070=1|1070=10|1040=1.0|1070=1|1040=0.6999999880790711|1040=1.0|1040=0.2000000029802322|1040=2.0|1010=-1.0,-1.0,-1.0|1040=0.3000000119209289|1070=49|1070=0|1070=0|1010=1.0,1.0,1.0|1040=45.0|1070=1|1070=0|1000=WCS|1040=0.0|1070=0|1040=10.0|1002=}"),
    (3, "1002={|1070=0|1000=Full Opt|1070=0|1070=1|1070=1|1070=1|1070=0|1070=0|1070=0|1070=1|1070=1|1040=0.0001|1040=0.0001|1002=}"),
    (4, "1002={|1070=0|1000=Quick Opt|1070=0|1070=1|1070=1|1070=1|1070=0|1070=0|1070=1|1002=}"),
    (5, "1002={|1070=1|1000=Scanl Opt|1070=2|1070=0|1070=2|1040=2.0|1040=4.0|1070=0|1070=1|1070=0|1040=0.03|1002=}"),
    (6, "1002={|1070=2|1000=Raytr Opt|1070=2|1070=0|1070=2|1040=2.0|1040=4.0|1070=0|1070=1|1070=1|1040=0.03|1070=3|1040=0.03|1002=}"),
    (7, "1002={|1070=0|1000=RFile Opt|1070=1|1070=640|1070=480|1070=10000|1070=1|1070=8|1070=8|1070=0|1070=0|1070=0|1070=0|1070=0|1070=0|1070=0|1002=}"),
    (8, "1002={|1070=0|1000=Fog Opt|1070=0|1070=0|1040=0.0|1040=1.0|1040=0.0|1040=1.0|1010=0.5,0.5,0.5|1002=}"),
    (9, "1002={|1070=0|1000=BG Opt|1070=0|1070=1|1010=0.0,0.0,0.0|1010=0.0,0.0,1.0|1010=0.0,1.0,0.0|1010=1.0,0.0,0.0|1040=0.5|1040=0.5|1040=0.0|1000=|1070=1|1000=|1070=1|1070=1|1040=1.0|1040=1.0|1040=0.0|1040=0.0|1070=0|1002=}"),
];

/// The global material record.
pub const GLOBAL_MATERIAL: &str = "1002={|1000=*GLOBAL*|1000=2|1000=Global finish.|1002={|1002=}|1002={|1070=63|1002={|1002={|1010=-1.0,-1.0,-1.0|1002=}|1040=0.6999999880790711|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002={|1002={|1010=-1.0,-1.0,-1.0|1002=}|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002={|1002={|1010=-1.0,-1.0,-1.0|1002=}|1040=0.2000000029802322|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002={|1070=0|1002={|1070=1|1002={|1040=1.0|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002=}|1040=1.0|1040=0.0|1040=0.0|1002=}|1002={|1040=1.0|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002={|1040=0.5|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002={|1070=0|1040=0.0|1002={|1002=}|1040=1.0|1002={|1002=}|1040=0.0|1002=}|1002={|1040=0.0|1002={|1002={|1010=0.0,0.0,0.0|1002=}|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002=}|1002={|1002={|1070=1|1002={|1040=0.0|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002=}|1040=0.0|1002={|1002=}|1010=1.0,0.0,0.0|1010=0.0,1.0,0.0|1010=0.0,0.0,1.0|1070=0|1002=}|1002={|1002={|1040=1.0|1040=1.0|1002={|1070=1|1002={|1040=1.0|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002=}|1002=}|1070=0|1040=0.0|1002={|1002=}|1002=}|1002={|1002=}|1002=}|1002={|1071=3|1070=3|1070=0|1002={|1070=2|1070=0|1070=0|1040=0.0|1002=}|1070=0|1070=2|1002={|1002=}|1002=}|1002={|1040=8030.0|1040=190.3000030517578|1040=0.3050000071525574|1040=228.0|1040=545.0|1040=14.0|1040=17.39999961853027|1040=0.4560000002384186|1002=}|1000=|1002=}";

/// A material record: `{N}` name, `{A}`/`{D}`/`{S}` ambient, diffuse and
/// specular, `{F}` the flags, `{M}` the diffuse map and its strength, `{O}`
/// the opacity, `{R}` the roughness, `{SH}` the shading, `{X}` the eight
/// trailing reals, `{H}` the material object's handle.
const MATERIAL: &str = "1002={|1000={N}|1000=2|1000=|1002={|1002=}|1002={|1070={F}|1002={|1002={|1010={A}|1002=}|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002={|1002={|1010={D}|1002=}|1040=1.0|1000=|1040=0.0|{M}|1002={|1002=}|1040=0.0|1002=}|1002={|1002={|1010={S}|1002=}|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002={|1070=0|1002={|1070=1|1002={|1040={O}|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002=}|1040=0.0|1040=0.0|1040=0.0|1002=}|1002={|1040=0.0|1040=0.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002={|1040={R}|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002={|1070=0|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002={|1040=0.0|1002={|1002={|1010=0.0,0.0,0.0|1002=}|1040=0.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002=}|1002={|1002={|1070=1|1002={|1040=0.0|1040=1.0|1000=|1040=0.0|1002={|1002=}|1040=0.0|1002={|1002=}|1040=0.0|1002=}|1002=}|1040=0.0|1002={|1002=}|1010=0.0,0.0,0.0|1010=0.0,0.0,0.0|1010=0.0,0.0,0.0|1070=0|1002=}|1002={|1002={|1040=0.0|1040=0.0|1002={|1070=0|1002=}|1002=}|1070=0|1040=0.0|1002={|1002=}|1002=}|1002={|1002=}|1002=}|1002={|1071=0|1070=3|1070=0|1002={|1070={SH}|1070=1|1070=0|1040=0.0|1002=}|1070=0|1070=0|1002={|1002=}|1002=}|1002={|{X}|1002=}|1000={H}|1002=}";

/// The trailing reals of the first material record of an import; later
/// ones are zero.
const FIRST_MATERIAL_TAIL: &str = "1040=8030.0|1040=190.3000030517578|1040=0.3050000071525574|1040=228.0|1040=545.0|1040=14.0|1040=17.39999961853027|1040=0.4560000002384186";

/// A light record: `{N}` name, `{C}` colour.
const LIGHT: &str = "1002={|1070=0|1000={N}|1011=0.0,0.0,0.0|1041=1.0|1010={C}|1002={|1070=544|1070=0|1040=0.0|1040=0.0|1040=0.0|1002=}|1002={|1070=9|1070=21|1070=15|1070=0|1040=37.62|1040=122.37|1002=}|1002=}";

/// The render material link every imported mesh carries.
pub const ENTITY_MATERIAL: &str = "1002={|1000=|1002={|1071=0|1070=0|1070=0|1002={|1070=0|1070=0|1070=0|1040=0.0|1002=}|1070=0|1070=0|1002={|1002=}|1002=}|1002={|1002=}|1002={|1002=}|1011=0.0,0.0,0.0|1011=1.0,0.0,0.0|1011=0.0,1.0,0.0|1011=0.0,0.0,1.0|1002=}";

/// A mesh's link to its material block reference, when the materials
/// stay out of the objects.
pub fn finish_xdata(material_record: document::Handle) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new("AVE_FINISH");
    record.add_value(XDataValue::ControlString("{".into()));
    record.add_value(XDataValue::Integer16(0));
    record.add_value(XDataValue::Handle(material_record));
    record.add_value(XDataValue::ControlString("}".into()));
    record
}

/// Extended data for `app` from `code=value` items separated by `|`
/// (points as `x,y,z`).
pub fn xdata(app: &str, spec: &str) -> ExtendedDataRecord {
    xdata_with(app, spec, &[])
}

/// [`xdata`] with each `1000=` item whose value is a placeholder of
/// `strings` holding that string as it is: names never pass through the
/// spec, so one holding `|` or `{A}` comes out unchanged.
fn xdata_with(app: &str, spec: &str, strings: &[(&str, &str)]) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(app);
    for item in spec.split('|') {
        let Some((code, value)) = item.split_once('=') else { continue };
        let real = || value.parse::<f64>().unwrap_or(0.0);
        let point = || {
            let mut v = value.split(',').map(|c| c.parse::<f64>().unwrap_or(0.0));
            Vector3::new(
                v.next().unwrap_or(0.0),
                v.next().unwrap_or(0.0),
                v.next().unwrap_or(0.0),
            )
        };
        record.add_value(match code {
            "1000" => XDataValue::String(
                strings
                    .iter()
                    .find(|(key, _)| *key == value)
                    .map_or(value, |(_, string)| *string)
                    .to_string(),
            ),
            "1002" => XDataValue::ControlString(value.to_string()),
            "1010" => XDataValue::Point3D(point()),
            "1011" => XDataValue::Position3D(point()),
            "1040" => XDataValue::Real(real()),
            "1041" => XDataValue::Distance(real()),
            "1070" => XDataValue::Integer16(value.parse().unwrap_or(0)),
            "1071" => XDataValue::Integer32(value.parse().unwrap_or(0)),
            _ => continue,
        });
    }
    record
}

fn triple(c: [f64; 3]) -> String {
    format!("{},{},{}", c[0], c[1], c[2])
}

/// A material's record: its ambient, diffuse and specular colours from 0
/// to 1, `first` for the first one an import converts, `handle` the
/// material object's.
pub fn material_xdata(
    name: &str,
    [ambient, diffuse, specular]: [[f64; 3]; 3],
    finish: &MaterialFinish,
    first: bool,
    handle: &str,
) -> ExtendedDataRecord {
    let tail = if first {
        FIRST_MATERIAL_TAIL.to_string()
    } else {
        ["1040=0.0"; 8].join("|")
    };
    // A transparent material sets the transparency bit of the flags.
    let flags = if finish.opacity < 1.0 { 47 } else { 39 };
    let map = match &finish.map {
        Some((_, strength)) => format!("{DIFFUSE_MAP}|1040={}", f64::from(*strength)),
        None => "1002={|1002=}|1040=0.0".to_string(),
    };
    let spec = MATERIAL
        .replace("{A}", &triple(ambient))
        .replace("{D}", &triple(diffuse))
        .replace("{S}", &triple(specular))
        .replace("{F}", &flags.to_string())
        .replace("{M}", &map)
        .replace("{O}", &f64::from(finish.opacity).to_string())
        .replace("{R}", &f64::from(finish.roughness).to_string())
        .replace("{SH}", &finish.shading.to_string())
        .replace("{X}", &tail);
    let file = finish.map.as_ref().map(|(file, _)| file.as_str()).unwrap_or("");
    xdata_with("AVE_MATERIAL", &spec, &[("{N}", name), ("{H}", handle), ("{T}", file)])
}

/// A diffuse texture map's record, `{T}` its file name (scale 1 and offset
/// 0 in both directions, tiled).
const DIFFUSE_MAP: &str = "1002={|1002={|1070=1|1002={|1002={|1000={T}|1002={|1002=}|1070=3|1070=0|1002={|1002=}|1002=}|1002={|1002={|1040=1.0|1040=0.0|1070=1|1002=}|1002={|1040=1.0|1040=0.0|1070=1|1002=}|1040=0.0|1070=0|1002=}|1002=}|1002=}|1002={|1002=}|1002=}";

/// What a 3D Studio material adds to its colours.
pub struct MaterialFinish {
    /// One less the shininess (single precision, as the reference keeps it).
    pub roughness: f32,
    /// One less the transparency.
    pub opacity: f32,
    /// The shading type less one: Gouraud (1) when the file gives none.
    pub shading: u16,
    /// The diffuse map's file name and strength.
    pub map: Option<(String, f32)>,
}

impl MaterialFinish {
    pub fn of(material: Option<&super::tds_import::TdsMaterial>) -> Self {
        Self {
            roughness: 1.0 - material.and_then(|m| m.shininess).unwrap_or(0.0),
            opacity: 1.0 - material.and_then(|m| m.transparency).unwrap_or(0.0),
            shading: material.and_then(|m| m.shading).unwrap_or(2).saturating_sub(1),
            map: material.and_then(|m| m.map.clone()),
        }
    }
}

/// A light's record.
pub fn light_xdata(name: &str, color: [f64; 3]) -> ExtendedDataRecord {
    xdata_with(
        "AVE_RENDER",
        &LIGHT.replace("{C}", &triple(color)),
        &[("{N}", name)],
    )
}

/// The material block's attribute tags, in order.
const MATERIAL_TAGS: [&str; 23] = [
    "NAME",
    "ACADCOLOR",
    "COLOR",
    "OPACITY",
    "SURFACE",
    "SURF1",
    "SURF2",
    "SURF3",
    "SURF4",
    "SHADINGRATE",
    "DISPLACEMENT",
    "DISP1",
    "DISP2",
    "DISP3",
    "DISP4",
    "DBOUND",
    "SMOOTH",
    // The legacy tag is spelled so.
    "ATMOSHPERE",
    "ATMO1",
    "ATMO2",
    "ATMO3",
    "ATMO4",
    "TCCORD",
];

/// The material block: one hidden attribute per tag.
pub fn material_block() -> Vec<EntityType> {
    MATERIAL_TAGS
        .iter()
        .map(|tag| {
            let mut def = AttributeDefinition::new(tag.to_string(), String::new(), String::new());
            def.common.layer = "0".into();
            def.height = 0.2;
            def.flags.invisible = true;
            EntityType::AttributeDefinition(def)
        })
        .collect()
}

/// The material block's attribute values for a material whose diffuse
/// colour is `diffuse` (0 to 1); `asked` when the meshes also link to the
/// record through their finish.
pub fn material_values(
    name: &str,
    diffuse: [f64; 3],
    roughness: f32,
    asked: bool,
) -> Vec<(&'static str, String)> {
    MATERIAL_TAGS
        .iter()
        .map(|tag| {
            let value = match *tag {
                "NAME" => name.to_string(),
                "ACADCOLOR" => if asked { " ".into() } else { String::new() },
                "COLOR" => format!("{:.4},{:.4},{:.4}", diffuse[0], diffuse[1], diffuse[2]),
                "OPACITY" => "1.0,1.0,1.0".into(),
                "SURFACE" => r#"("nullsurf" "")"#.into(),
                // The roughness in its shortest decimal form ("0.9", "1").
                "SURF1" => format!(r#"(11 "Ka" "1")(11 "Kd" "1")(11 "Ks" "1")(11 "roughness" "{roughness}")"#),
                "SHADINGRATE" => "-1.0".into(),
                "DISPLACEMENT" => r#"("nulldisp" "")"#.into(),
                "DBOUND" => "0.0".into(),
                "SMOOTH" => "0".into(),
                "ATMOSHPERE" => r#"("" "")"#.into(),
                "TCCORD" => "0.0 0.0 1.0 0.0 0.0 1.0 1.0 1.0".into(),
                _ => String::new(),
            };
            (*tag, value)
        })
        .collect()
}

fn on_layer(mut entity: EntityType, color: u8) -> EntityType {
    let common = entity.common_mut();
    common.layer = LAYER.into();
    common.linetype = "Continuous".into();
    common.color = Color::Index(color);
    entity
}

fn line(a: [f64; 2], b: [f64; 2]) -> EntityType {
    on_layer(
        EntityType::Line(Line::from_points(
            Vector3::new(a[0], a[1], 0.0),
            Vector3::new(b[0], b[1], 0.0),
        )),
        7,
    )
}

fn arc(start: f64, end: f64) -> EntityType {
    let mut arc = Arc::new();
    arc.radius = 0.3;
    arc.start_angle = start.to_radians();
    arc.end_angle = end.to_radians();
    on_layer(EntityType::Arc(arc), 7)
}

/// The light block: a bulb symbol, its name centred under it and the
/// light's hidden settings.
pub fn light_block() -> Vec<EntityType> {
    let (a, b, c, d) = (
        0.2041413561583015,
        0.2198324514393848,
        0.2721884748777353,
        0.2931099352525131,
    );
    let (e, f, g, h) = (
        0.1096591641591904,
        0.2792398032442862,
        0.1279356915190555,
        0.3257797704516673,
    );
    let (k, m, n) = (0.2703701166919155, 0.3154318028072347, 0.1516666666666666);
    let setting = |tag: &str, prompt: &str, value: &str, y: f64| {
        let mut def = AttributeDefinition::new(tag.to_string(), prompt.to_string(), value.to_string());
        def.insertion_point = Vector3::new(0.0, y, 0.0);
        def.height = 0.01;
        def.flags.invisible = true;
        on_layer(EntityType::AttributeDefinition(def), 7)
    };
    let mut name = AttributeDefinition::new("SNAME".into(), "Light Name".into(), String::new());
    name.insertion_point = Vector3::new(0.0, -0.1, 0.0);
    name.alignment_point = name.insertion_point;
    name.height = 0.2;
    name.text_style = LAYER.into();
    name.horizontal_alignment = HorizontalAlignment::Center;
    vec![
        on_layer(EntityType::AttributeDefinition(name), 1),
        arc(205.6792886194569, 334.3207113805431),
        line([-a, -b], [-c, -d]),
        line([-e, -f], [-g, -h]),
        line([0.0, -0.3], [0.0, -0.4]),
        line([e, -f], [g, -h]),
        line([a, -b], [c, -d]),
        arc(25.67928861945685, 154.3207113805431),
        line([-k, 0.13], [-m, n]),
        line([-k, -0.13], [-m, -n]),
        line([-a, b], [-c, d]),
        line([-e, f], [-g, h]),
        line([k, -0.13], [m, -n]),
        line([k, 0.13], [m, n]),
        line([0.0, 0.3], [0.0, 0.4]),
        setting("INTENSITY", "Light Intensity", "1", 0.22),
        setting("BEAMD", "*not used*", "-1", 0.16),
        setting("CONED", "*not used*", "-1", 0.175),
        setting("CONEA", "Depth Map Size", "0", 0.19),
        setting("COLOR", "Light Color (RGB)", "1,1,1", 0.205),
        setting("LAZ", "Look-at Z", "", 0.235),
        setting("LAY", "Look-at Y", "", 0.25),
        setting("LAX", "Look-at X", "", 0.265),
        setting("GNAME", "", "", 0.28),
        line([e, f], [g, h]),
        line([a, b], [c, d]),
    ]
}

/// A spotlight's record: its target, and the cone from its hotspot and
/// falloff angles (degrees).
pub fn spot_xdata(name: &str, target: [f64; 3], color: [f64; 3], hotspot: f64, falloff: f64) -> ExtendedDataRecord {
    let spec = LIGHT
        .replace("{C}", &triple(color))
        .replace("1011=0.0,0.0,0.0", &format!("1011={}", triple(target)))
        .replace(
            "1040=0.0|1040=0.0|1040=0.0",
            &format!("1040={}|1040={}|1040=0.0", falloff / 2.0, (falloff - hotspot) / 2.0),
        );
    xdata_with("AVE_RENDER", &spec, &[("{N}", name)])
}

/// The spotlight block: a lamp symbol aimed along its Y axis, its name
/// centred on it and the light's hidden settings.
pub fn spot_block() -> Vec<EntityType> {
    let (p, q, r) = (0.1045454545454545, 0.301304347826087, 0.3399999999999999);
    let setting = |tag: &str, prompt: &str, value: &str, y: f64| {
        let mut def = AttributeDefinition::new(tag.to_string(), prompt.to_string(), value.to_string());
        def.insertion_point = Vector3::new(0.0, y, 0.0);
        def.height = 0.01;
        def.flags.invisible = true;
        on_layer(EntityType::AttributeDefinition(def), 7)
    };
    let mut name = AttributeDefinition::new("SNAME".into(), "Light Name".into(), String::new());
    name.insertion_point = Vector3::new(0.0, -0.1, 0.0);
    name.alignment_point = name.insertion_point;
    name.height = 0.2;
    name.text_style = LAYER.into();
    name.horizontal_alignment = HorizontalAlignment::Center;
    vec![
        line([0.25, 0.26], [-0.25, 0.26]),
        line([-0.25, -r], [0.25, -r]),
        on_layer(EntityType::AttributeDefinition(name), 1),
        line([-0.25, -0.13], [-0.25, -r]),
        line([-0.25, -0.18], [-0.29, -0.18]),
        line([-0.29, -0.22], [-0.25, -0.22]),
        line([-0.29, -0.18], [-0.29, -0.22]),
        line([-p, 0.26], [-0.15, 0.46]),
        line([-0.21, 0.26], [-q, 0.46]),
        line([-0.25, 0.26], [-0.25, 0.13]),
        line([-0.25, 0.22], [-0.29, 0.22]),
        line([-0.29, 0.18], [-0.25, 0.18]),
        line([-0.29, 0.22], [-0.29, 0.18]),
        setting("ZSIZE", "Depth Map Size", "0", -0.24),
        setting("LFXYZ", "Look-from XYZ", "0,0,0", -0.33),
        setting("BEAMDI", "Beam Distribution", "0.0", -0.315),
        setting("CONEDE", "Cone Delta", "0.0", -0.3),
        setting("CONEAN", "Cone Angle", "10.0", -0.285),
        setting("BIASMAX", "*not used*", "-1", -0.27),
        setting("BIASMIN", "*not used*", "-1", -0.255),
        setting("COLOR", "Light Color (RGB)", "1,1,1", -0.225),
        setting("INTENSITY", "Light Intensity", "1", -0.21),
        setting("GNAME", "", "", -0.15),
        setting("LAZ", "Look-at Z", "", -0.195),
        setting("LAY", "Look-at Y", "", -0.18),
        setting("LAX", "Look-at X", "", -0.165),
        line([0.25, -r], [0.25, -0.13]),
        line([0.25, -0.18], [0.29, -0.18]),
        line([0.29, -0.22], [0.25, -0.22]),
        line([0.29, -0.18], [0.29, -0.22]),
        line([0.21, 0.26], [q, 0.46]),
        line([p, 0.26], [0.15, 0.46]),
        line([0.0, 0.26], [0.0, 0.46]),
        line([0.25, 0.13], [0.25, 0.26]),
        line([0.25, 0.22], [0.29, 0.22]),
        line([0.29, 0.18], [0.25, 0.18]),
        line([0.29, 0.22], [0.29, 0.18]),
    ]
}
