---
summary: Controls whether Open CAD Studio is registered with the operating system as the program that opens DWG and DXF files.
type: On/Off (0/1)
saved: Program settings
default: 1 (On)
related: FILEDIA
---

## Values
| Value | Meaning |
|---|---|
| 0 | Open CAD Studio removes its registration as a handler for these files. |
| 1 | Open CAD Studio registers itself as a handler for .dwg, .dxf and .bak files. |

## Notes
- Setting the value registers or unregisters at once and reports the result on the command line.
- You can also set it in the Options dialog: "Open .dwg and .dxf files with Open CAD Studio".
- Some operating systems ask you to confirm the default program yourself.
