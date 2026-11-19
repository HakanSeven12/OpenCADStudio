//! The files that node graph nodes read ("Read CSV", "Raster Image",
//! "Embedded Image"), read off the UI thread.
//!
//! A graph runs again after every edit, and one node's output goes to the
//! next node at once, so a node cannot wait for a read. A node asks this
//! cache for its file: a file read before, and not changed since (the same
//! modification time and size), is given at once. Otherwise a worker reads
//! the file, the node gives no output for now, and the graph runs again
//! when the read ends ([`OpenCADStudio::finish_graph_file_read`]). That run
//! joins the undo step of the run that started the read.

use super::{Message, OpenCADStudio};
use iced::Task;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

/// What a node reads from a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// The file's text ("Read CSV").
    Text,
    /// The image's size in pixels ("Raster Image").
    ImageSize,
    /// The image, prepared for embedding ("Embedded Image").
    Image,
}

/// What a read gave: `None` when the file cannot be read (or, for an
/// image, is no image).
#[derive(Clone, Debug)]
pub enum FileContent {
    Text(Option<Arc<String>>),
    ImageSize(Option<(u32, u32)>),
    Image(Option<Arc<crate::io::ole_embed::EmbeddedImage>>),
}

/// Which version of a file a read is for: its modification time and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl Stamp {
    /// The stamp of the file at `path` now; `None` when it does not exist.
    fn of(path: &std::path::Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

/// The end of a read: the drawing it is for, the file, its stamp when the
/// read started, and what the read gave.
#[derive(Clone, Debug)]
pub struct FileRead {
    pub(super) tab_id: u64,
    pub(super) generation: u64,
    pub(super) kind: FileKind,
    pub(super) path: PathBuf,
    pub(super) stamp: Stamp,
    /// What the read gave, or the panic of the reader.
    pub(super) content: Result<FileContent, String>,
}

#[derive(Clone, Debug)]
enum Entry {
    Reading(Stamp),
    Read(Stamp, FileContent),
}

/// A drawing's file cache. The entries that the last run did not use are
/// let go after the run.
#[derive(Clone, Debug, Default)]
pub struct GraphFiles {
    entries: HashMap<(FileKind, PathBuf), Entry>,
    used: HashSet<(FileKind, PathBuf)>,
    /// The runs in a row that a read started, since the last edit.
    reruns: u32,
}

/// The most runs in a row that reads start. A graph whose node writes a
/// file that another node reads, with new content at each run, would run
/// again and again without this limit.
const MAX_RERUNS: u32 = 8;

impl GraphFiles {
    /// Whether a read for this drawing has not ended yet.
    pub(super) fn reading(&self) -> bool {
        self.entries
            .values()
            .any(|entry| matches!(entry, Entry::Reading(_)))
    }

    /// An edit of the graph: the count of runs that reads started begins
    /// again.
    pub(super) fn edited(&mut self) {
        self.reruns = 0;
    }

    /// Starts a run: no entry is used yet.
    pub(super) fn begin_run(&mut self) {
        self.used.clear();
    }

    /// Ends a run: the entries that it did not use are let go (a read that
    /// has not ended stays, so that its result is not read again).
    pub(super) fn end_run(&mut self) {
        let used = &self.used;
        self.entries
            .retain(|key, entry| used.contains(key) || matches!(entry, Entry::Reading(_)));
    }
}

/// The content of a file that cannot be read.
fn nothing(kind: FileKind) -> FileContent {
    match kind {
        FileKind::Text => FileContent::Text(None),
        FileKind::ImageSize => FileContent::ImageSize(None),
        FileKind::Image => FileContent::Image(None),
    }
}

/// The file readers, run on a worker.
pub(in crate::app) type Reader = fn(FileKind, &std::path::Path) -> FileContent;

/// The readers the nodes used on the UI thread before.
pub(in crate::app) fn read_file(kind: FileKind, path: &std::path::Path) -> FileContent {
    match kind {
        FileKind::Text => FileContent::Text(std::fs::read_to_string(path).ok().map(Arc::new)),
        FileKind::ImageSize => FileContent::ImageSize(
            crate::io::image_file_size(path.to_path_buf())
                .ok()
                .map(|(_, width, height)| (width, height)),
        ),
        FileKind::Image => {
            let name = path
                .file_name()
                .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
                .into_owned();
            FileContent::Image(
                std::fs::read(path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| crate::io::ole_embed::EmbeddedImage::from_bytes(name, bytes))
                    .ok()
                    .map(Arc::new),
            )
        }
    }
}

impl OpenCADStudio {
    /// The content of the file at `path` for a node of drawing `i`, if it
    /// was read before and did not change since. If not, a read starts
    /// (its task is added to `tasks`) and the answer is `None`.
    pub(super) fn graph_file(
        &mut self,
        i: usize,
        kind: FileKind,
        path: PathBuf,
        tasks: &mut Vec<Task<Message>>,
    ) -> Option<FileContent> {
        let key = (kind, path);
        self.tabs[i].graph_files.used.insert(key.clone());
        let Some(stamp) = Stamp::of(&key.1) else {
            // A file that does not exist gives nothing, as a failed read did.
            self.tabs[i].graph_files.entries.remove(&key);
            return Some(nothing(kind));
        };
        match self.tabs[i].graph_files.entries.get(&key) {
            Some(Entry::Read(read, content)) if *read == stamp => return Some(content.clone()),
            Some(Entry::Reading(reading)) if *reading == stamp => return None,
            _ => {}
        }
        self.tabs[i]
            .graph_files
            .entries
            .insert(key.clone(), Entry::Reading(stamp));
        let reader = self.graph_file_reader;
        let (tab_id, generation) = (self.tabs[i].id, self.tabs[i].document_generation);
        let (kind, path) = key;
        tasks.push(crate::app::update::background_task(
            "Node graph",
            {
                let path = path.clone();
                // A panic gives no content and an error; the entry must end,
                // or the file is never read again and the step stays open.
                move || {
                    #[cfg(target_arch = "wasm32")]
                    {
                        Ok(reader(kind, &path))
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            reader(kind, &path)
                        }))
                        .map_err(crate::app::update::panic_detail)
                    }
                }
            },
            move |content| {
                Message::GraphFileRead(Box::new(FileRead {
                    tab_id,
                    generation,
                    kind,
                    path,
                    stamp,
                    content,
                }))
            },
        ));
        None
    }

    /// Takes a finished read into the cache of its drawing, and runs that
    /// drawing's graph again, in the undo step of the run that started the
    /// read when that step is still open. A read for a drawing that was
    /// closed or replaced, or for a file that changed or that the graph no
    /// longer reads, is dropped.
    pub(in crate::app) fn finish_graph_file_read(&mut self, read: FileRead) -> Task<Message> {
        let Some(i) = self
            .tabs
            .iter()
            .position(|tab| tab.id == read.tab_id && tab.document_generation == read.generation)
        else {
            return Task::none();
        };
        let key = (read.kind, read.path);
        let entry = self.tabs[i].graph_files.entries.get(&key);
        if !matches!(entry, Some(Entry::Reading(stamp)) if *stamp == read.stamp) {
            return Task::none();
        }
        let content = match read.content {
            Ok(content) => content,
            Err(detail) => {
                self.command_line
                    .push_error(&format!("Node graph failed: {detail}"));
                nothing(read.kind)
            }
        };
        let files = &mut self.tabs[i].graph_files;
        files.entries.insert(key, Entry::Read(read.stamp, content));
        if files.reruns >= MAX_RERUNS {
            self.command_line.push_error(
                "Node graph: a file that the graph reads changed at each run; the graph stopped running again.",
            );
            if let Some(pending) = self.tabs[i].history.pending.as_mut() {
                pending.held = false;
            }
            return Task::none();
        }
        files.reruns += 1;
        self.graph_rerun_after_read(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::node_graph::{GraphMsg, PaletteItem};
    use graph::{NodeId, Port};
    use serde_json::{json, Value};

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// A CSV file with one column of two numbers, written under `name`.
    fn csv_file(name: &str, numbers: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("ocs-graph-{name}-{}.csv", std::process::id()));
        std::fs::write(&path, numbers).expect("write the CSV file");
        path
    }

    /// Adds a "Read CSV" node for `path` to the current drawing's graph;
    /// returns its id and the task of the run, not run.
    fn read_csv(app: &mut OpenCADStudio, path: &std::path::Path) -> (NodeId, Task<Message>) {
        let i = app.active_tab;
        let spec = crate::app::node_graph::ops::resolve("Read CSV").expect("node type");
        let id = app.tabs[i]
            .graph
            .add_node(PaletteItem::Op(spec), Vec::new(), iced::Point::ORIGIN);
        let port = Port {
            node: id,
            field: "path".to_string(),
        };
        let task = app.on_graph(GraphMsg::Set(port, json!(path.to_string_lossy())));
        (id, task)
    }

    fn column(app: &OpenCADStudio, id: NodeId) -> Value {
        let graph = &app.tabs[app.active_tab].graph.engine;
        graph
            .node(id)
            .and_then(|node| node.outputs.get("column 1").cloned())
            .unwrap_or(Value::Null)
    }

    fn undo_steps(app: &mut OpenCADStudio) -> usize {
        app.finish_all_pending_history();
        app.tabs[app.active_tab].history.undo_stack.len()
    }

    /// The node gives nothing during the read, and its columns when the
    /// read ends; the run after the read joins the undo step of the edit.
    #[test]
    fn a_csv_node_gives_its_columns_when_the_read_ends() {
        let mut app = new_app();
        let path = csv_file("columns", "x\n1\n2\n");
        let before = undo_steps(&mut app);
        let (id, task) = read_csv(&mut app, &path);
        assert_eq!(column(&app, id), Value::Null, "nothing during the read");
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(column(&app, id), json!([1.0, 2.0]));
        assert_eq!(undo_steps(&mut app), before + 1, "one undo step");
        let _ = std::fs::remove_file(path);
    }

    /// A file that did not change is not read again: the next run gives
    /// its columns at once. A changed file is read again.
    #[test]
    fn only_a_changed_file_is_read_again() {
        let mut app = new_app();
        let i = app.active_tab;
        let path = csv_file("changed", "x\n1\n2\n");
        let (id, task) = read_csv(&mut app, &path);
        app.drive_headless_task(task).expect("the read task");
        let task = app.graph_run(i);
        assert_eq!(column(&app, id), json!([1.0, 2.0]), "no new read");
        assert!(!app.tabs[i].graph_files.reading());
        app.drive_headless_task(task).expect("the run's task");
        std::fs::write(&path, "x\n1\n2\n3\n").expect("change the file");
        let task = app.graph_run(i);
        assert_eq!(column(&app, id), Value::Null, "the changed file is read");
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(column(&app, id), json!([1.0, 2.0, 3.0]));
        let _ = std::fs::remove_file(path);
    }

    /// A reader that panics gives an error and no output; the undo step
    /// closes, and the next change of the file is read.
    #[test]
    fn a_reader_panic_gives_an_error_and_no_output() {
        let mut app = new_app();
        let i = app.active_tab;
        app.graph_file_reader = |_, _| panic!("a reader bug");
        let path = csv_file("panics", "x\n1\n");
        let (id, task) = read_csv(&mut app, &path);
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Node graph failed: a reader bug")
        );
        assert_eq!(column(&app, id), Value::Null);
        assert!(!app.tabs[i].graph_files.reading());
        assert!(app.tabs[i]
            .history
            .pending
            .as_ref()
            .is_none_or(|pending| !pending.held));
        let _ = std::fs::remove_file(path);
    }

    /// A file that changes at each read does not make the graph run
    /// without end: after the limit, an error, and the undo step closes.
    #[test]
    fn a_file_that_changes_at_each_read_stops_the_reruns() {
        let mut app = new_app();
        let i = app.active_tab;
        app.graph_file_reader = |kind, path| {
            let content = read_file(kind, path);
            let mut text = std::fs::read_to_string(path).unwrap_or_default();
            text.push_str("1\n");
            let _ = std::fs::write(path, text);
            content
        };
        let path = csv_file("changing", "x\n1\n");
        let (_, task) = read_csv(&mut app, &path);
        app.drive_headless_task(task).expect("the read tasks");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Node graph: a file that the graph reads changed at each run; the graph stopped running again.")
        );
        assert!(app.tabs[i]
            .history
            .pending
            .as_ref()
            .is_none_or(|pending| !pending.held));
        let _ = std::fs::remove_file(path);
    }

    /// A read for a drawing that was replaced during the read is dropped.
    #[test]
    fn a_read_for_a_replaced_drawing_is_dropped() {
        let mut app = new_app();
        let i = app.active_tab;
        let path = csv_file("replaced", "x\n1\n");
        let (id, task) = read_csv(&mut app, &path);
        app.tabs[i].document_generation += 1;
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(column(&app, id), Value::Null);
        let _ = std::fs::remove_file(path);
    }

    /// The image readers give a picture's size and the picture for
    /// embedding, and nothing for a file that is no picture.
    #[test]
    fn the_image_readers_give_the_size_and_the_picture() {
        let dir = std::env::temp_dir();
        let picture = dir.join(format!("ocs-graph-image-{}.png", std::process::id()));
        image::RgbaImage::new(3, 2)
            .save(&picture)
            .expect("write the picture");
        let junk = dir.join(format!("ocs-graph-junk-{}.png", std::process::id()));
        std::fs::write(&junk, b"no picture").expect("write the junk file");
        assert!(matches!(
            read_file(FileKind::ImageSize, &picture),
            FileContent::ImageSize(Some((3, 2)))
        ));
        assert!(matches!(
            read_file(FileKind::Image, &picture),
            FileContent::Image(Some(_))
        ));
        assert!(matches!(
            read_file(FileKind::ImageSize, &junk),
            FileContent::ImageSize(None)
        ));
        assert!(matches!(
            read_file(FileKind::Image, &junk),
            FileContent::Image(None)
        ));
        let _ = std::fs::remove_file(picture);
        let _ = std::fs::remove_file(junk);
    }
}
