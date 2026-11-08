---
summary: Places a raster image in the drawing as a reference to the image file.
related: IMAGEEMBED, IMAGECLIP, IMAGEADJUST, PDFATTACH, EXTERNALREFERENCES
ribbon: Insert tab › Reference panel
---
## Usage
1. Pick the image file in the file window. PNG, JPEG, BMP, TIFF, WebP and GIF files are accepted.
2. The command line shows the file name and its size in pixels.
3. Specify the insertion point (the lower-left corner of the image).
4. Specify the width: move the cursor to the right and click. A rectangle shows the size; the height follows the image's proportions.
5. Or press Enter after the insertion point to place the image with a default width of one drawing unit per 100 pixels.

## Options
| Option | What it does |
|---|---|
| **Georeferenced** (`G`) | Offered only when the file carries its own placement (GeoTIFF tags or a world file). Places the image at that location in world coordinates. It is also the default: press Enter at the first prompt. |

## Notes
- The drawing stores the path to the image file, not the pixels. To keep the picture inside the drawing, use [`IMAGEEMBED`](IMAGEEMBED-Command).
- The image is placed in the current UCS, except a georeferenced placement, which uses world coordinates.
- You can also attach an image by dropping the file on the drawing.
