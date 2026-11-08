---
summary: Locates a point at an offset from a base point while another command asks for a point.
related: MTP, M2P, OSNAP
---

## Usage
1. Start a command that asks for a point, for example `LINE`.
2. At the point prompt, type `FROM`, or choose **From** in the Snap Overrides submenu of the right-click menu.
3. At `Base point:`, pick the reference point. Object snaps work here.
4. At `<Offset>:`, type the offset relative to the base point, for example `@10,5` or `@20<45`. You can also click a point.
5. The resulting point goes back to the command you started.

## Notes
- `FROM` only works while a command is asking for a point. Choosing it from the menu at any other time shows `FROM needs an active point prompt.`
- A dashed guide line runs from the base point to the cursor while you give the offset.
- Press Enter or Esc to cancel `FROM` and return to the command's point prompt.
