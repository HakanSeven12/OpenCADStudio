---
summary: Creates an intersection of two alignments that cross or meet.
related: CREATEALIGNMENT, CREATEPROFILE, CREATECORRIDOR
---

## Usage
1. Select the main road alignment.
2. Select the crossing road alignment.

## Notes
- A crossing uses the alignments' newest profiles when both have one.
- The alignments must meet once, on straight segments: crossing away from their ends, or one starting or ending on the other (a tee junction, written without profiles; the through road becomes the main road).
- Only the intersection object is written: no offset alignments, curb returns or corridor regions.
