// Drawing units — three separate questions the word "units" runs together, and
// the tables that answer them. (#668)
//
// *How lengths read* is LUNITS: decimal, architectural, and so on. It is the
// only one of the three that shows, since every coordinate and distance the
// drawing displays is written in that format, which is why it is what the
// status-bar button offers.
//
// *What one unit measures* is INSUNITS. A drawing's geometry is plain numbers;
// this is a label saying what those numbers count. Nothing moves when the label
// changes — its job is to tell *other* drawings how to read this one, which is
// what scales a block or an xref on the way in.
//
// *Converting* is therefore a separate act from relabelling, and the only one
// that touches geometry. It reads the same table as the label, so a unit the
// picker offers is a unit DWGUNITS can convert to.

// ── LUNITS — how lengths are written ───────────────────────────────────────

/// LUNITS code, the name it is offered under, the short form for the
/// status-bar button, and a sample of the format so the choice can be
/// recognised rather than remembered.
const LINEAR_FORMATS: &[(i16, &str, &str, &str)] = &[
    (4, "Architectural", "Arch", "2'9 1/2\""),
    (2, "Decimal", "Dec", "33.5000"),
    (3, "Engineering", "Eng", "2'-9.5000\""),
    (5, "Fractional", "Frac", "33 1/2"),
    (1, "Scientific", "Sci", "3.3500E+01"),
];

/// Every linear format, in picker order: code, name, sample.
pub fn linear_formats() -> impl Iterator<Item = (i16, &'static str, &'static str)> {
    LINEAR_FORMATS
        .iter()
        .map(|&(code, label, _, sample)| (code, label, sample))
}

/// The name of a linear format code.
pub fn linear_format_label(code: i16) -> &'static str {
    LINEAR_FORMATS
        .iter()
        .find(|&&(candidate, ..)| candidate == code)
        .map(|&(_, label, _, _)| label)
        .unwrap_or("Decimal")
}

/// The short form shown on the status-bar button.
pub fn linear_format_short(code: i16) -> &'static str {
    LINEAR_FORMATS
        .iter()
        .find(|&&(candidate, ..)| candidate == code)
        .map(|&(_, _, short, _)| short)
        .unwrap_or("Dec")
}

// ── AUNITS — how angles are written ────────────────────────────────────────

/// AUNITS code, the name it is offered under, and a sample of the format.
///
/// Each convention carries its own mark so a written angle says which one it
/// is: `g` for grads, `r` for radians, `d`/`'`/`"` for degrees-minutes-seconds,
/// and a bearing between two compass letters for surveyor's units.
const ANGULAR_FORMATS: &[(i16, &str, &str)] = &[
    (0, "Decimal Degrees", "45.0000"),
    (1, "Deg/Min/Sec", "45d0'0\""),
    (2, "Grads", "50.0000g"),
    (3, "Radians", "0.7854r"),
    (4, "Surveyor's Units", "N 45d0'0\" E"),
];

/// Every angular format, in picker order: code, name, sample.
pub fn angular_formats() -> impl Iterator<Item = (i16, &'static str, &'static str)> {
    ANGULAR_FORMATS.iter().copied()
}

// ── INSUNITS — what one unit measures ──────────────────────────────────────

/// INSUNITS code, the name it is offered under, and how many metres one unit
/// measures. `None` for unitless, which measures nothing and so converts to
/// nothing.
const UNITS: &[(i16, &str, &str, Option<f64>)] = &[
    (0, "Unitless", "Unitless", None),
    (4, "Millimeters", "mm", Some(0.001)),
    (5, "Centimeters", "cm", Some(0.01)),
    (6, "Meters", "m", Some(1.0)),
    (7, "Kilometers", "km", Some(1000.0)),
    (1, "Inches", "in", Some(0.0254)),
    (2, "Feet", "ft", Some(0.3048)),
    (3, "Miles", "mi", Some(1609.344)),
    (10, "Yards", "yd", Some(0.9144)),
];

/// Every unit on offer, in picker order: code and label.
pub fn all() -> impl Iterator<Item = (i16, &'static str)> {
    UNITS.iter().map(|&(code, label, _, _)| (code, label))
}

/// The full name of a unit code.
pub fn label(code: i16) -> &'static str {
    UNITS
        .iter()
        .find(|&&(candidate, ..)| candidate == code)
        .map(|&(_, label, _, _)| label)
        .unwrap_or("Unit")
}

/// The short form shown on the status-bar pill.
pub fn short(code: i16) -> &'static str {
    UNITS
        .iter()
        .find(|&&(candidate, ..)| candidate == code)
        .map(|&(_, _, short, _)| short)
        .unwrap_or("Unit")
}

/// The code a typed name or abbreviation stands for.
pub fn code_for_keyword(name: &str) -> Option<i16> {
    let name = name.trim();
    UNITS
        .iter()
        .find(|&&(_, label, short, _)| {
            label.eq_ignore_ascii_case(name) || short.eq_ignore_ascii_case(name)
        })
        .map(|&(code, ..)| code)
}

/// What to multiply this drawing's lengths by to say the same distances in
/// `to`.
///
/// `None` when either side is unitless: a drawing that does not say what its
/// numbers count cannot be converted, only relabelled.
pub fn conversion_factor(from: i16, to: i16) -> Option<f64> {
    let metres = |code: i16| {
        UNITS
            .iter()
            .find(|&&(candidate, ..)| candidate == code)
            .and_then(|&(_, _, _, metres)| metres)
    };
    let (from, to) = (metres(from)?, metres(to)?);
    (to > 0.0).then(|| from / to)
}

/// Handles of everything that lives in model space.
///
/// Paper space is deliberately left out. Its geometry is measured on the sheet,
/// in sheet units, and means the same thing however the model is labelled —
/// scaling it would resize the drawing frame along with the building.
pub fn model_space_handles(scene: &crate::scene::Scene) -> Vec<codec::Handle> {
    let Some(model) = scene.document.block_records.get("*Model_Space") else {
        return Vec::new();
    };
    let owner = model.handle;
    scene
        .document
        .entities()
        .filter(|entity| entity.common().owner_handle == owner)
        .map(|entity| entity.common().handle)
        .collect()
}
