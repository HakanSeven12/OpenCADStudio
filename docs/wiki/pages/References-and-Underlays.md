# References and Underlays

A reference shows another file inside your drawing without copying its content. The other file stays separate: when it changes, the reference shows the change the next time it is loaded. Use references for a site plan under a building plan, a structural grid shared by several disciplines, a scanned survey or a PDF from a consultant.

<img src="images/icons/xclip.svg" width="24" alt=""> The **External References** palette (`EXTERNALREFERENCES`, `ER`) lists every reference in the drawing with its status, type, size and path.

## Attaching

`ATTACH` attaches any kind of reference and picks the right one from the file type. The dedicated commands are:

| Reference | Command | Files |
|---|---|---|
| Drawing | `XATTACH` (`XA`) | DWG |
| Image | `IMAGEATTACH` (`IAT`) | Raster images; georeferenced files (with geo tags or a world file) can be placed at their own location. |
| PDF | `PDFATTACH` | One page of a PDF file. |
| DWF | `DWFATTACH` | DWF and DWFx. |
| DGN | `DGNATTACH` | DGN. |
| Point cloud | `POINTCLOUDATTACH` | Point cloud projects and scans (`.rcp`, `.rcs`). |

When attaching you set the insertion point, scale and rotation (or choose to give them on screen) and the **path type**: full, relative to the drawing, or no path.

### Attachment and overlay

A drawing reference can be an **attachment** or an **overlay**. Both show the referenced drawing. The difference shows only when your drawing is itself referenced by another: attachments inside it come along, overlays do not. Use overlays for background drawings that only you need.

## Managing references

In the External References palette, right-click a reference to:

| Action | Effect |
|---|---|
| **Reload** | Reads the file again to show its latest state. |
| **Unload** | Hides the reference but keeps it in the list, so it can be loaded again. |
| **Detach** | Removes the reference from the drawing. |
| **Bind** | Makes a drawing reference a permanent part of the drawing, as a block. |
| **Open** | Opens the referenced drawing (`XOPEN`). |

Missing files are flagged; choose a new path to find them again. `XRELOAD` reloads from the command line.

## Clipping and adjusting

| | Command | What it does |
|---|---|---|
| <img src="images/icons/xclip.svg" width="24" alt=""> | `CLIP`, `XCLIP` (`XC`) | Shows only part of a reference, inside a rectangle, a polygon or a polyline. Clips can be inverted to hide the inside instead. |
| <img src="images/icons/xclip_remove.svg" width="24" alt=""> | `XCLIP` › Delete | Removes the clip. |
| <img src="images/icons/xadjust.svg" width="24" alt=""> | `ADJUST`, `IMAGEADJUST`, `PDFADJUST` | Sets fade, contrast and monochrome display of images and underlays. |
| <img src="images/icons/underlay_frames.svg" width="24" alt=""> | `FRAME`, `IMAGEFRAME`, `PDFFRAME` | Shows or hides the frames around images and underlays, and whether they plot. |
| <img src="images/icons/underlay_layers.svg" width="24" alt=""> | `UNDERLAY` › Layers | Turns the layers inside a PDF or DGN underlay on and off. |
| <img src="images/icons/snap_underlays.svg" width="24" alt=""> | `PDFOSNAP`, `DWFOSNAP`, `DGNOSNAP` | Lets object snaps work on the geometry of underlays. |

## Editing a reference in place

`REFEDIT` opens a drawing reference (or a block) for editing inside your drawing; the rest fades. Add or remove objects with <img src="images/icons/refset_add.svg" width="20" alt=""> and <img src="images/icons/refset_remove.svg" width="20" alt=""> `REFSET`, then `REFCLOSE` to save the changes back to the referenced file, or discard them.

## Converting PDF to drawing objects

`PDFIMPORT` turns a PDF page, or an attached PDF underlay, into lines, arcs, polylines, hatches, images and text. Text drawn with stroke (SHX) fonts arrives as geometry; `PDFSHXTEXT` recognises it and turns it back into text.

## Point clouds

The **Point Cloud Manager** (`POINTCLOUDMANAGER`) lists the scans and regions of attached point clouds and turns them on or off. Other tools:

| Command | What it does |
|---|---|
| `POINTCLOUDCROP` / `POINTCLOUDUNCROP` | Shows only the points inside a boundary, or all again. |
| `POINTCLOUDCOLORMAP` | Colours the points by their own colour, intensity, elevation or classification. |
| `POINTCLOUDSTYLIZE` | Sets the colour mode: RGB, object colour, intensity, elevation, normal or classification. |
| `PCEXTRACTSECTION` | Draws the section of the cloud on a plane as lines. |
| `PCEXTRACTEDGE` / `PCEXTRACTCORNER` | Picks an edge line or a corner point from the cloud. |
| `PCEXTRACTCENTERLINE` | Draws the centre line of a pipe-like part of the cloud. |



## Other files inside the drawing

- `INSERTOBJ` embeds or links an OLE object; `OLELINKS` manages the links.
- `HYPERLINK` (**Ctrl+K**) attaches a link to a web page or file to objects.
- `ETRANSMIT` packs the drawing and its resolvable external resources into one ZIP file, with Preserve, Structured-by-type, or Flat layout and rewritten references. The included report lists missing resources; same-name files receive unique names instead of being overwritten.
