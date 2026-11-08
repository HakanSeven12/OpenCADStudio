# Ribbon Reference

Every ribbon tab and panel, in the order they appear, with the commands on it. Click a command for its page. A button with an arrow opens a list of related tools; the last one used becomes the button.

Tabs that appear only in context: **Block Editor** (while editing a block) and **Table Cell** (while table cells are selected). Plugins can add tabs of their own, and `CUI` adds a **My Tools** tab with your own panels.

## Draw

![Draw tab](images/screens/ribbon-draw.png)

| Panel | Commands |
|---|---|
| Draw | [`LINE`](LINE-Command) · [`PLINE`](PLINE-Command) · [`CIRCLE`](CIRCLE-Command) list · [`ARC`](ARC-Command) list · [`RECTANG`](RECTANG-Command) / [`POLYGON`](POLYGON-Command) list · [`ELLIPSE`](ELLIPSE-Command) list · [`HATCH`](HATCH-Command) list; the slide-out adds points, splines, construction lines, rays, regions, wipeouts, revision clouds, donuts and more |
| Modify | [`MOVE`](MOVE-Command) · [`COPY`](COPY-Command) · [`STRETCH`](STRETCH-Command) · [`ROTATE`](ROTATE-Command) · [`MIRROR`](MIRROR-Command) · [`SCALE`](SCALE-Command) · [`TRIM`](TRIM-Command) / [`EXTEND`](EXTEND-Command) · [`FILLET`](FILLET-Command) / [`CHAMFER`](CHAMFER-Command) · [`ARRAY`](ARRAY-Command) list · [`ERASE`](ERASE-Command) · [`EXPLODE`](EXPLODE-Command) · [`OFFSET`](OFFSET-Command); the slide-out adds break, join, lengthen, align, draw order and the polyline and spline editors |
| Annotation | [`MTEXT`](MTEXT-Command) / [`TEXT`](TEXT-Command) · [`DIMLINEAR`](DIMLINEAR-Command) and the other dimensions · [`MLEADER`](MLEADER-Command) / [`LEADER`](LEADER-Command) |
| Layers | [`LAYERS`](LAYERS-Command) · the layer list · [`LAYOFF`](LAYOFF-Command) · [`LAYFRZ`](LAYFRZ-Command) · [`LAYLCK`](LAYLCK-Command) · [`LAYMCUR`](LAYMCUR-Command) · [`LAYISO`](LAYISO-Command) · [`LAYWALK`](LAYWALK-Command) · [`LAYON`](LAYON-Command) · [`LAYTHW`](LAYTHW-Command) · [`LAYULK`](LAYULK-Command) · [`LAYMCH`](LAYMCH-Command) · [`LAYUNISO`](LAYUNISO-Command) |
| Block | [`BLOCK`](BLOCK-Command) · [`BEDIT`](BEDIT-Command) · [`ATTEDIT`](ATTEDIT-Command) |
| Properties | [`MATCHPROP`](MATCHPROP-Command) · colour, linetype, lineweight and transparency for new objects |
| Groups | [`GROUP`](GROUP-Command) · [`UNGROUP`](UNGROUP-Command) |
| Clipboard | [`PASTECLIP`](PASTECLIP-Command) list · [`COPYCLIP`](COPYCLIP-Command) · [`CUTCLIP`](CUTCLIP-Command) |
| Measurement | [`DIST`](DIST-Command) · [`MEASURERADIUS`](MEASURERADIUS-Command) · [`MEASUREANGLE`](MEASUREANGLE-Command) · [`AREA`](AREA-Command) |

## Parametric

![Parametric tab](images/screens/ribbon-parametric.png)

| Panel | Commands |
|---|---|
| Geometric | [`GCCOINCIDENT`](GCCOINCIDENT-Command) · [`GCPARALLEL`](GCPARALLEL-Command) · [`GCTANGENT`](GCTANGENT-Command) · [`GCCOLLINEAR`](GCCOLLINEAR-Command) · [`GCPERPENDICULAR`](GCPERPENDICULAR-Command) · [`GCCONCENTRIC`](GCCONCENTRIC-Command) · [`GCHORIZONTAL`](GCHORIZONTAL-Command) · [`GCSYMMETRIC`](GCSYMMETRIC-Command) · [`GCFIX`](GCFIX-Command) · [`GCVERTICAL`](GCVERTICAL-Command) · [`GCEQUAL`](GCEQUAL-Command) · show and hide the constraint bars · [`AUTOCONSTRAIN`](AUTOCONSTRAIN-Command) |
| Dimensional | [`DCLINEAR`](DCLINEAR-Command) list ([`DCHORIZONTAL`](DCHORIZONTAL-Command), [`DCVERTICAL`](DCVERTICAL-Command), [`DCALIGNED`](DCALIGNED-Command), [`DCANGULAR`](DCANGULAR-Command), [`DCRADIUS`](DCRADIUS-Command), [`DCDIAMETER`](DCDIAMETER-Command)) · [`DCCONVERT`](DCCONVERT-Command) · show and hide dimensional constraints |
| Manage | [`DELCONSTRAINT`](DELCONSTRAINT-Command) · [`PARAMETERS`](PARAMETERS-Command) · [`CONSTRAINTSETTINGS`](CONSTRAINTSETTINGS-Command) |

## Model

![Model tab](images/screens/ribbon-model.png)

| Panel | Commands |
|---|---|
| Create | [`BOX`](BOX-Command) list ([`CYLINDER`](CYLINDER-Command), [`CONE`](CONE-Command), [`SPHERE`](SPHERE-Command), [`PYRAMID`](PYRAMID-Command), [`WEDGE`](WEDGE-Command), [`TORUS`](TORUS-Command), [`POLYSOLID`](POLYSOLID-Command)) · [`EXTRUDE`](EXTRUDE-Command) · [`REVOLVE`](REVOLVE-Command) · [`LOFT`](LOFT-Command) · [`SWEEP`](SWEEP-Command) · [`PRESSPULL`](PRESSPULL-Command) |
| Boolean | [`UNION`](UNION-Command) · [`SUBTRACT`](SUBTRACT-Command) · [`INTERSECT`](INTERSECT-Command) |
| Edges | [`FILLETEDGE`](FILLETEDGE-Command) · [`CHAMFEREDGE`](CHAMFEREDGE-Command) · [`SHELL`](SHELL-Command) |

## Insert

![Insert tab](images/screens/ribbon-insert.png)

| Panel | Commands |
|---|---|
| Block | [`INSERT`](INSERT-Command) · [`ATTEDIT`](ATTEDIT-Command) list · attribute display list ([`ATTDISP`](ATTDISP-System-Variable)) |
| Block Definition | [`BLOCK`](BLOCK-Command) · [`WBLOCK`](WBLOCK-Command) · [`ATTDEF`](ATTDEF-Command) · [`ATTMAN`](ATTMAN-Command) · [`BEDIT`](BEDIT-Command) |
| Reference | [`XATTACH`](XATTACH-Command) · [`ATTACH`](ATTACH-Command) list · [`PDFATTACH`](PDFATTACH-Command) · [`XCLIP`](XCLIP-Command) · [`ADJUST`](ADJUST-Command) · underlay layers ([`UNDERLAY`](UNDERLAY-Command)) · frames list ([`FRAME`](FRAME-System-Variable)) · underlay snapping list |
| Import | [`IMPORT`](IMPORT-Command) · [`PDFSHXTEXT`](PDFSHXTEXT-Command) · [`TXT2MTXT`](TXT2MTXT-Command) |
| Data | [`FIELD`](FIELD-Command) · [`UPDATEFIELD`](UPDATEFIELD-Command) · [`INSERTOBJ`](INSERTOBJ-Command) · [`HYPERLINK`](HYPERLINK-Command) |
| Linking & Extraction | [`DATALINK`](DATALINK-Command) · [`DATAEXTRACTION`](DATAEXTRACTION-Command) |
| Content | [`CONTENTBROWSER`](CONTENTBROWSER-Command) · [`ADCENTER`](ADCENTER-Command) (both list the drawing's blocks and layers at the command line for now) |

## Annotate

![Annotate tab](images/screens/ribbon-annotate.png)

| Panel | Commands |
|---|---|
| Text | [`MTEXT`](MTEXT-Command) / [`TEXT`](TEXT-Command) · [`SPELL`](SPELL-Command) · [`TEXTALIGN`](TEXTALIGN-Command) · [`JUSTIFYTEXT`](JUSTIFYTEXT-Command) · text style list |
| Dimensions | [`DIM`](DIM-Command) · dimension style list · [`DIMBREAK`](DIMBREAK-Command) · [`DIMSPACE`](DIMSPACE-Command) · [`DIMJOGLINE`](DIMJOGLINE-Command) · [`DIMINSPECT`](DIMINSPECT-Command) · apply the current style · [`DIMREASSOCIATE`](DIMREASSOCIATE-Command) · the dimension types list · [`QDIM`](QDIM-Command) · [`DIMCONTINUE`](DIMCONTINUE-Command) / [`DIMBASELINE`](DIMBASELINE-Command) |
| Centerlines | [`CENTERMARK`](CENTERMARK-Command) · [`CENTERLINE`](CENTERLINE-Command) |
| Leaders | [`MLEADER`](MLEADER-Command) · multileader style list · add, remove, align and collect leaders |
| Tables | [`TABLE`](TABLE-Command) · table style list · [`DATAEXTRACTION`](DATAEXTRACTION-Command) · [`DATALINK`](DATALINK-Command) |
| Markup | [`WIPEOUT`](WIPEOUT-Command) · [`REVCLOUD`](REVCLOUD-Command) list |
| Annotation Scaling | the current annotation scale · [`SCALELISTEDIT`](SCALELISTEDIT-Command) · [`OBJECTSCALE`](OBJECTSCALE-Command) · [`ANNORESET`](ANNORESET-Command) |

## View

![View tab](images/screens/ribbon-view.png)

| Panel | Commands |
|---|---|
| Viewport Tools | [`UCSICON`](UCSICON-Command) · [`NAVVCUBE`](NAVVCUBE-Command) |
| Navigate | [`ZOOM`](ZOOM-Command) (Extents, Window, In, Out) · [`PAN`](PAN-Command) · [`3DORBIT`](3DORBIT-Command) |
| Model Viewports | [`VPORTS`](VPORTS-Command) layouts and named configurations · [`VPJOIN`](VPJOIN-Command) · restore · highlight the active viewport |
| Preset | Top · Front · Right · Isometric ([`VIEW`](VIEW-Command)) |
| Palettes | [`TOOLPALETTES`](TOOLPALETTES-Command) · [`PROPERTIES`](PROPERTIES-Command) · [`COUNTLIST`](COUNTLIST-Command) · [`GRAPHICATTRIBUTES`](GRAPHICATTRIBUTES-Command) · [`SHEETSET`](SHEETSET-Command) |
| Interface | [`FILETAB`](FILETAB-Command) · layout tabs · [`HORIZONTAL`](HORIZONTAL-Command) · [`VERTICAL`](VERTICAL-Command) · [`CASCADE`](CASCADE-Command) |
| Plot | [`PAGESETUP`](PAGESETUP-Command) |

## Manage

![Manage tab](images/screens/ribbon-manage.png)

| Panel | Commands |
|---|---|
| Customization | [`CUI`](CUI-Command) · [`TOOLPALETTES`](TOOLPALETTES-Command) · [`CUIIMPORT`](CUIIMPORT-Command) · [`CUIEXPORT`](CUIEXPORT-Command) · [`ALIASEDIT`](ALIASEDIT-Command) |
| Cleanup | [`FINDNONPURGEABLE`](FINDNONPURGEABLE-Command) · [`PURGE`](PURGE-Command) · [`OVERKILL`](OVERKILL-Command) · [`AUDIT`](AUDIT-Command) |
| Application | [`OPTIONS`](OPTIONS-Command) · [`ABOUT`](ABOUT-Command) |

## Layout

Shown while a sheet layout is active.

| Panel | Commands |
|---|---|
| Viewport | [`MVIEW`](MVIEW-Command) |
| Plot | [`PAGESETUP`](PAGESETUP-Command) · [`PRINTALL`](PRINTALL-Command) |
