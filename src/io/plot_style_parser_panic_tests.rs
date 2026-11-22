//! A panic in the plot style table parser is the read's error at each place
//! that parses a file from the user or from a drawing.

use super::{
    parse_table_with, register_session_table, PickedPlotStyle, PlotStyleTable, PlotStyleTableExt,
    TEST_PARSER_PANICS,
};

/// Makes `parse_table` use a parser that panics, until it is dropped.
struct PanickingParser;

impl PanickingParser {
    fn install() -> Self {
        TEST_PARSER_PANICS.with(|flag| flag.set(true));
        Self
    }
}

impl Drop for PanickingParser {
    fn drop(&mut self) {
        TEST_PARSER_PANICS.with(|flag| flag.set(false));
    }
}

fn table_bytes(name: &str) -> Vec<u8> {
    PlotStyleTable::identity(name).to_bytes().expect("encode")
}

fn unique_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ocs-plot-style-panic-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn assert_reader_stopped(result: Result<PlotStyleTable, String>, place: &str) {
    let error = result.expect_err(place);
    assert!(
        error.contains("the plot style reader stopped: test parser panic"),
        "{place}: {error}"
    );
}

#[test]
fn a_parser_panic_is_the_error_of_the_read() {
    let result = parse_table_with(
        "Pens.ctb",
        b"bytes",
        |_, _| -> Result<PlotStyleTable, String> { panic!("index out of bounds: the len is 3") },
    );
    assert_eq!(
        result.unwrap_err(),
        "the plot style reader stopped: index out of bounds: the len is 3"
    );
}

#[test]
fn a_parser_result_passes_through_unchanged() {
    let table = parse_table_with("Pens.ctb", b"bytes", |name, bytes| {
        assert_eq!((name, bytes), ("Pens.ctb", &b"bytes"[..]));
        Ok::<_, String>(PlotStyleTable::identity(name))
    })
    .expect("the parser's table");
    assert_eq!(table.name, "Pens.ctb");

    let error = parse_table_with("Pens.ctb", b"bytes", |_, _| {
        Err::<PlotStyleTable, _>("bad header")
    })
    .unwrap_err();
    assert_eq!(error, "bad header");
}

#[test]
fn a_picked_table_survives_a_parser_panic() {
    let name = "Picked.ctb";
    let bytes = table_bytes(name);
    assert!(PickedPlotStyle::from_bytes(name.to_string(), bytes.clone()).is_ok());

    let _parser = PanickingParser::install();
    let error = PickedPlotStyle::from_bytes(name.to_string(), bytes).unwrap_err();
    assert!(
        error.starts_with("Invalid plot style table \"Picked.ctb\": the plot style reader stopped"),
        "{error}"
    );
}

#[test]
fn a_table_file_survives_a_parser_panic() {
    let dir = unique_dir("file");
    let path = dir.join("OnDisk.ctb");
    std::fs::write(&path, table_bytes("OnDisk.ctb")).unwrap();
    assert!(PlotStyleTable::load(&path).is_ok());

    {
        let _parser = PanickingParser::install();
        assert_reader_stopped(PlotStyleTable::load(&path), "load");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_session_table_survives_a_parser_panic() {
    let name = "PanicSession.ctb";
    register_session_table(name, std::sync::Arc::new(table_bytes(name)));
    assert!(PlotStyleTable::load_named(name).is_ok());

    let _parser = PanickingParser::install();
    assert_reader_stopped(PlotStyleTable::load_named(name), "load_named");
}

#[test]
fn a_table_next_to_the_drawing_survives_a_parser_panic() {
    let dir = unique_dir("drawing");
    let tables = dir.join("PlotStyles");
    std::fs::create_dir_all(&tables).unwrap();
    let name = format!("PanicPackaged-{}.ctb", std::process::id());
    std::fs::write(tables.join(&name), table_bytes(&name)).unwrap();
    assert!(PlotStyleTable::load_named_at(&name, Some(&dir)).is_ok());

    {
        let _parser = PanickingParser::install();
        assert_reader_stopped(
            PlotStyleTable::load_named_at(&name, Some(&dir)),
            "load_named_at",
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}
