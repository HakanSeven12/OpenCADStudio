---
summary: Sets the brightness, contrast and fade of the selected raster images in a window with a preview.
related: IMAGEATTACH, IMAGECLIP, ADJUST, TRANSPARENCY
---
## Usage
1. Select the images first, or run the command and pick them at `Select image(s):`. Only raster images are accepted.
2. The **Image Adjust** window opens with a **Before** and **After** preview of the first image.
3. Set **Brightness**, **Contrast** and **Fade**: click or drag a bar, use − / + for one step, or type a whole number from 0 to 100.
4. **Restore defaults** resets the three values.
5. Click **Apply**. The values go to every selected image as one undo step.

## Command-line form
`-IMAGEADJUST` asks for the values on the command line and suits scripts.

1. `Select image(s):`
2. `Enter image option [Contrast/Fade/Brightness] <Brightness>:`
3. Enter the value from 0 to 100. The default is the first image's current value.

| Option | What it does |
|---|---|
| **Contrast** (`C`) | Sets the contrast. |
| **Fade** (`F`) | Sets the fade. |
| **Brightness** (`B`) | Sets the brightness. |

## Notes
- Images on locked layers are not changed.
