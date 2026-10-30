//! Semantic block instance, attribute, definition-tag, and dynamic property extraction.

use super::failure;
use crate::app::control::text_ops::decode_dxf_escapes;
use crate::app::OpenCADStudio;
use crate::scene::block_authoring::{definition_dictionary, DictionaryOwners};
use crate::scene::model::object::PropValue;
use codec::objects::{DynamicBlockData, ObjectType};
use codec::xdata::XDataValue;
use codec::{Document, EntityType, Handle};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Map, Value};

const ELEMENT_TAG: &str = "AcDbBlockRepETag";
const REPRESENTATION_TAG: &str = "AcDbBlockRepBTag";
const TRUE_NAME: &str = "AcDbDynamicBlockTrueName";

fn entity_element_id(entity: &EntityType) -> Option<i32> {
    entity
        .common()
        .extended_data
        .get_record(ELEMENT_TAG)?
        .values
        .iter()
        .find_map(|value| match value {
            XDataValue::Integer32(id) => Some(*id),
            _ => None,
        })
}

fn resolve_visibility_state(
    doc: &Document,
    block_members: &FxHashMap<Handle, Vec<Handle>>,
    insert_handle: Handle,
    effective_def: Option<Handle>,
    evaluated_def: Option<Handle>,
) -> Option<(String, String)> {
    if let Some(prop) = OpenCADStudio::visibility_property(doc, insert_handle) {
        if let PropValue::Choice { selected, .. } | PropValue::ChoiceWithTooltip { selected, .. } = prop.value {
            if !selected.is_empty() {
                return Some((prop.label, selected));
            }
        }
    }

    let (def_h, param) = doc
        .dynamic_visibility_for_insert(insert_handle)
        .or_else(|| {
            let d = effective_def?;
            Some((d, crate::scene::block_visibility::parameter(doc, d)?))
        })?;

    let label = if param.name.trim().is_empty() {
        "Visibility1".to_string()
    } else {
        param.name.clone()
    };

    let rep_h = evaluated_def.unwrap_or(def_h);
    if let Some(rep_handles) = block_members.get(&rep_h) {
        // 1. Match by AcDbBlockRepETag element ID
        let by_id: FxHashMap<i32, bool> = rep_handles
            .iter()
            .filter_map(|&h| {
                let e = doc.get_entity(h)?;
                Some((entity_element_id(e)?, !e.common().invisible))
            })
            .collect();
        if !by_id.is_empty() && !param.all_blocks.is_empty() {
            if let Some(state) = param.states.iter().find(|state| {
                param.all_blocks.iter().all(|h| {
                    doc.get_entity(*h)
                        .and_then(entity_element_id)
                        .and_then(|id| by_id.get(&id))
                        .is_some_and(|&shown| shown == state.visible_blocks.contains(h))
                })
            }) {
                return Some((label, state.name.clone()));
            }
        }

        // 2. Match by parallel member index
        if let Some(def_handles) = block_members.get(&def_h) {
            if !param.all_blocks.is_empty() && def_handles.len() == rep_handles.len() {
                if let Some(state) = param.states.iter().find(|state| {
                    def_handles.iter().enumerate().all(|(idx, def_m)| {
                        if !param.all_blocks.contains(def_m) {
                            return true;
                        }
                        let shown = doc
                            .get_entity(rep_handles[idx])
                            .is_some_and(|e| !e.common().invisible);
                        shown == state.visible_blocks.contains(def_m)
                    })
                }) {
                    return Some((label, state.name.clone()));
                }
            }
        }
    }

    param
        .states
        .first()
        .map(|first| (label, first.name.clone()))
}

/// Precomputed block definition membership map (`BlockRecord` handle -> member entity handles).
pub(crate) fn build_block_members_map(doc: &Document) -> FxHashMap<Handle, Vec<Handle>> {
    let mut layouts: FxHashSet<Handle> = doc
        .objects
        .values()
        .filter_map(|object| match object {
            ObjectType::Layout(layout) if !layout.block_record.is_null() => {
                Some(layout.block_record)
            }
            _ => None,
        })
        .collect();
    layouts.extend(doc.block_records.iter().filter_map(|record| {
        let name = record.name.to_ascii_uppercase();
        (name.starts_with("*MODEL_SPACE") || name.starts_with("*PAPER_SPACE"))
            .then_some(record.handle)
    }));

    let mut members: FxHashMap<Handle, Vec<Handle>> = FxHashMap::default();
    let mut seen: FxHashMap<Handle, FxHashSet<Handle>> = FxHashMap::default();

    for record in doc.block_records.iter() {
        if layouts.contains(&record.handle) {
            continue;
        }
        let entry = members.entry(record.handle).or_default();
        let set = seen.entry(record.handle).or_default();
        for &h in &record.entity_handles {
            if !matches!(
                doc.get_entity(h),
                None | Some(EntityType::Block(_) | EntityType::BlockEnd(_))
            ) && set.insert(h)
            {
                entry.push(h);
            }
        }
    }

    for entity in doc.entities() {
        if matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_)) {
            continue;
        }
        let owner = entity.common().owner_handle;
        let handle = entity.common().handle;
        if let Some(entry) = members.get_mut(&owner) {
            if seen.entry(owner).or_default().insert(handle) {
                entry.push(handle);
            }
        }
    }

    members
}

/// Resolve an `Insert` entity's effective block name, effective definition `BlockRecord` handle,
/// and evaluated representation `BlockRecord` handle (e.g. `*U12`).
pub(crate) fn resolve_effective_block(
    doc: &Document,
    insert: &codec::entities::Insert,
) -> (String, Option<Handle>, Option<Handle>) {
    let insert_handle = insert.common.handle;
    let raw_name = &insert.block_name;
    let evaluated_record = crate::scene::render_graph::block_record_by_name(doc, raw_name);
    let evaluated_handle = evaluated_record.map(|r| r.handle);

    // 1. Standard dynamic block resolution via Document
    let mut def_handle = doc.dynamic_definition_for_insert(insert_handle);

    // 2. Fallback: check AcDbBlockRepBTag on the Insert's XData
    if def_handle.is_none() {
        def_handle = insert
            .common
            .extended_data
            .get_record(REPRESENTATION_TAG)
            .and_then(|rec| {
                rec.values.iter().find_map(|v| match v {
                    XDataValue::Handle(h) if !h.is_null() => Some(*h),
                    _ => None,
                })
            });
    }

    // 3. Fallback: check AcDbBlockRepBTag on the evaluated BlockRecord's XData
    if def_handle.is_none() {
        if let Some(eval_rec) = evaluated_record {
            def_handle = doc
                .object_xdata(eval_rec.handle)
                .and_then(|xdata| xdata.get_record(REPRESENTATION_TAG))
                .and_then(|rec| {
                    rec.values.iter().find_map(|v| match v {
                        XDataValue::Handle(h) if !h.is_null() => Some(*h),
                        _ => None,
                    })
                });
        }
    }

    // 4. Fallback: check evaluated BlockRecord extension dictionary for representation data
    if def_handle.is_none() {
        if let Some(eval_h) = evaluated_handle {
            if let Some(dict_h) = definition_dictionary(doc, eval_h) {
                if let Some(ObjectType::Dictionary(dict)) = doc.objects.get(&dict_h) {
                    for (_, entry_h) in &dict.entries {
                        match doc.objects.get(entry_h) {
                            Some(ObjectType::DynamicBlock(dyn_obj)) => {
                                if let DynamicBlockData::Representation(rep) = &dyn_obj.data {
                                    if !rep.block.is_null() {
                                        def_handle = Some(rep.block);
                                        break;
                                    }
                                }
                            }
                            Some(ObjectType::Dictionary(sub_dict)) => {
                                for (_, sub_h) in &sub_dict.entries {
                                    if let Some(ObjectType::DynamicBlock(dyn_obj)) =
                                        doc.objects.get(sub_h)
                                    {
                                        if let DynamicBlockData::Representation(rep) = &dyn_obj.data
                                        {
                                            if !rep.block.is_null() {
                                                def_handle = Some(rep.block);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    if let Some(def_h) = def_handle {
        if let Some(def_rec) = doc.block_records.iter().find(|r| r.handle == def_h) {
            let true_name = doc
                .object_xdata(def_rec.handle)
                .and_then(|xdata| xdata.get_record(TRUE_NAME))
                .and_then(|rec| {
                    rec.values.iter().find_map(|v| match v {
                        XDataValue::String(s) if !s.trim().is_empty() => Some(s.clone()),
                        _ => None,
                    })
                })
                .unwrap_or_else(|| def_rec.name.clone());
            return (true_name, Some(def_h), evaluated_handle);
        }
    }

    // 5. Fallback: check AcDbDynamicBlockTrueName XData on Insert or evaluated BlockRecord
    let xdata_true_name = insert
        .common
        .extended_data
        .get_record(TRUE_NAME)
        .or_else(|| {
            evaluated_record
                .and_then(|r| doc.object_xdata(r.handle))
                .and_then(|xdata| xdata.get_record(TRUE_NAME))
        })
        .and_then(|rec| {
            rec.values.iter().find_map(|v| match v {
                XDataValue::String(s) if !s.trim().is_empty() => Some(s.clone()),
                _ => None,
            })
        });

    if let Some(true_name) = xdata_true_name {
        let resolved_def = crate::scene::render_graph::block_record_by_name(doc, &true_name)
            .map(|r| r.handle)
            .or(evaluated_handle);
        return (true_name, resolved_def, evaluated_handle);
    }

    (raw_name.clone(), evaluated_handle, evaluated_handle)
}

/// Case-insensitive wildcard glob matcher supporting `*` and `?`.
pub(crate) fn glob_match_ci(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let pat: Vec<char> = pattern.chars().flat_map(|c| c.to_lowercase()).collect();
    let txt: Vec<char> = text.chars().flat_map(|c| c.to_lowercase()).collect();

    let (mut p_idx, mut t_idx) = (0, 0);
    let mut star_idx: Option<usize> = None;
    let mut match_idx = 0;

    while t_idx < txt.len() {
        if p_idx < pat.len() && (pat[p_idx] == '?' || pat[p_idx] == txt[t_idx]) {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < pat.len() && pat[p_idx] == '*' {
            star_idx = Some(p_idx);
            match_idx = t_idx;
            p_idx += 1;
        } else if let Some(s_idx) = star_idx {
            p_idx = s_idx + 1;
            match_idx += 1;
            t_idx = match_idx;
        } else {
            return false;
        }
    }

    while p_idx < pat.len() && pat[p_idx] == '*' {
        p_idx += 1;
    }

    p_idx == pat.len()
}

fn clean_text(raw: &str, is_mtext: bool) -> String {
    let base = if is_mtext {
        codec::entities::mtext_format::parse_mtext(raw, true).to_plain_text()
    } else {
        raw.replace("\\P", "\n").replace("\\p", "\n")
    };
    decode_dxf_escapes(&base)
}

/// Recognizes dimension strings commonly found in AEC block sub-entities
/// (e.g. `160x250`, `160 x 250`, `20X40`, `160×250`, `20*40`, `90/210`, `Ø30`, `⌀160`).
fn is_dimension_text(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Diameter prefix: Ø40, ⌀160
    if let Some(rest) = trimmed
        .strip_prefix('Ø')
        .or_else(|| trimmed.strip_prefix('⌀'))
        .or_else(|| trimmed.strip_prefix("ø"))
    {
        let rest = rest
            .trim()
            .trim_end_matches("mm")
            .trim_end_matches("cm")
            .trim_end_matches('m')
            .trim();
        if !rest.is_empty()
            && rest.chars().any(|c| c.is_ascii_digit())
            && rest
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | ' ' | 'x' | 'X' | '×' | '/'))
        {
            return true;
        }
    }

    // Normalize unit suffixes (mm, cm, m)
    let core = trimmed
        .trim_end_matches("mm")
        .trim_end_matches("MM")
        .trim_end_matches("cm")
        .trim_end_matches("CM")
        .trim_end_matches('m')
        .trim_end_matches('M')
        .trim();

    // Check for <num> <sep> <num> [<sep> <num>] where <sep> is x, X, ×, *, /
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut sep_found = false;

    for ch in core.chars() {
        if matches!(ch, 'x' | 'X' | '×' | '*' | '/') {
            sep_found = true;
            parts.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if ! sep_found {
        return false;
    }
    parts.push(current.trim().to_string());
    if parts.len() < 2 || parts.len() > 3 {
        return false;
    }

    parts.iter().all(|part| {
        !part.is_empty()
            && part.chars().any(|c| c.is_ascii_digit())
            && part
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '+' | ' '))
    })
}

fn is_valid_key_value_pair(line: &str) -> Option<(String, String)> {
    let sep_idx = line.find(':').or_else(|| line.find('='))?;
    let key = line[..sep_idx].trim();
    let val = line[sep_idx + 1..].trim();
    if key.is_empty() || key.len() > 24 || val.is_empty() {
        return None;
    }
    let first = key.chars().next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
    {
        return None;
    }
    Some((key.to_uppercase(), val.to_string()))
}

fn collect_definition_texts_and_nested_attrs(
    doc: &Document,
    block_members: &FxHashMap<Handle, Vec<Handle>>,
    def_handle: Handle,
    depth: u8,
    attrs: &mut Map<String, Value>,
    texts: &mut Vec<String>,
) {
    if depth > 3 {
        return;
    }
    let Some(handles) = block_members.get(&def_handle) else {
        return;
    };
    for &h in handles {
        let Some(entity) = doc.get_entity(h) else {
            continue;
        };
        let common = entity.common();
        if common.invisible || crate::scene::block_construction::is_construction(common) {
            continue;
        }
        match entity {
            EntityType::Text(t) => {
                let cleaned = clean_text(&t.value, false);
                for line in cleaned.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() && !texts.iter().any(|existing| existing == trimmed) {
                        texts.push(trimmed.to_string());
                    }
                }
            }
            EntityType::MText(t) => {
                let cleaned = clean_text(&t.value, true);
                for line in cleaned.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() && !texts.iter().any(|existing| existing == trimmed) {
                        texts.push(trimmed.to_string());
                    }
                }
            }
            EntityType::Insert(nested) => {
                for attr in &nested.attributes {
                    let tag = attr.tag.trim();
                    if !tag.is_empty() && !attrs.contains_key(tag) {
                        let val = clean_text(attr.get_value(), false);
                        attrs.insert(tag.to_string(), Value::String(val));
                    }
                }
                let (_, eff_h, eval_h) = resolve_effective_block(doc, nested);
                if let Some(next_h) = eval_h.or(eff_h) {
                    if next_h != def_handle {
                        collect_definition_texts_and_nested_attrs(
                            doc,
                            block_members,
                            next_h,
                            depth + 1,
                            attrs,
                            texts,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// Extract attributes (`ATTRIB` on insert, `ATTDEF` defaults, and static `TEXT`/`MTEXT` schedule tags)
/// for a block reference.
pub(crate) fn extract_block_attributes(
    doc: &Document,
    block_members: &FxHashMap<Handle, Vec<Handle>>,
    insert: &codec::entities::Insert,
    effective_def: Option<Handle>,
    evaluated_def: Option<Handle>,
) -> Map<String, Value> {
    let mut attrs = Map::new();

    // 1. Seed from ATTDEF defaults in the block definition(s)
    for def_h in [effective_def, evaluated_def].into_iter().flatten() {
        if let Some(handles) = block_members.get(&def_h) {
            for &h in handles {
                if let Some(EntityType::AttributeDefinition(ad)) = doc.get_entity(h) {
                    if crate::scene::block_construction::is_construction(&ad.common) {
                        continue;
                    }
                    let tag = ad.tag.trim();
                    if !tag.is_empty() && !attrs.contains_key(tag) {
                        let val = clean_text(&ad.default_value, false);
                        attrs.insert(tag.to_string(), Value::String(val));
                    }
                }
            }
        }
    }

    // 2. Override with explicit ATTRIB entities on the Insert
    for attr in &insert.attributes {
        let tag = attr.tag.trim();
        if !tag.is_empty() {
            let val = clean_text(attr.get_value(), false);
            attrs.insert(tag.to_string(), Value::String(val));
        }
    }

    // 3. Parse visible static TEXT/MTEXT sub-entities and nested Inserts in the block definition
    let mut static_texts = Vec::new();
    if let Some(active_def) = evaluated_def.or(effective_def) {
        collect_definition_texts_and_nested_attrs(
            doc,
            block_members,
            active_def,
            0,
            &mut attrs,
            &mut static_texts,
        );
    }

    // Remove static texts that already appear as attribute values
    static_texts.retain(|t| {
        !attrs
            .values()
            .any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(t)))
    });

    // Skip static text synthesis when a block is a grouped full-floor drawing (>12 static text labels)
    if static_texts.len() > 12 {
        return attrs;
    }

    let mut unlabelled = Vec::new();
    for line in static_texts {
        if let Some((k, v)) = is_valid_key_value_pair(&line) {
            if !attrs.contains_key(&k)
                || attrs
                    .get(&k)
                    .and_then(Value::as_str)
                    .is_some_and(str::is_empty)
            {
                attrs.insert(k, Value::String(v));
                continue;
            }
        }
        unlabelled.push(line);
    }

    // Classify dimension strings first so order of TEXT handles inside the block never matters
    let mut non_dims = Vec::new();
    for line in unlabelled {
        if is_dimension_text(&line) {
            let empty_dims = attrs
                .get("DIMS")
                .and_then(Value::as_str)
                .is_some_and(str::is_empty);
            if !attrs.contains_key("DIMS") || empty_dims {
                attrs.insert("DIMS".to_string(), Value::String(line));
            } else {
                let mut idx = 2usize;
                loop {
                    let key = format!("DIMS_{idx}");
                    if !attrs.contains_key(&key) {
                        attrs.insert(key, Value::String(line));
                        break;
                    }
                    idx += 1;
                }
            }
        } else {
            non_dims.push(line);
        }
    }

    // Assign non-dimension static texts: first free goes to TAG, rest to TEXT_2, TEXT_3, ...
    for line in non_dims {
        let empty_tag = attrs
            .get("TAG")
            .and_then(Value::as_str)
            .is_some_and(str::is_empty);
        if !attrs.contains_key("TAG") || empty_tag {
            attrs.insert("TAG".to_string(), Value::String(line));
        } else {
            let mut idx = 2usize;
            loop {
                let key = format!("TEXT_{idx}");
                if !attrs.contains_key(&key) {
                    attrs.insert(key, Value::String(line));
                    break;
                }
                idx += 1;
            }
        }
    }

    attrs
}

fn clean_float(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    let rounded = (v * 1e6).round() / 1e6;
    if rounded == 0.0 {
        0.0
    } else {
        rounded
    }
}

/// Compute world XY bounds for an `Insert` by transforming its visible block definition members.
fn insert_world_xy_bounds(
    doc: &Document,
    block_members: &FxHashMap<Handle, Vec<Handle>>,
    insert: &codec::entities::Insert,
    active_def: Option<Handle>,
) -> ([f64; 2], [f64; 2]) {
    let mut min_x = insert.insert_point.x;
    let mut min_y = insert.insert_point.y;
    let mut max_x = insert.insert_point.x;
    let mut max_y = insert.insert_point.y;

    let Some(def_h) = active_def else {
        return ([min_x, min_y], [max_x, max_y]);
    };
    let Some(handles) = block_members.get(&def_h) else {
        return ([min_x, min_y], [max_x, max_y]);
    };
    if handles.is_empty() {
        return ([min_x, min_y], [max_x, max_y]);
    }

    let transform = crate::scene::render_graph::insert_transform(doc, insert);
    let mut any_member = false;
    let mut b_min_x = f64::INFINITY;
    let mut b_min_y = f64::INFINITY;
    let mut b_max_x = f64::NEG_INFINITY;
    let mut b_max_y = f64::NEG_INFINITY;

    for &h in handles {
        let Some(entity) = doc.get_entity(h) else {
            continue;
        };
        if entity.common().invisible
            || crate::scene::block_construction::is_construction(entity.common())
        {
            continue;
        }
        let (emin, emax) = crate::scene::convert::tess::entity_bounds_in(doc, entity);
        if !emin[0].is_finite()
            || !emin[1].is_finite()
            || !emax[0].is_finite()
            || !emax[1].is_finite()
        {
            continue;
        }
        if emin[0] == 0.0 && emin[1] == 0.0 && emax[0] == 0.0 && emax[1] == 0.0 {
            continue;
        }
        for &(px, py) in &[
            (emin[0], emin[1]),
            (emin[0], emax[1]),
            (emax[0], emin[1]),
            (emax[0], emax[1]),
        ] {
            let wp = transform.apply(codec::types::Vector3::new(px, py, 0.0));
            if wp.x.is_finite() && wp.y.is_finite() {
                any_member = true;
                b_min_x = b_min_x.min(wp.x);
                b_min_y = b_min_y.min(wp.y);
                b_max_x = b_max_x.max(wp.x);
                b_max_y = b_max_y.max(wp.y);
            }
        }
    }

    if any_member {
        min_x = min_x.min(b_min_x);
        min_y = min_y.min(b_min_y);
        max_x = max_x.max(b_max_x);
        max_y = max_y.max(b_max_y);
    }

    ([min_x, min_y], [max_x, max_y])
}

impl OpenCADStudio {
    /// Extract dynamic block properties (parameters, visibility state, lookup choice) for an `Insert`.
    pub(crate) fn extract_dynamic_properties(
        &self,
        doc: &Document,
        block_members: &FxHashMap<Handle, Vec<Handle>>,
        tab_id: u64,
        insert_handle: Handle,
        effective_def: Option<Handle>,
        evaluated_def: Option<Handle>,
    ) -> Map<String, Value> {
        let mut dyn_props = Map::new();
        if effective_def.is_none() && doc.dynamic_definition_for_insert(insert_handle).is_none() {
            return dyn_props;
        }

        // 1. Action parameters (Linear, Polar, XY, Point, Rotation, Flip)
        for prop in
            crate::scene::block_parameter_properties::properties(doc, None, tab_id, insert_handle)
        {
            match prop.value {
                PropValue::BlockNumber { value, .. } => {
                    if let Some(num) = crate::app::block_parameter_properties::parse_value(&value)
                    {
                        dyn_props.insert(prop.label, json!(clean_float(num)));
                    } else {
                        dyn_props.insert(prop.label, json!(value));
                    }
                }
                PropValue::BlockChoice {
                    selected, options, ..
                } => {
                    if let Some(opt) = options.get(selected) {
                        dyn_props.insert(prop.label, json!(opt));
                    }
                }
                PropValue::Choice { selected, .. } | PropValue::ChoiceWithTooltip { selected, .. } => {
                    dyn_props.insert(prop.label, json!(selected));
                }
                PropValue::ReadOnly(val) | PropValue::PlainText(val) | PropValue::EditText(val) => {
                    dyn_props.insert(prop.label, json!(val));
                }
                _ => {}
            }
        }

        // 2. Visibility state parameter
        if let Some((label, selected)) = resolve_visibility_state(
            doc,
            block_members,
            insert_handle,
            effective_def,
            evaluated_def,
        ) {
            dyn_props.insert(label, json!(selected));
        }

        // 3. Lookup table property
        if let Some(prop) = crate::scene::block_lookup::reference_property(doc, insert_handle) {
            if let PropValue::Choice { selected, .. } = prop.value {
                if !selected.is_empty() {
                    dyn_props.insert(prop.label, json!(selected));
                }
            }
        }

        dyn_props
    }

    /// `blocks` — High-level semantic block instance, attribute, schedule tag, and dynamic property extractor.
    pub(crate) fn control_blocks(&self, req: &Value) -> Result<Value, Value> {
        let i = if let Some(doc_id) = req.get("document_id").and_then(Value::as_u64) {
            self.tabs
                .iter()
                .position(|t| t.id == doc_id)
                .ok_or_else(|| failure("document_not_found", "Specified document_id not found"))?
        } else {
            self.active_tab
        };

        let tab = &self.tabs[i];
        let scene = &tab.scene;
        let doc = &scene.document;

        // Publish dictionary owners once to avoid O(N*M) scans on large drawings.
        let _dict_owners = DictionaryOwners::publish(doc);

        // Parse name / names glob patterns (defaults to ["*"])
        let mut patterns: Vec<String> = Vec::new();
        if let Some(arr) = req.get("names").and_then(Value::as_array) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        patterns.push(trimmed.to_string());
                    }
                }
            }
        }
        if let Some(single) = req.get("name").and_then(Value::as_str) {
            let trimmed = single.trim();
            if !trimmed.is_empty() {
                patterns.push(trimmed.to_string());
            }
        }
        if patterns.is_empty() {
            patterns.push("*".to_string());
        }

        let extract_attributes = req
            .get("extract_attributes")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let extract_dynamic_properties = req
            .get("extract_dynamic_properties")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let include_anonymous = req
            .get("include_anonymous")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        // Check if any pattern explicitly requests anonymous blocks (e.g. "*U*", "*D*")
        let explicit_anonymous_pattern = patterns
            .iter()
            .any(|p| p.starts_with('*') && p.len() > 1 && p.chars().nth(1) != Some('*'));

        let scope = req
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("active_space");
        let layer_filter = req.get("layer").and_then(Value::as_str);

        let handles_filter: Option<FxHashSet<Handle>> = if let Some(h) = req["handle"].as_str() {
            let parsed = u64::from_str_radix(h.trim_start_matches("0x").trim_start_matches("0X"), 16)
                .map(Handle::new)
                .map_err(|_| failure("invalid_handle", "handle must be hexadecimal"))?;
            let mut set = FxHashSet::default();
            set.insert(parsed);
            Some(set)
        } else if let Some(arr) = req["handles"].as_array() {
            let mut set = FxHashSet::default();
            for item in arr {
                let s = item
                    .as_str()
                    .ok_or_else(|| failure("invalid_handles", "handles must be hex strings"))?;
                let parsed =
                    u64::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16)
                        .map(Handle::new)
                        .map_err(|_| {
                            failure(
                                "invalid_handles",
                                "handles contains an invalid hexadecimal handle",
                            )
                        })?;
                set.insert(parsed);
            }
            Some(set)
        } else {
            None
        };

        let bounds_filter: Option<[f64; 4]> = if let Some(b_val) = req.get("bounds") {
            let b = b_val
                .as_array()
                .and_then(|v| {
                    if v.len() != 4 {
                        return None;
                    }
                    Some([
                        v[0].as_f64()?,
                        v[1].as_f64()?,
                        v[2].as_f64()?,
                        v[3].as_f64()?,
                    ])
                })
                .ok_or_else(|| {
                    failure("invalid_bounds", "bounds expects [min_x, min_y, max_x, max_y]")
                })?;
            if !b.iter().all(|x| x.is_finite()) || b[0] > b[2] || b[1] > b[3] {
                return Err(failure(
                    "invalid_bounds",
                    "bounds must be finite with min <= max",
                ));
            }
            Some(b)
        } else {
            None
        };

        let where_filters = req.get("where").and_then(Value::as_array);
        let offset = req.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let limit = req
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(1000)
            .clamp(1, 10_000) as usize;

        let block_members = build_block_members_map(doc);
        let mut matched = Vec::new();

        for entity in doc.entities() {
            let EntityType::Insert(ins) = entity else {
                continue;
            };
            let handle = ins.common.handle;

            // Scope filter
            let in_active = scene.entity_belongs_to_active_space(handle);
            match scope {
                "active_space" | "model_space" if !in_active => continue,
                "blocks" if in_active => continue,
                _ => {}
            }

            // Layer filter
            if let Some(layer) = layer_filter {
                if !ins.common.layer.eq_ignore_ascii_case(layer) {
                    continue;
                }
            }

            // Handle filter
            if let Some(ref set) = handles_filter {
                if !set.contains(&handle) {
                    continue;
                }
            }

            // Resolve effective block name & definitions
            let (effective_name, effective_def, evaluated_def) = resolve_effective_block(doc, ins);

            // Filter out internal anonymous blocks (*U..., *D..., *T..., *X..., *A..., *E...)
            // unless resolved to a named dynamic block or explicitly requested.
            if effective_name.starts_with('*') && !include_anonymous && !explicit_anonymous_pattern
            {
                continue;
            }

            // Match against `names` glob patterns
            if !patterns
                .iter()
                .any(|pat| glob_match_ci(pat, &effective_name) || (include_anonymous && glob_match_ci(pat, &ins.block_name)))
            {
                continue;
            }

            // Spatial bounds check
            let active_def = evaluated_def.or(effective_def);
            if let Some(bounds) = bounds_filter {
                let (min_xy, max_xy) = insert_world_xy_bounds(doc, &block_members, ins, active_def);
                if max_xy[0] < bounds[0]
                    || max_xy[1] < bounds[1]
                    || min_xy[0] > bounds[2]
                    || min_xy[1] > bounds[3]
                {
                    continue;
                }
            }

            let mut record = Map::new();
            record.insert("name".into(), Value::String(effective_name.clone()));
            if ins.block_name != effective_name {
                record.insert("anonymous_name".into(), Value::String(ins.block_name.clone()));
            }
            record.insert("handle".into(), Value::String(format!("{:X}", handle.value())));
            record.insert("layer".into(), Value::String(ins.common.layer.clone()));
            record.insert(
                "position".into(),
                json!([
                    clean_float(ins.insert_point.x),
                    clean_float(ins.insert_point.y),
                    clean_float(ins.insert_point.z)
                ]),
            );
            record.insert(
                "rotation".into(),
                json!(clean_float(ins.rotation.to_degrees())),
            );
            record.insert(
                "scale".into(),
                json!([
                    clean_float(ins.x_scale()),
                    clean_float(ins.y_scale()),
                    clean_float(ins.z_scale())
                ]),
            );

            if extract_attributes {
                let attributes = extract_block_attributes(
                    doc,
                    &block_members,
                    ins,
                    effective_def,
                    evaluated_def,
                );
                record.insert("attributes".into(), Value::Object(attributes));
            }

            if extract_dynamic_properties {
                let dyn_props = self.extract_dynamic_properties(
                    doc,
                    &block_members,
                    tab.id,
                    handle,
                    effective_def,
                    evaluated_def,
                );
                record.insert("dynamic_properties".into(), Value::Object(dyn_props));
            }

            let record_val = Value::Object(record);

            // Optional `where` filter on the constructed record (e.g. `/attributes/TAG`)
            if let Some(filters) = where_filters {
                let mut ok = true;
                for filter in filters {
                    let Some(path) = filter["path"].as_str() else {
                        return Err(failure("invalid_where", "where filter requires path"));
                    };
                    let pointer = if path.is_empty() || path.starts_with('/') {
                        path.to_string()
                    } else {
                        format!("/{path}")
                    };
                    let actual = record_val.pointer(&pointer);
                    match crate::app::record_api::compare(
                        actual,
                        filter["op"].as_str().unwrap_or("eq"),
                        filter.get("value"),
                    ) {
                        Ok(true) => {}
                        Ok(false) => {
                            ok = false;
                            break;
                        }
                        Err(e) => return Err(failure("invalid_where", format!("where filter: {e}"))),
                    }
                }
                if !ok {
                    continue;
                }
            }

            matched.push(record_val);
        }

        let count = matched.len();
        let blocks: Vec<Value> = matched.into_iter().skip(offset).take(limit).collect();
        let returned = blocks.len();

        Ok(json!({
            "ok": true,
            "document_id": tab.id,
            "geometry_revision": scene.geometry_epoch,
            "count": count,
            "returned": returned,
            "next_offset": (offset + returned < count).then_some(offset + returned),
            "blocks": blocks,
        }))
    }

    pub(crate) fn automation_blocks(&self, req: &Value) -> Value {
        self.control_blocks(req).unwrap_or_else(|e| e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{AttributeDefinition, AttributeEntity, Circle, Insert, Line, Text};
    use codec::types::Vector3;

    #[test]
    fn extracts_schedule_tags_attributes_dynamic_properties_and_filters_anonymous_blocks() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        app.tabs[i].is_start = false;

        // 1. Block with static TEXT sub-entities (e.g. schedule tag PF.E8 and dims 160x250)
        let mut text_dims = Text::new();
        text_dims.value = "160x250".to_string();
        text_dims.insertion_point = Vector3::new(0.0, -5.0, 0.0);
        text_dims.height = 2.5;
        let mut text_tag = Text::new();
        text_tag.value = "PF.E8".to_string();
        text_tag.insertion_point = Vector3::new(0.0, 5.0, 0.0);
        text_tag.height = 3.0;
        app.tabs[i].scene.define_block_raw(
            "PF_FOOTING",
            Vector3::ZERO,
            vec![EntityType::Text(text_dims), EntityType::Text(text_tag)],
        );
        let pf_insert = Insert::new("PF_FOOTING", Vector3::new(120.0, 340.0, 0.0));
        let pf_handle = app.tabs[i].scene.add_entity(EntityType::Insert(pf_insert));

        // 2. Block with ATTDEF defaults + ATTRIB override on Insert
        let attdef_tag = AttributeDefinition::new(
            "TAG".to_string(),
            "Enter tag".to_string(),
            "DEFAULT_TAG".to_string(),
        );
        let attdef_dims = AttributeDefinition::new(
            "DIMS".to_string(),
            "Enter dims".to_string(),
            "200x400".to_string(),
        );
        app.tabs[i].scene.define_block_raw(
            "COL_SCHED",
            Vector3::ZERO,
            vec![
                EntityType::AttributeDefinition(attdef_tag),
                EntityType::AttributeDefinition(attdef_dims),
            ],
        );
        let mut col_insert = Insert::new("COL_SCHED", Vector3::new(50.0, 60.0, 0.0));
        col_insert.attributes.push(AttributeEntity::new(
            "TAG".to_string(),
            "P.12".to_string(),
        ));
        let col_handle = app.tabs[i].scene.add_entity(EntityType::Insert(col_insert));

        // 3. Unlinked internal anonymous block (*U99) — should be filtered out by default
        app.tabs[i].scene.define_block_raw(
            "*U99",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_coords(
                0.0, 0.0, 0.0, 10.0, 10.0, 0.0,
            ))],
        );
        if let Some(rec) = app.tabs[i].scene.document.block_records.get_mut("*U99") {
            rec.flags.anonymous = true;
        }
        app.tabs[i]
            .scene
            .add_entity(EntityType::Insert(Insert::new("*U99", Vector3::ZERO)));

        // 4. Dynamic block with visibility states evaluated to an anonymous *U copy
        app.tabs[i].scene.define_block_raw(
            "DOOR_DYN",
            Vector3::ZERO,
            vec![
                EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 90.0, 0.0, 0.0)),
                EntityType::Circle(Circle::from_center_radius(
                    Vector3::new(45.0, 45.0, 0.0),
                    20.0,
                )),
            ],
        );
        let door_def = app.tabs[i]
            .scene
            .document
            .block_records
            .get("DOOR_DYN")
            .unwrap()
            .handle;
        let vis_param = crate::scene::block_visibility::create(
            &mut app.tabs[i].scene.document,
            door_def,
            Vector3::ZERO,
        )
        .unwrap();
        let open_idx = crate::scene::block_visibility::add_state(
            &mut app.tabs[i].scene.document,
            vis_param,
            "Open_90",
            0,
        )
        .unwrap();
        let door_members = app.tabs[i]
            .scene
            .document
            .block_records
            .get("DOOR_DYN")
            .unwrap()
            .entity_handles
            .clone();
        crate::scene::block_visibility::set_visible(
            &mut app.tabs[i].scene.document,
            vis_param,
            open_idx,
            &[door_members[1]],
            false,
        );
        crate::scene::block_visibility::finish_editing(&mut app.tabs[i].scene.document, door_def);

        let door_insert_h = app.tabs[i].scene.add_entity(EntityType::Insert(Insert::new(
            "DOOR_DYN",
            Vector3::new(10.0, 20.0, 0.0),
        )));
        let anon_copy = app.tabs[i]
            .scene
            .visibility_representation(door_insert_h)
            .expect("dynamic *U copy");
        assert!(anon_copy.starts_with("*U"));

        // Query all blocks via `op: "blocks"`
        let (res, _) = app.control_request(json!({
            "op": "blocks",
            "names": ["*"],
            "extract_attributes": true,
            "extract_dynamic_properties": true
        }));
        assert_eq!(res["ok"], true, "{res}");
        // *U99 is excluded; PF_FOOTING, COL_SCHED, and DOOR_DYN (resolved from *U) are returned!
        assert_eq!(res["count"], 3, "{res}");

        let blocks = res["blocks"].as_array().unwrap();

        let pf = blocks
            .iter()
            .find(|b| b["name"] == "PF_FOOTING")
            .expect("PF_FOOTING present");
        assert_eq!(pf["handle"], format!("{:X}", pf_handle.value()));
        assert_eq!(pf["position"], json!([120.0, 340.0, 0.0]));
        assert_eq!(pf["attributes"]["TAG"], "PF.E8");
        assert_eq!(pf["attributes"]["DIMS"], "160x250");

        let col = blocks
            .iter()
            .find(|b| b["name"] == "COL_SCHED")
            .expect("COL_SCHED present");
        assert_eq!(col["handle"], format!("{:X}", col_handle.value()));
        assert_eq!(col["attributes"]["TAG"], "P.12");
        assert_eq!(col["attributes"]["DIMS"], "200x400");

        let door = blocks
            .iter()
            .find(|b| b["name"] == "DOOR_DYN")
            .expect("DOOR_DYN resolved from *U");
        assert_eq!(door["handle"], format!("{:X}", door_insert_h.value()));
        assert_eq!(door["anonymous_name"], anon_copy);
        assert_eq!(door["dynamic_properties"]["Visibility1"], "VisibilityState0");

        // Test `where` filter on extracted attributes
        let (filtered, _) = app.control_request(json!({
            "op": "blocks",
            "where": [{"path": "/attributes/TAG", "op": "eq", "value": "PF.E8"}]
        }));
        assert_eq!(filtered["count"], 1, "{filtered}");
        assert_eq!(filtered["blocks"][0]["name"], "PF_FOOTING");

        assert!(is_dimension_text("160x250"));
        assert!(is_dimension_text("110+155x220"));
        assert!(is_dimension_text("320 + 120 x220"));
    }
}
