---
summary: Creates a mirrored copy of selected objects across a line defined by two points, optionally erasing the originals.
related: 3DMIRROR, COPY, ROTATE, MOVE
ribbon: Draw tab › Modify panel
---

## Usage
1. Select objects, then press Enter. If objects are already selected, this step is skipped.
2. Specify the first point of the mirror line.
3. Specify the second point. A preview shows the mirrored objects.
4. At `Erase source objects? [Yes/No] <No>`, type `Y` to remove the originals, or press Enter to keep them.

## Options
| Option | What it does |
|---|---|
| **Yes** (`Y`) | Mirrors the original objects in place; nothing is left behind. |
| **No** (`N`) | Keeps the originals and adds mirrored copies. This is the default. |

## Notes
- [`MIRRTEXT`](MIRRTEXT-System-Variable) controls text: when it is on, text is reflected like other geometry; when it is off, text keeps reading left to right.
- The mirror line lies in the current working plane.
