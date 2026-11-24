//! Sheet lists (`.dsd`): the drawing set description other CAD programs'
//! PUBLISH save and `-PUBLISH` reads.
//!
//! A sheet list is an INI file. Every `[DWF6Sheet:<name>]` section is one
//! sheet, in publishing order: `DWG` names the drawing, `Layout` the layout
//! (`Model` for model space), `Setup` an optional named page setup to plot it
//! with instead of the layout's own. `[Target]` says where the sheets go —
//! `Type=6` one multi-sheet PDF at `DWF`, `Type=5` a PDF per sheet in the
//! `OUT` folder — and `[PdfOptions] CreateBookmarks` whether the PDF gets a
//! bookmark per sheet, titled with the sheet's name.

use std::path::{Path, PathBuf};

/// One sheet of a sheet list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sheet {
    /// The sheet's name: the bookmark title, and the file name of a
    /// single-sheet PDF.
    pub name: String,
    /// The drawing, resolved against the sheet list's folder.
    pub drawing: PathBuf,
    /// The layout to plot; `Model` is model space.
    pub layout: String,
    /// A named page setup of the drawing to plot with instead of the
    /// layout's own page setup.
    pub setup: Option<String>,
}

/// Where a sheet list publishes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// `Type=6`: every sheet into one PDF.
    MultiSheetPdf(PathBuf),
    /// `Type=5`: a PDF per sheet, named after the sheet, in this folder.
    SingleSheetPdfs(PathBuf),
}

/// A sheet list read from a `.dsd` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetList {
    pub sheets: Vec<Sheet>,
    pub target: Target,
    /// `[PdfOptions] CreateBookmarks`: a bookmark per sheet (the default).
    pub bookmarks: bool,
}

/// Whether `path` names a sheet list.
pub fn is_sheet_list_path(path: &str) -> bool {
    path.trim()
        .trim_matches('"')
        .to_ascii_lowercase()
        .ends_with(".dsd")
}

/// Read the sheet list at `path`; relative drawing and output paths are
/// taken from its folder.
pub fn read(path: &Path) -> Result<SheetList, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("Cannot read sheet list {}: {error}", path.display()))?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    parse(&decode(&bytes), base)
}

/// The text of a sheet list file. Other programs write them in the system's ANSI
/// code page or as Unicode with a byte-order mark; text that is not UTF-8
/// and has no mark is read as Windows-1252.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE
            .decode_without_bom_handling(rest)
            .0
            .into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE
            .decode_without_bom_handling(rest)
            .0
            .into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => encoding_rs::WINDOWS_1252
            .decode_without_bom_handling(bytes)
            .0
            .into_owned(),
    }
}

/// Parse sheet list text; `base` is the folder relative paths start from.
pub fn parse(text: &str, base: &Path) -> Result<SheetList, String> {
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut target_type: Option<String> = None;
    let mut target_file = String::new();
    let mut target_folder = String::new();
    let mut bookmarks = true;
    // The section the following keys belong to.
    enum Section {
        Sheet,
        Target,
        PdfOptions,
        Other,
    }
    let mut section = Section::Other;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(header) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = if let Some(name) = strip_prefix_ignore_case(header, "DWF6Sheet:") {
                sheets.push(Sheet {
                    name: name.to_string(),
                    drawing: PathBuf::new(),
                    layout: String::new(),
                    setup: None,
                });
                Section::Sheet
            } else if header.eq_ignore_ascii_case("Target") {
                Section::Target
            } else if header.eq_ignore_ascii_case("PdfOptions") {
                Section::PdfOptions
            } else {
                Section::Other
            };
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match section {
            Section::Sheet => {
                let sheet = sheets.last_mut().expect("a sheet section is open");
                if key.eq_ignore_ascii_case("DWG") {
                    sheet.drawing = resolve(value, base);
                } else if key.eq_ignore_ascii_case("Layout") {
                    sheet.layout = value.to_string();
                } else if key.eq_ignore_ascii_case("Setup") && !value.is_empty() {
                    sheet.setup = Some(value.to_string());
                }
            }
            Section::Target => {
                if key.eq_ignore_ascii_case("Type") {
                    target_type = Some(value.to_string());
                } else if key.eq_ignore_ascii_case("DWF") {
                    target_file = value.to_string();
                } else if key.eq_ignore_ascii_case("OUT") {
                    target_folder = value.to_string();
                }
            }
            Section::PdfOptions => {
                if key.eq_ignore_ascii_case("CreateBookmarks") {
                    bookmarks = !value.eq_ignore_ascii_case("FALSE") && value != "0";
                }
            }
            Section::Other => {}
        }
    }
    if sheets.is_empty() {
        return Err("The sheet list names no sheets.".into());
    }
    for sheet in &sheets {
        if sheet.drawing.as_os_str().is_empty() {
            return Err(format!("Sheet \"{}\" names no drawing (DWG=).", sheet.name));
        }
        if sheet.layout.is_empty() {
            return Err(format!(
                "Sheet \"{}\" names no layout (Layout=).",
                sheet.name
            ));
        }
    }
    let target = match target_type.as_deref() {
        Some("6") if !target_file.is_empty() => Target::MultiSheetPdf(resolve(&target_file, base)),
        Some("6") => return Err("The sheet list names no PDF file (DWF= in [Target]).".into()),
        Some("5") => Target::SingleSheetPdfs(if target_folder.is_empty() {
            base.to_path_buf()
        } else {
            resolve(&target_folder, base)
        }),
        Some(other) => {
            return Err(format!(
                "The sheet list publishes to target type {other}; only PDF \
                 (Type=5 single-sheet, Type=6 multi-sheet) is supported."
            ))
        }
        None => return Err("The sheet list has no [Target] Type.".into()),
    };
    Ok(SheetList {
        sheets,
        target,
        bookmarks,
    })
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &text[prefix.len()..])
}

/// A path from the sheet list, relative ones taken from `base`. A list
/// written on Windows separates with backslashes; elsewhere they become
/// slashes unless the path exists as written.
fn resolve(value: &str, base: &Path) -> PathBuf {
    let value = value.trim_matches('"');
    let path = PathBuf::from(value);
    #[cfg(not(windows))]
    let path = if value.contains('\\') && !base.join(&path).exists() {
        PathBuf::from(value.replace('\\', "/"))
    } else {
        path
    };
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

/// A file name for a single-sheet PDF: the sheet's name with the characters
/// file systems refuse replaced.
pub fn sheet_file_name(sheet: &str) -> String {
    let name: String = sheet
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let name = name.trim().trim_end_matches('.');
    if name.is_empty() {
        "sheet.pdf".to_string()
    } else {
        format!("{name}.pdf")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layout openCirt writes for BricsCAD's PUBLISH (sections and keys
    /// as its PUBLISH dialog saves them).
    const LIST: &str = "[DWF6Version]\nVer=1\n[DWF6MinorVersion]\nMinorVer=1\n\
        [DWF6Sheet:0001 - Cover]\nDWG=/set/a.dwg\nLayout=A2\nSetup=\n\
        OriginalSheetPath=/set/a.dwg\nHas Plot Port=0\nHas3DDWF=0\n\
        [DWF6Sheet:0002 - Übersicht]\nDWG=sub/b.dwg\nLayout=Model\nSetup=PDF A3\n\
        [Target]\nType=6\nDWF=/out/set.pdf\nOUT=/out\nLocationType=2\n\
        [SheetSet Properties]\nIncludeLayer=FALSE\n\
        [PdfOptions]\nExportHyperlinks=TRUE\nCreateBookmarks=TRUE\n";

    #[test]
    fn sheets_keep_their_order_names_layouts_and_setups() {
        let list = parse(LIST, Path::new("/base")).unwrap();
        assert_eq!(
            list.sheets,
            vec![
                Sheet {
                    name: "0001 - Cover".into(),
                    drawing: "/set/a.dwg".into(),
                    layout: "A2".into(),
                    setup: None,
                },
                Sheet {
                    name: "0002 - Übersicht".into(),
                    drawing: "/base/sub/b.dwg".into(),
                    layout: "Model".into(),
                    setup: Some("PDF A3".into()),
                },
            ]
        );
        assert_eq!(list.target, Target::MultiSheetPdf("/out/set.pdf".into()));
        assert!(list.bookmarks);
    }

    #[test]
    fn single_sheet_target_and_bookmarks_off() {
        let text = LIST
            .replace("Type=6", "Type=5")
            .replace("CreateBookmarks=TRUE", "CreateBookmarks=FALSE");
        let list = parse(&text, Path::new("/base")).unwrap();
        assert_eq!(list.target, Target::SingleSheetPdfs("/out".into()));
        assert!(!list.bookmarks);
    }

    #[test]
    fn other_targets_and_incomplete_sheets_are_refused() {
        let plotter = LIST.replace("Type=6", "Type=0");
        assert!(parse(&plotter, Path::new("/"))
            .unwrap_err()
            .contains("target type 0"));
        let no_file = LIST.replace("DWF=/out/set.pdf", "DWF=");
        assert!(parse(&no_file, Path::new("/"))
            .unwrap_err()
            .contains("DWF="));
        let no_layout = LIST.replace("Layout=A2", "Layout=");
        assert!(parse(&no_layout, Path::new("/"))
            .unwrap_err()
            .contains("0001 - Cover"));
        assert!(parse("[Target]\nType=6\nDWF=x.pdf\n", Path::new("/")).is_err());
    }

    #[test]
    fn unicode_and_ansi_files_read_alike() {
        let text = "[DWF6Sheet:Übersicht]";
        let utf8_bom = [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat();
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        let ansi = encoding_rs::WINDOWS_1252.encode(text).0.into_owned();
        for bytes in [text.as_bytes().to_vec(), utf8_bom, utf16, ansi] {
            assert_eq!(decode(&bytes), text);
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn windows_separators_become_slashes() {
        let text = LIST.replace("DWG=sub/b.dwg", "DWG=sub\\b.dwg");
        let list = parse(&text, Path::new("/base")).unwrap();
        assert_eq!(list.sheets[1].drawing, PathBuf::from("/base/sub/b.dwg"));
    }

    #[test]
    fn sheet_names_make_file_names() {
        assert_eq!(sheet_file_name("0001 - A/B: C?"), "0001 - A_B_ C_.pdf");
        assert_eq!(sheet_file_name(" .. "), "sheet.pdf");
        assert!(is_sheet_list_path("\"/x/Set.DSD\""));
        assert!(!is_sheet_list_path("/x/set.pdf"));
    }
}
