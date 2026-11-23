# Captures and visual verification

ocs_capture operates quietly in background and overlapped window states without stealing user focus. Viewports and captured snapshots are available as MCP resources under cad://session/{session_id}/viewport.png and cad://session/{session_id}/snapshot/{hash}.png; ocs_capture accepts delivery: 'resource' to avoid large inline base64 payloads, supplies standardized spatial grounding in _spatial, and provides multiscale DeepZoom pyramidal tiling via tile: {level, x, y} or cad://session/{session_id}/pyramid/manifest.json and cad://session/{session_id}/tile/{level}/{x}/{y}.png. 

Use annotate: true for entity IDs. To compare two frames, capture twice and diff the two images yourself: the retired `diff` parameter was removed because its baseline moved with the camera and the on-screen command line counted as changed pixels. delivery: resource returns a resource_link; read its URI only when you need the image. delivery: both includes the link and inline image.

## Headless / Off-Screen Layout & Aspect-Fit Capture

- `layout`: Pass `"Model"` or any Paper Space layout name to render headlessly off-screen without switching the active GUI tab, moving the live camera, or mutating selection. Works in headless `--serve` sessions, minimized windows, and inactive document tabs. Defaults `aspect_fit` to `true` unless `aspect_fit: false` is explicitly passed.
- `aspect_fit`: When `true` (default for off-screen `layout` captures), output PNG dimensions fit cleanly to the aspect ratio of `bounds`, `focus_handles`, `extents`, or sheet (longest edge = `max_dimension`). Pass `aspect_fit: false` to keep the live GUI viewport aspect ratio (or `4:3` when headless).
- `renderer`: Choose the off-screen backend (`"auto"`, `"gpu"`, or `"cpu"`):
  - `"auto"` (default): Uses headless `wgpu` WYSIWYG rendering (hardware adapter or software/WARP fallback on native) and falls back cleanly to `"cpu"` on web or when no adapter is available.
  - `"gpu"`: Requires headless `wgpu` WYSIWYG rendering (preserves active 3D perspective/shaded view on `view: "current"`, `Defpoints` display, and canvas styling).
  - `"cpu"`: Forces vector SVG/`resvg` plot-style rasterization (deterministic CPU output with polygonal viewport clipping, `draw_viewports_first` ordering, and plot visibility filtering).
- `document_id`: On `ocs_capture` and protocol-1 `capture`, pass an open tab's `document_id` (from `ocs_sessions`) to capture an inactive drawing tab headlessly without activating it or updating `snapshot/latest.png`. (`ocs_render` intentionally omits `document_id` and targets the bound session document.)
- Layout-scoped pyramid resources: Capturing a specific `layout` stores its manifest at `cad://session/{session_id}/layout/{layout}/pyramid/manifest.json` and tiles at `cad://session/{session_id}/layout/{layout}/tile/{level}/{x}/{y}.png`, keeping cached bounds isolated per layout.
- `scope: "window"` is refused on off-screen captures; use the default `scope: "viewport"`.

