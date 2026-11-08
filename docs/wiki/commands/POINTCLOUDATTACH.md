---
summary: Attaches a point cloud scan (.rcs) or scan project (.rcp) to the drawing and places it with an insertion point, scale and rotation.
related: POINTCLOUDMANAGER, POINTCLOUDCROP, POINTCLOUDSTYLIZE, POINTCLOUDCOLORMAP, EXTERNALREFERENCES
ribbon: Insert tab › Reference panel (Attach Underlay drop-down)
web: Point cloud files cannot be attached in the web app.
---

## Usage
1. Pick a point cloud file in the file browser. Both scan files (.rcs) and project files (.rcp) are accepted.
2. The Attach Point Cloud dialog opens. Check the file details and set the path type, placement and options.
3. Click **Attach**.
4. For each placement value left on **On screen**, answer the command-line prompt:
   - `Specify insertion point <0,0>:` pick or type a point. Enter uses the origin of the current UCS.
   - `Specify scale factor <1>:` type a positive number. Enter uses 1.
   - `Specify rotation angle <0>:` type an angle, or pick a second point to give the angle from the insertion point. Enter uses 0.

The command line reports `1 point cloud attached`.

## The Attach Point Cloud dialog
| Area | What it does |
|---|---|
| **Preview** | The project's preview picture, when the file has one. |
| **File details** | Number of scans and points, which data the file holds (RGB, intensity, normals), its size, unit, and where it was found. **Browse...** picks another file. |
| **Path type** | **Full path**, **Relative path** or **No path** (file name only). The saved path is shown below. |
| **Placement** | Insertion point (X, Y, Z), scale and rotation. Each one is either typed here or, with **On screen**, asked for on the command line. |
| **Options** | **Lock point cloud** attaches it locked. **Zoom to point cloud** zooms to it after it is placed (on by default). **Use geographic location** is shown but not available. |

## Command-line form
`-POINTCLOUDATTACH` skips the file browser and the dialog. It asks for everything on the command line, which suits scripts:
1. `Path to point cloud file to attach:` type the file path. A file that does not exist is asked for again.
2. `Specify insertion point <0,0>:`
3. `Specify scale factor <1>:`
4. `Specify rotation angle <0>:`

`POINTCLOUDATTACH` followed by a file name, or with [`FILEDIA`](FILEDIA-System-Variable) set to 0, also uses these prompts.

## Notes
- The command-line form stores the path relative to the drawing. If the drawing has not been saved yet, the full path is stored and made relative on the first save.
- From the command line, the cloud is locked when the `POINTCLOUDLOCK` variable is 1.
- If the file cannot be read, the command line reports `All scans are not found or invalid.` and nothing is attached.
- Attaching is one undo step.
