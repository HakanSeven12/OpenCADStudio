---
summary: Removes the part of an object you click, cut back to where it meets other objects.
related: EXTEND, EXTRIM, BREAK, FILLET, LENGTHEN
ribbon: Draw tab › Modify panel (Trim drop-down)
---
## Usage
1. At `Click segment to remove (Shift+click extends):`, click the part of an object you want to remove. A preview shows what will go before you click.
2. Keep clicking more segments, or type an option.
3. Press Enter or Esc to finish.

In **Standard** mode the command first asks `Select cutting edges [0 picked, Enter = done]:`. Click the objects that cut, then press Enter, then click the segments to remove.

## Options
| Option | What it does |
|---|---|
| **Cutting edges** (`T`) | Choose the cutting edges yourself. Click objects to add or remove them, then press Enter. Esc returns to the previous edges. |
| **Fence** (`F`) | Pick points of a fence line, then press Enter. Every segment the fence crosses is trimmed. |
| **Crossing** (`C`) | Pick two corners of a box. The segments inside the box are trimmed. |
| **Edge** (`E`) | Toggles between **Extend** and **No extend**. With **Extend**, cutting edges count as if they were extended, so an object can be cut where an edge would reach it. Not offered in Quick mode. |
| **Erase** (`R`) | Click objects to delete them without trimming. Press Enter to go back. |
| **mOde** (`O`) | Choose **Quick** (`Q`) or **Standard** (`S`). |

## Notes
- **Quick** mode is the default: every visible object is a cutting edge, so you can start clicking at once.
- If you select objects before you start, they become the cutting edges.
- Hold Shift while you click to extend the object instead of trimming it.
- Lines, arcs, circles, rays, construction lines, ellipses, splines, polylines, multilines and hatches can be trimmed. A hatch is trimmed as a fill, along the cutting edges.
- Block references and their contents can act as cutting edges, but are not trimmed themselves. Multilines are trimmed but never cut other objects.
- The chosen mode is kept for later TRIM and EXTEND runs in the same session.
- Each trim can be undone.
