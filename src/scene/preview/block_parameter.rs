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
            paper.then_some(1.0),
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

    #[doc(hidden)]
    pub fn bench_setup_dynamic_block_scene(
        background_lines: usize,
        background_hatches: usize,
        block_lines: usize,
    ) -> (Self, Handle, Handle, Handle) {
        use crate::scene::{
            block_actions as actions, block_distance_parameter as distance,
            block_orientation_parameter as orientation,
        };
        use document::entities::{BoundaryEdge, BoundaryPath, Hatch, Insert, Line, PolylineEdge};
        use document::types::Vector2;
        use document::Vector3;
        use glam::DVec3;

        let mut scene = Scene::new();
        for i in 0..background_lines {
            let y = (i as f64) * 2.0;
            let mut line = Line::new();
            line.start = Vector3::new(100.0, y, 0.0);
            line.end = Vector3::new(200.0, y, 0.0);
            scene.add_entity(EntityType::Line(line));
        }
        for i in 0..background_hatches {
            let mut hatch = Hatch::new();
            hatch.is_solid = true;
            let mut path = BoundaryPath::new();
            let base = 500.0 + (i as f64) * 5.0;
            path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
                vec![
                    Vector2::new(base, 0.0),
                    Vector2::new(base + 3.0, 0.0),
                    Vector2::new(base + 3.0, 3.0),
                    Vector2::new(base, 3.0),
                ],
                true,
            )));
            hatch.paths.push(path);
            scene.add_entity(EntityType::Hatch(Box::new(hatch)));
        }

        let mut block_entities = Vec::with_capacity(block_lines + 1);
        for i in 0..block_lines {
            let y = (i as f64) * 0.2;
            let mut line = Line::new();
            line.start = Vector3::new(0.0, y, 0.0);
            line.end = Vector3::new(10.0, y, 0.0);
            block_entities.push(EntityType::Line(line));
        }
        let mut hatch = Hatch::new();
        hatch.is_solid = true;
        let mut path = BoundaryPath::new();
        path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
            vec![
                Vector2::new(1.0, 0.0),
                Vector2::new(9.0, 0.0),
                Vector2::new(9.0, 2.0),
                Vector2::new(1.0, 2.0),
            ],
            true,
        )));
        hatch.paths.push(path);
        block_entities.push(EntityType::Hatch(Box::new(hatch)));
        scene.define_block_raw("DynBenchBlock", Vector3::ZERO, block_entities);
        let block = scene
            .document
            .block_records
            .get("DynBenchBlock")
            .expect("block record");
        let def = block.handle;
        let targets = block.entity_handles.clone();

        let stretch_param = distance::create_with(
            &mut scene.document,
            def,
            distance::Kind::Linear,
            Vector3::ZERO,
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(5.0, -2.0, 0.0),
            None,
            None,
        )
        .expect("linear parameter");
        let frame = [DVec3::new(5.0, -2.0, 0.0), DVec3::new(12.0, 12.0, 0.0)];
        actions::create_with_frame_points(
            &mut scene.document,
            def,
            actions::Kind::Stretch,
            stretch_param,
            None,
            &targets[..block_lines],
            DVec3::new(10.0, 0.0, 0.0),
            Some(&frame),
            None,
        )
        .expect("stretch action");

        let flip_param = orientation::create_with_label(
            &mut scene.document,
            def,
            orientation::Kind::Flip,
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(5.0, 5.0, 0.0),
            Vector3::new(6.0, 2.5, 0.0),
            None,
            None,
        )
        .expect("flip parameter");
        actions::create_with_frame_points(
            &mut scene.document,
            def,
            actions::Kind::Flip,
            flip_param,
            None,
            &targets[..block_lines],
            DVec3::new(5.0, 2.5, 0.0),
            None,
            None,
        )
        .expect("flip action");

        orientation::create_with_label(
            &mut scene.document,
            def,
            orientation::Kind::Alignment,
            Vector3::ZERO,
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::ZERO,
            None,
            None,
        )
        .expect("alignment parameter");

        let insert = Insert::new("DynBenchBlock", Vector3::ZERO);
        let insert_handle = scene.add_entity(EntityType::Insert(insert));
        scene.dynamic_representation(insert_handle);

        (scene, insert_handle, stretch_param, flip_param)
    }

    #[doc(hidden)]
    pub fn bench_block_parameter_preview(
        &mut self,
        handle: Handle,
        parameter: Handle,
        local_point: glam::DVec3,
    ) -> Option<(Vec<WireModel>, Vec<HatchModel>)> {
        let def = crate::scene::block_actions::instance::definition(&self.document, handle)?;
        let scoped = crate::scene::block_actions::instance::scoped_document_for_insert(
            &mut self.document,
            handle,
            def,
        );
        let staged = crate::scene::block_actions::instance::evaluate_value(
            &scoped,
            handle,
            parameter,
            crate::scene::block_actions::Value::Point(local_point),
        )
        .ok()?;
        self.reference_document_preview(staged.as_ref().unwrap_or(&scoped), handle)
    }

    #[doc(hidden)]
    pub fn bench_block_flip_preview(
        &mut self,
        handle: Handle,
        parameter: Handle,
        flipped: bool,
    ) -> Option<(Vec<WireModel>, Vec<HatchModel>)> {
        let def = crate::scene::block_actions::instance::definition(&self.document, handle)?;
        let scoped = crate::scene::block_actions::instance::scoped_document_for_insert(
            &mut self.document,
            handle,
            def,
        );
        let staged = crate::scene::block_actions::instance::evaluate_value(
            &scoped,
            handle,
            parameter,
            crate::scene::block_actions::Value::Flip(flipped),
        )
        .ok()?;
        self.reference_document_preview(staged.as_ref().unwrap_or(&scoped), handle)
    }

    #[doc(hidden)]
    pub fn bench_start_alignment_command(
        &self,
        insert: Handle,
    ) -> Option<Box<dyn crate::command::CadCommand>> {
        let alignment = self.alignment_grips(insert).into_iter().next()?;
        let targets = crate::scene::block_alignment::Targets::from_scene(
            self,
            insert,
            alignment.plane,
        );
        Some(Box::new(
            crate::modules::block_editor::alignment::AlignmentCommand::new(
                1, alignment, targets,
            ),
        ))
    }
}
