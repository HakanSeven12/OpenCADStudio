---
summary: Creates section views of the sample lines of a group.
related: CREATESAMPLELINES, CREATESURFACE
---

## Usage
1. Select a sample line group.
2. Specify the lower left corner of the section views.

## Notes
- One view per sample line in station order, laid out as a group plot: rows filled from the lower left, as many views a row as the square root of their number, each named by its station and a counter. The group lists the plot.
- A group OpenCAD Studio cannot write again gets one view per line, five a row; the first view's center line then sits at the point.
- A line must sample at most one surface, and its section must not be sampled yet.
- A drawing that lacks the section view style gets it.
