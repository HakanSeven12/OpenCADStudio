# The User Interface

![The main window](images/screens/main-window.png)

The window has the same parts in the desktop program and in the web app:

| Part | Where | What it is for |
|---|---|---|
| Application menu | Top-left button | New, open, save, export, recent drawings, plot, close. |
| Quick Access Toolbar | Beside the application menu | The commands you use all the time, plus the current layer and window switching. |
| Ribbon | Under the title bar | Every tool, arranged in tabs and panels. |
| Drawing tabs | Above the drawing | One tab per open drawing. Ctrl+Tab and Ctrl+Shift+Tab move between them. |
| Drawing area | Centre | The drawing, the cursor, the ViewCube and the UCS icon. |
| Layout tabs | Under the drawing | **Model** and one tab per sheet layout. |
| Command line | Bottom | Type commands, read prompts, pick options. |
| Status bar | Very bottom | Coordinates and the drafting aids (snap, ortho, tracking and the rest). |
| Palettes | Docked at the sides, or floating | Properties, Layers, Blocks, External References, QuickCalc and others. |

## The ribbon

The ribbon groups the tools in tabs; [Ribbon Reference](Ribbon-Reference) lists every panel with pictures. Each tab is divided into panels; a small arrow on a button opens a list of related tools, and the last tool you used from a list becomes its button.

| Tab | Panels |
|---|---|
| **Draw** | Draw · Modify · Annotation · Layers · Block · Properties · Groups · Clipboard · Measurement |
| **Parametric** | Geometric · Dimensional · Manage |
| **Model** | Create · Boolean · Edges |
| **Insert** | Block · Block Definition · Reference · Import · Data · Linking & Extraction · Content |
| **Annotate** | Text · Dimensions · Centerlines · Leaders · Tables · Markup · Annotation Scaling |
| **View** | Viewport Tools · Navigate · Model Viewports · Preset · Palettes · Interface · Plot |
| **Manage** | Customization · Cleanup · Application |
| **Layout** | Viewport · Plot (shown while a sheet layout is active) |

Some tabs appear only when they apply: **Block Editor** while you edit a block, **Table Cell** while table cells are selected.

**Tooltips.** Hold the pointer over a ribbon or Quick Access button to see its name, a one-line description of what it does and the command it runs.

**Keyboard access.** Press and release **Alt**: every tab and Quick Access button shows a key tip. Type a tab's letters to open it, then a button's letters to run it. **Esc** goes back one level.

**Search.** The search box finds any command on the ribbon, the menus or the Quick Access Toolbar by name and runs it as its button would.

To hide the ribbon, use `RIBBONCLOSE`; `RIBBON` brings it back. `CLEANSCREEN` (**Ctrl+0**) hides the ribbon and the palettes together.

## The command line

Everything in the program is a command, and every command can be typed. The command line:

- **Suggests** matching commands and system variables as you type, each with a one-line description. Arrow keys move through the list; Enter runs the highlighted one.
- **Accepts aliases**: `L` for `LINE`, `C` for `CIRCLE`, `TR` for `TRIM` and so on. See the [Command Reference](Command-Reference) and [Customization](Customization#command-aliases).
- **Shows options** in square brackets, for example `[Polyline/Radius]`. Type the capital letter of an option, or click it.
- **Repeats the last command** when you press Enter or Space at an empty prompt.
- **Remembers** what you typed: Up and Down walk through earlier input. **F2** opens the full command history.

**Ctrl+9** hides or shows the command line.

**Spaces in a line.** Space submits like Enter. To keep spaces in the line (for a command with its answers, or a text), start it with `>` or turn on the **>** button beside the command line.

Press **Esc** to cancel a running command. Enter or Space ends commands that take a series of points.

## The status bar

The status bar shows the cursor coordinates and buttons for the drafting aids. Click a button to turn the aid on or off; right-click it for its settings. Use the menu at the right end of the bar to choose which buttons are shown.

| | Button | Key |
|---|---|---|
| <img src="images/icons/status/ortho.svg" width="20" alt=""> | Ortho Mode | F8 |
| <img src="images/icons/status/polar.svg" width="20" alt=""> | Polar Tracking | F10 |
| <img src="images/icons/status/osnap.svg" width="20" alt=""> | Object Snap | F3 |
| <img src="images/icons/status/otrack.svg" width="20" alt=""> | Object Snap Tracking | F11 |
| <img src="images/icons/status/dyn.svg" width="20" alt=""> | Dynamic Input | F12 |
| <img src="images/icons/status/ducs.svg" width="20" alt=""> | Dynamic UCS | F6 |
| <img src="images/icons/status/lwt.svg" width="20" alt=""> | Show Lineweight | |
| <img src="images/icons/status/transparency.svg" width="20" alt=""> | Show Transparency | |
| <img src="images/icons/status/selcycle.svg" width="20" alt=""> | Selection Cycling | Ctrl+W |
| <img src="images/icons/status/filter.svg" width="20" alt=""> | Selection Filtering | |
| <img src="images/icons/status/quickprops.svg" width="20" alt=""> | Quick Properties | |
| <img src="images/icons/status/isolate.svg" width="20" alt=""> | Isolate Objects | |
| <img src="images/icons/status/cleanscreen.svg" width="20" alt=""> | Clean Screen | Ctrl+0 |

The bar also shows Model/Paper Space, the annotation scale and its options, the drawing units, viewport lock and scale sync on sheets, geographic coordinates when the drawing has a location, and the Quad cursor menu switch.

## Palettes

Palettes are panels that stay open beside the drawing. Dock them on any side, stack them as tabs, or pull one out by its title bar into a window of its own (on a second monitor, say), and dock it back the same way.

| Palette | Command |
|---|---|
| <img src="images/icons/properties.svg" width="20" alt=""> Properties | `PROPERTIES` (Ctrl+1) |
| <img src="images/icons/layers/panel.svg" width="20" alt=""> Layer Manager | `LAYER` |
| <img src="images/icons/blocks/insert.svg" width="20" alt=""> Blocks | `BLOCKSPALETTE` |
| <img src="images/icons/tool_palettes.svg" width="20" alt=""> Tool Palettes | `TOOLPALETTES` |
| <img src="images/icons/xclip.svg" width="20" alt=""> External References | `EXTERNALREFERENCES` |
| <img src="images/icons/quickcalc/quickcalc.svg" width="20" alt=""> QuickCalc | `QUICKCALC` |
| <img src="images/icons/sheetset.svg" width="20" alt=""> Sheet Set Manager | `SHEETSET` |
| <img src="images/icons/layers/laywalk.svg" width="20" alt=""> Layer Walk | `LAYWALK` |
| Count | `COUNTLIST` |
| Point Cloud Manager | `POINTCLOUDMANAGER` |
| Command History | F2 |

## Dialogs

Dialogs never block the drawing: while one is open you can still zoom, select objects and type commands, and several dialogs can be open at once. On the desktop each dialog is a window of its own that you can move anywhere. In the web app dialogs float inside the page.

## Right-click menus and the Quad

Right-clicking in the drawing opens a menu that fits what is under the cursor: with nothing selected it offers repeat, recent commands and clipboard tools; with objects selected it offers the edits that apply to them.

The **Quad** is a compact menu at the cursor. Hover an object or select some, and the Quad shows the commands that work on them, with the most likely one first. Turn it on or off with **Shift+F12** or its status bar button.

## Properties and Quick Properties

The **Properties** palette (Ctrl+1) lists the properties of the selected objects, grouped as General, Geometry, Text and so on, and lets you change them. With several kinds of objects selected, pick the kind from the list at the top, or edit what they all share.

**Quick Properties** shows a small panel with the most used properties next to the selection; turn it on from the status bar.

## Colour theme

Switch between the dark and light themes under **Options › Display**, or with `COLORTHEME`.
