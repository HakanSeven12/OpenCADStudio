# Local measurements (never commit run data)

This folder is a placeholder for internal benchmark reports, captures, logs,
machine settings, drawing fingerprints and analysis. Git ignores everything
here except this generic guide. Do not force-add ignored files.

Keep reusable harnesses, synthetic fixtures, tests and general reproduction
instructions in version control. Keep measurements and machine/drawing-specific
conclusions here. Reports removed from tracked locations belong in `reports/`;
new runs belong in a dated subfolder with their associated artifacts.

Use measurements as follows:

1. Preserve an unchanged baseline, including its executable/workload hashes,
   settings, instrumentation and raw samples.
2. Isolate one suspected cause with controlled workloads; combined-session
   growth or a correlated stall does not identify a root cause.
3. Run the proposed change against the same inputs, machine, driver, viewport
   dimensions, cadence and tracing settings in fresh processes.
4. Compare resident/private memory, private commit, frame-interval tails and
   operation timings. Check geometry correctness, source integrity and all
   validity flags before accepting a result.
5. Keep before/after evidence locally. Commit the reusable implementation and
   tests only after review; do not include measurement artifacts or results in
   source files, documentation or commit descriptions.

Existing reports whose recorded absolute artifact paths predate a move remain
unchanged for provenance. The summary tool can resolve the adjacent artifact
directory after relocation. Internal results are baselines and hypotheses until
a controlled before/after comparison supports a conclusion.
