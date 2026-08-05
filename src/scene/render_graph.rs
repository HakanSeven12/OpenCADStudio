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
            insert.0 = parent.insert.0;
            insert.4 = parent.insert.4;
        } else if self.layer0 && self.color_bylayer {
            insert.0 = parent.layer0.color;
            insert.4 = parent.layer0_aci;
        }
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

    pub fn for_owned(
        document: &CadDocument,
        entity: &EntityType,
        parent: Option<Self>,
        viewport: Option<Handle>,
    ) -> Self {
        let on_layer0 = is_effective_layer_zero(&entity.common().layer);
        let insert = parent
            .map(|style| style.resolve(document, entity, viewport))
            .unwrap_or_else(|| render_style_for_viewport(document, entity, viewport));
        Self {
            insert,
            layer0: if on_layer0 {
                parent
                    .map(|style| style.layer0)
                    .unwrap_or_else(|| layer_render_style_viewport(document, &entity.common().layer, viewport))
            } else {
                layer_render_style_viewport(document, &entity.common().layer, viewport)
            },
            layer0_aci: if on_layer0 {
                parent
                    .map(|style| style.layer0_aci)
                    .unwrap_or_else(|| layer_aci(document, &entity.common().layer))
            } else {
                layer_aci(document, &entity.common().layer)
            },
        }
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
            all_visible,
            depths,
            viewport: None,
        }
    }

    pub fn with_viewport(mut self, viewport: Option<Handle>) -> Self {
        self.viewport = viewport.filter(|handle| handle.is_valid());
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
            let local = insert_instance_transform(self.document, insert, offset);
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
        let Some(record) = self.document.block_records.get(block_name) else {
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
        match entity {
            EntityType::Dimension(dimension) => {
                let block_name = dimension.base().block_name.trim();
                if block_name.is_empty()
                    || stack
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(block_name))
                {
                    return;
                }
                let placement =
                    Transform::from_translation(dimension.base().insertion_point)
                        .then(&context.transform);
                let mut owned = context.clone();
                owned.transform = placement;
                if owned.root_handle.is_null() {
                    owned.root_handle = entity.common().handle;
                }
                owned.block_style = Some(BlockStyle::for_owned(
                    self.document,
                    entity,
                    context.block_style,
                    self.viewport,
                ));
                owned.nesting_depth += 1;
                stack.push(block_name.to_string());
                self.walk_block(block_name, &owned, visible, leaf, stack);
                stack.pop();
            }
            EntityType::Table(table) => {
                let Some(record) = table.block_record_handle.and_then(|handle| {
                    self.document
                        .block_records
                        .iter()
                        .find(|record| record.handle == handle)
                }) else {
                    return;
                };
                let mut insert = Insert::new(record.name.clone(), table.insertion_point);
                insert.rotation = table
                    .horizontal_direction
                    .y
                    .atan2(table.horizontal_direction.x);
                insert.common = table.common.clone();
                self.walk_insert_instances(&insert, context, visible, leaf);
            }
            EntityType::MultiLeader(multileader)
                if matches!(
                    multileader.content_type,
                    acadrust::entities::LeaderContentType::Block
                ) && multileader.context.has_block_contents =>
            {
                let Some(record) = multileader.block_content_handle.and_then(|handle| {
                    self.document
                        .block_records
                        .iter()
                        .find(|record| record.handle == handle)
                }) else {
                    return;
                };
                let mut insert = Insert::new(
                    record.name.clone(),
                    multileader.context.block_content_location,
                );
                insert.common = multileader.common.clone();
                insert.common.color = multileader.block_content_color.clone();
                insert.set_x_scale(multileader.block_scale.x);
                insert.set_y_scale(multileader.block_scale.y);
                insert.set_z_scale(multileader.block_scale.z);
                insert.rotation = multileader.block_rotation;
                insert.normal = multileader.context.block_content_normal;
                self.walk_insert_instances(&insert, context, visible, leaf);
            }
            _ => {}
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
        .get(block_name)
        .and_then(|record| document.get_entity(record.block_entity_handle))
        .and_then(|entity| match entity {
            EntityType::Block(block) => Some(block.base_point),
            _ => None,
        })
        .unwrap_or(Vector3::ZERO)
}

pub fn insert_transform(document: &CadDocument, insert: &Insert) -> Transform {
    let base = block_base_point(document, &insert.block_name);
    Transform::from_translation(Vector3::new(-base.x, -base.y, -base.z))
        .then(&insert.get_transform())
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
) -> Transform {
    let transform = insert_transform(document, insert);
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
    if let Some(&contains) = memo.get(block_name) {
        return contains;
    }
    memo.insert(block_name.to_string(), false);
    let contains = document
        .block_records
        .get(block_name)
        .is_some_and(|record| {
            record
                .entity_handles
                .iter()
                .any(|&handle| match document.get_entity(handle) {
                    Some(EntityType::Hatch(_)) => true,
                    Some(EntityType::Insert(insert)) => {
                        block_contains_hatch(document, &insert.block_name, memo)
                    }
                    Some(EntityType::Dimension(dimension)) => {
                        let name = dimension.base().block_name.trim();
                        !name.is_empty() && block_contains_hatch(document, name, memo)
                    }
                    Some(EntityType::Table(table)) => table
                        .block_record_handle
                        .and_then(|handle| {
                            document
                                .block_records
                                .iter()
                                .find(|record| record.handle == handle)
                        })
                        .is_some_and(|record| {
                            block_contains_hatch(document, &record.name, memo)
                        }),
                    Some(EntityType::MultiLeader(multileader)) => multileader
                        .block_content_handle
                        .and_then(|handle| {
                            document
                                .block_records
                                .iter()
                                .find(|record| record.handle == handle)
                        })
                        .is_some_and(|record| {
                            block_contains_hatch(document, &record.name, memo)
                        }),
                    _ => false,
                })
        });
    memo.insert(block_name.to_string(), contains);
    contains
}
