---
summary: Creates a profile of an offset alignment from its parent's profile.
related: CREATEOFFSETALIGNMENT, CREATEPROFILE, CREATEPROFILEVIEW
---

## Usage
1. Select an offset alignment.
2. Specify the slope across the offset (rise over run): the profile is the parent's profile raised by the slope times the offset distance. 0 copies it unchanged.

## Notes
- The source is the newest profile of the offset alignment's parent.
- The parent's profile is copied entity by entity, cut where the offset alignment's elements change and moved to the offset alignment's stations; it stays tied to the source.
- The offset alignment must have one offset along its whole length (no widenings), and the source must be made of tangents and symmetric parabolas within the stations the offset alignment follows.
- The profile is named Profile and a number.
