---
summary: Draws filled rings or filled circles from an inside and an outside diameter.
related: CIRCLE, PLINE, PEDIT
ribbon: Draw tab › Draw panel
---

## Usage
1. Specify the inside diameter. Type a value, pick two points, or press Enter to keep the value shown.
2. Specify the outside diameter the same way.
3. Specify the center of a donut. A preview follows the cursor.
4. Keep picking centers to place more donuts of the same size. Press Enter to finish.

## Notes
- An inside diameter of `0` gives a filled circle.
- If the inside diameter is larger than the outside one, the two are swapped.
- Each donut is a closed polyline with width, so you can edit it like any polyline.
- The diameters you used are remembered for the next time (`DONUTID`, `DONUTOD`).
- Donuts lie in the current UCS.
- The tool is in the slide-out of the Draw panel.
