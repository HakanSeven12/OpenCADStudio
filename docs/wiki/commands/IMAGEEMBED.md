---
summary: Places a raster image inside the drawing, so the drawing carries the picture without the image file.
related: IMAGEATTACH, INSERTOBJ, OLELINKS
---
## Usage
1. Pick the image file in the file window. PNG, JPEG, BMP and TIFF files are accepted.
2. The command line shows the file name and its size in pixels.
3. Specify the insertion point (the lower-left corner of the image).
4. Specify the width: move the cursor to the right and click. The height follows the image's proportions.
5. Or press Enter after the insertion point to use a default width of one drawing unit per 100 pixels.

## Notes
- The picture is stored in the drawing as an embedded OLE object, so the drawing file travels alone.
- To keep only a link to the file instead, use [`IMAGEATTACH`](IMAGEATTACH-Command).
