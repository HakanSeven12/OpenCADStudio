# Integrated fork AppImage test build

This integration branch combines current upstream OpenCADStudio with REGION object snaps, cursor/OTRACK performance fixes and both DWG 2010 codec corrections. It is a test candidate, not a stable release or an installed update.

## Source boundaries

- Integrated upstream base: `9eb3763e3ada710e0e306e246fd348cc2247f7ca`. This includes application PRs [#1986](https://github.com/HakanSeven12/OpenCADStudio/pull/1986) and [#1987](https://github.com/HakanSeven12/OpenCADStudio/pull/1987). REGION EXPLODE from PR #1857 was integrated separately as `758ab2863`.
- REGION snap and OTRACK changes retain their separate work-unit commits on this branch.
- Block Editor integration adds Polar Stretch Base/End bindings, native grip dependency preservation and parameter metadata editing from upstream commits `718fc6b4e`, `fdb29d9c6` and `9eb3763e3`. The local off-target alignment Flip correction remains in place.
- Throttled cursor drafting reuses upstream's authoritative Ortho/Polar calculation, including elevated planes and UCS Z tracking in 3D views.
- Codec: `sclMAX/opencadcodec` at `cb96711852f97db10811a4e8d632824c940a1134`, published as `integration/dwg2010-fixes`. A source-wide Cargo patch also redirects transitive codec users; no machine-local source override is required.
- The codec commit contains the associative-arc and REGION-tail fixes, now merged upstream through [codec PR #128](https://github.com/HakanSeven12/opencadcodec/pull/128) and [#129](https://github.com/HakanSeven12/opencadcodec/pull/129), as `369054b80b9dddb35f879600713d825c1bedb459`. The application upstream base still pins `ff75fdd`, before these fixes. Keep the existing fork pin during consolidation; upgrading to a newer upstream codec is a separate compatibility change.
- Historical transport and recipes are retained in [the checkpoint archive](checkpoints/dwg2010-codec-integration.md). They are not part of the active dependency resolution. Existing memory snapshots remain on their original branches; consolidation does not import them into the live memory database.

## Build from this branch

Install the normal Linux build prerequisites described in the project README. From the integration branch checkout:

```bash
cargo tree --locked --target x86_64-unknown-linux-gnu -p OpenCADStudio -i acadrust
cargo pkgid --offline acadrust
cargo test --locked --lib block_
cargo test --locked --lib region_snap
cargo test --locked --lib rapid_drawing_tests
cargo test --locked --workspace
cargo build --locked --release --bin OpenCADStudio
```

The resolved `acadrust` source must be the fork Git URL at the full codec SHA above, not an absolute local path. Keep the resolved lockfile committed with the source-wide patch.

Use the existing `.github/workflows/release.yml` AppDir/linuxdeploy recipe to package the release executable, desktop file, logo and metainfo. Give the artifact an explicit `TEST` filename. Do not run the release-upload or installation steps for this local candidate. Package only the curated runtime payload, never a repository or diagnostics directory.

## Try the resulting AppImage

```bash
chmod +x ./OpenCADStudio-TEST-all-improvements-linux-x86_64.AppImage
APPIMAGE_EXTRACT_AND_RUN=1 ./OpenCADStudio-TEST-all-improvements-linux-x86_64.AppImage --new-instance --read-only
```

Confirm both sides of the integration: affected DWGs open without recovery and retain their REGION profiles; F3 references work on REGION boundaries; cursor movement and dimension previews stay fluid with OTRACK enabled. Read-only mode prevents drawing saves. Passing command-line smoke tests does not establish these GUI outcomes.

## Limits

This is a native Linux test build. It does not establish Windows/macOS behavior, resolve the previously observed Windows print-spooler test failure, or establish new GUI acceptance for this consolidation. Host-linked Linux packages may require a recent glibc; inspect the actual artifact requirements before transferring it to another distribution. Record build identity and artifact checksum with the local test results. Local main consolidation does not authorize a public release, push or installed application replacement.
