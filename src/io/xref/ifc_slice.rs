use super::*;
use crate::ifc_plan::{self, PlanModel};
use document::{
    objects::{AssociativeData, ObjectType},
    tables::{AppId, Layer},
    types::LineWeight,
    xdata::{ExtendedDataRecord, XDataValue},
};
use sha2::{Digest, Sha256};

const VIEW_APP: &str = "OPENAEC_IFC_XREF";
const LINE_KEY_PREFIX: &str = "xref-key=";

#[derive(Clone, Debug)]
pub struct Ready {
    pub(crate) path: PathBuf,
    pub(crate) view: View,
    pub(crate) read: XrefRead,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct View {
    project: String,
    storey: String,
    cut: f64,
    depth: f64,
}

impl View {
    pub(crate) fn new(
        model: &PlanModel,
        storey_index: usize,
        cut: f64,
        depth: f64,
    ) -> Result<Self, String> {
        let project = model
            .project_global_id
            .clone()
            .ok_or("IFC project GlobalId is missing.")?;
        let storey = if model.storeys.is_empty() {
            String::new()
        } else {
            model
                .storeys
                .get(storey_index)
                .and_then(|s| s.global_id.clone())
                .ok_or("IFC storey GlobalId is missing.")?
        };
        Ok(Self {
            project,
            storey,
            cut,
            depth,
        })
    }

    pub(crate) fn set_on_insert(&self, insert: &mut document::entities::Insert) {
        let mut record = ExtendedDataRecord::new(VIEW_APP);
        for value in [
            "v1".into(),
            self.project.clone(),
            self.storey.clone(),
            self.cut.to_string(),
            self.depth.to_string(),
        ] {
            record.add_value(XDataValue::String(value));
        }
        insert.common.extended_data.upsert_record(record);
    }

    fn from_insert(insert: &document::entities::Insert) -> Option<Self> {
        let record = insert.common.extended_data.get_record(VIEW_APP)?;
        let [XDataValue::String(version), XDataValue::String(project), XDataValue::String(storey), XDataValue::String(cut), XDataValue::String(depth)] =
            record.values.as_slice()
        else {
            return None;
        };
        if version != "v1" {
            return None;
        }
        Some(Self {
            project: project.clone(),
            storey: storey.clone(),
            cut: cut.parse().ok()?,
            depth: depth.parse().ok()?,
        })
    }

    fn elevation(&self, model: &PlanModel) -> Result<f64, String> {
        if model.project_global_id.as_deref() != Some(self.project.as_str()) {
            return Err(
                "The IFC file is from a different project. Attach it as a new reference.".into(),
            );
        }
        if self.storey.is_empty() && model.storeys.is_empty() {
            return Ok(self.cut);
        }
        let mut levels = model
            .storeys
            .iter()
            .filter(|s| s.global_id.as_deref() == Some(self.storey.as_str()));
        match (levels.next(), levels.next()) {
            (Some(level), None) => Ok(level.elevation + self.cut),
            _ => Err("The IFC storey is missing or its GlobalId is ambiguous.".into()),
        }
    }
}

pub(crate) fn is_ifc(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ifc"))
}

pub(crate) fn ensure_app(doc: &mut Document) {
    register_app(doc, VIEW_APP);
}

fn register_app(doc: &mut Document, name: &str) {
    if doc.app_ids.get(name).is_none() {
        let mut app = AppId::new(name);
        app.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app);
    }
}

pub(super) fn views(doc: &Document) -> HashMap<Handle, View> {
    doc.entities()
        .filter_map(|entity| {
            let EntityType::Insert(insert) = entity else {
                return None;
            };
            let block = doc.block_records.get(&insert.block_name)?;
            ((block.flags.is_xref || block.flags.is_xref_overlay)
                && is_ifc(Path::new(&block.xref_path)))
            .then_some(())?;
            Some((block.handle, View::from_insert(insert)?))
        })
        .collect()
}

type SourceModel = OnceLock<Result<PlanModel, String>>;

/// Share a source snapshot across slices in one open or reload operation.
#[derive(Default)]
pub(super) struct ReadBatch {
    models: Mutex<HashMap<PathBuf, Arc<SourceModel>>>,
}

impl ReadBatch {
    pub(super) fn read(&self, raw: &str, base: &Path, view: &View) -> XrefRead {
        let resolved = resolve_path(raw, base);
        let outcome = resolved.as_ref().map(|path| {
            let key = path.canonicalize().unwrap_or_else(|_| path.clone());
            let slot = self.models.lock().unwrap().entry(key).or_default().clone();
            let model = slot.get_or_init(|| {
                let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
                ifc_plan::load_model(&bytes)
            });
            generate(model.as_ref().map_err(String::clone)?, path, view)
        });
        read_result(resolved, outcome)
    }
}

pub(crate) fn from_model(model: &PlanModel, path: &Path, view: &View) -> Result<XrefRead, String> {
    Ok(read_result(
        Some(path.to_path_buf()),
        Some(Ok(generate(model, path, view)?)),
    ))
}

fn read_result(
    resolved: Option<PathBuf>,
    outcome: Option<Result<document::ReadOutcome, String>>,
) -> XrefRead {
    XrefRead {
        resolved,
        outcome,
        source_sha256: None,
        content: None,
        context: None,
        resource_id: None,
        nested_rows: Vec::new(),
        graph_diagnostics: Vec::new(),
        guard: None,
        circular_root: false,
    }
}

type Endpoints = [[i64; 3]; 2];

fn line_points(line: &document::entities::Line, origin: Vector3) -> Endpoints {
    let local = |point: Vector3| {
        let p = point - origin;
        [p.x, p.y, p.z].map(|value| (value * 10.0).round() as i64)
    };
    let mut ends = [local(line.start), local(line.end)];
    ends.sort_unstable();
    ends
}

fn min_point(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
}

fn hash_points(hash: &mut Sha256, points: Endpoints) {
    for coordinate in points.into_iter().flatten() {
        hash.update(coordinate.to_le_bytes());
    }
}

fn handle(hash: Sha256, kind: u64) -> Handle {
    let digest = hash.finalize();
    let n = u64::from_le_bytes(digest[..8].try_into().unwrap());
    Handle::new((kind << 52) | (n & ((1 << 52) - 1)))
}

fn generate(model: &PlanModel, path: &Path, view: &View) -> Result<document::ReadOutcome, String> {
    let basename = path.file_name().unwrap_or_default().to_string_lossy();
    let entities = ifc_plan::entities(model, &basename, view.elevation(model)?, view.depth)?;
    let mut ids_seen = HashMap::default();
    let mut ambiguous = HashSet::default();
    let mut lines = Vec::new();
    for entity in entities {
        let EntityType::Line(line) = entity else {
            return Err("Unexpected entity in IFC slice".into());
        };
        let source = ifc_plan::source::read(&line.common).ok_or("IFC source is missing.")?;
        let mut ids = Vec::new();
        for element in source.elements {
            if ids_seen
                .insert(element.global_id.to_owned(), element.step_id)
                .is_some_and(|old| old != element.step_id)
            {
                ambiguous.insert(element.global_id.to_owned());
            }
            ids.push(element.global_id.to_owned());
        }
        ids.sort_unstable();
        ids.dedup();
        lines.push((ids, line));
    }
    let eligible = |ids: &[String]| {
        !ids.is_empty()
            && ids
                .iter()
                .all(|id| !id.is_empty() && !ambiguous.contains(id))
    };
    let mut origins = HashMap::<Vec<String>, Vector3>::default();
    for (ids, line) in lines.iter().filter(|(ids, _)| eligible(ids)) {
        let low = min_point(line.start, line.end);
        origins
            .entry(ids.clone())
            .and_modify(|p| *p = min_point(*p, low))
            .or_insert(low);
    }
    let mut groups = HashMap::<Vec<String>, Vec<(String, Endpoints)>>::default();
    for (ids, line) in lines.iter().filter(|(ids, _)| eligible(ids)) {
        groups
            .entry(ids.clone())
            .or_default()
            .push((line.common.layer.clone(), line_points(line, origins[ids])));
    }
    let group_keys: HashMap<_, _> = groups
        .into_iter()
        .map(|(ids, mut shape)| {
            shape.sort_unstable();
            let mut hash = Sha256::new();
            hash.update(b"ocs-ifc-slice-v1\0");
            hash.update(view.project.as_bytes());
            hash.update(view.storey.as_bytes());
            hash.update(view.cut.to_le_bytes());
            hash.update(view.depth.to_le_bytes());
            for id in &ids {
                hash.update(id.as_bytes());
            }
            for (layer, ends) in shape {
                hash.update(layer.as_bytes());
                hash.update([0]);
                hash_points(&mut hash, ends);
            }
            (ids, hash)
        })
        .collect();
    let mut counts = HashMap::default();
    let lines: Vec<_> = lines
        .into_iter()
        .map(|(ids, line)| {
            let key = group_keys.get(&ids).map(|prefix| {
                let mut hash = prefix.clone();
                hash.update(line.common.layer.as_bytes());
                hash.update([0]);
                hash_points(&mut hash, line_points(&line, origins[&ids]));
                let key = handle(hash, 1);
                *counts.entry(key).or_insert(0usize) += 1;
                key
            });
            (key, line)
        })
        .collect();

    let mut doc = Document::new();
    doc.header.insertion_units = 4;
    register_app(&mut doc, ifc_plan::source::APPLICATION);
    for (name, weight) in ifc_plan::LAYERS {
        let mut layer = Layer::new(name);
        layer.handle = doc.allocate_handle();
        layer.line_weight = LineWeight::from_value(weight);
        doc.layers.add(layer).map_err(|e| e.to_string())?;
    }
    let mut assigned = HashSet::default();
    for (index, (key, mut line)) in lines.into_iter().enumerate() {
        let key = key.filter(|key| counts[key] == 1).unwrap_or_else(|| {
            let mut hash = Sha256::new();
            hash.update(model.revision.as_bytes());
            hash.update((index as u64).to_le_bytes());
            handle(hash, 2)
        });
        if !assigned.insert(key) {
            return Err("IFC slice line keys collide.".into());
        }
        let mut tag = line
            .common
            .extended_data
            .get_record(ifc_plan::source::APPLICATION)
            .expect("IFC source was checked")
            .clone();
        tag.add_value(XDataValue::String(format!(
            "{LINE_KEY_PREFIX}{:X}",
            key.value()
        )));
        line.common.extended_data.upsert_record(tag);
        line.common.handle = key;
        doc.add_entity(EntityType::Line(line))
            .map_err(|e| e.to_string())?;
    }
    let stats = document::ReadStats {
        stream_completed: true,
        output_entities: doc.entities().count(),
        ..Default::default()
    };
    Ok(document::ReadOutcome::new(doc, stats))
}

pub(super) fn source_keys(doc: &Document, block: Handle) -> Option<HashMap<Handle, Handle>> {
    doc.block_records
        .iter()
        .find(|br| br.handle == block && is_ifc(Path::new(&br.xref_path)))?;
    Some(
        doc.entities()
            .filter(|e| e.common().owner_handle == block)
            .filter_map(|e| {
                let common = e.common();
                let data = common
                    .extended_data
                    .get_record(ifc_plan::source::APPLICATION)?;
                let key = data.values.iter().find_map(|value| match value {
                    XDataValue::String(value) => value.strip_prefix(LINE_KEY_PREFIX),
                    _ => None,
                })?;
                Some((
                    common.handle,
                    Handle::new(u64::from_str_radix(key, 16).ok()?),
                ))
            })
            .collect(),
    )
}

pub(crate) fn unresolved(
    doc: &Document,
    dimension: Option<Handle>,
    block: Option<Handle>,
) -> usize {
    let ifc_inserts: HashSet<_> = doc
        .entities()
        .filter_map(|entity| {
            let EntityType::Insert(insert) = entity else {
                return None;
            };
            let br = doc.block_records.get(&insert.block_name)?;
            (block.is_none_or(|handle| handle == br.handle)
                && is_ifc(Path::new(&br.xref_path))
                && View::from_insert(insert).is_some())
            .then_some(insert.common.handle)
        })
        .collect();
    doc.objects
        .values()
        .filter_map(|object| {
            let ObjectType::Associative(object) = object else {
                return None;
            };
            let AssociativeData::DimensionAssociation(assoc) = &object.data else {
                return None;
            };
            dimension
                .is_none_or(|id| id == assoc.dimension)
                .then_some(assoc)
        })
        .flat_map(|assoc| assoc.references.iter().flatten())
        .filter(|reference| {
            (!reference.xref_paths.is_empty()
                && reference
                    .xrefs
                    .last()
                    .is_some_and(|id| ifc_inserts.contains(id)))
                || (!reference.intersection_xref_paths.is_empty()
                    && reference
                        .intersection_objects
                        .last()
                        .is_some_and(|id| ifc_inserts.contains(id)))
        })
        .count()
}

pub(super) fn warn_unresolved(
    doc: &Document,
    updated: &[(String, Handle)],
    infos: &mut [XrefInfo],
) {
    for (name, block) in updated {
        let count = unresolved(doc, None, Some(*block));
        if let Some(info) = infos
            .iter_mut()
            .find(|info| info.name == *name && matches!(info.status, XrefStatus::Loaded))
        {
            if count != 0 {
                info.diagnostics.push(format!("{count} IFC XREF dimension reference(s) unresolved. Displayed values may be stale. Use DIMREASSOCIATE to repair."));
            }
        }
    }
}
