---
summary: Opens a dialog where you add or remove annotation scales on one annotative object.
related: ANNOSCALE, ANNORESET, SCALELISTEDIT
ribbon: Annotate tab › Annotation Scaling panel
---

## Usage
1. Select one annotative object, such as text, multiline text, a block reference or a multileader.
2. Run the command. The Annotation Object Scale dialog opens.
3. Click a scale in the list to add the object's representation for that scale, or click a checked scale to remove it.
4. Click **Close**.

## Command-line form
- `OBJECTSCALE ADD` adds the current annotation scale to the selected objects. `OBJECTSCALE ADD <scale>` adds the named scale.
- `OBJECTSCALE DELETE` removes the current annotation scale; `OBJECTSCALE DELETE <scale>` removes the named one.
- Without a selection, these forms ask you to select objects first. They suit scripts. On the ribbon they are the **Add Current Scale** and **Delete Current Scale** items of the Annotation Scaling drop-down.

## Notes
- The dialog needs exactly one selected object. If the object does not support annotation scales, a message says so.
