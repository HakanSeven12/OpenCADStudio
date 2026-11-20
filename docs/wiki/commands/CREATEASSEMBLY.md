---
summary: Creates an assembly of a road's two half sections.
related: CREATECORRIDOR, CREATEALIGNMENT, CREATEPROFILE
---

## Usage
1. Specify the location of the assembly's baseline.
2. Press Enter, or first set options: **Lane width**, **Lane slope**, **Shoulder width**, **Shoulder slope**, **Pavement depth**, **Side slope width**, **Side slope**.

## Notes
- Each side is three subassemblies from OpenCAD Studio's own packages: a lane from the baseline, a shoulder hanging from the lane's outer edge, and a side slope hanging from the shoulder's, with the pavement under lane and shoulder.
- Slopes are rise over run: -0.02 falls 2% away from the baseline, -0.5 falls 1 in 2.
- The packages are written to `OpenCAD Studio/Subassemblies` (`OCSLane.pkt`, `OCSShoulder.pkt`, `OCSSideSlope.pkt`) in the user's Documents folder; the drawing names each by that place (relative to the user's profile when it lies under it), so another computer finds them when the files are copied to the same folder.
- Use the assembly with CREATECORRIDOR; the corridor is built when it is rebuilt.
