---
summary: Sets the root drawing coordinate reference system stored in GEODATA.
related: UNITS, DWGUNITS, PDFATTACH
---

## Usage
1. Run `GEOGRAPHICLOCATION`, or choose **Geographic Location…** from the Edit menu.
2. Enter an **EPSG code** (for example `EPSG:5514`) or a **WKT** coordinate-system definition.
3. Click **OK**. An invalid or unsupported definition leaves the dialog open and does not change the drawing.

## Notes
- The command creates or updates the model-space `GEODATA` object and its native DWG/DXF ownership links. Reopening the dialog edits that object rather than adding another document CRS.
- Existing design/reference points, directions, unit scales and other GEODATA metadata are preserved. The change is one undo step and is saved in DWG and DXF.
- This sets metadata only. It does **not** move or reproject geometry, change `INSUNITS`, choose a CRS from coordinates, or load aerial maps.
- A PROJ string can be parsed for coordinate operations, but cannot be stored by this dialog unless it has EPSG or WKT metadata. MapGuide XML definitions are not supported for editing yet.
- EPSG:5514 means **S-JTSK / Krovak East North**, in metres. Drawing coordinates already in that system remain unchanged; negative easting/northing values are valid. Geographic coordinates produced from it are S-JTSK latitude/longitude, not implicitly WGS84.
- The root drawing supplies the scene CRS. GEODATA belonging to an xref is not selected independently.
