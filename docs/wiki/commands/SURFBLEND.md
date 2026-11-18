---
summary: Creates a smooth surface between the edges of two surfaces.
related: SURFPATCH, SURFFILLET
---

## Usage
1. Pick an edge of the first surface.
2. Pick an edge of the second surface.
3. Press Enter to accept, or choose Continuity or Bulge magnitude first.

## Notes
- Continuity sets how the blend meets both surfaces: 0 (position, G0), 1 (tangent, G1) or 2 (curvature, G2). The default is G1.
- Bulge magnitude (0 to 1) sets how far the blend swells out from the edges; 0.5 is the default.
