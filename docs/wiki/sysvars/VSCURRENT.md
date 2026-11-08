---
summary: Sets the visual style of the current view.
type: Command
saved: Drawing
default: Wireframe 2D
related: VISUALSTYLES, VSEDGECOLOR, VSEDGESMOOTH
---

## Values
| Value | Meaning |
|---|---|
| WIREFRAME2D (or 2) | Wireframe 2D |
| WIREFRAME3D (or 3, W) | Wireframe 3D |
| HIDDENLINE (or H) | Hidden Line |
| FLATSHADED | Flat Shaded |
| GOURAUDSHADED (or S, R, C) | Gouraud Shaded |
| FLATSHADEDWITHEDGES | Flat Shaded + Edges |
| GOURAUDSHADEDWITHEDGES (or E) | Gouraud Shaded + Edges |

## Notes
- Type `VSCURRENT` to choose from a list, or give the style in one line, for example `VSCURRENT HIDDENLINE`.
- In a paper layout with an active viewport, it sets the style of that viewport.
- `VS` and `SHADEMODE` do the same.
- The style is saved with the viewport in the drawing.
