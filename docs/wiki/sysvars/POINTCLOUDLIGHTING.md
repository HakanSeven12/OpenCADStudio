---
summary: Sets how point clouds are lit in shaded visual styles.
type: Integer
saved: Drawing
default: 0 (No lighting)
related: POINTCLOUDLIGHTSOURCE, POINTCLOUDSTYLIZE
---

## Values
| Value | Meaning |
|---|---|
| 0 | No lighting. Points show their own color. |
| 1 | Points that face the light are lit. |
| 2 | Points are lit on both sides. |

## Notes
- Lighting works only in shaded visual styles, not in wireframe or hidden styles.
- Only points that carry normals in the point cloud file are shaded.
