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

The whole command can go on one line. Put a name or description that has spaces in double quotes:

```
-LINETYPE CREATE "My Fence" "Fence line" 1.5,-0.25,0,-0.75
-LINETYPE RENAME "My Fence" FENCE2
-LINETYPE DELETE FENCE2
```

A pattern is a comma-separated list of 2 to 12 lengths: a positive number is a dash, a negative number is a gap and `0` is a dot. It must start with a dash or a dot and have at least one gap.

- A name cannot be one the built-in catalog already uses, such as `DASHED`. Use **Set** to load that linetype instead.
- `ByLayer`, `ByBlock`, `Continuous` and linetypes from an external reference cannot be renamed or deleted.
- **Delete** refuses a linetype that is current or still used by an object, layer, block, style or saved layer state. `PURGE` removes the ones nothing uses.
