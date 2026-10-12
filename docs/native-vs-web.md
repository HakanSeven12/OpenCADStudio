# Native vs. Web (WebAssembly)

Open CAD Studio ships as a native desktop app and as a WebAssembly build that
runs in the browser (https://hakanseven12.github.io/OpenCADStudio/). Both are
built from the same source; the web target drops or shims the pieces that a
browser can't provide. This page lists the differences.

## At a glance

| Area | Native (desktop) | Web (wasm) |
|------|------------------|------------|
| Windowing | Multi-window (`iced::daemon`) | Single window (`iced::application`); dialogs are in-canvas modals |
| 3D solid modeling | Yes | Yes — the geometry kernel is pure Rust |
| Hatch rendering | Yes | Yes — texture transport when WebGL2 lacks vertex-stage storage |
| Fonts | Embedded stroke fonts + system TrueType + shaping + fallback | Fetched Noto subsets (shaping, fallback, and family aliases) + embedded stroke fonts |
| File open | Native file dialog → path | Browser picker → bytes |
| File save | Path / Save dialog → write to disk | Save dialog → direct browser download |
| Parallelism | Multi-threaded (rayon) | Main thread + one Web Worker (DWG/DXF parse, hashing, save/autosave serialization) |
| GPU backend | Vulkan / DX12 / Metal (wgpu) | WebGL2 (the backend is pinned to OpenGL) |
| Update check / external links | `ureq`, `open` | skipped / `window.open` |
| External plugins | Native packages + Plugin Manager | **No** — native libraries cannot run in a browser |

## Details

### 3D solid modeling — web: the same as native
It used to be off. The modelling kernel was truck, whose mesh and boolean
crates reach `vtkio → xz2 → lzma-sys` — a C library that cannot cross-compile
to `wasm32` — so the web build dropped a `solid3d` feature and the Model tab,
solid tessellation and ACIS import all did nothing there.

The kernel is now opencadkernel, which is pure Rust and has no C dependency at
all, so none of that applies and the feature is gone. The web build makes
primitives, runs booleans, draws ACIS solids read from a file and writes them
back out as exact geometry, exactly as the desktop build does.

### Hatch rendering — web: same modes via a texture transport
The batched hatch pipeline binds a read-only storage buffer in the vertex
stage, which WebGL2 lacks. The hatch renderer selects its texture backend
instead (an RGBA32F data texture + `hatch_texture.wgsl`) and draws the same
solid, patterned, and gradient hatches as native, with the same dash math and
sub-pixel-to-solid LOD fallback. Everything else (lines, arcs, polylines,
text, images) draws normally.

### Fonts — web: fetched Noto subsets plus embedded stroke fonts
- **Native** discovers installed system fonts (fontdb), extracts TrueType
  outlines (ttf-parser), shapes runs with cosmic-text (ligatures, Arabic
  joining, kerning, bidi), and falls back to a system font for glyphs a stroke
  font lacks.
- **Web** fetches per-script Noto subsets from the server, registers them
  into the font database, and shapes runs with the same cosmic-text path
  (ligatures, Arabic joining, bidi) with per-glyph Noto fallback. Text styles
  naming a desktop family (Arial, Times New Roman, ...) resolve through an
  alias table to the Noto faces, so they render filled instead of falling to
  the stroke fonts; scripts without a fetched subset still render via the
  embedded LFF stroke fonts. (Text pops in once the first font fetch lands.)

### File I/O — web: bytes and downloads
- **Open**: the desktop returns a filesystem path and reads it (streaming).
  The web reads the picked file's bytes via the browser and parses them in
  memory (`io::load_bytes`).
- **Save**: both show the in-app Save dialog (filename + format). The desktop
  writes to the chosen path; the web serializes to bytes and triggers a direct
  browser download (a Blob + a programmatic `<a download>` click — no
  intermediate "click to download" link). The unsaved-changes prompt's *Save*
  routes through the same dialog on the web.
- There is no persistent filesystem path on the web, so a name-only path stands
  in for document tracking.

### Windowing — web: single window, in-canvas modals
The desktop uses `iced::daemon` and opens secondary OS windows for every
manager and style dialog. The browser has only the canvas, so the web uses
`iced::application` and renders all dialogs (layer/layout/plot managers, the
style editors, the colour picker, Save / unsaved prompts, About, shortcuts,
plugins, …) as in-canvas modal overlays. As of the Plan-B work this path is
shared: native renders the same modals too, so the desktop is effectively
single-window now. A modal's backdrop dims and blocks clicks but does not
dismiss; the ✕ button closes it.

### Parallelism — web: main thread plus one parse/serialize worker
`wasm32` has no threads without SharedArrayBuffer (which needs COOP/COEP
headers GitHub Pages can't set). `crate::par::prelude` re-exports `rayon` on
native and sequential `std` iterators on wasm, so the same call sites run in
parallel on the desktop and serially in the browser. Large drawings are slower
on the web.

### External plugins — web: unavailable

Marketplace plugins are native `.dll`, `.so`, or `.dylib` packages launched in
an isolated child process. Browsers cannot load those libraries or spawn the
plugin runner. The web Plugin button and `PLUGINS` command therefore show a
short explanation with a link to download the desktop app instead of opening
the marketplace. Installed desktop plugins remain in the per-user plugins
folder and load again on the next desktop launch.

### Platform shims (`src/sys.rs`)
- `open_url`: `open::that` on native, `window.open(_blank)` on web.
- `download_bytes`: web only (Blob + anchor download).
- `handle_path`: real path on native, a name-only `PathBuf` on web.
- `platform_info`: OS + arch on native, the browser user-agent on web (used to
  pre-fill the Send Feedback issue).
- The self-update check (`ureq`) is a no-op on the web.

## Build & deploy

- Native: `cargo build --release --bin OpenCADStudio`.
- Web app: `trunk build --release --public-url /app/ --dist dist/app --html-output index.html web-app.html`.
  Run `sh scripts/assemble-site.sh` afterward to add the landing page.
  `.github/workflows/pages.yml` builds and deploys to GitHub Pages on every
  release. No COOP/COEP headers are needed because the web build is
  single-threaded.
