---
summary: Moves selected objects between model space and paper space through a layout viewport, keeping their size and position on the sheet.
related: MSPACE, PSPACE, MVIEW
---

## Usage
1. Switch to a layout tab.
2. Select the objects, or run `CHSPACE` and select them, then press Enter.
3. If you are on the sheet and the layout has more than one viewport, the prompt asks you to make the target viewport active and press Enter.
4. The objects move to the other space. The command reports how many objects changed space.

## Notes
- Inside an active viewport, the selected model objects go to paper space. On the sheet, the selected paper objects go into model space through the viewport under them (or the one you made active).
- The objects are scaled and rotated by the viewport's scale and twist, so they look the same size and stay in the same place.
- The command does not run in the Model tab.
- If there is no plan-view viewport to pass through, you see "No plan-view viewport to change space through."
- The menu bar lists it as **Change Space**.
