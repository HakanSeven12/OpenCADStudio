//! Finding a file a drawing refers to, wherever the reference was written.

use std::path::Path;

/// Resolve a (possibly foreign / absolute) image path to an existing file:
/// as stored, then relative to the drawing folder, then just the file name
/// next to the drawing.
pub fn resolve_image_file(raw: &str, base_dir: Option<&Path>) -> Option<String> {
    if Path::new(raw).is_file() {
        return Some(raw.to_string());
    }
    let base_dir = base_dir?;
    // A relative path written on Windows (`.\images\a.tif`) keeps its
    // backslashes on other systems, where they are not separators.
    let relative = if cfg!(windows) {
        std::borrow::Cow::Borrowed(raw)
    } else {
        std::borrow::Cow::Owned(raw.replace('\\', "/"))
    };
    let joined = base_dir.join(&*relative);
    if joined.is_file() {
        return Some(joined.to_string_lossy().into_owned());
    }
    let name = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cand = base_dir.join(name);
    if cand.is_file() {
        return Some(cand.to_string_lossy().into_owned());
    }
    None
}
