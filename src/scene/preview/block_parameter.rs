use super::*;

impl Scene {
    /// Render an evaluated reference without installing its transient document
    /// or representation in the resident scene/cache.
    pub(crate) fn reference_document_preview(
        &self,
        document: &Document,
        handle: Handle,
    ) -> Option<(Vec<WireModel>, Vec<HatchModel>)> {
        let entity @ EntityType::Insert(insert) = document.get_entity(handle)? else {
            return None;
        };
        let (bg, anno, annotation, _resident_cache, paper) = self.tessellation_context();
        let depths = self.draw_depth_map();
        let cache = cache::block_cache::BlockCache::build_for_block(
            document,
            &insert.block_name,
            anno,
            annotation,
            self.annotation_all_visible(),
            bg,
            self.active_viewport,
            depths.as_ref(),
        );
        let wires = tessellate_entity(
            document,
            &self.selected,
            self.active_viewport,
            bg,
            anno,
            annotation,
            entity,
            Some(&cache),
            None,
            None,
            paper,
            self.reference_only.handles(),
        );
        let frozen: rustc_hash::FxHashSet<Handle> = self
            .interaction_viewport_frozen_layers()
            .into_iter()
            .flatten()
            .copied()
            .collect();
        let targets = [handle].into_iter().collect();
        let hatches = self.instanced_hatch_models_for_document(
            document,
            self.interaction_block_handle(),
            bg,
            true,
            (!frozen.is_empty()).then_some(&frozen),
            annotation,
            self.annotation_all_visible(),
            self.active_viewport,
            Some(&targets),
            true,
            false,
        );
        Some((wires, hatches))
    }
}
