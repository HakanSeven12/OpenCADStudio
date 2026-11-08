---
summary: Shows file information for the current drawing and lets you edit its title, subject, author, keywords, comments, hyperlink base and custom properties.
related: SAVEAS, HYPERLINK
---

## Usage
1. Run `DWGPROPS`. The Drawing Properties dialog opens.
2. Use the tabs:
   - **General** shows the file name, location, size, when the drawing was created and modified, the total editing time, who saved it last and the revision number. These values cannot be edited here.
   - **Summary** has the **Title**, **Subject**, **Author**, **Keywords**, **Comments** and **Hyperlink base** fields.
   - **Custom** lists your own properties as **Name** and **Value** pairs. Use **Add** and **Remove** to change the list.
3. Click **OK** to keep the changes, or **Cancel** to drop them.

## Notes
- The changes are saved with the drawing and can be undone in one step.
- Each custom property needs a name, and names must be unique (case does not matter). The dialog shows an error otherwise.
- The hyperlink base here is the same value that `HYPERLINKBASE` holds.
- DWG and DXF files older than version 2004 do not store these properties. When you save to such a version, you are warned that the filled-in properties will be lost.
- For a drawing that has not been saved yet, the file details show "(unsaved)".
