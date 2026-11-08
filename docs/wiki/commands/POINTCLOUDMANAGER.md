---
summary: Opens the Point Cloud Manager palette, which lists the scans of the drawing's point clouds and lets you show or hide them.
related: POINTCLOUDMANAGERCLOSE, POINTCLOUDATTACH, POINTCLOUDCROP, EXTERNALREFERENCES
---

## Usage
1. Run the command. The Point Cloud Manager palette opens docked on the right side.
2. Expand a point cloud to see its rows, and use each row's check box to show or hide those points.

## The palette
| Item | What it does |
|---|---|
| Point cloud row | Shows or hides the whole cloud. |
| **Regions** | Shown, but not available. |
| **Unassigned Points** | Shows or hides the points that belong to no region. |
| **Scans** | Shows or hides all scans; expand it to switch single scans. |
| **Collapse all** / **Expand all** | Folds or unfolds the tree. |
| **Search** | Filters the tree by name. |

## Notes
- When point clouds are selected, only those clouds are listed. Otherwise all point clouds in the drawing are listed.
- A parent check box shows a mixed state when only some of its children are on.
- Each switch is one undo step.
- Close the palette with [`POINTCLOUDMANAGERCLOSE`](POINTCLOUDMANAGERCLOSE-Command).
