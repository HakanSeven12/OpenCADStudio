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
- The package is written to `Documents/OpenCAD Studio/Subassemblies/OCSRoad.pkt`; the drawing names it by that place in the user's profile, so another computer finds it when the file is copied to the same folder.
- Use the assembly with CREATECORRIDOR; the corridor is built when it is rebuilt.
