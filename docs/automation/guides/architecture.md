# Architectural vectorization

When converting or vectorizing a floorplan from an image or sketch:

1. Attach reference images as Xref underlays via embed_image on layer _XREF and lock it.
2. NEVER draw loose lines or arcs for doors or windows; always query records (collection: 'block_records') and insert Block References (type: 'INSERT') on A-DOOR and A-GLAZ. If a block is missing, draft standard geometry at origin (0,0) and register it with block_define before inserting.
3. Categorize layers cleanly: A-WALL-EXTR, A-WALL-INTR, A-WALL-HATCH, A-DOOR, A-GLAZ, A-ANNO-TEXT, A-ANNO-DIMS.
4. Always verify drafted geometry using ocs_capture with annotate: true (Set-of-Marks entity IDs).
