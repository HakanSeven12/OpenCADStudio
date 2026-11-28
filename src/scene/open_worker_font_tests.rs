// Tests of what the open worker reads for the drawing (`text::shx`).

fn tmp(name: &str, bytes: &[u8]) -> String {
    let dir = std::env::temp_dir().join(format!("ocs-shx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

/// The open worker reads the font of each text style, also of a style
/// that no text in model space uses. The first refresh of the
/// Properties palette resolves each style on the UI thread
/// (`set_fixed_text_heights`), and read those font files there.
#[test]
fn the_open_worker_reads_the_font_of_a_style_that_no_text_uses() {
    let path = tmp(
        "unused-style.shx",
        &crate::text::shx::tests::synthetic_bigfont(),
    );
    let mut document = document::Document::new();
    let mut style = document::tables::TextStyle::new("UNUSED");
    style.font_file = path.clone();
    document.text_styles.add(style).unwrap();

    // The open worker.
    let prepared = std::thread::spawn(move || {
        let mut caches = crate::scene::build_derived_caches_impl(&document, None, None);
        let (document, _) =
            crate::scene::prepare_open_geometry(document, &mut caches, [0.0, 0.0, 0.0, 1.0]);
        (
            document,
            crate::text::shx::FILE_READS.with(|count| count.get()),
        )
    })
    .join()
    .unwrap();
    let (document, worker_reads) = prepared;
    assert_eq!(worker_reads, 1, "the worker read the font file");

    // The UI thread: the style is resolved from what the worker read.
    let before = crate::text::shx::FILE_READS.with(|count| count.get());
    crate::entities::common::set_fixed_text_heights(&document);
    assert_eq!(
        crate::text::shx::FILE_READS.with(|count| count.get()) - before,
        0,
        "the UI thread reads no font file"
    );
    assert!(
        crate::text::shx::is_bigfont(&path),
        "the font is in the cache for the draw"
    );
}
