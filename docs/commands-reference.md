# OpenCADStudio command reference

Generated from the command registry of OpenCADStudio `main` (commit f664c42, 2026-11-02).

| | Count |
|---|---|
| Registered commands (typed names, `-` command-line forms and internal sub-command tokens) | 700 |
| System variables | 202 |
| Command aliases in the default `ocad.pgp` | 247 |

## How to read this
* **Alias** – the shortcut from the default `ocad.pgp` (`assets/ocad.pgp`, copied to your user config folder on first launch). Edit it by hand or with `ALIASEDIT`; one `ALIAS, *COMMAND` per line.
* **`-COMMAND`** – the command-line form: the same command without its dialog, prompting in the command line.
* **`'NAME`** – prefix an apostrophe to run a command or system variable transparently inside another command.
* Tokens containing `_` (for example `ARC_3P`, `CIRCLE_TTR`) are internal sub-command tokens the ribbon and menus send; they can be typed but are not meant as everyday commands.
* **†** – description inferred from the command name, its place in the menus/ribbon and standard CAD meaning; not yet confirmed against the implementation.
* Descriptions come from the command reference text, the menu or ribbon label, or the comment beside the command in the source. A dash means none exists yet.

## Commands

### 3D Modeling (36)

| Command | Aliases | Description |
|---|---|---|
| `BOX` | — | Creates a 3D solid box primitive. *Tip:* Type C (Cube) to ensure all three dimensions (Length, Width, Height) remain equal. |
| `CHAMFEREDGE` | — | Chamfers selected edges of a 3D solid. † |
| `CONE` | — | Creates a 3D solid cone. † |
| `CYLINDER` | `CYL` | Creates a 3D solid cylinder primitive. *Tip:* Specify base center, radius, and height, or use the Elliptical sub-optio for oval cylinders. |
| `EDGE` | — | Changes the visibility of 3D face edges. † |
| `EXTRUDE` | `EXT` | Extrudes 2D closed boundaries or curves along a normal or direction path to create 3D solids or surfaces. *Tip:* Use T (Taper angle) option to create draft angles for molded or cast parts. |
| `FILLETEDGE` | — | Rounds selected edges of a 3D solid. † |
| `FLATSHOT` | `FSHOT` | Creates a 2D flattened vector representation of all 3D solids based on the current viewing perspective. *Tip:* Generates clean 2D elevations and plan views with optional hidden lin distinction. |
| `HIDE` | `HI` | Hidden-line view of the active viewport. |
| `HIDEOBJECTS` | — | Hide the current selection. |
| `INTERFERE` | `INF` | Clash detection between structural and mechanical parts. Highlights 3D solid clashing and generates temporary interferen solids at points of collision. |
| `INTERSECT` | `IN` | Creates a composite solid, surface, or region from the intersecting volume of two or more overlapping entities. *Tip:* Removes all non-overlapping material, leaving only shared interior volume. |
| `LOFT` | — | Creates a 3D solid or surface in the space between several cross- section profiles. *Tip:* Use guide curves or paths to govern transitions between different geometric cross-sections (e.g. circle to square). |
| `POLYSOLID` | `PSOLID` | Creates a wall-like 3D solid from a path, width and height. † |
| `PRESSPULL` | — | Dynamically pulls or presses bounded areas and faces to extrude or hollow out 3D solid geometry. *Tip:* The most intuitive 3D command in OCS: clicking inside any closed boundary immediately adds or subtracts volume. |
| `PYR` | — | Alias of PYRAMID. † |
| `PYRAMID` | — | Creates a 3D solid pyramid. † |
| `REVOLVE` | `REV` | Creates a 3D solid or surface by sweeping a 2D profile around an axis. *Tip:* Commonly used for shafts, bottles, domes, spherical vessels, and circular architectural columns. |
| `SECTION` | `SEC` | Uses the intersection of a plane and 3D solids to generate a 2D cross- sectional Region object. *Tip:* Great for obtaining exact 2D perimeter outlines through complex 3D assemblies at any cut angle. |
| `SECTIONCULL` | — | Erase what lies outside a section object. |
| `SECTIONPLANE` | — | Creates a section object that acts as a cutting plane through 3D solids, surfaces, and point clouds. *Tip:* Toggle Live Sectioning on to see inside buildings or mechanical assemblies in real time. |
| `SHADEMODE` | `SHA` | Sets the shading mode (legacy; see VISUALSTYLES). † |
| `SHELL` | — | Hollows a 3D solid to a given wall thickness. † |
| `SL` | — | Alias of SLICE. † |
| `SLICE` | — | Slice/section + interference + press-pull/thicken + 3D transforms + wall/pyramid. |
| `SOLID` | `SO` | 2D filled solid. Reached via SO / SOLID2D — the bare SOLID verb is currently the shaded-display toggle (token collision tracked). |
| `SOLIDCHAMFER` | — | Alias token for CHAMFEREDGE. † |
| `SOLIDEDIT` | — | Edits faces, edges and bodies of a 3D solid. † |
| `SOLIDFILLET` | — | Alias token for FILLETEDGE. † |
| `STLOUT` | — | Exports selected 3D solids to binary or ASCII Stereolithography (.stl) mesh files for 3D printing and toolpath CAM. *Tip:* Select only watertight 3D solids; standard OCS validates the manifold mesh before exporting. |
| `SUBTRACT` | `SU` | Subtracts the volume of one set of 3D solids or regions from another. *Tip:* First select objects to keep and press Enter; then select objects to subtract and press Enter. |
| `SWEEP` | — | Creates a 3D solid or surface by sweeping an open or closed 2D profile along an open or closed 2D/3D path curve. *Tip:* Ideal for pipes, moldings, handrails, conduits, and wiring harnesses. |
| `THICKEN` | — | Turns a surface into a 3D solid by thickening it. † |
| `TORUS` | `TOR` | Creates a 3D solid torus. † |
| `VSCURRENT` | — | Visual styles (mapped to the wireframe / shaded view). |
| `WEDGE` | `WE` | Creates a 3D solid wedge. † |

### Annotation (57)

| Command | Aliases | Description |
|---|---|---|
| `-DATAEXTRACTION` | — | -DATAEXTRACTION: run a saved extraction from the command line. |
| `-DIMINSPECT` | — | Command-line (no dialog) form of DIMINSPECT. |
| `-STYLE` | — | Command-line (no dialog) form of STYLE. |
| `-TABLE` | — | Command-line (no dialog) form of TABLE. |
| `AIMLEADEREDITADD` | — | Ribbon: Add Leader. |
| `AIMLEADEREDITREMOVE` | — | Ribbon: Remove Leader. |
| `ANNOALLVISIBLE` | — | Toggles whether annotative objects show every scale representation. † |
| `ANNOAUTOSCALE` | — | Toggles automatic addition of the current annotation scale to annotative objects when it changes. † |
| `ANNORESET` | — | Ribbon: Sync Scale Positions. |
| `ANNOSCALE` | — | ANNOSCALE / CANNOSCALE <ratio> — set the current annotation scale (e.g. 1:50, 2:1, or a plain factor). Drives annotative-object size in model space and is written to the drawing header. |
| `ANNOUPDATE` | — | Updates annotative objects to the current annotation scale/size. † |
| `ARCTEXT` | — | ARCTEXT <text> — lay the text out as one Text entity per character along the selected arc, each rotated to follow the tangent. |
| `CENTERDISASSOCIATE` | — | Detaches a centre mark or centreline from the objects it was created on. † |
| `CENTERREASSOCIATE` | — | Re-attaches a centre mark or centreline to chosen objects. † |
| `CENTERRESET` | — | CENTERRESET / CENTERDISASSOCIATE work on the preselected centre objects, or ask for them. |
| `COUNTAREA` | — | Count palette: counts objects by enclosed area. † |
| `COUNTCLOSE` | — | Closes the Count palette. † |
| `COUNTFIELD` | — | Inserts a field showing the count result. † |
| `COUNTLISTCLOSE` | — | Closes the Count list. † |
| `COUNTTABLE` | — | Inserts a table of the count results. † |
| `DATAEXTRACTION` | `DX` | Extracts geometric data, block attribute values, and drawing metadata into a dynamic table or external Excel file. *Tip:* Used to generate live doors, windows, and equipment schedules that dynamically update when drawing geometry changes. |
| `DATALINK` | `DL` | Establishes a live bidirectional data link between an Excel spreadsheet (.xlsx) and an OCS table. *Tip:* Update linked tables after changing Excel data using the DATALINKUPDATE command. |
| `DATALINKUPDATE` | `DLU` | Ribbon: Download from Source. |
| `DDEDIT` | `DE`, `ED` | Ribbon: Edit Text. |
| `DIM` | — | Menu: Dimension. |
| `DIMCENTER` | `DCE` | Menu: Center Mark. |
| `DIMDISASSOCIATE` | — | Removes association between dimensions and their geometry. † |
| `DIMINSPECT` | — | Menu: Inspection. |
| `DIMJOG` | — | Adds a jog to a radius dimension. † |
| `DIMJOGGED` | `DJO`, `JOG` | Menu: Jogged. |
| `DIMJOGLINE` | `DJL` | Menu: Jogged Linear. |
| `DIMTED` | — | Moves or rotates the text of a dimension (alias of DIMTEDIT). † |
| `DIMTEDIT` | — | Ribbon: Dim Text Edit. |
| `FIELD` | — | Inserts dynamic text with a field that updates automatically as object properties or drawing metadata change. *Tip:* Embed dynamic sheet numbers, drawing save dates, viewport scales, or closed polyline calculated areas. |
| `FIELDEVAL` | — | Evaluates and refreshes fields in the drawing. † |
| `FIND` | — | Finds and replaces text across single-line text, MText, block attributes, dimension annotations, and table cells. *Tip:* Check "Match case" or "Find whole words only". Supports project-wid replace in seconds. |
| `LEADER` | `LEAD` | Ribbon: Leader. |
| `MLEADER` | `MLD` | Creates a multileader object connecting text or block callouts to geometry with an arrow and landing. *Tip:* Configure leader arrowhead, landing distance, and default callout blo in MLEADERSTYLE (MLS). |
| `MLEADERALIGN` | `MLA` | Ribbon: Align. |
| `MLEADERCOLLECT` | `MLC` | Ribbon: Collect. |
| `MLEADEREDIT` | `MLE` | Adds leader lines to or removes leader lines from an existing multileader object. *Tip:* Allows pointing one note callout to multiple parts or locations simultaneously. |
| `MLEADERSTYLE` | `MLS` | Menu: Multileader Style.... |
| `MTEXT` | `T`, `MT` | Creates a multiline text object with full paragraph formatting, columns, bullets, and fractions (also alias T). *Tip:* Type %%c for diameter symbol (Ø), %%d for degree symbol (°), and %%p for plus/minus (±) in the in-place editor. |
| `OBJECTSCALE` | — | Ribbon: Add/Delete Scales. |
| `SCALELISTEDIT` | — | Menu: Scale List.... |
| `SPELL` | `SP` | Checks spelling of text, dimensions, block attributes, and table cells in the drawing. *Tip:* Choose custom engineering/architectural dictionaries to avoid false alarms on industry acronyms. |
| `STYLE` | `ST`, `TS` | Creates, modifies, or sets named text styles (fonts, height, oblique angle, true-type vs SHX). *Tip:* Leave text height at 0 in the text style dialog to allow custom heights per dimension style or annotation note. |
| `TABLE` | `TB` | Creates an empty table object in the drawing with rows, columns, formulas, and Excel data links. *Tip:* Link directly to an external Excel spreadsheet (.xlsx) so data updates live when the spreadsheet changes. |
| `TABLEDIT` | — | Edits the contents of a table cell (alias of TABLEEDIT). † |
| `TABLESTYLE` | `TS` | Menu: Table Style.... |
| `TCOUNT` | — | Express Tool that automatically numbers or re-numbers selected text entities sequentially (1, 2, 3...). *Tip:* Supports prefix, suffix, overwrite, and sort by X, Y, or selection order. Perfect for parking stalls and room numbers. |
| `TEXT` | `T`, `DT` | Creates single-line text objects where each line is an independent entity. *Tip:* Use Justify (J) options (MC for Middle Center, TC for Top Center) for clean alignment inside room labels or title blocks. |
| `TEXTMASK` | — | Express Tool that creates a background mask behind single-line or multi-line text to hide underlying linework. *Tip:* Use UNMASK to remove the mask if text is relocated. |
| `TEXTTOFRONT` | — | Brings all text, dimensions, and leaders to the front of all other drawing objects across the drawing. *Tip:* Select "Both" to ensure hatching and filled solids never obscure annotations and dimensions. |
| `TOLERANCE` | `TOL` | Menu: Tolerance.... |
| `TXT2MTXT` | — | Converts multiple single-line text (TEXT/DTEXT) objects into a single cohesive MTEXT paragraph block. *Tip:* Combines fragmented legacy notes imported from older drawings int an easily editable paragraph. |
| `UPDATEFIELD` | — | Manually recalculates and updates fields in selected objects to reflect latest drawing values. *Tip:* Run before issuing prints if FIELDEVAL variable is not set to update upon save or plot. |

### Block Editor & Blocks (30)

| Command | Aliases | Description |
|---|---|---|
| `BACTION` | — | Block Editor: adds an action to a dynamic-block parameter. † |
| `BACTIONBAR` | — | Block Editor: shows the action bar of the selected parameter or action. † |
| `BACTIONSET` | — | Block Editor: defines the objects an action acts on. † |
| `BACTIONTOOL` | — | Block Editor: opens the action tool palette. † |
| `BARRAY` | — | Block Editor: adds an array action. † |
| `BASSOCIATE` | — | Block Editor: associates an action with a parameter. † |
| `BAUTHORPALETTE` | — | Opens the Block Authoring palette. † |
| `BAUTHORPALETTECLOSE` | — | Closes the Block Authoring palette. † |
| `BCONSTRUCTION` | — | Block Editor: toggles construction geometry (excluded from the block when inserted). † |
| `BCYCLEORDER` | — | Block Editor: sets the order in which grips cycle. † |
| `BEDIT_DISCARD` | — | Block Editor internal: closes the editor discarding changes. † |
| `BEDIT_SAVE` | — | Block Editor internal: saves the block and closes the editor. † |
| `BESETTINGS` | — | Opens the Block Editor settings. † |
| `BGRIPSET` | — | Block Editor: sets the grips of a parameter. † |
| `BLOCKMRULIST` | — | Shows the recently used block list. † |
| `BLOCKNAVIGATE` | — | Navigates block-library folders in the blocks palette. † |
| `BLOCKREDEFINEMODE` | — | Sets how a block redefinition is handled on insertion. † |
| `BLOCKSPALETTE` | — | Opens the Blocks palette. † |
| `BLOCKSPALETTECLOSE` | — | Closes the Blocks palette. † |
| `BLOCKSTATE` | — | Opens block visibility states. † |
| `BLOOKUPTABLE` | — | Block Editor: edits the lookup table of a lookup parameter. † |
| `BPARAMETER` | — | Block Editor: adds a parameter to a dynamic block. † |
| `BPARAMETERS` | — | Opens the Block Editor Parameters Manager. † |
| `BPARAMETERSCLOSE` | — | Closes the Block Editor Parameters Manager. † |
| `BTABLE` | — | Block Editor: opens the block properties table. † |
| `BTESTBLOCK` | — | Block Editor: opens a test window to try the block. † |
| `BVHIDE` | — | Ribbon: Make Invisible. |
| `BVMODE` | — | Toggles how hidden objects are shown in the Block Editor. † |
| `BVSHOW` | — | Ribbon: Make Visible. |
| `BVSTATE` | `BVS` | Block Editor: manages visibility states. † |

### Blocks & References (37)

| Command | Aliases | Description |
|---|---|---|
| `-ATTDEF` | `-ATT` | Command-line (no dialog) form of ATTDEF. |
| `-ATTEDIT` | `-ATE`, `ATTE` | Command-line (no dialog) form of ATTEDIT. |
| `-BEDIT` | — | Command-line (no dialog) form of BEDIT. |
| `-BLOCK` | `-B` | Implemented command-line block forms must also validate in ALIASEDIT. |
| `-HYPERLINK` | — | Command-line (no dialog) form of HYPERLINK. |
| `-INSERT` | — | Command-line (no dialog) form of INSERT. |
| `-PDFIMPORT` | — | Command-line (no dialog) form of PDFIMPORT. |
| `-WBLOCK` | `-W` | Command-line (no dialog) form of WBLOCK. |
| `-XREF` | `-XR` | Command-line (no dialog) form of XREF. |
| `ADCENTER` | `ADC` | Menu: Design Center. |
| `ADJUST` | — | Ribbon: Adjust. |
| `ATTDEF` | `ATT` | Defines an attribute template (Tag, Prompt, Default value) to store text data within block definitions. *Tip:* Essential for smart title blocks, room tags, and equipment labels that prompt the user upon block insertion. |
| `ATTEDIT` | `ATE` | Ribbon: Edit\nAttribute. |
| `ATTMAN` | — | Ribbon: Manage. |
| `ATTSYNC` | — | Updates all existing block references with new and revised attribute definitions. *Tip:* Always run ATTSYNC after editing a block in BEDIT if new attributes a not showing up in existing instances. |
| `BATTMAN` | — | Block Attribute Manager: edits attribute properties, prompt order, visibility, and syncs changes to all existing block instances. *Tip:* Use BATTMAN to reorder attributes or change layer/color without exploding or recreating the block. |
| `BEDIT` | `BE` | Opens the Block Editor environment to define dynamic parameters, lookup tables, and actions (stretch, flip, visibility). *Tip:* Close with BCLOSE (BC) and save changes with BSAVE (BS). |
| `BLOCK` | `B` | Defines a block definition from selected objects in the current drawing. *Tip:* Always draw block geometry on Layer 0 with Color and Linetype set t ByBlock so it inherits properties of the inserting layer. |
| `BLOCKPALETTE` | — | Ribbon: Block\nPalette. |
| `CLEAR` | — | Clears the scrollback history buffer of the command line window. |
| `CLIP` | — | Menu: Underlay. |
| `CONTENTBROWSER` | — | Ribbon: Content\nBrowser. |
| `COUNT` | — | Modern interactive Count palette that counts instances of blocks, highlights duplicates, and inserts live schedule tables. *Tip:* Flags overlapping or mirrored blocks automatically and creates live updating table schedules. |
| `EATTEXT` | — | Enhanced attribute data extraction wizard for exporting block attributes and object counts. *Tip:* Predecessor to DATAEXTRACTION; still supported out-of-the-box in a standard OCS installations. |
| `HYPERLINK` | — | Menu: Hyperlink.... |
| `INSERT` | `I` | Inserts a named block or drawing file into the current drawing via the Blocks palette or classic dialog. *Tip:* Use CLASSICINSERT for the legacy modal dialog box if you prefer simple text inputs without opening the palette. |
| `INSERTOBJ` | `IO` | Menu: OLE Object.... |
| `MINSERT` | — | Ribbon: Array Insert. |
| `NCOPY` | — | Copies nested objects out of an Xref, DWF, DGN, or Block directly into the current drawing without exploding. *Tip:* Click on any line inside an external reference to extract and use it directly in your drawing. |
| `PDFIMPORT` | — | Imports vector geometry, TrueType text fonts, and raster images from a vector PDF file directly into OCS DWG entities. *Tip:* Transforms dead vector PDFs back into live editable lines, arcs, layers and polylines. |
| `PDFSHXTEXT` | — | Ribbon: Recognize SHX Text. |
| `SPHERE` | — | Ribbon: Sphere. |
| `WBLOCK` | `W`, `WB` | Writes selected objects or an internal block to a separate external DWG drawing file on disk (Write Block). *Tip:* The gold standard way to export custom components, details, or clea symbols into your team standard block library. |
| `XATTACH` | `XA` | Directly opens the file browser to select and attach an external DWG drawing as an Xref. *Tip:* Faster than opening the Xref manager when quickly bringing in a consultant drawing. |
| `XCLIP` | `XC` | Clips the visible boundary of an external reference (Xref) or block using a rectangular or polygonal border. *Tip:* Allows isolating just one tenant suite or building wing from a massive master site drawing without exploding. |
| `XOPEN` | — | Select an externally referenced drawing (Xref) and opens its source DWG file directly in a new tab. *Tip:* Much faster than navigating through Windows file explorer to find the referenced drawing path. |
| `XREF` | `XR` | Opens the External References palette to attach, reload, unload, detach, or bind external DWG, PDF, DWF, and image files. *Tip:* Choose "Overlay" rather than "Attachment" type for background architectural plans to avoid circular nesting loops. |

### Dimension (19)

| Command | Aliases | Description |
|---|---|---|
| `-DIMSTYLE` | — | Command-line (no dialog) form of DIMSTYLE. |
| `CENTERLINE` | — | Creates associative centerline geometry between two non-parallel or parallel lines. *Tip:* Automatically calculates the midpoint axis between two lines or wall faces and maintains associativity. |
| `CENTERMARK` | — | Creates an associative center mark at the center of a circle or arc. *Tip:* Associative center marks move and scale dynamically when the underlying circle is moved or resized. |
| `DIMALIGNED` | `DAL` | Creates a linear dimension parallel to the angle of the specified extension points. *Tip:* Use for angled walls, rafters, and diagonal distances that are not orthogonal. |
| `DIMANGULAR` | `DAN` | Measures and displays the angle between two selected lines, vertices, or circle sectors. *Tip:* Pick two lines to dimension the acute or obtuse angle between them depending on where you place the dimension text. |
| `DIMARC` | — | Measures and displays the arc length of an arc segment or polyline arc. *Tip:* Displays an arc length symbol above or beside the dimension text to distinguish from linear chord distances. |
| `DIMBASELINE` | `DBA` | Creates a series of stacked dimensions that start from a common baseline extension line. *Tip:* Line spacing between stacked baselines is governed by the DIMDLI dimension style variable. |
| `DIMBREAK` | `DBR` | Breaks or restores dimension lines and extension lines where they cross other dimensions or geometry. *Tip:* Prevents drafting clutter where extension lines cross dimension leade lines. Run Auto for automatic clean gaps. |
| `DIMCONTINUE` | `DCO` | Creates a chained dimension that continues from the second extension line of the previous dimension. *Tip:* Saves enormous time drafting continuous architectural wall strings an column grid dimensions. |
| `DIMDIAMETER` | `DDI` | Creates a diameter dimension for a circle or arc with the diameter symbol (Ø). *Tip:* Standard drafting practice: use diameter (DDI) for full circles and radi (DRA) for partial arcs. |
| `DIMEDIT` | `DED` | Edits dimension text, rotation angle, and oblique extension line angles. *Tip:* Type O (Oblique) to angle extension lines when dimensioning isometri drawings. |
| `DIMLINEAR` | `DLI` | Creates a horizontal or vertical linear dimension. *Tip:* Press Enter at the first prompt to select the object directly instead of picking two extension origin points. |
| `DIMORDINATE` | `DOR` | Creates ordinate dimensions showing X or Y coordinate distances from an established datum origin point. *Tip:* Commonly used in mechanical hole tables and CNC manufacturing coordinates to prevent tolerance stack-up. |
| `DIMOVERRIDE` | — | Clears or applies temporary overrides to system variables for selected dimensions without modifying the style. *Tip:* Useful for quick one-off adjustments like suppressing a single extension line. |
| `DIMRADIUS` | `DRA` | Creates a radius dimension for a circle or arc with an optional center mark. *Tip:* OCS prefixes the text with the standard R symbol. Drag cursor inside outside curve for placement. |
| `DIMREASSOCIATE` | — | Associates or reassociates selected dimensions to geometry points so they update automatically when geometry changes. *Tip:* Fixes yellow exclamation mark badges indicating orphaned or disassociated dimensions. |
| `DIMSPACE` | `DSPACE` | Adjusts the spacing between parallel dimensions or aligns them cleanly. *Tip:* Enter Auto or specify a set spacing distance to make dimension string look publication-grade. |
| `DIMSTYLE` | `D`, `DDIM` | Opens the Dimension Style Manager to create, modify, compare, and set active dimension styles. *Tip:* Use Annotative dimension styles so dimensions automatically scale themselves to fit different paper space viewports. |
| `QDIM` | — | Quickly creates a complete chain or baseline string of dimensions from a group of selected objects in one action. *Tip:* Select multiple vertices or lines across a floorplan; QDIM dimensions selected points in a single click. |

### Draw (79)

| Command | Aliases | Description |
|---|---|---|
| `3DPOLY` | — | Creates a 3D polyline with vertices that can have independent Z elevation coordinates. *Tip:* Unlike standard 2D PLINE, 3D polylines only support straight segment (no arcs) but can freely traverse 3D space. |
| `ARC` | `A` | Creates an arc using 11 different geometric methods (3-point, Start- Center-End, Start-End-Radius, etc.). *Tip:* By default, OCS draws arcs counter-clockwise. Hold Ctrl while draggi to invert the arc direction. |
| `ARC_3P` | — | Arc through three points. † |
| `ARC_CONT` | — | Arc continuing from the end of the last line or arc. † |
| `ARC_CSA` | — | Arc by centre, start point, included angle. † |
| `ARC_CSE` | — | Arc by centre, start point, end point. † |
| `ARC_CSL` | — | Arc by centre, start point, chord length. † |
| `ARC_SCA` | — | Arc by start point, centre, included angle. † |
| `ARC_SCE` | — | Arc by start point, centre, end point. † |
| `ARC_SCL` | — | Arc by start point, centre, chord length. † |
| `ARC_SEA` | — | Arc by start point, end point, included angle. † |
| `ARC_SED` | — | Arc by start point, end point, direction. † |
| `ARC_SER` | — | Arc by start point, end point, radius. † |
| `BCLOSE` | `BC` | Ribbon: Close Block Editor. |
| `BCLOSETESTBLOCK` | — | Ribbon: Close Test Block. |
| `BOUNDARY` | `BO` | Creates a region or closed polyline automatically from an enclosed area by clicking an internal point. *Tip:* Far faster than tracing complex overlapping geometry. Similar to the pick-point behavior in HATCH. |
| `BSAVE` | `BS` | Ribbon: Save Block. |
| `BSAVEAS` | — | Ribbon: Save Block As. |
| `BYLAYER` | — | Ribbon: ByLayer. |
| `CHANGELOG` | — | Displays commit history and recent feature release notes. |
| `CIRCLE` | `C` | Creates a circle using center-radius, center-diameter, 2-point, 3-point, or Tangent-Tangent-Radius (TTR). *Tip:* Default prompt asks for center and radius; enter D to specify diameter instead. Use 2P for fitting between two parallel lines. |
| `CIRCLE_2P` | — | Circle by two diameter end points. † |
| `CIRCLE_3P` | — | Circle through three points. † |
| `CIRCLE_CD` | — | Circle by centre and diameter. † |
| `CIRCLE_TTR` | — | Circle tangent to two objects with a radius. † |
| `CIRCLE_TTT` | — | Circle tangent to three objects. † |
| `COLOR` | `COL` | Menu: Color.... |
| `CONSTRUCTIONLINE` | — | Alias of XLINE. † |
| `COPYCLIP` | `CC` | Copies selected drawing objects into the operating system clipboard buffer (DWG vector + image). |
| `COPYTOLAYER` | — | Menu: Copy Objects to New Layer. |
| `CUTCLIP` | `CX` | Copies selected entities to clipboard and immediately erases them from drawing. |
| `DIVIDE` | `DIV` | Places evenly spaced point objects or blocks along the length of an entity. *Tip:* Does not physically cut the object; it inserts point nodes. Use the Blo (B) option to insert repeating symbols along a curve. |
| `DONATE` | — | Ribbon: Donate. |
| `DONUT` | `DO` | Draws a filled circle or solid wide ring by specifying inside and outside diameters. *Tip:* Set inside diameter to 0 to create a solid filled circular dot (commonly used for electrical terminal points). |
| `ELLIPSE` | `EL` | Creates an ellipse or an elliptical arc using major and minor axes. *Tip:* Type A to create an elliptical arc. Set PELLIPSE system variable to 1 if you want the ellipse created as a true polyline. |
| `ELLIPSE_ARC` | — | Elliptical arc. † |
| `ELLIPSE_AXIS` | — | Ellipse by axis end points. † |
| `FITSPLINE` | — | Alias of SPLINEFIT. † |
| `GRADIENT` | `GD` | Fills an enclosed boundary with a two-color or one-color smooth gradient shading. *Tip:* Used extensively for architectural diagrams, solar shading, and presentation graphics. |
| `GROUP` | `G` | Ribbon: Group. |
| `HATCH` | `H`, `BH` | Fills an enclosed area or selected objects with a hatch pattern, solid color fill, or gradient. *Tip:* Always ensure your boundary is cleanly closed. Use "Pick Points" for internal islands or "Select" to use existing closed polylines. |
| `HELIX` | — | Creates a 2D spiral or 3D spring/helix curve. *Tip:* Specify base radius, top radius, number of turns, and height. Used as paths for SWEEP to create threads and springs. |
| `LAYERS` | `LA` | Ribbon: Layers. |
| `LAYLCK` | — | Menu: Layer Lock. |
| `LAYMATCH` | — | Ribbon: Match Layer. |
| `LAYMCUR` | — | Menu: Make Object's Layer Current. |
| `LAYULK` | — | Menu: Layer Unlock. |
| `LINE` | `L` | Creates a series of contiguous straight 2D or 3D line segments. *Tip:* Press Spacebar or Enter to repeat. Right-click or Enter to exit. Type U while active to undo last vertex. |
| `MEASURE` | `ME` | Places point objects or blocks at specified distance intervals along an entity. *Tip:* Unlike DIVIDE (which divides by count), MEASURE divides by length o segment. Selection end determines starting point. |
| `MLINE` | `ML` | Menu: Multiline. |
| `MULTIPOINT` | — | Draws several points without leaving the command. † |
| `PLINE` | `PL` | Creates a 2D polyline, a single compound object composed of joined line and arc segments with uniform or tapering width. *Tip:* Switch between arc and line sub-modes while drawing by typing A or Use Close (C) to form watertight geometry. |
| `POINT` | `PO` | Creates a single point marker object at specified XYZ coordinates. *Tip:* Run DDPTYPE to change point visual style from invisible single dot to crosshairs, circles, or X marks. |
| `POLY` | — | Alias of POLYGON. † |
| `POLYGON` | `POL` | Creates an equilateral closed polyline with 3 to 1024 sides. *Tip:* Specify Inscribed in circle (I) to measure radius to vertices, or Circumscribed (C) to measure radius to edge midpoints. |
| `POLY_C` | — | Polygon circumscribed about a circle. † |
| `POLY_E` | — | Polygon defined by one edge. † |
| `RAY` | `RAY` | Creates a semi-infinite construction line extending infinitely in one direction from a starting point. *Tip:* Great for sightlines, radial sun-path diagrams, and projection radiatin from a single station point. |
| `RECT` | `REC` | Rectangle by two corners. † |
| `RECTANG` | `REC` | Creates a rectangular closed polyline from two opposing corner points. *Tip:* Type D after placing the first corner to input exact Length and Width dimensions before clicking the quadrant. |
| `RECT_CEN` | — | Rectangle from its centre. † |
| `RECT_ROT` | — | Rotated rectangle. † |
| `REGION` | — | Converts closed 2D shapes (lines, polylines, circles) into a 2D surface region object. *Tip:* Regions support boolean operations (UNION, SUBTRACT, INTERSECT and calculation of mass/area properties. |
| `REPORT` | — | Ribbon: Report. |
| `REVCLOUD` | — | Creates a revision cloud composed of contiguous arc segments to highlight drawing changes. *Tip:* Use the "Arc length" option to scale the cloud scallops to match the sheet viewport scale before placing. |
| `REVCLOUD_FREEHAND` | — | Revision cloud drawn freehand. † |
| `REVCLOUD_POLYGONAL` | — | Revision cloud from polygon points. † |
| `REVCLOUD_RECTANGULAR` | — | Revision cloud from a rectangle. † |
| `REVERSE` | — | Ribbon: Reverse. |
| `SKETCH` | — | Creates a series of freehand line segments as you drag the cursor. *Tip:* Set SKPOLY system variable to 1 so the resulting freehand paths are polylines rather than thousands of individual lines. |
| `SOLID2D` | — | Draws filled planar polygons with triangular or bow-tie vertice Creates solid-filled polygonal triangular and quadrilateral pla faces. |
| `SPLINE` | `SPL` | Creates a smooth curve that passes through or near a sequence of control points (NURBS curve). *Tip:* Toggle Method between "Fit" (curve passes directly through points) and "Control Vertices" (CVs pull the curve smoothly). |
| `SPLINECV` | — | Spline through control vertices. † |
| `SPLINEDIT` | `SPE` | Menu: Spline. |
| `SPLINEFIT` | — | Fit a smooth spline through the selected polyline's points. |
| `TRACE` | — | Legacy precursor to wide polylines (PLINE). Legacy drafting command that creates thick solid line segments mitred endpoints. |
| `UNGROUP` | `UG` | Disbands an object selection group into independent entities. |
| `WIPEOUT` | `WO` | Creates a polygonal background mask object that conceals underlying objects with the current background color. *Tip:* Run WIPEOUT -> Frames -> Off before final plotting to hide boundary outlines while keeping objects concealed underneath. |
| `XLINE` | `XL` | Creates an infinite construction line in both directions. *Tip:* Sub-options include H (Horizontal), V (Vertical), A (Angle), B (Bisect angle), and O (Offset distance). |

### Import, export & references (47)

| Command | Aliases | Description |
|---|---|---|
| `-ATTACH` | — | Command-line (no dialog) form of ATTACH. |
| `-DGNATTACH` | — | Command-line (no dialog) form of DGNATTACH. |
| `-DGNIMPORT` | — | Command-line (no dialog) form of DGNIMPORT. |
| `-DWFATTACH` | — | Command-line (no dialog) form of DWFATTACH. |
| `-IMAGEADJUST` | — | Command-line (no dialog) form of IMAGEADJUST. |
| `-PDFATTACH` | — | Command-line (no dialog) form of PDFATTACH. |
| `-REFEDIT` | — | Command-line (no dialog) form of REFEDIT. |
| `ACISIN` | — | Imports an ACIS (.sat) solid file into the drawing. † |
| `ATTACH` | — | Menu: Attach.... |
| `DGNADJUST` | — | Adjusts fade, contrast and monochrome of a DGN underlay. † |
| `DGNATTACH` | — | Menu: DGN Underlay.... |
| `DGNCLIP` | — | Clips a DGN underlay. † |
| `DGNIMPORT` | — | Imports a DGN file as drawing objects. † |
| `DWFADJUST` | — | Adjusts a DWF underlay. † |
| `DWFATTACH` | — | Menu: DWF Underlay.... |
| `DWFCLIP` | — | Clips a DWF underlay. † |
| `EXPORT` | `EXP` | Menu: Export.... |
| `EXPORTPDF` | `EPDF` | Exports the drawing to PDF. † |
| `EXPORTSTEP` | — | Exports solids to a STEP file. † |
| `EXPORTSTL` | — | Exports solids to STL. † |
| `EXPORTSVG` | — | The same direct export, but the save dialog proposes the SVG device's defaults (`stem.svg`). |
| `EXTERNALREFERENCES` | `ER` | Menu: External References. |
| `EXTERNALREFERENCESCLOSE` | — | Closes the External References palette. † |
| `FBXEXPORT` | — | Export 3D meshes to FBX for visualisation tools. |
| `IMAGE` | — | Attaches or manages raster images (alias of IMAGEATTACH). † |
| `IMAGEADJUST` | `IAD` | IMAGEADJUST: chosen images (pickfirst first), then the dialog. |
| `IMAGEATTACH` | `IAT` | Menu: Raster Image Reference.... |
| `IMAGECLIP` | `ICL` | Menu: Image. |
| `IMAGEEMBED` | — | Embeds an attached image in the drawing. † |
| `IMPORT` | — | Menu: Import.... |
| `IMPORTOBJ` | — | Imports an OBJ mesh file. † |
| `LANDXMLIMPORT` | — | LANDXMLIMPORT <path> — import survey points (LandXML <CgPoint> elements) as Point objects. Reads the coordinate text content (northing easting elevation) → Point at (easting, northing, elev). |
| `OBJIMPORT` | — | Imports an object file. † |
| `OLEFRAME` | — | Sets OLE frame display. † |
| `OLEHIDE` | — | Hides OLE objects. † |
| `OLELINKS` | — | Menu: OLE Links.... |
| `OLEQUALITY` | — | Sets OLE plot quality. † |
| `OLESTARTUP` | — | Sets whether OLE source apps start for plotting. † |
| `PDFADJUST` | — | Adjusts a PDF underlay. † |
| `PDFATTACH` | — | Menu: PDF Underlay.... |
| `PDFCLIP` | — | Clips a PDF underlay. † |
| `REFCLOSE` | — | Closes in-place reference editing. † |
| `REFEDIT` | — | Edits a block or xref in place. † |
| `REFSET` | — | Adds or removes objects from the working set of an in-place edit. † |
| `STEPOUT` | — | Export 3D meshes to STEP AP203 format. |
| `UNDERLAY` | — | Edit properties of selected PDF/DWF/DGN underlay entities. Usage: UNDERLAY FADE <0-80> UNDERLAY CONTRAST <0-100> UNDERLAY ON \| OFF UNDERLAY CLIP ON \| OFF UNDERLAY MONO ON \| OFF. |
| `WMFIN` | — | Imports a Windows metafile (WMF). † |

### Inquiry & Measure (6)

| Command | Aliases | Description |
|---|---|---|
| `DIST` | `DI` | Measures the distance, angle, and delta X/Y/Z coordinate offsets between two points. *Tip:* Use Shift + Right-click to trigger precision object snap overrides (Nod Perpendicular, Midpoint) for measurement. |
| `ID` | `ID` | Displays the precise World Coordinate System (X, Y, Z) coordinates of a selected point. *Tip:* Quick check to ensure that objects are at Z=0.0000 elevation in 2D drawings. |
| `LIST` | `LI` | Displays complete database property data for selected objects in the OCS text window (also alias LS). *Tip:* Displays exact polyline length, vertex coordinates, layer, handle, linetype, and true 3D normal vector. |
| `MASSPROP` | — | Calculates the mass, volume, bounding box, center of gravity (centroid), and moments of inertia for 3D solids or regions. *Tip:* Can write full engineering physical properties analysis to an external .mpr text file. |
| `MEASUREGEOM` | `MEA` | Interactive measurement suite with Quick Mode to display dimensions, distances, and angles dynamically as cursor hovers. *Tip:* Quick mode measures room dimensions and clear spans automatically in real-time as you move the cursor inside geometry. |
| `TIME` | — | Displays drawing creation date, last saved date, total elapsed editing time, and session timer. *Tip:* Check true billable hours and drafting time spent on a project drawing. |

### Layers & Properties (33)

| Command | Aliases | Description |
|---|---|---|
| `-LAYER` | — | Script forms of the table commands. |
| `-LAYOUT` | — | Command-line (no dialog) form of LAYOUT. |
| `-SETBYLAYER` | — | Command-line (no dialog) form of SETBYLAYER. |
| `CECOLOR` | — | Sets or shows the current object colour. † |
| `CETRANSPARENCY` | — | Sets or shows the transparency of new objects. † |
| `CLAYER` | — | System variable getters/setters CLAYER [name] — get or set current layer TEXTSTYLE [name] — already handled above under STYLE SET DIMSTYLE [name] — get or set active dim style LTSCALE [val] — global linetype scale. |
| `COLORSCHEME` | — | Switches the interface colour theme. † |
| `LAYCUR` | — | Changes the layer property of selected objects to the currently active layer. *Tip:* Quickly stamps selected stray objects onto whatever layer you are actively drafting on. |
| `LAYDEL` | — | Deletes an un-purgeable layer and all objects residing on it, even if referenced in blocks. *Tip:* Use with extreme caution. Force-removes stubborn layers that standa PURGE cannot delete. |
| `LAYER` | `LA`, `LAYERS` | Opens the Layer Properties Manager palette to manage layer colors, linetypes, lineweights, transparency, and viewport overrides. *Tip:* Use layer filters (group or property filters) to manage complex drawin with hundreds of AIA/ISO CAD standard layers. |
| `LAYERSTATE` | — | Menu: Layer States Manager.... |
| `LAYFRZ` | — | Freezes the layers of selected objects, suppressing them from regenerating and improving performance. *Tip:* In paper space viewports, freezes objects in the active viewport only (VP Freeze) without affecting other sheets. |
| `LAYISO` | — | Isolates the layers of selected objects by turning off, freezing, or locking/fading all other layers in the drawing. *Tip:* Run S (Settings) to choose whether non-selected layers are completel turned Off or merely Locked and Faded. |
| `LAYMCH` | — | Changes the layer of selected objects to match the layer of a destination object. *Tip:* Select objects to move, type LAYMCH, and click an object already on the desired target layer. |
| `LAYMRG` | — | Merges all objects on selected layers into a single target layer and automatically deletes the source layers. *Tip:* Consolidates duplicate or redundant contractor layers without losing single line or block. |
| `LAYOFF` | — | Turns off the layer of any object you click directly in the viewport. *Tip:* Clicking any element on the screen turns its layer off without needing to hunt for the name in the layer dropdown. |
| `LAYON` | — | Turns on all layers in the drawing. *Tip:* Instantly illuminates every layer that was turned off by LAYOFF or manual layer manager settings. |
| `LAYOUTMANAGER` | — | Opens the layout manager. † |
| `LAYOUTPANEL` | — | Toggles visibility of Paper Space layout management panel. |
| `LAYTHW` | — | Thaws all frozen layers in the drawing. *Tip:* Global counterpart to LAYFRZ. |
| `LAYTRANS` | — | Translates layer names and standards in the current drawing to match another company or AIA layer standard file. *Tip:* Save translation mappings to a .dws or .dwg standard file for recurrin project conversion. |
| `LAYUNISO` | — | Restores all layers that were turned off, frozen, or locked by the most recent LAYISO command. *Tip:* Quickly returns your workspace to its full visible state after isolated editing. |
| `LAYWALK` | — | Opens an interactive layer inspector dialog to quickly walk through and view objects on individual layers. *Tip:* The ultimate tool for CAD quality assurance and checking what rogue entities sit on unexpected layers. |
| `LINETYPE` | `LT` | Menu: Linetype.... |
| `MATCHPROP` | `MA` | Applies the properties (layer, color, linetype, text style, dimension style) of a selected source object to other target objects. *Tip:* Type S (Settings) after selecting source to toggle exactly which properties to copy (e.g. only layer without changing hatch scale). |
| `PROPERTIES` | `CH`, `MO`, `PR`, `PROPS` | Opens the Properties palette (Ctrl+1) to inspect and modify all geometric and styling parameters of selected objects. *Tip:* Aliases CH, MO also open this palette. Use the Quick Select (funnel icon) button at top-right for precision filtering. |
| `QLAYER` | `VV` | Quick layer toggle for the selected object. † |
| `QPLOCATION` | — | Sets where the Quick Properties panel appears. † |
| `QPMODE` | — | Sets Quick Properties display mode. † |
| `QUICKPROPERTIES` | — | Toggle the floating quick-properties readout. |
| `SETBYLAYER` | — | Changes the property overrides of selected objects to ByLayer (color, linetype, lineweight, material). *Tip:* Essential when receiving messy contractor drawings with hard-coded colors that defy layer management. |
| `TRANSPARENCY` | — | Menu: Transparency. |
| `VPLAYER` | — | Sets viewport-specific layer visibility (VP Freeze, VP Thaw, VP Color) without affecting other viewports. *Tip:* Essential for showing plumbing annotations in one viewport while hiding them in the electrical layout on the same sheet. |

### Layout (1)

| Command | Aliases | Description |
|---|---|---|
| `PRINTALL` | — | Triggers batch print sequence for all sheets in drawing. |

### Modify (64)

| Command | Aliases | Description |
|---|---|---|
| `-OVERKILL` | — | Command-line (no dialog) form of OVERKILL. |
| `3DALIGN` | — | Aligns selected objects in 3D to a destination using up to three source and destination point pairs. † |
| `3DARRAY` | — | Multi-level structural repetition (floors, grids). Creates a 3-dimensional rectangular array (rows, columns, level polar array around a 3D axis. |
| `3DFACE` | — | Fundamental legacy 3D surface facet entity. Creates a 3-sided or 4-sided surface planar polygon anywhere in space with optional invisible edges. |
| `3DMESH` | — | Builds complex organic terrain or curved 3D mesh shells. Creates a free-form 3D polygon mesh defined by an M x N grid of coordinate vertices. |
| `3DMIRROR` | — | 3DMIRROR [X\|Y\|Z] — add a mirror of the selected solid across a plane. |
| `3DMOVE` | — | Interactive 3D transformation triad with axis locking. Displays a 3D move coordinate gizmo to constrain translation st along X, Y, or Z axes. |
| `3DROTATE` | — | Rotates solids around X, Y, or Z gimbal rings. Displays 3D rotation gizmo rings to constrain rotation around a specific coordinate axis in space. |
| `ALIGN` | `AL` | Aligns objects with other objects in 2D or 3D using 1, 2, or 3 source/destination reference point pairs. *Tip:* Combines Move, Rotate, and Scale into a single operation. At the prompt "Scale objects based on alignment points?", select Yes. |
| `ALIGN3D` | — | Alias token for 3DALIGN. † |
| `ALIGNBOTTOM` | — | Aligns the selected objects to the bottom edge of their combined extents. † |
| `ALIGNHCENTER` | — | Aligns the selected objects on a common horizontal centre line. † |
| `ALIGNLEFT` | — | Aligns the selected objects to the left edge of their combined extents. † |
| `ALIGNRIGHT` | — | Aligns the selected objects to the right edge of their combined extents. † |
| `ALIGNTOP` | — | Aligns the selected objects to the top edge of their combined extents. † |
| `ALIGNVCENTER` | — | Aligns the selected objects on a common vertical centre line. † |
| `ARRAY` | `AR` | Creates copies of objects arranged in a pattern: Rectangular, Polar (circular), or along a Path curve. *Tip:* Modern arrays are associative: you can click any element later to edit rows, columns, levels, and spacing dynamically. |
| `ARRAYEDIT` | — | Edits associative array objects and their source elements without exploding. *Tip:* Allows editing individual items, replacing instances, or adding/deletin rows and columns in existing arrays. |
| `BLEND` | `BLE` | Creates a smooth spline curve between the endpoints of two non- touching curves or lines with G1 or G2 continuity. *Tip:* Provides superior aesthetic transitions compared to standard fillets when designing aerodynamic or product contours. |
| `BREAK` | `BR` | Breaks the selected object between two specified points or splits it at a single point. *Tip:* To break an object cleanly at one point, select the object, type F (First point), click the split point, and enter @. |
| `CHAMFER` | `CHA` | Bevels or chamfers the corners of two intersecting lines by distances or angle. *Tip:* Specify distance with D. Distance 0,0 produces an instant sharp corne just like Fillet R 0. |
| `CHSPACE` | — | Transfers selected objects between Model Space and Paper Space while automatically scaling them to match the viewport. *Tip:* Allows moving title blocks, notes, or detail callouts between paper space and model space with zero scale distortion. |
| `COPY` | `CO`, `CP` | Duplicates selected objects across one or multiple destination points (also alias CP). *Tip:* OCS defaults to multiple copy mode. Type A while copying to quickly create an array of copies along a line. |
| `COPYBASE` | — | Menu: Copy with Base Point. |
| `DRAWORDER` | `DR` | Changes the visual display and plotting order of overlapping objects (Bring to Front, Send to Back, Above, Under). *Tip:* Use "Bring to front" for text and dimensions over hatch patterns. Use TEXTTOFRONT to handle all annotations at once. |
| `DRAWORDER_ABOVE` | — | Draw order: puts selected objects above a reference object. † |
| `DRAWORDER_BACK` | — | Draw order: sends selected objects to the back. † |
| `DRAWORDER_FRONT` | — | Draw order: brings selected objects to the front. † |
| `DRAWORDER_UNDER` | — | Draw order: puts selected objects under a reference object. † |
| `ERASE` | `E` | Removes selected objects from a drawing. *Tip:* Type L after starting ERASE to delete the Last created object. Type P select Previous selection set. |
| `EXPLODE` | `X` | Breaks a compound object (block, polyline, hatch, dimension, table) into its component elements. *Tip:* Warning: Exploding attributed blocks turns attributes into unassigned raw tag definitions; use BURST instead to preserve values. |
| `EXTEND` | `EX` | Extends objects to meet the boundary edges of other objects. *Tip:* Hold Shift while extending to instantly switch into Trim mode without exiting the command. |
| `EXTRIM` | — | Express Tool that trims all intersecting geometry inside or outside a specified boundary polygon in one click. *Tip:* Select cutting boundary, then click on the side (inside or outside) that you want trimmed away instantly. |
| `FILLET` | `F` | Rounds or fillets the edges of two intersecting objects with a specified radius. *Tip:* Type R -> 0 for a sharp 90-degree corner cleanup between crossing o disconnected lines. Use P to fillet an entire polyline at once. |
| `FILTER` | `FI` | Opens the Object Selection Filter dialog to build complex multi-criteria boolean search filters. *Tip:* Create and name reusable filters (e.g., "All Red Lines on Layer WALLS for complex drawings. |
| `FLATTEN` | — | Converts 3D geometry and rogue non-zero Z elevations into 2D flat projections on the current UCS plane. *Tip:* Essential when working with survey or Revit DWG exports where lines sit at varying elevations and refuse to fillet or hatch. |
| `HATCHEDIT` | `HE` | Modifies an existing hatch pattern, angle, scale, origin point, or boundary associativity. *Tip:* Double-clicking any hatch pattern in the drawing automatically launches this command or the Hatch Editor contextual ribbon. |
| `HATCHTOBACK` | `HB` | Sends all hatch objects behind all other drawing geometry automatically. *Tip:* The counterpart to TEXTTOFRONT. Instantly fixes drawings where hatching covers walls, lines, or notes. |
| `JOIN` | `J` | Combines collinear lines, arcs, polylines, and splines into a single unified object. *Tip:* Objects must have touching endpoints or be collinear. Quickly turns fragmented lines into a single continuous polyline. |
| `JUSTIFYTEXT` | — | Menu: Text Justify. |
| `LENGTHEN` | `LEN` | Changes the length of objects and the included angle of arcs. *Tip:* Options include Delta (DE), Percent (P), Total (T), and Dynamic (DY). Total lets you set the final exact length directly. |
| `MIRROR` | `MI` | Creates a mirrored duplicate of selected objects across a 2-point reflection axis. *Tip:* Set MIRRTEXT system variable to 0 to prevent text from being mirrore backwards during reflection. |
| `MIRROR3D` | — | Mirrors objects about a plane in 3D. † |
| `MOVE` | `M` | Moves selected objects a specified distance in a specified direction. *Tip:* Use object snaps (OSNAP) for precision basepoints. Type @10,0 to move exactly 10 units right along the X axis. |
| `NCOPYALL` | — | Copies nested objects from xrefs/blocks into the current drawing. † |
| `OFFSET` | `O` | Creates concentric circles, parallel lines, and parallel curves at a designated offset distance. *Tip:* Use "Through" (T) option to offset through a specific cursor point rather than typing a numeric distance. Use E to erase source. |
| `OOPS` | — | Restores objects erased by the most recent ERASE command without undoing other subsequent drawing commands. *Tip:* Unlike UNDO (which rolls back everything in sequence), OOPS specifically restores only deleted entities while preserving newer work. |
| `OVERKILL` | — | Removes duplicate or overlapping lines, arcs, and polylines, and combines partially overlapping or collinear segments. *Tip:* Indispensable CAD cleanup tool. Dramatically shrinks drawing file size and prevents double-burns in CNC/laser cutting. |
| `PEDIT` | `PE` | Edits 2D polylines, joined 3D polylines, and 3D polygon meshes. *Tip:* Type M to select multiple lines and convert them all into polylines simultaneously, then type J (Join) with a fuzz distance. |
| `QSELECT` | `QS` | Quick Select: builds a filtered selection set based on object type, color, layer, linetype, or attribute value. *Tip:* Select all circles with radius > 5, or all text on a specific layer throughout the entire drawing in seconds. |
| `ROTATE` | `RO` | Rotates objects around a base point by angle or reference angle. *Tip:* Use the Reference (R) sub-option to align an object to an existing angled geometry without calculating the angle manually. |
| `SCALE` | `SC` | Enlarges or reduces selected objects proportionally in X, Y, and Z axes. *Tip:* Use Reference (R) option: pick an existing distance and type the desired new target length. Type C to scale a copy. |
| `SCALETEXT` | — | Menu: Text Scale. |
| `SELECTSIMILAR` | — | Finds and selects all objects in the drawing that match the properties (layer, color, type, block name) of chosen object. *Tip:* Type SE (Settings) to configure criteria: layer, linetype, block name, color, style, or lineweight. |
| `STRETCH` | `S`, `SS` | Stretches lines, polylines, dimensions, and curves crossing a crossing window. *Tip:* CRITICAL: You must use a Green Crossing Window (drag right-to-left) across the vertices you wish to move. |
| `TCASE` | — | Text tools: case, mask, width fit, sequential numbering, arc layout. |
| `TEDIT` | — | Alias of TEXTEDIT. † |
| `TEXTALIGN` | — | Aligns text objects to each other. † |
| `TEXTEDIT` | `ED` | Menu: Text Edit.... |
| `TEXTEDITMODE` | — | Sets whether the text editor repeats for each text object. † |
| `TEXTFIT` | — | (text width fit) TEXTFIT <width> — adjust the width factor of selected single-line text so its rendered width matches the target. |
| `TEXTTOBACK` | — | Sends all text to the back of the draw order. † |
| `TORIENT` | — | Rotates text to be readable. † |
| `TRIM` | `TR` | Trims objects to meet the cutting edges of other objects. *Tip:* In modern OCS Quick Mode, simply click or fence-drag across any segment. Hold Shift while trimming to switch to Extend. |

### Parametric (58)

| Command | Aliases | Description |
|---|---|---|
| `-PARAMETERS` | — | Command-line (no dialog) form of PARAMETERS. |
| `ACONSTRAINT` | — | Ribbon: Angle. |
| `AUTOCONSTRAIN` | — | Menu: AutoConstrain. |
| `CCONSTRAINT` | — | Ribbon: Coincident. |
| `CONSTRAINTBAR` | — | Menu: Select Objects. |
| `CONSTRAINTSETTINGS` | — | Menu: Constraint Settings.... |
| `CPCONSTRAINT` | — | Ribbon: Center Point. |
| `CREATESKETCH` | — | Open and close a sketch: a drawing plane, a square-on view and snapping, held together until the profile is done. |
| `DCALIGNED` | — | Menu: Aligned. |
| `DCANGULAR` | — | Menu: Angular. |
| `DCCONVERT` | — | Menu: Convert. |
| `DCDIAMETER` | — | Menu: Diameter. |
| `DCDISPLAY` | — | Menu: Select Objects. |
| `DCFORM` | — | Parametric: switches a dimensional constraint between annotational and dynamic form. † |
| `DCHIDE` | — | Parametric: hides selected dimensional constraints. † |
| `DCHIDEALL` | — | Parametric: hides all dimensional constraints. † |
| `DCHORIZONTAL` | — | Menu: Horizontal. |
| `DCLINEAR` | — | Parametric: linear dimensional constraint. † |
| `DCONSTRAINT` | — | Ribbon: Distance. |
| `DCRADIUS` | — | Menu: Radius. |
| `DCSHOW` | — | Parametric: shows selected dimensional constraints. † |
| `DCSHOWALL` | — | Parametric: shows all dimensional constraints. † |
| `DCVERTICAL` | — | Menu: Vertical. |
| `DELCONSTRAINT` | — | Menu: Delete Constraints. |
| `DIMCONSTRAINT` | — | Parametric: applies a dimensional constraint (type chosen by option). † |
| `ECONSTRAINT` | — | Ribbon: Equal. |
| `EDCONSTRAINT` | — | Ribbon: Equal Distance. |
| `FINISHSKETCH` | — | Ends a sketch started with CREATESKETCH. † |
| `FXCONSTRAINT` | — | Ribbon: Fix. |
| `GCCOINCIDENT` | — | Menu: Coincident. |
| `GCCOLLINEAR` | — | Menu: Collinear. |
| `GCCONCENTRIC` | — | Menu: Concentric. |
| `GCEQUAL` | — | Menu: Equal. |
| `GCFIX` | — | Menu: Fix. |
| `GCHIDE` | — | Parametric: hides geometric constraint bars on selected objects. † |
| `GCHIDEALL` | — | Parametric: hides all geometric constraint bars. † |
| `GCHORIZONTAL` | — | Menu: Horizontal. |
| `GCPARALLEL` | — | Menu: Parallel. |
| `GCPERPENDICULAR` | — | Menu: Perpendicular. |
| `GCRESET` | — | Parametric: removes constraints from the selected objects. † |
| `GCSHOW` | — | Parametric: shows geometric constraint bars on selected objects. † |
| `GCSHOWALL` | — | Parametric: shows all geometric constraint bars. † |
| `GCSMOOTH` | — | Menu: Smooth (G2). |
| `GCSYMMETRIC` | — | Menu: Symmetric. |
| `GCTANGENT` | — | Menu: Tangent. |
| `GCVERTICAL` | — | Menu: Vertical. |
| `GEOMCONSTRAINT` | — | Parametric: applies a geometric constraint (type chosen by option). † |
| `LCONSTRAINT` | — | Ribbon: Colinear. |
| `MPCONSTRAINT` | — | Ribbon: Midpoint. |
| `NRCONSTRAINT` | — | Ribbon: Normal. |
| `OCCONSTRAINT` | — | Ribbon: Point on Curve. |
| `PARAMETERS` | — | Menu: Parameters Manager. |
| `PARAMETERSCLOSE` | — | Closes the Parameters Manager. † |
| `PCONSTRAINT` | — | Ribbon: Parallel. |
| `QCONSTRAINT` | — | Ribbon: Perpendicular. |
| `SYCONSTRAINT` | — | Ribbon: Symmetric. |
| `TCONSTRAINT` | — | Ribbon: Tangent. |
| `VCONSTRAINT` | — | Ribbon: Vertical. |

### Plot & print (15)

| Command | Aliases | Description |
|---|---|---|
| `-PLOT` | — | Command-line (no dialog) form of PLOT. |
| `-PSETUPIN` | — | Command-line (no dialog) form of PSETUPIN. |
| `BACKGROUNDPLOT` | — | Bit 1: single plots run in the background; bit 2: batch plots. |
| `PLOT` | — | Menu: Plot.... |
| `PLOTOFFSET` | — | Plot preferences and the current-layout variables (commands/plotvars.rs). |
| `PLOTROTMODE` | — | Sets plot rotation behaviour. † |
| `PLOTSTYLE` | — | Menu: Plot Style.... |
| `PLOTSTYLEEDITOR` | — | Opens the plot style editor. † |
| `PLOTSTYLEPANEL` | — | Opens the plot style panel. † |
| `PLOTTRANSPARENCYOVERRIDE` | — | Controls whether plots use object transparency. † |
| `PLOTWINDOW` | `PW` | Plots a window selected in the drawing. † |
| `PRINT` | — | Plots the current layout (alias of PLOT). † |
| `PRINTERS` | — | What the system reports about printing, on the command line: the default printer, every printer it lists (or why it could not), and how a plot will reach them. The report a user pastes into a bug report instead of a screenshot. |
| `PSETUPIN` | — | Imports a page setup from another drawing. † |
| `QUICKPRINT` | `QP` | Sends active display window directly to default printer driver. |

### Point clouds (14)

| Command | Aliases | Description |
|---|---|---|
| `-PCEXTRACTSECTION` | — | Command-line (no dialog) form of PCEXTRACTSECTION. |
| `-POINTCLOUDATTACH` | — | Command-line (no dialog) form of POINTCLOUDATTACH. |
| `LAS` | — | Attaches a LAS point cloud. † |
| `PCEXTRACTCENTERLINE` | — | Point cloud: extracts a centreline. † |
| `PCEXTRACTCORNER` | — | Point cloud: extracts a corner. † |
| `PCEXTRACTEDGE` | — | Point cloud: extracts an edge. † |
| `PCEXTRACTSECTION` | — | Point cloud: extracts a section. † |
| `POINTCLOUDATTACH` | — | Attaches a point cloud. † |
| `POINTCLOUDCOLORMAP` | — | Point cloud: colour mapping. † |
| `POINTCLOUDCROP` | — | Point cloud: crops. † |
| `POINTCLOUDMANAGER` | — | The Point Cloud Manager docks on the right like External References and opens expanded. |
| `POINTCLOUDMANAGERCLOSE` | — | Closes the point cloud manager. † |
| `POINTCLOUDSTYLIZE` | — | Point cloud: sets the display style. † |
| `POINTCLOUDUNCROP` | — | Point cloud: removes crop. † |

### Sheet sets (7)

| Command | Aliases | Description |
|---|---|---|
| `NEWSHEETSET` | — | Menu: New Sheet Set.... |
| `OPENSHEETSET` | — | Menu: Open Sheet Set.... |
| `SHEETSETHIDE` | — | Hides the Sheet Set Manager. † |
| `SSMAUTOOPEN` | — | Sheet set: opens the manager automatically. † |
| `SSMPOLLTIME` | — | Sheet set: status polling interval. † |
| `SSMSHEETSTATUS` | — | Sheet set: sheet status display. † |
| `SSMSTATE` | — | Sheet set: manager visibility state. † |

### SpaceMouse (6)

| Command | Aliases | Description |
|---|---|---|
| `SPACEMOUSE` | — | Opens the 3Dconnexion SpaceMouse settings. † |
| `SPACEMOUSE3D` | — | SpaceMouse: 3D navigation mode. † |
| `SPACEMOUSEAUTO` | — | SpaceMouse: automatic mode. † |
| `SPACEMOUSEPAN` | — | SpaceMouse: pan only. † |
| `SPACEMOUSEPANZOOM` | — | SpaceMouse: pan and zoom. † |
| `SPACEMOUSEPAUSE` | — | SpaceMouse: pauses input. † |

### System & Utilities (64)

| Command | Aliases | Description |
|---|---|---|
| `-PURGE` | — | Command-line (no dialog) form of PURGE. |
| `ABOUT` | — | Displays application build version, commit hash, and engine credits. |
| `AI_EDITCUSTFILE` | — | OCS system command that prompts for and opens customization files (such as ocad.pgp) directly in Notepad. *Tip:* Type AI_EDITCUSTFILE -> enter ocad.pgp. Instantly opens the active user PGP file in Notepad without searching folder paths. |
| `ALIASEDIT` | — | Express Tools graphical dialog box to view, add, edit, and delete command aliases without manual text file editing. *Tip:* Available in full OCS. Modifies ocad.pgp automatically and reinitializes on save. |
| `APPLOAD` | — | Loads AutoLISP (.lsp), ObjectARX (.arx), VBA, and .NET applications into OCS. *Tip:* Add routinely used .lsp scripts to the "Startup Suite" briefcase icon to auto-load every time OCS starts. |
| `AUDIT` | — | Evaluates the database integrity of a drawing and corrects internal errors and orphaned pointers. *Tip:* Choose "Y" at the "Fix any errors detected?" prompt. Run regularly on large, sluggish, or crash-prone drawings. |
| `CLOSE` | — | Closes the current drawing document tab. |
| `CLOSEALL` | — | Closes all open drawings, prompting to save changes on any modified documents. *Tip:* Use when finishing a project session to close all tabs quickly instead o clicking each X. |
| `COMPARE` | — | Visually compares two revisions of a drawing and color-codes added (green), deleted (red), and unchanged objects. *Tip:* Modern OCS tool for instant design revision tracking and verifying consultant submittals. |
| `CUI` | — | Customizes User Interface: edits ribbons, toolbars, mouse buttons, keyboard shortcuts, and double-click actions. *Tip:* Manage custom enterprise workspaces and transfer ribbon panels between different machines. |
| `CUIEXPORT` | — | Ribbon: Export. |
| `CUIIMPORT` | — | Ribbon: Import. |
| `CUILOAD` | — | Loads a customization (CUI) file. † |
| `DBLIST` | — | List data for every entity in the drawing (LIST over the whole database rather than the current selection). |
| `DSETTINGS` | `DS` | Menu: Drafting Settings.... |
| `DWGPROPS` | — | Sets and displays file properties (Title, Subject, Author, Keywords, Custom fields) of the current drawing. *Tip:* Custom fields can be dynamically referenced by fields in title blocks and Sheet Sets. |
| `ETRANSMIT` | `ZIP` | Packages a drawing and all its associated external references, fonts, plot styles (CTB), and images into a single zip file. *Tip:* The standard protocol before emailing drawings to clients or contractors to ensure no missing Xrefs or fonts. |
| `EXIT` | — | Exits the application window after confirming unsaved changes. |
| `FILEASSOC` | — | Registers the app as the handler of DWG/DXF files. † |
| `FINDNONPURGEABLE` | — | Ribbon: Find Non-\nPurgeable Items. |
| `GEOGRAPHICLOCATION` | — | Assigns real-world GIS coordinates and loads live aerial satellite maps directly under your CAD geometry. *Tip:* Enables accurate civil, site layout, and sun-study simulations aligned with real geographic coordinates. |
| `HYPERLINKBASE` | — | Sets the base path for relative hyperlinks. † |
| `HYPERLINKOPTIONS` | — | Sets hyperlink cursor and tooltip display. † |
| `IM` | — | Alias of IMAGE. † |
| `ISAVEBAK` | — | Dispatched all along, but absent from the registry, so command-line completion never offered them. |
| `LIMITS` | — | Menu: Drawing Limits. |
| `MEASUREANGLE` | — | Measures an angle (MEASUREGEOM option). † |
| `MEASURERADIUS` | — | Measures a radius (MEASUREGEOM option). † |
| `MULTIPLE` | — | Repeats the next specified command automatically without needing to press Spacebar or Enter between repetitions. *Tip:* Type MULTIPLE -> CIRCLE; now draw circle after circle continuously. Press Esc when finished. |
| `NEW` | — | Creates a new drawing document from default template. |
| `OPEN` | — | Opens an existing DXF/DWG file from storage. |
| `OPTIONS` | — | Opens the comprehensive OCS Options dialog to configure file paths, display colors, crosshair size, open/save defaults, and plotting. *Tip:* Set Crosshair size to 100 on the Display tab for full-screen drafting crosshairs favored by veteran CAD drafters. |
| `PAGESETUP` | — | Configures paper sheet sizes, plot device drivers, CTB/STB pen tables, and orientation for each layout. *Tip:* Apply named page setups across your Sheet Set Manager for consistent company standard deliverables. |
| `PUBLISH` | — | Batch plots and publishes multi-sheet drawing sets to PDF, DWF, or physical plotters. *Tip:* Save sheet list descriptions (.dsd) so you can reprint an entire 50-she permit set in a single background click. |
| `PURGE` | `PU` | Removes unused named items (blocks, layers, linetypes, dimension styles, materials) to reduce file size. *Tip:* Check "Purge nested items" and click Purge All. Drastically reduces fil size and speeds up save and transmission times. |
| `QL` | — | Alias of QLEADER. † |
| `QLEADER` | `LE` | Quick leader (same as LEADER). |
| `QNEW` | — | QNEW starts from the default template (Options > Files); NEW asks which template to start from (#1635). |
| `QUICKCALC` | `QC` | Opens the QuickCalc interactive scientific and geometric calculator palette (Ctrl+8). *Tip:* Directly reads distances, coordinates, and angles from drawing object and converts units (e.g. feet to meters). |
| `QUIT` | — | Synonym for EXIT. |
| `RECOVER` | — | Repairs and opens a severely damaged or corrupted DWG drawing file that refuses to open normally. *Tip:* The first line of defense when a drawing crashes on opening with an internal error. |
| `RECOVERALL` | — | Repairs and opens a corrupted drawing file and all of its nested attached external references (Xrefs). *Tip:* Use when opening complex multi-sheet plans with dozens of linked consultant drawings. |
| `REDO` | — | Reverses the last UNDO. † |
| `RIBBON` | — | Shows the ribbon (RIBBONCLOSE hides it). † |
| `RIBBONCLOSE` | — | Closes the ribbon. † |
| `SAVE` | — | Saves the current drawing to disk. |
| `SAVEALL` | — | Saves all currently open drawing windows and tabs simultaneously without closing them. *Tip:* Silently runs QSAVE on all named files; prompts for file path only on newly created unsaved drawings. |
| `SAVEAS` | `SA` | Saves current drawing with a new file name or location. |
| `SCRIPT` | — | Executes a sequential command script (.scr) file for batch automated tasks. *Tip:* Used with batch utility tools like ScriptPro to process hundreds of DWGs overnight (e.g. Purge, Audit, Save). |
| `SELH` | — | Alias of SELHANDLES. † |
| `SELHANDLES` | — | Report the current selection for diagnostics: the active space (Model / which paper layout), a per-type and per-block breakdown, and the raw comma-separated hex handle list (so what renders on screen can be compared against the file). |
| `SELSIM` | — | Alias of SELECTSIMILAR. † |
| `SETVAR` | `SET` | Lists or changes the value of OCS system variables directly from the command prompt. *Tip:* Type ? to inspect the current state and description of all loaded syste variables. |
| `SHEETSET` | `SSM` | Opens the Sheet Set Manager palette (Ctrl+4) to manage multi-sheet drawing projects, automated callout numbers, and sheet lists. *Tip:* Automates project-wide sheet numbering, cover sheet indices, and automated title block revisions. |
| `SHORTCUTS` | — | Opens shortcut manager and keyboard keybindings dialog. |
| `STYLESMANAGER` | — | Menu: Plot Style Manager.... |
| `UNDO` | `U` | Reverses the last action. † |
| `UNITS` | `UN` | Controls coordinate and angle display formats (Architectural, Decimal, Engineering) and precision. *Tip:* Crucial setting: ensure Insertion Scale units match your project standard (e.g. Inches vs Millimeters) to avoid 25.4x scaling errors upon Xref insertion. Combines two or more 3D solids, surfaces, or 2D regions into a single composite solid. |
| `UOSNAP0` | — | User object-snap option off. † |
| `UOSNAP1` | — | User object-snap option on. † |
| `USERI` | — | Edit a USERI1..USERI5 / USERR1..USERR5 slot. Lets the user store drawing-scoped scalars (and save them through round-trip) even though we don't have a LISP / DIESEL runtime yet. USERI 1 42 → header.user_int1 = 42 USERR 3 1.5e-3 → header.user_real3 = 0.0015. |
| `USERR` | — | User real variable (USERR1–5). † |
| `WEBVERSION` | — | Opens the web version. † |
| `XDATA` | — | Read/write extended entity data XDATA LIST — show all xdata records on selected entities XDATA SET <app> <str> — append a string xdata value for <app> XDATA CLEAR — remove all xdata from selected entities XDATA CLEAR <app> — remove xdata for a specific application. |

### View & Navigation (47)

| Command | Aliases | Description |
|---|---|---|
| `-VIEW` | — | -VIEW: the command-line form of VIEW (modules/view/view_cmd.rs). The command gathers the input; `-VIEW >` carries out what it gathered. A whole line (`-VIEW _O _FRONT`) feeds its words to the command one by one, as typed. |
| `3DORBIT` | `3O`, `ORBIT` | Rotates the 3D view interactively in 3D space around a target center point. *Tip:* Hold Shift + Middle Mouse Wheel to orbit dynamically in 3D without invoking the command prompt. |
| `CASCADE` | — | Arranges open drawing document windows in a cascaded stack. |
| `CLEANSCREEN` | — | Menu: Clean Screen. |
| `COUNTLIST` | — | Ribbon: Count. |
| `CTAB` | — | Sets or shows the current layout tab. † |
| `CTABLESTYLE` | `CT` | Dispatched commands that the alias file names (#2452). |
| `DVIEW` | `DV` | Dynamic view: camera/target, distance, clipping and perspective. † |
| `FILETAB` | — | Ribbon: File\nTabs. |
| `FROM` | — | Object snap: offset from a reference point. † |
| `GRAPHICATTRIBUTES` | — | Ribbon: Graphic Attributes. |
| `GRID` | — | Drafting aids — same toggles the status-bar pills drive, also reachable by name from the command line. GRID — show / hide the reference grid. |
| `HORIZONTAL` | — | Ribbon: Tile\nHoriz.. |
| `ISOPLANE` | — | Cycle the isometric drafting axis pair (F5). |
| `LAYOUTTAB` | — | Toggles display of bottom layout tab strip. |
| `M2P` | — | Object snap: midpoint between two points. † |
| `MSPACE` | `MS` | Switches from paper space to model space through a layout viewport. † |
| `MTP` | — | Object snap: midpoint between two points (alias of M2P). † |
| `MVIEW` | `MV` | Creates and controls rectangular, polygonal, or object-based layout viewports in Paper Space. *Tip:* Always lock viewport display (Display Locked = Yes) in the Properties palette once scale is set to prevent accidental scrolling. |
| `MVSETUP` | — | Sheet border in model space, viewports on a layout. |
| `NAVVCUBE` | `CUBE` | Ribbon: View\nCube. |
| `ORTHO` | — | Ribbon: Ortho. |
| `OSNAP` | `OS` | OSNAP opens it on the Object Snap tab. |
| `PAN` | `P` | Shifts the display view without changing the magnification level. *Tip:* Most drafters click and drag the middle mouse scroll wheel button directly instead of typing the command. |
| `PERSP` | — | Ribbon: Persp. |
| `PERSPECTIVE` | — | Toggles perspective projection. † |
| `PLAN` | — | Displays an orthogonal plan view of the drawing relative to the Current UCS or World Coordinate System (WCS). *Tip:* Type PLAN -> C to rotate the screen so your custom angled UCS sits flat horizontally on your monitor. |
| `POLAR` | — | Toggles polar tracking. † |
| `PSPACE` | `PS` | Switches from a layout viewport to paper space. † |
| `REDRAW` | `R` | Force a full re-rasterize of the current viewport next frame, WITHOUT touching the DB (never bumps geometry_epoch / block_epoch, never pushes undo). Scope: Active. This arm does not itself clear previews or cancel commands (the normal non-transparent dispatch teardown, commands/mod.rs:90-96, governs that separately); it only queues a per-viewport cache invalidation. |
| `REDRAWALL` | `RA` | Menu: Redraw. |
| `REGEN` | `RE` | Regenerates the entire drawing to recompute screen coordinates, smoothing faceted circles and updating linetypes. *Tip:* When circles look like segmented octagons after zooming in, running RE instantly restores smooth curved display. |
| `REGENALL` | `REA` | Regenerates the entire drawing across all open tiled viewports and paper space viewports. *Tip:* Essential on multi-viewport sheets when zooming changes linetype scale appearances across viewports. |
| `SNAP` | `SN` | Toggle cursor snapping to the grid. |
| `SYNCPVIEWPORTS` | — | Copy the first selected viewport's display settings (view direction/target, scale, snap/grid, frozen layers) to the rest. |
| `TILEMODE` | — | 1: model space; 0: the paper layout that was last current (the drawing's own `CTAB`), else the first one. |
| `TOOLPALETTES` | `TP` | Menu: Tool Palettes. |
| `UCS` | — | Bare UCS → interactive front-end (option then value as steps), so `UCS Z 90` is typable in the command line and works headlessly. The front-end delegates back to the inline handler below. (#169) |
| `UCSFOLLOW` | — | Toggles UCS follow: the view follows the UCS. † |
| `VERTICAL` | — | Ribbon: Tile\nVert.. |
| `VIEW` | `V` | Saves and restores named Model and Paper Space views, cameras, 3D visual styles, and background settings. *Tip:* Named views can be directly dragged from the Sheet Set Manager int layout viewports with auto-scale. |
| `VISUALSTYLES` | `VSM` | Creates and modifies 3D visual styles (2D Wireframe, Conceptual, Hidden, Realistic, Shaded, X-Ray). *Tip:* Switch display mode to inspect 3D solid intersections and realistic renderings. |
| `VPCLIP` | — | Menu: Viewport. |
| `VPJOIN` | — | Ribbon: Join. |
| `VPORTS` | — | Divides the model space display into multiple tiled viewing windows (e.g., Plan, Front, Isometric simultaneously). *Tip:* In layout paper space, creates new floating viewport windows for scal drawing. |
| `VPSYNC` | — | Synchronises layout viewport settings. † |
| `ZOOM` | `Z`, `ZS` | Increases or decreases the apparent magnification of the active viewport drawing window. *Tip:* Z -> E (Extents) zooms to all drawn geometry. Z -> W (Window) zoom to a box. Z -> P returns to Previous view. |

### Other (80)

| Command | Aliases | Description |
|---|---|---|
| `-BMAKE` | — | Command-line (no dialog) form of BMAKE. |
| `-RENAME` | — | Command-line (no dialog) form of RENAME. |
| `-WB` | — | Command-line (no dialog) form of WB. |
| `ADDSELECTED` | — | Starts the creation command corresponding to selected object with matching layer and properties. |
| `AFLAGS` | — | Sets the attribute display flags (invisible, constant, verify, preset). † |
| `ARCHIVE` | — | Package the drawing and its referenced files (xrefs + raster images) into a sibling "<name>_archive" folder. Web: there is no folder to copy into — the same dependency set is collected from the session's bytes registries and delivered as a "<name>_transmit.zip" download (`archive_web` below). |
| `ARRAY3D` | — | Multi-tier 3D arraying. Synonym for 3DARRAY command. |
| `ARRAYPATH` | — | Menu: Path Array. |
| `ARRAYPOLAR` | — | Menu: Polar Array. |
| `ARRAYRECT` | — | Menu: Rectangular Array. |
| `ATTDISP` | — | Menu: Attribute Display. |
| `ATTEXT` | — | Extracts attribute data to a text file (legacy form of DATAEXTRACTION). † |
| `ATTMODE0` | — | Sets attribute display mode 0 (attributes off). † |
| `ATTMODE1` | — | Sets attribute display mode 1 (normal, as defined). † |
| `ATTMODE2` | — | Sets attribute display mode 2 (all attributes on). † |
| `BACKGROUND` | — | Background color Usage: BACKGROUND <r> <g> <b> (0–255 each) BACKGROUND DEFAULT\|BLACK\|DARKGRAY\|GRAY\|LIGHTGRAY\|WHITE (preset) BACKGROUND DEFAULT (restore the app default, rgb 33,40,48) The chosen colour is also stored as the persisted default (`default_bg_color` / `default_paper_bg_color`) so it survives restarts and applies to new drawings (#188). Bare BACKGROUND enters an interactive prompt for the colour, so the command works both as a one-shot (`BACKGROUND BLACK`) and as a type-then-choose flow (`BACKGROUND` ⏎, then `BLACK`). The prompt delegates back to the inline handler below via `Dispatch`. |
| `BASE` | — | Menu: Base. |
| `BMAKE` | — | Opens the Block Editor to create or edit a block definition. † |
| `BREAKATPOINT` | `BAP` | Breaks the selected object into two at a picked point, without removing any part. † |
| `BROWSER` | — | Toggles the model browser: outline of origin planes, open sketches and solid bodies. |
| `CAL` | — | Command-line arithmetic calculator. |
| `CANNOSCALE` | — | Sets or shows the current annotation scale. † |
| `CDIMSTY` | — | System variable-style command: current dimension style name. † |
| `CHPROP` | — | Change entity properties from command line. |
| `CLR` | — | Shorthand alias for CLEAR. |
| `COLOUR` | — | Alias of COLOR. † |
| `CONVTOSURFACE` | — | Generates NURBS or analytic surface geometry. Converts 2D objects, faces, and meshes into 3D surfaces. |
| `DDCOLOR` | — | Opens the colour selection dialog. † |
| `DDPTYPE` | — | Opens the point style dialog (alias of PTYPE). † |
| `DDUNITS` | — | Opens the drawing units dialog. † |
| `DESELALL` | — | Synonym for clearing selection. |
| `DESELECT` | `DE` | Clears the current object selection set. |
| `DWGPROP` | — | Alias of DWGPROPS. † |
| `DWGUNITS` | — | Change what one drawing unit measures, and convert the model to match. The units pill only relabels, which is all the label is for; this is the one place geometry moves. (#668) |
| `DXEVAL` | — | Evaluates data-extraction fields. † |
| `ERSTATE` | — | Shows the state of the External References palette. † |
| `EXPLMODE` | — | Sets whether non-uniformly scaled blocks can be exploded. † |
| `FRAMES0` | — | Sets frame display of images/underlays: off. † |
| `FRAMES1` | — | Frames on, not plotted. † |
| `FRAMES2` | — | Frames on and plotted. † |
| `FRAMES3` | — | Frame display: mixed state (shown, never chosen). † |
| `HELP` | — | Opens documentation or help viewer. |
| `INSBASE` | — | Sets the drawing insertion base point (alias of BASE). † |
| `INSNAME` | — | Sets the default block name for INSERT. † |
| `ISODRAFT` | — | Enable or disable isometric drafting. |
| `ISOLATEOBJECTS` | — | Hide every object except the current selection. |
| `LMAN` | — | Opens the layer state manager. † |
| `MLEDIT` | — | Menu: Multiline.... |
| `MLSTYLE` | — | Menu: Multiline Style.... |
| `MSOLESCALE` | — | Sets the scale of OLE objects pasted in model space. † |
| `OP` | — | Short token for OPTIONS. † |
| `PAPERUPDATE` | — | Updates paper size in the current layout. † |
| `PASTE` | — | Pastes clipboard geometry into drawing at active cursor coordinate. |
| `PASTEBLOCK` | — | Pastes copied entities grouped as an anonymous block reference (*A###). |
| `PASTECLIP` | `PC` | Pastes clipboard contents into the active space (Model or Layout). |
| `PASTEORIG` | — | Pastes clipboard geometry at the exact original XYZ coordinates from which it was copied. |
| `PASTESPEC` | — | Menu: Paste Special.... |
| `PDMODE` | — | Sets the point display style. † |
| `PDSIZE` | — | Sets the point display size. † |
| `PERF` | — | Toggles rendering performance HUD (FPS, draw calls, GPU memory). |
| `PLUGINMANAGER` | — | Opens plugin and extension management window. |
| `PLUGINS` | — | Shorthand alias for PLUGINMANAGER. |
| `PTYPE` | — | Menu: Point Style.... |
| `QSAVE` | `QS` | Quick saves current drawing without prompting for file path. |
| `REG` | — | Short token for REGION. † |
| `RENAME` | `REN` | Menu: Rename.... |
| `RESETBLOCK` | — | Resets a dynamic block to its default definition. † |
| `ROTATE3D` | — | Precision non-orthogonal 3D alignment. Rotates objects in 3D space around any arbitrary two-point line. |
| `SCR` | — | Short token for SCRIPT. † |
| `SELECTALL` | `SA` | Selects all unlocked visible objects in the current drawing space. |
| `SETBYLAYERMODE` | — | Clear color/linetype/lineweight overrides Resets the selected entities' direct property overrides back to ByLayer so they follow their layer again. |
| `SSFOUND` | — | Sheet set: shows matches found. † |
| `SSLOCATE` | — | Sheet set: locates a sheet. † |
| `STPOUT` | — | Short token for STLOUT/STEP output. † |
| `TXTEXP` | — | Text becomes the polylines of its character outlines. |
| `UNION` | `UNI` | Standard constructive solid geometry (CSG) boolean union. Combines two or more 3D solids, surfaces, or 2D regions into a single composite object by boolean addition. |
| `UNISOLATEOBJECTS` | `UNHIDE` | Bring back everything hidden by Isolate / Hide, including objects the drawing was saved with as invisible (e.g. hidden by another CAD program's HIDEOBJECTS). |
| `VS` | — | Visual-style commands. OCS renders either a wireframe or a shaded view; the named styles map onto the closest of the two and the chosen style is reported so the mapping is explicit. (`SOLID` is intentionally NOT a visual-style verb — it is the 2D filled-polygon draw command; the shaded ribbon button drives `SetWireframe`.) One interactive picker behind every verb that asks for a style, offering exactly what the render-mode widget offers. (#621) |
| `VW` | — | Alias of VIEW (named views). † |
| `XRELOAD` | — | Reloads attached external references. † |

## System variables

Type a name to read it and be prompted for a new value (`OSMODE <4133>`). A dash means no default or description is recorded yet.

| Variable | Default | Type / access | Description |
|---|---|---|---|
| `ANGBASE` | 0.0 (East) | Real Read/Write | Sets the direction of angle 0.0 relative to the current UCS (0=East, 90=North, 180=West, 270=South). Tip: Civil and surveyor drawings frequently set ANGBASE to 90 so 0 degrees points due North. |
| `ANGDIR` | 0 (Counter- Clockwise) | Boolean Read/Write | Sets angle positive direction: 0=Counter-clockwise (mathematical standard), 1=Clockwise (survey standard). Tip: Combine ANGBASE=90 and ANGDIR=1 to establish standard land survey bearings. |
| `APERTURE` | 10 (Pixels) | Integer Read/Write | Sets the display size of the Object Snap target aperture box in pixels. Tip: Larger aperture catches snap points from further away. |
| `AREA` | 0.0 | Real Read-Only | Stores the last calculated surface or perimeter area from AREA, MEASUREGEOM, or MASSPROP. Tip: Can be linked into dynamic text fields for live square-footage annotations. |
| `ATTDIA` | — | — | — |
| `ATTMODE` | — | — | — |
| `ATTREQ` | — | — | — |
| `AUNITS` | 0 (Degrees) | Integer Read/Write | Sets angular units format: 0=Decimal degrees, 1=Degrees/minutes/seconds, 2=Gradians, 3=Radians, 4=Surveyor units (N 45d E). Tip: Civil survey plans require 4 (Surveyor units) to parse boundary legal descriptions. |
| `AUPREC` | 0 | Integer Read/Write | Sets angular units display precision decimal places (0 to 8). Tip: Survey drawings typically require 2 to 4 places for seconds precision. |
| `AUTOSNAP` | 63 | Bitmask Read/Write | Controls Osnap visual marker (1), tooltip (2), magnet (4), polar tracking (8), and extension paths (16/32). Tip: Bit 4 (value 4) is the magnet that pulls the cursor directly to snap points. |
| `BLIPMODE` | — | — | — |
| `CAMERADISPLAY` | — | — | — |
| `CCONSTRAINTFORM` | — | — | — |
| `CDATE` | 20260912.0558 | Real Read-Only | Stores current calendar date and time in decimal format (YYYYMMDD.HHMMSSMSEC). Tip: Used in plot stamps, title block dates, and revision logging routines. |
| `CELTSCALE` | 1.0 | Real Read/Write | Sets the current entity linetype scale multiplier for newly created objects. Tip: Keep at 1.0; adjust LTSCALE or PSLTSCALE instead of setting individual object scales. |
| `CENTERCROSSGAP` | — | — | — |
| `CENTERCROSSSIZE` | — | — | — |
| `CENTEREXE` | — | — | — |
| `CENTERLAYER` | — | — | — |
| `CENTERLTSCALE` | — | — | — |
| `CENTERLTYPE` | — | — | — |
| `CENTERLTYPEFILE` | — | — | — |
| `CENTERMARKEXE` | — | — | — |
| `CLIPROMPTLINES` | — | — | — |
| `CMDDIA` | 1 (On) | Boolean Read/Write | Controls display of command dialog boxes (e.g. PLOT, QLEADER, ATTDEF dialogs). Tip: Set to 0 inside LISP scripts to automate plotting; set to 1 for interactive GUI dialogs. |
| `CMDECHO` | 1 (On) | Boolean Read/Write | Controls whether command prompts and echo outputs are printed to the command line during script/LISP execution. Tip: Set to 0 in AutoLISP programs to suppress terminal text flicker and maximize execution speed. |
| `CMLJUST` | — | — | — |
| `CMLSCALE` | — | — | — |
| `CMLSTYLE` | — | — | — |
| `COLORTHEME` | — | — | — |
| `COMMANDLINEFADETIME` | 2 (Seconds) | Integer Read/Write | Sets number of seconds floating semi-transparent command lines remain visible before fading out. Tip: Increase to 5-10 seconds if you want more time to read command prompts and error output. |
| `COMMANDLINEOPACITY` | — | — | — |
| `CONSTRAINTBARDISPLAY` | — | — | — |
| `CONSTRAINTBARMODE` | — | — | — |
| `CONSTRAINTINFER` | — | — | — |
| `CONSTRAINTNAMEFORMAT` | — | — | — |
| `CONSTRAINTSOLVEMODE` | — | — | — |
| `COORDS` | — | — | — |
| `CROSSINGAREACOLOR` | 90 (Green) | Integer Read/Write | Specifies the color of the right-to-left crossing selection window. Tip: Crossing window selects any object touched or crossed by the boundary. |
| `CURSORSIZE` | 5 (Percent) | Integer Read/Write | Sets the size of the crosshairs as a percentage of the screen size (1 to 100). Tip: Set to 100 for full-screen crosshairs spanning the entire drawing window (classic CAD style). |
| `CURSORTYPE` | — | — | — |
| `DELOBJ` | 1 | Integer Read/Write | Controls whether source 2D profile linework is retained or deleted when creating 3D solids (0=retain all, 1=delete profiles/paths, 2=delete all source curves). Tip: Set to 0 if you want to keep original 2D sketch paths after running EXTRUDE, REVOLVE, or SWEEP. |
| `DGNFRAME` | 1 | Integer Read/Write | Controls underlay frame visibility for attached Bentley MicroStation DGN files. Tip: 0=hidden, 1=plotted, 2=visible not plotted. |
| `DGNIMPORTMODE` | — | — | — |
| `DGNOSNAP` | — | — | — |
| `DIMASO` | — | — | — |
| `DIMASSOC` | 2 | Integer Read/Write | Controls dimension associativity: 0=exploded lines/text, 1=non-associative unified dimension, 2=associative (updates automatically when geometry moves or stretches). Tip: Always maintain DIMASSOC=2 so dimensions dynamically track stretched geometry and revision edits. |
| `DIMCLRD` | 0 (ByBlock) | Integer Read/Write | Assigns color to dimension lines, arrowheads, and leader lines. Tip: Set to 256 (ByLayer) for clean, professional layer management. |
| `DIMCLRE` | 0 (ByBlock) | Integer Read/Write | Assigns color to dimension extension lines. Tip: Set to 256 (ByLayer) for CAD standards consistency. |
| `DIMCLRT` | 0 (ByBlock) | Integer Read/Write | Assigns color to dimension annotation text. Tip: Set to 256 (ByLayer) so text color is governed by its assigned dimension layer. |
| `DIMCONTINUEMODE` | — | — | — |
| `DIMLFAC` | 1.0 | Real Read/Write | Sets a global linear scale factor multiplier for all measured dimension distances. Tip: Use in detail views (e.g. 2.0 or 0.5) if dimensioning geometry scaled manually rather than via viewport. |
| `DIMSCALE` | 1.0 | Real Read/Write | Sets overall scale factor applied to dimension variables that specify sizes, distances, or offsets. Tip: Set to 0 to enable paper space viewport scaling (DIMSCALE=0 calculates scale factor automatically from viewport zoom). |
| `DIMSHO` | — | — | — |
| `DIMTXT` | 0.18 | Real Read/Write | Sets the height of dimension text when the active text style does not have a fixed height. Tip: Set text height in your DIMSTYLE rather than altering DIMTXT directly. |
| `DISPSILH` | 0 (Off) | Boolean Read/Write | Controls display of 3D solid silhouette outlines when in wireframe and hidden visual styles (0=Off, 1=On). Tip: Set to 1 to produce clean silhouette outlines for 3D curved surfaces without internal mesh tessellation lines. |
| `DONUTID` | — | — | — |
| `DONUTOD` | — | — | — |
| `DRAGMODE` | — | — | — |
| `DWFFRAME` | 1 | Integer Read/Write | Controls underlay boundary frames for attached Autodesk DWF/DWFx underlays. Tip: Follows identical 0, 1, 2 settings as PDFFRAME and IMAGEFRAME. |
| `DWFOSNAP` | — | — | — |
| `DWGNAME` | Drawing1.dwg | String Read-Only | Stores the file name of the currently active drawing document. Tip: Referenced dynamically by Fields inside title blocks to auto-print sheet file names. |
| `DWGPREFIX` | C:\Drawings\ | String Read-Only | Stores the drive and directory folder path of the active drawing document. Tip: Used in title block sheet metadata fields and AutoLISP directory routing. |
| `DYNCONSTRAINTDISPLAY` | — | — | — |
| `DYNMODE` | — | — | — |
| `ELEVATION` | 0.0 | Real Read/Write | Sets default Z-elevation for newly drawn entities relative to the current UCS plane. Tip: If geometry refuses to snap to 2D Z=0, check ELEVATION and set back to 0. |
| `FACETRES` | 0.5 | Real Read/Write | Adjusts smoothness of shaded 3D solids, curves, and rendered views (0.01 to 10.0). Tip: Set to 5.0 or 10.0 for publication-quality shaded 3D exports with silky smooth curved surfaces. |
| `FIELDDISPLAY` | — | — | — |
| `FILEDIA` | 1 (On) | Boolean Read/Write | Controls display of file navigation dialog boxes (1=GUI file picker dialog, 0=command-line prompts only). Tip: The classic OCS panic fix! If OPEN, SAVEAS, or XREF suddenly only prompt in text at the command line without opening a window, type FILEDIA and set to 1. |
| `FRAME` | 3 | Integer Read/Write | Master override variable that sets IMAGEFRAME, PDFFRAME, WIPEOUTFRAME, and XCLIPFRAME simultaneously in a single command. Tip: Type FRAME -> 2 to set all underlay and wipeout borders to "visible on screen, suppressed on plot" in one step. |
| `GRIDDISPLAY` | 2 | Bitmask Read/Write | Controls grid visual behavior: 1=display beyond drawing limits, 2=adaptive grid, 4=subdivision beyond limits. Tip: Set to 2 to prevent excessive dense grid lines when zooming far out. |
| `GRIDMODE` | 0 (Off) | Boolean Read/Write | Turns grid display on (1) or off (0) in the current viewport. Tip: Toggle on/off with F7. |
| `GRIDUNIT` | 0.5, 0.5 | 2D/3D Point Read/Write | Specifies the grid X and Y spacing for the active viewport. Tip: Setting to 0,0 automatically synchronizes grid spacing with SNAPUNIT. |
| `GRIPCOLOR` | 160 (Blue) | Integer Read/Write | Controls the color of unselected (cold) grips (OCS ACI color index 1-255). Tip: Change if working on dark or customized viewport backgrounds. |
| `GRIPHOT` | 1 (Red) | Integer Read/Write | Controls the color of selected (hot) active grips. Tip: Hot grips are actively being stretched or transformed. |
| `GRIPHOVER` | 11 (Pink) | Integer Read/Write | Controls the color of a hovered grip before clicking. Tip: Pausing cursor over hover grips triggers dynamic multi-functional grip menus. |
| `GRIPOBJLIMIT` | — | — | — |
| `GRIPS` | 2 | Integer Read/Write | Controls display of selection grips (0=No grips, 1=Grips displayed, 2=Midpoint grips on polyline segments). Tip: Set to 2 to gain midpoint stretch and arc-convert grip menus on polylines. |
| `GRIPSIZE` | 5 (Pixels) | Integer Read/Write | Sets the size of the unselected grip box in pixels (1 to 255). Tip: Scale up on 4K monitors to make grabbing entity vertices easy. |
| `HALOGAP` | — | — | — |
| `HIGHLIGHT` | 1 (On) | Boolean Read/Write | Controls whether selected objects are highlighted on screen (0=no highlight, 1=highlighted). Tip: Keep at 1. If set to 0, objects will appear unselected even when actively part of a selection set. |
| `HPASSOC` | — | — | — |
| `HPGAPTOL` | — | — | — |
| `IMAGEFRAME` | 1 | Integer Read/Write | Controls raster image border frames: 0=frame hidden & not plotted; 1=frame displayed & plotted; 2=frame displayed but NOT plotted. Tip: Setting 2 is optimal: allows you to select, move, and edit aerial photos while preventing ugly black border lines from plotting. |
| `INSUNITS` | 1 (Inches) | Integer Read/Write | Specifies drawing unit value for automatic scaling when inserting blocks or Xrefs (0=Unspecified, 1=Inches, 2=Feet, 4=Millimeters, 5=Centimeters, 6=Meters). Tip: Crucial for DesignCenter and Xrefs. If a metric title block inserts 25.4x too small, check INSUNITS in both DWGs. |
| `INSUNITSDEFSOURCE` | 1 | Integer Read/Write | Sets source content unit value when INSUNITS is set to 0 (unspecified). Tip: Prevents unwanted unit conversion popups on legacy files. |
| `INSUNITSDEFTARGET` | 1 | Integer Read/Write | Sets target drawing unit value when INSUNITS is set to 0 (unspecified). Tip: Set default target drawing units for legacy files. |
| `ISOLINES` | 4 | Integer Read/Write | Specifies number of wireframe contour lines displayed on curved surfaces of 3D solids (0 to 2047). Tip: Increase to 16 or 32 for higher visual surface fidelity on curved solids. |
| `LAYOUTCREATEVIEWPORT` | — | — | — |
| `LIMCHECK` | — | — | — |
| `LOGINNAME` | (Windows User) | String Read-Only | Stores the Windows or Autodesk account user login name. Tip: Used in title blocks and plot stamps to track "Drawn By" and approval records automatically. |
| `LTSCALE` | 1.0 | Real Read/Write | Sets global linetype scale factor for all dotted, dashed, and patterned lines across the entire drawing. Tip: In Model Space, set LTSCALE to match your intended plot scale factor (e.g. 48 for 1/4"=1'-0", 100 for 1:100). |
| `LUNITS` | 2 (Decimal) | Integer Read/Write | Sets linear coordinate display unit format: 1=Scientific, 2=Decimal, 3=Engineering (feet & decimal inches), 4=Architectural (feet & fractional inches), 5=Fractional. Tip: Architects and interior designers use 4; civil and mechanical engineers use 2. |
| `LUPREC` | 4 | Integer Read/Write | Sets linear units display precision (decimal places or fractional denominator 0 to 8). Tip: In Architectural units, 4 = 1/16" precision; 6 = 1/64" precision. |
| `LWDEFAULT` | 25 (0.25mm) | Integer Read/Write | Sets the default lineweight value in hundredths of a millimeter for objects on Default lineweight. Tip: Standard default is 25 (0.25mm). |
| `LWDISPLAY` | 0 (Off) | Boolean Read/Write | Controls whether real lineweights (thicknesses) are visually displayed in Model and Paper space. Tip: Toggle on to verify pen weights before plotting, toggle off for crisp pixel-perfect vertex snapping. |
| `MAXACTVP` | 64 | Integer Read/Write | Sets maximum number of layout viewports that can be simultaneously active and regenerated. Tip: If some viewports on a multi-detail sheet appear blank, increase MAXACTVP (up to 64). |
| `MENUBAR` | — | — | — |
| `MIRRTEXT` | 0 (Off) | Boolean Read/Write | Controls how the MIRROR command reflects text: 0=retains text reading direction (left-to- right), 1=mirrors text backwards like a mirror image. Tip: Always keep MIRRTEXT=0 so mirrored floor plans or symmetrical parts do not produce backwards unreadable notes. |
| `MSLTSCALE` | 1 (On) | Boolean Read/Write | Scales linetypes displayed on the model tab by the current annotation scale (CANNOSCALE). Tip: Set to 1 so dashed lines preview accurately in Model Space as you change annotation scale. |
| `ORTHOMODE` | 0 (Off) | Boolean Read/Write | Constrains cursor movement strictly to horizontal and vertical axes (F8 toggle). Tip: Hold Shift temporarily while drawing to invert Ortho mode on the fly. |
| `OSMODE` | 4133 | Bitmask Read/Write | Sets running Object Snap (Osnap) modes using bitcode sum (1=Endpoint, 2=Midpoint, 4=Center, 8=Node, 16=Quadrant, 32=Intersection, 64=Insertion, 128=Perpendicular, 256=Tangent, 512=Nearest, 1024=Apparent, 2048=Extension, 4096=Parallel). Tip: Add 16384 to suppress running osnaps without losing your configured bitmask. 0 turns all osnaps off. |
| `OSNAPZ` | — | — | — |
| `PDFFRAME` | 1 | Integer Read/Write | Controls underlay boundary frame display for attached PDF documents (0=hidden, 1=plotted, 2=visible but not plotted). Tip: Set to 2 to work with attached PDF underlays without visible rectangular plot borders. |
| `PDFIMPORTFILTER` | — | — | — |
| `PDFIMPORTIMAGEPATH` | — | — | — |
| `PDFIMPORTLAYERS` | — | — | — |
| `PDFIMPORTMODE` | — | — | — |
| `PDFOSNAP` | — | — | — |
| `PDFSHXBESTFONT` | — | — | — |
| `PDFSHXLAYER` | — | — | — |
| `PDFSHXTEXTSETTINGS` | — | — | — |
| `PELLIPSE` | — | — | — |
| `PERIMETER` | 0.0 | Real Read-Only | Stores the last calculated perimeter or boundary length computed by inquiry commands. Tip: Referenced in automated takeoff calculations. |
| `PICKADD` | 2 | Integer Read/Write | Controls whether subsequent object selections replace or add to the current selection set (0=replace, 1=add, 2=add and retain selection after command ends). Tip: If clicking a new object deselects your previous object, PICKADD was accidentally changed to 0; set back to 2. |
| `PICKAUTO` | 5 | Integer Read/Write | Controls automatic window selection when clicking open space (window, crossing, fence, lasso). Tip: Set to 1 for classic window/crossing without accidental lasso dragging. |
| `PICKBOX` | 3 (Pixels) | Integer Read/Write | Sets the object selection target pickbox height in device pixels (0 to 50). Tip: Increase on 4K high-DPI displays to make selecting fine lines effortless. |
| `PICKDRAG` | 2 | Boolean Read/Write | Controls how selection windows are drawn: 0=two clicks, 1=press and drag, 2=auto-detect both. Tip: Set to 0 if you find yourself accidentally dragging selection windows. |
| `PICKFIRST` | 1 (On) | Boolean Read/Write | Controls whether you can select objects before starting a command (Noun/Verb selection). Tip: If pressing Delete key or running MOVE no longer works on pre-selected objects, set PICKFIRST to 1. |
| `PICKSTYLE` | — | — | — |
| `PLIMCHECK` | — | — | — |
| `PLINEGEN` | 1 (On) | Boolean Read/Write | Controls linetype dash generation around 2D polyline vertices: 1=continuous dash pattern throughout line, 0=restarts pattern at each vertex. Tip: Set to 1 so dashed lines around rounded corners do not appear solid or broken. |
| `POINTCLOUD2DVSDISPLAY` | — | — | — |
| `POINTCLOUDAUTOUPDATE` | — | — | — |
| `POINTCLOUDBOUNDARY` | — | — | — |
| `POINTCLOUDCACHESIZE` | — | — | — |
| `POINTCLOUDCLIPFRAME` | — | — | — |
| `POINTCLOUDDENSITY` | — | — | — |
| `POINTCLOUDLIGHTING` | — | — | — |
| `POINTCLOUDLIGHTSOURCE` | — | — | — |
| `POINTCLOUDLOCK` | — | — | — |
| `POINTCLOUDLOD` | — | — | — |
| `POINTCLOUDPOINTMAX` | — | — | — |
| `POINTCLOUDPOINTMAXLEGACY` | — | — | — |
| `POINTCLOUDPOINTSIZE` | — | — | — |
| `POINTCLOUDRTDENSITY` | — | — | — |
| `POINTCLOUDSHADING` | — | — | — |
| `POINTCLOUDVISRETAIN` | — | — | — |
| `POLARANG` | 90.0 | Real Read/Write | Sets polar tracking angle increment in degrees (e.g. 15, 30, 45, 90). Tip: Set to 45 or 30 for isometric/architectural drafting guides. |
| `POLARMODE` | 0 | Bitmask Read/Write | Controls polar and object snap tracking settings (relative vs. absolute angles). Tip: Set to 1 to measure tracking angles relative to selected segments. |
| `PRODUCT` | OCS | String Read-Only | Returns the official product name of the active CAD platform. Tip: Used by plugins to differentiate the OCS engine from full OCS or OCS. |
| `PSLTSCALE` | 1 (On) | Boolean Read/Write | Controls Paper Space linetype scaling: 1=viewports scale dashed lines identical on paper regardless of zoom scale; 0=viewports show raw model scale. Tip: Always keep PSLTSCALE=1 and MSLTSCALE=1 for consistent dashes across viewports at 1/8", 1/4", or 1" scales. |
| `QTEXTMODE` | — | — | — |
| `REGENMODE` | 1 (On) | Boolean Read/Write | Controls automatic regeneration of drawings when zooming or panning beyond current display bounds. Tip: Keep at 1 so circles and arcs automatically smooth out when zooming in. |
| `SAVETIME` | 10 (Minutes) | Integer Read/Write | Sets automatic backup save interval in minutes (0 to 120). 0 disables autosave. Tip: Recommended 5-10 minutes. Autosave files are written with .sv$ extension in your Windows %TEMP% folder. |
| `SDI` | — | — | — |
| `SELECTIONAREA` | 1 (On) | Boolean Read/Write | Controls whether selection windows and crossing windows are shaded with translucent color. Tip: Set to 0 for classic transparent wire outline selection frames. |
| `SELECTIONAREAOPACITY` | 25 (Percent) | Integer Read/Write | Sets transparency percentage of selection area tint (0 to 100). Tip: Default 25% provides clear visual contrast without obscuring underlying drawing lines. |
| `SELECTIONCYCLING` | — | — | — |
| `SELECTIONEFFECT` | 1 (Glow) | Boolean Read/Write | Controls visual selection highlight style: 1=solid glowing line color, 0=classic dashed bounding highlight. Tip: Classic drafters who dislike the neon glow set this to 0 for dashed lines. |
| `SELECTIONEFFECTCOLOR` | 0 | Integer Read/Write | Sets the custom color used for object selection glow highlighting. Tip: Specify color index or RGB to customize selection color. |
| `SELECTIONPREVIEW` | 3 | Bitmask Read/Write | Controls object glow highlighting when cursor hovers over objects (1=when command active, 2=when no command active). Tip: Set to 0 on massive civil maps or complex files to boost viewport cursor speed. |
| `SELECTSIMILARMODE` | — | — | — |
| `SHADEDGE` | — | — | — |
| `SHORTCUTMENU` | — | — | — |
| `SHORTCUTMENUDURATION` | — | — | — |
| `SHOWHIST` | — | — | — |
| `SKETCHINC` | — | — | — |
| `SKPOLY` | — | — | — |
| `SKTOLERANCE` | — | — | — |
| `SNAPANG` | 0.0 | Real Read/Write | Sets the snap and grid rotation angle for the current viewport relative to the current UCS. Tip: Rotate snap grid to align crosshairs with angled building wings or site property lines. |
| `SNAPISOPAIR` | 0 (Left) | Integer Read/Write | Controls the current isometric plane when SNAPTYPE=1 (0=Left, 1=Top, 2=Right). Tip: Press F5 or Ctrl+E to cycle isoplanes on the fly while drawing. |
| `SNAPMODE` | 0 (Off) | Boolean Read/Write | Turns Snap mode on (1) or off (0) — locks cursor movement to discrete grid increments. Tip: Toggle instantly using the F9 shortcut key. |
| `SNAPTYPE` | 0 (Standard) | Integer Read/Write | Sets snap style: 0 for standard rectangular grid, 1 for isometric snap. Tip: Set to 1 to activate 2.5D isometric drafting mode. |
| `SNAPUNIT` | 0.5, 0.5 | 2D/3D Point Read/Write | Sets the X and Y snap spacing intervals for the current viewport. Tip: Can be set independently for X and Y for non-square grid locking. |
| `SOLIDHIST` | — | — | — |
| `SORTENTS` | — | — | — |
| `SPLFRAME` | — | — | — |
| `SPLINESEGS` | — | — | — |
| `SPLINETYPE` | — | — | — |
| `STARTUP` | 3 | Integer Read/Write | Controls startup behavior when launching OCS (0=blank drawing, 1=Startup dialog box, 2=no drawing, 3=Start tab screen). Tip: Set to 0 to bypass the Start tab and immediately open a default template. |
| `SURFACEASSOCIATIVITY` | — | — | — |
| `SURFTYPE` | 6 (Cubic B- spline) | Integer Read/Write | Surface fitting type for smoothed polygon meshes: 5=Quadratic B-spline, 6=Cubic B- spline, 8=Bezier surface. Tip: Used with the EDGECUT and REVSURF mesh modeling tools. |
| `SURFU` | 6 | Integer Read/Write | Surface density in the M direction for 3D polygon meshes (2 to 200). Tip: Higher density increases mesh smoothness at the expense of drawing memory. |
| `SURFV` | 6 | Integer Read/Write | Surface density in the N direction for 3D polygon meshes (2 to 200). Tip: Controls longitudinal mesh resolution. |
| `TEXTFILL` | 1 (Filled) | Boolean Read/Write | Controls whether TrueType (TTF) fonts are displayed and plotted as solid fills or hollow wireframe outlines. Tip: Set to 0 to outline TrueType fonts for laser cutting, engraving, or CNC routing. |
| `TEXTQLTY` | 50 (Percent) | Integer Read/Write | Sets resolution and smoothness of TrueType text outlines (0 to 100). Tip: Increase to 100 for razor-sharp vector plotting and CNC profiling. |
| `THICKNESS` | 0.0 | Real Read/Write | Sets default 3D extrusion thickness applied to newly drawn 2D objects (lines, circles, arcs). Tip: If 2D lines suddenly look like tall 3D walls when viewed in 3D orbit, THICKNESS was accidentally set > 0; set back to 0. |
| `TRACEWID` | — | — | — |
| `TRIMEXTENDMODE` | — | — | — |
| `TXT2MTXTCOMBINEMTEXT` | — | — | — |
| `TXT2MTXTLINESPACING` | — | — | — |
| `TXT2MTXTTEXTORDER` | — | — | — |
| `TXT2MTXTWORDWRAP` | — | — | — |
| `UCSDETECT` | — | — | — |
| `UCSICON` | 3 | Bitmask Read/Write | Controls coordinate system icon: 0=Off, 1=On, 2=Placed at Origin (0,0,0), 3=On at Origin. Tip: Type UCSICON -> Noorigin to dock the coordinate marker in the lower-left corner instead of floating at 0,0. |
| `UCSORTHO` | — | — | — |
| `UCSVIEW` | — | — | — |
| `UCSVP` | 1 (On) | Boolean Read/Write | Controls whether viewport UCS remains locked or updates when switching between layout viewports. Tip: Allows each layout viewport to maintain its own independent rotation or plan orientation. |
| `UOSNAP` | — | — | — |
| `USRTIMER` | — | — | — |
| `VIEWRES` | 1000 | Integer Read/Write | Sets resolution for circle, arc, and curve tessellation on screen (1 to 20000). Tip: If circles look like faceted octagons, run VIEWRES, type Y to fast zoom, and set to 10000 or 20000. |
| `VISRETAIN` | 1 (On) | Boolean Read/Write | Controls Xref layer property retention: 1=keeps layer colors/visibility overrides set in host drawing; 0=resets to external file values every time drawing opens. Tip: Keep at 1. If layer freezes or colors reset every time you open a project DWG, VISRETAIN was turned to 0. |
| `VSEDGECOLOR` | — | — | — |
| `WINDOWAREACOLOR` | 150 (Blue) | Integer Read/Write | Specifies the color of the standard left-to-right selection window. Tip: Standard window selects only objects completely enclosed within the boundary. |
| `WINDOWSAREACOLOR` | — | — | — |
| `WIPEOUTFRAME` | 1 | Integer Read/Write | Controls wipeout boundary frames: 0=hidden & not plotted; 1=displayed & plotted; 2=displayed on screen but NOT plotted. Tip: Set to 2 so drafters can see and grip wipeout edges without printing thick perimeter lines. |
| `WORLDVIEW` | — | — | — |
| `XCLIPFRAME` | 0 | Integer Read/Write | Controls clipping boundary frame visibility for clipped Xrefs (0=hidden, 1=plotted, 2=displayed not plotted). Tip: Set to 2 to easily adjust xref clip boundaries without printing the border box. |
| `XDWGFADECTL` | — | — | — |
| `ZOOMFACTOR` | 60 (Percent) | Integer Read/Write | Controls mouse wheel zoom increment sensitivity (3 to 100). Higher = faster zoom per wheel notch. Tip: Set to 40 for finer precision zooming, or 80 for rapid navigation across large site plans. |
| `ZOOMWHEEL` | 0 | Boolean Read/Write | Toggles mouse wheel zoom direction: 0=forward zooms in, backward zooms out; 1=inverts wheel direction. Tip: Set to 1 if you are accustomed to GIS, 3ds Max, or Mac scroll wheel directions. |
