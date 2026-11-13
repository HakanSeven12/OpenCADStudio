//! ATTOUT / ATTIN: block attribute values out to, and back in from, a
//! tab-delimited text file. The header is `HANDLE`, `BLOCKNAME`, then one
//! column per attribute tag; a handle is written with a leading `'` so a
//! spreadsheet keeps it as text, and `<>` marks a tag the block lacks.

use document::types::Handle;
use document::EntityType;
use iced::Task;

use super::super::{Message, OpenCADStudio};

/// Cell of a tag the block does not carry; ATTIN leaves it alone.
const MISSING: &str = "<>";

/// The ATTOUT text for `handles`, or `None` when none is an attributed block.
pub(crate) fn attout_text(doc: &document::Document, handles: &[Handle]) -> Option<String> {
    let mut tags: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for &handle in handles {
        let Some(EntityType::Insert(insert)) = doc.get_entity(handle) else {
            continue;
        };
        if insert.attributes.is_empty() {
            continue;
        }
        for attribute in &insert.attributes {
            let tag = attribute.tag.to_uppercase();
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        let name = crate::app::control::block_ops::resolve_effective_block(doc, insert).0;
        rows.push((handle, name, insert));
    }
    if rows.is_empty() {
        return None;
    }
    // A tab or line break inside a value would split its row.
    let cell = |value: &str| value.replace(['\t', '\r', '\n'], " ");
    let mut text = format!("HANDLE\tBLOCKNAME\t{}\r\n", tags.join("\t"));
    for (handle, name, insert) in rows {
        let mut line = format!("'{:X}\t{}", handle.value(), cell(&name));
        for tag in &tags {
            line.push('\t');
            match insert
                .attributes
                .iter()
                .find(|attribute| attribute.tag.eq_ignore_ascii_case(tag))
            {
                Some(attribute) => line.push_str(&cell(attribute.get_value())),
                None => line.push_str(MISSING),
            }
        }
        text.push_str(&line);
        text.push_str("\r\n");
    }
    Some(text)
}

/// Rows of an ATTOUT file as (handle, [(tag, value)]); `Err` without a
/// HANDLE column. Blank rows, unparsable handles and `<>` cells are skipped.
pub(crate) fn attin_rows(text: &str) -> Result<Vec<(Handle, Vec<(String, String)>)>, ()> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
    let handle_col = header
        .iter()
        .position(|c| c.trim().eq_ignore_ascii_case("HANDLE"))
        .ok_or(())?;
    let mut rows = Vec::new();
    for line in lines {
        let cells: Vec<&str> = line.split('\t').collect();
        let Some(handle) = cells
            .get(handle_col)
            .map(|c| c.trim().trim_start_matches('\''))
            .and_then(|c| u64::from_str_radix(c, 16).ok())
        else {
            continue;
        };
        let values = header
            .iter()
            .zip(&cells)
            .filter(|(tag, value)| {
                let tag = tag.trim();
                !tag.eq_ignore_ascii_case("HANDLE")
                    && !tag.eq_ignore_ascii_case("BLOCKNAME")
                    && !tag.is_empty()
                    && **value != MISSING
            })
            .map(|(tag, value)| (tag.trim().to_string(), value.to_string()))
            .collect();
        rows.push((Handle::new(handle), values));
    }
    Ok(rows)
}

/// File bytes as text: UTF-16 (a spreadsheet's "Unicode text") or UTF-8.
fn decode(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => String::from_utf16_lossy(
            &rest
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect::<Vec<_>>(),
        ),
        [0xFE, 0xFF, rest @ ..] => String::from_utf16_lossy(
            &rest
                .chunks_exact(2)
                .map(|p| u16::from_be_bytes([p[0], p[1]]))
                .collect::<Vec<_>>(),
        ),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

impl OpenCADStudio {
    pub(super) fn dispatch_attout(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd.trim().to_ascii_uppercase().as_str() {
            "ATTOUT" => Some(self.attout(i)),
            "ATTIN" => Some(Task::perform(
                async {
                    let handle = crate::sys::file_dialog()
                        .set_title("ATTIN")
                        .add_filter(crate::t!("Text").as_ref(), &["txt"])
                        .add_filter(crate::t!("All Files").as_ref(), &["*"])
                        .pick_file()
                        .await?;
                    Some(decode(&handle.read().await))
                },
                Message::AttInPicked,
            )),
            _ => None,
        }
    }

    fn attout(&mut self, i: usize) -> Task<Message> {
        let handles = self.tabs[i].scene.selected_handles_in_order();
        if handles.is_empty() {
            use crate::command::CadCommand;
            let command =
                crate::modules::annotate::annotation_scale::AnnotationScaleSelectionCommand::objects(
                    "ATTOUT",
                );
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return Task::none();
        }
        let Some(text) = attout_text(&self.tabs[i].scene.document, &handles) else {
            self.command_line
                .push_error(crate::t!("ATTOUT: no blocks with attributes selected.").as_ref());
            return Task::none();
        };
        let name = self.tabs[i]
            .current_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| format!("{}.txt", s.to_string_lossy()))
            .unwrap_or_else(|| "attributes.txt".to_string());
        Task::perform(
            async move {
                let path = crate::sys::file_dialog()
                    .set_title("ATTOUT")
                    .set_file_name(&name)
                    .add_filter(crate::t!("Text").as_ref(), &["txt"])
                    .add_filter(crate::t!("All Files").as_ref(), &["*"])
                    .save_file()
                    .await
                    .map(|h| crate::sys::handle_path(&h));
                (text, path)
            },
            |(text, path)| Message::AttOutSave(text, path),
        )
    }

    pub(in crate::app) fn on_attout_save(&mut self, text: String, path: std::path::PathBuf) {
        let fname = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        #[cfg(target_arch = "wasm32")]
        let written: Result<(), String> = {
            crate::sys::download_bytes(&fname, text.as_bytes());
            Ok(())
        };
        #[cfg(not(target_arch = "wasm32"))]
        let written = std::fs::write(&path, text.as_bytes()).map_err(|e| e.to_string());
        match written {
            Ok(()) => {
                let count = text.lines().count().saturating_sub(1);
                self.command_line
                    .push_output(crate::tf!("ATTOUT: {count} block(s) written to \"{fname}\".").as_ref());
            }
            Err(e) => self
                .command_line
                .push_error(crate::tf!("ATTOUT: write failed: {e}").as_ref()),
        }
    }

    pub(in crate::app) fn on_attin_text(&mut self, text: String) {
        let i = self.active_tab;
        let Ok(rows) = attin_rows(&text) else {
            self.command_line
                .push_error(crate::t!("ATTIN: the file has no HANDLE column.").as_ref());
            return;
        };
        // Only the blocks whose values differ are touched (a mutable lookup
        // drops an entity's retained record).
        let scene = &self.tabs[i].scene;
        let edits: Vec<_> = rows
            .into_iter()
            .filter_map(|(handle, values)| {
                let Some(EntityType::Insert(insert)) = scene.document.get_entity(handle) else {
                    return None;
                };
                if scene.is_layer_locked(handle) {
                    return None;
                }
                let changes: Vec<_> = values
                    .into_iter()
                    .filter(|(tag, value)| {
                        insert.attributes.iter().any(|a| {
                            a.tag.eq_ignore_ascii_case(tag) && a.get_value() != value.as_str()
                        })
                    })
                    .collect();
                (!changes.is_empty()).then_some((handle, changes))
            })
            .collect();
        let count = edits.len();
        if count > 0 {
            self.push_undo_snapshot(i, "ATTIN");
            let mut changed = Vec::with_capacity(edits.len());
            for (handle, changes) in edits {
                if let Some(EntityType::Insert(insert)) =
                    self.tabs[i].scene.document.get_entity_mut(handle)
                {
                    for (tag, value) in changes {
                        if let Some(attribute) = insert
                            .attributes
                            .iter_mut()
                            .find(|a| a.tag.eq_ignore_ascii_case(&tag))
                        {
                            attribute.set_value(value);
                        }
                    }
                    changed.push(handle);
                }
            }
            self.invalidate_property_targets(i, &changed);
            self.tabs[i].dirty = true;
            self.refresh_properties();
        }
        self.command_line
            .push_output(crate::tf!("ATTIN: {count} block(s) updated.").as_ref());
    }
}
