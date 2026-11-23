---
summary: Creates a profile view of an alignment and its profiles.
related: CREATEPROFILE, CREATEALIGNMENT
---

## Usage
1. Select an alignment that has a profile.
2. Specify the lower left corner of the view.
3. Press Enter, or first set options:
   - **Start station** and **End station**: the stations the view shows; the elevations then fit the profiles between them.
   - **Minimum elevation** and **Maximum elevation**: the elevations the view shows.
   - **Vertical exaggeration**: how much the view stretches elevations against stations.
   - **Bands**: the bands drawn below the view: profile data, vertical geometry, horizontal geometry, all three or none.

## Notes
- The view shows every profile of the alignment at a vertical scale of 10, or at the vertical exaggeration given; another exaggeration uses the drawing's profile view style with it, or a new style named Exaggeration and the value. Without options its stations run from 200 before the alignment's start to 200 past its end and its elevations around the profiles.
- A range needs both of its ends. A view with bands takes the automatic range.
- Without a range the stations are rounded out to hundreds.
- A drawing that lacks the profile view style gets it.
