---
summary: Edits multilines: cleans up crossings, tees and corners, adds or deletes vertices, and cuts or welds the lines.
related: MLINE, MLSTYLE, TRIM
---

## Usage
1. At `Choose an edit tool`, type the tool's key or click it.
2. Follow the prompts for that tool (below).

## Options
| Option | What it does |
|---|---|
| **Closed Cross** (`CC`) | Select two crossing multilines. The second one is drawn whole; the first is broken where they cross. |
| **Open Cross** (`OC`) | Select two crossing multilines. The first is broken at the crossing, and the outer lines of the second open there. |
| **Merged Cross** (`MC`) | Select two crossing multilines. Their inner lines open at the crossing so the two merge. |
| **Closed Tee** (`CT`) | Select the multiline that ends at the other, then the other. The first is trimmed to the second. |
| **Open Tee** (`OT`) | Like Closed Tee, and the second multiline's outer lines open where they meet. |
| **Merged Tee** (`MT`) | Like Closed Tee, and the second multiline's inner lines open where they meet. |
| **Corner Joint** (`CJ`) | Select two multilines. Both are trimmed or extended to meet in a corner. |
| **Add Vertex** (`AV`) | Click a multiline to add a vertex at that point. |
| **Delete Vertex** (`DV`) | Click a multiline near a vertex to remove it. |
| **Cut Single** (`CS`) | Pick a multiline at the first cut point, then the second cut point. The line element you picked is cut between them. |
| **Cut All** (`CA`) | As Cut Single, but all line elements are cut. |
| **Weld All** (`WA`) | Pick a multiline, then the end of the weld range. Cuts in that range are closed again. |
