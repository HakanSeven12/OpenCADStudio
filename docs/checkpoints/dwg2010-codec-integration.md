# Restore the verified DWG 2010 codec fixes

> Historical checkpoint, not the current build recipe. Both fixes were merged upstream in [codec PR #128](https://github.com/HakanSeven12/opencadcodec/pull/128) and [#129](https://github.com/HakanSeven12/opencadcodec/pull/129), as `369054b80b9dddb35f879600713d825c1bedb459`. The commands and verification below describe the original pinned checkpoint, not new consolidation evidence. For the integrated application, use [the fork build guide](../fork-appimage-testing.md).

**Both fixes are preserved, built and user-tested.** The affected drawings now open without recovery. This checkpoint carries source transport and reproducible build instructions, not an installed application or published codec PR.

## Exact source identities

| Component | Commit / transport |
|---|---|
| Codec upstream base | `ff75fddc322bda4cf817c5f68535c5c2b316b9c4` |
| Associative-arc fix | `58eb8bab76482186367ee7a8b3fba9c24c1b4308`, branch `fix/dwg2010-associative-arc-upstream` |
| REGION tail fix, stacked on the arc fix | `cb96711852f97db10811a4e8d632824c940a1134`, branch `fix/dwg2010-region-tail` |
| Application tested with both fixes | `ea4537429dfbdda6a94ea31dbc6c5cc684363f26` from upstream OpenCADStudio |
| Current codec transport | `patches/opencadcodec/dwg2010-codec-fixes.bundle` plus ordered `0001-*.patch` and `0002-*.patch` |
| Historical transport only | `dwg2010-associative-arc.bundle` preserves the older `b058a27` fix on base `14fdae5`; do not use it for this build |

No codec fork, upstream PR, AppImage, installation or source dependency upgrade is included. The application base does not include our pending REGION snap/performance PRs. Private drawings, binaries, caches and new memory/task logs are not part of this checkpoint update.

## Restore on another PC

Use a **fresh checkout** and the Linux native prerequisites from the project README. Commands below create only new ignored build paths. Do not overwrite an existing `target/Cargo.toml` or reuse a dirty codec checkout.

```bash
git clone --branch checkpoint/dwg2010-codec-integration https://github.com/sclMAX/OpenCADStudio.git
cd OpenCADStudio
ROOT="$(pwd)"
mkdir -p target

git clone --no-checkout https://github.com/HakanSeven12/opencadcodec.git target/codec-assoc-2010
git -C target/codec-assoc-2010 fetch origin ff75fddc322bda4cf817c5f68535c5c2b316b9c4
git -C target/codec-assoc-2010 bundle verify "$ROOT/patches/opencadcodec/dwg2010-codec-fixes.bundle"
git -C target/codec-assoc-2010 fetch "$ROOT/patches/opencadcodec/dwg2010-codec-fixes.bundle" \
  refs/heads/fix/dwg2010-associative-arc-upstream:refs/heads/fix/dwg2010-associative-arc-upstream \
  refs/heads/fix/dwg2010-region-tail:refs/heads/fix/dwg2010-region-tail
git -C target/codec-assoc-2010 switch fix/dwg2010-region-tail
test "$(git -C target/codec-assoc-2010 rev-parse HEAD)" = cb96711852f97db10811a4e8d632824c940a1134

printf '[workspace]\nmembers = ["codec-assoc-2010"]\nresolver = "2"\n' > target/Cargo.toml

git fetch https://github.com/HakanSeven12/OpenCADStudio.git ea4537429dfbdda6a94ea31dbc6c5cc684363f26
git worktree add --detach target/worktrees/codec-upstream-test ea4537429dfbdda6a94ea31dbc6c5cc684363f26
mkdir -p target/assoc-arc-integration
python3 - <<'PY'
from pathlib import Path
root = Path.cwd()
template = (root / 'scripts/local_codec_integration/codec-local.toml.in').read_text()
config = template.replace('__LOCAL_CODEC_PATH__', (root / 'target/codec-assoc-2010').as_posix())
(root / 'target/assoc-arc-integration/codec-local.toml').write_text(config)
PY
```

The bundle requires its stated upstream base; it is not a complete repository. It restores the exact two commit identities. The textual patches are an alternative: apply them in order to `ff75fdd` with `git am`, **not after importing the bundle**.

## Resolve, test and build

Use native-target dependency inspection rather than full offline `cargo metadata`, which previously requested irrelevant Android packages. A new PC may download dependencies already pinned in the lockfile; do not upgrade their versions.

```bash
cargo test --manifest-path "$ROOT/target/codec-assoc-2010/Cargo.toml" \
  --test dwg2010_region_tail --test dwg_assoc_edge_arc \
  --test acis_wireframe_bounds --test issue129 \
  --target-dir "$ROOT/target/assoc-arc-integration/codec-tests"

cd "$ROOT/target/worktrees/codec-upstream-test"
git apply "$ROOT/scripts/local_codec_integration/Cargo.lock.integration.patch"
cargo --config "$ROOT/target/assoc-arc-integration/codec-local.toml" \
  tree --locked --target x86_64-unknown-linux-gnu -p OpenCADStudio -i acadrust
cargo --config "$ROOT/target/assoc-arc-integration/codec-local.toml" pkgid --offline acadrust
# Expected: path+file://.../target/codec-assoc-2010#acadrust@0.6.4
# git diff must show only removal of acadrust's Git source from Cargo.lock.
cargo --config "$ROOT/target/assoc-arc-integration/codec-local.toml" \
  build --release --bin OpenCADStudio --locked \
  --target-dir "$ROOT/target/assoc-arc-integration/build-upstream"
"$ROOT/target/assoc-arc-integration/build-upstream/release/OpenCADStudio" --new-instance --read-only
```

Do not use application base `0b414389c` with the updated codec: its plugin type registry fails with `MissingVariants(["DxfVersion"])`. The preserved `prepare.py` / `verify.py` are historical, unfinished snapshots for that older application and seven unavailable fixtures; **do not run them for this checkpoint**. Use the commands above. See [snapshot notes](../../scripts/local_codec_integration/README.md).

## Verification already observed

- Associative arcs: five public synthetic regressions pass; the user confirmed that the original fix opened previously failing drawings. The original seven-file/70-comparison evidence predates the rebase; it was not rerun on this PC.
- REGION framing: public synthetic RED reproduced the optional empty silhouette-count error; final focused checks passed **21 tests**, including two independent verifier cases retained as regressions.
- Codec suite: **2020 passed, 35 ignored**, with `properties_table_numeric_rows_roundtrip_in_dxf_and_dwg` explicitly excluded because its failure was previously reproduced on the untouched base. This is not an unfiltered-green claim; the count predates adding the final two independent tests.
- Six affected DWG 2010 drawings plus the DWG 2018 control read with **zero skipped records and zero recovered errors**. REGION geometry/materials survive an exact in-memory DWG save/reopen comparison. The nine previously skipped REGIONs are retained. Original file hashes remained unchanged.
- The DWG 2018 control produced identical stdout, stderr and exit code before/after the REGION fix. Independent structural/runtime verification found no blocker.
- The user subsequently confirmed that **all supplied drawings open without recovery in the UI** and moved them to their local `ok` folder. This is opening acceptance, not an exhaustive visual geometry audit.
- Final release build and `--version` / `--help` passed. Local binary SHA256: `f81cf3c664347f7b229b66cf1afb585235618c01648ff6638ad191b3c810427f`. This identifies the observed binary, not a promised reproducible-build checksum. No installed application was replaced.

## Delivery status

Both fixes have been integrated upstream through codec PRs #128 and #129. The patches, bundles and Python snapshots remain historical recovery artifacts; do not apply them on top of a codec revision that already contains the fixes. The original checkpoint did not publish or install an application. Consolidation does not authorize a release or replace an installed binary.
