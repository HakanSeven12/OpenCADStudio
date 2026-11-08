---
summary: Clips a block reference, underlay, raster image or layout viewport to a boundary.
related: XCLIP, IMAGECLIP, PDFCLIP, DWFCLIP, DGNCLIP, VPCLIP
ribbon: Insert tab › Reference panel
---

## Usage
1. Run `CLIP`.
2. At "Select Object to clip:", pick the object.
3. Continue with the prompts of the matching clip command for that object.

## Notes
- For a block reference or external reference, `CLIP` continues as [`XCLIP`](XCLIP-Command).
- For a PDF, DWF or DGN underlay, it continues as the underlay clip ([`PDFCLIP`](PDFCLIP-Command), [`DWFCLIP`](DWFCLIP-Command), [`DGNCLIP`](DGNCLIP-Command)).
- For a raster image, it continues as [`IMAGECLIP`](IMAGECLIP-Command).
- For a layout viewport, it continues as [`VPCLIP`](VPCLIP-Command). The layout's own paper viewport cannot be clipped.
- Any other object gives "*Invalid selection*". Press Enter before picking to end the command.
