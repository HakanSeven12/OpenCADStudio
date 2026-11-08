---
summary: Sets, turns on or off, or deletes the clipping boundary of a DGN underlay.
related: DGNATTACH, DGNADJUST, CLIP, PDFCLIP, DWFCLIP
---

## Usage
1. Select the DGN underlay to clip.
2. Choose a clipping option, or press Enter for **New boundary**.
3. For a new boundary, choose how to draw it, or press Enter for **Rectangular**.
4. Draw the boundary. The part of the underlay outside it is hidden.

## Options
| Option | What it does |
|---|---|
| **ON** (`ON`) | Turns the existing clipping boundary on. |
| **OFF** (`OFF`) | Turns clipping off and shows the whole underlay. The boundary is kept. |
| **Delete** (`D`) | Removes the clipping boundary. |
| **New boundary** (`N`) | Draws a new boundary. If one exists, you are asked whether to delete the old boundary first. |
| **Select polyline** (`S`) | Uses an existing polyline as the boundary. |
| **Polygonal** (`P`) | Pick the boundary points one by one. Type `U` to undo the last point, `C` to close. |
| **Rectangular** (`R`) | Pick two opposite corners. |
| **Invert clip** (`I`) | Switches between hiding the outside and hiding the inside of the boundary. |
