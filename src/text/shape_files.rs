//! Shape files (`.shx`) a linetype or text style names, and the shapes the
//! bundled substitutes stand in for.

/// Shape numbers for the shape files the bundled catalog references,
/// keyed by file stem and shape name. The `ltypeshp` 130–134 are the
/// standard ones; the rest exist only in the bundled LFF substitutes, so
/// their numbers are this application's own.
pub(crate) const BUNDLED_SHAPES: &[(&str, &str, i16)] = &[
    ("ltypeshp", "TRACK1", 130),
    ("ltypeshp", "ZIG", 131),
    ("ltypeshp", "BOX", 132),
    ("ltypeshp", "CIRC1", 133),
    ("ltypeshp", "BAT", 134),
    ("ltypeshp", "PT1", 135),
    ("ltypeshp", "PT2", 136),
    ("ltypeshp", "PLANT1", 137),
    ("ltypeshp", "PLANT2", 138),
    ("ltypeshp", "PLANT3", 139),
    ("ltypeshp", "PLANT4", 140),
    ("ltypeshp", "PLANT5", 141),
    ("ltypeshp", "PLANT6", 142),
    ("qcadshp", "RIGHT_ARROW", 130),
];

pub(crate) fn shape_file_stem(file: &str) -> String {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    name.split('.').next().unwrap_or(name).to_ascii_lowercase()
}

/// The bundled shape a stored shape number names in `file` (an empty file
/// reads as `ltypeshp`).
pub(crate) fn bundled_shape_name(file: &str, number: i16) -> Option<&'static str> {
    let stem = shape_file_stem(file);
    let stem = if stem.is_empty() {
        "ltypeshp".to_string()
    } else {
        stem
    };
    BUNDLED_SHAPES
        .iter()
        .find(|(file, _, n)| *file == stem && *n == number)
        .map(|(_, name, _)| *name)
}

/// Whether `file` is a shape file the bundled LFF substitutes stand in for,
/// so its absence on disk never needs reporting.
pub fn is_bundled_shape_file(file: &str) -> bool {
    let stem = shape_file_stem(file);
    BUNDLED_SHAPES.iter().any(|(f, _, _)| *f == stem)
}

fn file_named(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
        })
        .map(|entry| entry.path())
}

/// A shape file found as written, next to its drawing or `.lin` catalog (in
/// any letter case, as stored references rarely match the files' case), in an
/// AutoCAD-style transmittal `Fonts` folder, or in the configured font folders.
pub(crate) fn find_shape_file(file: &str, lin_dir: Option<&std::path::Path>) -> Option<String> {
    if let Some(found) = crate::paths::resolve_image_file(file, lin_dir) {
        return Some(found);
    }
    let name = file.rsplit(['/', '\\']).next()?;
    lin_dir
        .and_then(|dir| file_named(dir, name))
        .or_else(|| {
            super::font_repo::packaged_font_dirs(lin_dir)
                .into_iter()
                .find_map(|dir| file_named(&dir, name))
        })
        .or_else(|| super::font_repo::local_font_file(name))
        .map(|path| path.to_string_lossy().into_owned())
}
