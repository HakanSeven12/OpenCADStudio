# Multilines

A multiline is a single object made of several parallel lines, drawn in one pass: walls with their faces and core, roads with kerbs and centre line, cable trays.

## Drawing

<img src="images/icons/polyline.svg" width="20" alt=""> `MLINE` (`ML`) draws a multiline through the points you pick. Before the first point you can set:

| Option | What it sets |
|---|---|
| **Justification** | Which line follows the picked points: **Top**, **Zero** (the centre) or **Bottom**. |
| **Scale** | The width factor: the spacing of the style's lines is multiplied by it. |
| **Style** | The multiline style to use. |

**Close** joins the last point to the first; **Undo** removes the last segment. The current justification, scale and style are kept in `CMLJUST`, `CMLSCALE` and `CMLSTYLE`.

## Multiline styles

`MLSTYLE` manages multiline styles. A style lists its lines (each with an offset from the centre, a colour and a linetype) and can add end caps, a fill colour and the display of joints. Styles are saved in the drawing.

## Editing

`MLEDIT` cleans up where multilines meet: cross and tee intersections (closed, open or merged) and corner joints. It also adds or deletes vertices, cuts one line or all lines of a multiline, and welds cuts back together. Grips move the vertices; `EXPLODE` turns a multiline into separate lines.
