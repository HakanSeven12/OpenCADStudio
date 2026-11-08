---
summary: Attaches a link to a file, a web page or a view of this drawing to the selected objects, or edits the link they have.
related: HYPERLINKOPTIONS, HYPERLINKBASE
ribbon: Insert tab › Data panel
---
## Usage
1. Select the objects, then run `HYPERLINK`. If nothing is selected, the command asks you to select objects; press Enter when done.
2. The **Insert Hyperlink** window opens (**Edit Hyperlink** when the objects already have a link).
3. Choose what the link points to:
   - **File or web page**: type the **Address** (a file, a web address, or `file#location` to open a view or layout of it), or click **File...** to browse. Turn on **Relative path** to store the path relative to the drawing.
   - **View of this drawing**: pick one of the **Views and layouts** of the current drawing.
4. Type the **Text to display**, the text the link shows.
5. Click **OK** to apply. **Remove Link** removes the link from the objects. When the selected objects have different links, the window says so.

## Command-line form
`-HYPERLINK` works on the command line only and suits scripts.

1. `Enter an option [Remove/Insert] <Insert>:`
2. For **Insert**: `Enter hyperlink insert option [Area/Object] <Object>:`
   - **Object**: select the objects.
   - **Area**: specify the first corner and the other corner. A closed rectangle is drawn on the layer `URLLAYER` (created in red if it does not exist) and the link goes on it.
3. `Enter hyperlink <current drawing>:` type the file or web address.
4. `Enter named location <none>:` type a view or layout name, or press Enter.
5. `Enter description <none>:` type the text to display, or press Enter.

For **Remove**: select the objects. Their links are listed with numbers. At `Enter number, hyperlink, or * for all:` type a number, an address, or `*` to remove them all.

| Option | What it does |
|---|---|
| **Insert** (`I`) | Adds a link to objects or to a new rectangular area. |
| **Remove** (`R`) | Removes links from the selected objects. |
| **Area** (`A`) | Draws a rectangle on `URLLAYER` and links it. |
| **Object** (`O`) | Links the objects you select. |

## Notes
- You can also type `HYPERLINK <address>` with objects selected: the address of their link changes and the text and named location stay. Objects on locked layers are skipped.
- Every change can be undone.
