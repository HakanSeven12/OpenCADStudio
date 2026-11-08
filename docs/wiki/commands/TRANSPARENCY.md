---
summary: Turns transparent pixels on or off for selected raster images.
related: IMAGEATTACH, IMAGEADJUST, IMAGE
---
## Usage
1. At `Select image(s):`, select one or more raster images and press Enter.
2. At `Enter transparency mode [ON/OFF] <ON>:`, type `ON` or `OFF`, or press Enter to keep the value shown.

## Options
| Option | What it does |
|---|---|
| **ON** (`ON`) | The transparent pixels of the image are not drawn, so objects behind it show through. |
| **OFF** (`OFF`) | Every pixel is drawn in its color. |

## Notes
- The default shown in `< >` is the current mode of the first selected image.
- Only raster images are changed. Other objects in the selection, and images on locked layers, are ignored.
- This command does not set object transparency. Use the Properties panel or a layer's transparency for that.
- You can undo the change.
