---
summary: Clips block references and external references to a boundary, or changes or removes an existing clip.
related: CLIP, XATTACH, XREF, VPCLIP
---

## Usage
1. At `Select objects:`, select block references and press Enter. Other objects are left out. If you select block references before you start, this step is skipped.
2. At `Enter clipping option [ON/OFF/Clipdepth/Delete/generate Polyline/New boundary] <New>:`, press Enter for a new boundary or type an option.
3. For a new boundary on a reference that is already clipped, answer `Delete old boundary(s)? [Yes/No] <Yes>:`. **No** ends the command.
4. The command line shows the mode (`Outside mode - Objects outside boundary will be hidden.`). At `[Select polyline/Polygonal/Rectangular/Invert clip] <Rectangular>:`, choose how to give the boundary.
5. Pick two corners for a rectangle, pick points and press Enter for a polygon, or select a closed polyline.

## Options
| Option | What it does |
|---|---|
| **New boundary** (`N`) | Draws a new clip boundary. |
| **ON** / **OFF** | Turns an existing clip on or off without removing it. |
| **Clipdepth** (`C`) | Sets front and back clipping planes. At `Specify front clip point or [Distance/Remove]:` and then the back prompt, pick a point (its height above the boundary is used), type **Distance** (`D`) to enter a distance from the boundary, or **Remove** (`R`) to clear that plane. |
| **Delete** (`D`) | Removes the clip boundary. |
| **generate Polyline** (`P`) | Draws a polyline along the existing clip boundary. |
| **Rectangular** (`R`) | Boundary from two corners. |
| **Polygonal** (`P`) | Boundary from three or more points; **Undo** (`U`) removes the last point. |
| **Select polyline** (`S`) | Uses a closed polyline with at least three vertices. |
| **Invert clip** (`I`) | Switches between hiding what is outside the boundary and hiding what is inside it. |

## Notes
- **ON**, **OFF**, **Clipdepth**, **Delete** and **generate Polyline** need a reference that already has a clip; otherwise you see `No clip boundary found.`
- A front plane that lies behind the back plane is not applied.
- The ribbon's **Clip** tool runs [`CLIP`](CLIP-Command), which hands a selected block reference to `XCLIP`.
- [`XCLIPFRAME`](XCLIPFRAME-System-Variable) controls whether clip boundaries are shown.
