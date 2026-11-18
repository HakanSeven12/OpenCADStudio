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
