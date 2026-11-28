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

/// Create a uniquely named sibling temporary file with `create_new(true)` so
/// atomic writes never follow or collide with a predictable temp path.
#[cfg(not(target_arch = "wasm32"))]
pub fn create_atomic_temp(path: &Path) -> std::io::Result<(std::path::PathBuf, std::fs::File)> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let base = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty() && name != "." && name != "..")
        .unwrap_or_else(|| "file".to_string());
    for _ in 0..32 {
        let seq = SERIAL.fetch_add(1, Ordering::Relaxed);
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(seq);
        let rand_val = hasher.finish();
        let candidate = dir.join(format!(
            ".{base}.ocs-tmp-{}-{seq:x}-{rand_val:016x}",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate unique temporary file",
    ))
}

/// Write `bytes` atomically to `path` via a unique sibling temp file + `sync_all`.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_file_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let (temp_path, mut file) = create_atomic_temp(path)?;
    let write_res = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(err) = write_res {
        let _ = std::fs::remove_file(&temp_path);
        return Err(err);
    }
    let rename_res = std::fs::rename(&temp_path, path);
    if rename_res.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    rename_res
}
