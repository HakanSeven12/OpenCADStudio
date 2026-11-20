/// Table records the clipboard entities depend on, snapshotted from the source
/// drawing at copy time. On paste into a drawing that lacks any of them, the
/// missing records are recreated (with a fresh handle) so the entities keep
/// their layer / linetype / style instead of dangling. See #129.
#[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ClipboardDeps {
    pub layers: Vec<document::tables::Layer>,
    pub linetypes: Vec<document::tables::LineType>,
    pub text_styles: Vec<document::tables::TextStyle>,
    pub dim_styles: Vec<document::tables::DimStyle>,
    pub mleader_styles: Vec<document::objects::MultiLeaderStyle>,
    /// The text and shape-file styles the captured linetypes' shape and text
    /// segments point to. Segments name their style by handle, and shape-file
    /// styles have no name, so a paste matches them by font file instead and
    /// re-points the recreated linetype at the target's record.
    pub linetype_styles: Vec<document::tables::TextStyle>,
    /// The image definitions the copied raster images point to. A paste
    /// re-points each image at the target's definition of the same file, or
    /// recreates the definition there, so it never names the source's object.
    pub image_definitions: Vec<document::objects::ImageDefinition>,
    /// Block definitions the copied INSERTs reference (transitively),
    /// snapshotted from the source drawing. Recreated on paste so a block
    /// reference doesn't render empty in a drawing that lacks the
    /// definition. (#135)
    pub blocks: Vec<BlockDef>,
    /// Baked `*D` blocks referenced by copied dimensions, snapshotted from the
    /// source drawing. On paste each pasted dimension gets its own transformed
    /// copy of its block (its geometry is baked in WCS, so without this the
    /// paste renders at the source location — or, cross-drawing, not at all).
    /// See #290 (mirrors the in-drawing copy's #161 fix).
    pub dim_blocks: Vec<BlockDef>,
    /// Extension-dictionary object subtrees hanging off the copied entities
    /// (XCLIP spatial filters, attached XRecords, …). Each entity's whole
    /// `xdictionary` graph is snapshotted so a cross-drawing paste recreates it
    /// — without this a pasted clipped block loses its clip and renders whole.
    pub ext_objects: Vec<ClipExtObjects>,
    /// Groups whose *entire* membership is in the copied selection, snapshotted
    /// from the source drawing. On paste each is recreated over the pasted
    /// handles so a copied group stays grouped — cross-drawing too, the same way
    /// the in-drawing COPY path preserves it. Partly-selected groups are not
    /// captured (copying one member should not spawn a fragment). See #440.
    pub groups: Vec<document::objects::Group>,
    /// The APPID records the copied XDATA names. EED refers to its
    /// application by APPID handle, so a paste into another drawing
    /// re-points it at that drawing's own record of the same name.
    pub app_ids: Vec<document::tables::AppId>,
    /// Names of the source records the captured dimension styles point to by
    /// handle (text style, linetypes, arrow blocks), keyed by source handle.
    /// A paste re-points each recreated style at this drawing's records of
    /// the same name, so it never names an object of the source drawing.
    #[serde(with = "dim_style_refs_serde", default)]
    pub dim_style_refs: rustc_hash::FxHashMap<document::Handle, String>,
    /// The FIELD objects the copied tables' cells use, nested fields
    /// included, by source handle. A paste gives each pasted table its own
    /// clones of them, in this drawing or another.
    pub table_fields: Vec<document::objects::Field>,
}

mod dim_style_refs_serde {
    use document::Handle;
    use rustc_hash::FxHashMap;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(map: &FxHashMap<Handle, String>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut pairs: Vec<(Handle, String)> = map.iter().map(|(k, v)| (*k, v.clone())).collect();
        pairs.sort_by_key(|(h, _)| h.value());
        pairs.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<FxHashMap<Handle, String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let pairs = Vec::<(Handle, String)>::deserialize(deserializer)?;
        Ok(pairs.into_iter().collect())
    }
}

/// A captured block definition: its base point and the entities it owns
/// (in block-local coordinates), minus the structural Block/BlockEnd
/// markers which are rebuilt on paste.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockDef {
    pub name: String,
    pub base_point: document::types::Vector3,
    pub entities: Vec<document::EntityType>,
    /// The source drawing's record, which other captured graphs name.
    pub source: document::Handle,
    /// The record's object graph: a dynamic block's parameters and states.
    #[serde(skip)]
    pub graph: Option<document::BlockGraph>,
}

impl PartialEq for BlockDef {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.base_point == other.base_point
            && self.entities == other.entities
            && self.source == other.source
            && self.graph.is_some() == other.graph.is_some()
    }
}

/// The extension-dictionary object graph captured for one copied entity.
/// `objects` holds every object reachable from the entity's `xdictionary`
/// (dictionaries + their leaf objects), keyed by their source handles; `root`
/// is the xdictionary handle. On paste the whole set is cloned into the target
/// document with fresh handles and the references are remapped.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClipExtObjects {
    pub entity_index: usize,
    #[serde(default)]
    pub attribute_index: Option<usize>,
    pub src_entity_handle: document::Handle,
    pub root: document::Handle,
    pub objects: Vec<(document::Handle, document::objects::ObjectType)>,
    pub annotation_scales: Vec<(document::Handle, document::objects::Scale)>,
}

impl ClipboardDeps {
    /// Snapshot the records `entities` reference that exist in `doc`.
    pub fn capture(doc: &document::Document, entities: &[document::EntityType]) -> Self {
        use document::EntityType;
        use std::collections::BTreeSet;
        let (mut layers, mut ltypes, mut tstyles, mut dstyles) = (
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        );
        let is_special = |n: &str| {
            n.is_empty() || n.eq_ignore_ascii_case("ByLayer") || n.eq_ignore_ascii_case("ByBlock")
        };
        // Scan the copied entities AND the entities inside every captured
        // block definition, so a block-internal object's layer / linetype /
        // style is recreated too — not just the top-level selection's.
        let blocks = Self::capture_blocks(doc, entities);
        let block_entities = blocks.iter().flat_map(|d| d.entities.iter());
        for e in entities.iter().chain(block_entities) {
            let c = e.common();
            if !c.layer.is_empty() {
                layers.insert(c.layer.clone());
            }
            if !is_special(&c.linetype) && !c.linetype.eq_ignore_ascii_case("Continuous") {
                ltypes.insert(c.linetype.clone());
            }
            match e {
                EntityType::Text(t) if !t.style.is_empty() => {
                    tstyles.insert(t.style.clone());
                }
                EntityType::MText(t) if !t.style.is_empty() => {
                    tstyles.insert(t.style.clone());
                }
                EntityType::AttributeEntity(a) if !a.text_style.is_empty() => {
                    tstyles.insert(a.text_style.clone());
                }
                EntityType::AttributeDefinition(a) if !a.text_style.is_empty() => {
                    tstyles.insert(a.text_style.clone());
                }
                EntityType::Dimension(d) if !d.base().style_name.is_empty() => {
                    dstyles.insert(d.base().style_name.clone());
                }
                EntityType::Leader(l) if !l.dimension_style.is_empty() => {
                    dstyles.insert(l.dimension_style.clone());
                }
                _ => {}
            }
        }
        // A dimension style names its text style, linetypes and arrow blocks
        // by handle: bring the records along (the arrow blocks come with the
        // captured blocks) and remember their names for the paste.
        let mut dim_style_refs = rustc_hash::FxHashMap::default();
        let mut mleader_styles = Vec::new();
        for entity in entities
            .iter()
            .chain(blocks.iter().flat_map(|d| d.entities.iter()))
        {
            let EntityType::MultiLeader(m) = entity else {
                continue;
            };
            let mut m = m.clone();
            if let Some(document::objects::ObjectType::MultiLeaderStyle(style)) =
                m.style_handle.and_then(|h| doc.objects.get(&h))
            {
                if !mleader_styles
                    .iter()
                    .any(|s: &document::objects::MultiLeaderStyle| s.handle == style.handle)
                {
                    mleader_styles.push(style.clone());
                }
            }
            for h in crate::app::command_driver::clipboard::mleader_handles(&mut m)
                .into_iter()
                .filter_map(|h| *h)
                .chain(mleader_styles.iter().flat_map(|s| {
                    [
                        s.text_style_handle,
                        s.line_type_handle,
                        s.arrowhead_handle,
                        s.block_content_handle,
                    ]
                    .into_iter()
                    .flatten()
                }))
            {
                // Blocks come through `capture_blocks`; the paste maps all by name.
                if let Some(t) = doc.text_styles.iter().find(|t| t.handle == h) {
                    tstyles.insert(t.name.clone());
                } else if let Some(t) = doc.line_types.iter().find(|t| t.handle == h) {
                    ltypes.insert(t.name.clone());
                }
            }
        }
        for style in dstyles.iter().filter_map(|n| doc.dim_styles.get(n)) {
            let text = style.dimtxsty_handle;
            if let Some(t) = doc
                .text_styles
                .iter()
                .find(|t| !text.is_null() && t.handle == text)
            {
                tstyles.insert(t.name.clone());
                dim_style_refs.insert(text, t.name.clone());
            }
            for h in [
                style.dimltex_handle,
                style.dimltex1_handle,
                style.dimltex2_handle,
            ] {
                let Some(lt) = doc
                    .line_types
                    .iter()
                    .find(|lt| !h.is_null() && lt.handle == h)
                else {
                    continue;
                };
                if !is_special(&lt.name) && !lt.name.eq_ignore_ascii_case("Continuous") {
                    ltypes.insert(lt.name.clone());
                }
                dim_style_refs.insert(h, lt.name.clone());
            }
            for h in crate::app::command_driver::clipboard::dim_style_blocks(style) {
                if let Some(r) = doc
                    .block_records
                    .iter()
                    .find(|r| !h.is_null() && r.handle == h)
                {
                    dim_style_refs.insert(h, r.name.clone());
                }
            }
        }
        // Extension-dictionary subtree per entity (XCLIP filters, annotation
        // contexts, …) and per inline INSERT attribute.
        let mut ext_objects = Vec::new();
        let mut push_ext = |entity_index: usize,
                            attribute_index: Option<usize>,
                            src_entity_handle: document::Handle,
                            root: document::Handle| {
            if root.is_null() {
                return;
            }
            let objects = Self::collect_ext_subtree(doc, root);
            if objects.is_empty() {
                return;
            }
            let mut annotation_scales = Vec::new();
            for (_, object) in &objects {
                let document::objects::ObjectType::ObjectContextData(context) = object else {
                    continue;
                };
                if annotation_scales
                    .iter()
                    .any(|(handle, _)| *handle == context.scale)
                {
                    continue;
                }
                if let Some(document::objects::ObjectType::Scale(scale)) =
                    doc.objects.get(&context.scale)
                {
                    annotation_scales.push((context.scale, scale.clone()));
                }
            }
            ext_objects.push(ClipExtObjects {
                entity_index,
                attribute_index,
                src_entity_handle,
                root,
                objects,
                annotation_scales,
            });
        };
        for (entity_index, e) in entities.iter().enumerate() {
            let c = e.common();
            if let Some(root) = c.xdictionary_handle {
                push_ext(entity_index, None, c.handle, root);
            }
            if let EntityType::Insert(ins) = e {
                for (attr_index, att) in ins.attributes.iter().enumerate() {
                    if let Some(root) = att.common.xdictionary_handle {
                        push_ext(entity_index, Some(attr_index), att.common.handle, root);
                    }
                }
            }
        }

        // Baked `*D` blocks for copied dimensions (top-level and inside captured
        // blocks), so a pasted dimension can be given its own transformed block
        // instead of aliasing the source's — whose geometry is baked in WCS at
        // the source location. (#290)
        let mut dim_block_names: BTreeSet<String> = BTreeSet::new();
        for e in entities
            .iter()
            .chain(blocks.iter().flat_map(|d| d.entities.iter()))
        {
            if let EntityType::Dimension(d) = e {
                let bn = d.base().block_name.clone();
                if !bn.trim().is_empty() {
                    dim_block_names.insert(bn);
                }
            }
        }
        let dim_blocks: Vec<BlockDef> = dim_block_names
            .iter()
            .filter_map(|n| Self::snapshot_block(doc, n))
            .collect();

        // Groups fully contained in the top-level selection. Groups reference
        // top-level entities (not block internals), so match against `entities`
        // only. Mirrors the in-drawing `copy_complete_groups` membership test.
        let copied_handles: rustc_hash::FxHashSet<document::Handle> =
            entities.iter().map(|e| e.common().handle).collect();
        let groups: Vec<document::objects::Group> = doc
            .objects
            .values()
            .filter_map(|obj| match obj {
                document::objects::ObjectType::Group(g)
                    if !g.entities.is_empty()
                        && g.entities.iter().all(|h| copied_handles.contains(h)) =>
                {
                    Some(g.clone())
                }
                _ => None,
            })
            .collect();

        // APPIDs named by the copied XDATA, verbatim EED (by handle) and
        // structured records (by name) alike, block members and attributes
        // included.
        let mut app_handles = rustc_hash::FxHashSet::default();
        let mut app_names = rustc_hash::FxHashSet::default();
        let mut note_apps = |xdata: &document::xdata::ExtendedData| {
            app_handles.extend(xdata.raw_dwg_eed.iter().map(|(app, _)| *app));
            app_names.extend(xdata.records().iter().map(|r| r.application_name.clone()));
        };
        for e in entities
            .iter()
            .chain(blocks.iter().flat_map(|d| d.entities.iter()))
        {
            note_apps(&e.common().extended_data);
            if let EntityType::Insert(insert) = e {
                for attribute in &insert.attributes {
                    note_apps(&attribute.common.extended_data);
                }
            }
        }
        let app_ids = doc
            .app_ids
            .iter()
            .filter(|a| app_handles.contains(&a.handle.value()) || app_names.contains(&a.name))
            .cloned()
            .collect();

        let linetypes: Vec<document::tables::LineType> = ltypes
            .iter()
            .filter_map(|n| doc.line_types.get(n).cloned())
            .collect();
        let mut linetype_styles: Vec<document::tables::TextStyle> = Vec::new();
        for complex in linetypes
            .iter()
            .flat_map(|lt| lt.elements.iter())
            .filter_map(|element| element.complex.as_ref())
        {
            if complex.style_handle.is_null()
                || linetype_styles
                    .iter()
                    .any(|s| s.handle == complex.style_handle)
            {
                continue;
            }
            if let Some(style) = doc
                .text_styles
                .iter()
                .find(|s| s.handle == complex.style_handle)
            {
                linetype_styles.push(style.clone());
            }
        }

        let mut image_definitions: Vec<document::objects::ImageDefinition> = Vec::new();
        for e in entities
            .iter()
            .chain(blocks.iter().flat_map(|d| d.entities.iter()))
        {
            let EntityType::RasterImage(image) = e else {
                continue;
            };
            let Some(handle) = image.definition_handle.filter(|h| !h.is_null()) else {
                continue;
            };
            if image_definitions.iter().any(|d| d.handle == handle) {
                continue;
            }
            if let Some(document::objects::ObjectType::ImageDefinition(definition)) =
                doc.objects.get(&handle)
            {
                let mut definition = definition.clone();
                definition.handle = handle;
                image_definitions.push(definition);
            }
        }

        ClipboardDeps {
            layers: layers
                .iter()
                .filter_map(|n| doc.layers.get(n).cloned())
                .collect(),
            linetypes,
            linetype_styles,
            image_definitions,
            text_styles: tstyles
                .iter()
                .filter_map(|n| doc.text_styles.get(n).cloned())
                .collect(),
            dim_styles: dstyles
                .iter()
                .filter_map(|n| doc.dim_styles.get(n).cloned())
                .collect(),
            blocks,
            dim_blocks,
            ext_objects,
            groups,
            app_ids,
            dim_style_refs,
            mleader_styles,
            table_fields: entities
                .iter()
                .filter_map(|e| match e {
                    EntityType::Table(table) => {
                        Some(crate::scene::table_fields::table_fields(doc, table))
                    }
                    _ => None,
                })
                .flatten()
                .collect(),
        }
    }

    /// Breadth-first collect of every object reachable from extension-dictionary
    /// `root` (dictionary entries, nested xdictionaries, dictionary defaults,
    /// objects an XRecord hard-owns),
    /// returned as `(source_handle, object)` pairs. Cycle-safe.
    fn collect_ext_subtree(
        doc: &document::Document,
        root: document::Handle,
    ) -> Vec<(document::Handle, document::objects::ObjectType)> {
        use document::objects::ObjectType;
        use rustc_hash::FxHashSet;
        let mut seen: FxHashSet<document::Handle> = FxHashSet::default();
        let mut queue = vec![root];
        let mut out = Vec::new();
        while let Some(h) = queue.pop() {
            if h.is_null() || !seen.insert(h) {
                continue;
            }
            let Some(obj) = doc.objects.get(&h) else {
                continue;
            };
            // Enqueue children referenced by this object.
            match obj {
                ObjectType::Dictionary(d) => {
                    queue.extend(d.entries.iter().map(|(_, ch)| *ch));
                    if let Some(x) = d.xdictionary_handle {
                        queue.push(x);
                    }
                }
                ObjectType::DictionaryWithDefault(d) => {
                    queue.extend(d.entries.iter().map(|(_, ch)| *ch));
                    queue.push(d.default_handle);
                }
                // Hard-owned objects (codes 360-369), e.g. the TABLECONTENT
                // of a table's ACAD_ROUNDTRIP_2008_TABLE_ENTITY record.
                ObjectType::XRecord(x) => {
                    queue.extend(
                        x.entries
                            .iter()
                            .filter(|entry| (360..=369).contains(&entry.code))
                            .filter_map(|entry| entry.value.as_handle()),
                    );
                }
                // A field's nested fields (a formula's operands) hang off it,
                // not off a dictionary entry.
                ObjectType::Field(f) => queue.extend(f.child_fields.iter().copied()),
                _ => {}
            }
            out.push((h, obj.clone()));
        }
        out
    }

    /// Snapshot every block definition the `entities` reference through an
    /// INSERT, walking nested INSERTs transitively. Model/paper space and
    /// xref blocks are skipped — those aren't portable definitions.
    fn capture_blocks(
        doc: &document::Document,
        entities: &[document::EntityType],
    ) -> Vec<BlockDef> {
        use document::EntityType;
        use rustc_hash::FxHashSet;
        let mut seen: FxHashSet<String> = FxHashSet::default();
        let mut queue: Vec<String> = Vec::new();
        for e in entities {
            if let EntityType::Insert(ins) = e {
                // A reference showing an evaluated copy needs its dynamic
                // definition too.
                let definition = doc
                    .dynamic_definition_for_insert(ins.common.handle)
                    .and_then(|h| doc.block_records.iter().find(|r| r.handle == h))
                    .map(|record| record.name.clone());
                for name in std::iter::once(ins.block_name.clone()).chain(definition) {
                    if seen.insert(name.clone()) {
                        queue.push(name);
                    }
                }
            }
            Self::queue_table_blocks(doc, e, &mut seen, &mut queue);
            Self::queue_dim_style_blocks(doc, e, &mut seen, &mut queue);
        }
        let mut defs = Vec::new();
        while let Some(name) = queue.pop() {
            let Some(def) = Self::snapshot_block(doc, &name) else {
                continue;
            };
            // Follow nested INSERTs and table block cells so their
            // definitions are captured too.
            for e in &def.entities {
                if let EntityType::Insert(ins) = e {
                    if seen.insert(ins.block_name.clone()) {
                        queue.push(ins.block_name.clone());
                    }
                }
                Self::queue_table_blocks(doc, e, &mut seen, &mut queue);
                Self::queue_dim_style_blocks(doc, e, &mut seen, &mut queue);
            }
            defs.push(def);
        }
        defs
    }

    /// Queue the blocks a table's block cells show; the cells name them by
    /// record handle.
    fn queue_table_blocks(
        doc: &document::Document,
        entity: &document::EntityType,
        seen: &mut rustc_hash::FxHashSet<String>,
        queue: &mut Vec<String>,
    ) {
        let document::EntityType::Table(table) = entity else {
            return;
        };
        for name in crate::app::command_driver::clipboard::table_cell_blocks(table)
            .filter_map(|h| doc.block_records.iter().find(|r| r.handle == h))
            .map(|record| record.name.clone())
        {
            if seen.insert(name.clone()) {
                queue.push(name);
            }
        }
    }

    /// Queue the arrow blocks of a dimension's (or leader's) style.
    fn queue_dim_style_blocks(
        doc: &document::Document,
        entity: &document::EntityType,
        seen: &mut rustc_hash::FxHashSet<String>,
        queue: &mut Vec<String>,
    ) {
        if let document::EntityType::MultiLeader(m) = entity {
            let mut m = m.clone();
            let style = m.style_handle.and_then(|h| doc.objects.get(&h));
            let handles = crate::app::command_driver::clipboard::mleader_handles(&mut m);
            let style_handles = match style {
                Some(document::objects::ObjectType::MultiLeaderStyle(s)) => {
                    [s.arrowhead_handle, s.block_content_handle]
                }
                _ => [None, None],
            };
            for h in handles
                .into_iter()
                .filter_map(|h| *h)
                .chain(style_handles.into_iter().flatten())
            {
                if let Some(r) = doc.block_records.iter().find(|r| r.handle == h) {
                    if seen.insert(r.name.clone()) {
                        queue.push(r.name.clone());
                    }
                }
            }
            return;
        }
        let style = match entity {
            document::EntityType::Dimension(d) => &d.base().style_name,
            document::EntityType::Leader(l) => &l.dimension_style,
            _ => return,
        };
        let Some(style) = doc.dim_styles.get(style) else {
            return;
        };
        for name in crate::app::command_driver::clipboard::dim_style_blocks(style)
            .into_iter()
            .filter(|h| !h.is_null())
            .filter_map(|h| doc.block_records.iter().find(|r| r.handle == h))
            .map(|record| record.name.clone())
        {
            if seen.insert(name.clone()) {
                queue.push(name);
            }
        }
    }

    /// Snapshot one block definition as a portable `BlockDef`: its base point
    /// and owned entities, minus the structural Block/BlockEnd markers. Returns
    /// None for model/paper space and xref blocks (not portable definitions).
    fn snapshot_block(doc: &document::Document, name: &str) -> Option<BlockDef> {
        use document::EntityType;
        let br = doc.block_records.get(name)?;
        if name.starts_with("*Model_Space") || name.starts_with("*Paper_Space") || br.flags.is_xref
        {
            return None;
        }
        let base_point = match doc.get_entity(br.block_entity_handle) {
            Some(EntityType::Block(b)) => b.base_point,
            _ => document::types::Vector3::ZERO,
        };
        let mut owned = Vec::new();
        for &eh in &br.entity_handles {
            let Some(e) = doc.get_entity(eh) else {
                continue;
            };
            if matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)) {
                continue;
            }
            owned.push(e.clone());
        }
        Some(BlockDef {
            name: name.to_string(),
            base_point,
            entities: owned,
            source: br.handle,
            graph: crate::scene::block_authoring::export_graph(doc, br.handle),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
            && self.linetypes.is_empty()
            && self.text_styles.is_empty()
            && self.dim_styles.is_empty()
            && self.blocks.is_empty()
    }
}
