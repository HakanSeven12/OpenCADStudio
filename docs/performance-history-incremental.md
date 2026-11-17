Incremental history accounting removes the repeated retained-history census from ordinary edits. It updates reference counts only for entries added, removed or changed, while keeping the payload estimator and shared-image/raw-record deduplication. The measured geometry savings are unchanged.

The before arm copies both the full-scan counter and trimming policy from `ea48570fc`; the copies were checked against that commit. Both arms use current dependencies, release optimization and the Windows application's mimalloc allocator. No counting allocator is installed. Hardware: Intel Core Ultra 5 245KF, Windows 11 Pro 26200, rustc 1.94.1.

[Versioned measurements, raw samples and source hashes](../benches/results/history-incremental-2026-11-17.json) record the tested working copy. Source hashes normalize CRLF to LF for comparison with committed Git blobs. This report follows the [audit that identified the regression](performance-memory-audit.md). Measurements were rerun on 17 November after fetching and rebasing onto `origin/main` at `f92aeb3ad`, preserving the two local save/audit commits.

Each edit case has 100 alternating paired samples; the first six cases have three warmup calls per arm. Each new entry shares its before-images with the preceding history entry and owns new after-images. Timing includes pushing the entry, updating the incremental census, invalidating redo and trimming. Fixture cloning, initial census construction, validation and deferred destruction are outside both timed arms. Stack headroom is reserved equally before timing. Cached counts and payload bytes are checked against a fresh census after every measured incremental edit.

| Recording operation | Median before → after (µs) | p95 before → after (µs) | Median change | Paired median delta 95% interval (µs) |
|---|---:|---:|---:|---:|
| 256 × 1 retained; record 1 entity | 54.400 → 0.800 | 73.100 → 1.300 | -98.53% | [-55.250, -52.750] |
| 128 × 100 retained; record 100 entities | 495.700 → 1.800 | 662.100 → 2.600 | -99.64% | [-518.700, -466.600] |
| 256 × 1,000 shared versions; record 1,000 entities | 1532.150 → 53.350 | 1691.000 → 74.600 | -96.52% | [-1488.600, -1455.500] |
| 64 × 1,000 distinct versions; record 1,000 entities | 5209.750 → 31.000 | 6251.600 → 55.800 | -99.40% | [-5402.200, -4950.150] |
| 64 × 1,000 distinct versions; record 1 entity | 5269.000 → 1.000 | 6231.600 → 1.300 | -99.98% | [-5431.500, -5205.050] |
| 256 × 1 retained; record 1, unlimited bytes | 1.000 → 1.000 | 1.400 → 1.300 | +0.00% | [0.000, 0.100] |
| 32 undo + 32 redo × 100; record 100, clear redo | 143.250 → 39.700 | 181.500 → 44.100 | -72.29% | [-109.600, -96.500] |

A check with no new entry on the 64 × 1,000 distinct-version fixture costs 5156.300 → 0.300 µs median. This cached check is separate from the full recording costs above. The earlier general history audit still measures cold census construction; this new audit measures recording work with an initialized cache.

For the same dense fixture, a stack/accounting-only undo-and-redo roundtrip costs 0.022 → 0.041 µs median and 0.023 → 0.058 µs p95 of sample averages. Samples average 1,000 roundtrips. Replay preserves entity reference counts across stack transfers and refreshes snapshot overhead because structure replay can swap stored state. This is not complete scene replay or frame latency, and the small bookkeeping increase remains a cost.

The retention probe records 160 populated polylines, each with 100,000 vertices, enforcing 256 entries and 512 MiB after each edit. It runs each strategy sequentially three times and synchronously drops discarded snapshots before measuring. All three runs agree exactly. These are unique retained vertex-buffer capacities and budget estimates, not RSS or an allocator census.

| Policy | Retained steps | Retained vertex bytes | Vertex MiB | Persistent census heap allowance (bytes) | Payload/snapshot + census budget estimate (bytes) |
|---|---:|---:|---:|---:|---:|
| Original coarse estimate | 160 | 768,000,000 | 732.422 | 0 | 769,107,840 |
| Before: full scan | 111 | 532,800,000 | 508.118 | 0 | 533,568,564 |
| After: incremental | 111 | 532,800,000 | 508.118 | 11,200 | 533,579,764 |

The incremental version retains the same 111 steps and 532,800,000 vertex bytes as the full-scan version, preserving the 30.625% reduction from the original coarse estimate. Its 11,200-byte conservative heap allowance is charged to the byte budget; the total stays below 536,870,912 bytes. The optional census also adds 88 bytes of inline state. It stores addresses, costs and reference counts, without owning entity Arcs. Table capacity is tracked across tombstone deletion and sparse tables are rebuilt; an unlimited byte budget allocates no census tables.

Read-only stack wrappers prevent callers from mutating history without accounting. A mutable-entry guard unregisters and re-registers only the edited snapshot, including early returns. Live after-image capture, style merging, block-editor coalescing, redo clearing and discards use these paths. Five new correctness tests cover deterministic mixed operations with shared raw records, growing live payloads, unlimited-budget transitions, weak-reference payload release, sparse-table compaction and allocation high-water accounting.

Validation: 73 related release functional checks passed, including the five new tests and existing history, command undo/redo sweep, style merge, layer isolation, block save coalescing, live entity and grip undo tests, plus save preparation, xref save state and autosave recovery checks. Both ignored release audit tests passed. Native test compilation and the WebAssembly library check passed. Existing compiler warnings remain.

Reproduce from the repository root with PERF unset:

```sh
cargo test --locked --release --features performance-audit --lib app::history -- --nocapture --test-threads=1
cargo test --locked --release --features performance-audit --lib performance_audit_history_incremental -- --ignored --nocapture --test-threads=1
cargo check --locked --target wasm32-unknown-unknown --lib
```

The audit writes `target/performance-audit/history-incremental-latency.json` and `history-incremental-retention.json`. p95 uses nearest rank; paired median-delta intervals use 4,000 deterministic bootstrap resamples and are pointwise conditional on the fixtures. Initial census construction remains linear when enabling byte accounting on an existing history. Budget estimates retain the existing approximations for uncommon entity/structure payloads. These measurements exclude full command/scene work, browser input/frame latency, allocator overhead, RSS and deferred-drop backlog; results do not establish zero impact across other drawings or hardware.
