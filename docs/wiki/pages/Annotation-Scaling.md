# Annotation Scaling

Drawings in model space are drawn at full size, but they are printed at a scale: 1:50, 1:100, 1/4" = 1'-0". Text, dimensions and hatches must be a fixed size **on paper**, so a 2.5 mm note needs to be 125 units tall at 1:50 and 250 units tall at 1:100.

**Annotative** objects do this for you. You give their paper size once; they show at the right size for the **annotation scale** of each view, and one object can carry several scales at the same time.

## What can be annotative

Text and multiline text, dimensions, multileaders and leaders, hatches, blocks and attribute definitions, and geometric tolerances. Make an object annotative through its style (an annotative text, dimension or multileader style), with the **Annotative** option when you create it, or in **Properties**.

## The annotation scale

<img src="images/icons/add_scale.svg" width="24" alt=""> The current annotation scale is shown in the status bar. In model space, choose it there (`CANNOSCALE`). In a layout viewport, the annotation scale follows the viewport scale.

New annotative objects get the current annotation scale.

## Status bar switches

| Switch | Variable | Effect |
|---|---|---|
| **Show Annotation Objects** | `ANNOALLVISIBLE` | Off: only annotative objects that support the current scale are shown. On: all are shown. |
| **Automatically Add Scales** | `ANNOAUTOSCALE` | On: when you change the annotation scale, every annotative object gets the new scale as well. |
| **Viewport / Annotation Scale Sync** | | Keeps the annotation scale of a viewport equal to its scale. |

## Adding and removing scales

<img src="images/icons/scale_list.svg" width="24" alt=""> `OBJECTSCALE` adds scales to selected annotative objects or removes them. **Properties** shows the scales an object supports.

`ANNOUPDATE` updates annotative objects after their style has changed. <img src="images/icons/sync.svg" width="24" alt=""> `ANNORESET` puts the separate positions of each scale representation back where the current scale's representation is.

Each scale representation can be moved on its own: an annotative text can sit at a different place at 1:50 than at 1:100.

## The scale list

`SCALELISTEDIT` adds scales to the list offered for viewports, plotting and annotation, deletes the ones you never use, and lists them. The status bar's scale menu opens the same list in a window.
