---
summary: Creates a point group of the COGO points you select, or of those matching a name, description or elevation rule.
related: CREATEPOINTS, CREATESURFACE
---

## Usage
1. Select COGO points and press Enter, or press Enter without selecting to choose the points by a rule.
2. Press Enter, or first set one option: **Name pattern**, **Description pattern**, **Minimum elevation**, **Maximum elevation**.

## Notes
- Selected points make a group of their numbers.
- Patterns match the whole name or description, case ignored: `*` stands for any run of characters, `?` for one character. Separate several patterns with commas; a point matching any of them belongs to the group.
- Minimum and maximum elevation together keep the points between them; one alone keeps the points above or below it.
- A group takes one rule: selected points, a name pattern, a description pattern or an elevation range.
- The group keeps its rule, so points added later that match it belong to it too.
- The group is named Point Group and a number.
