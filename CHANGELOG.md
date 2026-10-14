# OpenCADStudio Changelog

> **Summary:** OpenCADStudio follows a weekly release cycle (Sunday 12:00 UTC) with versioning formatted as `v<Year>.<Week>` (e.g. `v2026.41`). This master changelog indexes all releases with an executive summary and links to dedicated per-release changelog documents located in [`changelogs/`](changelogs/).

---

## Index of Releases

| Release | Date | Highlights | Details |
| :--- | :--- | :--- | :--- |
| **[Unreleased](changelogs/unreleased.md)** | *2026-10-11 to Present* | Docking icon rails, block cycle protection & codec explosion, Web/WASM parity wave (in-place save, offline PWA, workers), MCP dynamic schemas & batch bindings | [changelogs/unreleased.md](changelogs/unreleased.md) |
| **[v2026.41](changelogs/v2026.41.md)** | *2026-10-11* | Sheet Set Manager (`SHEETSET`), `COUNT` command & live tables, associative `SWEEP`, RTL text, DimStyle per-end arrowheads (`DIMBLK1`/`DIMBLK2`), GPU options (Metal/OpenGL) | [changelogs/v2026.41.md](changelogs/v2026.41.md) |
| **[v2026.40 / v2026.40.1](changelogs/v2026.40.md)** | *2026-10-05* | `FIELD` command & dialog, `ATTDEF` parity, Blocks palette, MCP DeepZoom pyramidal tiling & SoM visual ID stamps, DWF/DGN underlays, `DYNMODE`, embedded TrueType PDF, instant layer toggling | [changelogs/v2026.40.md](changelogs/v2026.40.md) |
| **[v2026.39](changelogs/v2026.39.md)** | *2026-09-27* | Block creation & `WBLOCK` dialogs, Enter autocomplete, Python Host Model architecture, parametric constraints parity, `XCLIP` boundary clipping, trackpad pan/pinch, tessellation performance | [changelogs/v2026.39.md](changelogs/v2026.39.md) |
| **[v2026.38](changelogs/v2026.38.md)** | *2026-09-20* | 2D geometric constraints engine (`GEOMCONSTRAINT`), External References (`XREF`) overhaul, 3D OSNAP system (22.7x speedup), `DSETTINGS` & `ALIASEDIT`, associative viewport dimensions, 3D SpaceMouse, Windows printing | [changelogs/v2026.38.md](changelogs/v2026.38.md) |

---

## [Unreleased] - 2026-10-13

> **Summary:** Overhaul of the workspace docking and layout system with Photoshop-style vertical icon rails and draggable tabs; deep hardening of block definitions against circular recursion and standard-compliant block explosion (`INSERT`/`MINSERT`); major Web/WASM platform parity wave including in-place saving via File System Access API, offline PWA support, and worker-routed operations; plus custom `.pat` and `.lin` pattern loading.

### 🌟 Key Highlights
* **Modernized Docking & Workspace System:** Photoshop-style collapsible vertical icon rails, auto-hide flyouts, draggable tab reordering, and independent floating palettes. *(Contributed by [@posnofa](https://github.com/posnofa) and [@MarcelP102](https://github.com/MarcelP102))*
* **Block System Hardening & Safe Explosion:** Prevents circular/recursive block definitions and render loops. Delegates `INSERT`/`MINSERT` explosions to `opencadcodec`, restoring transformed `ATTDEF`s, correcting rotated arrays, and consolidating definition ops. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **MCP Extensibility & Schema Preflight:** Added side-effect-free preflight validation, on-demand entity creation schemas, batch parameter bindings, and specialist workflow guides. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Major Web / WASM Platform Wave:** In-place local saving via File System Access API, offline PWA, crash recovery (`restore-all`), clipboard image pasting, worker-routed `WBLOCK`/`SAVEALL`/`ARCHIVE`, and in-browser PDF plotting. *(Contributed by [@posnofa](https://github.com/posnofa), [@HakanSeven12](https://github.com/HakanSeven12), and Claude Opus 5.5)*
* **Custom Resources & Compatibility:** Loading for user `.pat` hatch and `.lin` linetype files, view backgrounds, and strict DWG reader compliance. *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12) and [@ramox81](https://github.com/ramox81))*

👉 **[View Full Unreleased Release Notes](changelogs/unreleased.md)**

---

## [v2026.41] - 2026-10-11

> **Summary:** Major feature milestone introducing the Sheet Set Manager (`SHEETSET`) with `.dst` project support; the automated entity `COUNT` command and palette with dynamic table generation; network-integrated associative swept surfaces (`SWEEP`); complete right-to-left (RTL) text layout in `MTEXT`; independent dimension style per-end arrowheads (`DIMBLK1`/`DIMBLK2`); table cell style overrides; GPU hardware selection options (Metal/OpenGL); MCP differential capture payload optimizations; and extensive drafting, plotting, spatial indexing, and memory fixes.

### 🌟 Key Highlights
* **Sheet Set Manager (`SHEETSET`):** Complete Sheet Set Manager palette, `.dst` project wizard, cross-drawing sheet fields, drawing lock-file coordination, and background sheet opening. *(Contributed by [@ramox81](https://github.com/ramox81) and [@HakanSeven12](https://github.com/HakanSeven12))*
* **Automated Entity Counting (`COUNT`):** Interactive block counting across model space or designated boundaries, collision diagnostics, and live evaluated count table insertion. *(Contributed by [@ramox81](https://github.com/ramox81) and [@HakanSeven12](https://github.com/HakanSeven12))*
* **Associative Swept Surfaces & 3D Modeling:** Network-tracked associative swept surfaces (`SWEEP`) with path-relative base points, miter corner history, B-rep `MASSPROP`, and curved offset/fillets. *(Contributed by [@ramox81](https://github.com/ramox81) and [@HakanSeven12](https://github.com/HakanSeven12))*
* **Advanced Typography & Table Overrides:** Fully compliant right-to-left (RTL) text rendering in `MTEXT`, exact bounding boxes for shaped/rotated text fields, and in-place table cell style overrides with automated formula updates. *(Contributed by [@ramox81](https://github.com/ramox81))*
* **Dimension Style Independent Arrowheads (`DIMBLK1`/`DIMBLK2`):** Supported independent arrowhead configurations for start and end leaders with real-time preview synchronization. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **GPU Platform Routing & Hardware Info:** Hardware-aware backend routing (Metal on macOS, OpenGL fallback for legacy hardware), hardware info in About dialog, and dedicated Graphics options tab. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*

👉 **[View Full v2026.41 Release Notes](changelogs/v2026.41.md)**

---

## [v2026.40 / v2026.40.1] - 2026-10-05 *(v2026.40.1 replaced pulled v2026.40)*

> **Summary:** Major release for release week 2026.40 (replacing the pulled `v2026.40` release). Introduces the interactive `FIELD` evaluation command and dialog, full `ATTDEF` attribute definition workflow parity, a redesigned tabbed Blocks palette with drag-and-drop insertion, multiscale pyramidal DeepZoom tiling & Set-of-Marks visual ID stamping for MCP vision tools, DWF/DGN and point cloud underlay support, dynamic input control (`DYNMODE`), script engine upgrades (`.scr`), TrueType embedded font PDF export, and smooth exponential zoom navigation. Incorporates immediate stability hotfixes for instant layer visibility toggling without CPU re-tessellation, 3D cylinder height input validation, load-time `BLOCK`/`ENDBLK` sentinel recovery, and MCP server request lifecycle & cancellation hardening.

### 🌟 Key Highlights
* **Instant Layer Visibility Toggling:** Toggle visibility states directly on GPU batches without triggering CPU re-tessellation passes. *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **`FIELD` Evaluation System & Dialog:** Real-time text/MText field evaluation, table formulas, block placeholders, and plot-time updates. *(Contributed by [@ramox81](https://github.com/ramox81))*
* **Attribute Definition (`ATTDEF`) Parity:** Complete `ATTDEF`/`-ATTDEF` dialogs, tag visibility management, and ribbon integration. *(Contributed by [@ramox81](https://github.com/ramox81))*
* **Blocks Palette Overhaul:** Redesigned Blocks palette with categorized tabs (Current Drawing, Recent, Favorites) and drag-and-drop insertion. *(Contributed by [@ramox81](https://github.com/ramox81))*
* **MCP Multiscale Pyramidal Tiling (DeepZoom):** On-demand cached multi-resolution pyramid tiling protocol (`manifest.json` and `tile/{level}/{x}/{y}`) allowing LLM vision agents to inspect dense drawings without exceeding token budgets. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Set-of-Marks Visual Grounding & Vision ID Stamp:** High-contrast numbered visual ID badge stamps on entity centroids in viewport captures with bidirectional grounding manifests mapping tags to handles and world bounds. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Visual Differential Streaming & Spatial CRS:** Standardized WCS spatial metadata under `_spatial` and visual diff engine (`compute_visual_diff`) with dirty region cropping (`diff_mode: crop`). *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Drawing Load Sentinel Recovery:** Materialized missing `BLOCK`/`ENDBLK` sentinels on load, preventing crashes and corrupt database states. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **PDF TrueType Font Embedding:** Subset TrueType font embedding directly in vector PDF export. *(Contributed by [@Chrisyeh1983](https://github.com/Chrisyeh1983))*

👉 **[View Full v2026.40 / v2026.40.1 Release Notes](changelogs/v2026.40.md)**

---

## [v2026.39] - 2026-09-27

> **Summary:** Milestone release featuring the initial Python Host Model plugin architecture, full dimensional and geometric parametric constraint parity (Linear, Aligned, Angular, Radial, Diameter), interactive Block Definition (`BLOCK`/`BMAKE`) and Write Block (`WBLOCK`) dialogs, command-line autocomplete execution on Enter, the `XCLIP` boundary clipping tool, redesigned PDF/XREF underlay integration panels, interactive plugin command previews, trackpad gesture navigation (pan and pinch-to-zoom), major rendering & tessellation performance optimizations, and robust metric drawing template defaults.

### 🌟 Key Highlights
* **Block Definition (`BLOCK`/`BMAKE`) & WBLOCK Dialogs:** Interactive block creation dialogs with on-screen sequencing, base point coordinate pickers, unit settings, and external file block export. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Command-Line Autocomplete & Architecture Modularization:** Pressing Enter executes highlighted autocomplete suggestions immediately. Modularized monolithic 11.5k-line command driver into clean domain modules. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Python Host Model & Plugin Architecture:** Foundational embedded Python runtime host with CAD entity data structures and automation APIs. *(Contributed by [@felixriestra](https://github.com/felixriestra))*
* **Parametric Constraints Parity:** Complete geometric and dimensional constraint toolsets (Linear, Aligned, Angular, Radial, Diameter). *(Contributed by [@ramox81](https://github.com/ramox81))*
* **Tessellation & Pipeline Performance:** Optimized polyline arc tessellation sampling, bypassed wide fills on thin polylines, memoized constraint glyphs, and capped selection grips at 4096. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Boundary Clipping (`XCLIP`):** Polygonal and rectangular clipping boundaries for external references and block inserts. *(Contributed by [@ramox81](https://github.com/ramox81))*
* **Trackpad Navigation Gestures:** Smooth two-finger panning and pinch-to-zoom gestures. *(Contributed by [@swit33](https://github.com/swit33))*

👉 **[View Full v2026.39 Release Notes](changelogs/v2026.39.md)**

---

## [v2026.38] - 2026-09-20

> **Summary:** Massive foundational release bringing the geometric 2D constraint engine (Horizontal, Vertical, Concentric, Smooth, Symmetric, Equal, Fixed, Autoconstrain), complete External References (XREF) Manager overhaul, independent 3D object-snap engine with 22.7x tracking speedup, modernized `DSETTINGS` dialog, `ALIASEDIT` command alias manager, associative paper space viewport dimensions, 3Dconnexion SpaceMouse navigation on Windows, comprehensive Windows printing & CTB routing, context right-click menus, and 3D format fixes (STEP/OBJ).

### 🌟 Key Highlights
* **External References (XREF) Manager Overhaul:** Docked palette with tree/list views, resizable columns, status detection (loaded, unloaded, stale, orphaned), detach/reload/bind/path ops, and split toolbar. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Independent 3D Object Snap System & Performance:** 3D OSNAP engine supporting 6 distinct modes, coupled with a 22.7x broadphase and tangent gating speedup in cursor snap tracking. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Drafting Settings (`DSETTINGS`) & `ALIASEDIT` Modernization:** Modern tabbed `DSETTINGS` dialog with editable grid/snap spacing, and full shortcut-editor parity in `ALIASEDIT`. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Geometric Parametric Constraints Engine:** 2D constraint engine with `AUTOCONSTRAIN` and the interactive `CONSTRAINTBAR`. *(Contributed by [@ramox81](https://github.com/ramox81))*
* **Polyline Arc & Offset Grip Editing:** Interactive radius grip editing for LWPOLYLINE arcs and parallel segment offset grips preserving connected geometry. *(Contributed by [@MarcelP102](https://github.com/MarcelP102))*
* **Fusion Themes & UCS Face/Object Alignment:** Fusion Black and Fusion White visual themes, interactive sketch mode, and direct UCS alignment to solid 3D faces (`UCS FACE`). *(Contributed by [@dasmatus](https://github.com/dasmatus))*
* **Associative Viewport Dimensions:** Associative dimensioning in layout viewports dynamically tied to 3D model geometry. *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12), [@CiberNin](https://github.com/CiberNin), and [@kevin9327](https://github.com/kevin9327))*
* **3Dconnexion SpaceMouse Support:** Native 6-DOF controller navigation support on Windows. *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Windows Plotting & Printing Subsystem:** Reliable DEVMODE queries, custom paper routing, and lenient CTB loading. *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12) and [@CiberNin](https://github.com/CiberNin))*

👉 **[View Full v2026.38 Release Notes](changelogs/v2026.38.md)**
