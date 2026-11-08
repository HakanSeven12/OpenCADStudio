---
summary: Creates a wipeout, a polygon that masks the objects behind it with the background color.
related: TEXTMASK, DRAWORDER, REVCLOUD
ribbon: Annotate tab › Markup panel
---

## Usage
1. At `WIPEOUT  Specify first point or [Frames/Polyline] <Polyline>:`, pick the first point of the boundary.
2. Pick the next points. From the third point on you can type `C`, press Enter, or click the first point again to close the wipeout.

To use an existing polyline instead, press Enter (or type `P`) at the first prompt:
1. At `WIPEOUT  Select a closed polyline:`, pick the polyline.
2. At `WIPEOUT  Erase polyline? [Yes/No] <No>:`, choose whether the polyline is erased.

## Options
| Option | What it does |
|---|---|
| **Polyline** (`P`) | Makes the wipeout from a closed polyline. |
| **Frames** (`F`) | Asks `Enter mode [ON/OFF/Display but not plot]`: **ON** shows and plots the frames of all wipeouts, **OFF** hides them, **Display but not plot** (`D`) shows them on screen only. Enter keeps the current mode. |
| **Close** (`C`) | Closes the boundary (three points or more). |
| **Undo** (`U`) | Takes back the last point(s). |

## Notes
- The polyline must be closed and made of straight segments only. A polyline with width is not accepted.
- The frame mode applies to every wipeout in the drawing; it is the [`WIPEOUTFRAME`](WIPEOUTFRAME-System-Variable) setting.
- `WIPEOUT RECTANGULAR` (or `WIPEOUT R`) asks for two corners and makes a rectangular wipeout; `WIPEOUT POLYLINE` goes straight to the polyline prompt.
