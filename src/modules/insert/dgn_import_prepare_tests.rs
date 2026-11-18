use super::*;
use crate::scene::model::underlay_vector::DgnReference;

fn request(path: &str, references: ReferenceMode) -> DgnImportRequest {
    DgnImportRequest {
        path: path.to_string(),
        model: "Default".to_string(),
        sub_units: false,
        explode_text_nodes: false,
        current_drawing: true,
        prefix_duplicates: false,
        references,
        existing_reference: ExistingReference::Prompt,
        code_page: None,
    }
}

/// A folder of its own in the temporary folder, removed when dropped.
struct Folder(std::path::PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ocs-dgn-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("test folder");
        Self(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn prepare_import_reads_and_converts_the_model() {
    let path = format!("memory://dgn-prepare-{}.dgn", std::process::id());
    crate::scene::model::pdf_raster::register_source(
        &path,
        std::sync::Arc::new(test_files::minimal_v7_line()),
    );
    let prepared = prepare_import(&request(&path, ReferenceMode::Translate)).expect("read");
    assert_eq!(prepared.converted.entities.len(), 1);
    assert!(prepared.import.v7);
    assert!(prepared.references.is_empty());
}

#[test]
fn prepare_import_refuses_a_file_that_is_no_dgn_model() {
    let path = format!("memory://dgn-prepare-junk-{}.dgn", std::process::id());
    crate::scene::model::pdf_raster::register_source(
        &path,
        std::sync::Arc::new(b"not a design file".to_vec()),
    );
    assert_eq!(
        prepare_import(&request(&path, ReferenceMode::Ignore)).unwrap_err(),
        DgnImportError::InvalidDgn
    );
}

/// A reference that names another folder is found by its file name in
/// the host file's folder; a translation converts its model unless the
/// drawing is already there and is kept; an underlay converts nothing.
#[test]
fn a_reference_is_converted_only_when_its_drawing_is_written() {
    let folder = Folder::new("reference");
    std::fs::write(folder.0.join("site.dgn"), test_files::minimal_v7_line()).unwrap();
    let host = folder.0.join("host.dgn").to_string_lossy().into_owned();
    let reference = DgnReference {
        file: "C:\\elsewhere\\site.dgn".to_string(),
        ..Default::default()
    };

    let translate = request(&host, ReferenceMode::Translate);
    let prepared = prepare_reference(&translate, &reference).expect("found");
    assert_eq!(prepared.file, folder.0.join("site.dgn"));
    assert_eq!(prepared.model, "Default");
    let converted = prepared.converted.expect("no drawing yet: converted");
    assert_eq!(converted.entities.len(), 1);

    let underlay = request(&host, ReferenceMode::Underlay);
    let prepared = prepare_reference(&underlay, &reference).expect("found");
    assert!(prepared.converted.is_none(), "an underlay converts nothing");

    let drawing = reference_drawing(&folder.0.join("site.dgn"), "Default");
    assert_eq!(drawing, folder.0.join("site-Default.dwg"));
    std::fs::write(&drawing, b"").unwrap();
    let prepared = prepare_reference(&translate, &reference).expect("found");
    assert!(prepared.converted.is_none(), "the drawing is kept");
    let overwrite = DgnImportRequest {
        existing_reference: ExistingReference::Overwrite,
        ..translate
    };
    let prepared = prepare_reference(&overwrite, &reference).expect("found");
    assert!(prepared.converted.is_some(), "the drawing is written again");
}

#[test]
fn a_reference_whose_file_is_not_found_is_not_placed() {
    let folder = Folder::new("missing-reference");
    let host = folder.0.join("host.dgn").to_string_lossy().into_owned();
    let reference = DgnReference {
        file: "absent.dgn".to_string(),
        model: "Default".to_string(),
        ..Default::default()
    };
    assert!(prepare_reference(&request(&host, ReferenceMode::Underlay), &reference).is_none());
}

/// A model with nothing to import but a reference is imported for the
/// reference, unless references are ignored; then it is empty.
#[test]
fn a_model_of_only_references_is_empty_when_they_are_ignored() {
    let folder = Folder::new("only-references");
    std::fs::write(folder.0.join("site.dgn"), test_files::minimal_v7_line()).unwrap();
    let host = folder.0.join("host.dgn").to_string_lossy().into_owned();
    let import = DgnImport {
        references: vec![DgnReference {
            file: "site.dgn".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let prepared = prepare_read(&request(&host, ReferenceMode::Underlay), import.clone())
        .expect("the reference is imported");
    assert_eq!(prepared.references.len(), 1);
    assert!(prepared.converted.entities.is_empty());
    assert_eq!(
        prepare_read(&request(&host, ReferenceMode::Ignore), import).unwrap_err(),
        DgnImportError::EmptyModel
    );
}
