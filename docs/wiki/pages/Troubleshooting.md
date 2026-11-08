# Troubleshooting

## The drawing area is black, flickers or the program does not start

The program draws with the graphics card. Most display problems come from the graphics driver.

1. Update the graphics driver from the card maker's site.
2. Choose another backend under **Options › Graphics › Graphics backend**, then restart. The available backends depend on the system (Vulkan, DirectX 12, Metal or OpenGL).
3. On Linux with a dedicated card, check that its driver is loaded; the program falls back to software drawing otherwise, which is slow.

Very old integrated graphics chips (around ten years old and more) do not offer the features the program needs. On such computers, use the [web app](https://www.opencadstudio.com) in a current browser.

## A drawing does not open, or opens with errors

- If the program offers **recovery**, accept it: it reads what it can and reports what it skipped. Save the result under a new name. See [Files and Formats](Files-and-Formats#damaged-drawings).
- Run `AUDIT` on the opened drawing to list the remaining errors.
- If an autosave or backup exists (`.sv$`, `.bak`, next to the drawing or in the autosave folder), open it directly.

## Text looks wrong

The drawing uses fonts that are not on your computer, and substitutes are used. Open `STYLE`: missing fonts are marked. Copy the font files (`.shx`, `.ttf`) into the drawing's folder, or into a font folder set under **Options › Files**, and reopen the drawing.

## Objects are missing

- Check that their layer is on, thawed and not frozen in the viewport (Layer Manager, VP Freeze column).
- In a layout viewport, annotative objects show only at the scales they support; turn on **Show Annotation Objects** in the status bar to see them all.
- `UNISOLATEOBJECTS` shows objects hidden with `HIDEOBJECTS` or `ISOLATEOBJECTS`.
- `ZOOM` › **Extents** shows everything; an object far away from the rest can make the drawing look empty.

## A large drawing is slow

- Freeze the layers you do not need: frozen layers are skipped entirely.
- Unload external references and underlays you are not using.
- Turn off **Show Lineweight** and **Show Transparency** while you work.
- `PURGE` and `OVERKILL` reduce the size of drawings that collected unused items and duplicates.

## Snaps do not work

- Check that **Object Snap** is on (**F3**) and that the snap you need is ticked (`OSNAP`).
- Objects on locked layers can be snapped to; objects on off or frozen layers cannot.
- Snapping to PDF, DWF and DGN underlays has to be turned on (`PDFOSNAP`, `DWFOSNAP`, `DGNOSNAP`).

## A command does something unexpected

Read the command line: every command says what it is asking for, and the options in square brackets are what you can type. Press **Esc** to cancel, and **Ctrl+Z** to undo.

## Keeping a record

`LOGFILEON` writes everything shown in the command line to a log file, one per session, in the `logs` folder of your settings folder (`LOGFILEPATH`). `LOGFILEOFF` stops it. A log helps when you report a problem.

## Reporting a problem

<img src="images/icons/report.svg" width="20" alt=""> `REPORT` opens a new issue on the project's [issue tracker](https://github.com/HakanSeven12/OpenCADStudio/issues) with the program version and platform already filled in. Describe the steps that lead to the problem, and attach the drawing if you can (zipped). Questions and ideas are welcome in [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions).
