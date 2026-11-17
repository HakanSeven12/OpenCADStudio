GPU memory collection now borrows resource handles and visits each identical immutable wire batch once per census. The eight-viewport stress case fell from 334.425 to 5.635 µs per call (98.32% less CPU time). All logical byte categories and wire/material record layouts match the before implementation. Earlier history and save memory gains were reproduced.

[Versioned evidence and raw samples](../benches/results/gpu-collection-2026-11-17.json) include executable hashes, source fingerprints, full category snapshots, process-paired samples, component timings and memory-preservation results. Run date: 17 November 2026. Core Ultra 5 245KF, Windows 11 Pro 26200, NVIDIA RTX 5070 / Vulkan, rustc 1.94.1. Release timing uses the application's mimalloc allocator without allocation counting.

The old counter cloned a buffer/texture handle before every hash lookup, including duplicates, and scanned identical Arc-backed wire batches again for each slot and cache entry. The new sets store borrowed handles, retaining the same handle-based equality. A second set identifies wire slices by data pointer and length. Borrowing keeps batch storage alive and prevents changes to its wire records during the census; prefixes and full slices have different keys. All sets are local to one collection. Every collection reads current resources, so replacement, cold-cache retention and eviction need no generation/invalidation mechanism.

The renderer comparison alternates ten fresh-process pairs. Both binaries run the unchanged `performance_audit_gpu` fixture on actual wgpu resources, including the real `report_gpu_live` guard/change-detection path. Each process supplies 100 samples averaging ten enabled calls. The headline median is the median of ten process medians; p95 pools the sample averages. The original fixture's complete-counter arm is selected in both binaries, rather than comparing the older partial counter with the new full counter.

| Viewports | Wire ranges/viewport | Median before → after (µs/call) | p95 sample average before → after (µs) | Median change | Paired-process median delta 95% interval (µs) |
|---|---:|---:|---:|---:|---:|
| 1 | 0 | 0.560 → 0.350 | 0.630 → 0.410 | -37.50% | [-0.260, -0.200] |
| 1 | 1,000 | 72.615 → 4.055 | 79.650 → 4.560 | -94.42% | [-68.710, -68.390] |
| 4 | 0 | 2.130 → 1.105 | 2.360 → 1.240 | -48.12% | [-1.040, -1.020] |
| 4 | 1,000 | 181.803 → 4.925 | 193.710 → 5.550 | -97.29% | [-177.188, -176.387] |
| 8 | 0 | 8.375 → 1.945 | 10.160 → 2.090 | -76.78% | [-6.480, -6.280] |
| 8 | 1,000 | 334.425 → 5.635 | 355.490 → 6.360 | -98.32% | [-329.385, -327.067] |

The stress fixture shares 32 instance buffers and one constants buffer across slots and a cold cache. All twenty processes reported identical six category snapshots for the 1/4/8-slot empty/populated cases; the eight-slot populated total remains 154,848 logical bytes, including 135,168 wire bytes. Every before/after pair also matched the reported per-record sizes. PERF-disabled cases average 1,000 guard calls per sample and stayed at the batched measurement floor (about 0.0004 µs); collection is bypassed before any counter is constructed. This does not establish application-level zero overhead or GPU frame performance.

The baseline executable was preserved before editing. It reports revision `06f6aef6d80c-dirty` from the previously tested incremental-history working copy. Before making changes, the complete counter/audit source and all collector/report helper bodies were checked against the optimization base `d47434950` and matched. The after binary reports `d47434950832-dirty`. This provenance is included with both executable hashes. The component audit below independently compares the exact old wire-counting operations against the new ones in one executable/context.

| Wire component fixture (1,000 ranges/slot plus cache alias) | Median before → after (µs) | Change | Reserved key capacity before → after (bytes) |
|---|---:|---:|---:|
| shared_batch_1 | 71.650 → 4.260 | -94.05% | 1,792 → 496 |
| shared_batch_4 | 178.585 → 4.500 | -97.48% | 1,792 → 496 |
| shared_batch_8 | 319.220 → 4.395 | -98.62% | 1,792 → 496 |
| separate_batches_shared_buffers_8 | 318.950 → 29.780 | -90.66% | 1,792 → 672 |
| unique_resources_1 | 84.510 → 15.130 | -82.10% | 57,344 → 14,384 |
| unique_resources_8 | 567.030 → 112.300 | -80.20% | 458,752 → 114,912 |

Component cases use 100 alternating pairs after three warmups, averaging ten calls each. Timings include constructing/dropping the collection tables and, in the old arm, cloning/dropping their handles. GPU allocation/setup is excluded. The separate-batch case shares handles across distinct vectors; the unique-resource cases use different buffers for every range. Both arms return identical buffer-byte totals. The key-capacity figures multiply HashSet capacity by Rust key size and exclude controls, alignment, allocator overhead and headers; they are not heap-peak or VRAM measurements. Buffer keys shrink from 32-byte handles to 8-byte references. Wire records remain 88 bytes and material records remain 48 bytes; existing upload metadata costs remain outside this optimization.

The preservation checks reran both history release audits and the native save allocation probe. Three sequential history runs again retain 111 steps / 532,800,000 vertex bytes, versus the original coarse policy's 160 steps / 768,000,000 bytes. The 30.625% vertex-memory saving is preserved, including the same 11,200-byte census allowance and 88-byte inline state. The history recording implementation and its source fingerprints are unchanged.

The save probe again records extra peak requested System allocation above captured snapshots of 12,583,618 → 2,925,806 bytes for AUTOSAVE (76.749% reduction), and 12,583,618 → 6,672,261 bytes for SAVEALL (46.977%). Eager/lazy output sizes and hashes match. This dev-profile counting probe has no timing claims and excludes GUI/GPU, allocator overhead and browser/worker memory. Save preparation and snapshot scheduling source are unchanged.

Validation: 76 functional checks passed (73 existing save/history/undo/style/layer/block/live-entity/grip checks and three GPU memory checks), plus two GPU audit tests and both history audit tests. The new GPU regression test covers overlapping slices, cloned handles in separate vectors, equal-sized distinct resources, sharing across eight slots/overlays/cache, cold cache, replacement and eviction. Native test compilation and the WebAssembly library check passed. Existing compiler warnings remain.

Reproduce with PERF unset, preserving the before executable before rebuilding:

```powershell
# On the before revision, build and copy the audit test executable shown by Cargo.
cargo test --locked --release --features performance-audit --lib --no-run
# Copy that executable to a separate path, e.g. target/performance-audit/gpu-collection-before.exe.
# On the optimized revision:
cargo test --locked --release --features performance-audit --lib scene::pipeline::gpu_memory -- --include-ignored --nocapture --test-threads=1
python benches/run_gpu_collection_audit.py --before target/performance-audit/gpu-collection-before.exe --after target/release/deps/OpenCADStudio-b99b9f64b6d60d4f.exe --pairs 10
cargo test --locked --release --features performance-audit --lib performance_audit_history_incremental -- --ignored --nocapture --test-threads=1
cargo bench --locked --features performance-audit --profile dev --bench save_batch_memory -- --output target/gpu-collection-save-memory-new.json
cargo check --locked --target wasm32-unknown-unknown --lib
```

Use the actual audit executable path printed by Cargo if its suffix differs. The save probe requires a new output path. The full renderer runner writes `target/performance-audit/gpu-collection-full.json`; the component audit writes `gpu-collection-wire-paths.json`. Pointwise paired intervals use 4,000 deterministic bootstrap resamples. These CPU/bookkeeping measurements exclude GPU draw/present duration, complete upload latency, changed-state formatting and browser frame/input latency. Logical bytes exclude driver heaps/alignment, staging and iced/ViewCube resources. Results are conditional on these fixtures/hardware, not a universal speedup or RSS claim.
