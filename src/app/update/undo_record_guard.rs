//! A change of the drawing must be in an undo step. In a test build, the
//! places that run a handler (`update`, `automation_op`, `control_request`)
//! compare each drawing before and after it, and fail the test when a
//! drawing changed with no undo record. The names that can do that are in
//! `undo_record_guard_known.txt`, with reasons. `OCS_UNDO_GUARD_LIST=<file>`
//! writes the names that the tests give now and fails no test.

use std::cell::Cell;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value;

use crate::app::{Message, OpenCADStudio};

thread_local! {
    /// How many guarded handlers run inside each other on this thread.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// The depth of a handler that changed a drawing with no undo record
    /// and whose name is in the list. The handlers around it (a smaller
    /// depth) see the same change and do not report it again.
    static KNOWN_AT: Cell<Option<u32>> = const { Cell::new(None) };
}

/// The names that can change a drawing with no undo record.
fn known() -> &'static HashSet<&'static str> {
    static KNOWN: OnceLock<HashSet<&'static str>> = OnceLock::new();
    KNOWN.get_or_init(|| parse_known(include_str!("undo_record_guard_known.txt")))
}

pub(super) fn parse_known(text: &'static str) -> HashSet<&'static str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

fn list_file() -> Option<&'static PathBuf> {
    static FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
    FILE.get_or_init(|| std::env::var_os("OCS_UNDO_GUARD_LIST").map(PathBuf::from))
        .as_ref()
}

/// The name of a message: its variant, and the variant of its first field
/// when that is an enum or a struct (`Linetype(SetCurrent)`).
pub(super) fn message_name(debug: &str) -> String {
    let ident = |text: &str| -> usize {
        text.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(text.len())
    };
    let first = ident(debug);
    let mut name = debug[..first].to_string();
    if let Some(rest) = debug[first..].strip_prefix('(') {
        if rest.starts_with(|c: char| c.is_ascii_uppercase()) {
            let second = ident(rest);
            let _ = write!(name, "({})", &rest[..second]);
        }
    }
    name
}

/// The messages that carry a line of the command line.
const COMMAND_LINES: [&str; 2] = ["ScriptLine", "CommandInput"];

/// The command that a command line message starts: the first word of its
/// text (`ScriptLine("tilemode 0")` gives `TILEMODE`).
pub(super) fn command_word(debug: &str) -> Option<String> {
    let (variant, rest) = debug.split_once("(\"")?;
    if !COMMAND_LINES.contains(&variant) {
        return None;
    }
    let word: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(40)
        .collect();
    (!word.is_empty()).then(|| word.to_ascii_uppercase())
}

/// The command that is active before the handler.
fn active_command(app: &OpenCADStudio) -> Option<String> {
    app.tabs
        .get(app.active_tab)
        .and_then(|tab| tab.active_cmd.as_ref())
        .map(|command| command.name().to_ascii_uppercase())
}

/// A message does different work in different commands, so the command is a
/// part of the name: the active one (`CommandEscape during BACTION`), or
/// with none active the one that a command line starts (`ScriptLine
/// TILEMODE`).
pub(super) fn full_name(name: String, active: Option<String>, started: Option<String>) -> String {
    match (active, started) {
        (Some(command), _) => format!("{name} during {command}"),
        (None, Some(command)) => format!("{name} {command}"),
        (None, None) => name,
    }
}

/// The name of a request: its kind and operation, and for `run` the command.
pub(super) fn request_name(kind: &str, request: &Value) -> String {
    let op = request["op"].as_str().unwrap_or("?");
    match request["cmd"]
        .as_str()
        .and_then(|cmd| cmd.split_whitespace().next())
    {
        Some(command) if op == "run" => format!("{kind}:run {}", command.to_ascii_uppercase()),
        _ => format!("{kind}:{op}"),
    }
}

/// The start of a `Debug` text; a message can hold a whole drawing.
struct Start(String);

impl std::fmt::Write for Start {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        if self.0.len() >= 160 {
            return Err(std::fmt::Error);
        }
        self.0.push_str(text);
        Ok(())
    }
}

struct TabBefore {
    id: u64,
    document: document::Document,
    undo_len: usize,
    redo_len: usize,
    edit_revision: u64,
    record_open: bool,
}

/// What the guard keeps from before one handler.
pub(in crate::app) struct Frame {
    name: String,
    /// 0 for the handler that no other guarded handler runs.
    depth: u32,
    tabs: Vec<TabBefore>,
}

impl Frame {
    pub(super) fn depth(&self) -> u32 {
        self.depth
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(self.depth));
        if self.depth == 0 {
            KNOWN_AT.with(|known| known.set(None));
        }
    }
}

/// A handler inside this one already answered for the change.
fn inner_answered(depth: u32) -> bool {
    KNOWN_AT.with(Cell::get).is_some_and(|at| at > depth)
}

fn record_open(tab: &crate::app::DocumentTab) -> bool {
    tab.history.pending.is_some() || tab.scene.is_recording_undo()
}

fn begin(app: &OpenCADStudio, name: String) -> Frame {
    let depth = DEPTH.with(|depth| {
        let now = depth.get();
        depth.set(now + 1);
        now
    });
    Frame {
        name,
        depth,
        tabs: app
            .tabs
            .iter()
            .map(|tab| TabBefore {
                id: tab.id,
                document: tab.scene.document.clone(),
                undo_len: tab.history.undo_stack.len(),
                redo_len: tab.history.redo_stack.len(),
                edit_revision: tab.edit_revision,
                record_open: record_open(tab),
            })
            .collect(),
    }
}

/// Before the handler of a message.
pub(in crate::app) fn begin_message(app: &OpenCADStudio, message: &Message) -> Frame {
    let mut start = Start(String::new());
    let _ = write!(start, "{message:?}");
    let name = full_name(
        message_name(&start.0),
        active_command(app),
        command_word(&start.0),
    );
    begin(app, name)
}

/// Before the handler of an automation or control request.
pub(in crate::app) fn begin_request(app: &OpenCADStudio, kind: &str, request: &Value) -> Frame {
    begin(
        app,
        full_name(request_name(kind, request), active_command(app), None),
    )
}

/// What one handler did to one drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Evidence {
    pub(super) changed: bool,
    /// An undo record was open before the handler or is open after it: the
    /// record has the change, or will have it.
    pub(super) record_open: bool,
    /// The handler added an undo step, or ran UNDO or REDO.
    pub(super) history_moved: bool,
}

impl Evidence {
    pub(super) fn has_no_undo_record(self) -> bool {
        self.changed && !self.record_open && !self.history_moved
    }
}

/// The parts of two drawings that differ, as a short text; `None` when the
/// drawings are equal.
fn difference(before: &mut document::Document, after: &mut document::Document) -> Option<String> {
    let mut added = 0usize;
    let mut modified = 0usize;
    let mut kept = 0usize;
    for entity in after.entities() {
        match before.get_entity(entity.common().handle) {
            None => added += 1,
            Some(old) => {
                kept += 1;
                // An entity that no handler touched is the same allocation.
                if !std::ptr::eq(old, entity) && old != entity {
                    modified += 1;
                }
            }
        }
    }
    let removed = before.entity_count() - kept;
    // All that is not an entity: tables, objects, header, block records.
    let old = before.snapshot_structure();
    let new = after.snapshot_structure();
    let mut parts: Vec<&str> = Vec::new();
    if old != new {
        macro_rules! part {
            ($($field:ident),*) => {$(
                if old.$field != new.$field {
                    parts.push(stringify!($field));
                }
            )*};
        }
        part!(
            header,
            layers,
            line_types,
            text_styles,
            block_records,
            dim_styles,
            app_ids,
            views,
            vports,
            ucss,
            objects
        );
        if parts.is_empty() {
            parts.push("other data");
        }
    }
    if added == 0 && modified == 0 && removed == 0 && parts.is_empty() {
        return None;
    }
    let mut text = format!("{added} entities added, {modified} modified, {removed} removed");
    if !parts.is_empty() {
        let _ = write!(text, "; changed: {}", parts.join(", "));
    }
    Some(text)
}

/// After the handler, and after the message boundary closed its undo
/// records. Fails the test when a drawing changed with no undo record.
pub(in crate::app) fn end(app: &mut OpenCADStudio, frame: Frame) {
    if let Some(report) = check(app, frame, known(), list_file().map(PathBuf::as_path)) {
        panic!("{report}");
    }
}

/// The report for one handler, or `None`. `known` has the names that can
/// change a drawing with no undo record; with `list`, each such name goes to
/// that file and nothing is reported.
pub(super) fn check(
    app: &mut OpenCADStudio,
    mut frame: Frame,
    known: &HashSet<&'static str>,
    list: Option<&Path>,
) -> Option<String> {
    let name = std::mem::take(&mut frame.name);
    for mut before in std::mem::take(&mut frame.tabs) {
        let Some(tab) = app.tabs.iter_mut().find(|tab| tab.id == before.id) else {
            continue;
        };
        let Some(what) = difference(&mut before.document, &mut tab.scene.document) else {
            continue;
        };
        let evidence = Evidence {
            changed: true,
            record_open: before.record_open || record_open(tab),
            history_moved: tab.history.undo_stack.len() != before.undo_len
                || tab.history.redo_stack.len() != before.redo_len
                || tab.edit_revision != before.edit_revision,
        };
        if !evidence.has_no_undo_record() {
            continue;
        }
        // This handler answers for the change to the handlers around it too.
        let answered = || KNOWN_AT.with(|known| known.set(Some(frame.depth)));
        if inner_answered(frame.depth) {
            answered();
            continue;
        }
        if let Some(path) = list {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = file.write_all(format!("{name}\n").as_bytes());
            }
            answered();
            continue;
        }
        if known.contains(name.as_str()) {
            answered();
            continue;
        }
        return Some(format!(
            "`{name}` changed a drawing and made no undo step ({what}). Open one \
             (`push_undo_snapshot`) before the change, or add the name with its \
             reason to src/app/update/undo_record_guard_known.txt."
        ));
    }
    None
}
