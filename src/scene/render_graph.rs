//! Shared render-scene traversal.
//!
//! The drawing database stays authoritative. This layer only resolves the
//! hierarchy and per-instance context needed by render backends: space roots,
//! nested block references, transforms, arrays, visibility, style inheritance,
//! draw order, and clip boundaries. Leaf entities remain responsible for
//! producing their normal wire, hatch, image, wipeout, or mesh model.

use acadrust::entities::Insert;
use acadrust::types::{Color, Transform, Vector3};
use acadrust::{CadDocument, EntityType, Handle};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::scene::view::render::{
    has_resolved_book_color, is_effective_layer_zero, layer_render_style_viewport,
    render_style_for_block_sub_viewport, render_style_for_viewport, InheritStyle,
};

pub type ResolvedStyle = ([f32; 4], f32, [f32; 8], f32, u8);

/// A block record used as a render root. Model space, paper space, and an
/// ordinary definition opened for editing share the same ownership mechanism;
/// only their runtime role differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockRoot {
    pub record: Handle,
    pub role: BlockRootRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockRootRole {
    ModelSpace,
    PaperSpace,
    DefinitionEdit,
}

/// Semantic root of one render traversal. A viewport is a projection edge from
/// its paper-space owner to model-space content, not another storage container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneRoot {
    Block(BlockRoot),
    Viewport {
        paper_block: Handle,
        viewport: Handle,
        model_block: Handle,
    },
}

impl SceneRoot {
    pub fn content_block(self) -> Handle {
        match self {
            Self::Block(root) => root.record,
            Self::Viewport { model_block, .. } => model_block,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BlockStyle {
    pub insert: ResolvedStyle,
    pub layer0: InheritStyle,
    pub layer0_aci: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct InsertStyleSpec {
    own: BlockStyle,
    color_byblock: bool,
    color_bylayer: bool,
    transparency_byblock: bool,
    transparency_bylayer: bool,
    linetype_byblock: bool,
    linetype_bylayer: bool,
    lineweight_byblock: bool,
    lineweight_bylayer: bool,
    layer0: bool,
}

impl InsertStyleSpec {
    pub fn new(document: &CadDocument, insert: &Insert, viewport: Option<Handle>) -> Self {
        let entity = EntityType::Insert(insert.clone());
        let has_book_color = has_resolved_book_color(document, &entity);
        let linetype = &insert.common.linetype;
        Self {
            own: BlockStyle::for_entity(document, &entity, viewport),
            color_byblock: !has_book_color && insert.common.color == Color::ByBlock,
            color_bylayer: !has_book_color && insert.common.color == Color::ByLayer,
            transparency_byblock: insert.common.transparency.is_by_block(),
            transparency_bylayer: insert.common.transparency.is_by_layer(),
            linetype_byblock: linetype.eq_ignore_ascii_case("byblock"),
            linetype_bylayer: linetype.is_empty() || linetype.eq_ignore_ascii_case("bylayer"),
            lineweight_byblock: matches!(
                insert.common.line_weight,
                acadrust::types::LineWeight::ByBlock
            ),
            lineweight_bylayer: matches!(
                insert.common.line_weight,
                acadrust::types::LineWeight::ByLayer
                    | acadrust::types::LineWeight::Default
            ),
            layer0: is_effective_layer_zero(&insert.common.layer),
        }
    }

    pub fn resolve(self, parent: BlockStyle) -> BlockStyle {
        let mut insert = self.own.insert;
        if self.color_byblock {
            insert.0[0] = parent.insert.0[0];
            insert.0[1] = parent.insert.0[1];
            insert.0[2] = parent.insert.0[2];
            insert.4 = parent.insert.4;
        } else if self.layer0 && self.color_bylayer {
            insert.0[0] = parent.layer0.color[0];
            insert.0[1] = parent.layer0.color[1];
            insert.0[2] = parent.layer0.color[2];
            insert.4 = parent.layer0_aci;
        }
        insert.0[3] = if self.transparency_byblock {
            parent.insert.0[3]
        } else if self.layer0 && self.transparency_bylayer {
            parent.layer0.color[3]
        } else {
            insert.0[3]
        };
        if self.linetype_byblock {
            insert.1 = parent.insert.1;
            insert.2 = parent.insert.2;
        } else if self.layer0 && self.linetype_bylayer {
            insert.1 = parent.layer0.pat_len;
            insert.2 = parent.layer0.pat;
        }
        if self.lineweight_byblock {
            insert.3 = parent.insert.3;
        } else if self.layer0 && self.lineweight_bylayer {
            insert.3 = parent.layer0.lw_px;
        }
        BlockStyle {
            insert,
            layer0: if self.layer0 {
                parent.layer0
            } else {
                self.own.layer0
            },
            layer0_aci: if self.layer0 {
                parent.layer0_aci
            } else {
                self.own.layer0_aci
            },
        }
    }
}

impl BlockStyle {
    pub fn for_entity(
        document: &CadDocument,
        entity: &EntityType,
        viewport: Option<Handle>,
    ) -> Self {
        Self {
            insert: render_style_for_viewport(document, entity, viewport),
            layer0: layer_render_style_viewport(document, &entity.common().layer, viewport),
            layer0_aci: layer_aci(document, &entity.common().layer),
        }
    }

    pub fn for_nested(
        document: &CadDocument,
        insert: &Insert,
        parent: Self,
        viewport: Option<Handle>,
    ) -> Self {
        InsertStyleSpec::new(document, insert, viewport).resolve(parent)
    }

    pub fn resolve(
        self,
        document: &CadDocument,
        entity: &EntityType,
        viewport: Option<Handle>,
    ) -> ResolvedStyle {
        let mut resolved = render_style_for_block_sub_viewport(
            document,
            entity,
            self.insert.0,
            self.insert.1,
            self.insert.2,
            self.insert.3,
            self.layer0,
            viewport,
        );
        let common = entity.common();
        let has_book_color = has_resolved_book_color(document, entity);
        resolved.4 = if !has_book_color && common.color == Color::ByBlock {
            self.insert.4
        } else if !has_book_color
            && is_effective_layer_zero(&common.layer)
            && common.color == Color::ByLayer
        {
            self.layer0_aci
        } else {
            resolved.4
        };
        resolved
    }
}

fn layer_aci(document: &CadDocument, layer: &str) -> u8 {
    document
        .layers
        .get(layer)
        .and_then(|layer| match &layer.color {
            Color::Index(index) => Some(*index),
            _ => None,
        })
        .unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct RenderContext {
    pub transform: Transform,
    pub root_handle: Handle,
    pub parent_insert: Handle,
    pub insert_path: Vec<Insert>,
    pub clips: Vec<Vec<[f64; 2]>>,
    pub block_style: Option<BlockStyle>,
    pub depth_base: f32,
    pub depth_scale: f32,
    pub nesting_depth: usize,
    pub viewport: Option<Handle>,
}

impl RenderContext {
    fn direct(depth_base: f32, viewport: Option<Handle>) -> Self {
        Self {
            transform: Transform::identity(),
            root_handle: Handle::NULL,
            parent_insert: Handle::NULL,
            insert_path: Vec::new(),
            clips: Vec::new(),
            block_style: None,
            depth_base,
            depth_scale: 1.0,
            nesting_depth: 0,
            viewport,
        }
    }

    pub fn is_instanced(&self) -> bool {
        !self.root_handle.is_null()
    }

    pub fn style_for(&self, document: &CadDocument, entity: &EntityType) -> ResolvedStyle {
        self.block_style
            .map(|style| style.resolve(document, entity, self.viewport))
            .unwrap_or_else(|| render_style_for_viewport(document, entity, self.viewport))
    }

    pub fn draw_depth(&self, handle: Handle, depths: &FxHashMap<u64, [f32; 2]>) -> f32 {
        if self.is_instanced() {
            self.depth_base
                + depths
                    .get(&handle.value())
                    .map_or(0.0, |depth| depth[0])
                    * self.depth_scale
        } else {
            depths
                .get(&handle.value())
                .map_or(self.depth_base, |depth| depth[0])
        }
    }
}

pub struct RenderSceneGraph<'a> {
    document: &'a CadDocument,
    frozen_layers: Option<&'a FxHashSet<Handle>>,
    annotation_scale_handle: Option<Handle>,
    annotation_scale: f32,
    all_visible: bool,
    depths: &'a FxHashMap<u64, [f32; 2]>,
    viewport: Option<Handle>,
}

impl<'a> RenderSceneGraph<'a> {
    pub fn new(
        document: &'a CadDocument,
        frozen_layers: Option<&'a FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        depths: &'a FxHashMap<u64, [f32; 2]>,
    ) -> Self {
        Self {
            document,
            frozen_layers,
            annotation_scale_handle,
            annotation_scale: 1.0,
            all_visible,
            depths,
            viewport: None,
        }
    }

    pub fn with_viewport(mut self, viewport: Option<Handle>) -> Self {
        self.viewport = viewport.filter(|handle| handle.is_valid());
        self
    }

    pub fn with_annotation_scale(mut self, annotation_scale: f32) -> Self {
        self.annotation_scale = annotation_scale.max(1.0e-6);
        self
    }

    /// Walk direct root entities and every referenced block subtree. `visible`
    /// can add session-only rules such as isolate/preview hiding; returning
    /// false for an Insert removes its whole subtree.
    pub fn walk_root<V, F>(&self, root: SceneRoot, mut visible: V, mut leaf: F)
    where
        V: FnMut(&EntityType, &RenderContext) -> bool,
        F: FnMut(&EntityType, &RenderContext),
    {
        let block = root.content_block();
        let viewport = match root {
            SceneRoot::Viewport { viewport, .. } => Some(viewport),
            SceneRoot::Block(_) => self.viewport,
        };
        let Some(record) = self
            .document
            .block_records
            .iter()
            .find(|record| record.handle == block)
        else {
            return;
        };
        for &handle in &record.entity_handles {
            let Some(source) = self.document.get_entity(handle) else {
                continue;
            };
            let contextual = crate::scene::annotative::entity_for_annotation_context(
                self.document,
                source,
                self.annotation_scale_handle,
            );
            let entity = contextual.as_ref();
            let direct_depth = self
                .depths
                .get(&handle.value())
                .map_or(0.0, |depth| depth[0]);
            let context = RenderContext::direct(direct_depth, viewport);
            if !self.document_visible(entity) || !visible(entity, &context) {
                continue;
            }
            if let EntityType::Insert(insert) = entity {
                self.walk_insert_instances(insert, &context, &mut visible, &mut leaf);
            } else {
                leaf(entity, &context);
                self.walk_owned_content(entity, &context, &mut visible, &mut leaf, &mut Vec::new());
            }
        }
    }

    /// Walk one synthetic or document-owned Insert. Used by entity renderers
    /// whose content is itself a block reference.
    pub fn walk_insert<V, F>(
        &self,
        insert: &Insert,
        root_handle: Handle,
        mut visible: V,
        mut leaf: F,
    ) where
        V: FnMut(&EntityType, &RenderContext) -> bool,
        F: FnMut(&EntityType, &RenderContext),
    {
        let mut root_insert = insert.clone();
        root_insert.common.handle = root_handle;
        let entity = EntityType::Insert(root_insert.clone());
        let depth_base = self
            .depths
            .get(&root_handle.value())
            .map_or(0.0, |depth| depth[0]);
        let context = RenderContext::direct(depth_base, self.viewport);
        if self.document_visible(&entity) && visible(&entity, &context) {
            self.walk_insert_instances(&root_insert, &context, &mut visible, &mut leaf);
        }
    }


    fn walk_insert_instances<V, F>(
        &self,
        insert: &Insert,
        parent: &RenderContext,
        visible: &mut V,
        leaf: &mut F,
    ) where
        V: FnMut(&EntityType, &RenderContext) -> bool,
        F: FnMut(&EntityType, &RenderContext),
    {
        let insert_entity = EntityType::Insert(insert.clone());
        let block_style = parent
            .block_style
            .map(|style| BlockStyle::for_nested(self.document, insert, style, self.viewport))
            .unwrap_or_else(|| BlockStyle::for_entity(self.document, &insert_entity, self.viewport));
        let root_handle = if parent.root_handle.is_null() {
            insert.common.handle
        } else {
            parent.root_handle
        };
        let [depth_base, depth_scale] = if parent.root_handle.is_null() {
            self.depths
                .get(&insert.common.handle.value())
                .copied()
                .unwrap_or([0.0, 1.0])
        } else {
            let base = parent.draw_depth(insert.common.handle, self.depths);
            let count = self
                .document
                .block_records
                .get(&insert.block_name)
                .map_or(1, |record| record.entity_handles.len().max(1));
            [base, parent.depth_scale / (count as f32 + 1.0)]
        };

        for offset in array_offsets(insert) {
            let local = insert_instance_transform(
                self.document,
                insert,
                offset,
                self.annotation_scale,
            );
            let transform = local.then(&parent.transform);
            let mut context = parent.clone();
            context.transform = transform;
            context.root_handle = root_handle;
            context.parent_insert = insert.common.handle;
            context.insert_path.push(insert.clone());
            context.block_style = Some(block_style);
            context.depth_base = depth_base;
            context.depth_scale = depth_scale;
            context.nesting_depth += 1;
            if let Some(filter) = crate::scene::pick::xclip::insert_spatial_filter(
                self.document,
                insert,
            ) {
                let polygon = crate::scene::pick::xclip::world_clip_polygon_for_transform(
                    filter,
                    &transform,
                );
                if polygon.len() >= 3 {
                    context.clips.push(polygon);
                }
            }
            let mut stack = context
                .insert_path
                .iter()
                .map(|insert| insert.block_name.clone())
                .collect();
            self.walk_block(
                &insert.block_name,
                &context,
                visible,
                leaf,
                &mut stack,
            );
        }
    }

    fn walk_block<V, F>(
        &self,
        block_name: &str,
        context: &RenderContext,
        visible: &mut V,
        leaf: &mut F,
        stack: &mut Vec<String>,
    ) where
        V: FnMut(&EntityType, &RenderContext) -> bool,
        F: FnMut(&EntityType, &RenderContext),
    {
        if context.nesting_depth > 32 {
            return;
        }
        let Some(record) = block_record_by_name(self.document, block_name) else {
            return;
        };
        for &handle in &record.entity_handles {
            let Some(source) = self.document.get_entity(handle) else {
                continue;
            };
            let contextual = crate::scene::annotative::entity_for_annotation_context(
                self.document,
                source,
                self.annotation_scale_handle,
            );
            let entity = contextual.as_ref();
            if !self.document_visible(entity) || !visible(entity, context) {
                continue;
            }
            match entity {
                EntityType::Block(_)
                | EntityType::BlockEnd(_)
                | EntityType::AttributeDefinition(_) => {}
                EntityType::Insert(nested) => {
                    if stack
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(&nested.block_name))
                    {
                        continue;
                    }
                    stack.push(nested.block_name.clone());
                    self.walk_insert_instances(nested, context, visible, leaf);
                    stack.pop();
                }
                _ => {
                    leaf(entity, context);
                    self.walk_owned_content(entity, context, visible, leaf, stack);
                }
            }
        }
    }

    fn walk_owned_content<V, F>(
        &self,
        entity: &EntityType,
        context: &RenderContext,
        visible: &mut V,
        leaf: &mut F,
        stack: &mut Vec<String>,
    ) where
        V: FnMut(&EntityType, &RenderContext) -> bool,
        F: FnMut(&EntityType, &RenderContext),
    {
        for block_use in entity_render_block_uses(self.document, entity, self.annotation_scale)
            .into_iter()
            .filter(|block_use| block_use.active)
        {
            if stack
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&block_use.insert.block_name))
            {
                continue;
            }
            stack.push(block_use.insert.block_name.clone());
            self.walk_insert_instances(&block_use.insert, context, visible, leaf);
            stack.pop();
        }
    }

    fn document_visible(&self, entity: &EntityType) -> bool {
        let common = entity.common();
        if common.invisible {
            return false;
        }
        let layer = self.document.layers.get(&common.layer);
        if layer
            .map(|layer| layer.flags.off || layer.flags.frozen)
            .unwrap_or(false)
        {
            return false;
        }
        if self.frozen_layers.is_some_and(|frozen| {
            layer.is_some_and(|layer| frozen.contains(&layer.handle))
        }) {
            return false;
        }
        !crate::scene::annotative::annotative_offscale_for(
            self.document,
            common,
            self.annotation_scale_handle,
            self.all_visible,
        )
    }
}

pub fn block_base_point(document: &CadDocument, block_name: &str) -> Vector3 {
    document
        .block_records
        .iter()
        .find(|record| record.name.eq_ignore_ascii_case(block_name))
        .map(|record| {
            document
                .get_entity(record.block_entity_handle)
                .and_then(|entity| match entity {
                    EntityType::Block(block) => Some(block.base_point),
                    _ => None,
                })
                .unwrap_or(record.base_point)
        })
        .unwrap_or(Vector3::ZERO)
}

pub fn insert_transform(document: &CadDocument, insert: &Insert) -> Transform {
    let base = block_base_point(document, &insert.block_name);
    Transform::from_translation(Vector3::new(-base.x, -base.y, -base.z))
        .then(&insert.get_transform())
}

pub fn insert_transform_at_scale(
    document: &CadDocument,
    insert: &Insert,
    annotation_scale: f32,
) -> Transform {
    let mut transform = insert_transform(document, insert);
    if (annotation_scale - 1.0).abs() > 1.0e-6
        && insert
            .common
            .extended_data
            .get_record("AcAnnotativeData")
            .is_some()
    {
        let point = insert.insert_point;
        let scale = Transform::from_translation(Vector3::new(-point.x, -point.y, -point.z))
            .then(&Transform::from_scale(annotation_scale as f64))
            .then(&Transform::from_translation(Vector3::new(
                point.x, point.y, point.z,
            )));
        transform = transform.then(&scale);
    }
    transform
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockRole {
    Insert,
    DimensionPicture,
    TablePicture,
    TableCell,
    MultiLeaderContent,
    LineStyleSymbol,
    ArrowHead,
}

#[derive(Clone)]
pub struct BlockUse {
    pub block: Handle,
    pub insert: Insert,
    pub role: BlockRole,
    /// Whether this edge belongs to the active render representation. Inactive
    /// edges remain dependencies so cache invalidation and purge stay complete.
    pub active: bool,
    pub replaces_host_wire: bool,
    pub suppress_root_points: bool,
}

pub(crate) fn block_record_by_name<'a>(
    document: &'a CadDocument,
    name: &str,
) -> Option<&'a acadrust::tables::BlockRecord> {
    document.block_records.get(name).or_else(|| {
        document
            .block_records
            .iter()
            .find(|record| record.name.eq_ignore_ascii_case(name))
    })
}

fn block_use(
    document: &CadDocument,
    mut insert: Insert,
    role: BlockRole,
    active: bool,
    replaces_host_wire: bool,
    suppress_root_points: bool,
) -> BlockUse {
    let block =
        block_record_by_name(document, &insert.block_name).map_or(Handle::NULL, |record| {
            insert.block_name.clone_from(&record.name);
            record.handle
        });
    BlockUse {
        block,
        insert,
        role,
        active,
        replaces_host_wire,
        suppress_root_points,
    }
}

pub fn block_use_from_handle(
    document: &CadDocument,
    block: Handle,
    role: BlockRole,
    insertion: Vector3,
) -> Option<BlockUse> {
    let record = document
        .block_records
        .iter()
        .find(|record| record.handle == block)?;
    Some(block_use(
        document,
        Insert::new(record.name.clone(), insertion),
        role,
        true,
        false,
        false,
    ))
}

fn push_dependency_use(
    uses: &mut Vec<BlockUse>,
    document: &CadDocument,
    block: Handle,
    role: BlockRole,
) {
    if block.is_null()
        || uses
            .iter()
            .any(|block_use| block_use.block == block && block_use.role == role)
    {
        return;
    }
    if let Some(mut block_use) =
        block_use_from_handle(document, block, role, Vector3::ZERO)
    {
        block_use.active = false;
        uses.push(block_use);
    }
}

/// Every direct block edge owned by an entity. One host may have multiple
/// edges, such as block-valued table cells. Representation flags let renderers
/// choose the active picture while dependency consumers keep the full graph.
fn entity_owned_block_uses(
    document: &CadDocument,
    entity: &EntityType,
    annotation_scale: f32,
) -> Vec<BlockUse> {
    match entity {
        EntityType::Insert(insert) => vec![block_use(
            document,
            insert.clone(),
            BlockRole::Insert,
            true,
            true,
            false,
        )],
        EntityType::Dimension(dimension) => {
            let name = dimension.base().block_name.trim();
            if let Some(record) = block_record_by_name(document, name)
                .filter(|record| !record.entity_handles.is_empty())
            {
                let mut insert = Insert::new(record.name.clone(), Vector3::ZERO);
                insert.common = dimension.base().common.clone();
                let picture = !crate::scene::annotative::is_annotative(document, entity);
                vec![block_use(
                    document,
                    insert,
                    BlockRole::DimensionPicture,
                    picture,
                    picture,
                    true,
                )]
            } else {
                Vec::new()
            }
        }
        EntityType::Table(table) => {
            let picture_record = table.block_record_handle.and_then(|handle| {
                document
                    .block_records
                    .iter()
                    .find(|record| record.handle == handle)
                    .filter(|record| !record.entity_handles.is_empty())
            });
            let mut uses = Vec::new();
            if let Some(record) = picture_record {
                let mut insert = Insert::new(record.name.clone(), table.insertion_point);
                insert.rotation = table
                    .horizontal_direction
                    .y
                    .atan2(table.horizontal_direction.x);
                insert.common = table.common.clone();
                uses.push(block_use(
                    document,
                    insert,
                    BlockRole::TablePicture,
                    true,
                    true,
                    false,
                ));
            }
            let cells_active = picture_record.is_none();
            uses.extend(
                crate::entities::table::block_cell_inserts(table, document, annotation_scale)
                    .into_iter()
                    .map(|mut insert| {
                        insert
                            .common
                            .extended_data
                            .remove_record("AcAnnotativeData");
                        block_use(
                            document,
                            insert,
                            BlockRole::TableCell,
                            cells_active,
                            false,
                            false,
                        )
                    }),
            );
            uses
        }
        EntityType::MultiLeader(multileader) => {
            crate::entities::multileader::block_content_insert(document, multileader)
                .map(|mut insert| {
                    insert
                        .common
                        .extended_data
                        .remove_record("AcAnnotativeData");
                    vec![block_use(
                        document,
                        insert,
                        BlockRole::MultiLeaderContent,
                        true,
                        false,
                        false,
                    )]
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

pub fn entity_render_block_uses(
    document: &CadDocument,
    entity: &EntityType,
    annotation_scale: f32,
) -> Vec<BlockUse> {
    entity_owned_block_uses(document, entity, annotation_scale)
}

pub fn entity_block_uses(
    document: &CadDocument,
    entity: &EntityType,
    annotation_scale: f32,
) -> Vec<BlockUse> {
    let mut uses = entity_owned_block_uses(document, entity, annotation_scale);

    let line_type = crate::scene::view::render::linetype_name_for(document, entity);
    for symbol in crate::scene::convert::dgn_linestyle::symbol_blocks(document, line_type) {
        push_dependency_use(
            &mut uses,
            document,
            symbol.block,
            BlockRole::LineStyleSymbol,
        );
    }

    match entity {
        EntityType::Dimension(dimension) => {
            let style_name = dimension.base().style_name.trim();
            if let Some(style) = document.dim_styles.iter().find(|style| {
                style.name.eq_ignore_ascii_case(style_name)
                    || (style_name.is_empty() && style.name.eq_ignore_ascii_case("Standard"))
            }) {
                let style = crate::entities::dimension::resolved_dimension_style(
                    style,
                    dimension,
                    document,
                );
                for handle in [style.dimblk, style.dimblk1, style.dimblk2, style.dimldrblk] {
                    push_dependency_use(
                        &mut uses,
                        document,
                        handle,
                        BlockRole::ArrowHead,
                    );
                }
                for name in [style.dimblk_name, style.dimblk1_name, style.dimblk2_name] {
                    if let Some(record) = block_record_by_name(document, &name) {
                        push_dependency_use(
                            &mut uses,
                            document,
                            record.handle,
                            BlockRole::ArrowHead,
                        );
                    }
                }
            }
        }
        EntityType::Leader(leader) => {
            let style = document.dim_styles.iter().find(|style| {
                style.name.eq_ignore_ascii_case(&leader.dimension_style)
                    || (leader.dimension_style.trim().is_empty()
                        && style.name.eq_ignore_ascii_case("Standard"))
            });
            let handle = crate::entities::dim_override::handle(
                &leader.common.extended_data,
                crate::entities::dim_override::DIMLDRBLK,
            )
            .or_else(|| style.map(|style| style.dimldrblk));
            if let Some(handle) = handle {
                push_dependency_use(
                    &mut uses,
                    document,
                    handle,
                    BlockRole::ArrowHead,
                );
            }
        }
        EntityType::MultiLeader(multileader) => {
            if let Some(handle) = multileader.arrowhead_handle {
                push_dependency_use(
                    &mut uses,
                    document,
                    handle,
                    BlockRole::ArrowHead,
                );
            }
            for line in multileader
                .context
                .leader_roots
                .iter()
                .flat_map(|root| &root.lines)
            {
                if let Some(handle) = line.arrowhead_handle {
                    push_dependency_use(
                        &mut uses,
                        document,
                        handle,
                        BlockRole::ArrowHead,
                    );
                }
            }
        }
        _ => {}
    }
    uses
}

/// Blocks referenced by document-level styles rather than a concrete entity.
pub fn document_block_uses(document: &CadDocument) -> Vec<BlockUse> {
    let mut uses = Vec::new();
    for style in document.dim_styles.iter() {
        for handle in [style.dimblk, style.dimblk1, style.dimblk2, style.dimldrblk] {
            push_dependency_use(&mut uses, document, handle, BlockRole::ArrowHead);
        }
        for name in [&style.dimblk_name, &style.dimblk1_name, &style.dimblk2_name] {
            if let Some(record) = block_record_by_name(document, name) {
                push_dependency_use(
                    &mut uses,
                    document,
                    record.handle,
                    BlockRole::ArrowHead,
                );
            }
        }
    }
    for object in document.objects.values() {
        let acadrust::objects::ObjectType::MultiLeaderStyle(style) = object else {
            continue;
        };
        if let Some(handle) = style.arrowhead_handle {
            push_dependency_use(&mut uses, document, handle, BlockRole::ArrowHead);
        }
        if let Some(handle) = style.block_content_handle {
            push_dependency_use(
                &mut uses,
                document,
                handle,
                BlockRole::MultiLeaderContent,
            );
        }
    }
    for line_type in document.line_types.iter() {
        for symbol in
            crate::scene::convert::dgn_linestyle::symbol_blocks(document, &line_type.name)
        {
            push_dependency_use(
                &mut uses,
                document,
                symbol.block,
                BlockRole::LineStyleSymbol,
            );
        }
    }
    uses
}

pub fn array_offsets(insert: &Insert) -> Vec<[f64; 3]> {
    if !insert.is_minsert() {
        return vec![[0.0; 3]];
    }
    let mut offsets = Vec::with_capacity(insert.instance_count());
    for row in 0..insert.row_count {
        for column in 0..insert.column_count {
            offsets.push([
                column as f64 * insert.column_spacing,
                row as f64 * insert.row_spacing,
                0.0,
            ]);
        }
    }
    offsets
}

pub fn insert_instance_transform(
    document: &CadDocument,
    insert: &Insert,
    offset: [f64; 3],
    annotation_scale: f32,
) -> Transform {
    let transform = insert_transform_at_scale(document, insert, annotation_scale);
    if offset == [0.0; 3] {
        transform
    } else {
        Transform::from_translation(Vector3::new(offset[0], offset[1], offset[2]))
            .then(&transform)
    }
}

pub fn block_contains_hatch(
    document: &CadDocument,
    block_name: &str,
    memo: &mut std::collections::HashMap<String, bool>,
) -> bool {
    let key = block_name.to_ascii_lowercase();
    if let Some(&contains) = memo.get(&key) {
        return contains;
    }
    memo.insert(key.clone(), false);
    let contains = block_record_by_name(document, block_name).is_some_and(|record| {
        record
            .entity_handles
            .iter()
            .any(|&handle| match document.get_entity(handle) {
                Some(EntityType::Hatch(_)) => true,
                Some(entity) => entity_render_block_uses(document, entity, 1.0)
                    .into_iter()
                    .filter(|block_use| block_use.active)
                    .any(|block_use| {
                        block_contains_hatch(document, &block_use.insert.block_name, memo)
                    }),
                _ => false,
            })
    });
    memo.insert(key, contains);
    contains
}
