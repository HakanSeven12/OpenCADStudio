use std::{fs, io, path::Path};

/// Before overwriting `path`, copy the existing file to a sibling
/// `<name>.bak` so a faulty or accidental save can be recovered. A failed
/// backup never blocks the save and never truncates the previous backup.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn write_backup(path: &Path) {
    if path.exists() {
        let _ = copy_backup(path, |src, dst| fs::copy(src, dst));
    }
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn copy_backup(path: &Path, copy: impl FnOnce(&Path, &Path) -> io::Result<u64>) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let backup = path.with_extension("bak");
    if path == backup {
        return Ok(());
    }

    // Reserve a sibling so concurrent saves cannot share a partial copy.
    let temporary = loop {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let candidate =
            backup.with_file_name(format!(".ocs-backup-{}-{serial}.tmp", std::process::id()));
        match fs::File::create_new(&candidate) {
            Ok(_) => break candidate,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = copy(path, &temporary).and_then(|_| fs::rename(&temporary, &backup));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn partial_copy_preserves_previous_backup() -> io::Result<()> {
        let dir = std::env::temp_dir().join(format!("ocs-backup-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir)?;
        let drawing = dir.join("drawing.dwg");
        let backup = drawing.with_extension("bak");
        fs::write(&drawing, b"current drawing")?;
        fs::write(&backup, b"previous backup")?;

        let result = copy_backup(&drawing, |_, destination| {
            fs::File::create(destination)?.write_all(b"partial")?;
            Err(io::Error::other("injected copy failure"))
        });
        let after_failure = fs::read(&backup)?;
        let files_after_failure = fs::read_dir(&dir)?.count();
        write_backup(&drawing);
        let after_success = fs::read(&backup)?;
        fs::remove_dir_all(dir)?;

        assert_eq!(result.unwrap_err().to_string(), "injected copy failure");
        assert_eq!(after_failure, b"previous backup");
        assert_eq!(files_after_failure, 2);
        assert_eq!(after_success, b"current drawing");
        Ok(())
    }
}
