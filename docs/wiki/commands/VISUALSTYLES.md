---
summary: Opens the Visual Styles Manager, where you pick the current viewport's visual style and set the edge settings.
related: VSCURRENT, VS, SHADEMODE, VSEDGESMOOTH, VSOCCLUDEDEDGES, DISPSILH
ribbon: View tab › Visual Style panel
---
## Usage
1. Run `VISUALSTYLES` (or `VSM`, or **View › Visual Styles › Visual Styles Manager…** in the menu bar).
2. Under **Available Visual Styles**, click a style; the current viewport redraws in it. The highlighted style is the one in use.
3. Under **Edge Settings**, change how edges are drawn. Changes apply at once.
4. Click **Close**.

You can also give the style on the same line, for example `VISUALSTYLES HIDDENLINE`, to set it without opening the manager.

## Edge settings
| Setting | What it does |
|---|---|
| **Crease angle** (`VSEDGESMOOTH`) | Mesh edges between faces that meet at less than this angle (0–180 degrees) are not drawn, so the facets of curved meshes do not show. Type the angle and press Enter. |
| **Show occluded edges dashed in Hidden Line** (`VSOCCLUDEDEDGES`) | In the Hidden Line style, edges behind faces are drawn dashed instead of hidden. |
| **Show silhouette edges on solids** (`DISPSILH`) | Draws the silhouette edges of curved solids. Saved with the drawing. |

## Notes
- The manager switches between the seven built-in styles (Wireframe 2D, Wireframe 3D, Hidden Line, Flat Shaded, Gouraud Shaded and the two "+ Edges" styles); you cannot create your own styles yet.
- The Visual Style drop-down on the ribbon and the style control in the viewport apply the same styles. To pick a style at the command line, use [`VSCURRENT`](VSCURRENT-Command) (or `VS`).
