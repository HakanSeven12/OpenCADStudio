// Point cloud scans drawn as points: the scan files a PointCloudEx entity
// names are decoded once per file version, and the points it shows (placed
// in the drawing, crops applied, coloured) are kept per placement so a
// re-tessellate or a new render set reuses them.

mod rcp;
mod rcs;

use codec::entities::{PointCloudExCrop, PointCloudExData};
use codec::objects::{ClassObjectData, ObjectType, PointCloudDefinition};
use codec::types::{Transform, Vector3};
use codec::CadDocument;
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::SystemTime;

/// A scan file's points, in the file's own coordinates (the frame a
/// PointCloudEx places with its origin and axes).
pub struct CloudPoints {
    /// Unique per decoded file version: keys the placed point sets.
    id: u64,
    pub positions: Vec<[f64; 3]>,
    pub colors: Vec<[u8; 3]>,
    pub intensity: Vec<u8>,
    /// Unit normals in the file's frame (empty without normals).
    pub normals: Vec<[f32; 3]>,
    pub has_rgb: bool,
    pub has_intensity: bool,
    pub has_normals: bool,
    /// Scans read (one for a scan file).
    pub scans: usize,
    /// Each scan's name (its file stem) and its points' index range.
    pub scan_ranges: Vec<ScanRange>,
    /// A project's preview picture (JPEG).
    pub preview: Option<Vec<u8>>,
    /// Bounds from the scans' headers, placed like their points: the
    /// extents a definition records.
    pub bounds: Option<[[f64; 3]; 2]>,
}

/// Scan points by file path and modification time. A file that fails to
/// decode is remembered too, so it is not re-read on every tessellate.
pub fn load(path: &Path) -> Option<Arc<CloudPoints>> {
    type Cache = Mutex<FxHashMap<PathBuf, (SystemTime, Option<Arc<CloudPoints>>)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let mut cache = CACHE.get_or_init(Default::default).lock().ok()?;
    if let Some((stamp, cloud)) = cache.get(path) {
        if *stamp == modified {
            return cloud.clone();
        }
    }
    let started = crate::perf::enabled().then(iced::time::Instant::now);
    let cloud = decode(path).map(Arc::new);
    crate::perf::record(format_args!(
        "[perf] point-cloud-load {:>7.1}ms points={} {}",
        crate::perf::elapsed_ms(started),
        cloud.as_ref().map_or(0, |cloud| cloud.positions.len()),
        path.display(),
    ));
    cache.insert(path.to_path_buf(), (modified, cloud.clone()));
    cloud
}

fn decode(path: &Path) -> Option<CloudPoints> {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let bytes = std::fs::read(path).ok()?;
    let is_project = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rcp"));
    // A project lists its scans with their own transforms; a scan file
    // carries the same transform in its header.
    let stem = |p: &Path| p.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let scans: Vec<(rcs::Scan, [[f64; 3]; 3], String, String)> = if is_project {
        rcp::scans(&bytes, path)?
            .into_iter()
            .filter_map(|entry| {
                let scan = rcs::decode(&std::fs::read(&entry.path).ok()?)?;
                // The project's identifier, else the scan file's own.
                let id = if entry.id.is_empty() { scan.id.clone() } else { entry.id.clone() };
                Some((scan, [entry.translation, entry.rotation, entry.scale], stem(&entry.path), id))
            })
            .collect()
    } else {
        let scan = rcs::decode(&bytes)?;
        let transform = [scan.translation, scan.rotation, scan.scale];
        let id = scan.id.clone();
        vec![(scan, transform, stem(path), id)]
    };
    let mut cloud = CloudPoints {
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        positions: Vec::new(),
        colors: Vec::new(),
        intensity: Vec::new(),
        normals: Vec::new(),
        has_rgb: scans.iter().any(|(scan, ..)| scan.has_rgb),
        has_intensity: scans.iter().any(|(scan, ..)| scan.has_intensity),
        has_normals: scans.iter().any(|(scan, ..)| scan.has_normals),
        scans: scans.len(),
        scan_ranges: Vec::new(),
        preview: is_project.then(|| rcp::preview(&bytes)).flatten(),
        bounds: None,
    };
    for (scan, [translation, rotation, scale], name, id) in scans {
        let rotate = euler(rotation.map(f64::to_radians));
        let place = |p: &[f64; 3]| {
            let s = [p[0] * scale[0], p[1] * scale[1], p[2] * scale[2]];
            let r = rotate.map(|row| row[0] * s[0] + row[1] * s[1] + row[2] * s[2]);
            [r[0] + translation[0], r[1] + translation[1], r[2] + translation[2]]
        };
        let first = cloud.positions.len();
        cloud.positions.extend(scan.local.iter().map(place));
        let id = if id.is_empty() { name.clone() } else { id };
        cloud.scan_ranges.push(ScanRange { name, id, range: first..cloud.positions.len() });
        let [lo, hi] = scan.bounds;
        for corner in 0..8 {
            let p = place(&[
                if corner & 1 == 0 { lo[0] } else { hi[0] },
                if corner & 2 == 0 { lo[1] } else { hi[1] },
                if corner & 4 == 0 { lo[2] } else { hi[2] },
            ]);
            let [min, max] = cloud.bounds.get_or_insert([p, p]);
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        if scan.has_rgb || !cloud.has_rgb {
            cloud.colors.extend_from_slice(&scan.colors);
        } else {
            // A scan without colours among coloured ones shows its intensity.
            cloud.colors.extend(scan.intensity.iter().map(|&i| [i; 3]));
        }
        cloud.intensity.extend_from_slice(&scan.intensity);
        cloud.normals.extend(scan.normals.iter().map(|n| {
            let r = rotate.map(|row| row[0] * f64::from(n[0]) + row[1] * f64::from(n[1]) + row[2] * f64::from(n[2]));
            r.map(|v| v as f32)
        }));
    }
    Some(cloud)
}

/// Rotation matrix of a scan's rotation angles (radians here; projects and
/// scan headers store degrees), applied about X, then Y, then Z — the
/// reference's extents of rotated scans match this order.
fn euler([x, y, z]: [f64; 3]) -> [[f64; 3]; 3] {
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ]
}

/// The point cloud definition a PointCloudEx names.
pub(crate) fn definition<'a>(
    document: &'a CadDocument,
    data: &PointCloudExData,
) -> Option<&'a PointCloudDefinition> {
    match document.objects.get(&data.definition_handle)? {
        ObjectType::ClassObject(object) => match &object.data {
            ClassObjectData::PointCloudDefinitionEx(definition)
            | ClassObjectData::PointCloudDefinition(definition) => Some(definition),
            _ => None,
        },
        _ => None,
    }
}

type Sources = Mutex<FxHashMap<String, PathBuf>>;

fn sources() -> &'static Sources {
    static SOURCES: OnceLock<Sources> = OnceLock::new();
    SOURCES.get_or_init(Default::default)
}

/// Where a stored (relative or moved) scan path was found for an opened
/// drawing — the definition names the stored path.
// ponytail: keyed by the stored path alone, like underlay sources; two open
// drawings naming the same relative path in different folders share one.
pub(crate) fn register_source(stored: &str, resolved: PathBuf) {
    if let Ok(mut map) = sources().lock() {
        map.insert(stored.to_string(), resolved);
    }
}

/// The scan file a PointCloudEx shows, when it exists: the stored path as
/// is, else where it was found next to the drawing, else relative to the
/// working folder.
pub(crate) fn resolve_source(document: &CadDocument, data: &PointCloudExData) -> Option<PathBuf> {
    // An unloaded cloud draws as a missing one: its box and saved path.
    if definition(document, data).is_some_and(|def| !def.is_loaded) {
        return None;
    }
    let stored = definition(document, data)?.source_filename.trim();
    if stored.is_empty() {
        return None;
    }
    let path = PathBuf::from(stored.replace('\\', "/"));
    if path.is_absolute() && path.is_file() {
        return Some(path);
    }
    if let Some(found) = sources().lock().ok().and_then(|map| map.get(stored).cloned()) {
        if found.is_file() {
            return Some(found);
        }
    }
    path.is_file().then_some(path)
}

/// One drawn point: position as two f32 (high + low, like every other
/// renderer vertex), its colour and its world normal (zero without normals;
/// lit only then).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PointInstance {
    pub pos: [f32; 3],
    pub pos_low: [f32; 3],
    pub color: [u8; 4],
    pub normal: [i8; 4],
}

/// The points one cloud shows, placed in the drawing.
#[derive(Debug)]
pub struct PlacedCloud {
    /// Identity of the source file version, placement and crops: equal keys
    /// hold equal points, so GPU buffers are reused by key.
    pub key: u64,
    pub instances: Vec<PointInstance>,
}

/// Every cloud a view shows, with the POINTCLOUDPOINTSIZE they draw at.
#[derive(Debug, Default)]
pub struct PointCloudSet {
    pub point_size: f32,
    pub clouds: Vec<Arc<PlacedCloud>>,
}

/// POINTCLOUDPOINTSIZE in pixels (1–10, default 2).
pub(crate) fn point_size(document: &CadDocument) -> f32 {
    crate::io::drawing_variable(document, "POINTCLOUDPOINTSIZE")
        .and_then(|v| v.trim().parse::<i16>().ok())
        .unwrap_or(2)
        .clamp(1, 10) as f32
}

/// Whether the drawing defines any point cloud, so drawings without one
/// skip the render walk.
pub(crate) fn any_definition(document: &CadDocument) -> bool {
    document.objects.values().any(|object| {
        matches!(
            object,
            ObjectType::ClassObject(object)
                if matches!(object.data, ClassObjectData::PointCloudDefinitionEx(_)
                    | ClassObjectData::PointCloudDefinition(_))
        )
    })
}

/// One scan's points in a cloud: its name (the file's), its identifier and
/// its index range.
#[derive(Debug, Clone)]
pub struct ScanRange {
    pub name: String,
    pub id: String,
    pub range: std::ops::Range<usize>,
}

/// Stands in a hidden list for the cloud's unassigned points (region 0),
/// which are every point of a cloud without regions.
pub const UNASSIGNED_OFF: &str = "*UNASSIGNED";

/// What a cloud turns off: the identifiers of its hidden scans, and
/// [`UNASSIGNED_OFF`] when its unassigned points are hidden.
pub(crate) fn hidden(data: &PointCloudExData) -> Vec<String> {
    let mut hidden = data.hidden_scans.clone();
    if data.hidden_regions.contains(&0) {
        hidden.push(UNASSIGNED_OFF.to_string());
    }
    hidden
}

/// Whether a cloud draws none of its points: its file is not found, or its
/// unassigned points or every scan are turned off.
pub(crate) fn shows_no_points(document: &CadDocument, data: &PointCloudExData) -> bool {
    let Some(cloud) = resolve_source(document, data).and_then(|path| load(&path)) else {
        return true;
    };
    data.hidden_regions.contains(&0)
        || (!cloud.scan_ranges.is_empty() && cloud.scan_ranges.iter().all(|s| data.hidden_scans.contains(&s.id)))
}

/// The points `data` shows, placed through its origin and axes and then
/// `transform` (a block instance), with its crops applied and the `hidden`
/// scans left out. `None` when the scan file is missing or unreadable.
pub(crate) fn placed(
    document: &CadDocument,
    data: &PointCloudExData,
    object_color: [u8; 3],
    transform: Option<&Transform>,
    hidden: &[String],
) -> Option<Arc<PlacedCloud>> {
    let cloud = load(&resolve_source(document, data)?)?;
    let skipped: Vec<std::ops::Range<usize>> =
        if hidden.iter().any(|h| h == UNASSIGNED_OFF) {
            vec![0..cloud.positions.len()]
        } else {
            cloud.scan_ranges.iter().filter(|s| hidden.contains(&s.id)).map(|s| s.range.clone()).collect()
        };
    // The points shown: POINTCLOUDDENSITY per cent of POINTCLOUDPOINTMAX,
    // in tenths by POINTCLOUDLOD, every n-th point of a larger cloud.
    let shown = |name| setting(name).map_or(0, |s| setting_value(document, s));
    let limit =
        (shown("POINTCLOUDPOINTMAX") * shown("POINTCLOUDDENSITY") / 100 * shown("POINTCLOUDLOD") / 10).max(1) as usize;
    let step = cloud.positions.len().div_ceil(limit).max(1);
    let style = Stylization::of(document, data, object_color);
    let mut origin = data.ucs_origin;
    let mut axes = [data.ucs_x_direction, data.ucs_y_direction, data.ucs_z_direction];
    if let Some(transform) = transform {
        let placed = transform.apply(origin);
        axes = axes.map(|axis| transform.apply(origin + axis) - placed);
        origin = placed;
    }
    let crops: Vec<&PointCloudExCrop> = if data.show_cropping {
        data.croppings
            .iter()
            .filter(|crop| match crop.crop_type {
                1 | 2 => crop.points.len() >= 3,
                3 => crop.points.len() >= 2,
                _ => false,
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut h = rustc_hash::FxHasher::default();
    cloud.id.hash(&mut h);
    let bits = |v: &Vector3| [v.x, v.y, v.z].map(f64::to_bits);
    bits(&origin).hash(&mut h);
    axes.iter().map(bits).for_each(|v| v.hash(&mut h));
    for crop in &crops {
        let frame = [crop.plane, crop.x_direction, crop.y_direction];
        frame.iter().chain(&crop.points).map(bits).for_each(|v| v.hash(&mut h));
        (crop.inside, crop.inverted).hash(&mut h);
    }
    step.hash(&mut h);
    style.hash(&mut h);
    skipped.hash(&mut h);
    let key = h.finish();

    type Placed = Mutex<FxHashMap<u64, Weak<PlacedCloud>>>;
    static PLACED: OnceLock<Placed> = OnceLock::new();
    let placed_cache = PLACED.get_or_init(Default::default);
    if let Some(hit) = placed_cache.lock().ok()?.get(&key).and_then(Weak::upgrade) {
        return Some(hit);
    }

    let started = crate::perf::enabled().then(iced::time::Instant::now);
    let world = |p: &[f64; 3]| origin + axes[0] * p[0] + axes[1] * p[1] + axes[2] * p[2];
    // Elevation runs over the cloud's own height unless a fixed range is set.
    let (low, high) = if data.elevation_apply_to_fixed_range {
        (data.elevation_min, data.elevation_max)
    } else {
        cloud.positions.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
            let z = world(p).z;
            (lo.min(z), hi.max(z))
        })
    };
    let scan_color = |i: usize| {
        if cloud.has_rgb {
            cloud.colors[i]
        } else if cloud.has_intensity {
            [cloud.intensity[i]; 3]
        } else {
            object_color
        }
    };
    // A value outside a scheme's range takes the end colour, the scan
    // colour, or is hidden (`None`), as the cloud's out-of-range choice says.
    let along = |colors: &[[u8; 3]], t: f64, gradient: bool, out_of_range: i16, fixed: bool, i: usize| {
        if fixed && !(0.0..=1.0).contains(&t) {
            return match out_of_range {
                0 => Some(ramp(colors, t, gradient)),
                2 => None,
                _ => Some(scan_color(i)),
            };
        }
        Some(ramp(colors, t, gradient))
    };
    let color = |i: usize, z: f64| -> Option<[u8; 4]> {
        let [r, g, b] = match &style {
            Stylization::Object(c) => *c,
            Stylization::Normal => match cloud.normals.get(i) {
                Some(n) => {
                    let w = axes[0] * f64::from(n[0]) + axes[1] * f64::from(n[1]) + axes[2] * f64::from(n[2]);
                    let w = w / w.length().max(1e-12);
                    [w.x, w.y, w.z].map(|v| ((v + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8)
                }
                None => object_color,
            },
            Stylization::Ramp { colors, gradient, intensity: true, range, out_of_range } => {
                let percent = f64::from(cloud.intensity.get(i).copied().unwrap_or(0)) * 100.0 / 255.0;
                let (from, to) = (range.0 as f64, range.1 as f64);
                along(colors, (percent - from) / (to - from).max(1e-9), *gradient, *out_of_range, true, i)?
            }
            Stylization::Ramp { colors, gradient, out_of_range, .. } => along(
                colors,
                (z - low) / (high - low).max(1e-9),
                *gradient,
                *out_of_range,
                data.elevation_apply_to_fixed_range,
                i,
            )?,
            Stylization::Scan => scan_color(i),
        };
        Some([r, g, b, 255])
    };
    let mut instances = Vec::with_capacity(cloud.positions.len() / step + 1);
    for (i, p) in cloud.positions.iter().enumerate().step_by(step) {
        if skipped.iter().any(|r| r.contains(&i)) || !crops.iter().all(|crop| keeps(crop, p)) {
            continue;
        }
        let w = world(p);
        let Some(color) = color(i, w.z) else {
            continue;
        };
        let pos = [w.x as f32, w.y as f32, w.z as f32];
        let normal = cloud.normals.get(i).map_or([0; 4], |n| {
            let v = axes[0] * f64::from(n[0]) + axes[1] * f64::from(n[1]) + axes[2] * f64::from(n[2]);
            let v = v / v.length().max(1e-12);
            [v.x, v.y, v.z, 0.0].map(|c| (c * 127.0).round() as i8)
        });
        instances.push(PointInstance {
            pos,
            pos_low: [
                (w.x - pos[0] as f64) as f32,
                (w.y - pos[1] as f64) as f32,
                (w.z - pos[2] as f64) as f32,
            ],
            color,
            normal,
        });
    }
    // Spread the points so any leading part of the list is an even sample:
    // a real-time density draws only that part.
    let instances = spread(instances);
    crate::perf::record(format_args!(
        "[perf] point-cloud-place {:>7.1}ms shown={}",
        crate::perf::elapsed_ms(started),
        instances.len(),
    ));
    let placed = Arc::new(PlacedCloud { key, instances });
    let mut cache = placed_cache.lock().ok()?;
    cache.retain(|_, weak| weak.strong_count() > 0);
    cache.insert(key, Arc::downgrade(&placed));
    Some(placed)
}

/// The items in a stride order coprime to their count: every prefix samples
/// the whole list evenly.
fn spread<T: Copy>(items: Vec<T>) -> Vec<T> {
    let n = items.len();
    if n < 3 {
        return items;
    }
    let gcd = |mut a: usize, mut b: usize| {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    };
    // Near the golden section, so consecutive picks land far apart.
    let mut stride = (n as f64 * 0.618_033_988_75) as usize | 1;
    while gcd(stride, n) != 1 {
        stride += 2;
    }
    (0..n).map(|k| items[k * stride % n]).collect()
}

/// Whether a crop keeps point `p` (file coordinates). The crop is a prism
/// through the plane at `plane` spanned by its two directions; its polygon
/// is in that plane's coordinates. `inside` keeps the prism's interior and
/// `inverted` flips it.
fn keeps(crop: &PointCloudExCrop, p: &[f64; 3]) -> bool {
    // Points and crop outline on the crop plane, along its normal.
    let on_plane = |v: Vector3| {
        let d = v - crop.plane;
        Vector3::new(d.dot(&crop.x_direction), d.dot(&crop.y_direction), 0.0)
    };
    let Vector3 { x, y, .. } = on_plane(Vector3::new(p[0], p[1], p[2]));
    let points: Vec<Vector3> = crop.points.iter().copied().map(on_plane).collect();
    // A circle: its centre and a point on it.
    if crop.crop_type == 3 {
        let (c, e) = (points[0], points[1]);
        let r2 = (e.x - c.x).powi(2) + (e.y - c.y).powi(2);
        let inside = (x - c.x).powi(2) + (y - c.y).powi(2) <= r2;
        return (inside == crop.inside) != crop.inverted;
    }
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[j]);
        if (a.y > y) != (b.y > y) && x < (b.x - a.x) * (y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    (inside == crop.inside) != crop.inverted
}

/// How a cloud's points are coloured (its stylization).
#[derive(Hash)]
enum Stylization {
    /// The scan's own colours (intensity as grey when it has none).
    Scan,
    /// The cloud's object colour.
    Object([u8; 3]),
    /// Each point's world normal as a colour, (n + 1) / 2 per component.
    Normal,
    /// A colour scheme over intensity (per cent, in the range given) or
    /// elevation, blended or in bands; values out of range per
    /// `out_of_range` (0 end colours, 1 scan colours, 2 hidden).
    Ramp { colors: Vec<[u8; 3]>, gradient: bool, intensity: bool, range: (i64, i64), out_of_range: i16 },
}

impl Stylization {
    fn of(document: &CadDocument, data: &PointCloudExData, object_color: [u8; 3]) -> Self {
        let (scheme, intensity, gradient) = match data.stylization_type {
            2 => return Self::Object(object_color),
            3 => return Self::Normal,
            5 => (&data.intensity_color_scheme, true, data.intensity_as_gradient),
            4 => (&data.current_color_scheme, false, data.elevation_as_gradient),
            // Classification has no data here: scan colours.
            _ => return Self::Scan,
        };
        let colors = ramp_colors(document, scheme, if intensity { "Spectrum" } else { "Earth" });
        let range = if intensity && data.intensity_max > data.intensity_min {
            (i64::from(data.intensity_min), i64::from(data.intensity_max))
        } else {
            (0, 100)
        };
        let out_of_range =
            if intensity { data.intensity_out_of_range_behavior } else { data.elevation_out_of_range_behavior };
        Self::Ramp { colors, gradient, intensity, range: (range.0, range.1), out_of_range }
    }
}

/// The colour schemes a new drawing's colour map carries, high end first.
const BUILTIN_RAMPS: [(&str, &[u32]); 7] = [
    ("Hydro", &[0x00bfff, 0x007ca5, 0x005f7f, 0x00394c, 0x00264c, 0x00134c, 0x00004c, 0x000026]),
    ("Grayscale", &[0xffffff, 0xcccccc, 0x999999, 0x666666, 0x333333, 0x000000]),
    ("Earth", &[0xffffc8, 0xf0d796, 0xe1af64, 0xd28732, 0xc35f00]),
    ("Blues", &[0x0000ff, 0x0000a5, 0x00007f, 0x00004c, 0x000026]),
    ("Greens", &[0x00ff00, 0x00a500, 0x007f00, 0x004c00, 0x002600]),
    ("Spectrum", &[0xff00ff, 0x0000ff, 0x00ffff, 0x00ff00, 0xffff00, 0xff0000]),
    ("Reds", &[0xff0000, 0xa50000, 0x7f0000, 0x4c0000, 0x260000]),
];

fn rgb(color: u32) -> [u8; 3] {
    [(color >> 16) as u8, (color >> 8) as u8, color as u8]
}

/// The colour schemes the drawing offers (identifier, name, colours high end
/// first): its colour map's, or the built-in ones.
pub(crate) fn schemes(document: &CadDocument) -> Vec<(String, String, Vec<[u8; 3]>)> {
    for object in document.objects.values() {
        let ObjectType::ClassObject(object) = object else {
            continue;
        };
        let ClassObjectData::PointCloudColorMap(map) = &object.data else {
            continue;
        };
        if !map.color_ramps.is_empty() {
            return map
                .color_ramps
                .iter()
                .map(|r| {
                    let colors = r.colors.iter().filter(|c| c.visible).map(|c| rgb(c.color as u32)).collect();
                    (r.id.clone(), r.name.clone(), colors)
                })
                .collect();
        }
    }
    BUILTIN_RAMPS
        .iter()
        .map(|(name, colors)| {
            let id = crate::entities::extended::point_cloud_ramp_id(name).to_string();
            (id, name.to_string(), colors.iter().map(|c| rgb(*c)).collect())
        })
        .collect()
}

/// The schemes new intensity and elevation colouring starts from: the
/// colour map's defaults, else Spectrum and Earth.
pub(crate) fn default_schemes(document: &CadDocument) -> (String, String) {
    let map = document.objects.values().find_map(|object| match object {
        ObjectType::ClassObject(object) => match &object.data {
            ClassObjectData::PointCloudColorMap(map) => Some(map),
            _ => None,
        },
        _ => None,
    });
    let pick = |stored: Option<&String>, name: &str| {
        stored
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| crate::entities::extended::point_cloud_ramp_id(name).to_string())
    };
    (
        pick(map.map(|m| &m.default_intensity_scheme), "Spectrum"),
        pick(map.map(|m| &m.default_elevation_scheme), "Earth"),
    )
}

/// A scheme's colours: from the drawing's colour map when it has the scheme,
/// else the built-in one of that name (or `fallback` for an unset scheme).
fn ramp_colors(document: &CadDocument, id: &str, fallback: &str) -> Vec<[u8; 3]> {
    for object in document.objects.values() {
        let ObjectType::ClassObject(object) = object else {
            continue;
        };
        let ClassObjectData::PointCloudColorMap(map) = &object.data else {
            continue;
        };
        if let Some(found) = map.color_ramps.iter().find(|r| !id.is_empty() && r.id.eq_ignore_ascii_case(id)) {
            let colors: Vec<[u8; 3]> =
                found.colors.iter().filter(|c| c.visible).map(|c| rgb(c.color as u32)).collect();
            if !colors.is_empty() {
                return colors;
            }
        }
    }
    let name = crate::entities::extended::POINT_CLOUD_RAMPS
        .iter()
        .find(|(_, guid)| guid.eq_ignore_ascii_case(id))
        .map_or(fallback, |(name, _)| name);
    BUILTIN_RAMPS
        .iter()
        .find(|(ramp, _)| *ramp == name)
        .map_or_else(|| vec![[255; 3]], |(_, colors)| colors.iter().map(|c| rgb(*c)).collect())
}

/// The colour at `t` (0–1, clamped) along a scheme: blended between its
/// colours, or the band `t` falls in. A scheme lists its high end first
/// (Spectrum: magenta at the highest value, red at the lowest).
fn ramp(colors: &[[u8; 3]], t: f64, gradient: bool) -> [u8; 3] {
    let t = 1.0 - if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    if colors.len() < 2 {
        return colors.first().copied().unwrap_or([255; 3]);
    }
    if !gradient {
        return colors[((t * colors.len() as f64) as usize).min(colors.len() - 1)];
    }
    let x = t * (colors.len() - 1) as f64;
    let k = (x as usize).min(colors.len() - 2);
    let f = x - k as f64;
    let (a, b) = (colors[k], colors[k + 1]);
    [0, 1, 2].map(|c| (f64::from(a[c]) + (f64::from(b[c]) - f64::from(a[c])) * f).round() as u8)
}

/// A point cloud system variable: its default and the integers it takes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Setting {
    pub name: &'static str,
    pub default: i64,
    min: i64,
    max: i64,
}

/// The reference's defaults and ranges.
const SETTINGS: [Setting; 15] = [
    Setting { name: "POINTCLOUDDENSITY", default: 15, min: 1, max: 100 },
    Setting { name: "POINTCLOUDRTDENSITY", default: 5, min: 1, max: 100 },
    Setting { name: "POINTCLOUDPOINTSIZE", default: 2, min: 1, max: 10 },
    Setting { name: "POINTCLOUDLOD", default: 10, min: 1, max: 10 },
    Setting { name: "POINTCLOUDLOCK", default: 0, min: 0, max: 1 },
    Setting { name: "POINTCLOUDAUTOUPDATE", default: 0, min: 0, max: 1 },
    Setting { name: "POINTCLOUDVISRETAIN", default: 1, min: 0, max: 1 },
    Setting { name: "POINTCLOUDSHADING", default: 0, min: 0, max: 1 },
    Setting { name: "POINTCLOUD2DVSDISPLAY", default: 0, min: 0, max: 1 },
    Setting { name: "POINTCLOUDLIGHTSOURCE", default: 0, min: 0, max: 1 },
    Setting { name: "POINTCLOUDLIGHTING", default: 0, min: 0, max: 2 },
    Setting { name: "POINTCLOUDBOUNDARY", default: 1, min: 0, max: 2 },
    Setting { name: "POINTCLOUDCACHESIZE", default: 512, min: 0, max: 32000 },
    Setting { name: "POINTCLOUDPOINTMAX", default: 10_000_000, min: 1_000_000, max: 25_000_000 },
    Setting { name: "POINTCLOUDPOINTMAXLEGACY", default: 1_500_000, min: 1_500_000, max: 10_000_000 },
];

/// How a value was refused: the lines to show, and whether the prompt asks
/// again (a small range) or the command ends (the point and cache limits).
pub(crate) struct Refusal {
    pub lines: Vec<String>,
    pub ask_again: bool,
}

impl Setting {
    pub(crate) fn check(&self, text: &str) -> Result<i64, Refusal> {
        let Ok(value) = text.trim().parse::<i64>() else {
            return Err(Refusal { lines: vec!["Requires an integer value.".to_string()], ask_again: true });
        };
        if (self.min..=self.max).contains(&value) {
            return Ok(value);
        }
        Err(if self.max > 100 {
            Refusal {
                lines: vec![format!("Cannot set {} to that value.", self.name), "*Invalid*".to_string()],
                ask_again: false,
            }
        } else if self.min == 0 && self.max == 1 {
            Refusal { lines: vec!["Requires 0 or 1 only.".to_string()], ask_again: true }
        } else {
            Refusal {
                lines: vec![format!("Requires an integer between {} and {}.", self.min, self.max)],
                ask_again: true,
            }
        })
    }
}

pub(crate) fn setting(name: &str) -> Option<Setting> {
    SETTINGS.iter().copied().find(|setting| setting.name.eq_ignore_ascii_case(name))
}

/// A setting's value in the drawing, or its default.
pub(crate) fn setting_value(document: &CadDocument, setting: Setting) -> i64 {
    crate::io::drawing_variable(document, setting.name)
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(setting.default)
}
