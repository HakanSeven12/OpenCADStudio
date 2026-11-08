---
summary: Hides the selected objects for the current session until you run `UNISOLATEOBJECTS`.
related: ISOLATEOBJECTS, UNISOLATEOBJECTS, LAYOFF, LAYISO
---
## Usage
1. Select the objects you want to hide.
2. Run `HIDEOBJECTS`. The objects disappear and the command line reports how many were hidden.

## Notes
- Select the objects first. With nothing selected, the command stops with a message.
- Hiding is temporary and does not change the objects or their layers. Run [`UNISOLATEOBJECTS`](UNISOLATEOBJECTS-Command) to show them again.
- You can undo the hide.
- The command is also on the right-click menu (**Hide Objects**) and in the isolate button menu on the status bar.
