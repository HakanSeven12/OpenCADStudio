# PDF thin-line regression checklist (#1800)

Check that toggling LWT changes vector PDF stroke widths immediately on the active underlay, preserves text, fills and images, and does not affect print output.

Code paths: `src/app/update/mod.rs`, `src/scene/model/image_model.rs`, `src/scene/model/pdf_raster.rs`.
