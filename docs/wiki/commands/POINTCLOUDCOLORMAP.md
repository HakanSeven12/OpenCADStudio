---
summary: Opens the Point Cloud Color Map dialog, where you edit the color schemes and ranges used to color point clouds by intensity or elevation.
related: POINTCLOUDSTYLIZE, POINTCLOUDMANAGER, POINTCLOUDATTACH
---

## Usage
1. `Select point cloud or [None] <None>:` pick a point cloud, or press Enter (or type `N`) to edit the drawing's default schemes.
2. The Point Cloud Color Map dialog opens. Make your changes.
3. Click **OK**, or **Apply** to keep the dialog open.

## Options
| Option | What it does |
|---|---|
| **None** (`N`) | Opens the dialog without a point cloud, for the drawing's color schemes only. |

## The dialog
| Area | What it does |
|---|---|
| **Intensity** / **Elevation** tabs | Choose which coloring you edit. A Classification tab is shown but not available. |
| **Color scheme** | Pick a scheme. **New**, **Rename** and **Delete** manage schemes. Set the **Number of colors** and whether it is shown with **Display as gradient**. |
| **Range of colorized points** | Intensity: **Maximum intensity** and **Minimum intensity**. Elevation: **Apply to extents of point cloud**, or **Maximum elevation**, **Minimum elevation** and **Interval height**. **Out of range points** sets how points outside the range are shown. |
| **Make stylization current** | Switches the cloud to the coloring of the open tab. |

## Notes
- The range fields can only be edited when you picked a point cloud.
- Schemes belong to the drawing, so every point cloud that uses a scheme is redrawn when it changes.
- OK and Apply are one undo step.
