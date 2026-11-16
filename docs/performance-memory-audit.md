The release audit found measurable CPU costs. The memory changes cannot be described as having zero performance impact.

Lazy save preparation improves the tested batch work and reduces its longest synchronous preparation burst. History accounting adds work when recording an edit, reaching 4.40 ms median for the dense distinct-version fixture. Expanded GPU counting costs CPU with PERF enabled; maintained GPU metadata also costs time and host memory with PERF disabled.

Run date: 16 November 2026. Windows 11 Pro 26200, Intel Core Ultra 5 245KF (14 logical processors), NVIDIA RTX 5070 / Vulkan, driver 32.0.16.1088, rustc 1.94.1. The audit uses release optimization and mimalloc, matching the Windows application allocator. No counting allocator is installed. Both arms use the same current dependencies, hardware, allocator and fixture.

Coverage is `f336fbb37` (cache benchmark), `c0ccb87d0` (GPU telemetry), `b9013d919` (history accounting), and `22497ee3b` (browser save scheduling). The cache benchmark changes benchmark executables/documentation; its allocation counter is not installed in the application executable. Audit helpers are opt-in test code and leave the audited runtime algorithms unchanged.

[Versioned measurements and raw samples](../benches/results/memory-performance-audit-2026-11-16.json) include source hashes, raw samples, environment data and pointwise paired bootstrap intervals. Generated reports live in `target/performance-audit/`.

The shared save pipeline uses 30 fresh-process pairs per case with alternating strategy order. Each batch has six drawings. Preparation, serial DXF writing and prepared-copy disposal are timed; capture/setup and output-validation hashing are excluded. All output sizes and hashes match. Browser worker transfers, OPFS and browser frame/input latency remain unmeasured.

| Save case | Median before → after (ms) | p95 before → after (ms) | Largest preparation burst, median (ms) | Paired median delta 95% interval (ms) |
|---|---:|---:|---:|---:|
| AUTOSAVE, 6 × 250 dimensions | 26.372 → 25.666 | 27.095 → 26.089 | 12.017 → 2.342 | [-0.807, -0.405] |
| SAVEALL, 6 × 250 dimensions | 26.138 → 25.373 | 26.764 → 26.080 | 11.678 → 2.348 | [-1.134, -0.232] |
| AUTOSAVE, 6 × 1000 dimensions | 104.561 → 101.631 | 109.683 → 107.239 | 48.955 → 9.069 | [-3.611, -1.709] |
| SAVEALL, 6 × 1000 dimensions | 102.202 → 99.974 | 104.499 → 101.574 | 48.611 → 9.218 | [-3.557, -1.939] |

History accounting/eviction uses 100 paired measurements after three warmups per arm. Fixture cloning and deferred snapshot destruction are excluded. The original estimator/removal policy is copied from `c0ccb87d0`. Both arms retain equal entry counts in these latency cases. The timed code runs on the edit-recording path; this is not full undo/redo replay or complete command latency. The earlier memory probe separately measured 160 → 111 retained steps under the 512 MiB budget; that reduction enforces a limit rather than compressing history.

| History case | Median before → after (ms) | p95 before → after (ms) | Paired median delta 95% interval (ms) |
|---|---:|---:|---:|
| 256 entries × 1 image, distinct versions | 0.0062 → 0.0390 | 0.0071 → 0.0520 | [0.0322, 0.0334] |
| 128 × 100, distinct versions | 0.0029 → 0.4308 | 0.0036 → 0.4942 | [0.4241, 0.4325] |
| 256 × 1,000, shared images | 0.0104 → 1.2145 | 0.0119 → 1.3029 | [1.1890, 1.2279] |
| 64 × 1,000, distinct versions | 0.0027 → 4.4032 | 0.0030 → 4.9584 | [4.3096, 4.4722] |
| 512 → 256 entries, default byte budget | 0.2595 → 0.0878 | 0.3116 → 0.1172 | [-0.1748, -0.1698] |
| 512 → 256 entries, unlimited byte budget | 0.3719 → 0.0192 | 0.4311 → 0.0254 | [-0.3529, -0.3506] |

The full retained-reference scan is the main measured regression. Faster prefix eviction does not cancel that scan on ordinary edit commits. Incremental accounting is the next optimization to investigate.

GPU tests use real wgpu resources on the reported adapter. The 1,000-range fixture shares 32 wire buffers and one constants buffer across viewport slots and the cache. The baseline counter covers fewer categories. With PERF disabled, the unchanged guard bypasses collection and both arms remain below 0.002 µs in this batched guard microbenchmark. GPU bookkeeping still exists during uploads/clones.

| PERF-enabled collection | Median before → after (µs/call) | p95 of sample averages before → after (µs) |
|---|---:|---:|
| 1 slots, 0 ranges/slot | 0.010 → 0.560 | 0.010 → 0.580 |
| 1 slots, 1,000 ranges/slot | 0.010 → 66.130 | 0.040 → 73.250 |
| 4 slots, 0 ranges/slot | 0.020 → 8.940 | 0.020 → 9.230 |
| 4 slots, 1,000 ranges/slot | 0.020 → 178.820 | 0.050 → 196.020 |
| 8 slots, 0 ranges/slot | 0.020 → 4.120 | 0.030 → 4.920 |
| 8 slots, 1,000 ranges/slot | 0.030 → 320.255 | 0.080 → 338.000 |

Enabled samples average ten collection calls, and disabled samples average 1,000 guard calls. These p95 values describe sample averages, not GPU frame times. The actual renderer reports once after its viewport-prepare loop.

Changed-state formatting/enqueue costs 1.520 → 5.110 µs median (6.960 → 9.360 µs p95 of sample averages). The companion compiles the actual PERF sink source, fills its UI history, and drains the stderr queue between samples. There are 200 paired samples averaging five updates each. Resource collection, background drain time and PERF widget rendering are excluded. Do not add these p95 values to collection p95 to infer frame latency.

Bookkeeping tests clone and drop 1,000 records per batch. Each of 200 paired samples averages ten batches. This isolates reference-counting/record-copy costs; it is not a complete GPU upload benchmark.

| Bookkeeping batch | Median before → after (µs/1,000 records) | Host record bytes before → after |
|---|---:|---:|
| wire_packed | 15.520 → 16.750 | 56 → 88 |
| wire_storage | 24.395 → 41.235 | 56 → 88 |
| material_metadata | 10.200 → 32.350 | 8 → 48 |

The material-resource container additionally retains 720 bytes of struct/Arc bookkeeping per unique resource set, excluding internal allocations. These are host bookkeeping sizes, not added VRAM. Empty-arena idle-slot eligibility checks stayed below 0.01 µs for the tested 1/4/8-slot fixtures in both arms; actual cleanup/destruction is outside that microbenchmark.

The audit does not establish browser end-to-end latency, requestAnimationFrame/input latency, GPU draw/present duration, complete driver-upload latency, history-destruction backlog, PERF widget rendering, or results on other workloads/hardware. Bootstrap intervals are pointwise resampling intervals conditional on the recorded fixtures, not simultaneous confidence bounds or a universal zero-regression proof.

Reproduce with PERF unset, from the repository root:

```sh
cargo test --locked --release --features performance-audit --lib performance_audit -- --ignored --nocapture --test-threads=1 > target/performance-audit-release.log 2>&1
python benches/run_gpu_logging_audit.py
```

The optional feature installs mimalloc only in the Windows audit test executable. It has no effect in the application executable. Five audit test entries passed (one is the save child runner); the audit records slower results instead of treating them as correctness failures. Another 22 existing save/history/undo/GPU functional checks passed in the same release build.
