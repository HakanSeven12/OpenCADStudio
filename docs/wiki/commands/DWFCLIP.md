---
summary: Clips a DWF underlay to a rectangular or polygonal boundary, or turns its clipping on, off or removes it.
related: CLIP, DWFATTACH, DWFADJUST, PDFCLIP
---

## Usage
1. Select the DWF underlay to clip.
2. Choose a clipping option, or press Enter for **New boundary**.
3. For a new boundary, choose how to draw it, or press Enter for **Rectangular**.
4. Draw the boundary. The part of the underlay outside it is hidden.

## Options
| Option | What it does |
|---|---|
| **ON** (`ON`) | Turns the existing clipping boundary on. |
| **OFF** (`OFF`) | Turns the clipping boundary off, so the whole underlay shows. The boundary is kept. |
| **Delete** (`D`) | Removes the clipping boundary. |
| **New boundary** (`N`) | Draws a new boundary. If the underlay already has one, you are asked **Delete old boundary? [Yes/No]** first; **No** ends the command without changes. |

When you draw a new boundary:

| Option | What it does |
|---|---|
| **Rectangular** (`R`) | Pick two opposite corners. This is the default. |
| **Polygonal** (`P`) | Pick the corner points one by one. Type `U` to undo the last point. After three points, type `C` or press Enter to close the boundary. |
| **Select polyline** (`S`) | Pick an existing polyline with at least three vertices; its vertices form the boundary. |
| **Invert clip** (`I`) | Switches between hiding the outside and hiding the inside of the boundary. The prompt then asks for the boundary type again. |

## Notes
- The selected object must be a DWF underlay. Otherwise the command reports that it was not a DWF underlay.
- [`CLIP`](CLIP-Command) does the same for references, images, any underlay and viewports.
