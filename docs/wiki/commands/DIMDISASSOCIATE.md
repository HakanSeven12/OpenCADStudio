---
summary: Removes the association between dimensions and the objects they measure.
related: DIMREASSOCIATE, DIMEDIT
---

## Usage
1. Select the dimensions, or run the command with dimensions already selected.
2. Press Enter. The selected dimensions lose their link to the measured objects, and the command line reports how many were disassociated.

## Notes
- Only associative dimensions can be selected.
- Dimensions on locked layers are not changed.
- A disassociated dimension no longer updates when the measured object changes. Use [`DIMREASSOCIATE`](DIMREASSOCIATE-Command) to link it again.
- See also [`DIMASSOC`](DIMASSOC-System-Variable).
