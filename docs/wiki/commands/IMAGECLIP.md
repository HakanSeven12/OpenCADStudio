---
summary: Clips a raster image to a rectangular or polygonal boundary, or turns its clipping on, off or removes it.
related: IMAGEATTACH, IMAGEADJUST, CLIP, XCLIP, PDFCLIP
---
## Usage
1. At `Select image to clip:`, pick a raster image.
2. At `Enter image clipping option [ON/OFF/Delete/New boundary] <New>:`, choose an option or press Enter for a new boundary.
3. If the image is already clipped, answer `Delete old boundary? [No/Yes] <Yes>:`. **No** ends the command and keeps the old boundary.
4. The command line shows the clipping mode (outside or inside), then asks: `[Select polyline/Polygonal/Rectangular/Invert clip] <Rectangular>:`.
5. Draw the boundary:
   - **Rectangular**: specify the first corner and the opposite corner.
   - **Polygonal**: specify points; after three points type `C` or press Enter to close. `U` removes the last point.
   - **Select polyline**: pick a polyline with at least three vertices.

## Options
| Option | What it does |
|---|---|
| **ON** (`ON`) | Turns the existing clipping boundary on. |
| **OFF** (`OFF`) | Turns clipping off; the boundary is kept. |
| **Delete** (`D`) | Removes the boundary; the whole image shows again. |
| **New boundary** (`N`) | Draws a new boundary. |
| **Invert clip** (`I`) | Switches between hiding what is outside the boundary and hiding what is inside. |
| **Close** (`C`) | Closes a polygonal boundary. |
| **Undo** (`U`) | Removes the last polygon point. |

## Notes
- Picking an object that is not an image asks for an image again.
- [`CLIP`](CLIP-Command) also clips images when you pick one.
