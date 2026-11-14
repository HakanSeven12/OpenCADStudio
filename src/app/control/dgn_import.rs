//! Prompt-free DGN model import for the protocol-1 control envelope.

use super::*;
use crate::modules::insert::dgn_import::{DgnImportError, DgnImportOptions, DgnImportRequest};

fn optional_string<'a>(req: &'a Value, key: &str) -> Result<Option<&'a str>, Value> {
    match req.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(failure(
            "invalid_request",
            format!("{key} must be a string"),
        )),
    }
}

fn dgn_failure(error: DgnImportError) -> Value {
    let mut response = failure(error.code(), error.to_string());
    if let DgnImportError::ModelMissing {
        available_models, ..
    } = &error
    {
        response["available_models"] = json!(available_models);
    }
    response
}

impl OpenCADStudio {
    /// Import a DGN model from named fields, without constructing the
    /// positional -DGNIMPORT command or leaving an interactive prompt active.
    pub(super) fn control_dgn_import(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let path = string(req, "path")?;
        let model = optional_string(req, "model")?;
        let units = optional_string(req, "units")?;
        let mapping = optional_string(req, "mapping")?;
        let code_page = match req.get("codepage") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .and_then(|page| u16::try_from(page).ok())
                    .ok_or_else(|| {
                        failure(
                            "invalid_codepage",
                            "codepage must be a nonnegative integer no greater than 65535",
                        )
                    })?,
            ),
        };
        let request = DgnImportRequest::from_options(DgnImportOptions {
            path,
            model,
            units,
            mapping,
            code_page,
        })
        .map_err(dgn_failure)?;
        let result_path = request.path.clone();
        let canonical_model = request.model.clone();
        let i = self.active_tab;
        let (task, entities) = self.try_run_dgn_import(i, request).map_err(dgn_failure)?;
        self.set_control_result(json!({
            "entities": entities,
            "model": canonical_model,
            "path": result_path,
        }));
        Ok(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::insert::dgn_import::{
        DgnImportSettings, ExistingReference, OwnSettings, ReferenceMode,
    };
    use std::collections::BTreeMap;

    struct RestoreSettings(DgnImportSettings);

    impl Drop for RestoreSettings {
        fn drop(&mut self) {
            crate::modules::insert::dgn_import::set_settings(self.0);
        }
    }

    fn put_word(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    /// V7 stores a 32-bit integer as its high word followed by its low word,
    /// with each word little-endian.
    fn put_i32(bytes: &mut [u8], at: usize, value: i32) {
        let value = value as u32;
        put_word(bytes, at, (value >> 16) as u16);
        put_word(bytes, at + 2, value as u16);
    }

    /// A V7 header with 1000 subunits per master unit and one 2D line from
    /// (0,0) to (2000,3000) UORs. Zero VAX doubles give it a zero global origin.
    fn minimal_v7_line() -> Vec<u8> {
        const HEADER: usize = 1256;
        const LINE: usize = 52;
        let mut bytes = vec![0u8; HEADER + LINE + 2];
        bytes[0] = 8; // 2D design-file header (type 9 below).
        bytes[1] = 9;
        put_word(&mut bytes, 2, ((HEADER - 4) / 2) as u16);
        put_i32(&mut bytes, 1112, 1000); // subunits / master unit
        put_i32(&mut bytes, 1116, 1); // UORs / subunit
        bytes[1120] = b'm'; // Two-byte fields are zero-padded.
        bytes[1122..1124].copy_from_slice(b"mm");

        let line = HEADER;
        bytes[line] = 1; // level 1
        bytes[line + 1] = 3; // line element
        put_word(&mut bytes, line + 2, ((LINE - 4) / 2) as u16);
        put_i32(&mut bytes, line + 44, 2000);
        put_i32(&mut bytes, line + 48, 3000);
        bytes[HEADER + LINE..].copy_from_slice(&[0xff, 0xff]);
        bytes
    }

    fn entity_census(app: &OpenCADStudio) -> (BTreeMap<String, usize>, Vec<Value>) {
        let scene = &app.tabs[app.active_tab].scene;
        let space = scene.current_layout_block_handle_pub();
        let mut census = BTreeMap::new();
        let mut serialized = Vec::new();
        for entity in scene
            .document
            .entities()
            .filter(|entity| entity.common().owner_handle == space)
        {
            *census
                .entry(crate::entities::names::dxf_name(entity).to_string())
                .or_default() += 1;
            serialized.push(serde_json::to_value(entity).expect("entity serializes"));
        }
        (census, serialized)
    }

    fn fresh_drawing() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let response = app.automation_op(r#"{"op":"new"}"#);
        assert_eq!(response["ok"], true, "{response}");
        app
    }

    #[test]
    fn model_errors_carry_the_available_models() {
        let error = dgn_failure(DgnImportError::ModelMissing {
            requested: "Missing".into(),
            path: "site.dgn".into(),
            available_models: vec!["Default".into(), "Site Plan".into()],
        });
        assert_eq!(error["code"], "model_missing");
        assert_eq!(error["available_models"], json!(["Default", "Site Plan"]));
    }

    #[test]
    fn command_and_direct_import_match_for_minimal_v7() {
        let _own = OwnSettings::take();
        let _restore = RestoreSettings(crate::modules::insert::dgn_import::settings());
        let mut legacy = fresh_drawing();
        let mut direct = fresh_drawing();
        crate::modules::insert::dgn_import::set_settings(DgnImportSettings {
            mode: 1,
            explode_text_nodes: false,
            references: ReferenceMode::Ignore,
            existing_reference: ExistingReference::Keep,
            code_page: None,
        });

        let serial = std::process::id();
        let legacy_path = format!("memory://dgn-import-legacy-{serial}.dgn");
        let direct_path = format!("memory://dgn-import-direct-{serial}.dgn");
        let bytes = minimal_v7_line();
        crate::scene::model::pdf_raster::register_source(
            &legacy_path,
            std::sync::Arc::new(bytes.clone()),
        );
        crate::scene::model::pdf_raster::register_source(&direct_path, std::sync::Arc::new(bytes));

        let legacy_undo = legacy.tabs[legacy.active_tab].history.undo_stack.len();
        let direct_undo = direct.tabs[direct.active_tab].history.undo_stack.len();
        let legacy_response = legacy.automation_op(
            &json!({
                "op":"run",
                "cmd":format!("-DGNIMPORT {legacy_path} Default Sub Standard"),
            })
            .to_string(),
        );
        assert_eq!(legacy_response["ok"], true, "{legacy_response}");
        assert_eq!(legacy_response["added"], 1, "{legacy_response}");
        assert_eq!(
            legacy.tabs[legacy.active_tab].history.undo_stack.len(),
            legacy_undo + 1
        );

        let document_id = direct.automation_op(r#"{"protocol":1,"op":"state"}"#)["document_id"]
            .as_u64()
            .expect("document id");
        let request = json!({
            "protocol":1,
            "op":"dgn_import",
            "request_id":format!("dgn-import-parity-{serial}"),
            "document_id":document_id,
            "path":direct_path.clone(),
            "model":"default",
            "units":"sub",
            "mapping":"standard",
            "codepage":1252,
        });
        let request_text = request.to_string();
        let direct_response = direct.automation_op(&request_text);
        assert_eq!(direct_response["ok"], true, "{direct_response}");
        assert_eq!(direct_response["status"], "completed");
        assert_eq!(direct_response["result"]["entities"], 1);
        assert_eq!(direct_response["result"]["model"], "Default");
        assert_eq!(direct_response["result"]["path"], direct_path);
        assert!(direct.tabs[direct.active_tab].active_cmd.is_none());
        assert_eq!(
            direct.tabs[direct.active_tab].history.undo_stack.len(),
            direct_undo + 1
        );

        let legacy_census = entity_census(&legacy);
        let direct_census = entity_census(&direct);
        assert_eq!(legacy_census.0.values().sum::<usize>(), 1);
        assert!(legacy_census
            .0
            .keys()
            .any(|name| name.eq_ignore_ascii_case("LINE")));
        assert_eq!(direct_census, legacy_census);
        let scene = &direct.tabs[direct.active_tab].scene;
        let line = scene.document.entities().find_map(|entity| match entity {
            document::EntityType::Line(line) => Some(line),
            _ => None,
        });
        let line = line.expect("the V7 line imported");
        assert_eq!(line.start.x, 0.0);
        assert_eq!(line.start.y, 0.0);
        assert_eq!(line.end.x, 2000.0);
        assert_eq!(line.end.y, 3000.0);
        assert_eq!(line.common.layer, "1");

        let replay = direct.automation_op(&request_text);
        assert_eq!(replay, direct_response);
        assert_eq!(entity_census(&direct), direct_census);
        assert_eq!(
            direct.tabs[direct.active_tab].history.undo_stack.len(),
            direct_undo + 1
        );

        let atomic_state = |app: &OpenCADStudio| {
            let tab = &app.tabs[app.active_tab];
            (
                app.tabs.len(),
                tab.id,
                tab.edit_revision,
                tab.scene.geometry_epoch,
                tab.history.undo_stack.len(),
                tab.active_cmd.is_some(),
                entity_census(app),
            )
        };
        let before_errors = atomic_state(&direct);
        let missing_path = format!("memory://dgn-import-absent-{serial}.dgn");
        for (request, code) in [
            (
                json!({
                    "protocol":1,"op":"dgn_import","request_id":"dgn-missing-model",
                    "document_id":document_id,"path":direct_path,"model":"Missing",
                }),
                "model_missing",
            ),
            (
                json!({
                    "protocol":1,"op":"dgn_import","request_id":"dgn-invalid-file",
                    "document_id":document_id,"path":missing_path,
                }),
                "invalid_dgn",
            ),
        ] {
            let response = direct.automation_op(&request.to_string());
            assert_eq!(response["ok"], false, "{response}");
            assert_eq!(response["code"], code, "{response}");
            assert_eq!(atomic_state(&direct), before_errors, "{response}");
            if code == "model_missing" {
                assert_eq!(response["available_models"], json!(["Default"]));
            }
        }
    }
}
