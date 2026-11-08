---
summary: Asks for one of the seven visual styles and applies it to the current viewport.
related: VSCURRENT, VS, SHADEMODE
ribbon: View tab › Visual Style panel
---
## Usage
1. At `Visual style [Wireframe 2D/Wireframe 3D/Hidden Line/Flat Shaded/Gouraud Shaded/Flat Shaded + Edges/Gouraud Shaded + Edges]:`, pick a style.
2. The current viewport redraws in that style, and the command line names the style.

You can also give the style on the same line, for example `VISUALSTYLES HIDDENLINE`.

## Options
| Option | What it does |
|---|---|
| **Wireframe 2D** (`2`) | Shows edges and curves only, flat. |
| **Wireframe 3D** (`3` or `W`) | Shows edges and curves only, in 3D. |
| **Hidden Line** (`H`) | Shows edges, hiding the ones behind faces. |
| **Flat Shaded** | Shades faces without smoothing. |
| **Gouraud Shaded** (`S`, `R` or `C`) | Shades faces smoothly. |
| **Flat Shaded + Edges** | Flat shading with the edges drawn. |
| **Gouraud Shaded + Edges** (`E`) | Smooth shading with the edges drawn. |

## Notes
- This command only switches between the built-in styles. There is no manager for creating or editing your own visual styles.
- The Visual Style drop-down on the ribbon and the style control in the viewport apply the same styles.
- Same as [`VSCURRENT`](VSCURRENT-Command).
