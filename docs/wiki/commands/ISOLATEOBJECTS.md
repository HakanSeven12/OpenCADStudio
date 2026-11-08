---
summary: Hides every object except the selected ones for the current session, until you run `UNISOLATEOBJECTS`.
related: HIDEOBJECTS, UNISOLATEOBJECTS, LAYISO
---
## Usage
1. Select the objects you want to keep visible.
2. Run `ISOLATEOBJECTS`. All other objects are hidden and the command line reports how many objects were isolated.

## Notes
- Select the objects first. With nothing selected, the command stops with a message.
- Isolation is temporary and does not change the objects or their layers. Run [`UNISOLATEOBJECTS`](UNISOLATEOBJECTS-Command) to show everything again.
- Only objects in the current space (or in the block you are editing) are hidden.
- You can undo the isolation.
- The command is also on the right-click menu (**Isolate Objects**) and in the isolate button menu on the status bar.
