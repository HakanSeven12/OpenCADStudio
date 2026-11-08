---
summary: Sets the base folder or address that relative hyperlinks in the drawing are resolved against.
type: String
saved: Drawing
default: "" (empty: the folder of the drawing itself)
related: HYPERLINK
---

## Values
Any path or address. Type `.` to clear it. When it is empty, relative hyperlinks are resolved against the folder of the drawing (an unsaved drawing has no folder).

## Notes
- Used when you open a hyperlink added with [`HYPERLINK`](HYPERLINK-Command) whose address is relative.
- Changing it can be undone.
