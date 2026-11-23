---
summary: Creates an intersection of two alignments that cross or meet.
related: CREATEALIGNMENT, CREATEPROFILE, CREATECORRIDOR
---

## Usage
1. Select the main road alignment.
2. Select the crossing road alignment.
3. Press Enter, or first set **Offset width**: the width of offset alignments on each side of both roads (0 for none).

## Notes
- The intersection uses the alignments' newest profiles when both have one.
- The alignments must meet once, on straight segments: crossing away from their ends, or one starting or ending on the other (a tee junction, written without profiles; the through road becomes the main road).
- With an offset width, each road gets a left and a right offset alignment named after the road and the width, kept by actions that follow the road, and the intersection gets its location label. Without it only the intersection object is written.
- Curb returns and corridor regions are not written; see CREATECURBRETURN for curb returns.
