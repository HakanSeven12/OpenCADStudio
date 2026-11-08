---
summary: Imports the survey points of a LandXML file as point objects.
related: IMPORT, ZOOM
ribbon: Insert tab › Import panel
web: Not available in the web app.
---
## Usage
1. At `LANDXMLIMPORT  path to the .xml file:`, type the full path of the LandXML file.
2. Each `CgPoint` survey point in the file is added as a point object. The command line reports how many points were imported.
3. Use [`ZOOM`](ZOOM-Command) Extents to see them.

## Notes
- You can type the path after the command, for example `LANDXMLIMPORT C:\survey\site.xml`.
- The point coordinates are read in the order northing, easting, elevation and placed at X = easting, Y = northing, Z = elevation.
- Only survey points are read. Surfaces, alignments and other LandXML data are ignored.
- A file without survey points adds nothing.
- The import can be undone.
