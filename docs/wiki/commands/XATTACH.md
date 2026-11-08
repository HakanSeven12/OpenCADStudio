---
summary: Attaches a DWG or DXF drawing to the current drawing as an external reference.
related: XREF, ATTACH, XCLIP, XOPEN, XRELOAD, EXTERNALREFERENCES
ribbon: Insert tab › Reference panel (Attach XREF)
---

## Usage
1. Run `XATTACH`. In the file dialog, choose a drawing (DWG, DXF or backup file).
2. In the Attach External Reference dialog, check the settings and click **OK**.
3. Answer the prompts for every value marked **Specify On-screen**:
   - `Specify insertion point or [Scale/X/Y/Z/Rotate/PScale/PX/PY/PZ/PRotate]:`
   - `Enter X scale factor, specify opposite corner, or [Corner/XYZ] <1>:` and `Enter Y scale factor <use X scale factor>:`
   - `Specify rotation angle <0>:`

## Dialog
| Setting | What it does |
|---|---|
| **Name** / **Browse...** | The drawing to attach; Browse picks another file. A preview is shown. |
| **Reference Type** | **Attachment** shows the reference in drawings that reference this one too; **Overlay** does not. |
| **Scale** | The X, Y and Z scale, or **Specify On-screen**. **Uniform Scale** keeps them equal. |
| **Insertion point** | The X, Y and Z of the insertion point, or **Specify On-screen**. |
| **Path type** | How the path is stored: **Full path**, **Relative path** or **No path**. |
| **Rotation** | The angle, or **Specify On-screen**. |
| **Block Unit** | The unit of the attached drawing and the scale factor that converts it to this drawing's units. |
| **Show Details** | Shows where the file was found and the path that will be saved. |

## Options
| Option | What it does |
|---|---|
| **Scale** (`S`), **X**, **Y**, **Z** | Fix the scale (all axes, or one axis) before you pick the insertion point. |
| **Rotate** (`R`) | Fixes the rotation angle before you pick the insertion point. |
| **PScale** (`PS`), **PX**, **PY**, **PZ**, **PRotate** | Change only the preview scale or rotation while you place the reference. |
| **Corner** / **XYZ** | At the scale prompt: set the scale by an opposite corner, or by X, Y and Z factors. |

## Notes
- `XATTACH <file>` skips the file dialog and opens the Attach External Reference dialog for that file.
- When [`FILEDIA`](FILEDIA-System-Variable) is 0, the command asks `-XREF  Enter name of file to attach:` on the command line instead.
- The reference definition and its insertion are one undo step.
