# OpenCADStudio - Unreleased (2026-10-11 to Present)

> **Summary:** Overhaul of the workspace docking and layout system with Photoshop-style vertical icon rails and draggable tabs; deep hardening of block definitions against circular recursion and standard-compliant block explosion (`INSERT`/`MINSERT`); major Web/WASM platform parity wave including in-place saving via File System Access API, offline PWA support, and worker-routed operations; plus custom `.pat` and `.lin` pattern loading.

---

### 🌟 Notable Release Notes

* **Modernized Docking & Workspace System:** Introduced Photoshop-style collapsible vertical icon rails for docked groups, 150ms auto-hide flyout behavior, draggable tab reordering, and independent floating windows for palettes (including a dedicated `LAYERS` manager). *(Contributed by [@posnofa](https://github.com/posnofa) and [@MarcelP102](https://github.com/MarcelP102))*
* **Block System Hardening & Safe Explosion:** Complete overhaul of block references and definitions. Prevents circular/recursive block nesting crashes at both commit time and render time. Block explosions (`INSERT` and `MINSERT`) now delegate to `opencadcodec`, properly restoring transformed `ATTDEF` templates and handling rotated arrays with non-zero base points. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Major Web / WASM Platform Wave:** In-place local file saving via the File System Access API, offline PWA support, automatic crash recovery (`restore-all`), clipboard image pasting, worker-threaded `WBLOCK` / `SAVEALL` / `ARCHIVE` tasks, and in-browser PDF plotting. *(Contributed by [@posnofa](https://github.com/posnofa), [@HakanSeven12](https://github.com/HakanSeven12), and Claude Opus 5.5)*
* **Custom Resources & Compatibility:** Native support for loading custom user hatch patterns (`.pat`) and linetypes (`.lin`), custom view backgrounds (solid, gradient, and device-pixel scaled images), and strict DWG reader compliance fixes. *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12) and [@ramox81](https://github.com/ramox81))*

---

### 🚀 New Features

#### UI & Workspace
* **Docking Panels & Icon Strips:** Added vertical icon rails for docked palette groups, per-edge auto-hide flyouts, customizable slot heights, and double-click float/dock toggles. *(Contributed by [@posnofa](https://github.com/posnofa))*
* **Draggable Tabs & Groups:** Palette tabs can now be reordered by dragging; floating windows support grouped tabs. *(Contributed by [@posnofa](https://github.com/posnofa))*
* **Layer Manager Palette:** Integrated `LAYERS` into the docking system as a native pallet. *(Contributed by [@posnofa](https://github.com/posnofa))*
* **Graphic Attributes Palette:** Added toggles, controls for open boundaries, color swatches with `*VARIES*` indicator, and persistent docking layout. *(Contributed by [@MarcelP102](https://github.com/MarcelP102))*
* **MATCHPROP Paintbrush Cursor:** While `MATCHPROP` is armed the CAD crosshair gives way to a monochrome paintbrush whose tip rides the pick point, echoing the AutoCAD "match properties" cursor. The glyph borrows the crosshair's own colour — including a user-customised one — so only the shape changes, never the ink. *(Contributed by [@Ch4rlos](https://github.com/Ch4rlos))*

#### Selection & Drafting
* **Advanced Selection Filter (`FILTER`):** Added comprehensive selection filtering with normalized logical operators and attribute filters (v2.2). *(Contributed by [@turbuczn](https://github.com/turbuczn))*
* **Transparent Object Snap Overrides:** Introduced transparent object snap overrides within active drafting commands. *(Contributed by [@yztink](https://github.com/yztink))*
* **Chamfer & Fillet No-Trim Mode:** Added support for preserving source entities during `CHAMFER` and `FILLET` (`TRIMMODE=0`). *(Contributed by [@YsDirector](https://github.com/YsDirector))*
* **Line Tangency in Arcs:** Supported line tangency constraints during arc construction with improved trim contact precision. *(Contributed by [@yztink](https://github.com/yztink))*
* **SVG Export:** Added native SVG export via `EXPORTSVG`. *(Contributed by [@posnofa](https://github.com/posnofa))*
* **Custom Resource Loading:** Added direct loading for user `.pat` hatch pattern and `.lin` linetype files. *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*

#### Views & Layouts
* **`-VIEW` Command Line Interface:** Added command-line view presets, orthographic UCS alignment, and window definitions. *(Contributed by [@ramox81](https://github.com/ramox81))*
* **View Backgrounds:** Added support for solid, two-color linear gradient, and scaled/offset device-pixel raster backgrounds. *(Contributed by [@ramox81](https://github.com/ramox81))*
* **Automatic Layout Viewport (`LAYOUTCREATEVIEWPORT`):** Automatically creates and enables an initial viewport upon first opening a layout tab. *(Contributed by [@ramox81](https://github.com/ramox81))*

#### Web & Platform
* **In-Place File System Access:** Native saving in browser via File System Access API. *(Contributed by [@posnofa](https://github.com/posnofa))*
* **PWA & Offline Recovery:** Full offline PWA capabilities with deep linking (`?file=`) and session recovery (`restore-all`). *(Contributed by [@posnofa](https://github.com/posnofa))*
* **Web Worker File Operations:** Offloaded `WBLOCK`, `SAVEALL`, and `ARCHIVE` transmittal generation to background web workers. *(Contributed by [@posnofa](https://github.com/posnofa))*
* **3D Formats in Web:** Enabled STEP/STL export and OBJ import on web builds. *(Contributed by [@posnofa](https://github.com/posnofa))*
* **In-Page Plot Preview:** Embedded PDF plot preview directly in-page on wasm32 with quality presets. *(Contributed by [@posnofa](https://github.com/posnofa))*

#### Automation & MCP (Model Context Protocol)
* **Canvas Pointer Event Replay:** Added pointer event replay on the canvas for automated test drivers. *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Dynamic Entity Schemas & Preflight:** Added on-demand entity creation schema inspection and side-effect-free preflight validation in MCP server (`006be890`, `97afb573`). *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **MCP Batch Step Bindings & Summaries:** Added batch parameter bindings to earlier step results and batch summaries with read-only detail retrieval (`50030be8`, `7b68d274`). *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Specialist Guides & Schema Deduplication:** Served architectural and workflow specialist guides on demand and deduplicated operations with shared schemas (`8943a6fd`, `f4b33d5c`). *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Resource Links for Captures:** Returned dedicated resource URIs for reference-only image captures (`6ff8d8d7`). *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Spline Automation API:** Added spline creation and parameter serialization with strict knot/weight validation (`entities_create`). *(Contributed by [@YsDirector](https://github.com/YsDirector))*
* **Selection Event Streaming:** Automation clients now receive real-time selection change notifications. *(Contributed by [@YsDirector](https://github.com/YsDirector))*
* **Host API v8:** Added name-addressed `XRecord` CRUD operations on `HostApi`. *(Contributed by [@schoeller](https://github.com/schoeller))*

---

### 🐛 Bug Fixes

#### Blocks & Attributes
* **Circular Block Prevention:** Added cycle detection in `block_service` preventing recursive block definitions (`INSERT` in `BEDIT` or redefinition) and safely truncated render-time cycles with session warnings (`9c08e8a3`). *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Block Explosion & ATTDEF Restoration:** Restored transformed `ATTDEF` definitions when exploding `INSERT` / `MINSERT` blocks, dropped instance attribute values, and corrected rotated array spacings (`8d287dec`, `59bb6faa`). *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Constant Attributes in ATTEDIT:** Enforced constant attribute flags in attribute editor by disabling constant fields and ignoring edits (`59bb6faa`). *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Block Service Consolidation:** Consolidated delete, rename, and purge operations through a single block definition service (`2b50b6c8`). *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Paste Block Origin:** Fixed `PASTEBLOCK` placement to align accurately with the picked point (`925c91cc`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Wipeout Render Cache & Draw Order:** Fixed wipeout render cache invalidations on insert copy/grip moves and preserved draw order for overlapping block wipeouts and hatches (`fe0edb61`, `1074c81f`, `72e97c6f`). *(Contributed by [@gianlucafiore](https://github.com/gianlucafiore))*
* **Cross-Document Dictionaries:** Prevented copied entities from incorrectly sharing extension dictionaries across documents (`b5b505ee`). *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*

#### Geometry & Drafting
* **Model Space Grip Dragging:** Constrained grip drags to the drawing plane, preventing grips from collapsing onto themselves (`9cc890c9`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Fillet Polyline Join:** Automatically join filleted lines or arcs into the target polyline (`2f087190`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Boundary Edge Preselection in Extend:** Enabled `EXTEND` to honor preselected boundary entities (`3840630a`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Angular Distance Calculations:** Corrected `DIST` XY angle measurement to measure from `ANGBASE` in the direction defined by `ANGDIR` (`fa00c27e`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Dimension Text & Blocks:** Kept picture block positions consistent during world-space edits (`DIMTMOVE=2`) (`c5b1269c`, `41db8d67`). *(Contributed by [@ramox81](https://github.com/ramox81) and [@YsDirector](https://github.com/YsDirector))*
* **Extrude Taper History:** Restored fallback profiles in place under group 295 and applied taper angles strictly when chosen (`656f0cea`, `f4a0b1d6`). *(Contributed by [@ramox81](https://github.com/ramox81))*

#### UI & General
* **Input Focus & Rebound Spacebar:** Allowed rebound spacebar execution while the command line retains focus (`7e0866e6`, `cbefecb1`). *(Contributed by [@theluckystrike](https://github.com/theluckystrike))*
* **Field Focus Persistence:** Retained typed layer and text style names when UI input fields lose focus (`f6503ad4`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Start Screen Quick-Access:** Restored functionality of the New button on the initial Start screen (`88803cd6`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Windows Default Apps Fallback:** Gracefully fall back to Windows Settings when the system default-apps dialog fails (`46395464`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **macOS Release DMG:** Restored Applications shortcut link in macOS DMG bundles (`f769b875`). *(Contributed by [@KevinGriffin-new](https://github.com/KevinGriffin-new))*
* **Plugin Grip Refresh:** Refreshed selected grip markers following plugin-driven entity edits (`c6dcee2b`). *(Contributed by [@schoeller](https://github.com/schoeller))*
* **Localizations:** Completed Hungarian CAD translations and updated message catalogs (`49f12e01`, `a5435a6d`). *(Contributed by [@turbuczn](https://github.com/turbuczn), [@hvfrancesco](https://github.com/hvfrancesco), and [@HakanSeven12](https://github.com/HakanSeven12))*

#### Rendering, Plotting & File I/O
* **Transparent Hatch Layering:** Corrected depth sorting to draw transparent hatches cleanly over background lines (`d1a2e9db`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Wide Polyline PDF Output:** Rendered wide polylines in PDF export with square endcaps and mitered joins (`088dec4a`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Plot Style Precedence:** Prioritized explicitly requested CTB plot styles over stored layout page setup tables during headless plots (`468d1e4f`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12) and Claude Opus 5.5)*
* **Window-Plot Sheet Centering:** Centered layout sheets accurately over the plot window (`710a837a`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12) and Claude Opus 5.5)*
* **Atomic Save Retry:** Added retry handling during atomic file replacements to tolerate brief locks (`9e31af3f`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Image Memory Budgeting:** Enforced streaming decode checks and memory budget limits against malformed or excessive GIF/TIFF images (`f015c3b5`, `ce6bd2c7`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12) and [@posnofa](https://github.com/posnofa))*
* **DWG Strict Compliance:** Pinned upstream `opencadcodec` fixes to ensure saved DWGs pass strict audits without recovery dialogs (`390e1216`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12) and Claude Opus 5.5)*

---

### 👥 Contributors

Huge thanks to everyone who contributed to this cycle:

* **Hakan Seven** ([@HakanSeven12](https://github.com/HakanSeven12)) — Upstream kernel/codec syncs, DWG/DXF compatibility, headless plot style precedence, window-plot sheet centering.
* **Afonso Portela** ([@posnofa](https://github.com/posnofa)) — Web/WASM platform wave (in-place save, offline PWA, worker saves, 3D formats), docking system overhaul, SVG export.
* **Karim Jerbi** ([@KarimJerbi](https://github.com/KarimJerbi)) — Circular block cycle prevention, block explosion delegating to codec, constant attribute enforcement, rotated MINSERT arrays, unified block definition service, MCP dynamic entity schemas & preflight validation, batch step bindings, specialist guides, and cross-document extension dictionary isolation.
* **ramox81** ([@ramox81](https://github.com/ramox81)) — View backgrounds, `-VIEW` preset CLI, associative surfaces, initial layout viewport generation.
* **Claude** (Claude Opus 5.5) — Core CAD engine, layout/plot, and web/WASM co-authoring.
* **ysdirector** ([@YsDirector](https://github.com/YsDirector)) — Automation API: spline creation/validation, selection event publishing, task completion guards.
* **gianlucafiore** ([@gianlucafiore](https://github.com/gianlucafiore)) — Block wipeout render cache invalidation, GPU boundary masks, draw-order sorting.
* **Marcel** ([@MarcelP102](https://github.com/MarcelP102)) — Graphic Attributes palette and docking placement.
* **Sebastian** ([@schoeller](https://github.com/schoeller)) — Host API v8 XRecord CRUD operations, plugin grip refresh.
* **turbuczn** ([@turbuczn](https://github.com/turbuczn)) — Hungarian localizations, selection filter list enhancements.
* **yztink** ([@yztink](https://github.com/yztink)) — Transparent object snap overrides, arc line tangency construction.
* **Kevin Griffin** ([@KevinGriffin-new](https://github.com/KevinGriffin-new)) — macOS DMG bundle Applications shortcut link (`#769`).
* **francesco fantoni** ([@hvfrancesco](https://github.com/hvfrancesco)) — Translations and UI improvements.
* **theluckystrike** ([@theluckystrike](https://github.com/theluckystrike)) — Command line rebound spacebar execution (`#1710`).
* **Carlos Quiroga** ([@Ch4rlos](https://github.com/Ch4rlos)) — `MATCHPROP` paintbrush cursor.
