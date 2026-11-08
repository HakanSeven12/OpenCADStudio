---
summary: Clips a PDF underlay to a boundary, or turns, removes or replaces its clipping boundary.
related: CLIP, IMAGECLIP, XCLIP, PDFATTACH, PDFADJUST
---

## Usage
1. Select the PDF to clip.
2. Enter a clipping option, or press Enter for **New boundary**.
3. If the underlay already has a boundary, answer whether to delete the old one.
4. Choose how to draw the boundary, or press Enter for **Rectangular**.
5. Specify the boundary: two corners, the points of a polygon, or an existing polyline.

## Options
| Option | What it does |
|---|---|
| **ON** (`ON`) | Turns the existing clipping boundary on. |
| **OFF** (`OFF`) | Turns the clipping boundary off, so the whole underlay shows. |
| **Delete** (`D`) | Removes the clipping boundary. |
| **New boundary** (`N`) | Draws a new boundary. |
| **Select polyline** (`S`) | Uses an existing polyline as the boundary. |
| **Polygonal** (`P`) | Draws the boundary point by point. Type **Close** (`C`) to close it or **Undo** (`U`) to remove the last point. |
| **Rectangular** (`R`) | Draws a rectangular boundary by two corners. |
| **Invert clip** (`I`) | Switches between hiding what is outside the boundary (the default) and hiding what is inside it. |

## Notes
- The command line shows the current mode: outside mode hides what is outside the boundary, inside mode hides what is inside.
- [`CLIP`](CLIP-Command) chooses the right clip command for the object you select.
