---
summary: Creates a 3D solid or surface by sweeping one or more profiles along a path curve.
related: EXTRUDE, REVOLVE, LOFT, PRESSPULL
ribbon: Model tab › Create panel
---

## Usage
1. Select the objects to sweep (the profiles), or type an option. Press Enter when done.
2. Select the sweep path, or type an option to set Alignment, Base point, Scale or Twist first.
3. The swept object is created.

## Options
At the profile prompt:

| Option | What it does |
|---|---|
| **Mode** (`MO`) | Choose **Solid** or **Surface** for the result. The default is Solid. |

At the path prompt:

| Option | What it does |
|---|---|
| **Alignment** (`A`) | Whether the profile is turned to stand perpendicular to the path (**Yes**/**No**). |
| **Base point** (`B`) | Pick the point on the profiles that follows the path. |
| **Scale** (`S`) | Scale factor from the start to the end of the sweep. **Reference** sets it from a start length and an end length; **Expression** takes a formula. |
| **Twist** (`T`) | Twist angle along the sweep. **Bank** lets the profile bank along a non-planar path; **EXpression** takes a formula. |

## Notes
- Objects selected before you start are used as profiles.
- The path must be a curve with a real length (line, arc, polyline, spline and similar). A profile cannot be its own path.
- Some paths cannot be used with a scale factor other than 1; you then see "The selected path curve is not valid."
- Wire density of the result follows [`ISOLINES`](ISOLINES-System-Variable).
