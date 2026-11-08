---
summary: Opens the Keyboard Shortcuts dialog, where you view, add, change and remove key bindings.
related: ALIASEDIT, CUIEXPORT, CUIIMPORT, OPTIONS
---

## Usage
1. Start the command. The Keyboard Shortcuts dialog opens with every key and the command it runs.
2. Change a binding, add a new one, or reset all shortcuts to the defaults.
3. Apply your changes. Duplicate keys are ignored, and the dialog asks before it closes with unsaved changes.

## Command-line form
You can also manage shortcuts without the dialog:
- `SHORTCUTS LIST` (or `SHORTCUTS ?`) opens the dialog.
- `SHORTCUTS SET <key> <command>` binds a key, for example `SHORTCUTS SET CTRL+D DIST`.
- `SHORTCUTS CLEAR <key>` removes a binding (`DELETE` and `REMOVE` work too).

## Notes
- Changes are saved with your settings.
- The dialog is also reachable from the Options dialog.
