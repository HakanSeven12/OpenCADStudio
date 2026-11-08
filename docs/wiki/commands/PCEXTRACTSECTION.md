---
summary: Traces the points of a point cloud on a live section plane and draws them as lines or polylines.
related: SECTIONPLANE, PCEXTRACTEDGE, PCEXTRACTCORNER, PCEXTRACTCENTERLINE, POINTCLOUDATTACH
---

## Usage
1. Make sure the drawing has a section object with live section turned on (see [`SECTIONPLANE`](SECTIONPLANE-Command)).
2. Start `PCEXTRACTSECTION`.
3. Select the point cloud.
4. In the settings dialog, adjust the options and click **Create**.
5. If **Preview result** is on, the result is shown in green and the command asks **[Accept/Settings/Undo] <Settings>**. Type an option.

## Options
| Option | What it does |
|---|---|
| **Accept** (`A`) | Creates the previewed lines. |
| **Settings** (`S`) | Opens the settings dialog again with the same cloud and section. This is the default. |
| **Undo** (`U`) | Ends the command without creating anything. |

The settings dialog has these settings:

| Setting | What it does |
|---|---|
| **Extract** | **Entire cross section** or **Perimeter only**. |
| **Maximum points to process** | How many points are used. Fewer is faster; more is more accurate. |
| **Output geometry** | **Lines** or **2D Polylines**, with the layer, colour and polyline width to use. |
| **Minimum line length** | Shorter pieces are dropped. |
| **Connect lines tolerance** | How far apart pieces can be and still be joined. |
| **Collinear angle tolerance** | Segments that turn less than this angle (0 to 10 degrees) are merged. |
| **Preview result** | Shows the result and asks before creating it. When off, the lines are created at once. |

## Command-line form
`-PCEXTRACTSECTION` asks only for the point cloud. It uses the current settings and creates the lines at once, without the dialog or the preview. It suits scripts.

## Notes
- If the drawing has no section object, or none has live section turned on, the command stops with a message. The first live section found is used.
- The lines are drawn on the section plane, from the points on the side the section keeps.
- Only structured (segmented) scans give section lines; an unstructured scan gives nothing.
- The settings are kept for the rest of the session.
