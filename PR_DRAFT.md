# PR Draft: perf(selection): selection preview at scale

> **Hedef:** `HakanSeven12/OpenCADStudio` ← `emrahoksak/OpenCADStudio:perf/selection-preview-scaling`
> **Başlık önerisi:** `perf(selection): keep selection preview smooth with large selections`

## Summary

Selecting many objects made rollover, box-preview (`SELECTIONPREVIEW`) and
selection-commit stutter. The pick algorithms were already spatially indexed —
the cost lived in the *state diff + overlay rebuild* layer that runs on every
pointer move and ~16 ms marquee tick. This series separates the hover path from
the committed-selection path and caches each layer's derived GPU state so a
bump rebuilds only what changed.

## Root causes fixed (with measurements context)

| # | Before | After |
|---|---|---|
| 1 | Every hover / marquee tick re-emitted **the whole committed selection's** highlight wires + glyphs into fresh GPU buffers (`upload_split_highlights` hover half rebuilt everything; O(selected) set-equality per call) | Split caches: selected half keyed on `selection_generation`, hover half on a new `hover_generation`; hover-only bumps reuse the cached selected buffers untouched |
| 2 | `handles_expanded_for_selectable_groups` scanned **all document objects** on every pointer move / tick / rendered frame (PICKSTYLE groups on) | Lazily built snapshot keyed by `(groups_epoch, PICKSTYLE bit)`; bumped by group edits, undo/redo, `PURGE` |
| 3 | `hover_highlight_handles()` re-expanded the hover group + cloned three hover sets **every rendered frame** | Stored raw `hover_expanded`; per-frame cost = set unions only |
| 4 | `ViewportData` built a fresh selected-set clone + hover expansion + `selected_set_sig` **per viewport per frame** (idle CPU scaled with selection) | `cached_viewport_interaction_sets` keyed by the two bit-encoded generations |
| 5 | One dense (8192+ pt) entity in the selection cloned + uploaded **the whole scene's const array** per highlight rebuild (`WireArena::highlight_dense`) | Persistent per-(arena, tint) overlay const buffer; partial `write_buffer` of only highlighted slab ranges |
| 6 | Hovered text re-copied every glyph vertex per tick (no cache for text) | `HoverTextGpuCache` keyed by the render-side hover identity |
| 7 | `SELECTIONPREVIEWLIMIT` was all-or-nothing: >20 000 candidates = **no preview at all** | Cap to the limit (stable ascending-handle subset), applied after group expansion too |
| 8 | Committing a ~186 k-entity selection blocked the click ~112 ms on the Properties rebuild | `queue_properties_refresh`: >`MAX_PROP_AGGREGATE` defers the panel build one frame via the existing frame-pulse tick pattern |

## Commits

1. `perf(selection): split hover generation from selection generation`
2. `perf(selection): cache ViewportData interaction sets per generation`
3. `perf(selection): cap marquee preview to the limit instead of dropping it`
4. `perf(highlight): persistent overlay const buffer for dense-entity highlights`
5. `perf(highlight): cache hover-half text uploads per hover identity`
6. `perf(properties): defer the panel rebuild on huge selection commits`
7. `test(selection): cover the preview performance split`

## Testing

- New `scene::selection_preview_tests` (5 tests) pin: hover bumps hover-only,
  marquee bumps hover-only, group-snapshot invalidation on group edits,
  erase drops stale hover overlay, marquee cap semantics.
- Full `cargo test --lib`: **6250 passed**. 8 pre-existing environment failures
  (`app::text_style_*` font/layout UI tests) verified failing on the base
  commit as well (font-dependent, unrelated to selection).
- GPU-adapter-gated `pipeline::gpu_tests` split-highlight tests updated for the
  new signature.
- Suggested manual check on a large drawing (100 k+ entities): marquee-select
  thousands of objects, then roll over others and drag further boxes — the
  committed selection's highlight no longer re-uploads per tick
  (`[perf] wire-highlight-detail` logs confirm).

## Notes for reviewers

- `selection_generation` semantics unchanged for *committed-selection* events;
  consumers audited: orbit-pivot bounds cache (`navigation.rs` — correct, hover
  never affected pivot), static hatch/face3d tint (`selected_sig`, untouched),
  render signature hash now includes both generations.
- The dense-highlight overlay const buffer is validated per use
  (`consts_len`, instance-buffer size); stale entries are replaced, and
  `forget_cached_keys` clears it with the arenas.
- The marquee cap sorts candidates ascending by handle so the highlighted
  subset is stable across ticks instead of flickering with hash-set order.
