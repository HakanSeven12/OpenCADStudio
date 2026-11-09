---
summary: Lists, loads, creates, renames and deletes the drawing's linetypes, or sets the current linetype.
related: LAYER, PROPERTIES, SETBYLAYER, PURGE
---

## Usage
1. At `LINETYPE  [List / Set / Load]:`, type an option.
2. For **Set**, type the linetype name at the next prompt.

## Options
| Option | What it does |
|---|---|
| **List** (`LIST`) | Lists the linetypes in the drawing with their descriptions. |
| **Set** (`SET`) | Makes a linetype current for new objects. Type a linetype name, `ByLayer` or `ByBlock`. |
| **Load** (`LOAD`) | Opens a file dialog. Pick a linetype file, and all its linetypes are added to the drawing. |

## Notes
- You can type the option on one line, for example `LINETYPE SET DASHED` or `LINETYPE LIST`.
- **Set** loads a linetype from the built-in catalog the first time you use it. A name that is neither loaded nor in the catalog is refused.
- To give a layer a linetype, use `-LAYER` with the **Ltype** option, or the Layer Manager.

## Command-line form
`-LINETYPE` asks for an option and then each value, so it suits scripts. It offers everything above, plus:

| Option | What it does |
|---|---|
| **Create** (`CREATE`) | Adds a linetype to the drawing from a name, an optional description and a pattern. |
| **Rename** (`RENAME`) | Renames a linetype. Everything that uses it follows the new name. |
| **Delete** (`DELETE`) | Removes a linetype nothing uses. |
| **Export** (`EXPORT`) | Writes linetypes to a `.lin` file. Give names separated by commas, wildcards such as `F*`, or `*` for all. |
| **Save** (`SAVE`) | Adds a linetype to your own library, or replaces your earlier copy, so it is offered in every drawing. |
| **Reload** (`RELOAD`) | Reads your linetype folders again, so a `.lin` file you added needs no restart. |

The whole command can go on one line. Put a name or description that has spaces in double quotes:

```
-LINETYPE CREATE "My Fence" "Fence line" 1.5,-0.25,0,-0.75
-LINETYPE RENAME "My Fence" FENCE2
-LINETYPE EXPORT "FENCE*,HIDDEN" "/Users/me/My Linetypes.lin"
-LINETYPE SAVE FENCE2
-LINETYPE RELOAD
-LINETYPE DELETE FENCE2
```

A pattern is a comma-separated list of 2 to 12 lengths: a positive number is a dash, a negative number is a gap and `0` is a dot. It must start with a dash or a dot and have at least one gap.

- A name cannot be one the built-in catalog already uses, such as `DASHED`. Use **Set** to load that linetype instead.
- `ByLayer`, `ByBlock`, `Continuous` and linetypes from an external reference cannot be renamed or deleted.
- **Delete** refuses a linetype that is current or still used by an object, layer, block, style or saved layer state. `PURGE` removes the ones nothing uses.
- **Export** and **Save** skip a linetype a `.lin` file cannot hold, and say which and why. Text containing a double quote or a square bracket is one example.
- Text in an exported linetype needs a text style of the same name in the drawing that loads it, or only the dashes load.
- A definition past AutoCAD's own limits (a description over 47 characters, a pattern line over 80 characters, more than 12 elements) is written in full, with a warning.
- A drawing keeps the linetypes it already holds when a library changes.
- For a file path with spaces, use the one-line form with the path in double quotes. An answer at a prompt ends at the first space.
- **Export**, **Save** and **Reload** are not available in the web version.
