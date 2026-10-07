//! POINTCLOUDCOLORMAP: the Point Cloud Color Map dialog for a chosen cloud
//! (or the drawing's default schemes), and what OK / Apply write — the
//! drawing's colour map (its schemes) and the cloud's intensity and
//! elevation colouring — as one undo step.

use super::*;
use codec::entities::{ExtendedEntityData, PointCloudExData};
use codec::objects::{ClassObject, ClassObjectData, Dictionary, ObjectType, PointCloudColorMap, PointCloudColorRamp, PointCloudRampColor};
use codec::types::Handle;

use crate::ui::window::pdf_dialogs::{short, MapRamp, MapSlot, OutOfRange, PdfDialogMsg, PointCloudColorMapState};

const MAP_DICTIONARY: &str = "ACAD_POINTCLOUD_COLORMAP_DICT";
const MAP_ENTRY: &str = "ACAD_POINTCLOUD_COLORMAP";
/// The classification scheme the colour map names by default.
const CLASSIFICATION_SCHEME: &str = "8AF59545-D22F-46B7-923C-912F71ED57DE";

fn cloud_data(document: &codec::CadDocument, handle: Handle) -> Option<&PointCloudExData> {
    match document.get_entity(handle)? {
        codec::EntityType::Extended(extended) => match &extended.data {
            ExtendedEntityData::PointCloudEx(data) => Some(data),
            _ => None,
        },
        _ => None,
    }
}

/// The drawing's colour map object, created (with the built-in schemes)
/// under its dictionary when the drawing has none.
fn ensure_color_map(document: &mut codec::CadDocument) -> Option<Handle> {
    let existing = document.objects.iter().find_map(|(handle, object)| match object {
        ObjectType::ClassObject(object) if matches!(object.data, ClassObjectData::PointCloudColorMap(_)) => Some(*handle),
        _ => None,
    });
    if existing.is_some() {
        return existing;
    }
    let root = document.header.named_objects_dict_handle;
    let ObjectType::Dictionary(root_dictionary) = document.objects.get(&root)? else {
        return None;
    };
    let dictionary = match root_dictionary.get(MAP_DICTIONARY) {
        Some(handle) if matches!(document.objects.get(&handle), Some(ObjectType::Dictionary(_))) => handle,
        _ => {
            let handle = document.allocate_handle();
            let mut dictionary = Dictionary::new();
            dictionary.handle = handle;
            dictionary.owner = root;
            document.objects.insert(handle, ObjectType::Dictionary(dictionary));
            if let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get_mut(&root) {
                root_dictionary.add_entry(MAP_DICTIONARY, handle);
            }
            handle
        }
    };
    let handle = document.allocate_handle();
    let ramps = crate::scene::model::point_cloud::schemes(document)
        .into_iter()
        .map(|(id, name, colors)| to_codec_ramp(&MapRamp { id, name, colors }))
        .collect();
    let mut map = ClassObject::new(ClassObjectData::PointCloudColorMap(PointCloudColorMap {
        class_version: 4,
        default_intensity_scheme: crate::entities::extended::point_cloud_ramp_id("Spectrum").to_string(),
        default_elevation_scheme: crate::entities::extended::point_cloud_ramp_id("Earth").to_string(),
        default_classification_scheme: CLASSIFICATION_SCHEME.to_string(),
        color_ramps: ramps,
        classification_color_ramps: Vec::new(),
    }));
    map.handle = handle;
    map.owner = dictionary;
    document.objects.insert(handle, ObjectType::ClassObject(map));
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
        dictionary.add_entry(MAP_ENTRY, handle);
    }
    Some(handle)
}

/// A scheme as the colour map stores it: true colours, all shown.
fn to_codec_ramp(ramp: &MapRamp) -> PointCloudColorRamp {
    PointCloudColorRamp {
        id: ramp.id.clone(),
        class_version: 1,
        colors: ramp
            .colors
            .iter()
            .map(|[r, g, b]| PointCloudRampColor {
                color: (0xC2u32 << 24 | u32::from(*r) << 16 | u32::from(*g) << 8 | u32::from(*b)) as i32,
                visible: true,
            })
            .collect(),
        name: ramp.name.clone(),
    }
}

/// A new scheme identifier: a random GUID in the stored form.
fn new_scheme_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS random source");
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

/// `count` colours taken evenly along a scheme's blend.
fn resample(colors: &[[u8; 3]], count: usize) -> Vec<[u8; 3]> {
    let count = count.clamp(2, 25);
    (0..count)
        .map(|k| crate::ui::window::pdf_dialogs::blend(colors, k as f64 / (count - 1) as f64))
        .collect()
}

impl OpenCADStudio {
    /// POINTCLOUDCOLORMAP after the prompt: the dialog for `cloud`, or for
    /// the drawing's default schemes.
    pub(in crate::app) fn open_point_cloud_color_map(&mut self, i: usize, cloud: Option<Handle>) {
        let document = &self.tabs[i].scene.document;
        let ramps: Vec<MapRamp> = crate::scene::model::point_cloud::schemes(document)
            .into_iter()
            .map(|(id, name, colors)| MapRamp { id, name, colors })
            .collect();
        let defaults = crate::scene::model::point_cloud::default_schemes(document);
        let data = cloud.and_then(|handle| cloud_data(document, handle));
        let height = data
            .map(|data| {
                let corners = codec::entities::point_cloud_ex_corners(data);
                let low = corners.iter().map(|c| c.z).fold(f64::MAX, f64::min);
                let high = corners.iter().map(|c| c.z).fold(f64::MIN, f64::max);
                (low, high)
            })
            .unwrap_or((0.0, 0.0));
        let slot = |scheme: &str, fallback: &str, gradient: bool, max: String, min: String, out: i16| MapSlot {
            scheme: if scheme.is_empty() { fallback.to_string() } else { scheme.to_string() },
            gradient,
            max,
            min,
            out_of_range: OutOfRange(out),
        };
        let (intensity, elevation, extents, elevation_tab) = match data {
            Some(data) => (
                slot(
                    &data.intensity_color_scheme,
                    &defaults.0,
                    data.intensity_as_gradient,
                    data.intensity_max.to_string(),
                    data.intensity_min.to_string(),
                    data.intensity_out_of_range_behavior,
                ),
                slot(
                    &data.current_color_scheme,
                    &defaults.1,
                    data.elevation_as_gradient,
                    short(if data.elevation_apply_to_fixed_range { data.elevation_max } else { height.1 }),
                    short(if data.elevation_apply_to_fixed_range { data.elevation_min } else { height.0 }),
                    data.elevation_out_of_range_behavior,
                ),
                !data.elevation_apply_to_fixed_range,
                data.stylization_type == 4,
            ),
            None => (
                slot("", &defaults.0, true, String::new(), String::new(), 1),
                slot("", &defaults.1, false, String::new(), String::new(), 1),
                true,
                false,
            ),
        };
        let mut state = PointCloudColorMapState {
            cloud: cloud.filter(|_| data.is_some()),
            elevation_tab,
            ramps,
            intensity,
            elevation,
            extents,
            height: (clean(height.0), clean(height.1)),
            make_current: true,
            interval: String::new(),
            naming: None,
        };
        refresh_interval(&mut state);
        self.point_cloud_color_map = Some(state);
        self.active_modal = Some(crate::app::ModalKind::PointCloudColorMap);
    }

    pub(in crate::app) fn update_point_cloud_color_map(&mut self, message: PdfDialogMsg) -> Task<Message> {
        let i = self.active_tab;
        let Some(state) = self.point_cloud_color_map.as_mut() else {
            return Task::none();
        };
        match message {
            PdfDialogMsg::MapTab(elevation) => {
                state.elevation_tab = elevation;
                state.naming = None;
                refresh_interval(state);
            }
            PdfDialogMsg::MapScheme(name) => {
                if let Some(ramp) = state.ramps.iter().find(|r| r.name == name) {
                    state.slot_mut().scheme = ramp.id.clone();
                    refresh_interval(state);
                }
            }
            PdfDialogMsg::MapCount(count) => {
                if let Some(ramp) = state.ramp_mut() {
                    ramp.colors = resample(&ramp.colors, count);
                }
                refresh_interval(state);
            }
            // The colours between the two ends, blended evenly.
            PdfDialogMsg::MapEven => {
                if let Some(ramp) = state.ramp_mut() {
                    let (first, last) = (ramp.colors[0], ramp.colors[ramp.colors.len() - 1]);
                    ramp.colors = resample(&[first, last], ramp.colors.len());
                }
            }
            PdfDialogMsg::MapReverse => {
                if let Some(ramp) = state.ramp_mut() {
                    ramp.colors.reverse();
                }
            }
            PdfDialogMsg::MapGradient(on) => state.slot_mut().gradient = on,
            PdfDialogMsg::MapNew => {
                // "Color Scheme 0", or the first number not taken.
                let name = (0..)
                    .map(|n| format!("Color Scheme {n}"))
                    .find(|name| !state.ramps.iter().any(|r| r.name.eq_ignore_ascii_case(name)))
                    .unwrap_or_default();
                state.naming = Some((true, name));
            }
            PdfDialogMsg::MapRename => {
                let name = state.ramp().map(|r| r.name.clone()).unwrap_or_default();
                state.naming = Some((false, name));
            }
            PdfDialogMsg::MapNameInput(value) => {
                if let Some((_, name)) = state.naming.as_mut() {
                    *name = value;
                }
            }
            PdfDialogMsg::MapNameCancel => state.naming = None,
            PdfDialogMsg::MapNameOk => {
                let Some((new, name)) = state.naming.take() else {
                    return Task::none();
                };
                let name = name.trim().to_string();
                let taken = state.ramps.iter().any(|r| r.name.eq_ignore_ascii_case(&name))
                    && !(!new && state.ramp().is_some_and(|r| r.name == name));
                if name.is_empty() || taken {
                    self.command_line.push_error(crate::t!("A color scheme with this name already exists.").as_ref());
                    self.point_cloud_color_map.as_mut().map(|s| s.naming = Some((new, name)));
                    return Task::none();
                }
                if new {
                    let colors = state.ramp().map(|r| r.colors.clone()).unwrap_or_else(|| vec![[255; 3], [0; 3]]);
                    let id = new_scheme_id();
                    state.ramps.push(MapRamp { id: id.clone(), name, colors });
                    state.slot_mut().scheme = id;
                } else if let Some(ramp) = state.ramp_mut() {
                    ramp.name = name;
                }
            }
            // The last scheme stays; a slot naming a deleted one takes the first.
            PdfDialogMsg::MapDelete => {
                if state.ramps.len() > 1 {
                    let id = state.ramp().map(|r| r.id.clone()).unwrap_or_default();
                    state.ramps.retain(|r| r.id != id);
                    let first = state.ramps[0].id.clone();
                    for slot in [&mut state.intensity, &mut state.elevation] {
                        if slot.scheme.eq_ignore_ascii_case(&id) {
                            slot.scheme = first.clone();
                        }
                    }
                    refresh_interval(state);
                }
            }
            PdfDialogMsg::MapMax(value) => {
                state.slot_mut().max = value;
                refresh_interval(state);
            }
            PdfDialogMsg::MapMin(value) => {
                state.slot_mut().min = value;
                refresh_interval(state);
            }
            // A band height sets the maximum from the minimum.
            PdfDialogMsg::MapInterval(value) => {
                let count = state.ramp().map_or(1, |r| r.colors.len()) as f64;
                if let (Ok(step), Ok(min)) = (value.trim().parse::<f64>(), state.elevation.min.trim().parse::<f64>()) {
                    state.elevation.max = short(min + step * count);
                }
                state.interval = value;
            }
            PdfDialogMsg::MapExtents(on) => {
                state.extents = on;
                if on {
                    state.elevation.max = short(state.height.1);
                    state.elevation.min = short(state.height.0);
                }
                refresh_interval(state);
            }
            PdfDialogMsg::MapOutOfRange(choice) => state.slot_mut().out_of_range = choice,
            PdfDialogMsg::MapCurrent(on) => state.make_current = on,
            PdfDialogMsg::MapApply => {
                self.apply_point_cloud_color_map(i);
            }
            PdfDialogMsg::MapOk => {
                if self.apply_point_cloud_color_map(i) {
                    self.point_cloud_color_map = None;
                    self.active_modal = None;
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// Writes the dialog's schemes and the cloud's colouring as one undo
    /// step; `false` (with a message) when a range value is not a number.
    fn apply_point_cloud_color_map(&mut self, i: usize) -> bool {
        let Some(state) = self.point_cloud_color_map.as_ref() else {
            return false;
        };
        let intensity = (state.intensity.min.trim().parse::<i32>(), state.intensity.max.trim().parse::<i32>());
        let elevation = (state.elevation.min.trim().parse::<f64>(), state.elevation.max.trim().parse::<f64>());
        let cloud = state.cloud;
        if cloud.is_some() {
            let ok_intensity = matches!(intensity, (Ok(lo), Ok(hi)) if (0..=100).contains(&lo) && (0..=100).contains(&hi) && lo < hi);
            let ok_elevation = state.extents || matches!(elevation, (Ok(lo), Ok(hi)) if lo < hi);
            if !ok_intensity || !ok_elevation {
                self.command_line.push_error(crate::t!("Invalid range of colorized points.").as_ref());
                return false;
            }
        }
        let ramps: Vec<PointCloudColorRamp> = state.ramps.iter().map(to_codec_ramp).collect();
        let (intensity_slot, elevation_slot) = (state.intensity.clone(), state.elevation.clone());
        let (extents, make_current, elevation_tab) = (state.extents, state.make_current, state.elevation_tab);
        self.push_undo_snapshot(i, "POINTCLOUDCOLORMAP");
        let document = &mut self.tabs[i].scene.document;
        if let Some(handle) = ensure_color_map(document) {
            if let Some(ObjectType::ClassObject(object)) = document.objects.get_mut(&handle) {
                if let ClassObjectData::PointCloudColorMap(map) = &mut object.data {
                    map.color_ramps = ramps;
                    if cloud.is_none() {
                        map.default_intensity_scheme = intensity_slot.scheme.clone();
                        map.default_elevation_scheme = elevation_slot.scheme.clone();
                    }
                }
            }
        }
        if let Some(handle) = cloud {
            if let Some(codec::EntityType::Extended(extended)) = document.get_entity_mut(handle) {
                if let ExtendedEntityData::PointCloudEx(data) = &mut extended.data {
                    data.intensity_color_scheme = intensity_slot.scheme;
                    data.intensity_as_gradient = intensity_slot.gradient;
                    data.intensity_out_of_range_behavior = intensity_slot.out_of_range.0;
                    if let (Ok(lo), Ok(hi)) = intensity {
                        (data.intensity_min, data.intensity_max) = (lo, hi);
                    }
                    data.current_color_scheme = elevation_slot.scheme;
                    data.elevation_as_gradient = elevation_slot.gradient;
                    data.elevation_out_of_range_behavior = elevation_slot.out_of_range.0;
                    data.elevation_apply_to_fixed_range = !extents;
                    if let (false, Ok(lo), Ok(hi)) = (extents, elevation.0, elevation.1) {
                        (data.elevation_min, data.elevation_max) = (lo, hi);
                    }
                    if make_current {
                        data.stylization_type = if elevation_tab { 4 } else { 5 };
                    }
                }
            }
        }
        // Every cloud colours from the schemes: redraw them all.
        let changes: Vec<_> = self.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| crate::scene::is_point_cloud(entity))
            .map(|entity| (entity.common().handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
        self.tabs[i].dirty = true;
        self.refresh_properties();
        true
    }

    pub(super) fn dispatch_pc_colormap(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        use crate::command::CadCommand;
        if cmd == "POINTCLOUDCOLORMAP" {
            let command = crate::modules::insert::pc_stylize::PointCloudColorMapCommand::new();
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return Some(self.finish_dispatch(cmd));
        }
        let rest = cmd.strip_prefix("_PCCOLORMAP")?;
        let cloud = u64::from_str_radix(rest.trim(), 16).ok().map(Handle::new);
        let task = self.finish_dispatch(cmd);
        self.open_point_cloud_color_map(i, cloud);
        Some(task)
    }
}

/// A height with rounding noise dropped (the extents box's -0.0016 reads 0).
fn clean(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn refresh_interval(state: &mut PointCloudColorMapState) {
    let count = state.ramp().map_or(1, |r| r.colors.len()).max(1) as f64;
    let lo = state.elevation.min.trim().parse::<f64>();
    let hi = state.elevation.max.trim().parse::<f64>();
    state.interval = match (lo, hi) {
        (Ok(lo), Ok(hi)) => short((hi - lo) / count),
        _ => String::new(),
    };
}
