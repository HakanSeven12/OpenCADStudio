---
summary: Creates an assembly of a road's two half sections.
related: CREATECORRIDOR, CREATEALIGNMENT, CREATEPROFILE
---

## Usage
1. Specify the location of the assembly's baseline.
2. Press Enter, or first set options: **Lane width**, **Lane slope**, **Shoulder width**, **Shoulder slope**, **Pavement depth**, **Side slope width**, **Side slope**.

## Notes
- Each side is one subassembly from OpenCAD Studio's road package: a lane from the baseline, a shoulder, a side slope out from the shoulder, and the pavement under lane and shoulder.
- Slopes are rise over run: -0.02 falls 2% away from the baseline, -0.5 falls 1 in 2.
- The package is written to `OpenCAD Studio/Subassemblies/OCSRoad.pkt` in the user's Documents folder; the drawing names it by that place (relative to the user's profile when it lies under it), so another computer finds it when the file is copied to the same folder.
- Use the assembly with CREATECORRIDOR; the corridor is built when it is rebuilt.
