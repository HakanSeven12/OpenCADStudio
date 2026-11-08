---
summary: Creates a 3D solid or surface through a series of cross-section curves, optionally guided by guide curves or a path.
related: EXTRUDE, SWEEP, REVOLVE
ribbon: Model tab › Create panel
---

## Usage
1. Select cross sections in lofting order, or type an option. Objects you selected before the command start the list in the order you picked them.
2. Press Enter when you have at least two cross sections.
3. At `Enter an option`, press Enter to loft through the cross sections only, or choose Guides, Path or Settings.
4. The new solid or surface is created and selected.

## Options
At the cross-section prompt:

| Option | What it does |
|---|---|
| **Point** (`POINT`) | Uses a point as the first or last cross section, so the loft ends in a tip. |
| **Join** (`JOIN`) | Picks several connected edges that together form one cross section. Press Enter to finish the group. |
| **Mode** (`MODE`) | Chooses **Solid** or **Surface**. The default is Solid. |

At the `Enter an option` prompt:

| Option | What it does |
|---|---|
| **Guides** (`GUIDES`) | Select guide curves that control the shape between sections. Press Enter to create the loft. |
| **Path** (`PATH`) | Select one path curve. The loft is created as soon as you pick it. |
| **Cross sections only** (`CROSSSECTIONS`) | Creates the loft from the sections alone (same as pressing Enter). |
| **Settings** (`SETTINGS`) | Opens the settings below. Choose **Done** or press Enter to return. |
| **Continuity** (`CONTINUITY`) | Only with a point end: G0 or G1 continuity at that end. |
| **Bulge magnitude** (`BULGE`) | Only with a point end: how far the shape bulges at that end. |

Settings:

| Option | What it does |
|---|---|
| **Normals** (`NORMALS`) | Ruled, Smooth, First normal, Last normal, Ends normal, All normal, or Use draft angles. |
| **Draft angles** (`DRAFTANGLES`) | Start and end draft angle (0 to 180 degrees). Needs Normals set to Use draft angles. |
| **Magnitudes** (`MAGNITUDES`) | Start and end magnitude for the draft angles. Needs Normals set to Use draft angles. |
| **Closed** (`CLOSED`) | Yes/No: closes the loft back to the first section. Needs at least three curve sections and no point ends. |
| **Align direction** (`ALIGNDIRECTION`) | Yes/No: aligns the direction of the cross sections. |
| **Undo** (`UNDO`) | Steps back one choice. |

## Notes
- Cross sections must be planar curves or regions. Point ends are allowed only at the first or last place, with a curve between them.
- A preview shows the result while you work.
- [`DELOBJ`](DELOBJ-System-Variable) decides whether the cross sections, guides and path are deleted afterwards. If a source to be deleted is on a locked layer, the loft is not created.
- A surface lofted from single-object sections, with no guides or path, can stay associative and follow later changes to its sections.
