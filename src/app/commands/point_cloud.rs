//! POINTCLOUDATTACH: the point cloud, its definition (one per file, in
//! ACAD_POINTCLOUD_EX_DICT) and the reactor linking them, added as one undo
//! step.

use super::*;
use document::objects::{
    ClassObject, ClassObjectData, Dictionary, ObjectType, PointCloudDefinition,
    PointCloudDefinitionReactor,
};
use document::types::{Handle, Vector3};

use crate::modules::insert::pc_attach::PointCloudPlacement;
use crate::scene::model::point_cloud;
use crate::ui::window::pdf_dialogs::PdfDialogMsg;
use std::sync::Arc;

const DEFINITIONS: &str = "ACAD_POINTCLOUD_EX_DICT";

fn vector(v: glam::DVec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

/// The point cloud definitions dictionary under the named-objects root,
/// created when the drawing has none.
fn definitions_dictionary(document: &mut document::Document) -> Option<Handle> {
    let root = document.header.named_objects_dict_handle;
    let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get(&root) else {
        return None;
    };
    if let Some(existing) = root_dictionary.get(DEFINITIONS) {
        if matches!(
            document.objects.get(&existing),
            Some(ObjectType::Dictionary(_))
        ) {
            return Some(existing);
        }
    }
    let handle = document.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = root;
    document
        .objects
        .insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get_mut(&root) {
        root_dictionary.add_entry(DEFINITIONS, handle);
    }
    Some(handle)
}

/// The definition for a file: the existing one, or a new one registered
/// under the file's name ("Chair", "Chair(2)" …). Its point count and
/// extents come from the file.
fn ensure_definition(
    document: &mut document::Document,
    path: &str,
    name: &str,
    points: &crate::scene::model::point_cloud::CloudPoints,
) -> (Handle, Vector3, Vector3) {
    let [lo, hi] = points.bounds.unwrap_or_default();
    let (min, max) = (
        Vector3::new(lo[0], lo[1], lo[2]),
        Vector3::new(hi[0], hi[1], hi[2]),
    );
    let same = |stored: &str| {
        stored
            .replace('/', "\\")
            .eq_ignore_ascii_case(&path.replace('/', "\\"))
    };
    let existing = document
        .objects
        .iter()
        .find_map(|(handle, object)| match object {
            ObjectType::ClassObject(object) => match &object.data {
                ClassObjectData::PointCloudDefinitionEx(def) if same(&def.source_filename) => {
                    Some(*handle)
                }
                _ => None,
            },
            _ => None,
        });
    if let Some(handle) = existing {
        return (handle, min, max);
    }
    let handle = document.allocate_handle();
    let mut definition = ClassObject::new(ClassObjectData::PointCloudDefinitionEx(
        PointCloudDefinition {
            class_version: 1,
            source_filename: path.to_string(),
            is_loaded: true,
            point_count: points.positions.len() as i64,
            extents_min: min,
            extents_max: max,
        },
    ));
    definition.handle = handle;
    if let Some(dictionary) = definitions_dictionary(document) {
        definition.owner = dictionary;
        definition.reactors.push(dictionary);
        if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&dictionary) {
            let mut key = name.to_string();
            let mut suffix = 1;
            while entries.get(&key).is_some() {
                suffix += 1;
                key = format!("{name}({suffix})");
            }
            entries.add_entry(key, handle);
        }
    }
    document
        .objects
        .insert(handle, ObjectType::ClassObject(definition));
    (handle, min, max)
}

impl OpenCADStudio {
    /// The end of POINTCLOUDATTACH: the file is read off the UI thread,
    /// and [`Self::finish_point_cloud_attach`] places the cloud when the
    /// read ends. A panic in the reader becomes an error on the command
    /// line.
    pub(crate) fn attach_point_cloud(
        &mut self,
        i: usize,
        label: String,
        placement: PointCloudPlacement,
    ) -> Task<Message> {
        self.attach_point_cloud_with(i, label, placement, point_cloud::load)
    }

    /// [`Self::attach_point_cloud`] with the reader given, so a test can
    /// give one that panics.
    pub(in crate::app) fn attach_point_cloud_with(
        &mut self,
        i: usize,
        label: String,
        placement: PointCloudPlacement,
        reader: fn(&std::path::Path) -> Option<Arc<point_cloud::CloudPoints>>,
    ) -> Task<Message> {
        let tab_id = self.tabs[i].id;
        let generation = self.tabs[i].document_generation;
        let path = placement.path.clone();
        crate::app::update::background_task(
            "POINTCLOUDATTACH",
            move || reader(&path),
            move |cloud| {
                Message::PdfDialog(PdfDialogMsg::CloudAttachRead(
                    tab_id,
                    generation,
                    Box::new((label, placement)),
                    cloud,
                ))
            },
        )
    }

    /// The -POINTCLOUDATTACH file name prompt: the file is read off the UI
    /// thread while the command asks for the insertion point. A panic in
    /// the reader becomes an error on the command line.
    pub(in crate::app) fn read_point_cloud_for_prompt(
        &mut self,
        i: usize,
        path: std::path::PathBuf,
    ) -> Task<Message> {
        self.read_point_cloud_for_prompt_with(i, path, point_cloud::load)
    }

    /// [`Self::read_point_cloud_for_prompt`] with the reader given, so a
    /// test can give one that panics.
    pub(in crate::app) fn read_point_cloud_for_prompt_with(
        &mut self,
        i: usize,
        path: std::path::PathBuf,
        reader: fn(&std::path::Path) -> Option<Arc<point_cloud::CloudPoints>>,
    ) -> Task<Message> {
        self.command_line.push_info(&format!(
            "Reading {}…",
            crate::entities::underlay::display_path(&path.to_string_lossy())
        ));
        let tab_id = self.tabs[i].id;
        self.point_cloud_prompt_read = Some((tab_id, path.clone()));
        let worker_path = path.clone();
        crate::app::update::background_task(
            "POINTCLOUDATTACH",
            move || reader(&worker_path).is_some(),
            move |found| Message::PdfDialog(PdfDialogMsg::CloudPromptRead(tab_id, path, found)),
        )
    }

    /// The end of the prompt's read. If no scan could be read, and the
    /// command that asked is still the current command of the current
    /// drawing, the command ends with the two lines that the prompt gave
    /// before the read moved to a worker. If not, the attach reads the file
    /// again and reports there.
    pub(in crate::app) fn finish_point_cloud_prompt_read(
        &mut self,
        tab_id: u64,
        path: &std::path::Path,
        found: bool,
    ) -> Task<Message> {
        if self
            .point_cloud_prompt_read
            .as_ref()
            .is_none_or(|(tab, read)| *tab != tab_id || read != path)
        {
            return Task::none();
        }
        self.point_cloud_prompt_read = None;
        let i = self.active_tab;
        let asked = !found
            && self.tabs[i].id == tab_id
            && self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.name() == "POINTCLOUDATTACH");
        if !asked {
            return Task::none();
        }
        self.apply_cmd_result(crate::command::CmdResult::Measurement(
            "All scans are not found or invalid.\nAttach point cloud failed".to_string(),
        ))
    }

    /// The end of a read that the drawing started
    /// ([`point_cloud::cached`]): each open drawing with a cloud of that
    /// file draws it again, now with its points, and the clouds that no
    /// open drawing refers to are let go. A reader that panicked gives an
    /// error; the drawing keeps the extents box.
    pub(in crate::app) fn finish_point_cloud_display_read(&mut self, done: &point_cloud::ReadDone) {
        if let Some(detail) = &done.failure {
            let shown = crate::entities::underlay::display_path(&done.path.to_string_lossy());
            self.command_line
                .push_error(&format!("Point cloud {shown} cannot be read: {detail}"));
        }
        let mut referenced = std::collections::HashSet::new();
        for tab in &mut self.tabs {
            let document = &tab.scene.document;
            let mut changed = Vec::new();
            for entity in document.entities() {
                let document::EntityType::Extended(extended) = entity else {
                    continue;
                };
                let document::entities::ExtendedEntityData::PointCloudEx(data) = &extended.data
                else {
                    continue;
                };
                let Some(path) = point_cloud::resolve_source(document, data) else {
                    continue;
                };
                if path == done.path {
                    changed.push((entity.common().handle, crate::scene::ChangeKind::Modified));
                }
                referenced.insert(path);
            }
            if !changed.is_empty() {
                tab.scene.bump_entities(&changed);
            }
        }
        point_cloud::release_unreferenced(|path| referenced.contains(path));
    }

    /// Places a cloud read off the UI thread, one undo step. A result for
    /// a drawing that was closed, replaced or cleared is dropped, and so is
    /// one for a drawing that is not the current one now (the commit
    /// writes to the current drawing).
    pub(in crate::app) fn finish_point_cloud_attach(
        &mut self,
        tab_id: u64,
        generation: u64,
        label: String,
        placement: PointCloudPlacement,
        cloud: Option<Arc<point_cloud::CloudPoints>>,
    ) {
        let Some(i) = self.import_target(tab_id, generation) else {
            return;
        };
        if i != self.active_tab {
            self.command_line.push_error(
                "Attach point cloud cancelled: a different drawing became current while the file was read.",
            );
            return;
        }
        let Some(points) = cloud else {
            self.command_line
                .push_error("All scans are not found or invalid.");
            self.command_line.push_error("Attach point cloud failed");
            return;
        };
        let path = placement.path.to_string_lossy().into_owned();
        let options = placement.options.clone();
        // The path stored: as chosen; relative to the drawing once it has a
        // file (until then the full path, made relative on the first save).
        let host = self.tabs[i].current_path.clone();
        let stored = match &options.stored {
            Some(stored) => stored.clone(),
            None if options.relative => host
                .as_deref()
                .and_then(|host| {
                    crate::io::xref_model::to_pathtype_result(
                        &path,
                        host,
                        crate::io::xref_model::Pathtype::Relative,
                    )
                    .ok()
                })
                .map(|relative| relative.replace('/', "\\"))
                .unwrap_or_else(|| path.clone()),
            None => path.clone(),
        };
        if stored != path {
            crate::scene::model::point_cloud::register_source(&stored, placement.path.clone());
        }
        let relative_later = options.relative && options.stored.is_none() && host.is_none();
        let name = placement
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let pending = self.begin_undo(i, label, 1, false);
        let document = &mut self.tabs[i].scene.document;
        // POINTCLOUDLOCK 1 attaches clouds locked.
        let locked = options.locked.unwrap_or_else(|| {
            crate::scene::model::point_cloud::setting("POINTCLOUDLOCK").is_some_and(|setting| {
                crate::scene::model::point_cloud::setting_value(document, setting) == 1
            })
        });
        let (definition, extents_min, extents_max) =
            ensure_definition(document, &stored, &name, &points);
        let [x, y, z] = placement.axes;
        // As the reference creates a cloud: scan colours, the intensity
        // range 0–100 as a gradient, crops shown.
        let data = document::entities::PointCloudExData {
            class_version: 1,
            extents_min,
            extents_max,
            ucs_origin: vector(placement.insertion),
            ucs_x_direction: vector(x),
            ucs_y_direction: vector(y),
            ucs_z_direction: vector(z),
            locked,
            definition_handle: definition,
            reactor_handle: Handle::NULL,
            name,
            show_intensity: false,
            show_cropping: true,
            hidden_scans: Vec::new(),
            hidden_regions: Vec::new(),
            stylization_type: 1,
            intensity_color_scheme: String::new(),
            current_color_scheme: String::new(),
            classification_color_scheme: String::new(),
            elevation_min: 0.0,
            elevation_max: 0.0,
            intensity_min: 0,
            intensity_max: 100,
            intensity_out_of_range_behavior: 1,
            elevation_out_of_range_behavior: 1,
            elevation_apply_to_fixed_range: false,
            intensity_as_gradient: true,
            elevation_as_gradient: false,
            croppings: Vec::new(),
        };
        let entity = document::EntityType::Extended(Box::new(document::entities::ExtendedEntity {
            common: document::entities::EntityCommon::new(),
            data: document::entities::ExtendedEntityData::PointCloudEx(data),
        }));
        if let Some(cloud) = self.commit_entity_handle(entity) {
            // The reactor, owned by the cloud, tells the definition who uses it.
            let document = &mut self.tabs[i].scene.document;
            let reactor = document.allocate_handle();
            let mut object = ClassObject::new(ClassObjectData::PointCloudDefinitionReactorEx(
                PointCloudDefinitionReactor { class_version: 1 },
            ));
            object.handle = reactor;
            object.owner = cloud;
            document
                .objects
                .insert(reactor, ObjectType::ClassObject(object));
            if let Some(ObjectType::ClassObject(definition)) = document.objects.get_mut(&definition)
            {
                definition.reactors.push(reactor);
            }
            if let Some(document::EntityType::Extended(extended)) = document.get_entity_mut(cloud) {
                if let document::entities::ExtendedEntityData::PointCloudEx(data) =
                    &mut extended.data
                {
                    data.reactor_handle = reactor;
                }
            }
            self.command_line.push_output("1 point cloud attached");
            if relative_later {
                self.tabs[i].xref_relative_on_save.insert(format!(
                    "{}{:X}",
                    crate::io::xref::POINT_CLOUD_KEY,
                    definition.value()
                ));
            }
            if options.zoom {
                self.tabs[i].scene.zoom_to_entities(&[cloud]);
            }
        }
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }
}

#[cfg(test)]
mod attach_read_tests {
    use super::*;
    use crate::modules::insert::pc_attach::AttachOptions;
    use crate::scene::model::point_cloud::test_scan_file;
    use glam::DVec3;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn placement(path: &std::path::Path) -> PointCloudPlacement {
        PointCloudPlacement {
            path: path.to_path_buf(),
            insertion: DVec3::ZERO,
            axes: [DVec3::X, DVec3::Y, DVec3::Z],
            options: AttachOptions::default(),
        }
    }

    fn clouds(app: &OpenCADStudio, i: usize) -> usize {
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| {
                matches!(
                    entity,
                    document::EntityType::Extended(extended)
                        if matches!(
                            extended.data,
                            document::entities::ExtendedEntityData::PointCloudEx(_)
                        )
                )
            })
            .count()
    }

    fn history(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The attach reads on a worker: nothing changes in the drawing until
    /// the worker ends, then the cloud is one undo step.
    #[test]
    fn the_cloud_is_placed_only_when_the_worker_ends() {
        let mut app = new_app();
        let i = app.active_tab;
        let path = test_scan_file("attach-placed", 40);
        let before = app.tabs[i].history.undo_stack.len();
        let task = app.attach_point_cloud(i, "POINTCLOUDATTACH".into(), placement(&path));
        assert_eq!(clouds(&app, i), 0, "nothing is placed during the read");
        app.drive_headless_task(task).expect("the attach task");
        assert_eq!(clouds(&app, i), 1);
        assert_eq!(app.tabs[i].history.undo_stack.len(), before + 1);
        assert!(
            history(&app).ends_with("1 point cloud attached"),
            "{}",
            history(&app)
        );
        let _ = std::fs::remove_file(path);
    }

    /// A reader that panics gives an error, the drawing does not change,
    /// and the next attach works.
    #[test]
    fn a_reader_panic_is_an_error_and_the_drawing_does_not_change() {
        let mut app = new_app();
        let i = app.active_tab;
        let path = test_scan_file("attach-panic", 10);
        let before = app.tabs[i].history.undo_stack.len();
        let task =
            app.attach_point_cloud_with(i, "POINTCLOUDATTACH".into(), placement(&path), |_| {
                panic!("a reader bug")
            });
        app.drive_headless_task(task).expect("the attach task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("POINTCLOUDATTACH failed: a reader bug")
        );
        assert_eq!(clouds(&app, i), 0);
        assert_eq!(app.tabs[i].history.undo_stack.len(), before);
        let task = app.attach_point_cloud(i, "POINTCLOUDATTACH".into(), placement(&path));
        app.drive_headless_task(task).expect("the attach task");
        assert_eq!(clouds(&app, i), 1);
        let _ = std::fs::remove_file(path);
    }

    /// A file with no scan that can be read gives the two errors when the
    /// worker ends, and the drawing does not change.
    #[test]
    fn a_file_that_cannot_be_read_gives_the_two_errors() {
        let mut app = new_app();
        let i = app.active_tab;
        let path = std::env::temp_dir().join(format!(
            "ocs-pointcloud-attach-place-bad-{}.rcs",
            std::process::id()
        ));
        std::fs::write(&path, b"not a scan").expect("write the bad scan");
        let task = app.attach_point_cloud(i, "POINTCLOUDATTACH".into(), placement(&path));
        app.drive_headless_task(task).expect("the attach task");
        let lines: Vec<&str> = app
            .command_line
            .history
            .iter()
            .rev()
            .take(2)
            .map(|entry| entry.text.as_str())
            .collect();
        assert!(
            lines[0].ends_with("Attach point cloud failed")
                && lines[1].ends_with("All scans are not found or invalid."),
            "lines: {lines:?}"
        );
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Attach point cloud failed")
        );
        assert_eq!(clouds(&app, i), 0);
        let _ = std::fs::remove_file(path);
    }

    /// A result for a drawing that was replaced during the read is
    /// dropped; the new drawing gets nothing.
    #[test]
    fn a_result_for_a_replaced_drawing_is_dropped() {
        let mut app = new_app();
        let i = app.active_tab;
        let path = test_scan_file("attach-replaced", 10);
        let task = app.attach_point_cloud(i, "POINTCLOUDATTACH".into(), placement(&path));
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.drive_headless_task(task).expect("the attach task");
        assert_eq!(clouds(&app, app.active_tab), 0);
        assert!(history(&app).contains("was replaced"), "{}", history(&app));
        let _ = std::fs::remove_file(path);
    }

    /// A result for a drawing that is no longer the current one is
    /// dropped: the commit writes to the current drawing, so neither
    /// drawing gets the cloud.
    #[test]
    fn a_result_for_a_drawing_that_is_not_current_is_dropped() {
        let mut app = new_app();
        let i = app.active_tab;
        let path = test_scan_file("attach-not-current", 10);
        let task = app.attach_point_cloud(i, "POINTCLOUDATTACH".into(), placement(&path));
        app.push_test_document();
        app.active_tab = app.tabs.len() - 1;
        app.drive_headless_task(task).expect("the attach task");
        assert_eq!(clouds(&app, i), 0);
        assert_eq!(clouds(&app, app.active_tab), 0);
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some(
                "Attach point cloud cancelled: a different drawing became current while the file was read."
            )
        );
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod prompt_read_tests {
    use super::*;
    use crate::command::StepInput;
    use crate::scene::model::point_cloud::test_scan_file;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn history(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn command(app: &OpenCADStudio) -> Option<&'static str> {
        app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .map(|command| command.name())
    }

    fn bad_scan(tag: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ocs-pointcloud-prompt-{tag}-{}.rcs",
            std::process::id()
        ));
        std::fs::write(&path, b"not a scan").expect("write the bad scan");
        path
    }

    /// The prompt asks for the insertion point at once, while the file is
    /// read; a good file keeps the command.
    #[test]
    fn the_prompt_asks_for_the_point_during_the_read() {
        let mut app = new_app();
        let path = test_scan_file("prompt-good", 10);
        let _ = app.dispatch_command("-POINTCLOUDATTACH");
        let task = app.feed_command(StepInput::Text(path.to_string_lossy().into_owned()));
        assert_eq!(command(&app), Some("POINTCLOUDATTACH"));
        let shown = crate::entities::underlay::display_path(&path.to_string_lossy());
        assert!(
            history(&app).contains(&format!("Reading {shown}…")),
            "{}",
            history(&app)
        );
        assert!(
            history(&app).ends_with("Specify insertion point <0,0>:"),
            "{}",
            history(&app)
        );
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(command(&app), Some("POINTCLOUDATTACH"));
        assert_eq!(app.command_line.last_error, None);
        let _ = std::fs::remove_file(path);
    }

    /// A file with no scan that can be read ends the command with the two
    /// lines when the read ends.
    #[test]
    fn a_bad_file_ends_the_command_when_the_read_ends() {
        let mut app = new_app();
        let path = bad_scan("bad");
        let _ = app.dispatch_command("-POINTCLOUDATTACH");
        let task = app.feed_command(StepInput::Text(path.to_string_lossy().into_owned()));
        assert_eq!(command(&app), Some("POINTCLOUDATTACH"));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(command(&app), None);
        assert!(
            history(&app)
                .ends_with("All scans are not found or invalid.\nAttach point cloud failed"),
            "{}",
            history(&app)
        );
        let _ = std::fs::remove_file(path);
    }

    /// A failed read for a command that ended before the read ended
    /// changes nothing: the command that is current now continues.
    #[test]
    fn a_failed_read_after_the_command_ended_changes_nothing() {
        let mut app = new_app();
        let path = bad_scan("ended");
        let _ = app.dispatch_command("-POINTCLOUDATTACH");
        let task = app.feed_command(StepInput::Text(path.to_string_lossy().into_owned()));
        let _ = app.feed_command(StepInput::Escape);
        let _ = app.dispatch_command("LINE");
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(command(&app), Some("LINE"));
        assert!(
            !history(&app).contains("All scans are not found or invalid."),
            "{}",
            history(&app)
        );
        let _ = std::fs::remove_file(path);
    }

    /// A failed read for a command in another drawing changes nothing:
    /// the -POINTCLOUDATTACH command of the current drawing continues.
    #[test]
    fn a_failed_read_for_another_drawing_changes_nothing() {
        let mut app = new_app();
        let bad = bad_scan("other-tab");
        let _ = app.dispatch_command("-POINTCLOUDATTACH");
        let task = app.feed_command(StepInput::Text(bad.to_string_lossy().into_owned()));
        app.push_test_document();
        app.active_tab = app.tabs.len() - 1;
        let _ = app.dispatch_command("-POINTCLOUDATTACH");
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(command(&app), Some("POINTCLOUDATTACH"));
        assert!(
            !history(&app).contains("All scans are not found or invalid."),
            "{}",
            history(&app)
        );
        let _ = std::fs::remove_file(bad);
    }

    /// A failed read for an older file name changes nothing: the read of
    /// the newer name is the one that counts.
    #[test]
    fn a_failed_read_for_an_older_file_name_changes_nothing() {
        let mut app = new_app();
        let i = app.active_tab;
        let bad = bad_scan("older");
        let good = test_scan_file("prompt-newer", 10);
        let _ = app.dispatch_command("-POINTCLOUDATTACH");
        let older = app.feed_command(StepInput::Text(bad.to_string_lossy().into_owned()));
        let newer = app.read_point_cloud_for_prompt(i, good.clone());
        app.drive_headless_task(older).expect("the older read");
        assert_eq!(command(&app), Some("POINTCLOUDATTACH"));
        assert!(
            !history(&app).contains("All scans are not found or invalid."),
            "{}",
            history(&app)
        );
        app.drive_headless_task(newer).expect("the newer read");
        assert_eq!(command(&app), Some("POINTCLOUDATTACH"));
        let _ = std::fs::remove_file(bad);
        let _ = std::fs::remove_file(good);
    }

    /// A reader that panics gives an error, and the command continues (the
    /// attach reads the file again at its end).
    #[test]
    fn a_reader_panic_is_an_error_and_the_command_continues() {
        let mut app = new_app();
        let i = app.active_tab;
        let _ = app.dispatch_command("-POINTCLOUDATTACH");
        let task = app
            .read_point_cloud_for_prompt_with(i, "panics.rcs".into(), |_| panic!("a reader bug"));
        app.drive_headless_task(task).expect("the read task");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("POINTCLOUDATTACH failed: a reader bug")
        );
        assert_eq!(command(&app), Some("POINTCLOUDATTACH"));
    }

    /// The full prompt sequence attaches the cloud when the attach's read
    /// ends.
    #[test]
    fn the_prompts_attach_the_cloud() {
        let mut app = new_app();
        let i = app.active_tab;
        let path = test_scan_file("prompt-attach", 10);
        let _ = app.dispatch_command("-POINTCLOUDATTACH");
        let task = app.feed_command(StepInput::Text(path.to_string_lossy().into_owned()));
        app.drive_headless_task(task).expect("the read task");
        let mut last = Task::none();
        for _ in 0..3 {
            last = app.feed_command(StepInput::Enter);
        }
        app.drive_headless_task(last).expect("the attach task");
        let clouds = app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| matches!(entity, document::EntityType::Extended(_)))
            .count();
        assert_eq!(clouds, 1, "{}", history(&app));
        assert!(
            history(&app).ends_with("1 point cloud attached"),
            "{}",
            history(&app)
        );
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod display_read_tests {
    use super::*;
    use crate::modules::insert::pc_attach::AttachOptions;
    use crate::scene::model::point_cloud::{
        test_held_lock, test_is_held, test_scan_file, test_wait_for_read,
    };
    use glam::DVec3;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// A drawing with an attached cloud of a new scan file of `points`
    /// points, and that file.
    fn attached(app: &mut OpenCADStudio, tag: &str, points: u32) -> std::path::PathBuf {
        let i = app.active_tab;
        let path = test_scan_file(tag, points);
        let placement = PointCloudPlacement {
            path: path.clone(),
            insertion: DVec3::ZERO,
            axes: [DVec3::X, DVec3::Y, DVec3::Z],
            options: AttachOptions::default(),
        };
        let task = app.attach_point_cloud(i, "POINTCLOUDATTACH".into(), placement);
        app.drive_headless_task(task).expect("the attach task");
        path
    }

    fn cloud_data(app: &OpenCADStudio) -> document::entities::PointCloudExData {
        let document = &app.tabs[app.active_tab].scene.document;
        document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Extended(extended) => match &extended.data {
                    document::entities::ExtendedEntityData::PointCloudEx(data) => {
                        Some(data.clone())
                    }
                    _ => None,
                },
                _ => None,
            })
            .expect("the attached cloud")
    }

    /// A new version of the file: the drawing has nothing for it until a
    /// worker reads it, so it shows the extents box. When the read ends,
    /// the drawing has the points.
    #[test]
    fn the_drawing_shows_the_points_when_the_read_ends() {
        let _held = test_held_lock();
        let mut app = new_app();
        let path = attached(&mut app, "display-app", 25);
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(later))
            .expect("touch the scan");
        let data = cloud_data(&app);
        let document = &app.tabs[app.active_tab].scene.document;
        // Only the first look is sure to come before the read that it
        // starts: a second look can find the cloud already read.
        assert!(
            point_cloud::shows_no_points(document, &data),
            "the box shows during the read"
        );
        test_wait_for_read(&path);
        let epoch = app.tabs[app.active_tab].scene.geometry_epoch;
        let _ = app.update(Message::PointCloudRead(point_cloud::ReadDone {
            path: path.clone(),
            failure: None,
        }));
        assert_ne!(
            app.tabs[app.active_tab].scene.geometry_epoch, epoch,
            "the drawing draws the cloud again"
        );
        let document = &app.tabs[app.active_tab].scene.document;
        assert!(!point_cloud::shows_no_points(document, &data));
        let placed = point_cloud::placed(document, &data, [255; 3], None, &[])
            .expect("the points after the read");
        assert_eq!(placed.instances.len(), 25);
        assert!(test_is_held(&path), "a cloud the drawing shows is held");
        assert_eq!(app.command_line.last_error, None);
        let _ = std::fs::remove_file(path);
    }

    /// The Point Cloud Manager's list does not read: it shows no scans
    /// until the read ends.
    #[test]
    fn the_manager_list_does_not_read() {
        let _held = test_held_lock();
        let mut app = new_app();
        let path = attached(&mut app, "display-manager", 15);
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(later))
            .expect("touch the scan");
        let scans = |app: &OpenCADStudio| {
            crate::ui::window::pc_manager::clouds(&app.tabs[app.active_tab].scene.document, &[])
                .iter()
                .map(|cloud| cloud.scans.len())
                .sum::<usize>()
        };
        assert_eq!(scans(&app), 0, "no scans before the read ends");
        test_wait_for_read(&path);
        assert_eq!(scans(&app), 1);
        let _ = std::fs::remove_file(path);
    }

    /// The end of a read lets go of the clouds that no open drawing refers
    /// to.
    #[test]
    fn the_end_of_a_read_lets_go_of_unreferenced_clouds() {
        let _held = test_held_lock();
        let mut app = new_app();
        let unused = test_scan_file("display-unused", 10);
        assert!(point_cloud::cached(&unused).is_none());
        test_wait_for_read(&unused);
        assert!(test_is_held(&unused));
        let _ = app.update(Message::PointCloudRead(point_cloud::ReadDone {
            path: unused.clone(),
            failure: None,
        }));
        assert!(!test_is_held(&unused));
        let _ = std::fs::remove_file(unused);
    }

    /// A read that panicked gives one error with the file and the message.
    #[test]
    fn a_read_that_panicked_gives_an_error() {
        let _held = test_held_lock();
        let mut app = new_app();
        let path = std::path::PathBuf::from("display-panic.rcs");
        let _ = app.update(Message::PointCloudRead(point_cloud::ReadDone {
            path,
            failure: Some("a reader bug".into()),
        }));
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Point cloud display-panic.rcs cannot be read: a reader bug")
        );
    }
}
