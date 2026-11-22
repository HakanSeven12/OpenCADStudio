//! Where each reference's analytical instances live in the retained GPU
//! buffers, so a move can overwrite them in place.
//!
//! Blocks, circles and ellipses are uploaded as whole buffers built from every
//! wire in the drawing. Moving one object leaves how many instances it has
//! unchanged, only their values, so instead of rebuilding the buffers the new
//! values are written over the old ones. An object that is hidden meanwhile
//! gets a depth no entity has, which the draw-order shader function sends
//! outside the clip volume; writing its real values back shows it again.
//! Anything that does not fit this (a different number of instances, another
//! definition, another draw order) is reported as needing the full rebuild.

use super::circle_gpu::CircleInstance;
use super::ellipse_gpu::EllipseInstance;
use super::gpu_upload::{rebuild_is_cheaper, write_all, WriteBatch};
use super::wire_gpu::BlockWireInstance;
use document::Handle;
use iced::wgpu;
use rustc_hash::FxHashMap;

/// A depth no entity has. `draw_order.wgsl` treats anything above `1.0e30`
/// as hidden.
pub(crate) const HIDDEN_DEPTH: f32 = 3.0e38;

/// `OCS_ANALYTIC_PATCH=0` turns in-place patching off and keeps the full
/// rebuild on every edit, to compare the two on screen.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OCS_ANALYTIC_PATCH").map_or(true, |value| value != "0"))
}

/// What a wire looks like apart from its shape, part by part, so a patch that
/// cannot be applied can say which part differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WireLook {
    pub color: [u32; 4],
    pub selected: bool,
    pub weight: u32,
    /// Hash of the dash pattern and the other line-style flags.
    pub dashes: u64,
}

/// One placement in a block arena's instance buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BlockSlot {
    /// Which arena, in the order the uploaded batches name them.
    pub chunk: u32,
    /// Instance index within that arena's instance buffer.
    pub index: u32,
    pub source_id: u64,
    pub mesh_edge: bool,
    /// Placement the definition's geometry was emitted at; instances are
    /// stored relative to it.
    pub base: [f64; 3],
    /// Which entry of [`AnalyticSlots::shapes`] holds the geometry this
    /// placement is drawn with.
    pub group: u32,
    /// The wire's shape and look. A definition draws with one set of
    /// constants, so a changed look cannot be patched into a single instance.
    pub style: WireLook,
}

/// One circle or ellipse in its (depth-sorted) instance buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CurveSlot {
    pub index: u32,
    /// The draw depth the instance was sorted by; a patch that changes it
    /// would leave the buffer out of order.
    pub depth_bits: u32,
}

/// Every analytical instance one reference owns.
#[derive(Default, Debug, Clone, PartialEq)]
pub(crate) struct HandleSlots {
    pub blocks: Vec<BlockSlot>,
    pub circles: Vec<CurveSlot>,
    pub ellipses: Vec<CurveSlot>,
}

impl HandleSlots {
    fn is_empty(&self) -> bool {
        self.blocks.is_empty() && self.circles.is_empty() && self.ellipses.is_empty()
    }
}

/// The position slots of one retained upload, with the buffers they index.
pub(crate) struct AnalyticSlots {
    pub by_handle: FxHashMap<Handle, HandleSlots>,
    pub block_buffers: Vec<wgpu::Buffer>,
    /// The block-local geometry each group of placements is drawn with.
    pub shapes: Vec<Vec<[f32; 3]>>,
    pub circle_buffer: Option<wgpu::Buffer>,
    pub ellipse_buffer: Option<wgpu::Buffer>,
    /// Identity of the GPU lists these slots describe; a slot that finds the
    /// lists replaced behind its back must not write into them.
    pub blocks_id: usize,
    pub circles_id: usize,
    pub ellipses_id: usize,
    /// Draw-order labels the recorded depths belong to.
    pub depth_generation: u64,
}

/// What one reference now contributes, in the order the build gave it.
pub(crate) struct NewBlock {
    pub source_id: u64,
    pub mesh_edge: bool,
    /// World placement of the instance (translation plus the copy's offset).
    pub position: [f64; 3],
    pub depth: f32,
    pub style: WireLook,
    /// The wire's points relative to its own translation: the block-local
    /// shape, the same for every placement of one definition.
    pub points: Vec<[f32; 3]>,
    /// Largest absolute coordinate, which sets how far f32 rounding can move a
    /// relative point.
    pub magnitude: f32,
}

pub(crate) struct Contribution<'a> {
    pub blocks: &'a [NewBlock],
    pub circles: &'a [CircleInstance],
    pub ellipses: &'a [EllipseInstance],
}

impl Contribution<'_> {
    fn is_empty(&self) -> bool {
        self.blocks.is_empty() && self.circles.is_empty() && self.ellipses.is_empty()
    }
}

/// One write into a retained buffer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Edit {
    Block {
        chunk: u32,
        index: u32,
        value: BlockWireInstance,
    },
    Circle {
        index: u32,
        value: CircleInstance,
    },
    Ellipse {
        index: u32,
        value: EllipseInstance,
    },
    /// Only the depth field: enough to hide an instance.
    HideCircle {
        index: u32,
    },
    HideEllipse {
        index: u32,
    },
}

#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    /// Nothing about this reference's analytical instances changed shape or
    /// existence.
    Unchanged,
    Edits(Vec<Edit>),
    /// Cannot be expressed as in-place writes, and why not.
    Rebuild(&'static str),
}

/// The instance record for a placement relative to its definition's base,
/// split into the high and low parts the shaders recombine.
pub(crate) fn block_instance_at(
    position: [f64; 3],
    base: [f64; 3],
    depth: f32,
) -> BlockWireInstance {
    let delta = [
        position[0] - base[0],
        position[1] - base[1],
        position[2] - base[2],
    ];
    let high = delta.map(|value| value as f32);
    BlockWireInstance {
        translation: high,
        translation_low: [
            (delta[0] - high[0] as f64) as f32,
            (delta[1] - high[1] as f64) as f32,
            (delta[2] - high[2] as f64) as f32,
        ],
        depth: [depth, 0.0],
    }
}

fn hidden_block() -> BlockWireInstance {
    BlockWireInstance {
        translation: [0.0; 3],
        translation_low: [0.0; 3],
        depth: [HIDDEN_DEPTH, 0.0],
    }
}

/// Whether two block-local point lists are the same shape, allowing for the
/// rounding a placement's f32 coordinates pick up when they are moved.
pub(crate) fn same_shape(recorded: &[[f32; 3]], now: &[[f32; 3]], magnitude: f32) -> bool {
    if recorded.len() != now.len() {
        return false;
    }
    let tolerance = 1.0e-4 + 4.0e-7 * magnitude;
    // A polyline of several paths separates them with NaN points; two NaNs in
    // the same place are the same separator.
    let close = |a: f32, b: f32| (a.is_nan() && b.is_nan()) || (a - b).abs() <= tolerance;
    recorded
        .iter()
        .zip(now)
        .all(|(a, b)| close(a[0], b[0]) && close(a[1], b[1]) && close(a[2], b[2]))
}

/// Decide how a reference's changed contribution is written over its old one.
pub(crate) fn plan_reference(
    old: Option<&HandleSlots>,
    new: &Contribution<'_>,
    shapes: &[Vec<[f32; 3]>],
) -> Outcome {
    let Some(old) = old.filter(|old| !old.is_empty()) else {
        // Nothing recorded: only fine while nothing is contributed now.
        return if new.is_empty() {
            Outcome::Unchanged
        } else {
            Outcome::Rebuild("a reference gained analytical instances")
        };
    };
    if new.is_empty() {
        // Hidden or erased: keep the slots so that showing it again lands on
        // the same instances.
        let mut edits =
            Vec::with_capacity(old.blocks.len() + old.circles.len() + old.ellipses.len());
        edits.extend(old.blocks.iter().map(|slot| Edit::Block {
            chunk: slot.chunk,
            index: slot.index,
            value: hidden_block(),
        }));
        edits.extend(
            old.circles
                .iter()
                .map(|slot| Edit::HideCircle { index: slot.index }),
        );
        edits.extend(
            old.ellipses
                .iter()
                .map(|slot| Edit::HideEllipse { index: slot.index }),
        );
        return Outcome::Edits(edits);
    }
    if old.blocks.len() != new.blocks.len() {
        return Outcome::Rebuild("the number of block placements changed");
    }
    if old.circles.len() != new.circles.len() {
        return Outcome::Rebuild("the number of circles changed");
    }
    if old.ellipses.len() != new.ellipses.len() {
        return Outcome::Rebuild("the number of ellipses changed");
    }
    let mut edits = Vec::with_capacity(new.blocks.len() + new.circles.len() + new.ellipses.len());
    let mut used = vec![false; old.blocks.len()];
    for (position, block) in new.blocks.iter().enumerate() {
        // A placement usually lands in the slot at its own position. When the
        // wires came back in another order, any free slot with the same shape
        // and look draws it the same way, so it takes that one instead.
        let fits = |index: usize| -> bool {
            let slot = &old.blocks[index];
            !used[index]
                && slot.mesh_edge == block.mesh_edge
                && slot.style == block.style
                && shapes
                    .get(slot.group as usize)
                    .is_some_and(|shape| same_shape(shape, &block.points, block.magnitude))
        };
        let found = if fits(position) {
            Some(position)
        } else {
            (0..old.blocks.len()).find(|&index| fits(index))
        };
        let Some(index) = found else {
            if crate::perf::enabled() {
                let slot = &old.blocks[position];
                let shape = shapes.get(slot.group as usize);
                let first = shape.and_then(|shape| {
                    shape
                        .iter()
                        .zip(&block.points)
                        .position(|(a, b)| (0..3).any(|axis| (a[axis] - b[axis]).abs() > 1.0e-4))
                });
                crate::perf_record!(
                    "[perf] analytic-shape position={} recorded={:?} now={} magnitude={:.1} \
look_equal={} kind_equal={} first_difference={:?} recorded_point={:?} now_point={:?}",
                    position,
                    shape.map(|shape| shape.len()),
                    block.points.len(),
                    block.magnitude,
                    slot.style == block.style,
                    slot.mesh_edge == block.mesh_edge,
                    first,
                    first.and_then(|i| shape.map(|shape| shape[i])),
                    first.map(|i| block.points[i]),
                );
            }
            return Outcome::Rebuild("no free slot has this placement's shape and look");
        };
        used[index] = true;
        let slot = &old.blocks[index];
        edits.push(Edit::Block {
            chunk: slot.chunk,
            index: slot.index,
            value: block_instance_at(block.position, slot.base, block.depth),
        });
    }
    // Circles and ellipses are written whole, so any free slot of the same
    // draw order (which keeps the buffer sorted) will do.
    let free_slots = |slots: &[CurveSlot]| {
        let mut free: FxHashMap<u32, Vec<usize>> = FxHashMap::default();
        for (index, slot) in slots.iter().enumerate().rev() {
            free.entry(slot.depth_bits).or_default().push(index);
        }
        free
    };
    let mut free = free_slots(&old.circles);
    for circle in new.circles {
        let Some(index) = free
            .get_mut(&circle.params[2].to_bits())
            .and_then(|slots| slots.pop())
        else {
            return Outcome::Rebuild("a circle changed draw order");
        };
        edits.push(Edit::Circle {
            index: old.circles[index].index,
            value: *circle,
        });
    }
    let mut free = free_slots(&old.ellipses);
    for ellipse in new.ellipses {
        let Some(index) = free
            .get_mut(&ellipse.params[2].to_bits())
            .and_then(|slots| slots.pop())
        else {
            return Outcome::Rebuild("an ellipse changed draw order");
        };
        edits.push(Edit::Ellipse {
            index: old.ellipses[index].index,
            value: *ellipse,
        });
    }
    Outcome::Edits(edits)
}

/// The writes of a patch, buffer by buffer.
#[derive(Default)]
pub(crate) struct PlannedWrites {
    /// One per block arena, in the order the uploaded batches name them.
    pub blocks: Vec<WriteBatch>,
    pub circles: WriteBatch,
    pub ellipses: WriteBatch,
}

/// Sort the edits into the buffers they write, `chunks` block arenas among
/// them.
pub(crate) fn plan_writes(edits: &[Edit], chunks: usize) -> PlannedWrites {
    use std::mem::{offset_of, size_of};
    let mut planned = PlannedWrites::default();
    planned.blocks.resize_with(chunks, WriteBatch::default);
    for edit in edits {
        match edit {
            Edit::Block {
                chunk,
                index,
                value,
            } => {
                if let Some(blocks) = planned.blocks.get_mut(*chunk as usize) {
                    blocks.push(
                        *index as u64 * size_of::<BlockWireInstance>() as u64,
                        bytemuck::bytes_of(value),
                    );
                }
            }
            Edit::Circle { index, value } => planned.circles.push(
                *index as u64 * size_of::<CircleInstance>() as u64,
                bytemuck::bytes_of(value),
            ),
            Edit::Ellipse { index, value } => planned.ellipses.push(
                *index as u64 * size_of::<EllipseInstance>() as u64,
                bytemuck::bytes_of(value),
            ),
            Edit::HideCircle { index } => planned.circles.push(
                *index as u64 * size_of::<CircleInstance>() as u64
                    + offset_of!(CircleInstance, params) as u64
                    + 2 * size_of::<f32>() as u64,
                bytemuck::bytes_of(&HIDDEN_DEPTH),
            ),
            Edit::HideEllipse { index } => planned.ellipses.push(
                *index as u64 * size_of::<EllipseInstance>() as u64
                    + offset_of!(EllipseInstance, params) as u64
                    + 2 * size_of::<f32>() as u64,
                bytemuck::bytes_of(&HIDDEN_DEPTH),
            ),
        }
    }
    planned
}

impl AnalyticSlots {
    /// Write the edits into the buffers the slots describe. `Err` names why
    /// the full rebuild is the better way, and nothing was written.
    pub(crate) fn apply(&self, queue: &wgpu::Queue, edits: &[Edit]) -> Result<(), &'static str> {
        use std::mem::size_of;
        let planned = plan_writes(edits, self.block_buffers.len());
        let block = size_of::<BlockWireInstance>();
        let mut targets: Vec<(&wgpu::Buffer, &WriteBatch, usize)> = self
            .block_buffers
            .iter()
            .zip(&planned.blocks)
            .map(|(buffer, batch)| (buffer, batch, block))
            .collect();
        if let Some(buffer) = &self.circle_buffer {
            targets.push((buffer, &planned.circles, size_of::<CircleInstance>()));
        }
        if let Some(buffer) = &self.ellipse_buffer {
            targets.push((buffer, &planned.ellipses, size_of::<EllipseInstance>()));
        }
        let mut writes = Vec::with_capacity(targets.len());
        for (buffer, batch, element) in targets {
            let pieces = batch.writes();
            if rebuild_is_cheaper(pieces.len(), buffer.size() / element as u64) {
                return Err("more separate writes than the rebuild costs");
            }
            writes.push((buffer, pieces));
        }
        for (buffer, pieces) in &writes {
            write_all(queue, buffer, pieces);
        }
        Ok(())
    }
}

/// Sort by a depth, stable, and report where every original item landed.
pub(crate) fn sort_by_depth<T: Copy + Send + Sync>(
    items: &[T],
    depth: impl Fn(&T) -> f32 + Sync,
) -> (Vec<T>, Vec<u32>) {
    let mut order: Vec<u32> = (0..items.len() as u32).collect();
    let compare =
        |a: &u32, b: &u32| depth(&items[*a as usize]).total_cmp(&depth(&items[*b as usize]));
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        order.par_sort_by(compare);
    }
    #[cfg(target_arch = "wasm32")]
    order.sort_by(compare);
    let sorted = order.iter().map(|&index| items[index as usize]).collect();
    let mut position = vec![0u32; items.len()];
    for (sorted_index, &original) in order.iter().enumerate() {
        position[original as usize] = sorted_index as u32;
    }
    (sorted, position)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> WireLook {
        WireLook {
            color: [0; 4],
            selected: false,
            weight: 0,
            dashes: 0,
        }
    }

    /// One group whose shape is a unit segment.
    fn shapes() -> Vec<Vec<[f32; 3]>> {
        vec![vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]]
    }

    fn plan(old: Option<&HandleSlots>, new: &Contribution<'_>) -> Outcome {
        plan_reference(old, new, &shapes())
    }

    fn slots(blocks: usize, circles: usize) -> HandleSlots {
        HandleSlots {
            blocks: (0..blocks)
                .map(|i| BlockSlot {
                    chunk: 0,
                    index: 10 + i as u32,
                    source_id: 7,
                    mesh_edge: false,
                    base: [100.0, 200.0, 0.0],
                    group: 0,
                    style: look(),
                })
                .collect(),
            circles: (0..circles)
                .map(|i| CurveSlot {
                    index: 50 + i as u32,
                    depth_bits: 0.5f32.to_bits(),
                })
                .collect(),
            ellipses: Vec::new(),
        }
    }

    fn block(source_id: u64, x: f64) -> NewBlock {
        NewBlock {
            source_id,
            mesh_edge: false,
            position: [x, 200.0, 0.0],
            depth: 0.25,
            style: look(),
            // Definition 7 is the recorded unit segment; any other is longer.
            points: vec![
                [0.0, 0.0, 0.0],
                [if source_id == 7 { 1.0 } else { 2.0 }, 0.0, 0.0],
            ],
            magnitude: 100.0,
        }
    }

    fn circle(depth: f32) -> CircleInstance {
        let mut circle = <CircleInstance as bytemuck::Zeroable>::zeroed();
        circle.params[2] = depth;
        circle
    }

    #[test]
    fn a_move_of_the_same_shape_rewrites_the_recorded_slots() {
        let old = slots(2, 1);
        let blocks = [block(7, 110.0), block(7, 111.0)];
        let circles = [circle(0.5)];
        let outcome = plan(
            Some(&old),
            &Contribution {
                blocks: &blocks,
                circles: &circles,
                ellipses: &[],
            },
        );
        let Outcome::Edits(edits) = outcome else {
            panic!("a same-shape move patches in place");
        };
        assert_eq!(edits.len(), 3);
        // The instance is stored relative to the definition's base.
        match &edits[0] {
            Edit::Block {
                chunk,
                index,
                value,
            } => {
                assert_eq!((*chunk, *index), (0, 10));
                assert_eq!(value.translation, [10.0, 0.0, 0.0]);
                assert_eq!(value.depth, [0.25, 0.0]);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(&edits[2], Edit::Circle { index: 50, .. }));
    }

    #[test]
    fn hiding_marks_every_slot_and_showing_restores_the_same_ones() {
        let old = slots(2, 1);
        let hidden = plan(
            Some(&old),
            &Contribution {
                blocks: &[],
                circles: &[],
                ellipses: &[],
            },
        );
        let Outcome::Edits(edits) = hidden else {
            panic!("hiding patches in place");
        };
        assert_eq!(edits.len(), 3);
        assert!(matches!(
            &edits[0],
            Edit::Block { value, .. } if value.depth[0] == HIDDEN_DEPTH
        ));
        assert!(matches!(&edits[2], Edit::HideCircle { index: 50 }));
        // Hiding twice in a row writes the same thing again.
        let again = plan(
            Some(&old),
            &Contribution {
                blocks: &[],
                circles: &[],
                ellipses: &[],
            },
        );
        assert_eq!(again, Outcome::Edits(edits));
        // Showing it again lands on the very same slots.
        let blocks = [block(7, 120.0), block(7, 121.0)];
        let circles = [circle(0.5)];
        let shown = plan(
            Some(&old),
            &Contribution {
                blocks: &blocks,
                circles: &circles,
                ellipses: &[],
            },
        );
        assert!(matches!(shown, Outcome::Edits(ref e) if e.len() == 3));
    }

    #[test]
    fn a_change_of_shape_needs_the_full_rebuild() {
        let old = slots(2, 1);
        let one_block = [block(7, 110.0)];
        let circles = [circle(0.5)];
        // Fewer instances than recorded.
        assert_eq!(
            plan(
                Some(&old),
                &Contribution {
                    blocks: &one_block,
                    circles: &circles,
                    ellipses: &[],
                },
            ),
            Outcome::Rebuild("the number of block placements changed")
        );
        // Another definition in a slot.
        let blocks = [block(7, 110.0), block(8, 111.0)];
        assert_eq!(
            plan(
                Some(&old),
                &Contribution {
                    blocks: &blocks,
                    circles: &circles,
                    ellipses: &[],
                },
            ),
            Outcome::Rebuild("no free slot has this placement's shape and look")
        );
        // A circle whose draw order moved would sit out of order.
        let blocks = [block(7, 110.0), block(7, 111.0)];
        let moved_depth = [circle(0.75)];
        assert_eq!(
            plan(
                Some(&old),
                &Contribution {
                    blocks: &blocks,
                    circles: &moved_depth,
                    ellipses: &[],
                },
            ),
            Outcome::Rebuild("a circle changed draw order")
        );
    }

    #[test]
    fn a_reference_with_nothing_recorded_only_stays_unchanged_while_it_contributes_nothing() {
        let nothing = Contribution {
            blocks: &[],
            circles: &[],
            ellipses: &[],
        };
        assert_eq!(plan(None, &nothing), Outcome::Unchanged);
        let blocks = [block(7, 1.0)];
        assert_eq!(
            plan(
                None,
                &Contribution {
                    blocks: &blocks,
                    circles: &[],
                    ellipses: &[],
                },
            ),
            Outcome::Rebuild("a reference gained analytical instances")
        );
    }

    #[test]
    fn placements_that_return_in_another_order_take_the_slot_of_their_shape() {
        let two_shapes = vec![
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        ];
        let slot = |index: u32, group: u32| BlockSlot {
            chunk: 0,
            index,
            source_id: 1,
            mesh_edge: false,
            base: [0.0; 3],
            group,
            style: look(),
        };
        let old = HandleSlots {
            blocks: vec![slot(10, 0), slot(11, 1)],
            circles: Vec::new(),
            ellipses: Vec::new(),
        };
        // The long placement comes first this time.
        let blocks = [block(8, 5.0), block(7, 6.0)];
        let outcome = plan_reference(
            Some(&old),
            &Contribution {
                blocks: &blocks,
                circles: &[],
                ellipses: &[],
            },
            &two_shapes,
        );
        let Outcome::Edits(edits) = outcome else {
            panic!("a reordered return still patches in place");
        };
        // The long shape (group 1) goes to its own slot, the short one to its.
        match (&edits[0], &edits[1]) {
            (
                Edit::Block {
                    index: first,
                    value: a,
                    ..
                },
                Edit::Block {
                    index: second,
                    value: b,
                    ..
                },
            ) => {
                assert_eq!((*first, *second), (11, 10));
                assert_eq!(a.translation[0], 5.0);
                assert_eq!(b.translation[0], 6.0);
            }
            other => panic!("unexpected {other:?}"),
        }
        // Two long placements cannot both fit the one slot of that shape.
        let blocks = [block(8, 5.0), block(8, 6.0)];
        assert!(matches!(
            plan_reference(
                Some(&old),
                &Contribution {
                    blocks: &blocks,
                    circles: &[],
                    ellipses: &[],
                },
                &two_shapes,
            ),
            Outcome::Rebuild(_)
        ));
    }
    #[test]
    fn shapes_match_through_f32_rounding_but_not_through_a_real_difference() {
        let recorded = vec![[0.0, 0.0, 0.0], [1.0, 0.5, 0.0]];
        // A moved placement picks up a few ulp of rounding.
        let rounded = vec![[0.000001, 0.0, 0.0], [1.000003, 0.499998, 0.0]];
        assert!(same_shape(&recorded, &rounded, 100.0));
        let longer = vec![[0.0, 0.0, 0.0], [1.5, 0.5, 0.0]];
        assert!(!same_shape(&recorded, &longer, 100.0));
        assert!(!same_shape(&recorded, &recorded[..1], 100.0));
        // Separator points between the paths of one polyline are NaN.
        let with_separator = vec![[0.0, 0.0, 0.0], [f32::NAN; 3], [1.0, 0.5, 0.0]];
        assert!(same_shape(&with_separator, &with_separator.clone(), 100.0));
        let separator_elsewhere = vec![[0.0, 0.0, 0.0], [1.0, 0.5, 0.0], [f32::NAN; 3]];
        assert!(!same_shape(&with_separator, &separator_elsewhere, 100.0));
    }

    fn spans(writes: &[(u64, Vec<u8>)], element: usize) -> Vec<(u64, usize)> {
        writes
            .iter()
            .map(|(offset, bytes)| (*offset / element as u64, bytes.len() / element))
            .collect()
    }

    /// Moving every circle of a drawing rewrites its whole buffer, in the
    /// order the edit names them.
    #[test]
    fn curves_side_by_side_are_one_write() {
        let circle_at = |index| Edit::Circle {
            index,
            value: circle(0.5),
        };
        let ellipse_at = |index| Edit::Ellipse {
            index,
            value: bytemuck::Zeroable::zeroed(),
        };
        let edits = [
            circle_at(6),
            ellipse_at(2),
            circle_at(5),
            circle_at(7),
            ellipse_at(1),
            circle_at(20),
        ];
        let planned = plan_writes(&edits, 0);
        assert_eq!(
            spans(
                &planned.circles.writes(),
                std::mem::size_of::<CircleInstance>()
            ),
            vec![(5, 3), (20, 1)]
        );
        assert_eq!(
            spans(
                &planned.ellipses.writes(),
                std::mem::size_of::<EllipseInstance>()
            ),
            vec![(1, 2)]
        );
    }

    /// Erasing or freezing objects hides their curves. A hidden one is written
    /// whole, so a run of them is one write too.
    #[test]
    fn hidden_curves_side_by_side_are_one_write() {
        let edits = [
            Edit::HideCircle { index: 4 },
            Edit::HideEllipse { index: 9 },
            Edit::HideCircle { index: 3 },
            Edit::HideEllipse { index: 8 },
            Edit::HideCircle { index: 5 },
        ];
        let planned = plan_writes(&edits, 0);
        let circles = planned.circles.writes();
        assert_eq!(
            spans(&circles, std::mem::size_of::<CircleInstance>()),
            vec![(3, 3)]
        );
        let hidden: Vec<CircleInstance> = bytemuck::pod_collect_to_vec(&circles[0].1);
        assert!(hidden.iter().all(|circle| circle.params[2] == HIDDEN_DEPTH));
        let ellipses = planned.ellipses.writes();
        assert_eq!(
            spans(&ellipses, std::mem::size_of::<EllipseInstance>()),
            vec![(8, 2)]
        );
        let hidden: Vec<EllipseInstance> = bytemuck::pod_collect_to_vec(&ellipses[0].1);
        assert!(hidden
            .iter()
            .all(|ellipse| ellipse.params[2] == HIDDEN_DEPTH));
    }

    #[test]
    fn block_placements_are_joined_within_their_own_arena() {
        let block_at = |chunk, index| Edit::Block {
            chunk,
            index,
            value: block_instance_at([1.0, 2.0, 0.0], [0.0; 3], 0.5),
        };
        let edits = [
            block_at(1, 11),
            block_at(0, 10),
            block_at(1, 10),
            block_at(0, 12),
            // An arena the slots do not hold is skipped, as before.
            block_at(7, 0),
        ];
        let planned = plan_writes(&edits, 2);
        let size = std::mem::size_of::<BlockWireInstance>();
        assert_eq!(
            spans(&planned.blocks[0].writes(), size),
            vec![(10, 1), (12, 1)]
        );
        assert_eq!(spans(&planned.blocks[1].writes(), size), vec![(10, 2)]);
    }

    #[test]
    fn sorting_reports_where_each_item_landed() {
        let items = [0.5f32, 0.1, 0.5, 0.3, 0.1];
        let (sorted, position) = sort_by_depth(&items, |value| *value);
        assert_eq!(sorted, vec![0.1, 0.1, 0.3, 0.5, 0.5]);
        // Equal depths keep their original relative order (a stable sort).
        assert_eq!(position, vec![3, 0, 4, 2, 1]);
        for (original, &landed) in position.iter().enumerate() {
            assert_eq!(sorted[landed as usize], items[original]);
        }
    }

    #[test]
    fn block_instance_matches_the_split_the_build_uses() {
        let value = block_instance_at([123456.789, 0.25, 0.0], [100000.0, 0.0, 0.0], 0.1);
        let recombined = value.translation[0] as f64 + value.translation_low[0] as f64;
        assert!((recombined - 23456.789).abs() < 1e-6);
        assert_eq!(value.depth, [0.1, 0.0]);
    }
}
