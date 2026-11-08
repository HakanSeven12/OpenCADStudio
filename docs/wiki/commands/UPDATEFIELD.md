---
summary: Recalculates the fields in the selected objects so they show current values.
related: FIELD, REGEN
ribbon: Insert tab › Data panel
---
## Usage
1. At `Select objects:`, select the objects that hold fields and press Enter. You can also select them before you start.
2. The command line reports how many fields were found and how many were updated.

## Notes
- Fields in text, multiline text, attributes, and in the contents of block references and tables are updated.
- If the selection holds no fields, the command reports `0 field(s) found.`
- Each run is one undo step, even when no value changed.
