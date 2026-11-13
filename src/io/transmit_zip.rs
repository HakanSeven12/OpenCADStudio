//! Shared ETRANSMIT ZIP packaging for native and web.

use std::collections::HashSet;
use std::io::{Cursor, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

/// Return the safe trailing filename of a stored resource reference.
///
/// Both slash styles are recognized regardless of the host platform. Parent,
/// absolute, UNC, and drive paths contribute only their trailing filename;
/// a trailing separator or dot component is not a filename.
pub(crate) fn file_name(raw: &str) -> Result<String, String> {
    if raw.chars().any(char::is_control) {
        return Err("resource filename contains a control character".to_owned());
    }
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("resource filename is empty".to_owned());
    }
    if raw.ends_with('/') || raw.ends_with('\\') {
        return Err("resource filename ends with a path separator".to_owned());
    }

    let mut name = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    // A drive-relative path such as `C:plan.dwg` has no separator to remove
    // its drive prefix. Strip only a single ASCII drive letter and colon.
    if name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && name.as_bytes().get(1) == Some(&b':')
    {
        name = &name[2..];
    }
    if name.is_empty() {
        return Err("resource filename is empty".to_owned());
    }
    if matches!(name, "." | "..") {
        return Err("resource filename must not be a dot component".to_owned());
    }
    Ok(name.to_owned())
}

/// Validate a package-relative ZIP/folder entry, retaining curated subfolders.
/// Absolute paths, traversal, dot components, and empty components are rejected
/// rather than silently flattened into a different resource's name.
pub(crate) fn entry_name(raw: &str) -> Result<String, String> {
    if raw.chars().any(char::is_control) {
        return Err("ZIP entry name contains a control character".to_owned());
    }
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("ZIP entry name is empty".to_owned());
    }
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with('/')
        || (normalized
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            && normalized.as_bytes().get(1) == Some(&b':'))
    {
        return Err("ZIP entry name must be relative".to_owned());
    }
    if normalized
        .split('/')
        .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err("ZIP entry name contains an empty or dot path component".to_owned());
    }
    Ok(normalized)
}

/// Build a deflated ZIP in memory, preserving entry order and bytes.
pub(crate) fn build_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut seen = HashSet::with_capacity(entries.len());
    let mut validated = Vec::with_capacity(entries.len());
    for (raw_name, data) in entries {
        let name = entry_name(raw_name)?;
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(format!(
                "duplicate ZIP entry name '{name}' (ASCII case-insensitive)"
            ));
        }
        validated.push((name, data));
    }

    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in validated {
        writer
            .start_file(name.as_str(), options)
            .map_err(|error| format!("cannot create ZIP entry '{name}': {error}"))?;
        writer
            .write_all(data)
            .map_err(|error| format!("cannot write ZIP entry '{name}': {error}"))?;
    }
    writer
        .finish()
        .map(Cursor::into_inner)
        .map_err(|error| format!("cannot finish ZIP: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
fn temporary_path(path: &Path) -> PathBuf {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    PathBuf::from(temporary)
}

/// Write a ZIP through a sibling temporary file, leaving an existing output
/// intact if building, writing, or renaming fails.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_zip_atomic(path: &Path, entries: &[(String, Vec<u8>)]) -> Result<(), String> {
    let bytes = build_zip(entries)?;
    if path.file_name().is_none() {
        return Err(format!(
            "ZIP output path has no filename: {}",
            path.display()
        ));
    }

    let temporary = temporary_path(path);
    let result = std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|error| format!("cannot write ZIP '{}': {error}", path.display()));
    if result.is_err() && temporary.is_file() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn read_archive(bytes: &[u8]) -> Vec<(String, Vec<u8>, zip::CompressionMethod)> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut entries = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let name = file.name().to_owned();
            let compression = file.compression();
            let mut data = Vec::new();
            file.read_to_end(&mut data).unwrap();
            entries.push((name, data, compression));
        }
        entries
    }

    fn temp_dir(tag: &str) -> PathBuf {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "ocs-transmit-zip-{tag}-{}-{id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resource_names_flatten_stored_paths() {
        for (raw, expected) in [
            ("drawing.dwg", "drawing.dwg"),
            ("../drawing.dwg", "drawing.dwg"),
            (r"..\drawing.dwg", "drawing.dwg"),
            ("folder/../drawing.dwg", "drawing.dwg"),
            ("/var/drawings/drawing.dwg", "drawing.dwg"),
            (r"C:\projects\drawing.dwg", "drawing.dwg"),
            ("C:drawing.dwg", "drawing.dwg"),
            (r"\\server\share\drawing.dwg", "drawing.dwg"),
            (r"mixed\folder/drawing.dwg", "drawing.dwg"),
            ("  folder/drawing.dwg  ", "drawing.dwg"),
        ] {
            assert_eq!(file_name(raw).as_deref(), Ok(expected), "{raw:?}");
        }
    }

    #[test]
    fn resource_names_reject_empty_dot_and_trailing_separators() {
        for raw in [
            "",
            "   ",
            ".",
            "..",
            "/",
            "\\",
            "folder/",
            r"folder\",
            "folder/.",
            "folder/..",
            "C:",
            "C:.",
            "C:..",
        ] {
            assert!(file_name(raw).is_err(), "{raw:?}");
        }
        assert_eq!(file_name(".hidden").as_deref(), Ok(".hidden"));
        assert_eq!(file_name("..hidden").as_deref(), Ok("..hidden"));
    }

    #[test]
    fn package_entries_retain_only_safe_relative_folders() {
        for (raw, expected) in [
            ("drawing.dwg", "drawing.dwg"),
            ("Fonts/arial.ttf", "Fonts/arial.ttf"),
            (r"xref\site.dwg", "xref/site.dwg"),
            ("  xref/nested/site.dwg  ", "xref/nested/site.dwg"),
            (".hidden", ".hidden"),
            ("..hidden", "..hidden"),
        ] {
            assert_eq!(entry_name(raw).as_deref(), Ok(expected), "{raw:?}");
        }
        for raw in [
            "../drawing.dwg",
            "folder/../drawing.dwg",
            "./drawing.dwg",
            "folder/./drawing.dwg",
            "/var/drawings/drawing.dwg",
            r"C:\projects\drawing.dwg",
            "C:drawing.dwg",
            r"\\server\share\drawing.dwg",
            "folder//drawing.dwg",
            "folder/",
            "",
        ] {
            assert!(entry_name(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn entry_names_reject_nul_and_other_controls_anywhere() {
        for raw in [
            "bad\0name.dwg",
            "bad\nname.dwg",
            "bad\rname.dwg",
            "bad\tname.dwg",
            "bad\u{1f}name.dwg",
            "bad\u{7f}name.dwg",
            "bad\u{85}name.dwg",
            "discarded\nfolder/good.dwg",
        ] {
            assert!(entry_name(raw).is_err(), "{raw:?}");
            assert!(file_name(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn zip_round_trip_preserves_names_order_and_exact_bytes() {
        let entries = vec![
            ("Drawing.dwg".to_owned(), vec![0, 1, 2, 0, 255, 254, 0, 128]),
            (
                "site notes.txt".to_owned(),
                b"synthetic notes\nnot a drawing format".to_vec(),
            ),
            ("empty.bin".to_owned(), Vec::new()),
            (
                "Fonts/ÜMLAUT.SHX".to_owned(),
                (0..=255).collect::<Vec<u8>>(),
            ),
        ];

        let bytes = build_zip(&entries).unwrap();
        let actual = read_archive(&bytes);
        assert_eq!(actual.len(), entries.len());
        for ((name, data, compression), (expected_name, expected_data)) in
            actual.iter().zip(&entries)
        {
            assert_eq!(name, expected_name);
            assert_eq!(data, expected_data);
            assert_eq!(*compression, zip::CompressionMethod::Deflated);
        }
    }

    #[test]
    fn zip_entries_preserve_safe_folders_and_reject_unsafe_paths() {
        let entries = vec![
            ("drawing.dwg".to_owned(), vec![1, 2, 3]),
            (r"Fonts\simplex.shx".to_owned(), vec![4, 5]),
            ("xref/site.dwg".to_owned(), vec![6]),
            ("plot.ctb".to_owned(), vec![7, 8]),
        ];
        let actual = read_archive(&build_zip(&entries).unwrap());
        let names = actual
            .iter()
            .map(|(name, _, _)| name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "drawing.dwg",
                "Fonts/simplex.shx",
                "xref/site.dwg",
                "plot.ctb"
            ]
        );
        for raw in [
            "../drawing.dwg",
            r"C:\project\fonts\simplex.shx",
            "/absolute/image.png",
            r"\\server\share\plot.ctb",
        ] {
            assert!(build_zip(&[(raw.to_owned(), vec![1])]).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn zip_validates_the_drawing_and_every_dependency() {
        let invalid_drawing = vec![
            ("..".to_owned(), vec![1]),
            ("valid.txt".to_owned(), vec![2]),
        ];
        assert!(build_zip(&invalid_drawing).is_err());

        let invalid_dependency = vec![
            ("drawing.dwg".to_owned(), vec![1]),
            ("bad\0dependency.shx".to_owned(), vec![2]),
        ];
        assert!(build_zip(&invalid_dependency).is_err());
    }

    #[test]
    fn zip_rejects_ascii_case_insensitive_duplicates_including_the_drawing() {
        let drawing_collision = vec![
            ("Site.DWG".to_owned(), b"drawing".to_vec()),
            ("site.dwg".to_owned(), b"dependency".to_vec()),
        ];
        let error = build_zip(&drawing_collision).unwrap_err();
        assert!(error.contains("duplicate"), "{error}");

        let dependency_collision = vec![
            ("drawing.dwg".to_owned(), vec![1]),
            ("Fonts/font.SHX".to_owned(), vec![2]),
            ("fonts/FONT.shx".to_owned(), vec![3]),
        ];
        assert!(build_zip(&dependency_collision).is_err());
    }

    #[test]
    fn duplicate_checks_fold_ascii_only() {
        let entries = vec![("Ä.txt".to_owned(), vec![1]), ("ä.txt".to_owned(), vec![2])];
        assert_eq!(read_archive(&build_zip(&entries).unwrap()).len(), 2);
    }

    #[test]
    fn atomic_write_replaces_existing_output_with_the_archive() {
        let dir = temp_dir("replace");
        let path = dir.join("transmit.zip");
        std::fs::write(&path, b"old archive").unwrap();
        let entries = vec![
            ("drawing.dwg".to_owned(), vec![0, 1, 255]),
            ("notes.txt".to_owned(), b"new notes".to_vec()),
        ];

        write_zip_atomic(&path, &entries).unwrap();

        let actual = read_archive(&std::fs::read(&path).unwrap());
        assert_eq!(actual.len(), 2);
        assert_eq!(actual[0].0, "drawing.dwg");
        assert_eq!(actual[0].1, [0, 1, 255]);
        assert_eq!(actual[1].0, "notes.txt");
        assert_eq!(actual[1].1, b"new notes");
        assert!(!temporary_path(&path).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn atomic_io_failure_preserves_existing_output() {
        let dir = temp_dir("io-failure");
        let path = dir.join("transmit.zip");
        let old = b"existing archive remains intact";
        std::fs::write(&path, old).unwrap();
        // Occupying the temporary name with a directory makes the write fail
        // after the archive has been built but before the rename.
        let temporary = temporary_path(&path);
        std::fs::create_dir(&temporary).unwrap();

        assert!(write_zip_atomic(&path, &[("drawing.dwg".to_owned(), vec![1, 2, 3])]).is_err());

        assert_eq!(std::fs::read(&path).unwrap(), old);
        assert!(temporary.is_dir(), "the blocking directory is untouched");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_entries_do_not_touch_existing_output_or_create_a_temporary() {
        let dir = temp_dir("validation-failure");
        let path = dir.join("transmit.zip");
        let old = b"previous output";
        std::fs::write(&path, old).unwrap();
        let entries = vec![
            ("drawing.dwg".to_owned(), vec![1]),
            ("DRAWING.DWG".to_owned(), vec![2]),
        ];

        assert!(write_zip_atomic(&path, &entries).is_err());

        assert_eq!(std::fs::read(&path).unwrap(), old);
        assert!(!temporary_path(&path).exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
