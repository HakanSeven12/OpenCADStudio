# OpenCADStudio - Unreleased (2026-11-15 to Present)

> **Summary:** Added the contextual Text Editor ribbon tab with paragraph formatting and background mask controls, the `POPMENU` custom cursor menu, `ADDVERTEX` and `DELVERTEX` polyline vertex editing, `PanelStyle::Form` plugin panels, localized command-line command names, annotation scale prompting for dimensions/multileaders/blocks, smooth curve export for hatch boundary arcs in PDF/SVG plots, and fixes across Text Style Apply/Discard, TrueType glyph widths, DGN V8/WMF/3DS/PDF readers, undo granularity, and WASM linking.

---

### 🌟 Notable Release Notes

* **Text Editor Ribbon Tab:** Editing multiline text opens a contextual Text Editor ribbon tab and companion windows, supporting measured paragraph units, lettered lists, Combine Paragraphs, background mask selection (defaulting to red when unmasked), and translated line-spacing shortcuts (`#3095`). *(Contributed by [@ramox81](https://github.com/ramox81) and [@HakanSeven12](https://github.com/HakanSeven12))*
* **`POPMENU` Custom Cursor Menu:** Displays the user's own configured commands in a menu right at the cursor (`#3066`). *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **`ADDVERTEX` and `DELVERTEX`:** Adds and removes polyline vertices interactively by picking. *(Contributed by [@HakanSeven12](https://github.com/HakanSeven12))*
* **Plugin Form Panels (`PanelStyle::Form`):** Allows plugin panels to choose a structured form layout alongside `Palette` via the shared `FormControlView` (`#3092`). *(Contributed by [@yztink](https://github.com/yztink))*

---

### 🚀 New Features

* **Localized Command Names:** Accepts localized command names typed directly at the command line (`#3077`). *([@HakanSeven12](https://github.com/HakanSeven12))*
* **Annotation Scale Question:** Prompts for annotation scale before creating annotative dimensions, multileaders and block references (`#3090`). *([@ramox81](https://github.com/ramox81))*
* **PDF and SVG Hatch Boundary Curves:** Plots hatch boundary arcs as smooth curves rather than faceted chords in PDF and SVG exports (`#3076`). *([@HakanSeven12](https://github.com/HakanSeven12))*
* **`CIRCLE` Tangent Snaps & `MASSPROP` Prompt:** `CIRCLE` `2P` and `3P` accept Tangent object snaps; `MASSPROP` prompts for objects when nothing is selected. *([@HakanSeven12](https://github.com/HakanSeven12))*

---

### 🐛 Bug Fixes

* **Text, Styles & Fonts:** `Text Style` Apply preserves the values existing texts store (`#3089`) and Discard no longer requires a mutable stage; `TEXT` and attributes draw with their own oblique angle including `0`; `_MTED` matches as a whole word only in Text Editor tools; the Missing Fonts window can be turned off and back on. *([@ramox81](https://github.com/ramox81) and [@HakanSeven12](https://github.com/HakanSeven12))*
* **Modify, Layers & Naming:** `COPY` preserves the original objects' drawing order; `RENAME` keeps names with spaces whole whether typed quoted or answered step by step (`#3083`); each `LAYER` option commits as a single undo step (`#3080`). *([@KarimJerbi](https://github.com/KarimJerbi), [@assamaral](https://github.com/assamaral), [@travisjhughes](https://github.com/travisjhughes) and [@HakanSeven12](https://github.com/HakanSeven12))*
* **PDF, DGN, WMF & 3DS Interchange:** PDF export embeds TrueType faces with their real glyph widths; PDF reader skips path segments that have no current point (`#3085`); DGN V8 reads level names from every non-model stream and UTF-16 string linkages; WMF import skips records claiming more points than they hold (`#3071`); 3D Studio import bounds chunk lengths safely without 32-bit overflow. *([@HakanSeven12](https://github.com/HakanSeven12) and [@travisjhughes](https://github.com/travisjhughes))*
* **Web, CI & Regression Tests:** Kept `proj4wkt` native-only so the WASM web app links cleanly; added regression suites for PDF import (`#3087`), Windows metafile import (`#3081`), 3D Studio import (`#3075`), a 31-command undo sweep (`#3079`), `RENAME` quoted names, hatch panel cached fill rebuilds, and group save/reload persistence; fixed UTC release code-health measurement (`#3073`) and wiki CI error reporting (`#3074`). *([@HakanSeven12](https://github.com/HakanSeven12), [@travisjhughes](https://github.com/travisjhughes), [@KarimJerbi](https://github.com/KarimJerbi) and [@assamaral](https://github.com/assamaral))*

---

### ⚡ Performance

* **Selection Preview at Scale:** Rollover and marquee (`SELECTIONPREVIEW`) highlight updates no longer rebuild the committed selection's GPU overlay: hover changes ride a separate hover generation, so previewing a selection box over a drawing with thousands of objects already selected stays smooth. Selectable-group expansion reads a cached group snapshot instead of scanning every document object on every pointer move, marquee tick and rendered frame; the per-frame viewport selection data is cached per selection generation; dense-entity highlights reuse a persistent overlay constant buffer with partial updates instead of cloning the whole scene's constants; hovered text highlights are cached per hover identity; a marquee over `SELECTIONPREVIEWLIMIT` now caps the preview (stable subset) instead of dropping it entirely; and committing a huge selection defers the Properties panel rebuild by one frame so the click or box release returns instantly. *([@emrahoksak](https://github.com/emrahoksak))*

---

### 👥 Contributors

* **ramox81** ([@ramox81](https://github.com/ramox81)) — Text Editor ribbon tab and windows (`#3095`), annotation scale question (`#3090`), Text Style Apply (`#3089`) and oblique `TEXT`/attribute rendering.
* **Travis Hughes** ([@travisjhughes](https://github.com/travisjhughes)) — undo sweep across 31 commands (`#3079`), `LAYER` undo steps (`#3080`), PDF/WMF/3DS import tests and safety fixes (`#3071`, `#3075`, `#3081`, `#3085`, `#3087`), CI health and wiki fixes (`#3073`, `#3074`).
* **Hakan Seven** ([@HakanSeven12](https://github.com/HakanSeven12)) — `POPMENU` (`#3066`), `ADDVERTEX`/`DELVERTEX`, localized command names (`#3077`), hatch boundary curve plotting (`#3076`), TrueType PDF glyph widths, DGN V8 UTF-16 linkages, `CIRCLE` tangent snaps, `MASSPROP` prompt, WASM build fix and PR reviews.
* **yztink** ([@yztink](https://github.com/yztink)) — `PanelStyle::Form` layout via `FormControlView` (`#3092`).
* **Karim Jerbi** ([@KarimJerbi](https://github.com/KarimJerbi)) — `COPY` drawing-order preservation and hatch/group regression tests.
* **assamaral** ([@assamaral](https://github.com/assamaral)) — `RENAME` quoted names with spaces and tests (`#3083`).
