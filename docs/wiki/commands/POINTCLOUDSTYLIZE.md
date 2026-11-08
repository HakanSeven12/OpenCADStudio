---
summary: Sets how the selected point clouds are colored: by scan color, object color, intensity, elevation or normal.
related: POINTCLOUDCOLORMAP, POINTCLOUDMANAGER, POINTCLOUDATTACH
---

## Usage
1. `Select point cloud objects:` select one or more point clouds and press Enter. Other objects in the selection are ignored.
2. `Enter a stylization option [RGB/Object color/Intensity/Elevation/Normal/Classification] <RGB>:` type an option, or press Enter for the default.

## Options
| Option | What it does |
|---|---|
| **RGB** (`R`) | Uses the colors stored in the scan. |
| **Object color** (`O`) | Uses the point cloud's own color. |
| **Intensity** (`I`) | Colors by intensity, with the intensity color scheme. |
| **Elevation** (`E`) | Colors by height, with the elevation color scheme. |
| **Normal** (`N`) | Colors by the direction each point faces. |
| **Classification** (`C`) | Listed, but not supported yet. |

## Notes
- The last option you chose is the next default.
- A cloud whose file lacks the data (for example no intensity or no normals) is left unchanged, and the command line names it.
- Clouds on locked layers are skipped.
- Edit the intensity and elevation schemes with [`POINTCLOUDCOLORMAP`](POINTCLOUDCOLORMAP-Command).
- The change is one undo step.
