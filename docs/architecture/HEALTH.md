# Code health

Open CAD Studio measures its own code health on every release and on every pull request. The results are on the wiki's **Code Health** page; each pull request's checks summary has a **Code health** section. Nothing here blocks a merge. The point is to make each pull request's effect visible, thank the ones that improve things, and give contributors small, concrete cleanups to pick up.

## Where to see it

- **Code Health wiki page**: the scorecard, the trend over recent releases, thanks to the pull requests that improved a measure, and a list of cleanups anyone can pick up. Rebuilt on every release by `.github/workflows/wiki.yml`.
- **Your pull request**: the `health (informational)` job compares your branch with the commit it merges into and lists what got better and what got worse, with the file and line, including the tests you add. If you edit a file that no test runs yet (as of the last release), it says so: a test there counts toward the coverage goal. The lint job lists clippy and rustfmt warnings in the files you touched only, so the list is about your change, not the whole tree.

## The measures

Goals, baselines and the reason for each measure are in [`health-goals.json`](health-goals.json). Goals are milestones: each count's next milestone is half the previous one, so progress shows release by release. Changing a goal is a reviewed pull request, like changing the layer rules.

### Structure

- **Upward dependencies**: code that uses a module in a higher layer, counted in files and in module pairs. The layers and how they are counted are in [`RULES.md`](RULES.md).
- <a id="modules-that-use-each-other"></a>**Modules that use each other**: top-level modules A and B where A uses B and B uses A (any reference, not only upward ones). Such modules can only be changed and understood together.
- <a id="size"></a>**Files over 3,000 lines** and **functions over 150 lines** (from `fn` to the closing brace, production code only). Large units are hard to review and collect merge conflicts. Generated files (an opening comment that says `Generated` or `@generated`, such as `src/locale_catalog.rs`) are left out of both size measures: their size comes from the generator. **Top-level `Message` variants** counts the variants of the app's message enum.

### Robustness

- <a id="robustness"></a>**unwrap/expect/panic!** in production code (comments, strings and test code are skipped). A panic closes the app; returning an error lets the user recover. Counted separately for `src/io`, where file import and export run on input the app does not control.
- **unsafe without a SAFETY note**: an `unsafe` block needs a `// SAFETY:` comment on its line or directly above it; an `unsafe fn` or `unsafe impl` needs `SAFETY:` or a `# Safety` doc section directly above it (attributes in between are fine). These follow clippy's `undocumented_unsafe_blocks` and `missing_safety_doc`.

### Hygiene

- <a id="hygiene"></a>**Clippy warnings**, **compiler warnings** (`cargo clippy --workspace --all-targets`) and **files rustfmt would change** (`cargo fmt --all -- --check`). Measured on releases; pull requests see only the warnings in the files they touch.

### Tests and dependencies

- <a id="tests-and-dependencies"></a>**Test functions**: the number of `#[test]` functions in `src/`, `tests/` and the workspace crates (comments and strings are ignored). Counted on every pull request, so adding tests shows in your summary and in the release's thanks list. Also **RustSec advisories** (`cargo audit`) and **crates in the dependency tree** (`cargo metadata`), on releases.

### Coverage

- <a id="coverage"></a>**Line coverage of the core layers**: the share of production lines in layers 0 to 4 (see [`RULES.md`](RULES.md)) that the test suite runs, measured with `cargo llvm-cov` on each release. The long-term target is 100%; milestones are every 5 points.
- **What counts:** production lines only. Test files and lines inside `#[cfg(test)]` / `#[cfg(all(test, ...))]` modules are left out (counting them would read about six points high), and so are generated files and the paths listed under `coverage.exclude` in [`health-goals.json`](health-goals.json), each with its reason (today: GPU rendering, which needs a graphics device, and the profiling aid `src/perf.rs`). An exclusion is a folder (it ends with a slash) or one exact file, so a new file is never excluded by accident. The release measurement runs on Linux; code for other platforms is not compiled there and not counted.
- **On the wiki page:** coverage per layer, and **Add tests here**: the core files with the most lines no test runs, weighted by how often each changed in the last 28 days, plus the files no test runs at all.
- Coverage is measured on releases only, because it runs the whole test suite again. Pull requests see the test functions they add instead.

## Thanks

On each release the wiki job replays every commit merged since the previous release with the source measures (structure, robustness and test functions) and thanks each contributor whose merged work improved one: removing a panic site, or adding tests, both count. The list has one line per contributor, in alphabetical order, with the totals and the pull requests they came from; it is never a ranking, and nobody is listed for making a number worse. If the replay does not finish in its time limit, the page says the list was not computed. Hygiene and coverage are compared release to release only, because rebuilding and testing every commit would take hours.

## Running it yourself

```
python3 scripts/health.py measure                 # source measures, a few seconds
python3 scripts/health.py measure --cargo         # plus clippy, rustfmt, tests, dependencies
python3 scripts/health.py measure --cargo --coverage   # plus core-layer line coverage (needs cargo-llvm-cov)
python3 scripts/health.py diff ../main-checkout   # what your branch changes
```

`measure` prints every finding with its file and line, so you can find the panic sites or long functions in the module you are working on.
