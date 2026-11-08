---
summary: Sets whether picking one member of a group selects the whole group.
type: Integer
saved: Drawing
default: 1 (Group selection on)
related: GROUP, PICKADD
---

## Values
| Value | Meaning |
|---|---|
| 0 | A pick selects only the object you click. |
| 1 | A pick on a member of a selectable group selects every member of the group. |

This is a bit code; values add up. Only bit 1 (group selection) has an effect in Open CAD Studio. Other bits are stored with the drawing.

## Notes
- Groups that are not selectable are always picked one object at a time.
- The rollover highlight shows the whole group when group selection is on.
