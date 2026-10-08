//! POINTCLOUDATTACH: the point cloud, its definition (one per file, in
//! ACAD_POINTCLOUD_EX_DICT) and the reactor linking them, added as one undo
//! step.

use super::*;
use codec::objects::{
    ClassObject, ClassObjectData, Dictionary, ObjectType, PointCloudDefinition,
    PointCloudDefinitionReactor,
};
use codec::types::{Handle, Vector3};

use crate::modules::insert::pc_attach::PointCloudPlacement;

const DEFINITIONS: &str = "ACAD_POINTCLOUD_EX_DICT";

fn vector(v: glam::DVec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

/// The point cloud definitions dictionary under the named-objects root,
/// created when the drawing has none.
fn definitions_dictionary(document: &mut codec::CadDocument) -> Option<Handle> {
    let root = document.header.named_objects_dict_handle;
    let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get(&root) else {
        return None;
    };
    if let Some(existing) = root_dictionary.get(DEFINITIONS) {
        if matches!(document.objects.get(&existing), Some(ObjectType::Dictionary(_))) {
            return Some(existing);
        }
    }
    let handle = document.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = root;
    document.objects.insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get_mut(&root) {
        root_dictionary.add_entry(DEFINITIONS, handle);
    }
    Some(handle)
}

/// The definition for a file: the existing one, or a new one registered
/// under the file's name ("Chair", "Chair(2)" …). Its point count and
/// extents come from the file.
fn ensure_definition(
    document: &mut codec::CadDocument,
    path: &str,
    name: &str,
    points: &crate::scene::model::point_cloud::CloudPoints,
) -> (Handle, Vector3, Vector3) {
    let [lo, hi] = points.bounds.unwrap_or_default();
    let (min, max) = (Vector3::new(lo[0], lo[1], lo[2]), Vector3::new(hi[0], hi[1], hi[2]));
    let same = |stored: &str| stored.replace('/', "\\").eq_ignore_ascii_case(&path.replace('/', "\\"));
    let existing = document.objects.iter().find_map(|(handle, object)| match object {
        ObjectType::ClassObject(object) => match &object.data {
            ClassObjectData::PointCloudDefinitionEx(def) if same(&def.source_filename) => Some(*handle),
            _ => None,
        },
        _ => None,
    });
    if let Some(handle) = existing {
        return (handle, min, max);
    }
    let handle = document.allocate_handle();
    let mut definition = ClassObject::new(ClassObjectData::PointCloudDefinitionEx(PointCloudDefinition {
        class_version: 1,
        source_filename: path.to_string(),
        is_loaded: true,
        point_count: points.positions.len() as i64,
        extents_min: min,
        extents_max: max,
    }));
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
    document.objects.insert(handle, ObjectType::ClassObject(definition));
    (handle, min, max)
}

impl OpenCADStudio {
    pub(crate) fn attach_point_cloud(&mut self, i: usize, label: String, placement: PointCloudPlacement) {
        let Some(points) = crate::scene::model::point_cloud::load(&placement.path) else {
            self.command_line.push_error("All scans are not found or invalid.");
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
                    crate::io::xref_model::to_pathtype_result(&path, host, crate::io::xref_model::Pathtype::Relative).ok()
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
            crate::scene::model::point_cloud::setting("POINTCLOUDLOCK")
                .is_some_and(|setting| crate::scene::model::point_cloud::setting_value(document, setting) == 1)
        });
        let (definition, extents_min, extents_max) = ensure_definition(document, &stored, &name, &points);
        let [x, y, z] = placement.axes;
        // As the reference creates a cloud: scan colours, the intensity
        // range 0–100 as a gradient, crops shown.
        let data = codec::entities::PointCloudExData {
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
        let entity = codec::EntityType::Extended(Box::new(codec::entities::ExtendedEntity {
            common: codec::entities::EntityCommon::new(),
            data: codec::entities::ExtendedEntityData::PointCloudEx(data),
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
            document.objects.insert(reactor, ObjectType::ClassObject(object));
            if let Some(ObjectType::ClassObject(definition)) = document.objects.get_mut(&definition) {
                definition.reactors.push(reactor);
            }
            if let Some(codec::EntityType::Extended(extended)) = document.get_entity_mut(cloud) {
                if let codec::entities::ExtendedEntityData::PointCloudEx(data) = &mut extended.data {
                    data.reactor_handle = reactor;
                }
            }
            self.command_line.push_output("1 point cloud attached");
            if relative_later {
                self.tabs[i].xref_relative_on_save.insert(format!("{}{:X}", crate::io::xref::POINT_CLOUD_KEY, definition.value()));
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
