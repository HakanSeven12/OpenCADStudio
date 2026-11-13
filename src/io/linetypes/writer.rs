//! Writes linetypes back to the `.lin` text format, the reverse of the parser
//! in the parent module.
//!
//! A linetype is written whole or not at all: anything the format cannot
//! represent is reported as a [`SkipReason`] instead of being written wrong.
//! Limits that only some other programs enforce are reported as a
//! [`LinWarning`], and the definition is kept intact.

use super::{bundled_shape_name, find_shape_file, parse};
use document::tables::linetype::{LineTypeComplexContent, LineTypeComplexData};
use document::tables::LineType;
use document::types::Handle;
use document::Document;
use std::io;
use std::path::{Path, PathBuf};

const MAX_LINE_CHARS: usize = 80;
const MAX_DESCRIPTION_CHARS: usize = 47;
const MAX_ELEMENTS: usize = 12;
/// The file the user's saved linetypes go to, in the `linetypes` folder.
#[cfg(not(target_arch = "wasm32"))]
const LIBRARY_FILE: &str = "user.lin";

/// Why a linetype was left out of the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    NoPattern,
    InvalidName,
    InvalidDescription,
    InvalidLength,
    UnwritableText,
    UnknownShape,
    MissingStyle,
}

impl SkipReason {
    pub fn label(self) -> String {
        match self {
            Self::NoPattern => crate::t!("no pattern to write"),
            Self::InvalidName => crate::t!("the name cannot be stored in a linetype file"),
            Self::InvalidDescription => crate::t!("the description holds control characters"),
            Self::InvalidLength => crate::t!("a length is not a finite number"),
            Self::UnwritableText => crate::t!("its text holds a quote, a bracket or a line break"),
            Self::UnknownShape => crate::t!("its shape cannot be identified"),
            Self::MissingStyle => crate::t!("its text style is missing or has an unusable name"),
        }
        .into_owned()
    }
}

/// A limit other programs apply to `.lin` files that this definition goes past.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinWarning {
    LongDescription,
    LongLine,
    TooManyElements,
    LeadingGap,
    NonAscii,
}

impl LinWarning {
    pub fn label(self) -> String {
        match self {
            Self::LongDescription => crate::t!("description over 47 characters"),
            Self::LongLine => crate::t!("pattern line over 80 characters"),
            Self::TooManyElements => crate::t!("more than 12 pattern elements"),
            Self::LeadingGap => crate::t!("pattern starts with a gap"),
            Self::NonAscii => crate::t!("non-ASCII characters"),
        }
        .into_owned()
    }
}

/// `.lin` text and what happened to each linetype on the way to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinOutput {
    pub text: String,
    pub written: Vec<String>,
    pub skipped: Vec<(String, SkipReason)>,
    pub warnings: Vec<(String, LinWarning)>,
}

pub fn write_linetypes<'a>(
    doc: &Document,
    linetypes: impl IntoIterator<Item = &'a LineType>,
) -> LinOutput {
    let mut output = LinOutput::default();
    for linetype in linetypes {
        match definition(doc, linetype) {
            Ok((text, warnings)) => {
                output.text.push_str(&text);
                output.written.push(linetype.name.clone());
                output.warnings.extend(
                    warnings
                        .into_iter()
                        .map(|warning| (linetype.name.clone(), warning)),
                );
            }
            Err(reason) => output.skipped.push((linetype.name.clone(), reason)),
        }
    }
    output
}

/// The `*NAME,description` and `A,...` lines of one linetype.
fn definition(
    doc: &Document,
    linetype: &LineType,
) -> Result<(String, Vec<LinWarning>), SkipReason> {
    if linetype.elements.is_empty() {
        return Err(SkipReason::NoPattern);
    }
    let name = &linetype.name;
    if name.trim() != name || !crate::scene::valid_block_name(name) {
        return Err(SkipReason::InvalidName);
    }
    let description = linetype.description.trim();
    if description.chars().any(char::is_control) {
        return Err(SkipReason::InvalidDescription);
    }
    let mut pattern = String::from("A");
    for element in &linetype.elements {
        if !element.length.is_finite() {
            return Err(SkipReason::InvalidLength);
        }
        pattern.push(',');
        pattern.push_str(&number(element.length));
        if let Some(extra) = element.complex.as_ref().filter(|data| is_real(data)) {
            pattern.push(',');
            pattern.push_str(&complex_element(doc, extra)?);
        }
    }
    let mut warnings = Vec::new();
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        warnings.push(LinWarning::LongDescription);
    }
    if pattern.chars().count() > MAX_LINE_CHARS {
        warnings.push(LinWarning::LongLine);
    }
    if linetype.elements.len() > MAX_ELEMENTS {
        warnings.push(LinWarning::TooManyElements);
    }
    if linetype.elements[0].length < 0.0 {
        warnings.push(LinWarning::LeadingGap);
    }
    if !(name.is_ascii() && description.is_ascii() && pattern.is_ascii()) {
        warnings.push(LinWarning::NonAscii);
    }
    let header = if description.is_empty() {
        format!("*{name}")
    } else {
        format!("*{name},{description}")
    };
    Ok((format!("{header}\n{pattern}\n"), warnings))
}

/// Some DWGs hang a placeholder that draws nothing on a plain dash.
fn is_real(data: &LineTypeComplexData) -> bool {
    match &data.content {
        LineTypeComplexContent::Text { text } => !text.trim().is_empty(),
        LineTypeComplexContent::Shape { .. } => !data.style_handle.is_null(),
    }
}

fn number(value: f64) -> String {
    if value == 0.0 {
        "0".to_owned()
    } else {
        value.to_string()
    }
}

fn complex_element(doc: &Document, data: &LineTypeComplexData) -> Result<String, SkipReason> {
    let head = match &data.content {
        LineTypeComplexContent::Text { text } => {
            if text
                .chars()
                .any(|c| matches!(c, '"' | '[' | ']') || c.is_control())
            {
                return Err(SkipReason::UnwritableText);
            }
            let style = text_style_name(doc, data.style_handle)?;
            format!("\"{text}\",{style}")
        }
        LineTypeComplexContent::Shape { shape_number } => {
            shape_reference(doc, data.style_handle, *shape_number)?
        }
    };
    let mut element = format!("[{head}");
    if data.scale != 1.0 {
        element.push_str(&format!(",S={}", number(data.scale)));
    }
    let degrees = (data.rotation.to_degrees() * 1.0e6).round() / 1.0e6;
    if data.upright {
        element.push_str(&format!(",U={}", number(degrees)));
    } else if data.absolute_rotation {
        element.push_str(&format!(",A={}", number(degrees)));
    } else if degrees != 0.0 {
        element.push_str(&format!(",R={}", number(degrees)));
    }
    if data.offset[0] != 0.0 {
        element.push_str(&format!(",X={}", number(data.offset[0])));
    }
    if data.offset[1] != 0.0 {
        element.push_str(&format!(",Y={}", number(data.offset[1])));
    }
    element.push(']');
    Ok(element)
}

fn usable_in_element(name: &str) -> bool {
    !name.trim().is_empty()
        && !name
            .chars()
            .any(|c| matches!(c, ',' | '"' | '[' | ']') || c.is_control())
}

fn text_style_name(doc: &Document, handle: Handle) -> Result<String, SkipReason> {
    doc.text_styles
        .iter()
        .find(|style| style.handle == handle && !style.is_shape_file)
        .map(|style| style.name.clone())
        .filter(|name| usable_in_element(name))
        .ok_or(SkipReason::MissingStyle)
}

/// `NAME,file.shx` for a shape stored by number under a shape-file style.
fn shape_reference(doc: &Document, handle: Handle, number: i16) -> Result<String, SkipReason> {
    let file = doc
        .text_styles
        .iter()
        .find(|style| style.handle == handle && style.is_shape_file)
        .map(|style| style.font_file.clone())
        .ok_or(SkipReason::UnknownShape)?;
    let name = bundled_shape_name(&file, number)
        .map(str::to_owned)
        .or_else(|| {
            let path = find_shape_file(&file, None).filter(|_| !file.is_empty())?;
            crate::scene::text::shx::shape_name(&path, u16::try_from(number).ok()?)
        })
        .filter(|name| usable_in_element(name))
        .ok_or(SkipReason::UnknownShape)?;
    let base = file.rsplit(['/', '\\']).next().unwrap_or(&file);
    let shape_file = match base {
        "" => "ltypeshp.shx".to_owned(),
        base if base.to_ascii_lowercase().ends_with(".shx") => base.to_owned(),
        base => format!("{base}.shx"),
    };
    if !usable_in_element(&shape_file) {
        return Err(SkipReason::UnknownShape);
    }
    Ok(format!("{name},{shape_file}"))
}

/// Write through a temporary file beside `path`, so a failure leaves whatever
/// was there intact.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let result = std::fs::write(&temporary, text).and_then(|()| std::fs::rename(&temporary, path));
    if result.is_err() && temporary.is_file() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// The web build has no user folder.
#[cfg(target_arch = "wasm32")]
pub fn library_path() -> Option<PathBuf> {
    None
}

/// The user's own linetype library.
#[cfg(not(target_arch = "wasm32"))]
pub fn library_path() -> Option<PathBuf> {
    crate::config::config_dir().map(|dir| dir.join("linetypes").join(LIBRARY_FILE))
}

/// Why a linetype could not go into a library file.
#[derive(Clone, Debug, PartialEq)]
pub enum SaveError {
    NotFound,
    Skipped(SkipReason),
    /// The bundled catalog or another library file owns the name, so a copy
    /// here would never be used.
    Reserved,
    Io(String),
}

/// Add linetype `name` to the library file at `path`, or replace its
/// definition there. `reserved` says whether the catalog already has a name.
pub fn save_definition(
    path: &Path,
    doc: &Document,
    name: &str,
    reserved: impl Fn(&str) -> bool,
) -> Result<LinOutput, SaveError> {
    let linetype = doc.line_types.get(name.trim()).ok_or(SaveError::NotFound)?;
    let output = write_linetypes(doc, [linetype]);
    if let Some((_, reason)) = output.skipped.first() {
        return Err(SaveError::Skipped(*reason));
    }
    // The rest of the file is written back as it is read: a file that is not
    // UTF-8 would come back with its other definitions mangled.
    let existing = match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| {
            SaveError::Io(crate::t!("the library file is not UTF-8 text").into_owned())
        })?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(SaveError::Io(error.to_string())),
    };
    let ours = parse(&existing)
        .iter()
        .any(|known| known.name.eq_ignore_ascii_case(&linetype.name));
    if !ours && reserved(&linetype.name) {
        return Err(SaveError::Reserved);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| SaveError::Io(error.to_string()))?;
    }
    let updated = replace_definition(&existing, &linetype.name, &output.text);
    write_atomic(path, &updated).map_err(|error| SaveError::Io(error.to_string()))?;
    Ok(output)
}

/// `source` with the definition of `name` swapped for `block`, or `block`
/// added at the end. Comments and other definitions stay as they are.
fn replace_definition(source: &str, name: &str, block: &str) -> String {
    let is_header = |line: &str, wanted: &str| {
        line.trim().strip_prefix('*').is_some_and(|rest| {
            rest.split(',')
                .next()
                .is_some_and(|found| found.trim().eq_ignore_ascii_case(wanted))
        })
    };
    let mut out = String::new();
    let mut replaced = false;
    let mut skipping = false;
    for line in source.lines() {
        if skipping {
            let trimmed = line.trim();
            if trimmed.starts_with('*') {
                skipping = false;
            } else {
                if trimmed.to_ascii_uppercase().starts_with('A') {
                    skipping = false;
                }
                continue;
            }
        }
        if !replaced && is_header(line, name) {
            out.push_str(block);
            replaced = true;
            skipping = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !replaced {
        out.push_str(block);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{parse, parse_complex, populate_document_from_source, Catalog, LIN_SOURCE};
    use super::*;
    use document::tables::linetype::LineTypeElement;
    use document::tables::TextStyle;

    fn simple(name: &str, description: &str, pattern: &[f64]) -> LineType {
        let mut linetype = LineType::new(name);
        linetype.description = description.to_owned();
        for length in pattern {
            linetype.add_element(LineTypeElement {
                length: *length,
                complex: None,
            });
        }
        linetype.pattern_length = pattern.iter().map(|v| v.abs()).sum();
        linetype
    }

    fn with_style(doc: &mut Document, name: &str) -> Handle {
        let mut style = TextStyle::new(name);
        style.handle = doc.allocate_handle();
        let handle = style.handle;
        doc.text_styles.add(style).unwrap();
        handle
    }

    fn shape_style(doc: &mut Document, file: &str) -> Handle {
        let mut style = TextStyle::new("");
        style.handle = doc.allocate_handle();
        style.is_shape_file = true;
        style.font_file = file.to_owned();
        let handle = style.handle;
        doc.text_styles.add_allow_duplicate(style);
        handle
    }

    fn text_element(length: f64, text: &str, style: Handle) -> LineTypeElement {
        LineTypeElement {
            length,
            complex: Some(LineTypeComplexData {
                content: LineTypeComplexContent::Text { text: text.into() },
                style_handle: style,
                scale: 2.54,
                offset: [-2.5, -1.25],
                ..Default::default()
            }),
        }
    }

    fn lengths(linetype: &LineType) -> Vec<f64> {
        linetype.elements.iter().map(|e| e.length).collect()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ocs-lin-writer-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn only_written(doc: &Document, linetype: &LineType) -> String {
        let output = write_linetypes(doc, [linetype]);
        assert_eq!(output.skipped, vec![], "{output:?}");
        output.text
    }

    fn reason_for(doc: &Document, linetype: &LineType) -> SkipReason {
        let output = write_linetypes(doc, [linetype]);
        assert_eq!(output.written, Vec::<String>::new(), "{output:?}");
        assert_eq!(output.text, "");
        output.skipped[0].1
    }

    #[test]
    fn a_simple_pattern_comes_back_exactly() {
        let doc = Document::new();
        for pattern in [
            vec![1.5, -0.25, 0.5, -0.75],
            vec![1.5, -0.25, 0.0, -0.75],
            vec![0.0, -0.3],
            vec![1.0e-7, -123456.789],
        ] {
            let linetype = simple("ASYM", "Asymmetric", &pattern);
            let text = only_written(&doc, &linetype);
            let back = parse(&text);
            assert_eq!(back.len(), 1, "{text}");
            assert_eq!(back[0].name, "ASYM");
            assert_eq!(back[0].description, "Asymmetric");
            assert_eq!(lengths(&back[0]), pattern, "{text}");
        }
    }

    #[test]
    fn a_description_is_optional_and_may_hold_commas() {
        let doc = Document::new();
        let bare = only_written(&doc, &simple("PLAIN", "", &[1.0, -0.5]));
        assert_eq!(bare, "*PLAIN\nA,1,-0.5\n");
        assert_eq!(parse(&bare)[0].description, "");
        let commas = only_written(&doc, &simple("C", "One, two, three", &[1.0, -0.5]));
        assert_eq!(parse(&commas)[0].description, "One, two, three");
    }

    /// Every bundled linetype survives parse, write and parse, the complex
    /// ones with their text and shapes.
    #[test]
    fn the_whole_catalog_round_trips() {
        let mut first = Document::new();
        populate_document_from_source(&mut first, LIN_SOURCE, None);
        let wanted: Vec<&LineType> = first
            .line_types
            .iter()
            .filter(|linetype| !linetype.elements.is_empty())
            .collect();
        let bundled = parse(LIN_SOURCE).len();
        assert_eq!(wanted.len(), bundled, "every definition was loaded");
        let output = write_linetypes(&first, wanted.iter().copied());
        assert_eq!(output.skipped, vec![]);

        let mut second = Document::new();
        populate_document_from_source(&mut second, &output.text, None);
        let mut complex = 0;
        for original in &wanted {
            let copy = second
                .line_types
                .get(&original.name)
                .unwrap_or_else(|| panic!("{} lost", original.name));
            assert_eq!(copy.description, original.description, "{}", original.name);
            assert_eq!(lengths(copy), lengths(original), "{}", original.name);
            for (a, b) in original.elements.iter().zip(&copy.elements) {
                match (&a.complex, &b.complex) {
                    (None, None) => {}
                    (Some(a), Some(b)) => {
                        complex += 1;
                        assert_eq!(a.content, b.content, "{}", original.name);
                        assert_eq!(a.scale, b.scale, "{}", original.name);
                        assert_eq!(a.offset, b.offset, "{}", original.name);
                        assert!((a.rotation - b.rotation).abs() < 1e-6, "{}", original.name);
                        assert_eq!(a.absolute_rotation, b.absolute_rotation);
                        assert_eq!(a.upright, b.upright);
                    }
                    (a, b) => panic!("{}: {a:?} became {b:?}", original.name),
                }
            }
        }
        let authored = parse_complex(LIN_SOURCE).len();
        assert_eq!(authored, 12, "the catalog's complex linetypes");
        assert!(complex >= authored, "{complex} complex elements");
    }

    #[test]
    fn text_keeps_its_place_scale_and_offset() {
        let mut doc = Document::new();
        let style = with_style(&mut doc, "NOTES");
        let mut linetype = simple("HW", "Hot water", &[12.7, -5.08]);
        linetype.add_element(text_element(-3.0, "HW, 2", style));
        linetype.add_element(LineTypeElement::dash(4.0));
        let text = only_written(&doc, &linetype);
        assert_eq!(
            text,
            "*HW,Hot water\nA,12.7,-5.08,-3,[\"HW, 2\",NOTES,S=2.54,X=-2.5,Y=-1.25],4\n"
        );

        let mut copy = Document::new();
        with_style(&mut copy, "NOTES");
        populate_document_from_source(&mut copy, &text, None);
        let back = copy.line_types.get("HW").unwrap();
        assert_eq!(lengths(back), vec![12.7, -5.08, -3.0, 4.0]);
        let extra = back.elements[2].complex.as_ref().expect("text kept");
        assert_eq!(extra.text(), Some("HW, 2"));
        assert_eq!(extra.scale, 2.54);
        assert_eq!(extra.offset, [-2.5, -1.25]);
    }

    #[test]
    fn each_rotation_mode_survives_at_an_angle() {
        let mut doc = Document::new();
        let style = with_style(&mut doc, "NOTES");
        for (key, absolute, upright) in
            [("R", false, false), ("A", true, false), ("U", false, true)]
        {
            let mut element = text_element(-3.0, "T", style);
            let extra = element.complex.as_mut().unwrap();
            extra.rotation = 37.5_f64.to_radians();
            extra.absolute_rotation = absolute;
            extra.upright = upright;
            let mut linetype = simple("TURN", "", &[6.0]);
            linetype.add_element(element);
            let text = only_written(&doc, &linetype);
            assert!(text.contains(&format!(",{key}=37.5,")), "{text}");

            let mut copy = Document::new();
            with_style(&mut copy, "NOTES");
            populate_document_from_source(&mut copy, &text, None);
            let back = copy.line_types.get("TURN").unwrap();
            let extra = back.elements[1].complex.as_ref().expect("text kept");
            assert!((extra.rotation.to_degrees() - 37.5).abs() < 1e-5, "{key}");
            assert_eq!(extra.absolute_rotation, absolute, "{key}");
            assert_eq!(extra.upright, upright, "{key}");
        }
    }

    #[test]
    fn a_bundled_shape_is_written_by_name() {
        let mut doc = Document::new();
        let style = shape_style(&mut doc, "ltypeshp.shx");
        let mut linetype = simple("ROW", "", &[6.0]);
        linetype.add_element(LineTypeElement {
            length: -2.0,
            complex: Some(LineTypeComplexData {
                content: LineTypeComplexContent::Shape { shape_number: 132 },
                style_handle: style,
                scale: 0.75,
                rotation: 90.0_f64.to_radians(),
                offset: [0.25, 0.0],
                ..Default::default()
            }),
        });
        let text = only_written(&doc, &linetype);
        assert_eq!(text, "*ROW\nA,6,-2,[BOX,ltypeshp.shx,S=0.75,R=90,X=0.25]\n");
        let mut copy = Document::new();
        populate_document_from_source(&mut copy, &text, None);
        let back = copy.line_types.get("ROW").unwrap();
        let extra = back.elements[1].complex.as_ref().expect("shape kept");
        assert_eq!(extra.shape_number(), Some(132));
    }

    #[test]
    fn a_complex_linetype_without_a_description_keeps_its_shapes() {
        let source = "*BARE\nA,6,-2,[BOX,ltypeshp.shx,S=0.75],-1\n";
        assert!(parse_complex(source).contains_key("BARE"));
        let mut doc = Document::new();
        populate_document_from_source(&mut doc, source, None);
        assert!(doc.line_types.get("BARE").unwrap().is_complex());
    }

    #[test]
    fn a_placeholder_that_draws_nothing_is_a_plain_dash() {
        let doc = Document::new();
        let mut linetype = simple("HOLLOW", "", &[1.0]);
        linetype.add_element(LineTypeElement {
            length: -1.0,
            complex: Some(LineTypeComplexData::default()),
        });
        assert_eq!(only_written(&doc, &linetype), "*HOLLOW\nA,1,-1\n");
    }

    #[test]
    fn what_a_file_cannot_hold_is_skipped_and_reported() {
        let mut doc = Document::new();
        let style = with_style(&mut doc, "NOTES");
        let shapes = shape_style(&mut doc, "ltypeshp.shx");
        let missing = Handle::new(0xFFFF);

        assert_eq!(
            reason_for(&doc, &LineType::continuous()),
            SkipReason::NoPattern
        );
        assert_eq!(
            reason_for(&doc, &LineType::by_layer()),
            SkipReason::NoPattern
        );
        assert_eq!(
            reason_for(&doc, &simple("A,B", "", &[1.0, -1.0])),
            SkipReason::InvalidName
        );
        assert_eq!(
            reason_for(&doc, &simple("X|DASH", "", &[1.0, -1.0])),
            SkipReason::InvalidName
        );
        assert_eq!(
            reason_for(&doc, &simple("BROKEN", "two\nlines", &[1.0, -1.0])),
            SkipReason::InvalidDescription
        );
        for bad in [f64::NAN, f64::INFINITY] {
            assert_eq!(
                reason_for(&doc, &simple("NAN", "", &[1.0, bad])),
                SkipReason::InvalidLength
            );
        }
        for text in ["say \"hi\"", "a]b", "a[b", "two\nlines"] {
            let mut linetype = simple("QUOTE", "", &[1.0]);
            linetype.add_element(text_element(-1.0, text, style));
            assert_eq!(
                reason_for(&doc, &linetype),
                SkipReason::UnwritableText,
                "{text}"
            );
        }
        let mut orphan = simple("ORPHAN", "", &[1.0]);
        orphan.add_element(text_element(-1.0, "T", missing));
        assert_eq!(reason_for(&doc, &orphan), SkipReason::MissingStyle);

        let shape = |number: i16, handle: Handle| LineTypeElement {
            length: -1.0,
            complex: Some(LineTypeComplexData {
                content: LineTypeComplexContent::Shape {
                    shape_number: number,
                },
                style_handle: handle,
                ..Default::default()
            }),
        };
        let mut unknown = simple("UNKNOWN", "", &[1.0]);
        unknown.add_element(shape(9999, shapes));
        assert_eq!(reason_for(&doc, &unknown), SkipReason::UnknownShape);
        let mut no_style = simple("NOSTYLE", "", &[1.0]);
        no_style.add_element(shape(132, missing));
        assert_eq!(reason_for(&doc, &no_style), SkipReason::UnknownShape);
    }

    #[test]
    fn one_bad_linetype_does_not_stop_the_others() {
        let doc = Document::new();
        let good = simple("GOOD", "", &[1.0, -1.0]);
        let bad = simple("BAD,NAME", "", &[1.0, -1.0]);
        let output = write_linetypes(&doc, [&bad, &good]);
        assert_eq!(output.written, vec!["GOOD"]);
        assert_eq!(output.skipped.len(), 1);
        assert_eq!(parse(&output.text).len(), 1);
    }

    #[test]
    fn autocad_limits_warn_and_keep_the_definition_intact() {
        let doc = Document::new();
        let long = "D".repeat(60);
        let pattern: Vec<f64> = (0..14)
            .map(|n| {
                if n % 2 == 0 {
                    1.123456789
                } else {
                    -2.123456789
                }
            })
            .collect();
        let linetype = simple("LÅNG", &long, &pattern);
        let output = write_linetypes(&doc, [&linetype]);
        let warnings: Vec<LinWarning> = output.warnings.iter().map(|(_, w)| *w).collect();
        assert_eq!(
            warnings,
            vec![
                LinWarning::LongDescription,
                LinWarning::LongLine,
                LinWarning::TooManyElements,
                LinWarning::NonAscii,
            ]
        );
        let back = parse(&output.text);
        assert_eq!(back[0].description, long);
        assert_eq!(lengths(&back[0]), pattern);

        let gap = write_linetypes(&doc, [&simple("GAP", "", &[-1.0, 1.0])]);
        assert_eq!(gap.warnings[0].1, LinWarning::LeadingGap);
        assert_eq!(gap.written, vec!["GAP"]);
    }

    fn doc_with(linetypes: &[LineType]) -> Document {
        let mut doc = Document::new();
        for linetype in linetypes {
            doc.line_types.add(linetype.clone()).unwrap();
        }
        doc
    }

    #[test]
    fn saving_creates_the_folder_and_the_file() {
        let dir = temp_dir("create");
        let path = dir.join("linetypes").join("user.lin");
        let doc = doc_with(&[simple("POSTS", "Posts", &[3.0, -1.0, 0.5, -1.0])]);
        save_definition(&path, &doc, "posts", |_| false).unwrap();
        let back = parse(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(back[0].name, "POSTS");
        assert_eq!(lengths(&back[0]), vec![3.0, -1.0, 0.5, -1.0]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn saving_replaces_a_name_and_keeps_everything_else() {
        let dir = temp_dir("replace");
        let path = dir.join("user.lin");
        std::fs::write(
            &path,
            ";; my notes\n*FIRST,One\nA,1,-1\n*posts,Old\nA,9,-9\n;; keep me\n*LAST,Two\nA,2,-2\n",
        )
        .unwrap();
        let doc = doc_with(&[simple("POSTS", "New", &[3.0, -1.0])]);
        // A name this file already holds is its own to replace, even though
        // the catalog now knows it.
        save_definition(&path, &doc, "POSTS", |_| true).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            ";; my notes\n*FIRST,One\nA,1,-1\n*POSTS,New\nA,3,-1\n;; keep me\n*LAST,Two\nA,2,-2\n"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn saving_refuses_a_name_someone_else_owns() {
        let dir = temp_dir("reserved");
        let path = dir.join("user.lin");
        let doc = doc_with(&[simple("DASHED", "", &[3.0, -1.0])]);
        assert_eq!(
            save_definition(&path, &doc, "DASHED", |_| true),
            Err(SaveError::Reserved)
        );
        assert!(!path.exists());
        assert_eq!(
            save_definition(&path, &doc, "NOPE", |_| false),
            Err(SaveError::NotFound)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_intact() {
        let dir = temp_dir("failed");
        let path = dir.join("user.lin");
        let old = "*KEEP,Old\nA,1,-1\n";
        std::fs::write(&path, old).unwrap();
        // The temporary file's name is taken by a folder, so writing fails.
        std::fs::create_dir(dir.join("user.lin.tmp")).unwrap();
        let doc = doc_with(&[simple("POSTS", "", &[3.0, -1.0])]);
        assert!(matches!(
            save_definition(&path, &doc, "POSTS", |_| false),
            Err(SaveError::Io(_))
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
        assert!(
            dir.join("user.lin.tmp").is_dir(),
            "the blocker is untouched"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_built_catalog_keeps_the_first_definition_of_a_name() {
        let catalog = Catalog::from_source(
            "*BOTH,First\nA,1,-1\n*both,Second\nA,2,-2\n*OTHER,x\nA,3,-3\n".to_owned(),
        );
        assert_eq!(
            catalog.entries,
            vec![
                ("BOTH".to_owned(), "First".to_owned()),
                ("OTHER".to_owned(), "x".to_owned())
            ]
        );
    }
}
