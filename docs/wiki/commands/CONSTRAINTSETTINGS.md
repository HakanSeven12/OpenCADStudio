---
summary: Opens the Auto Constrain settings, which set the constraint types AUTOCONSTRAIN applies, their order and the tolerances.
related: AUTOCONSTRAIN, CONSTRAINTBAR, DELCONSTRAINT
---

## Usage
1. Type `CONSTRAINTSETTINGS`. The Auto Constrain dialog opens.
2. Change the settings.
3. Click **OK** to keep them and close, **Apply** to keep them and stay, or **Cancel** to discard them.

## Dialog
| Control | What it does |
|---|---|
| Constraint list | Each constraint type has a priority and an **Apply** check box. Only checked types are applied. |
| **Move Up** / **Move Down** | Changes the priority of the selected type. |
| **Select All** / **Clear All** | Checks or clears every type. |
| **Reset** | Restores all default settings, tolerances included. |
| **Tangent objects must share an intersection point** | Only adds a tangent constraint when the objects meet. |
| **Perpendicular objects must share an intersection point** | Only adds a perpendicular constraint when the objects meet. |
| **Distance** / **Angle** tolerances | How close objects must be, in distance and in degrees, to be constrained. |
