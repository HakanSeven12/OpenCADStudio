---
summary: Lists the drawing's linetypes, loads linetypes from a file, or sets the current linetype.
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
