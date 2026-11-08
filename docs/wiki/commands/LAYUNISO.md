---
summary: Restores the layers that the last `LAYISO` turned off or locked.
related: LAYISO, LAYON, LAYULK, LAYERS
ribbon: Draw tab › Layers panel
---

## Usage
1. Run `LAYUNISO`. The layers isolated by the last [`LAYISO`](LAYISO-Command) go back to how they were.

## Notes
- Only settings that still have the value `LAYISO` gave them are restored. If you changed a layer yourself after `LAYISO`, your change is kept.
- If no layers were isolated, the command only reports that.
- The change can be undone.
