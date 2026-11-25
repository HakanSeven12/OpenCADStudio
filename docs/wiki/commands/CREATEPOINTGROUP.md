---
summary: Opens the Point Group Properties window to make a point group from point numbers, elevations, names, descriptions, other groups or a query.
related: CREATEPOINTS, CREATEPOINTSFROMCORRIDOR, CREATESURFACE
---

## Usage
1. Run the command; the Point Group Properties window opens.
2. On **Information**, set the name, description and the group's point and label styles.
3. Choose the points on **Point Groups**, **Include**, **Exclude** or **Query Builder**, and set **Overrides** if the group should change the points' raw description, elevation or styles.
4. Check **Point List** and **Summary**, then press **OK** (or **Apply** to keep the window open).

## Notes
- Number rules take a comma list of numbers and `a-b` ranges; elevation rules take `v`, `<v`, `>v`, `<=v`, `>=v` and `a-b`. A point without elevation matches no elevation rule.
- Name and description rules take comma lists of `*` / `?` patterns, case ignored unless the Query Builder's case-sensitive option is on.
- Include rules are joined by OR; Exclude rules take points away.
- **Selection Set in Drawing >>** fills the number rule with the COGO points you pick.
- **Modify query** turns the rules into Query Builder rows (AND, OR, NOT, parentheses).
- The group keeps its rule, so points added later that match it belong to it too.
- Apply and OK are one undo step each.
