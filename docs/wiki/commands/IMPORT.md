---
summary: Imports a file of another format (PDF, DGN, WMF, SVG, 3DS, SAT, IGES or STEP) into the drawing, choosing the importer by format.
related: PDFIMPORT, DGNIMPORT, ACISIN, IMPORTOBJ, INSERT
ribbon: Insert tab › Import panel
---
## Usage
1. Run `IMPORT`. The **Import File** window opens with the last imported file filled in and a list of recent files.
2. Choose the file and, if you want, its format. With **All Files**, the format follows the file extension.
3. Confirm. The file goes to the importer of its format:
   - **PDF**: the PDF import.
   - **DGN**: the **DGN Import** window (levels become layers, cells become blocks).
   - **3D Studio** (`.3ds`): the **3D Studio Import** window, where you pick the objects and materials to bring in.
   - **ACIS** (`.sat`), **IGES** (`.igs`, `.iges`) and **STEP** (`.ste`, `.stp`, `.step`): 3D models.
   - **Metafile** (`.wmf`) and **SVG** (`.svg`, `.svgz`): vector drawings.
   - **Wavefront OBJ** (`.obj`, with **All Files**): one MESH per object of the file, as [`IMPORTOBJ`](IMPORTOBJ-Command) does.

## Command-line form
When [`FILEDIA`](FILEDIA-System-Variable) is 0, `IMPORT` asks `Enter import file name:` on the command line instead of opening the window. You can also type the file name after the command, for example `IMPORT plan.dgn`. This form suits scripts.

## Notes
- The window lists more formats than can be read. Choosing one without an importer shows a message.
- A file that does not exist, or a `.dgn` file that cannot be read, is reported on the command line.
