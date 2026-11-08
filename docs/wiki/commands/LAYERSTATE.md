---
summary: Opens the Layer States Manager, where you save, restore, edit and delete named sets of layer settings.
related: LAYERS, LAYER, LAYWALK, LAYISO
---

## Usage
1. Run `LAYERSTATE`.
2. The Layer States Manager opens and lists the layer states saved in the drawing.
3. Click **New**, type a name and an optional description, and click **Save New State** to capture the current layer settings.
4. Select a state and click **Restore** to apply it to the layers.

## Options
You can also type a subcommand after `LAYERSTATE`. This form does not open the dialog.

| Option | What it does |
|---|---|
| **List** (`LIST` or `?`) | Lists the saved layer states. |
| **Save** (`SAVE` or `S`) `<name>` | Saves the current layer settings under a name. An existing state of that name is replaced. |
| **Restore** (`RESTORE` or `R`) `<name> [properties]` | Restores a saved state. You can add a list of the properties to restore. |
| **Rename** (`RENAME` or `REN`) `<old> <new>` | Renames a saved state. |
| **Delete** (`DELETE` or `D`) `<name>` | Deletes a saved state. |
| **Export** (`EXPORT` or `E`) `<name> <file.las>` | Writes a saved state to a `.las` file. |
| **Import** (`IMPORT` or `I`) `<file.las>` | Reads a state from a `.las` file into the drawing. |

The property list for **Restore** can name `ON`, `FROZEN`, `LOCKED`, `PLOT`, `NEWVP`, `COLOR`, `LINETYPE`, `LINEWEIGHT`, `PLOTSTYLE`, `TRANSPARENCY` and `CURRENT`, or `ALL`.

## Notes
- Layer states are stored in the drawing, so they are still there after you save and reopen it.
- In the dialog, **Edit** shows the saved values of each layer. Choose which properties the state restores: on/off, freeze, lock, plot, new viewport freeze, color, linetype, lineweight, plot style, transparency and the current layer.
- **Update from Drawing** replaces a state's saved values with the current layer settings.
- Saving, restoring, renaming and deleting a state can be undone.
- `LAS` and `LMAN` run the same command.
