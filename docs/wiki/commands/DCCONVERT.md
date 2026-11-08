---
summary: Converts associative dimensions into dimensional constraints.
related: DIMCONSTRAINT, DCFORM, PARAMETERS
ribbon: Parametric tab › Dimensional panel
---

## Usage
1. Select associative dimensions to convert.
2. Press Enter. Each dimension becomes a dimensional constraint driven by a new parameter.

## Notes
- Only associative dimensions can be selected.
- A dimension is skipped if it is on a locked layer, already drives a constraint, or cannot be turned into a valid constraint.
- The converted constraints use the current constraint form, set by [`DCFORM`](DCFORM-Command) or [`CCONSTRAINTFORM`](CCONSTRAINTFORM-System-Variable).
- Each new parameter is named the same way as for a constraint you add by hand.
