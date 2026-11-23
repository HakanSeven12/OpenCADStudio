use super::*;


    type Handles = rustc_hash::FxHashSet<document::Handle>;

    /// Triangle indices of the block's solid.
    const INDICES: u32 = 36;

    fn handle(index: u32) -> document::Handle {
        document::Handle::new(0x100 + u64::from(index))
    }

    /// The resident ranges of `count` references of one block: the solid's
    /// triangles once, and an instance of them for each reference.
    fn references_of_one_block(
        count: u32,
    ) -> rustc_hash::FxHashMap<document::Handle, Vec<MeshResidentRange>> {
        (0..count)
            .map(|index| {
                (
                    handle(index),
                    vec![MeshResidentRange {
                        chunk: 0,
                        dynamic: false,
                        index_start: 0,
                        index_count: INDICES,
                        transparent: false,
                        instance_start: index,
                        instance_count: 1,
                    }],
                )
            })
            .collect()
    }

    /// Each index of each instance the draws cover, in which buffers and with
    /// which tint, as often as it is drawn.
    fn drawn(draws: &[MeshHighlightDraw]) -> Vec<(bool, bool, usize, bool, u32, u32)> {
        let mut out = Vec::new();
        for draw in draws {
            let range = &draw.range;
            for instance in range.instance_start..range.instance_start + range.instance_count {
                for index in range.index_start..range.index_start + range.index_count {
                    out.push((
                        draw.kind == MeshHighlightKind::Hover,
                        range.dynamic,
                        range.chunk,
                        range.transparent,
                        index,
                        instance,
                    ));
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// With every reference of a block picked, the solids under the highlight
    /// are one instanced draw. The highlight must not be a draw for each
    /// reference on every frame.
    #[test]
    fn every_reference_of_a_block_picked_is_highlighted_in_one_draw() {
        const REFERENCES: u32 = 1_000;
        let ranges = references_of_one_block(REFERENCES);
        let selected: Handles = (0..REFERENCES).map(handle).collect();

        let draws = Pipeline::mesh_highlight_draws(
            &ranges,
            &selected,
            &Handles::default(),
            &Handles::default(),
        );

        let every_index_of_every_reference: Vec<_> = (0..INDICES)
            .flat_map(|index| {
                (0..REFERENCES).map(move |instance| (false, false, 0, false, index, instance))
            })
            .collect();
        let mut expected = every_index_of_every_reference;
        expected.sort_unstable();
        assert_eq!(drawn(&draws), expected, "each reference once, whole");
        assert_eq!(
            draws.len(),
            1,
            "{} highlight draws for {REFERENCES} picked references of one block",
            draws.len(),
        );
    }

    /// The draws as they were before any were joined: every range of every
    /// picked or hovered solid, on its own.
    fn one_draw_for_each_range(
        ranges: &rustc_hash::FxHashMap<document::Handle, Vec<MeshResidentRange>>,
        selected: &Handles,
        hovered: &Handles,
        edge_handles: &Handles,
    ) -> Vec<MeshHighlightDraw> {
        let mut out = Vec::new();
        for (handle, ranges) in ranges {
            let kind = if edge_handles.contains(handle) {
                continue;
            } else if selected.contains(handle) {
                MeshHighlightKind::Selected
            } else if hovered.contains(handle) {
                MeshHighlightKind::Hover
            } else {
                continue;
            };
            out.extend(ranges.iter().map(|range| MeshHighlightDraw {
                range: *range,
                kind,
            }));
        }
        out
    }

    /// References that do not follow one another keep a draw each.
    #[test]
    fn references_apart_keep_a_draw_each() {
        const REFERENCES: u32 = 200;
        let ranges = references_of_one_block(REFERENCES);
        let selected: Handles = (0..REFERENCES).step_by(2).map(handle).collect();
        let none = Handles::default();

        let draws = Pipeline::mesh_highlight_draws(&ranges, &selected, &none, &none);

        assert_eq!(draws.len(), REFERENCES as usize / 2);
        assert_eq!(
            drawn(&draws),
            drawn(&one_draw_for_each_range(&ranges, &selected, &none, &none)),
        );
    }

    /// Solids of their own, whose triangles follow one another in a chunk,
    /// are one draw when they are all picked.
    #[test]
    fn solids_whose_triangles_follow_one_another_are_one_draw() {
        const SOLIDS: u32 = 300;
        let ranges: rustc_hash::FxHashMap<_, _> = (0..SOLIDS)
            .map(|index| {
                (
                    handle(index),
                    vec![MeshResidentRange {
                        chunk: 2,
                        dynamic: false,
                        index_start: index * INDICES,
                        index_count: INDICES,
                        transparent: false,
                        instance_start: 0,
                        instance_count: 1,
                    }],
                )
            })
            .collect();
        let selected: Handles = (0..SOLIDS).map(handle).collect();
        let none = Handles::default();

        let draws = Pipeline::mesh_highlight_draws(&ranges, &selected, &none, &none);

        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].range.index_start, 0);
        assert_eq!(draws[0].range.index_count, SOLIDS * INDICES);
        assert_eq!(
            drawn(&draws),
            drawn(&one_draw_for_each_range(&ranges, &selected, &none, &none)),
        );
    }

    /// A picked reference and the hovered one after it keep their own tints.
    #[test]
    fn a_picked_reference_and_a_hovered_one_are_not_joined() {
        let ranges = references_of_one_block(3);
        let selected: Handles = [handle(0), handle(2)].into_iter().collect();
        let hovered: Handles = [handle(1), handle(2)].into_iter().collect();
        let none = Handles::default();

        let draws = Pipeline::mesh_highlight_draws(&ranges, &selected, &hovered, &none);

        assert_eq!(draws.len(), 3, "picked, hovered, picked");
        assert_eq!(
            draws
                .iter()
                .filter(|draw| draw.kind == MeshHighlightKind::Hover)
                .map(|draw| draw.range.instance_start)
                .collect::<Vec<_>>(),
            vec![1],
            "the picked reference under the cursor is drawn as picked only",
        );
        assert_eq!(
            drawn(&draws),
            drawn(&one_draw_for_each_range(&ranges, &selected, &hovered, &none)),
        );
    }

    /// Whatever is picked, hovered or highlighted by its edges, the joined
    /// draws cover what a draw for each range covers: the same indices of the
    /// same instances in the same buffers, with the same tint, as many times.
    #[test]
    fn joined_draws_cover_what_a_draw_for_each_range_covers() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = |below: u32| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 33) as u32) % below
        };
        let mut joined_any = false;
        for round in 0..300 {
            let mut ranges: rustc_hash::FxHashMap<document::Handle, Vec<MeshResidentRange>> =
                rustc_hash::FxHashMap::default();
            let mut id = 0;
            for chunk in 0..1 + next(3) as usize {
                let dynamic = next(4) == 0;
                let transparent = next(4) == 0;
                if next(2) == 0 {
                    // References of one block: one set of triangles, an
                    // instance apiece.
                    let index_count = 3 * (1 + next(4));
                    for instance in 0..1 + next(30) {
                        ranges
                            .entry(handle(id))
                            .or_default()
                            .push(MeshResidentRange {
                                chunk,
                                dynamic,
                                index_start: 0,
                                index_count,
                                transparent,
                                instance_start: instance,
                                instance_count: 1,
                            });
                        id += 1;
                    }
                } else {
                    // Solids of their own: one instance, the triangles of each
                    // after the last, now and then with a gap or in two parts.
                    let mut index_start = 0;
                    for _ in 0..1 + next(30) {
                        let index_count = 3 * (1 + next(4));
                        let solid = ranges.entry(handle(id)).or_default();
                        solid.push(MeshResidentRange {
                            chunk,
                            dynamic,
                            index_start,
                            index_count,
                            transparent,
                            instance_start: 0,
                            instance_count: 1,
                        });
                        index_start += index_count;
                        match next(8) {
                            0 => index_start += 3,
                            1 => {
                                solid.push(MeshResidentRange {
                                    chunk,
                                    dynamic,
                                    index_start,
                                    index_count: 3,
                                    transparent: !transparent,
                                    instance_start: 0,
                                    instance_count: 1,
                                });
                            }
                            // The same range listed twice is drawn twice.
                            2 => {
                                let again = solid[0];
                                solid.push(again);
                            }
                            _ => {}
                        }
                        id += 1;
                    }
                }
            }
            let mut selected = Handles::default();
            let mut hovered = Handles::default();
            let mut edge_handles = Handles::default();
            let picked_share = 1 + next(4);
            for index in 0..id {
                if next(4) < picked_share {
                    selected.insert(handle(index));
                }
                if next(6) == 0 {
                    hovered.insert(handle(index));
                }
                if next(12) == 0 {
                    edge_handles.insert(handle(index));
                }
            }

            let draws =
                Pipeline::mesh_highlight_draws(&ranges, &selected, &hovered, &edge_handles);
            let separate = one_draw_for_each_range(&ranges, &selected, &hovered, &edge_handles);

            assert_eq!(drawn(&draws), drawn(&separate), "round {round}");
            assert!(draws.len() <= separate.len(), "round {round}");
            joined_any |= draws.len() < separate.len();
        }
        assert!(joined_any, "the rounds must reach draws that are joined");
    }
