# Files and Formats

Open CAD Studio reads and writes DWG and DXF itself, with no conversion service in between. Drawings you save open in other programs that read these formats, and theirs open here.

## Opening drawings

- **Ctrl+O** (`OPEN`), the application menu, or the recent drawings list.
- Drag files from the file manager onto the window; each opens in its own tab.
- On Windows and Linux with the installer package, double-click a DWG or DXF file. If the program is already running, the drawing opens as a new tab in it.

DWG, DXF, backup (`.bak`) and autosave (`.sv$`) files can all be opened.

**Missing fonts and references.** When a drawing uses fonts or external references that cannot be found, it still opens: substitute fonts are used and missing references are listed in the External References palette.

## Saving

| Command | Key | What it does |
|---|---|---|
| `SAVE`, `QSAVE` | Ctrl+S | Saves under the current name; the first time asks for one. |
| `SAVEAS` | Ctrl+Shift+S | Saves under a new name, type or version. |
| `SAVEALL` | | Saves every open drawing. |

**Versions.** Both DWG and DXF can be saved as any version from R14 up to 2018. Choose an older version when the drawing goes to someone with an older program. Choose the default (2018) otherwise: it keeps everything.

**Backups.** When you save over a drawing, the previous file is kept as `name.bak` (turn this off with `ISAVEBAK`). Rename a `.bak` to `.dwg`, or simply open it, to get the earlier version back.

**Autosave.** Every few minutes open drawings are saved to a recovery file (`.sv$`). Set the interval in minutes with `SAVETIME` (0 turns autosave off) and the folder with `SAVEFILEPATH`, or under **Options › Open and Save**.

## Templates

`QNEW` (**Ctrl+N**) starts a drawing from the default template (`.dwt`), set under **Options › Files**. `NEW` asks which template to start from. A template is an ordinary drawing saved as `.dwt`, with your layers, styles, layouts and title blocks ready.

## Damaged drawings

If a drawing cannot be read cleanly, the program offers to open it in **recovery** mode. Recovery reads what it can, skips damaged records and opens the drawing with a report of what it found. Save the recovered drawing under a new name and check it.

| Command | What it does |
|---|---|
| `RECOVER` | Opens a drawing in recovery mode on purpose. |
| `RECOVERALL` | Recovers a drawing and the drawings it references. |
| `AUDIT` | Checks the open drawing for errors, such as objects on missing layers or references to missing blocks, and reports them. |

## Cleaning up

| | Command | What it does |
|---|---|---|
| <img src="images/icons/purge.svg" width="24" alt=""> | `PURGE` (`PU`) | Removes unused layers, blocks, styles, linetypes and other named items. |
| <img src="images/icons/find_nonpurgeable.svg" width="24" alt=""> | `FINDNONPURGEABLE` | Lists the named items that cannot be purged, and what uses them. |
| <img src="images/icons/overkill.svg" width="24" alt=""> | `OVERKILL` | Removes duplicate and overlapping objects. |
| | `COMPARE` | Shows what differs between the open drawing and another one. |

## Drawing properties

`DWGPROPS` shows and edits the drawing's properties: title, subject, author, keywords, comments, custom properties, and statistics such as creation and edit time. Fields can show these properties in title blocks.

## Sending a drawing

`ETRANSMIT` (`ZIP`) packs the drawing, recursive external references, images, underlays, point clouds, fonts, SHX shapes, custom plot style tables, and external `CENTERLTYPEFILE` sources into a ZIP file. Options selects source-folder preservation, type-based folders, or a flat package; document references are rewritten to match and filename collisions receive unique names. Its `ETRANSMIT.txt` report lists missing resources; see the command page for browser limitations.

## Importing and exporting

| Format | Import | Export |
|---|---|---|
| DWG | Open | Save |
| DXF | Open | Save |
| PDF | `PDFIMPORT` (as objects), `PDFATTACH` (as underlay) | `PLOT`, `EXPORTPDF` |
| SVG, SVGZ | `SVGIN` or `IMPORT` (as a block) | `EXPORTSVG` |
| OBJ | `IMPORTOBJ` (meshes) | |
| 3D Studio (3DS) | `IMPORT` | |
| SAT | `ACISIN` (solids) | |
| DGN | `DGNIMPORT` | |
| WMF | `WMFIN` | |
| LandXML | `LANDXMLIMPORT` (survey points) | |
| STL | | `EXPORTSTL` |
| STEP | | `EXPORTSTEP` |
| FBX | | `FBXEXPORT` |
| CSV | `DATALINK` | `DATAEXTRACTION` |
| CTB, STB | Plot style tables | Plot style tables |

`EXPORT` lists all export formats in one dialog.

### IFCCAD (experimental)

**Save As › IFCCAD** writes the drawing to the experimental IFCCAD profile of IFCX (`.ifccad`). This is for testing only: later versions may not read these files back. Keep your DWG or DXF original. The [IFCCAD guide](https://github.com/HakanSeven12/OpenCADStudio/blob/main/docs/ifccad.md) explains what is kept and what is not.

## Converting from the command line

The desktop program can convert drawings without opening a window:

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --export old.dxf new.dwg --target-version R14
```

See [Automation and Plugins](Automation-and-Plugins).

## Web app

In the browser, drawings are opened from the device with the browser's file picker and saved as downloads. Everything else works as in the desktop program, apart from plugins, printing to system printers and command-line use.
