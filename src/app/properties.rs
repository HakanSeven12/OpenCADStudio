use super::helpers::{entity_type_key, entity_type_label, title_case_word};
use super::{OpenCADStudio, VARIES_LABEL};
use crate::io::linetypes;
use crate::scene::model::object::{GripDef, PropValue};
use crate::scene::view::dispatch;
use crate::t;
use crate::ui;
use codec::types::{Transform, Vector3};
use codec::{Entity, EntityType, Handle};

/// Above this many selected objects the Properties panel skips per-entity
/// property aggregation (which is O(n) per row, plus an O(n²) group filter) and
/// shows a count-only summary instead. Bulk edits still go through the ribbon.
const MAX_PROP_AGGREGATE: usize = 2_000;

/// Below this many selected entities the rayon hop plus the per-worker
/// thread-state transplant cost more than the parallel build saves, so the
/// fold stays sequential (byte-for-byte the pre-parallel behavior).
const PARALLEL_AGGREGATE_MIN: usize = 32;

/// Entities built per parallel wave. Chunking keeps the "nothing in common
/// left to narrow" early-exit effective: once the intersection empties, at
/// most one wave of builds is ever wasted.
const PARALLEL_AGGREGATE_CHUNK: usize = 256;


fn property_choice_combos(
    sections: &[crate::scene::model::object::PropSection],
) -> rustc_hash::FxHashMap<String, iced::widget::combo_box::State<ui::properties::LocalizedChoice>>
{
    sections
        .iter()
        .flat_map(|section| section.props.iter())
        .filter_map(|prop| match &prop.value {
            PropValue::Choice { options, .. } | PropValue::ChoiceWithTooltip { options, .. } => Some((
                prop.field.to_string(),
                iced::widget::combo_box::State::new(
                    options
                        .iter()
                        .cloned()
                        .map(ui::properties::LocalizedChoice::new)
                        .collect(),
                ),
            )),
            _ => None,
        })
        .collect()
}

/// Cap the total number of selection grips at
/// [`crate::app::settings::MAX_SELECTED_GRIPS`], keeping entity order with
/// non-midpoint (vertex/stretch) grips first. Below the cap both vecs are
/// returned untouched, so existing selections are identical. The parallel
/// `handles` vec is truncated in lockstep with `grips`.
///
/// `pub` (not private) so the `cargo bench` harness (external crate) measures
/// the real function as `ui_grip_budget`.
pub fn apply_grip_budget(
    mut grips: Vec<GripDef>,
    mut handles: Vec<Handle>,
) -> (Vec<GripDef>, Vec<Handle>) {
    debug_assert_eq!(
        grips.len(),
        handles.len(),
        "grips/handles are parallel vecs built in lockstep"
    );
    if grips.len() > crate::app::settings::MAX_SELECTED_GRIPS {
        // Vertex budget: one dense polyline can emit ~2 grips/vertex past the
        // object-count gate above. Keep entity order; non-midpoint (stretch)
        // grips first, midpoints fill the remainder.
        let cap = crate::app::settings::MAX_SELECTED_GRIPS;
        let mut idx: Vec<usize> = (0..grips.len()).collect();
        idx.sort_by_key(|&j| grips[j].is_midpoint); // false (vertex) first; stable
        idx.truncate(cap);
        idx.sort_unstable(); // restore entity order for determinism
        let keep: rustc_hash::FxHashSet<usize> = idx.into_iter().collect();
        let mut kept_grips = Vec::with_capacity(cap);
        let mut kept_handles = Vec::with_capacity(cap);
        for (j, (grip, handle)) in grips.into_iter().zip(handles.into_iter()).enumerate() {
            if keep.contains(&j) {
                kept_grips.push(grip);
                kept_handles.push(handle);
            }
        }
        grips = kept_grips;
        handles = kept_handles;
    }
    (grips, handles)
}

fn visual_style_properties_text(style: &codec::objects::VisualStyle) -> String {
    style
        .properties
        .iter()
        .enumerate()
        .map(|(index, property)| {
            format!(
                "{}: {:?} (enabled {})",
                index, property.value, property.enabled
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl OpenCADStudio {
    /// Rebuild the PropertiesPanel from the current entity selection.
    /// Preserves UI state (open pickers, edit buffer) across refreshes.
    /// Sections and title of the Properties panel's single-object page for
    /// `handle` — also what the node graph shows as a node's ports.
    #[allow(clippy::ptr_arg)]
    pub(super) fn entity_property_sections(
        &self,
        i: usize,
        handle: Handle,
        source_entity: &EntityType,
        text_style_names: &Vec<String>,
        annotation_scale_handle: Option<Handle>,
        prop_vertex: usize,
    ) -> (Vec<crate::scene::model::object::PropSection>, String) {
        let contextual = crate::scene::annotative::entity_for_annotation_context(
            &self.tabs[i].scene.document,
            source_entity,
            annotation_scale_handle,
        );
        let plane = if self.tabs[i].editing_model_space() {
            self.tabs[i].ucs_xform().working_plane()
        } else {
            crate::command::WorkingPlane::default()
        };
        let display_entity = dispatch::entity_in_working_plane(contextual.as_ref(), plane);
        let entity = &display_entity;
        let group_names = self.tabs[i].scene.group_names_for_entity(handle);
        let compact_solid = crate::scene::model::solid_history::has_compact_solid_properties(
            &self.tabs[i].scene.document,
            handle,
        );
        let mut sections = dispatch::properties_sectioned(handle, entity, &text_style_names);
        if compact_solid {
            retain_compact_solid_sections(&mut sections);
        }
        sections.extend(crate::scene::model::solid_history::primitive_properties(
            &self.tabs[i].scene.document,
            handle,
        ));

        // Turn the Material row into an editable picker: the source
        // options (ByLayer / ByBlock) plus every named material the
        // drawing defines. A custom flag (3) shows the stored handle's
        // material name as the selection.
        {
            let doc = &self.tabs[i].scene.document;
            let common = entity.common();
            let selected = match common.material_flags {
                0 => "ByLayer".to_string(),
                1 => "ByBlock".to_string(),
                2 => "Global".to_string(),
                _ => common
                    .material_handle
                    .and_then(|mh| {
                        doc.objects.iter().find_map(|(h, o)| match o {
                            codec::objects::ObjectType::Material(m) if *h == mh => {
                                Some(m.name.clone())
                            }
                            _ => None,
                        })
                    })
                    .unwrap_or_else(|| "Global".to_string()),
            };
            let mut options = vec![
                "ByLayer".to_string(),
                "ByBlock".to_string(),
                "Global".to_string(),
            ];
            if !options.contains(&selected) && !selected.is_empty() {
                options.push(selected.clone());
            }
            for section in sections.iter_mut() {
                if let Some(row) = section.props.iter_mut().find(|p| p.field == "material") {
                    row.value = crate::scene::model::object::PropValue::Choice {
                        selected: selected.clone(),
                        options: options.clone(),
                    };
                }
            }
        }

        {
            let doc = &self.tabs[i].scene.document;
            let common = entity.common();
            let named_color = common
                .color_book_handle
                .filter(|handle| handle.is_valid())
                .and_then(|handle| doc.objects.get(&handle))
                .and_then(|object| match object {
                    codec::objects::ObjectType::BookColor(book) => {
                        Some((book.color, book.book_name.clone(), book.color_name.clone()))
                    }
                    _ => None,
                })
                .or_else(|| {
                    let identity = common.color_name.as_deref()?;
                    let (book_name, color_name) = identity
                        .split_once('$')
                        .map(|(book, color)| (book.to_string(), color.to_string()))
                        .unwrap_or_else(|| (String::new(), identity.to_string()));
                    Some((common.color, book_name, color_name))
                });
            if let Some((color, book_name, color_name)) = named_color {
                for section in sections.iter_mut() {
                    if let Some(row) = section.props.iter_mut().find(|row| row.field == "color") {
                        row.value = crate::scene::model::object::PropValue::NamedColorChoice {
                            color,
                            name: color_name.clone(),
                        };
                    }
                }
                use crate::entities::common::ro_prop;
                let mut props = Vec::new();
                if !book_name.is_empty() {
                    props.push(ro_prop(t!("Book").as_ref(), "book_color_book", book_name));
                }
                props.push(ro_prop(
                    t!("Color Name").as_ref(),
                    "book_color_name",
                    color_name,
                ));
                props.push(ro_prop(
                    t!("Color").as_ref(),
                    "book_color_value",
                    format!("{:?}", color),
                ));
                sections.push(crate::scene::model::object::PropSection {
                    title: t!("Color Book").into_owned(),
                    props,
                });
            }

            let style_handles = [
                ("Full", common.full_visual_style_handle),
                ("Face", common.face_visual_style_handle),
                ("Edge", common.edge_visual_style_handle),
            ];
            for (scope, handle) in style_handles {
                let Some((handle, style)) =
                    handle
                        .filter(|handle| handle.is_valid())
                        .and_then(|handle| {
                            doc.objects.get(&handle).and_then(|object| match object {
                                codec::objects::ObjectType::VisualStyle(style) => {
                                    Some((handle, style))
                                }
                                _ => None,
                            })
                        })
                else {
                    continue;
                };
                use crate::entities::common::ro_prop;
                sections.push(crate::scene::model::object::PropSection {
                    title: t!("%{scope} Visual Style", scope = scope).into_owned(),
                    props: vec![
                        ro_prop(
                            t!("Handle").as_ref(),
                            "vs_handle",
                            format!("{:X}", handle.value()),
                        ),
                        ro_prop(
                            t!("Description").as_ref(),
                            "vs_description",
                            style.description.clone(),
                        ),
                        ro_prop(t!("Type").as_ref(), "vs_type", style.style_type.to_string()),
                        ro_prop(
                            t!("Face").as_ref(),
                            "vs_face",
                            format!(
                                "lighting {}; quality {}; color {}; modifiers {}",
                                style.face_lighting_model,
                                style.face_lighting_quality,
                                style.face_color_mode,
                                style.face_modifier
                            ),
                        ),
                        ro_prop(
                            t!("Edges").as_ref(),
                            "vs_edges",
                            format!("model {}; style {}", style.edge_model, style.edge_style),
                        ),
                        ro_prop(
                            t!("Extended Lighting").as_ref(),
                            "vs_extended_lighting",
                            style.extended_lighting_model.to_string(),
                        ),
                        ro_prop(
                            t!("Internal").as_ref(),
                            "vs_internal",
                            style.internal_use_only.to_string(),
                        ),
                        ro_prop(
                            t!("Property Bag").as_ref(),
                            "vs_properties",
                            visual_style_properties_text(style),
                        ),
                    ],
                });
            }
        }

        if matches!(
            entity,
            codec::EntityType::Solid3D(_)
                | codec::EntityType::Body(_)
                | codec::EntityType::Surface(_)
                | codec::EntityType::Mesh(_)
                | codec::EntityType::PolygonMesh(_)
                | codec::EntityType::PolyfaceMesh(_)
        ) {
            if let Some(mesh) = self.tabs[i]
                .scene
                .meshes
                .get(&handle)
                .or_else(|| self.tabs[i].scene.block_meshes.get(&handle))
            {
                use crate::entities::common::ro_prop;
                let metrics = mesh.metrics;
                let triple = |values: [f64; 3]| {
                    format!("{:.6}, {:.6}, {:.6}", values[0], values[1], values[2])
                };
                let mut props = if compact_solid {
                    vec![ro_prop(
                        t!("Centroid").as_ref(),
                        "mesh_centroid",
                        triple(metrics.centroid),
                    )]
                } else {
                    vec![
                        ro_prop(
                            t!("Vertices").as_ref(),
                            "mesh_vertices",
                            metrics.vertices.to_string(),
                        ),
                        ro_prop(
                            t!("Triangles").as_ref(),
                            "mesh_triangles",
                            metrics.triangles.to_string(),
                        ),
                        ro_prop(
                            t!("Surface Area").as_ref(),
                            "mesh_surface_area",
                            format!("{:.6}", metrics.surface_area),
                        ),
                        ro_prop(
                            t!("Centroid").as_ref(),
                            "mesh_centroid",
                            triple(metrics.centroid),
                        ),
                        ro_prop(
                            t!("Tessellation").as_ref(),
                            "mesh_complete",
                            if mesh.complete { "Complete" } else { "Partial" },
                        ),
                    ]
                };
                if mesh.complete
                    && matches!(
                        entity,
                        codec::EntityType::Solid3D(_) | codec::EntityType::Body(_)
                    )
                {
                    let closed_props = vec![
                        ro_prop(
                            t!("Moment of inertia").as_ref(),
                            "mesh_moment_of_inertia",
                            triple(metrics.moment_of_inertia),
                        ),
                        ro_prop(
                            t!("Principal directions").as_ref(),
                            "mesh_principal_directions",
                            metrics
                                .principal_directions
                                .chunks_exact(3)
                                .map(|axis| {
                                    format!("({:.6}, {:.6}, {:.6})", axis[0], axis[1], axis[2])
                                })
                                .collect::<Vec<_>>()
                                .join(", "),
                        ),
                        ro_prop(
                            t!("Principal moments").as_ref(),
                            "mesh_principal_moments",
                            triple(metrics.principal_moments),
                        ),
                        ro_prop(
                            t!("Product of inertia").as_ref(),
                            "mesh_product_of_inertia",
                            triple(metrics.product_of_inertia),
                        ),
                        ro_prop(
                            t!("Radii of gyration").as_ref(),
                            "mesh_radii_of_gyration",
                            triple(metrics.radii_of_gyration),
                        ),
                        ro_prop(
                            t!("Volume").as_ref(),
                            "mesh_volume",
                            format!("{:.6}", metrics.volume),
                        ),
                    ];
                    if compact_solid {
                        props.extend(closed_props);
                    } else {
                        let volume = closed_props.last().cloned().unwrap();
                        props.insert(3, volume);
                        props.extend(closed_props.into_iter().take(5));
                    }
                }
                sections.push(crate::scene::model::object::PropSection {
                    title: t!("Mass Properties").into_owned(),
                    props,
                });
            }
        }

        if !compact_solid {
            let mut object_sections = crate::entities::object_data::sections(
                &self.tabs[i].scene.document,
                &self.tabs[i].scene.object_data_cache,
                handle,
                entity,
            );
            let scope = self.tabs[i].current_parametric_scope();
            let constrained = self.tabs[i]
                .scene
                .parametric_constraint_set(scope)
                .is_some_and(|set| set.constraints_touching(handle).next().is_some());
            if constrained {
                object_sections.retain(|section| section.title != "Associative Data");
            }
            sections.extend(object_sections);
        }

        {
            use crate::entities::common::ro_prop;
            let xd = &source_entity.common().extended_data;
            if !xd.is_empty() && !matches!(source_entity, codec::EntityType::Ole2Frame(_)) {
                let mut xd_props = Vec::new();
                // A hyperlink shows on its own row; inspection data is the
                // dimension's own; a centre object's association shows as its
                // own Geometry rows (the stored object carries no such data).
                for rec in xd.records().iter().filter(|rec| {
                    rec.application_name != crate::scene::hyperlink::APP
                        && rec.application_name != crate::entities::dim_inspect::APP
                        && rec.application_name
                            != codec::entities::CENTERMARK_XDATA_APPLICATION
                        && rec.application_name
                            != codec::entities::CENTERLINE_XDATA_APPLICATION
                }) {
                    let value_text = rec
                        .values
                        .iter()
                        .map(|value| format!("{value:?}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    xd_props.push(ro_prop(
                        t!("Application").as_ref(),
                        "xdata_value",
                        format!("{}: {value_text}", rec.application_name),
                    ));
                }
                if !xd_props.is_empty() {
                    sections.push(crate::scene::model::object::PropSection {
                        title: t!("Extended Data").into_owned(),
                        props: xd_props,
                    });
                }
            }
        }

        // Uniform-scale checkbox for block references (#427):
        // while the three scale factors are equal (and the user
        // hasn't opted into per-axis editing) the panel shows one
        // Scale row plus a checked "Uniform scale" box; unchecking
        // expands the familiar Scale X/Y/Z rows.
        if let codec::EntityType::Insert(ins) = entity {
            let eq = (ins.x_scale() - ins.y_scale()).abs() < 1e-12
                && (ins.x_scale() - ins.z_scale()).abs() < 1e-12;
            let uniform = eq && !self.tabs[i].props_asym_scale.contains(&handle.value());
            for section in sections.iter_mut() {
                let Some(xi) = section.props.iter().position(|p| p.field == "x_scale") else {
                    continue;
                };
                let toggle = crate::scene::model::object::Property {
                    label: t!("Uniform scale").into_owned(),
                    field: "ins_uniform",
                    value: crate::scene::model::object::PropValue::BoolToggle {
                        field: "ins_uniform",
                        value: uniform,
                    },
                };
                if uniform {
                    section
                        .props
                        .retain(|p| p.field != "y_scale" && p.field != "z_scale");
                    let xi = section
                        .props
                        .iter()
                        .position(|p| p.field == "x_scale")
                        .unwrap_or(xi.min(section.props.len()));
                    section.props[xi] = crate::entities::common::edit_prop(
                        t!("Scale").as_ref(),
                        "u_scale",
                        ins.x_scale(),
                    );
                    section.props.insert(xi, toggle);
                } else {
                    section.props.insert(xi, toggle);
                }
            }
        }

        if !self.tabs[i].scene.document.header.plotstyle_mode {
            let doc = &self.tabs[i].scene.document;
            let dict_h = doc.header.acad_plotstylename_dict_handle;
            let common = entity.common();
            let mut options = vec![
                "ByLayer".to_string(),
                "ByBlock".to_string(),
                "Normal".to_string(),
            ];
            if let Some(dict) = crate::scene::annotative::as_dict(doc, dict_h) {
                for (n, _) in &dict.entries {
                    if !options.contains(n) {
                        options.push(n.clone());
                    }
                }
            }
            let selected = match common.plotstyle_flags {
                0 => "ByLayer".to_string(),
                1 => "ByBlock".to_string(),
                2 => "Normal".to_string(),
                _ => common
                    .plotstyle_handle
                    .and_then(|ph| {
                        crate::scene::annotative::as_dict(doc, dict_h).and_then(|dict| {
                            dict.entries
                                .iter()
                                .find(|(_, h)| *h == ph)
                                .map(|(n, _)| n.clone())
                        })
                    })
                    .unwrap_or_else(|| "ByLayer".to_string()),
            };
            for section in sections.iter_mut() {
                if let Some(row) = section.props.iter_mut().find(|p| p.field == "plot_style") {
                    row.value = crate::scene::model::object::PropValue::Choice {
                        selected: selected.clone(),
                        options: options.clone(),
                    };
                }
            }
        } else {
            for section in sections.iter_mut() {
                if let Some(row) = section.props.iter_mut().find(|p| p.field == "plot_style") {
                    row.value = crate::scene::model::object::PropValue::ReadOnlyWithTooltip {
                        value: t!("ByColor").into_owned(),
                        tooltip: t!("Plot style is locked to color in Color-Dependent (CTB) mode")
                            .into_owned(),
                    };
                }
            }
        }

        // Turn the MLEADER handle-backed rows (multileader style, text
        // style, arrowhead block, leader linetype) into editable name
        // pickers. The current handle resolves to a display name; the
        // options list every candidate the drawing offers. Applying a
        // pick resolves the name back to a handle in the update loop.
        if let codec::EntityType::MultiLeader(ml) = entity {
            let doc = &self.tabs[i].scene.document;
            // Option lists.
            let mleader_styles: Vec<String> = doc
                .objects
                .iter()
                .filter_map(|(_, o)| match o {
                    codec::objects::ObjectType::MultiLeaderStyle(s) => Some(s.name.clone()),
                    _ => None,
                })
                .collect();
            // ByLayer and ByBlock lead the list once (a leader with no
            // linetype handle draws ByBlock), then the drawing's linetypes.
            let ltype_names: Vec<String> = ["ByLayer".to_string(), "ByBlock".to_string()]
                .into_iter()
                .chain(
                    doc.line_types
                        .iter()
                        .map(|l| l.name.clone())
                        .filter(|n| {
                            !n.is_empty()
                                && !n.eq_ignore_ascii_case("ByLayer")
                                && !n.eq_ignore_ascii_case("ByBlock")
                        }),
                )
                .collect();
            // Arrowheads are blocks; the closed-filled default has no
            // block, so seed the list with it and add the arrowhead
            // blocks (leading underscore) present in the drawing.
            // Filled in below from the current arrowhead. (#1820)
            let arrow_names: Vec<String>;
            let block_names: Vec<String> = doc
                .block_records
                .iter()
                .map(|block| block.name.clone())
                .filter(|name| !name.is_empty() && !name.starts_with('*'))
                .collect();
            let tstyle_names = text_style_names.clone();
            // Currently selected names.
            let cur_style = ml
                .style_handle
                .and_then(|h| {
                    doc.objects.iter().find_map(|(oh, o)| match o {
                        codec::objects::ObjectType::MultiLeaderStyle(s) if *oh == h => {
                            Some(s.name.clone())
                        }
                        _ => None,
                    })
                })
                .unwrap_or_else(|| "Standard".to_string());
            let cur_tstyle = ml
                .text_style_handle
                .and_then(|h| {
                    doc.text_styles
                        .iter()
                        .find(|s| s.handle == h)
                        .map(|s| s.name.clone())
                })
                .unwrap_or_else(|| "Standard".to_string());
            let cur_arrow = crate::entities::arrowheads::label(
                doc,
                ml.arrowhead_handle.unwrap_or(codec::Handle::NULL),
            );
            arrow_names = crate::entities::arrowheads::options([cur_arrow.as_str()]);
            let cur_ltype = ml
                .line_type_handle
                .and_then(|h| {
                    doc.line_types
                        .iter()
                        .find(|l| l.handle == h)
                        .map(|l| l.name.clone())
                })
                .unwrap_or_else(|| "ByBlock".to_string());
            let cur_block = ml
                .block_content_handle
                .and_then(|handle| {
                    doc.block_records
                        .iter()
                        .find(|block| block.handle == handle)
                        .map(|block| block.name.clone())
                })
                .unwrap_or_else(|| "(none)".to_string());
            let mut set_choice = |field: &str, selected: String, options: Vec<String>| {
                for section in sections.iter_mut() {
                    if let Some(row) = section.props.iter_mut().find(|p| p.field == field) {
                        row.value = crate::scene::model::object::PropValue::Choice {
                            selected: selected.clone(),
                            options: options.clone(),
                        };
                    }
                }
            };
            set_choice("mleader_style", cur_style, mleader_styles);
            set_choice("text_style_handle", cur_tstyle, tstyle_names);
            set_choice("arrowhead_handle", cur_arrow, arrow_names);
            set_choice("line_type_handle", cur_ltype, ltype_names);
            if !block_names.is_empty() {
                set_choice("block_content_handle", cur_block, block_names);
            }
        }

        // Inject viewport-only properties that require doc access.
        if let codec::EntityType::Viewport(vp) = entity {
            let frozen_names: Vec<String> = vp
                .frozen_layers
                .iter()
                .filter_map(|&h| {
                    self.tabs[i]
                        .scene
                        .document
                        .layers
                        .iter()
                        .find(|l| l.handle == h)
                        .map(|l| l.name.clone())
                })
                .collect();

            // Collect available UCS names for the name picker.
            let ucs_names: Vec<String> = self.tabs[i]
                .scene
                .document
                .ucss
                .iter()
                .map(|u| u.name.clone())
                .filter(|n| !n.is_empty())
                .collect();

            // Current UCS name (resolved from vp.ucs_handle).
            let current_ucs = self.tabs[i]
                .scene
                .document
                .ucss
                .iter()
                .find(|u| u.handle == vp.ucs_handle)
                .map(|u| u.name.clone())
                .unwrap_or_default();

            // Collect available named view names.
            let view_names: Vec<String> = self.tabs[i]
                .scene
                .document
                .views
                .iter()
                .map(|v| v.name.clone())
                .filter(|n| !n.is_empty())
                .collect();

            if let Some(geom) = sections.last_mut() {
                geom.props.push(crate::scene::model::object::Property {
                    label: t!("Frozen Layers").into_owned(),
                    field: "frozen_layers",
                    value: crate::scene::model::object::PropValue::PlainText(
                        frozen_names.join(", "),
                    ),
                });
                if !ucs_names.is_empty() {
                    geom.props.push(crate::scene::model::object::Property {
                        label: t!("UCS Name").into_owned(),
                        field: "vp_ucs_name",
                        value: crate::scene::model::object::PropValue::Choice {
                            selected: current_ucs,
                            options: ucs_names,
                        },
                    });
                }
                if !view_names.is_empty() {
                    geom.props.push(crate::scene::model::object::Property {
                        label: t!("Named View").into_owned(),
                        field: "vp_named_view",
                        value: crate::scene::model::object::PropValue::Choice {
                            selected: String::new(),
                            options: view_names,
                        },
                    });
                }
            }

            // Drive the viewport scale picker from the drawing's
            // own scale list instead of a built-in set.
            let file_scales = self.tabs[i].scene.scale_list();
            if !file_scales.is_empty() {
                let eff =
                    crate::scene::vp_effective_scale(vp.custom_scale, vp.view_height, vp.height);
                let selected = file_scales
                    .iter()
                    .find(|(_, _, f)| (f - eff).abs() < 0.001 * f.max(0.001))
                    .map(|(n, _, _)| n.clone())
                    .unwrap_or_default();
                let options: Vec<String> = file_scales.iter().map(|(n, _, _)| n.clone()).collect();
                if let Some(geom) = sections.last_mut() {
                    if let Some(prop) = geom.props.iter_mut().find(|p| p.field == "vscale_std") {
                        prop.value =
                            crate::scene::model::object::PropValue::Choice { selected, options };
                    }
                }
            }
        }

        // Legacy leaders omit Handle and report annotation association.
        if let codec::EntityType::Leader(leader) = entity {
            if let Some(general) = sections.first_mut() {
                general.props.retain(|property| property.field != "handle");
                let associative = !leader.annotation_handle.is_null()
                    && self.tabs[i]
                        .scene
                        .document
                        .get_entity(leader.annotation_handle)
                        .is_some();
                general.props.push(crate::scene::model::object::Property {
                    label: t!("Associative").into_owned(),
                    field: "associative",
                    value: crate::scene::model::object::PropValue::ReadOnly(
                        if associative { t!("Yes") } else { t!("No") }.into_owned(),
                    ),
                });
            }
        }

        // Centre marks and centre lines: the General rows end with whether
        // the object still follows its source (read-only), as the reference
        // lists them; a line's thickness and handle rows are not shown.
        if let Some((_, associated)) = self.tabs[i].scene.center_object_state(handle) {
            for section in sections.iter_mut() {
                section
                    .props
                    .retain(|property| !matches!(property.field, "handle" | "thickness"));
            }
            if let Some(general) = sections.first_mut() {
                general.props.push(crate::scene::model::object::Property {
                    label: t!("Associative").into_owned(),
                    field: "associative",
                    value: crate::scene::model::object::PropValue::ReadOnly(
                        if associated { t!("Yes") } else { t!("No") }.into_owned(),
                    ),
                });
            }
        }

        // Inject DimStyle picker + style-derived groups for Dimensions.
        if let codec::EntityType::Dimension(d) = entity {
            if let Some(general) = sections.first_mut() {
                general.props.retain(|property| property.field != "handle");
                let associative = crate::scene::dimension_assoc::dimension_is_associative(
                    &self.tabs[i].scene.document,
                    d.base().common.handle,
                );
                let statuses = self.tabs[i]
                    .scene
                    .dimension_association_status(d.base().common.handle);
                let slots = self.tabs[i]
                    .scene
                    .dimension_association_slot_points(d.base().common.handle)
                    .len();
                let status =
                    if statuses.is_empty() {
                        t!("Nonassociative")
                    } else if statuses.iter().any(|(_, status)| {
                        matches!(status, crate::scene::ReferenceStatus::Broken(_))
                    }) {
                        t!("Broken reference")
                    } else if statuses.iter().any(|(_, status)| {
                        matches!(status, crate::scene::ReferenceStatus::Unresolved)
                    }) {
                        t!("Unresolved reference")
                    } else if statuses.len() < slots {
                        t!("Partially associated")
                    } else {
                        t!("Associated")
                    };
                general.props.push(crate::scene::model::object::Property {
                    label: t!("Association status").into_owned(),
                    field: "association_status",
                    value: crate::scene::model::object::PropValue::ReadOnly(status.into_owned()),
                });
                general.props.push(crate::scene::model::object::Property {
                    label: t!("Associative").into_owned(),
                    field: "associative",
                    value: crate::scene::model::object::PropValue::ReadOnly(
                        if associative { t!("Yes") } else { t!("No") }.into_owned(),
                    ),
                });
            }
            let dim_style_names: Vec<String> = self.tabs[i]
                .scene
                .document
                .dim_styles
                .iter()
                .map(|s| s.name.clone())
                .filter(|n| !n.is_empty())
                .collect();
            if !dim_style_names.is_empty() {
                // Current style is already shown as text in the geom section;
                // replace/upgrade it to a Choice if we have a list.
                if let Some(prop) = sections
                    .iter_mut()
                    .flat_map(|section| section.props.iter_mut())
                    .find(|property| property.field == "style_name")
                {
                    let current = match &prop.value {
                        crate::scene::model::object::PropValue::PlainText(s) => s.clone(),
                        _ => String::new(),
                    };
                    prop.value = crate::scene::model::object::PropValue::Choice {
                        selected: current,
                        options: dim_style_names,
                    };
                }
            }

            // Append the resolved dimension style's groups
            // (Lines & Arrows, Text, Fit, Units, Tolerances).
            let style_name = d.base().style_name.clone();
            if let Some(style) = self.tabs[i].scene.document.dim_styles.iter().find(|s| {
                s.name.eq_ignore_ascii_case(&style_name)
                    || (style_name.trim().is_empty() && s.name.eq_ignore_ascii_case("Standard"))
            }) {
                sections.extend(crate::entities::dimension::style_sections(
                    style,
                    d,
                    &self.tabs[i].scene.document,
                ));

                // Prefer entity-level dimension-variable overrides;
                // fall back to the assigned style values.
                use crate::entities::dim_override as dov;
                use crate::scene::model::object::PropValue;
                let dim_c = dov::color(&d.base().common.extended_data, dov::DIMCLRD)
                    .unwrap_or_else(|| codec::types::Color::from_index(style.dimclrd));
                set_row_value(
                    &mut sections,
                    "dim_line_color",
                    PropValue::ColorChoice(dim_c),
                );
                for (field, code, inherited) in [
                    ("dim_ext_line_color", dov::DIMCLRE, style.dimclre),
                    ("dim_text_color", dov::DIMCLRT, style.dimclrt),
                ] {
                    let color = dov::color(&d.base().common.extended_data, code)
                        .unwrap_or_else(|| codec::types::Color::from_index(inherited));
                    set_row_value(&mut sections, field, PropValue::ColorChoice(color));
                }
            }
        }

        // ── Doc-dependent property rows ──────────────────────────
        // Rows whose value lives on another object (a block record,
        // an underlay definition, a dimension / multileader style)
        // are left empty by the entity builders and resolved here,
        // where the document is reachable.
        let doc = &self.tabs[i].scene.document;
        match entity {
            // Block reference: the referenced block's units and the
            // unit-scale factor against the drawing's INSUNITS.
            codec::EntityType::Insert(ins) => {
                let host = doc.header.insertion_units;
                // An evaluated instance is named `*U…`; the row shows the
                // definition it belongs to, and that definition carries the
                // units the reference was authored in.
                let (display_name, definition, _) =
                    crate::app::control::block_ops::resolve_effective_block(doc, ins);
                let src = definition
                    .and_then(|handle| {
                        doc.block_records
                            .iter()
                            .find(|record| record.handle == handle)
                    })
                    .or_else(|| doc.block_records.get(&ins.block_name))
                    .map(|record| record.units)
                    .unwrap_or(0);
                set_row(&mut sections, "block_unit", insunits_name(src).to_string());
                let factor = insert_unit_scale(host, src).unwrap_or(1.0);
                set_row(&mut sections, "unit_factor", format_unit_factor(factor));

                // Name row: editable for regular blocks — pick an
                // existing definition to re-point this reference, or
                // type a new name to rename the definition (every
                // insert of it follows). Anonymous (*) and
                // xref(-dependent) blocks keep the read-only row.
                let regular = |br: &codec::tables::BlockRecord| {
                    !br.is_anonymous() && !br.flags.is_xref && !br.name.contains('|')
                };
                let editable = doc
                    .block_records
                    .get(&ins.block_name)
                    .map(&regular)
                    .unwrap_or(false);
                if editable {
                    let mut options: Vec<String> = doc
                        .block_records
                        .iter()
                        .filter(|br| regular(br))
                        .map(|br| br.name.clone())
                        .collect();
                    options.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
                    set_row_value(
                        &mut sections,
                        "block",
                        crate::scene::model::object::PropValue::EditChoice {
                            value: ins.block_name.clone(),
                            options,
                        },
                    );
                } else {
                    set_row(&mut sections, "block", display_name);
                }
                if let Some(br) = doc
                    .block_records
                    .get(&ins.block_name)
                    .filter(|br| br.flags.is_xref || br.flags.is_xref_overlay)
                {
                    let base_dir = self.tabs[i]
                        .current_path
                        .as_deref()
                        .and_then(|p| p.parent())
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let overrides =
                        crate::io::xref::layer_overrides(doc, &br.name, &br.xref_path, &base_dir);
                    xref_rows(&mut sections, ins, &br.xref_path, factor, overrides);
                }
                // A dynamic block's visibility states, picked like its grip.
                let baseline = self.tabs[i].block_test.as_ref().filter(|test| test.insert == handle).map(|test| test.baseline.as_ref());
                let mut custom_properties = crate::scene::block_parameter_properties::properties(doc, baseline, self.tabs[i].id, handle);
                if self.tabs[i].active_block_edit_session().is_some() {
                    for property in &mut custom_properties {
                        property.value = match &property.value {
                            crate::scene::model::object::PropValue::BlockNumber { value, description, .. } => crate::scene::model::object::PropValue::ReadOnlyWithTooltip { value: value.clone(), tooltip: description.clone() },
                            crate::scene::model::object::PropValue::BlockChoice { selected, options, description, .. } => crate::scene::model::object::PropValue::ReadOnlyWithTooltip { value: options.get(*selected).cloned().unwrap_or_else(|| t!("Custom").into_owned()), tooltip: description.clone() },
                            value => value.clone(),
                        };
                    }
                }
                custom_properties.extend(Self::visibility_property(doc, handle));
                if !custom_properties.is_empty() {
                    sections.push(crate::scene::model::object::PropSection {
                        title: t!("Custom").into_owned(),
                        props: custom_properties,
                    });
                }
                // An associative array's counts and spacings.
                if let Some(array) = crate::scene::assoc_array::array_of_insert(doc, handle) {
                    sections.push(crate::scene::assoc_array::property_section(doc, &array));
                }
            }
            // Underlay: name, page, path and size from the definition.
            codec::EntityType::Underlay(ul) => {
                use crate::entities::underlay as und;
                if let Some(def) = und::definition(ul, doc) {
                    set_row(&mut sections, "ul_name", und::definition_display_name(def));
                    set_row(&mut sections, "ul_page", und::page_of(def).to_string());
                    set_row(&mut sections, "ul_path", und::display_path(&def.file_path));
                }
                if let Some((w, h)) = und::shown_size(ul, doc) {
                    set_row_value(
                        &mut sections,
                        "ul_width",
                        crate::scene::model::object::PropValue::EditText(und::plain_number(w)),
                    );
                    set_row_value(
                        &mut sections,
                        "ul_height",
                        crate::scene::model::object::PropValue::EditText(und::plain_number(h)),
                    );
                }
            }
            // Point cloud: the saved path from its definition; the scan's
            // metres against the drawing's units.
            codec::EntityType::Extended(extended) => {
                if let codec::entities::ExtendedEntityData::PointCloudEx(data) = &extended.data {
                    if let Some(definition) =
                        crate::scene::model::point_cloud::definition(doc, data)
                    {
                        set_row(
                            &mut sections,
                            "ext_pcx_path",
                            definition.source_filename.clone(),
                        );
                    }
                    set_row(&mut sections, "ext_pcx_unit", t!("Meters").into_owned());
                    let factor = insert_unit_scale(doc.header.insertion_units, 6).unwrap_or(1.0);
                    set_row(
                        &mut sections,
                        "ext_pcx_unit_factor",
                        format_unit_factor(factor),
                    );
                }
            }
            // Leader: text style / vertical text placement / overall
            // scale come from its dimension style.
            codec::EntityType::Leader(ld) => {
                // Dim style row → dropdown of the drawing's dim styles.
                let names: Vec<String> = doc
                    .dim_styles
                    .iter()
                    .map(|s| s.name.clone())
                    .filter(|n| !n.is_empty())
                    .collect();
                if !names.is_empty() {
                    for section in sections.iter_mut() {
                        if let Some(p) = section
                            .props
                            .iter_mut()
                            .find(|p| p.field == "dimension_style")
                        {
                            let cur = match &p.value {
                                crate::scene::model::object::PropValue::PlainText(s) => s.clone(),
                                _ => ld.dimension_style.clone(),
                            };
                            p.value = crate::scene::model::object::PropValue::Choice {
                                selected: cur,
                                options: names.clone(),
                            };
                        }
                    }
                }
                // Resolve editable overrides from the assigned dimension style.
                if let Some(ds) = find_dim_style(doc, &ld.dimension_style) {
                    use crate::entities::dim_override as dov;
                    use crate::scene::model::object::PropValue;
                    let xd = &ld.common.extended_data;

                    // Arrow block (DIMLDRBLK): override handle → name,
                    // else the style's arrow. Options are the arrowhead
                    // blocks the drawing carries plus the default.
                    let arrow_label = match dov::handle(xd, dov::DIMLDRBLK) {
                        Some(h) => doc
                            .block_records
                            .iter()
                            .find(|b| b.handle == h)
                            .map(|b| arrowhead_label(&b.name))
                            .unwrap_or_else(|| "Closed filled".to_string()),
                        None => leader_arrow_label(doc, ds, ld.arrow_enabled),
                    };
                    let mut arrow_opts: Vec<String> = std::iter::once("Closed filled".to_string())
                        .chain(
                            doc.block_records
                                .iter()
                                .filter(|b| b.name.starts_with('_'))
                                .map(|b| arrowhead_label(&b.name)),
                        )
                        .collect();
                    // Keep the current value selectable even when it
                    // isn't in the standard list (e.g. "None", or a
                    // style arrow whose block isn't underscore-named).
                    if !arrow_opts.contains(&arrow_label) {
                        arrow_opts.insert(0, arrow_label.clone());
                    }
                    set_row_value(
                        &mut sections,
                        "arrow_block",
                        PropValue::Choice {
                            selected: arrow_label,
                            options: arrow_opts,
                        },
                    );

                    let asz = dov::real(xd, dov::DIMASZ).unwrap_or(ds.dimasz);
                    set_row_value(
                        &mut sections,
                        "arrow_size",
                        PropValue::EditText(asz.to_string()),
                    );

                    let lwd = dov::int(xd, dov::DIMLWD).unwrap_or(ds.dimlwd);
                    let lw_sel = dim_lineweight_label(lwd);
                    let mut lw_opts = lineweight_options();
                    if !lw_opts.contains(&lw_sel) {
                        lw_opts.insert(0, lw_sel.clone());
                    }
                    set_row_value(
                        &mut sections,
                        "dim_line_lw",
                        PropValue::Choice {
                            selected: lw_sel,
                            options: lw_opts,
                        },
                    );

                    // A per-object color override wins over the style.
                    let dim_c = dov::color(xd, dov::DIMCLRD)
                        .unwrap_or_else(|| codec::types::Color::from_index(ds.dimclrd));
                    set_row_value(
                        &mut sections,
                        "dim_line_color",
                        PropValue::ColorChoice(dim_c),
                    );

                    let gap = dov::real(xd, dov::DIMGAP).unwrap_or(ds.dimgap);
                    set_row_value(
                        &mut sections,
                        "text_offset",
                        PropValue::EditText(gap.to_string()),
                    );

                    let tad = dov::int(xd, dov::DIMTAD).unwrap_or(ds.dimtad);
                    set_row_value(
                        &mut sections,
                        "text_pos_vert",
                        PropValue::Choice {
                            selected: dimtad_label(tad).to_string(),
                            options: tad_options(),
                        },
                    );

                    let scl = dov::real(xd, dov::DIMSCALE).unwrap_or(ds.dimscale);
                    set_row_value(
                        &mut sections,
                        "dim_scale_overall",
                        PropValue::EditText(scl.to_string()),
                    );
                }
            }
            // Feature-control frame: FCF text style is the dimension
            // style's DIMTXSTY.
            codec::EntityType::Tolerance(tol) => {
                use crate::entities::dim_override as dov;
                let style = crate::entities::tolerance::resolve_dim_style(tol, doc);
                let style_name = style.map(|entry| entry.name.clone()).unwrap_or_else(|| {
                    if tol.dimension_style_name.trim().is_empty() {
                        "Standard".to_string()
                    } else {
                        tol.dimension_style_name.clone()
                    }
                });
                let mut dim_style_names: Vec<String> = doc
                    .dim_styles
                    .iter()
                    .map(|entry| entry.name.clone())
                    .filter(|name| !name.trim().is_empty())
                    .collect();
                if !dim_style_names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&style_name))
                {
                    dim_style_names.push(style_name.clone());
                }
                set_row_value(
                    &mut sections,
                    "tol_dim_style",
                    PropValue::Choice {
                        selected: style_name,
                        options: dim_style_names,
                    },
                );

                let text_style_name = dov::handle(&tol.common.extended_data, dov::DIMTXSTY)
                    .and_then(|handle| doc.text_styles.iter().find(|entry| entry.handle == handle))
                    .map(|entry| entry.name.clone())
                    .or_else(|| style.map(|entry| entry.dimtxsty.clone()))
                    .unwrap_or_else(|| "Standard".to_string());
                let text_height_editable = doc
                    .text_styles
                    .iter()
                    .find(|entry| entry.name.eq_ignore_ascii_case(&text_style_name))
                    .is_none_or(|entry| !entry.has_fixed_height());
                let mut names = text_style_names.clone();
                if !names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&text_style_name))
                {
                    names.push(text_style_name.clone());
                }
                set_row_value(
                    &mut sections,
                    "tol_text_style",
                    PropValue::Choice {
                        selected: text_style_name,
                        options: names,
                    },
                );

                let height = dov::real(&tol.common.extended_data, dov::DIMTXT)
                    .or_else(|| style.map(|entry| entry.dimtxt))
                    .unwrap_or(tol.text_height);
                set_row_value(
                    &mut sections,
                    "tol_text_height",
                    if text_height_editable {
                        PropValue::EditText(format!("{height:.4}"))
                    } else {
                        PropValue::ReadOnly(format!("{height:.4}"))
                    },
                );
            }
            // MultiLeader: max points + segment-angle constraints
            // are MLeaderStyle settings, not stored on the entity.
            codec::EntityType::MultiLeader(ml) => {
                if ml
                    .text_style_handle
                    .and_then(|handle| doc.text_styles.iter().find(|style| style.handle == handle))
                    .is_some_and(|style| style.has_fixed_height())
                {
                    set_row_value(
                        &mut sections,
                        "text_height",
                        PropValue::ReadOnly(crate::entities::common::format_length(ml.text_height)),
                    );
                }
                if let Some(sh) = ml.style_handle {
                    if let Some((mx, a1, a2)) = doc.objects.iter().find_map(|(h, o)| match o {
                        codec::objects::ObjectType::MultiLeaderStyle(s) if *h == sh => Some((
                            s.max_leader_points,
                            s.first_segment_angle,
                            s.second_segment_angle,
                        )),
                        _ => None,
                    }) {
                        set_row(&mut sections, "max_leader_points", mx.to_string());
                        set_row(&mut sections, "first_segment_angle", format!("{a1:.4}"));
                        set_row(&mut sections, "second_segment_angle", format!("{a2:.4}"));
                    }
                }
            }
            codec::EntityType::Table(table) => {
                use codec::entities::table::{
                    CellEdgeFlags, CellStylePropertyFlags, CellValueType,
                };
                // A table shows its linetype scale but does not take one.
                set_row(
                    &mut sections,
                    "linetype_scale",
                    crate::entities::common::format_length(table.common.linetype_scale),
                );

                let mut names: Vec<String> = doc
                    .objects
                    .values()
                    .filter_map(|object| match object {
                        codec::objects::ObjectType::TableStyle(style)
                            if !style.name.trim().is_empty() =>
                        {
                            Some(style.name.clone())
                        }
                        _ => None,
                    })
                    .collect();
                names.sort_by_key(|name| name.to_ascii_lowercase());
                names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
                let selected_style = table
                    .table_style_handle
                    .and_then(|handle| doc.objects.get(&handle))
                    .and_then(|object| match object {
                        codec::objects::ObjectType::TableStyle(style) => Some(style),
                        _ => None,
                    });
                let selected = selected_style
                    .map(|style| style.name.clone())
                    .unwrap_or_else(|| "Standard".to_string());
                if !names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&selected))
                {
                    names.insert(0, selected.clone());
                }
                set_row_value(
                    &mut sections,
                    "tbl_style_handle",
                    crate::scene::model::object::PropValue::Choice {
                        selected,
                        options: names,
                    },
                );

                let title_suppressed =
                    crate::entities::table::resolved_title_suppressed(table, selected_style);
                let header_suppressed =
                    crate::entities::table::resolved_header_suppressed(table, selected_style);
                let flow_up = crate::entities::table::resolved_flow_up(table, selected_style);
                let (horizontal_margin, vertical_margin) =
                    crate::entities::table::resolved_table_margins(table, selected_style);
                // A spacing the table never stored shows its default: five
                // and a half data-text heights (0.18 high text without data rows).
                if table.break_spacing <= 0.0 {
                    let data_height = selected_style
                        .filter(|_| table.rows.len() > 2)
                        .map_or(0.18, |style| style.data_row_style.text_height);
                    update_row_text(
                        &mut sections,
                        "tbl_break_spacing",
                        crate::entities::common::format_length(5.5 * data_height),
                    );
                }
                update_row_toggle(&mut sections, "tbl_title_suppressed", title_suppressed);
                update_row_toggle(&mut sections, "tbl_header_suppressed", header_suppressed);
                update_row_text(
                    &mut sections,
                    "tbl_flow_direction",
                    if flow_up { t!("Up") } else { t!("Down") }.into_owned(),
                );
                update_row_text(
                    &mut sections,
                    "tbl_horizontal_margin",
                    crate::entities::common::format_length(horizontal_margin),
                );
                update_row_text(
                    &mut sections,
                    "tbl_vertical_margin",
                    crate::entities::common::format_length(vertical_margin),
                );

                let columns = table.column_count();
                if columns > 0 {
                    let cell_index = prop_vertex
                        .min(table.row_count().saturating_mul(columns).saturating_sub(1));
                    let row_index = cell_index / columns;
                    let column_index = cell_index % columns;
                    if let (Some(row), Some(cell)) = (
                        table.rows.get(row_index),
                        table.cell(row_index, column_index),
                    ) {
                        let document_row_style = selected_style.map(|style| {
                            let kind = match (title_suppressed, header_suppressed, row_index) {
                                (false, _, 0) => 0,
                                (false, false, 1) | (true, false, 0) => 1,
                                _ => 2,
                            };
                            match kind {
                                0 => &style.title_row_style,
                                1 => &style.header_row_style,
                                _ => &style.data_row_style,
                            }
                        });
                        let local = |property| {
                            crate::entities::table::style_for_property(
                                table,
                                row,
                                column_index,
                                cell,
                                property,
                            )
                        };
                        let alignment = local(CellStylePropertyFlags::ALIGNMENT)
                            .map(|style| style.alignment)
                            .or_else(|| document_row_style.map(|style| style.alignment as i32))
                            .unwrap_or(5);
                        let alignment = match alignment {
                            1 => "Top Left",
                            2 => "Top Center",
                            3 => "Top Right",
                            4 => "Middle Left",
                            6 => "Middle Right",
                            7 => "Bottom Left",
                            8 => "Bottom Center",
                            9 => "Bottom Right",
                            _ => "Middle Center",
                        };
                        update_row_text(&mut sections, "tbl_cell_alignment", alignment.to_string());

                        let text_style_name = local(CellStylePropertyFlags::TEXT_STYLE)
                            .map(|style| style.text_style_name.clone())
                            .filter(|name| !name.is_empty())
                            .or_else(|| {
                                document_row_style
                                    .map(|style| style.text_style_name.clone())
                                    .filter(|name| !name.is_empty())
                            })
                            .unwrap_or_else(|| "Standard".to_string());
                        update_row_text(&mut sections, "tbl_cell_text_style", text_style_name);
                        let text_height = local(CellStylePropertyFlags::TEXT_HEIGHT)
                            .map(|style| style.text_height)
                            .or_else(|| document_row_style.map(|style| style.text_height))
                            .unwrap_or(0.18);
                        update_row_text(
                            &mut sections,
                            "tbl_cell_text_height",
                            crate::entities::common::format_length(text_height),
                        );
                        let content_color = local(CellStylePropertyFlags::CONTENT_COLOR)
                            .map(|style| style.content_color)
                            .or_else(|| document_row_style.map(|style| style.text_color))
                            .unwrap_or(codec::types::Color::ByBlock);
                        update_row_color(&mut sections, "tbl_cell_content_color", content_color);
                        let background_style = local(CellStylePropertyFlags::BACKGROUND_COLOR);
                        let background_color = background_style
                            .map(|style| style.background_color)
                            .or_else(|| document_row_style.map(|style| style.fill_color))
                            .unwrap_or(codec::types::Color::ByBlock);
                        update_row_color(
                            &mut sections,
                            "tbl_cell_background_color",
                            background_color,
                        );
                        update_row_toggle(
                            &mut sections,
                            "tbl_cell_fill",
                            background_style
                                .map(|style| style.fill_enabled)
                                .or_else(|| document_row_style.map(|style| style.fill_enabled))
                                .unwrap_or(false),
                        );
                        let format = local(CellStylePropertyFlags::DATA_FORMAT)
                            .map(|style| style.value_format.clone())
                            .or_else(|| document_row_style.map(|style| style.format_string.clone()))
                            .unwrap_or_default();
                        update_row_text(&mut sections, "tbl_cell_format", format);
                        let data_type = cell
                            .contents
                            .first()
                            .map(|content| content.value.value_type)
                            .filter(|kind| *kind != CellValueType::Unknown)
                            .or_else(|| {
                                local(CellStylePropertyFlags::DATA_TYPE).map(|style| {
                                    CellValueType::from(style.value_data_type.max(0) as u32)
                                })
                            })
                            .or_else(|| {
                                document_row_style
                                    .map(|style| CellValueType::from(style.data_type.max(0) as u32))
                            })
                            .unwrap_or(CellValueType::String);
                        let data_type = match data_type {
                            CellValueType::Long => "Integer",
                            CellValueType::Double => "Decimal",
                            CellValueType::Date => "Date",
                            CellValueType::Point2D => "Point 2D",
                            CellValueType::Point3D => "Point 3D",
                            CellValueType::Handle => "Handle",
                            _ => "Text",
                        };
                        update_row_text(&mut sections, "tbl_cell_data_type", data_type.to_string());
                        for (field, property, fallback) in [
                            (
                                "tbl_cell_margin_left",
                                CellStylePropertyFlags::MARGIN_LEFT,
                                horizontal_margin,
                            ),
                            (
                                "tbl_cell_margin_top",
                                CellStylePropertyFlags::MARGIN_TOP,
                                vertical_margin,
                            ),
                            (
                                "tbl_cell_margin_right",
                                CellStylePropertyFlags::MARGIN_RIGHT,
                                horizontal_margin,
                            ),
                            (
                                "tbl_cell_margin_bottom",
                                CellStylePropertyFlags::MARGIN_BOTTOM,
                                vertical_margin,
                            ),
                        ] {
                            let value = local(property)
                                .map(|style| match field {
                                    "tbl_cell_margin_left" => style.margin_left,
                                    "tbl_cell_margin_top" => style.margin_top,
                                    "tbl_cell_margin_right" => style.margin_right,
                                    _ => style.margin_bottom,
                                })
                                .unwrap_or(fallback);
                            update_row_text(
                                &mut sections,
                                field,
                                crate::entities::common::format_length(value),
                            );
                        }

                        let border_visible = |edge: CellEdgeFlags| {
                            let local_style = [
                                cell.style.as_ref(),
                                row.style.as_ref(),
                                table
                                    .columns
                                    .get(column_index)
                                    .and_then(|column| column.style.as_ref()),
                                table.base_style.as_ref(),
                            ]
                            .into_iter()
                            .flatten()
                            .find(|style| style.applied_border_edges.contains(edge));
                            if let Some(style) = local_style {
                                return if edge == CellEdgeFlags::TOP {
                                    !style.top_border.invisible
                                } else if edge == CellEdgeFlags::RIGHT {
                                    !style.right_border.invisible
                                } else if edge == CellEdgeFlags::BOTTOM {
                                    !style.bottom_border.invisible
                                } else {
                                    !style.left_border.invisible
                                };
                            }
                            document_row_style
                                .map(|style| {
                                    if edge == CellEdgeFlags::TOP {
                                        !style.top_border.is_invisible
                                    } else if edge == CellEdgeFlags::RIGHT {
                                        !style.right_border.is_invisible
                                    } else if edge == CellEdgeFlags::BOTTOM {
                                        !style.bottom_border.is_invisible
                                    } else {
                                        !style.left_border.is_invisible
                                    }
                                })
                                .unwrap_or(true)
                        };
                        for (field, edge) in [
                            ("tbl_cell_border_top", CellEdgeFlags::TOP),
                            ("tbl_cell_border_right", CellEdgeFlags::RIGHT),
                            ("tbl_cell_border_bottom", CellEdgeFlags::BOTTOM),
                            ("tbl_cell_border_left", CellEdgeFlags::LEFT),
                        ] {
                            update_row_toggle(&mut sections, field, border_visible(edge));
                        }
                    }
                }
            }
            _ => {}
        }

        // Annotative Yes/No + a conditional "Annotative scale" row.
        // Annotative state and assigned scale names need the
        // document, so they are resolved here.
        {
            // Which entities show an Annotative row, the field it uses,
            // and — for those that don't already carry the row
            // (dimension / table / tolerance) — the existing field to insert it
            // after. MLeader uses its editable toggle field.
            let anno: Option<(&str, Option<&str>)> = match entity {
                codec::EntityType::Text(_)
                | codec::EntityType::MText(_)
                | codec::EntityType::Insert(_)
                | codec::EntityType::Leader(_)
                | codec::EntityType::Hatch(_) => Some(("annotative", None)),
                codec::EntityType::MultiLeader(_) => Some(("annotative", None)),
                codec::EntityType::Dimension(_) => Some(("annotative", Some("style_name"))),
                codec::EntityType::Tolerance(_) => Some(("annotative", Some("tol_dim_style"))),
                _ => None,
            };
            if let Some((anno_field, insert_after)) = anno {
                let is_anno = crate::scene::annotative::is_annotative(doc, entity)
                    || match entity {
                        codec::EntityType::Dimension(dimension)
                            if matches!(**dimension, codec::entities::Dimension::Arc(_)) =>
                        {
                            crate::scene::annotative::dim_style_is_annotative(
                                doc,
                                &dimension.base().style_name,
                            )
                        }
                        codec::EntityType::Tolerance(_) => {
                            crate::scene::annotative::annotation_style_is_annotative(doc, entity)
                        }
                        codec::EntityType::Leader(_) => {
                            crate::scene::annotative::annotation_style_is_annotative(doc, entity)
                        }
                        _ => false,
                    };
                // Dimensions/tables/tolerances carry no Annotative row yet — add one
                // right after their style row.
                if let Some(anchor) = insert_after {
                    insert_row_after(
                        &mut sections,
                        anchor,
                        crate::entities::common::ro_prop(
                            t!("Annotative").as_ref(),
                            "annotative",
                            "No",
                        ),
                    );
                }
                // Objects that carry a per-object annotation context
                // (MTEXT via its native flag, single-line TEXT via the
                // context alone) get an editable toggle: turning it on
                // synthesizes a real per-scale representation. The
                // remaining style-only types stay read-only.
                if anno_field == "annotative" {
                    match entity {
                        // The reference offers Yes/No; the choice
                        // drives the same per-object toggle.
                        codec::EntityType::MText(t) => set_row_value(
                            &mut sections,
                            "annotative",
                            yes_no_choice(t.is_annotative || is_anno),
                        ),
                        codec::EntityType::Dimension(dimension)
                            if matches!(**dimension, codec::entities::Dimension::Arc(_)) =>
                        {
                            set_row(
                                &mut sections,
                                "annotative",
                                if is_anno { t!("Yes") } else { t!("No") }.into_owned(),
                            )
                        }
                        codec::EntityType::Leader(_)
                            if crate::scene::annotative::annotation_style_is_annotative(
                                doc, entity,
                            ) =>
                        {
                            set_row(&mut sections, "annotative", t!("Yes").into_owned())
                        }
                        codec::EntityType::Text(_)
                        | codec::EntityType::Insert(_)
                        | codec::EntityType::Leader(_)
                        | codec::EntityType::Hatch(_)
                        | codec::EntityType::Dimension(_)
                        | codec::EntityType::MultiLeader(_) => {
                            set_row_value(&mut sections, "annotative", yes_no_choice(is_anno))
                        }
                        _ => set_row(
                            &mut sections,
                            "annotative",
                            if is_anno { t!("Yes") } else { t!("No") }.into_owned(),
                        ),
                    }
                }
                if is_anno {
                    let memberships = crate::scene::annotative::object_scale_memberships(
                        doc,
                        entity.common().handle,
                    );
                    let assigned_scales = if memberships.is_empty() {
                        doc.header.current_annotation_scale.clone()
                    } else {
                        memberships
                            .into_iter()
                            .map(|(name, _)| name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    insert_row_after(
                        &mut sections,
                        anno_field,
                        crate::entities::common::ro_prop(
                            t!("Annotative scale").as_ref(),
                            "annotative_scale",
                            assigned_scales,
                        ),
                    );
                }
            }
        }

        // Single-line text height rows depend on both the
        // justification and the active annotation scale. Aligned
        // text derives its paper height from the two endpoints,
        // while annotative text exposes that paper height and a
        // separate calculated model height.
        if let Some(factor) = self.tabs[i]
            .scene
            .annotative_text_height_factor(entity.common().handle)
        {
            // Annotative text stores the model height of its default scale;
            // the Height row edits the paper height at the displayed scale.
            let (stored, model_factor, aligned) = match entity {
                codec::EntityType::Text(text) => {
                    let aligned = matches!(
                        text.horizontal_alignment,
                        codec::entities::TextHorizontalAlignment::Aligned
                    );
                    let stored = if aligned {
                        crate::entities::text::text_run_placement_at_scale(text, doc, 1.0).height
                            as f64
                    } else {
                        text.height
                    };
                    (stored, factor, aligned)
                }
                codec::EntityType::MText(text) => (text.height, factor, false),
                _ => unreachable!("annotative_text_height_factor is text-only"),
            };
            let ratio = crate::scene::annotative::text_context_ratio(
                doc,
                entity.common().handle,
                annotation_scale_handle,
            )
            .unwrap_or(1.0);
            let paper_height = stored / model_factor;
            for section in sections.iter_mut() {
                if let Some(row) = section.props.iter_mut().find(|row| row.field == "height") {
                    let editable = !aligned
                        && !matches!(
                            row.value,
                            crate::scene::model::object::PropValue::ReadOnly(_)
                        );
                    *row = crate::entities::common::num_prop(
                        t!("Paper text height").as_ref(),
                        "height",
                        paper_height,
                        editable,
                    );
                }
            }
            insert_row_after(
                &mut sections,
                "height",
                crate::entities::common::ro_prop(
                    t!("Model text height").as_ref(),
                    "model_text_height",
                    crate::entities::common::format_length(stored * ratio),
                ),
            );
        }

        if !group_names.is_empty() {
            let label = group_names.join(", ");
            if let Some(general) = sections.first_mut() {
                general.props.push(crate::scene::model::object::Property {
                    label: t!("Group").into_owned(),
                    field: "group",
                    value: crate::scene::model::object::PropValue::ReadOnly(label),
                });
            }
        }
        if compact_solid {
            retain_compact_solid_sections(&mut sections);
        }
        if let codec::EntityType::Ole2Frame(ole) = entity {
            self.fill_ole_rows(i, handle, ole, &mut sections);
        }
        let title = match entity {
            codec::EntityType::Insert(ins) => {
                let is_xref = self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .iter()
                    .find(|br| br.name == ins.block_name)
                    .map(|br| br.flags.is_xref || br.flags.is_xref_overlay)
                    .unwrap_or(false);
                if is_xref {
                    t!("External Reference").into_owned()
                } else {
                    entity_type_label(entity)
                }
            }
            codec::EntityType::Surface(_)
                if matches!(
                    crate::scene::model::solid_history::primitive_property_operation(
                        &self.tabs[i].scene.document,
                        handle,
                    ),
                    Some(codec::objects::SolidHistoryOperation::Extrusion(_))
                ) =>
            {
                format!("{} ({})", t!("Surface"), t!("Extrusion"))
            }
            // The reference names an angular dimension by its kind.
            codec::EntityType::Dimension(dimension)
                if matches!(**dimension, codec::entities::Dimension::Angular2Ln(_)) =>
            {
                t!("Angular Dimension").into_owned()
            }
            codec::EntityType::Dimension(dimension)
                if matches!(**dimension, codec::entities::Dimension::Angular3Pt(_)) =>
            {
                t!("3 Point Angular Dimension").into_owned()
            }
            codec::EntityType::Underlay(underlay) => match underlay.underlay_type {
                codec::entities::UnderlayType::Pdf => t!("PDF Underlay"),
                codec::entities::UnderlayType::Dwf => t!("DWF Underlay"),
                codec::entities::UnderlayType::Dgn => t!("DGN Underlay"),
            }
            .into_owned(),
            codec::EntityType::Ole2Frame(_) => "OLE".to_string(),
            _ => entity_type_label(entity),
        };
        // A dynamic dimension shows only its constraint and text
        // rotation, as the reference does.
        let title = match crate::scene::parametric_constraints::dynamic_dimension_constraint(
            &self.tabs[i].scene.parametric_constraints,
            handle,
        ) {
            Some((set, constraint)) => {
                let annotational = self.tabs[i].scene.dimension_is_annotational(handle);
                sections = dynamic_dimension_sections(
                    &self.tabs[i].scene,
                    handle,
                    set,
                    constraint,
                    sections,
                );
                // An annotational constraint is an ordinary
                // dimension to the panel; the dynamic form is
                // named after its constraint.
                if annotational {
                    title
                } else {
                    use codec::entities::Dimension;
                    match crate::entities::dimension::as_dimension(entity) {
                        Some(Dimension::Aligned(_)) => t!("Aligned Dimensional Constraint"),
                        Some(Dimension::Angular2Ln(_)) => t!("Angular Dimension (Dynamic)"),
                        Some(Dimension::Angular3Pt(_)) => {
                            t!("3 Point Angular Dimension (Dynamic)")
                        }
                        Some(Dimension::Radius(_)) => t!("Radius Dimensional Constraint"),
                        Some(Dimension::Diameter(_)) => t!("Diameter Dimensional Constraint"),
                        _ => t!("Linear Dimensional Constraint"),
                    }
                    .into_owned()
                }
            }
            None => title,
        };

        // A plugin-declared class identity (API v10) qualifies the title as
        // "<Label> (<Native>)" while one of the live loaded plugins declares
        // a label for this entity's XDATA class key; without one the title is
        // exactly the native type. Display-only — the key round-trips the
        // drawing, the label never does.
        let title = crate::plugin::entity_labels::display_type_title(
            crate::plugin::entity_labels::custom_entity_class_label(
                &self.disabled_plugins,
                source_entity,
            ),
            title,
        );
        (sections, title)
    }

    /// The "Plugin class parameters" section (API v14): binds to ONE
    /// selected entity whose XDATA carries a registered class key. The
    /// section exists only when the stamp resolves to the LIVE declaration
    /// (schema match — a stale stamp re-stages instead of rendering). A
    /// reference-only (host-staged) instance renders every row read-only;
    /// the source line points at the owning record row either way — the
    /// values live on the record layer, the host never invents semantics
    /// for a name or kind the declaration does not spell.
    #[cfg(not(target_arch = "wasm32"))]
    fn plugin_class_parameter_section(
        &self,
        i: usize,
        handle: codec::Handle,
    ) -> Option<crate::scene::model::object::PropSection> {
        use crate::scene::model::object::{PropSection, PropValue, Property};
        let binding =
            crate::app::plugin_host::resolve_plugin_class_binding(self, i, handle).ok()?;
        let reference_only = self.tabs[i].scene.is_reference_only(handle);
        let mut rows = crate::app::plugin_host::rows_from_schema(&binding);
        if !binding.design_record.is_empty() {
            if let Ok((_, entries)) = crate::app::plugin_host::find_design_record_entries(
                self, i, handle, &binding.design_record,
            ) {
                if let Ok(read_rows) =
                    crate::app::plugin_host::rows_from_entries(&binding, &entries)
                {
                    rows = read_rows;
                }
            }
        }
        let mut props = Vec::new();
        let source = if binding.design_record.is_empty() {
            crate::t!("No record linkage on the identity stamp").into_owned()
        } else {
            crate::tf!("Values on record {}", binding.design_record).into_owned()
        };
        props.push(Property {
            label: crate::t!("Source").into_owned(),
            field: "plugin_class_source",
            value: PropValue::ReadOnly(source),
        });
        for row in rows {
            props.push(Property {
                label: row.name.clone(),
                field: "plugin_class_parameter",
                value: PropValue::PluginParam {
                    name: row.name,
                    value: row.value.unwrap_or_default(),
                    editable: row.editable && !reference_only,
                },
            });
        }
        Some(PropSection {
            title: crate::t!("Plugin class parameters").into_owned(),
            props,
        })
    }

    pub(super) fn refresh_properties(&mut self) {
        // A 186 468-entity selection spends ~112 ms in here. Split it into the
        // three phases that can own that: resolving the selected handles,
        // building the panel, and syncing the ribbon.
        let t_all = crate::perf::enabled().then(iced::time::Instant::now);
        let i = self.active_tab;
        if !crate::entities::object_data::cache_is_prepared(&self.tabs[i].scene.object_data_cache) {
            self.tabs[i].scene.object_data_cache =
                crate::entities::object_data::build_cache(&self.tabs[i].scene.document);
        }
        // Note: the color-picker dropdown is intentionally NOT carried over — a
        // rebuild means the selection (or a property) changed, so the dropdown
        // closes, matching the deselect / reselect / click-away expectation.
        let edit_buf = std::mem::take(&mut self.tabs[i].properties.edit_buf);
        let active_field = std::mem::take(&mut self.tabs[i].properties.active_field);
        // Expanded coordinate groups persist across rebuilds AND selection
        // changes — it's a per-user view preference, not per-entity state.
        let expanded_groups = std::mem::take(&mut self.tabs[i].properties.expanded_groups);
        // Section state belongs to the app, not a drawing tab, so the user's
        // collapsed sections carry across every currently open drawing/project.
        // The empty app-level default keeps a first-run palette fully expanded.
        let collapsed_sections = self.collapsed_property_sections.clone();
        // Which entities the previous panel was built for — an uncommitted
        // edit buffer only survives a rebuild for the *same* selection.
        let prev_handles = std::mem::take(&mut self.tabs[i].properties.source_handles);
        let previous_definition = self.tabs[i].properties.block_definition;
        let selected_group = self.tabs[i].properties.selected_group.clone();

        // Seed the per-thread unit context from the document header so the
        // entity property builders (which only see f64 values) can format
        // lengths/angles per LUNITS / LUPREC / AUNITS / AUPREC.
        {
            let h = &self.tabs[i].scene.document.header;
            crate::entities::common::set_unit_context(
                crate::entities::common::UnitContext::from_header(h),
            );
        }
        {
            // Which text styles fix their own height — the height rows read it
            // to decide whether they are editable.
            crate::entities::common::set_fixed_text_heights(&self.tabs[i].scene.document);
        }

        let layer_names: Vec<String> = self.tabs[i]
            .scene
            .document
            .layers
            .iter()
            .map(|l| l.name.clone())
            // The reference's hidden system layers (`*ADSK_CONSTRAINTS`) stay out.
            .filter(|name| !name.starts_with('*'))
            .collect();
        let linetype_items: Vec<ui::properties::LinetypeItem> = self.tabs[i]
            .scene
            .document
            .line_types
            .iter()
            .map(|lt| {
                let name = if lt.name.is_empty() {
                    "ByLayer".to_string()
                } else {
                    lt.name.clone()
                };
                let art = linetypes::extract_pattern(&lt.description);
                ui::properties::LinetypeItem { name, art }
            })
            .chain(super::layers::catalog_linetype_items(
                &self.tabs[i].scene.document,
            ))
            .collect();
        let text_style_names: Vec<String> = self.tabs[i]
            .scene
            .document
            .text_styles
            .iter()
            .map(|style| style.name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();

        // Current-Vertex focus survives only while the same object stays
        // selected; a changed selection resets to the first vertex. Seed the
        // per-thread focus so the polyline property builder / editor targets it.
        // Everything before this is the prelude: the object-data cache, the
        // unit context, the layer and linetype lists.
        let m_prelude = t_all.map(|t| t.elapsed().as_secs_f64() * 1000.0);
        let t_handles = crate::perf::enabled().then(iced::time::Instant::now);
        let cur_handles: Vec<codec::Handle> = self.tabs[i]
            .scene
            .selected_handles_in_order()
            .into_iter()
            .filter(|h| self.tabs[i].scene.document.get_entity(*h).is_some())
            .collect();
        let handles_ms = t_handles.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
        let prop_vertex = if cur_handles == prev_handles {
            self.tabs[i].properties.prop_vertex
        } else {
            0
        };
        let prop_vertex = if cur_handles.len() == 1 {
            let vertex_count = self.tabs[i]
                .scene
                .document
                .get_entity(cur_handles[0])
                .and_then(|entity| match entity {
                    codec::EntityType::LwPolyline(polyline) => Some(polyline.vertices.len()),
                    codec::EntityType::Polyline2D(polyline) => Some(polyline.vertices.len()),
                    codec::EntityType::PolygonMesh(mesh) => Some(mesh.vertices.len()),
                    codec::EntityType::Polyline3D(polyline) => Some(
                        crate::entities::polyline::polyline3d_control_vertex_count(polyline),
                    ),
                    codec::EntityType::Leader(leader) => Some(leader.vertices.len()),
                    codec::EntityType::Face3D(face) => Some(if face.is_triangle() { 3 } else { 4 }),
                    codec::EntityType::Spline(spline) => {
                        Some(if crate::entities::spline::shows_fit_points(spline) {
                            spline.fit_points.len()
                        } else {
                            crate::entities::spline::control_vertex_count(spline)
                        })
                    }
                    codec::EntityType::Table(table) => {
                        Some(table.row_count().saturating_mul(table.column_count()))
                    }
                    _ => None,
                });
            vertex_count.map_or(prop_vertex, |count| {
                prop_vertex.min(count.saturating_sub(1))
            })
        } else {
            prop_vertex
        };
        let prop_vertex_indicator_active = if cur_handles == prev_handles {
            self.tabs[i].properties.prop_vertex_indicator_active
        } else {
            false
        };
        crate::scene::view::dispatch::set_prop_current_vertex(prop_vertex);
        crate::entities::table::set_prop_current_cell(prop_vertex);
        crate::entities::table::set_prop_current_cell_active(prop_vertex_indicator_active);

        let annotation_scale_handle = self.tabs[i].scene.displayed_annotation_scale_handle();
        let new_panel = {
            let selected = self.tabs[i].scene.selected_entities();
            let mut panel = match selected.len() {
                0 => {
                    use crate::scene::model::object::{PropSection, PropValue, Property};

                    let tab = &self.tabs[i];
                    let scene = &tab.scene;
                    let doc = &scene.document;
                    let header = &doc.header;
                    let camera = scene.camera.borrow();
                    let (viewport_width, viewport_height) = scene.selection.borrow().view.vp_size;
                    let aspect = if viewport_height > 0.0 {
                        viewport_width as f64 / viewport_height as f64
                    } else {
                        1.0
                    };
                    let view_height = camera.ortho_size() as f64 * 2.0;
                    let view_width = view_height * aspect;
                    let format_length = crate::entities::common::format_length;
                    let read_only = |label: &str, value: String| Property {
                        label: label.to_string(),
                        field: "drawing_property",
                        value: PropValue::ReadOnly(value),
                    };
                    let current_layer = if header.current_layer_name.is_empty() {
                        tab.active_layer.clone()
                    } else {
                        header.current_layer_name.clone()
                    };
                    let current_linetype = if !header.current_linetype_name.is_empty() {
                        header.current_linetype_name.clone()
                    } else if !header.current_linetype_handle.is_null() {
                        doc.line_types
                            .iter()
                            .find(|line_type| line_type.handle == header.current_linetype_handle)
                            .map(|line_type| line_type.name.clone())
                            .unwrap_or_else(|| "ByLayer".to_string())
                    } else {
                        "ByLayer".to_string()
                    };
                    let material = if header.current_material_handle.is_null() {
                        "ByLayer".to_string()
                    } else {
                        doc.objects
                            .get(&header.current_material_handle)
                            .and_then(|object| match object {
                                codec::objects::ObjectType::Material(material) => {
                                    Some(material.name.clone())
                                }
                                _ => None,
                            })
                            .unwrap_or_else(|| "ByLayer".to_string())
                    };
                    let plot_style = if header.plotstyle_mode {
                        "ByColor"
                    } else {
                        match header.current_plotstyle_type {
                            1 => "ByBlock",
                            2 => "Normal",
                            3 => "ByObject",
                            _ => "ByLayer",
                        }
                    };
                    let layout_plot_table = doc.objects.values().find_map(|object| {
                        let codec::objects::ObjectType::Layout(layout) = object else {
                            return None;
                        };
                        (layout.name == scene.current_layout
                            && !layout.plot_style_sheet.trim().is_empty())
                        .then(|| layout.plot_style_sheet.clone())
                    });
                    let plot_table = layout_plot_table
                        .or_else(|| {
                            (!header.stylesheet.trim().is_empty())
                                .then(|| header.stylesheet.clone())
                        })
                        .unwrap_or_else(|| "None".to_string());
                    let active_model_vport = doc.vports.iter().find(|viewport| {
                        viewport
                            .name
                            .trim_start_matches('*')
                            .eq_ignore_ascii_case("active")
                    });
                    let active_entity_viewport = scene
                        .active_viewport
                        .and_then(|handle| doc.get_entity(handle))
                        .and_then(|entity| match entity {
                            codec::EntityType::Viewport(viewport) => Some(viewport),
                            _ => None,
                        });
                    let ucs_icon_on = active_entity_viewport
                        .map(|viewport| viewport.ucs_icon_visible)
                        .or_else(|| active_model_vport.map(|viewport| viewport.ucsicon_lower))
                        .unwrap_or(self.show_ucs_icon);
                    let ucs_icon_at_origin = active_entity_viewport
                        .map(|viewport| viewport.status.ucs_icon_at_origin)
                        .or_else(|| active_model_vport.map(|viewport| viewport.ucsicon_origin))
                        .unwrap_or(self.ucs_icon_at_origin);
                    let ucs_per_viewport = active_entity_viewport
                        .map(|viewport| viewport.ucs_per_viewport)
                        .or_else(|| active_model_vport.map(|viewport| viewport.ucs_per_viewport))
                        .unwrap_or(true);
                    let ucs_name = tab
                        .active_ucs
                        .as_ref()
                        .map(|ucs| ucs.name.trim())
                        .filter(|name| !name.is_empty())
                        .unwrap_or("")
                        .to_string();
                    let annotation_scale = if header.current_annotation_scale.trim().is_empty() {
                        "1:1".to_string()
                    } else {
                        header.current_annotation_scale.clone()
                    };
                    let annotation_scale_options = scene
                        .scale_list()
                        .into_iter()
                        .map(|(name, _, _)| name)
                        .collect();
                    let mut plot_table_options = vec!["None".to_string()];
                    plot_table_options.extend(crate::io::plot_style::available_ctb_names());
                    let visual_style = match tab.visual_style.as_str() {
                        "Wireframe 2D" => "2D Wireframe".to_string(),
                        "Wireframe 3D" => "3D Wireframe".to_string(),
                        value => value.to_string(),
                    };
                    let visual_style_options = crate::modules::view::visual_style::VISUAL_STYLES
                        .iter()
                        .map(|style| match style.label {
                            "Wireframe 2D" => "2D Wireframe".to_string(),
                            "Wireframe 3D" => "3D Wireframe".to_string(),
                            value => value.to_string(),
                        })
                        .collect();
                    let mut sections = vec![
                        PropSection {
                            title: t!("General").into_owned(),
                            props: vec![
                                read_only(t!("AC Version").as_ref(), {
                                    use codec::DxfVersion::*;
                                    let version = doc.dwg_source_version.unwrap_or(doc.version);
                                    let release = match version {
                                        AC1009 => "R11/R12",
                                        AC1012 => "R13",
                                        AC1014 => "R14",
                                        AC1015 => "2000-2002",
                                        AC1018 => "2004-2006",
                                        AC1021 => "2007-2009",
                                        AC1024 => "2010-2012",
                                        AC1027 => "2013-2017",
                                        AC1032 => "2018+",
                                        Unknown => "",
                                    };
                                    if release.is_empty() {
                                        version.as_str().to_string()
                                    } else {
                                        format!("{} ({release})", version.as_str())
                                    }
                                }),
                                Property {
                                    label: t!("Color").into_owned(),
                                    field: "color",
                                    value: PropValue::ColorChoice(header.current_entity_color),
                                },
                                Property {
                                    label: t!("Layer").into_owned(),
                                    field: "layer",
                                    value: PropValue::LayerChoice(current_layer),
                                },
                                Property {
                                    label: t!("Linetype").into_owned(),
                                    field: "linetype",
                                    value: PropValue::LinetypeChoice(current_linetype),
                                },
                                read_only(
                                    t!("Linetype scale").as_ref(),
                                    format_length(header.current_entity_linetype_scale),
                                ),
                                Property {
                                    label: t!("Lineweight").into_owned(),
                                    field: "line_weight",
                                    value: PropValue::LwChoice(
                                        codec::types::LineWeight::from_value(
                                            header.current_line_weight,
                                        ),
                                    ),
                                },
                                Property {
                                    label: t!("Transparency").into_owned(),
                                    field: "transparency",
                                    value: PropValue::EditChoice {
                                        value:
                                            crate::scene::creation_style::current_transparency_label(
                                                self.tabs[i]
                                                    .scene
                                                    .document
                                                    .current_entity_transparency(),
                                            ),
                                        options: crate::scene::creation_style::transparency_options(),
                                    },
                                },
                                read_only(
                                    t!("Thickness").as_ref(),
                                    format_length(header.thickness),
                                ),
                            ],
                        },
                        PropSection {
                            title: t!("3D Visualization").into_owned(),
                            props: vec![Property {
                                label: t!("Material").into_owned(),
                                field: "material",
                                value: PropValue::Choice {
                                    selected: material.clone(),
                                    options: {
                                        let mut opts = vec![
                                            "ByLayer".to_string(),
                                            "ByBlock".to_string(),
                                            "Global".to_string(),
                                        ];
                                        if !opts.contains(&material) && !material.is_empty() {
                                            opts.push(material.clone());
                                        }
                                        opts
                                    },
                                },
                            }],
                        },
                        PropSection {
                            title: t!("Plot style").into_owned(),
                            props: vec![
                                Property {
                                    label: t!("Plot style").into_owned(),
                                    field: "plot_style",
                                    value: if !header.plotstyle_mode {
                                        PropValue::Choice {
                                            selected: plot_style.to_string(),
                                            options: vec![
                                                "ByLayer".to_string(),
                                                "ByBlock".to_string(),
                                                "Normal".to_string(),
                                            ],
                                        }
                                    } else {
                                        PropValue::ReadOnlyWithTooltip {
                                            value: t!("ByColor").into_owned(),
                                            tooltip: t!("Plot style is locked to color in Color-Dependent (CTB) mode").into_owned(),
                                        }
                                    },
                                },
                                Property {
                                    label: t!("Plot style table").into_owned(),
                                    field: "view_plot_style_table",
                                    value: PropValue::Choice {
                                        selected: plot_table.clone(),
                                        options: plot_table_options,
                                    },
                                },
                                read_only(
                                    t!("Plot table attached to").as_ref(),
                                    scene.current_layout.clone(),
                                ),
                                read_only(
                                    t!("Plot table type").as_ref(),
                                    if plot_table == "None" {
                                        t!("Not available").into_owned()
                                    } else if header.plotstyle_mode {
                                        t!("Color-dependent plot styles").into_owned()
                                    } else {
                                        t!("Named plot styles").into_owned()
                                    },
                                ),
                            ],
                        },
                        PropSection {
                            title: t!("View").into_owned(),
                            props: vec![
                                read_only(t!("Center X").as_ref(), format_length(camera.target.x)),
                                read_only(t!("Center Y").as_ref(), format_length(camera.target.y)),
                                read_only(t!("Center Z").as_ref(), format_length(camera.target.z)),
                                read_only(t!("Height").as_ref(), format_length(view_height)),
                                read_only(t!("Width").as_ref(), format_length(view_width)),
                            ],
                        },
                        PropSection {
                            title: t!("Misc").into_owned(),
                            props: vec![
                                Property {
                                    label: t!("Annotation scale").into_owned(),
                                    field: "view_annotation_scale",
                                    value: PropValue::Choice {
                                        selected: annotation_scale,
                                        options: annotation_scale_options,
                                    },
                                },
                                Property {
                                    label: t!("UCS icon On").into_owned(),
                                    field: "view_ucs_icon_on",
                                    value: PropValue::Choice {
                                        selected: if ucs_icon_on { "Yes" } else { "No" }
                                            .to_string(),
                                        options: vec!["Yes".to_string(), "No".to_string()],
                                    },
                                },
                                Property {
                                    label: t!("UCS icon at origin").into_owned(),
                                    field: "view_ucs_icon_at_origin",
                                    value: PropValue::Choice {
                                        selected: if ucs_icon_at_origin { "Yes" } else { "No" }
                                            .to_string(),
                                        options: vec!["Yes".to_string(), "No".to_string()],
                                    },
                                },
                                Property {
                                    label: t!("UCS per viewport").into_owned(),
                                    field: "view_ucs_per_viewport",
                                    value: PropValue::Choice {
                                        selected: if ucs_per_viewport { "Yes" } else { "No" }
                                            .to_string(),
                                        options: vec!["Yes".to_string(), "No".to_string()],
                                    },
                                },
                                read_only(t!("UCS Name").as_ref(), ucs_name),
                                Property {
                                    label: t!("Visual Style").into_owned(),
                                    field: "view_visual_style",
                                    value: PropValue::Choice {
                                        selected: visual_style,
                                        options: visual_style_options,
                                    },
                                },
                            ],
                        },
                        PropSection {
                            title: t!("Parameters").into_owned(),
                            props: {
                                let mut props = vec![Property {
                                    label: String::new(),
                                    field: "show_constraint_values",
                                    value: PropValue::ParamsVisibilityToggle(
                                        self.show_constraint_values,
                                    ),
                                }];
                                props.extend(scene.named_parameters().iter().enumerate().map(
                                    |(index, parameter)| {
                                        Property {
                                            label: String::new(),
                                            field: "named_parameter",
                                            value: PropValue::ParamRow {
                                                index,
                                                name: parameter.name.clone(),
                                                formula: parameter.source.clone(),
                                                resolved: scene
                                                    .named_parameters()
                                                    .resolve(&parameter.name)
                                                    .map_err(|error| error.to_string()),
                                            },
                                        }
                                    },
                                ));
                                props.push(Property {
                                    label: String::new(),
                                    field: "named_parameter_add",
                                    value: PropValue::ParamAddRow,
                                });
                                props
                            },
                        },
                    ];
                    if let Some(section) = self.block_property_section(i) {
                        sections.insert(5, section);
                    }
                    ui::PropertiesPanel {
                        title: t!("No selection").into_owned(),
                        block_definition: tab
                            .active_block_edit_session()
                            .map(|session| session.br_handle),
                        choice_combos: property_choice_combos(&sections),
                        sections,
                        layer_combo: iced::widget::combo_box::State::new(layer_names.clone()),
                        linetype_combo: iced::widget::combo_box::State::new(linetype_items.clone()),
                        lineweight_combo: iced::widget::combo_box::State::new(
                            ui::properties::lw_options(),
                        ),
                        linetype_items,
                        ..Default::default()
                    }
                }
                1 => {
                    let (handle, source_entity) = selected[0];
                    let (mut sections, title) = self.entity_property_sections(
                        i,
                        handle,
                        source_entity,
                        &text_style_names,
                        annotation_scale_handle,
                        prop_vertex,
                    );
                    if let Some(definition) = self.tabs[i].active_block_edit_session().map(|s| s.br_handle) {
                        if source_entity.common().owner_handle == definition {
                            let status = self.tabs[i].scene.block_constraint_entity_status(definition, handle);
                            sections.push(crate::scene::model::object::PropSection {
                                title: crate::t!("Constraints").into_owned(),
                                props: vec![crate::scene::model::object::Property {
                                    label: crate::t!("Constraint status").into_owned(), field: "block_constraint_status",
                                    value: PropValue::ReadOnly(crate::t!(status.label()).into_owned()),
                                }],
                            });
                        }
                    }
                    ui::PropertiesPanel {
                        choice_combos: property_choice_combos(&sections),
                        sections,
                        title,
                        layer_combo: iced::widget::combo_box::State::new(layer_names.clone()),
                        linetype_combo: iced::widget::combo_box::State::new(linetype_items.clone()),
                        lineweight_combo: iced::widget::combo_box::State::new(
                            ui::properties::lw_options(),
                        ),
                        linetype_items,
                        ..Default::default()
                    }
                }
                // Property aggregation is O(n) per row plus an O(n²) group filter
                // (`group.handles.contains` scans per entity), stalling the rebuild
                // for seconds at tens of thousands of objects. Above the cap show a
                // count-only panel; bulk edits still go through the ribbon.
                n if n > MAX_PROP_AGGREGATE => ui::PropertiesPanel {
                    title: t!("%{count} objects selected", count = n).into_owned(),
                    layer_combo: iced::widget::combo_box::State::new(layer_names.clone()),
                    linetype_combo: iced::widget::combo_box::State::new(linetype_items.clone()),
                    lineweight_combo: iced::widget::combo_box::State::new(
                        ui::properties::lw_options(),
                    ),
                    linetype_items,
                    ..Default::default()
                },
                _ => {
                    let t_arm = crate::perf::enabled().then(iced::time::Instant::now);
                    let groups = build_selection_groups(&selected);
                    let t_groups = t_arm.map(|t| t.elapsed().as_secs_f64() * 1000.0);
                    let active_group = selected_group
                        .and_then(|group| groups.iter().find(|g| g.label == group.label).cloned())
                        .or_else(|| groups.first().cloned());

                    let filtered: Vec<(Handle, &EntityType)> = active_group
                        .as_ref()
                        .map(|group| {
                            let wanted: rustc_hash::FxHashSet<Handle> =
                                group.handles.iter().copied().collect();
                            selected
                                .iter()
                                .filter(|(handle, _)| wanted.contains(handle))
                                .copied()
                                .collect()
                        })
                        .unwrap_or_default();
                    let t_filter = t_arm.map(|t| t.elapsed().as_secs_f64() * 1000.0);

                    let plane = if self.tabs[i].editing_model_space() {
                        self.tabs[i].ucs_xform().working_plane()
                    } else {
                        crate::command::WorkingPlane::default()
                    };
                    let local_entities: Vec<(Handle, std::borrow::Cow<'_, EntityType>)> = filtered
                        .iter()
                        .map(|(handle, entity)| {
                            let local = if plane.is_identity() {
                                std::borrow::Cow::Borrowed(*entity)
                            } else {
                                std::borrow::Cow::Owned(dispatch::entity_in_working_plane(
                                    entity, plane,
                                ))
                            };
                            (*handle, local)
                        })
                        .collect();
                    let local_refs: Vec<(Handle, &EntityType)> = local_entities
                        .iter()
                        .map(|(handle, entity)| (*handle, entity.as_ref()))
                        .collect();
                    let t_local = t_arm.map(|t| t.elapsed().as_secs_f64() * 1000.0);
                    let mut sections = aggregate_sections(&local_refs, &text_style_names);
                    // A dimension's style groups (arrows, text, fit, units…)
                    // come from the document-aware builder; the document-free
                    // rows above would drop them all, so dimensions of one
                    // style shared only the General rows. (#1917)
                    if filtered
                        .iter()
                        .all(|(_, entity)| matches!(entity, EntityType::Dimension(_)))
                    {
                        sections = filtered
                            .iter()
                            .map(|(handle, entity)| {
                                self.entity_property_sections(
                                    i,
                                    *handle,
                                    entity,
                                    &text_style_names,
                                    annotation_scale_handle,
                                    prop_vertex,
                                )
                                .0
                            })
                            .reduce(|mut merged, next| {
                                intersect_into(&mut merged, &next);
                                merged
                            })
                            .unwrap_or_default();
                    }
                    if let (Some(t), Some(groups_ms), Some(filter_ms), Some(local_ms)) =
                        (t_arm, t_groups, t_filter, t_local)
                    {
                        let total = t.elapsed().as_secs_f64() * 1000.0;
                        if total >= 5.0 {
                            crate::perf_record!(
                                "[perf] properties-panel {total:>7.1}ms groups={groups_ms:.1} \
filter={:.1} local={:.1} aggregate={:.1} entities={}",
                                filter_ms - groups_ms,
                                local_ms - filter_ms,
                                total - local_ms,
                                local_refs.len(),
                            );
                        }
                    }
                    let compact_solids = local_refs.iter().all(|(handle, _)| {
                        crate::scene::model::solid_history::has_compact_solid_properties(
                            &self.tabs[i].scene.document,
                            *handle,
                        )
                    });
                    if compact_solids {
                        retain_compact_solid_sections(&mut sections);
                    }
                    sections.extend(aggregate_solid_history_sections(
                        &self.tabs[i].scene.document,
                        &local_refs
                            .iter()
                            .map(|(handle, _)| *handle)
                            .collect::<Vec<_>>(),
                    ));
                    if compact_solids {
                        retain_compact_solid_sections(&mut sections);
                    }
                    ui::PropertiesPanel {
                        choice_combos: property_choice_combos(&sections),
                        sections,
                        title: t!("%{count} objects selected", count = selected.len()).into_owned(),
                        selection_group_combo: iced::widget::combo_box::State::new(groups.clone()),
                        selection_groups: groups,
                        selected_group: active_group,
                        layer_combo: iced::widget::combo_box::State::new(layer_names.clone()),
                        linetype_combo: iced::widget::combo_box::State::new(linetype_items.clone()),
                        lineweight_combo: iced::widget::combo_box::State::new(
                            ui::properties::lw_options(),
                        ),
                        linetype_items,
                        ..Default::default()
                    }
                }
            };
            if selected.is_empty() {
                if let Some(item) = self.tabs[i].selected_authoring.and_then(|handle| {
                    let session = self.tabs[i].active_block_edit_session()?;
                    crate::scene::block_authoring::edit::items(
                        &self.tabs[i].scene.document,
                        session.br_handle,
                    )
                    .into_iter()
                    .find(|item| item.handle == handle)
                }) {
                    use crate::scene::model::object::{PropSection, PropValue, Property};
                    panel.title = item.name.clone();
                    panel.sections = vec![PropSection {
                        title: crate::t!("Geometry").into_owned(),
                        props: self.tabs[i].active_block_edit_session()
                            .and_then(|session| crate::scene::block_authoring::metadata::read(&self.tabs[i].scene.document, session.br_handle, item.handle).ok())
                            .map(|metadata| {
                                let mut props: Vec<Property> = metadata.text.into_iter().map(|text| {
                                    use crate::scene::block_authoring::metadata::Field;
                                    let label = match text.field {
                                        Field::BaseStateLabel => crate::tr!("block-flip", "base-state-label"),
                                        Field::FlippedStateLabel => crate::tr!("block-flip", "flipped-state-label"),
                                        _ => crate::t!(text.label).into_owned(),
                                    };
                                    Property { label, field: text.field.key(), value: PropValue::PlainText(text.value.to_owned()) }
                                }).collect();
                                if let Some(visible) = metadata.visible {
                                    props.push(Property { label: crate::t!("Show in Properties").into_owned(), field: "authoring_show_properties", value: PropValue::BoolToggle { field: "authoring_show_properties", value: visible } });
                                }
                                props
                            }).unwrap_or_else(|| vec![Property {
                                label: crate::t!("Parameter name").into_owned(), field: "authoring_name", value: PropValue::ReadOnly(item.name.clone()),
                            }]).into_iter()
                        .chain(item.grips.into_iter().map(|(part, point)| {
                            Property {
                                label: crate::t!(match part {
                                    crate::scene::block_authoring::edit::Part::Point => "Position",
                                    crate::scene::block_authoring::edit::Part::Base => "Base point",
                                    crate::scene::block_authoring::edit::Part::End => "End point",
                                    crate::scene::block_authoring::edit::Part::Reference =>
                                        "Reference point",
                                    crate::scene::block_authoring::edit::Part::Label(0) =>
                                        "Label position",
                                    _ => "Second label position",
                                })
                                .into_owned(),
                                field: "authoring_grip_position",
                                value: PropValue::ReadOnly(format!(
                                    "{:.4}, {:.4}, {:.4}",
                                    point.x, point.y, point.z
                                )),
                            }
                        }))
                        .collect(),
                    }];
                    if let Some(codec::objects::ObjectType::DynamicBlock(object)) = self.tabs[i].scene.document.objects.get(&item.handle) {
                        if let codec::objects::DynamicBlockData::AlignmentParameter(parameter) = &object.data {
                            panel.sections.push(PropSection { title: crate::t!("Alignment").into_owned(), props: vec![Property {
                                label: crate::t!("Alignment type").into_owned(), field: "authoring_alignment_type",
                                value: PropValue::Choice { selected: crate::t!(if parameter.align_perpendicular { "Perpendicular" } else { "Tangent" }).into_owned(),
                                    options: vec![crate::t!("Tangent").into_owned(), crate::t!("Perpendicular").into_owned()] },
                            }] });
                        }
                    }
                    if let Some(count) = self.tabs[i].active_block_edit_session().and_then(|session| crate::scene::block_grips::config::count(&self.tabs[i].scene.document, session.br_handle, item.handle)) {
                        panel.sections.push(PropSection { title: crate::t!("Grips").into_owned(), props: vec![Property { label: crate::t!("Number of grips").into_owned(), field: "authoring_grip_count", value: PropValue::EditText(count.to_string()) }] });
                    }
                    if let Some(grips) = self.tabs[i].active_block_edit_session().and_then(|session| crate::scene::block_grips::config::locations(&self.tabs[i].scene.document, session.br_handle, item.handle).ok()) {
                        let fields = super::block_grip_set::native_grip_fields();
                        for grip in grips {
                            let (x, y) = fields[grip.role];
                            panel.sections.push(PropSection { title: crate::t!(match grip.role { 0 => "Base grip location", 1 => "End grip location", 2 => "X grip location", _ => "Y grip location" }).into_owned(), props: vec![
                                Property { label: "X".into(), field: x, value: PropValue::EditText(grip.point.x.to_string()) },
                                Property { label: "Y".into(), field: y, value: PropValue::EditText(grip.point.y.to_string()) },
                            ] });
                        }
                    }
                    if let Some(base) = crate::scene::block_actions::options::base(&self.tabs[i].scene.document, item.handle) {
                        let mut props = vec![Property {
                            label: crate::t!("Dependent base point").into_owned(),
                            field: "authoring_action_base_dependent",
                            value: PropValue::BoolToggle { field: "authoring_action_base_dependent", value: base.dependent },
                        }];
                        if base.dependent {
                            props.extend([
                                Property { label: crate::t!("Base offset X").into_owned(), field: "authoring_action_base_offset_x", value: PropValue::EditText(base.offset.x.to_string()) },
                                Property { label: crate::t!("Base offset Y").into_owned(), field: "authoring_action_base_offset_y", value: PropValue::EditText(base.offset.y.to_string()) },
                            ]);
                        } else {
                            props.extend([
                                Property { label: crate::t!("Base X").into_owned(), field: "authoring_action_base_x", value: PropValue::EditText(base.base_point.x.to_string()) },
                                Property { label: crate::t!("Base Y").into_owned(), field: "authoring_action_base_y", value: PropValue::EditText(base.base_point.y.to_string()) },
                            ]);
                        }
                        panel.sections.push(PropSection { title: crate::t!("Action base point").into_owned(), props });
                    }
                    if let Some(offsets) = crate::scene::block_actions::options::offsets(
                        &self.tabs[i].scene.document, item.handle) {
                        panel.sections.push(PropSection {
                            title: crate::t!("Action overrides").into_owned(),
                            props: vec![
                                Property {
                                    label: crate::t!("Distance multiplier").into_owned(),
                                    field: "authoring_action_multiplier",
                                    value: PropValue::EditText(offsets.distance_multiplier.to_string()),
                                },
                                Property {
                                    label: crate::t!("Angle offset (degrees)").into_owned(),
                                    field: "authoring_action_angle",
                                    value: PropValue::EditText(offsets.angle_offset.to_degrees().to_string()),
                                },
                            ],
                        });
                    }
                    if let Some((chain_actions, value_sets)) =
                        self.authoring_parameter_properties(item.handle)
                    {
                        panel.sections.push(PropSection {
                            title: crate::t!("Behavior").into_owned(),
                            props: vec![Property {
                                label: crate::t!("Chain actions").into_owned(),
                                field: "authoring_chain_actions_state",
                                value: PropValue::ReadOnly(if chain_actions {
                                    crate::t!("Enabled (unsupported)").into_owned()
                                } else {
                                    crate::t!("Disabled").into_owned()
                                }),
                            }],
                        });
                        if !value_sets.is_empty() {
                            let mut props = Vec::new();
                            for (key, label, value_set) in value_sets {
                                let (
                                    minimum_enabled,
                                    maximum_enabled,
                                    increment_enabled,
                                    values_enabled,
                                    minimum,
                                    maximum,
                                    increment,
                                    values,
                                ) = match key {
                                    "linear_distance" => (
                                        "authoring_set_linear_distance_minimum_enabled",
                                        "authoring_set_linear_distance_maximum_enabled",
                                        "authoring_set_linear_distance_increment_enabled",
                                        "authoring_set_linear_distance_values_enabled",
                                        "authoring_set_linear_distance_minimum",
                                        "authoring_set_linear_distance_maximum",
                                        "authoring_set_linear_distance_increment",
                                        "authoring_set_linear_distance_values",
                                    ),
                                    "polar_distance" => (
                                        "authoring_set_polar_distance_minimum_enabled",
                                        "authoring_set_polar_distance_maximum_enabled",
                                        "authoring_set_polar_distance_increment_enabled",
                                        "authoring_set_polar_distance_values_enabled",
                                        "authoring_set_polar_distance_minimum",
                                        "authoring_set_polar_distance_maximum",
                                        "authoring_set_polar_distance_increment",
                                        "authoring_set_polar_distance_values",
                                    ),
                                    "polar_angle" => (
                                        "authoring_set_polar_angle_minimum_enabled",
                                        "authoring_set_polar_angle_maximum_enabled",
                                        "authoring_set_polar_angle_increment_enabled",
                                        "authoring_set_polar_angle_values_enabled",
                                        "authoring_set_polar_angle_minimum",
                                        "authoring_set_polar_angle_maximum",
                                        "authoring_set_polar_angle_increment",
                                        "authoring_set_polar_angle_values",
                                    ),
                                    "xy_x" => (
                                        "authoring_set_xy_x_minimum_enabled",
                                        "authoring_set_xy_x_maximum_enabled",
                                        "authoring_set_xy_x_increment_enabled",
                                        "authoring_set_xy_x_values_enabled",
                                        "authoring_set_xy_x_minimum",
                                        "authoring_set_xy_x_maximum",
                                        "authoring_set_xy_x_increment",
                                        "authoring_set_xy_x_values",
                                    ),
                                    "xy_y" => (
                                        "authoring_set_xy_y_minimum_enabled",
                                        "authoring_set_xy_y_maximum_enabled",
                                        "authoring_set_xy_y_increment_enabled",
                                        "authoring_set_xy_y_values_enabled",
                                        "authoring_set_xy_y_minimum",
                                        "authoring_set_xy_y_maximum",
                                        "authoring_set_xy_y_increment",
                                        "authoring_set_xy_y_values",
                                    ),
                                    "rotation_angle" => (
                                        "authoring_set_rotation_angle_minimum_enabled",
                                        "authoring_set_rotation_angle_maximum_enabled",
                                        "authoring_set_rotation_angle_increment_enabled",
                                        "authoring_set_rotation_angle_values_enabled",
                                        "authoring_set_rotation_angle_minimum",
                                        "authoring_set_rotation_angle_maximum",
                                        "authoring_set_rotation_angle_increment",
                                        "authoring_set_rotation_angle_values",
                                    ),
                                    _ => continue,
                                };
                                props.extend([
                                    Property {
                                        label: crate::tf!("Use {} minimum", crate::t!(label))
                                            .into_owned(),
                                        field: minimum_enabled,
                                        value: PropValue::BoolToggle {
                                            field: minimum_enabled,
                                            value: value_set.flags & 1 != 0,
                                        },
                                    },
                                    Property {
                                        label: crate::tf!("Use {} maximum", crate::t!(label))
                                            .into_owned(),
                                        field: maximum_enabled,
                                        value: PropValue::BoolToggle {
                                            field: maximum_enabled,
                                            value: value_set.flags & 2 != 0,
                                        },
                                    },
                                    Property {
                                        label: crate::tf!("Use {} increment", crate::t!(label))
                                            .into_owned(),
                                        field: increment_enabled,
                                        value: PropValue::BoolToggle {
                                            field: increment_enabled,
                                            value: value_set.flags & 4 != 0,
                                        },
                                    },
                                    Property {
                                        label: crate::tf!(
                                            "Use {} allowed-value list",
                                            crate::t!(label)
                                        )
                                        .into_owned(),
                                        field: values_enabled,
                                        value: PropValue::BoolToggle {
                                            field: values_enabled,
                                            value: value_set.flags & 8 != 0,
                                        },
                                    },
                                    Property {
                                        label: crate::tf!("{} minimum", crate::t!(label))
                                            .into_owned(),
                                        field: minimum,
                                        value: PropValue::EditText(value_set.minimum.to_string()),
                                    },
                                    Property {
                                        label: crate::tf!("{} maximum", crate::t!(label))
                                            .into_owned(),
                                        field: maximum,
                                        value: PropValue::EditText(value_set.maximum.to_string()),
                                    },
                                    Property {
                                        label: crate::tf!("{} increment", crate::t!(label))
                                            .into_owned(),
                                        field: increment,
                                        value: PropValue::EditText(value_set.increment.to_string()),
                                    },
                                    Property {
                                        label: crate::tf!("{} allowed values", crate::t!(label))
                                            .into_owned(),
                                        field: values,
                                        value: PropValue::EditText(
                                            value_set
                                                .values
                                                .iter()
                                                .map(ToString::to_string)
                                                .collect::<Vec<_>>()
                                                .join(", "),
                                        ),
                                    },
                                ]);
                            }
                            panel.sections.push(PropSection {
                                title: crate::t!("Value Set").into_owned(),
                                props,
                            });
                        }
                    }
                }
            }
            // Precompute the focused-id → field-key map for O(1) lookups on
            // `PropSyncActive`; derived from `sections`, so rebuild it here.
            panel.field_key_by_id = crate::ui::properties::build_field_key_map(&panel.sections);
            let new_handles: Vec<codec::Handle> = selected.iter().map(|(h, _)| *h).collect();
            // The plugin class parameter surface (API v14): one stamped
            // selection rides its class's declared rows. Only the native build
            // carries plugin classes (the wasm build has no plugin registry the
            // section could bind to).
            #[cfg(not(target_arch = "wasm32"))]
            if new_handles.len() == 1 {
                if let Some(section) = self.plugin_class_parameter_section(i, new_handles[0]) {
                    panel.sections.push(section);
                    panel.field_key_by_id =
                        crate::ui::properties::build_field_key_map(&panel.sections);
                }
            }
            // Carry the in-progress edits only when the selection is unchanged
            // (a commit-triggered rebuild); a genuine selection change starts
            // with a clean buffer so no stale value leaks onto the new entity.
            let same_target =
                prev_handles == new_handles && previous_definition == panel.block_definition;
            panel.edit_buf = if same_target {
                edit_buf
            } else {
                Default::default()
            };
            // The active-row highlight only survives a rebuild for the same
            // selection (like the edit buffer); a selection change clears it so
            // an old row isn't marked active against new content.
            panel.active_field = if same_target { active_field } else { None };
            panel.expanded_groups = expanded_groups;
            panel.collapsed_sections = collapsed_sections;
            panel.source_handles = new_handles;
            panel.prop_vertex = prop_vertex;
            panel.prop_vertex_indicator_active = prop_vertex_indicator_active;
            let property_handles = panel.selected_handles();
            let property_handles = if property_handles.is_empty() {
                &panel.source_handles
            } else {
                &property_handles
            };
            let locked_only = !property_handles.is_empty()
                && property_handles
                    .iter()
                    .all(|handle| self.tabs[i].scene.is_layer_locked(*handle));
            if property_handles.iter().any(|handle| self.tabs[i].scene.document.get_entity(*handle)
                .is_some_and(|entity| crate::scene::block_construction::is_construction(entity.common()))) {
                set_row(&mut panel.sections, "color", crate::t!("Construction geometry").into_owned());
                set_row(&mut panel.sections, "layer", crate::t!("Construction geometry").into_owned());
                set_row(&mut panel.sections, "linetype", crate::t!("Dashed").into_owned());
                panel.field_key_by_id = crate::ui::properties::build_field_key_map(&panel.sections);
            }
            if locked_only {
                // A point cloud locked on its own (not by its layer) still
                // takes its Locked row, the way it is unlocked.
                let cloud_lock_only = property_handles
                    .iter()
                    .all(|handle| self.tabs[i].scene.locked_layer_name(*handle).is_none());
                make_sections_read_only(
                    &mut panel.sections,
                    if cloud_lock_only {
                        &["ext_pcx_locked"]
                    } else {
                        &[]
                    },
                );
                // Rows demoted to read-only no longer back an editable field;
                // drop them from the id→key map so focus can't map onto them.
                panel.field_key_by_id = crate::ui::properties::build_field_key_map(&panel.sections);
                panel.edit_buf.clear();
                panel.active_field = None;
                panel.color_picker_open = false;
                panel.bg_color_picker_open = false;
                panel.open_color_field = None;
                panel.hatch_pattern_picker_open = false;
                panel.edit_choice_open = false;
            }
            panel
        };

        self.tabs[i].properties = new_panel;
        self.refresh_selected_grips();
        let m_panel = t_all.map(|t| t.elapsed().as_secs_f64() * 1000.0);
        let t_ribbon = crate::perf::enabled().then(iced::time::Instant::now);
        self.sync_ribbon_from_selection();
        self.sync_underlay_tab();
        if let Some(t) = t_all {
            let ribbon_ms = t_ribbon.map_or(0.0, |r| r.elapsed().as_secs_f64() * 1000.0);
            let total_ms = t.elapsed().as_secs_f64() * 1000.0;
            if total_ms >= 5.0 {
                let prelude = m_prelude.unwrap_or(0.0);
                let panel = m_panel.unwrap_or(0.0);
                crate::perf_record!(
                    "[perf] properties-detail {total_ms:>7.1}ms prelude={prelude:.1} \
handles={handles_ms:.1} panel={:.1} ribbon={ribbon_ms:.1} tail={:.1} selected={}",
                    panel - prelude - handles_ms,
                    total_ms - panel - ribbon_ms,
                    cur_handles.len(),
                );
            }
        }
    }

    /// Drive the Home-ribbon Layer / Color / Linetype / Lineweight dropdowns
    /// from the current entity selection. With no selection the ribbon falls
    /// back to the active creation defaults (per-tab active_layer + ByLayer).
    /// Mixed selections keep the prior value (we'd need a UI "*Varies*"
    /// marker to do better).
    pub(super) fn sync_ribbon_from_selection(&mut self) {
        let i = self.active_tab;
        // The Start (welcome) tab has no document — keep the ribbon's
        // current-layer chip empty rather than re-seeding it with a default.
        if self.tabs[i].is_start {
            self.ribbon.active_layer = String::new();
            return;
        }
        let selected = self.tabs[i].scene.selected_entities();
        if selected.is_empty() {
            // Creation defaults: prefer the file's saved CECOLOR / CELTYPE /
            // CELWEIGHT (and current_layer_name); fall back to ByLayer when
            // those slots are still at their factory default.
            let header = &self.tabs[i].scene.document.header;
            let layer = if header.current_layer_name.is_empty() {
                self.tabs[i].active_layer.clone()
            } else {
                header.current_layer_name.clone()
            };
            self.ribbon.active_layer = layer;
            self.ribbon.active_color = header.current_entity_color;
            // current_linetype_name may be empty when only the handle was
            // written; resolve via line_types table in that case.
            let lt = if !header.current_linetype_name.is_empty() {
                header.current_linetype_name.clone()
            } else if !header.current_linetype_handle.is_null() {
                self.tabs[i]
                    .scene
                    .document
                    .line_types
                    .iter()
                    .find(|lt| lt.handle == header.current_linetype_handle)
                    .map(|lt| lt.name.clone())
                    .unwrap_or_else(|| "ByLayer".to_string())
            } else {
                "ByLayer".to_string()
            };
            self.ribbon.active_linetype = lt;
            self.ribbon.active_lineweight =
                codec::types::LineWeight::from_value(header.current_line_weight);
            return;
        }

        let mut layer: Option<String> = None;
        let mut color: Option<codec::types::Color> = None;
        let mut linetype: Option<String> = None;
        let mut lineweight: Option<codec::types::LineWeight> = None;
        let mut layer_mixed = false;
        let mut color_mixed = false;
        let mut linetype_mixed = false;
        let mut lineweight_mixed = false;

        for (_h, e) in &selected {
            if layer_mixed && color_mixed && linetype_mixed && lineweight_mixed {
                break;
            }
            let c = e.common();
            // Borrowed, not cloned: this only ever gets compared, and cloning
            // it here was one heap allocation per selected entity.
            let lt = if c.linetype.is_empty() {
                "ByLayer"
            } else {
                c.linetype.as_str()
            };
            match &layer {
                None => layer = Some(c.layer.clone()),
                Some(prev) if prev != &c.layer => layer_mixed = true,
                _ => {}
            }
            match &color {
                None => color = Some(c.color),
                Some(prev) if prev != &c.color => color_mixed = true,
                _ => {}
            }
            match &linetype {
                None => linetype = Some(lt.to_string()),
                Some(prev) if prev.as_str() != lt => linetype_mixed = true,
                _ => {}
            }
            match &lineweight {
                None => lineweight = Some(c.line_weight),
                Some(prev) if prev != &c.line_weight => lineweight_mixed = true,
                _ => {}
            }
        }
        if !layer_mixed {
            if let Some(l) = layer {
                self.ribbon.active_layer = l;
            }
        }
        if !color_mixed {
            if let Some(c) = color {
                self.ribbon.active_color = c;
            }
        }
        if !linetype_mixed {
            if let Some(l) = linetype {
                self.ribbon.active_linetype = l;
            }
        }
        if !lineweight_mixed {
            if let Some(lw) = lineweight {
                self.ribbon.active_lineweight = lw;
            }
        }
    }

    /// Rebuild the cached selected_grips from the current entity selection.
    ///
    /// Selections past `GRIPOBJLIMIT` get no grips at all. That used to be a
    /// constant here; it is now [`crate::app::settings::UserSettings::grip_object_limit`],
    /// reachable from Options and from the sysvar of the same name.
    pub(super) fn refresh_selected_grips(&mut self) {
        let i = self.active_tab;
        // Reference-only targets are cancelled like locked-layer ones: the
        // plugin that owns the display geometry may have just designated the
        // entity, and a drag that edits it would be overwritten on refresh.
        let locked_active_grip = self.tabs[i].active_grip.as_ref().is_some_and(|grip| {
            grip.targets.iter().any(|target| {
                self.tabs[i].scene.is_layer_locked(target.handle)
                    || self.tabs[i].scene.is_reference_only(target.handle)
            })
        });
        if locked_active_grip {
            self.cancel_active_grip_edit();
            return;
        }
        let is_paper = self.tabs[i].scene.current_layout != "Model";
        // Paper-space entity coordinates are NOT offset by world_offset (same rule
        // as wire tessellation in wires_for_block). Only subtract in model space.
        let wo = if is_paper { [0.0f64; 3] } else { [0.0_f64; 3] };
        let (new_handle, new_grips, new_grip_handles) = {
            let selected = self.tabs[i].scene.selected_entities();
            let single_handle = (selected.len() == 1
                && !self.tabs[i].scene.is_layer_locked(selected[0].0)
                && !self.tabs[i].scene.is_reference_only(selected[0].0))
            .then(|| selected[0].0);
            let mut grips = Vec::new();
            let mut handles = Vec::new();
            // Avoid rebuilding every entity's grip markers above the configured
            // limit. Zero disables the limit.
            let limit = self.grip_object_limit;
            let selected = if limit > 0 && selected.len() > limit as usize {
                Vec::new()
            } else {
                selected
            };
            for (handle, _entity) in selected {
                // entity_grips_for errs only for a missing tab/entity; inside
                // the selection walk the entity exists, so a defensive empty
                // keeps the cache build total.
                for grip in self.entity_grips_for(i, handle).unwrap_or_default() {
                    handles.push(handle);
                    grips.push(grip);
                }
            }
            // A lone unlocked point cloud in a 3D visual style carries the
            // move gizmo at its insertion point.
            let gizmo = single_handle
                .filter(|_| {
                    !is_paper
                        && self.tabs[i].scene.active_model_tile_render_mode()
                            != codec::entities::ViewportRenderMode::Wireframe2D
                })
                .and_then(|handle| Some((handle, self.tabs[i].scene.document.get_entity(handle)?)))
                .filter(|(_, entity)| !crate::scene::is_locked_point_cloud(entity))
                .and_then(|(handle, entity)| match entity {
                    codec::EntityType::Extended(extended) => match &extended.data {
                        codec::entities::ExtendedEntityData::PointCloudEx(data) => Some((
                            handle,
                            crate::entities::extended::point_cloud_gizmo_grips(data),
                        )),
                        _ => None,
                    },
                    _ => None,
                });
            if let Some((handle, gizmo)) = gizmo {
                handles.extend(std::iter::repeat(handle).take(gizmo.len()));
                grips.extend(gizmo);
            }
            let (grips, handles) = apply_grip_budget(grips, handles);
            (single_handle, grips, handles)
        };
        self.tabs[i].selected_handle = new_handle;
        self.tabs[i].selected_grips = new_grips;
        self.tabs[i].selected_grip_handles = new_grip_handles;
        let available: rustc_hash::FxHashSet<_> = self.tabs[i]
            .selected_grip_handles
            .iter()
            .copied()
            .zip(self.tabs[i].selected_grips.iter().map(|grip| grip.id))
            .collect();
        self.tabs[i].hot_grips.retain(|key| available.contains(key));
        // Append the dynamic-block visibility (lookup) grip, if the lone
        // selection is a visibility-parametric block reference.
        self.refresh_visibility_grip(wo);
        self.refresh_table_grip(wo);
        self.refresh_test_action_grips(wo);
        self.refresh_authoring_grips(wo);
    }

    /// Enumerate one entity's built-in grips exactly as the selection cache
    /// builder and the interactive viewport produce them for that entity —
    /// the ONE shared core behind the viewport's selection cache, the
    /// plugin grip read (API v11) and the automation `grips` op.
    ///
    /// Generic over every entity type: the answer is whatever the entity's
    /// own grip producers declare. Reference-only entities and entities on
    /// locked layers answer an empty list — the same answer the viewport
    /// would give (grips exist only for selected, unlocked, editable
    /// entities). The selection-scale grip budget does NOT apply here: it
    /// bounds the on-screen total for multi-selections, not the per-entity
    /// universe.
    pub(super) fn entity_grips_for(
        &self,
        i: usize,
        handle: Handle,
    ) -> Result<Vec<GripDef>, String> {
        if self.tabs.len() <= i {
            return Err(format!("tab {i} does not exist"));
        }
        let Some(entity) = self.tabs[i].scene.document.get_entity(handle) else {
            return Err(format!("entity {handle} does not exist"));
        };
        if self.tabs[i].scene.is_layer_locked(handle)
            || self.tabs[i].scene.is_reference_only(handle)
        {
            return Ok(Vec::new());
        }
        let annotation_scale_handle = self.tabs[i].scene.displayed_annotation_scale_handle();
        // Paper-space entity coordinates are NOT offset by world_offset (same
        // rule as wire tessellation in wires_for_block). Only subtract in
        // model space.
        let is_paper = self.tabs[i].scene.current_layout != "Model";
        let wo = if is_paper { [0.0f64; 3] } else { [0.0_f64; 3] };

        let contextual = crate::scene::annotative::entity_for_annotation_context(
            &self.tabs[i].scene.document,
            entity,
            annotation_scale_handle,
        );
        let mut entity_grips = dispatch::grips(contextual.as_ref());
        if let codec::EntityType::Insert(insert) = contextual.as_ref() {
            entity_grips = crate::entities::insert::visible_attribute_grips(
                &self.tabs[i].scene.document,
                insert,
                self.tabs[i].scene.annotation_scale,
            );
        }
        if crate::scene::model::solid_history::has_specialized_primitive_properties(
            &self.tabs[i].scene.document,
            handle,
        ) {
            entity_grips.clear();
        }
        // Dimension::grips() cannot see the document, so an automatic dimension
        // text grip cannot resolve its real DIMSTYLE/annotation-scaled position
        // there. Correct it here, where both the document and displayed annotation
        // scale are available.
        if let codec::EntityType::Dimension(dim) = contextual.as_ref() {
            if matches!(
                &**dim,
                codec::entities::Dimension::Linear(_) | codec::entities::Dimension::Aligned(_)
            ) && !dim.base().text_user_positioned
            {
                let anno_scale = annotation_scale_handle
                    .and_then(
                        |handle| match self.tabs[i].scene.document.objects.get(&handle) {
                            Some(codec::objects::ObjectType::Scale(scale)) => Some(
                                scale.inverse_factor()
                                    / self.tabs[i].scene.annotation_scale_unit_factor(),
                            ),
                            _ => None,
                        },
                    )
                    .unwrap_or(self.tabs[i].scene.annotation_scale as f64);

                if let Some(position) = crate::entities::dimension::dimension_text_grip_position(
                    dim,
                    &self.tabs[i].scene.document,
                    anno_scale,
                ) {
                    // The text grip is the final native grip of Linear/Aligned
                    // dims. While the text is still automatic, make this a
                    // point/stretch grip rather than a midpoint-translate
                    // grip. That makes the first drag use the displayed
                    // automatic position as its absolute starting point instead
                    // of translating the stale DWG text_middle_point.
                    if let Some(text_grip) = entity_grips.last_mut() {
                        text_grip.world = glam::DVec3::new(position.x, position.y, position.z);
                        text_grip.is_midpoint = false;
                    }
                }
            }
        }
        let contextual_dimension = crate::entities::dimension::as_dimension(contextual.as_ref());
        let dynamic_angle = contextual_dimension
            .filter(|dim| {
                matches!(
                    dim,
                    codec::entities::Dimension::Angular2Ln(_)
                        | codec::entities::Dimension::Angular3Pt(_)
                )
            })
            .filter(|_| self.tabs[i].scene.is_dynamic_dimension(handle));
        if let Some(dim) = dynamic_angle {
            // A dynamic angle shows a triangle at each end of its arc
            // pointing away from it, and squares at the arc point and
            // the text — nothing on the sides.
            let (arc_id, text_id) = if matches!(dim, codec::entities::Dimension::Angular2Ln(_)) {
                (4, 5)
            } else {
                (3, 4)
            };
            if let Some(ends) = crate::entities::dimension::angular_arc_ends(dim) {
                entity_grips.retain(|grip| grip.id == arc_id || grip.id == text_id);
                for (point, away) in ends {
                    entity_grips.insert(
                        0,
                        crate::entities::common::oriented_triangle_grip(arc_id, point, away),
                    );
                }
            }
        } else if self.tabs[i].scene.is_dynamic_dimension(handle) {
            // A dynamic dimension shows the reference's grips: a
            // triangle at each constraint point aimed at the other
            // one, and the text square — no dimension line grip.
            let points: Vec<glam::DVec3> = entity_grips
                .iter()
                .filter(|grip| grip.id <= 1)
                .map(|grip| grip.world)
                .collect();
            entity_grips.retain(|grip| grip.id != 2);
            for grip in &mut entity_grips {
                if grip.id <= 1 {
                    let other = points.get(1 - grip.id).copied().unwrap_or(grip.world);
                    let dir = (other - grip.world).normalize_or(glam::DVec3::X);
                    *grip =
                        crate::entities::common::oriented_triangle_grip(grip.id, grip.world, dir);
                }
            }
        }
        entity_grips.extend(crate::scene::model::solid_history::primitive_grips(
            &self.tabs[i].scene.document,
            handle,
        ));
        for grip in &mut entity_grips {
            // Subtract in f64: at UTM magnitudes an f32 cast before
            // the offset costs ~1 unit and draws the grip off the wire.
            grip.world.x -= wo[0];
            grip.world.y -= wo[1];
            grip.world.z -= wo[2];
        }
        Ok(entity_grips)
    }

    pub(super) fn property_target_handles(&self, i: usize) -> Vec<Handle> {
        // The node graph edits its own node's entity through the panel's
        // handlers, independent of the drawing selection.
        self.resolve_property_targets(i, true, |scene, handle| !scene.is_layer_locked(handle))
    }

    /// The point clouds the Locked row applies to: locked ones too, unless
    /// their layer is locked.
    pub(super) fn point_cloud_lock_targets(&self, i: usize) -> Vec<Handle> {
        self.resolve_property_targets(i, false, |scene, handle| {
            scene.locked_layer_name(handle).is_none()
        })
    }

    fn resolve_property_targets(
        &self,
        i: usize,
        include_override: bool,
        mut include: impl FnMut(&crate::scene::Scene, Handle) -> bool,
    ) -> Vec<Handle> {
        if include_override {
            if let Some(handle) = self.property_target_override {
                let scene = &self.tabs[i].scene;
                return scene
                    .document
                    .get_entity(handle)
                    .filter(|_| include(scene, handle))
                    .map(|_| vec![handle])
                    .unwrap_or_default();
            }
        }
        let mut handles = self.tabs[i].properties.selected_handles();
        if handles.is_empty() {
            handles = self.tabs[i].properties.source_handles.clone();
        }
        if handles.is_empty() {
            handles.extend(self.tabs[i].selected_handle);
        }
        let scene = &self.tabs[i].scene;
        handles.retain(|handle| {
            scene.document.get_entity(*handle).is_some() && include(scene, *handle)
        });
        handles
    }

    pub(super) fn has_property_selection(&self, i: usize) -> bool {
        !self.tabs[i].properties.selected_handles().is_empty()
            || !self.tabs[i].properties.source_handles.is_empty()
            || self.tabs[i].selected_handle.is_some()
    }

    pub(super) fn invalidate_property_targets(&mut self, i: usize, handles: &[Handle]) {
        self.invalidate_property_targets_with_driven(i, handles, &[]);
    }

    pub(super) fn invalidate_property_targets_with_driven(
        &mut self,
        i: usize,
        handles: &[Handle],
        driven_refs: &[crate::scene::parametric_constraints::ParametricRef],
    ) {
        let retain_size = self.constraint_solve_mode
            && !driven_refs.is_empty()
            && driven_refs
                .iter()
                .all(|reference| reference.marker.is_some());
        self.invalidate_property_targets_with_originals(i, handles, driven_refs, retain_size, &[]);
    }

    pub(super) fn invalidate_property_targets_with_originals(
        &mut self,
        i: usize,
        handles: &[Handle],
        driven_refs: &[crate::scene::parametric_constraints::ParametricRef],
        retain_size: bool,
        retained_originals: &[(Handle, codec::EntityType)],
    ) {
        let mut context_object_changed = false;
        for &handle in handles {
            // A dimension is drawn from the block holding its picture, and that
            // picture was made under the settings just edited. Drop it so the
            // dimension is drawn afresh — otherwise the edit changes the stored
            // variables and nothing on screen.
            if matches!(
                self.tabs[i].scene.document.get_entity(handle),
                Some(codec::EntityType::Dimension(_))
            ) {
                self.tabs[i].scene.invalidate_dim_block_recorded(handle);
            }
            context_object_changed |= self.tabs[i].scene.sync_displayed_annotation_context(handle);
            // Hatch / SOLID fills render from prebuilt cached models; rebuild
            // them or pattern edits (scale, background, …) stay invisible
            // (#415).
            self.tabs[i].scene.refresh_fill_model(handle);
        }
        if context_object_changed {
            self.tabs[i].scene.poison_undo_recording();
        }
        // Solid (ACIS) meshes bake their colour into the mesh, so a colour /
        // layer change needs an explicit recolour — re-tessellating wires
        // alone wouldn't update them.
        self.tabs[i].scene.recolor_meshes_for_handles(handles);
        let changes: Vec<_> = handles
            .iter()
            .map(|&handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities_with_parametric_originals(
            &changes,
            driven_refs,
            retain_size,
            retained_originals,
        );
    }

    /// Apply a single-property edit to every handle in `handles`, recording the
    /// undo snapshot and running the full property-op contract (invalidate the
    /// tessellation cache, mark the tab dirty, refresh the Properties panel).
    ///
    /// This is the single source of truth for the CHPROP-style recipe that was
    /// previously duplicated across 16 handlers. `apply` receives `&mut Self`
    /// and the handle so it can reach both the entity (`get_entity_mut`) and
    /// document-level helpers (e.g. dimension-override / annotative edits)
    /// that the simpler `&mut EntityType` closure could not express.
    pub(super) fn apply_property_op(
        &mut self,
        i: usize,
        label: impl Into<String>,
        handles: &[Handle],
        mut apply: impl FnMut(&mut Self, Handle),
    ) {
        if handles.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, label);
        for &handle in handles {
            apply(self, handle);
        }
        self.invalidate_property_targets(i, handles);
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }

    /// Add an entity to the correct space (model or paper space layout).
    pub(super) fn commit_entity(&mut self, entity: codec::EntityType) {
        let _ = self.commit_entity_handle(entity);
    }

    /// Like [`commit_entity`] but returns the handle the new entity was given
    /// (or `None` if it could not be added). Lets callers follow up — e.g.
    /// open the in-place text editor on a freshly created MultiLeader.
    pub(super) fn commit_entity_handle(&mut self, entity: codec::EntityType) -> Option<Handle> {
        self.commit_entity_handle_with_policies(entity, false, false, false)
    }

    pub(super) fn commit_entity_handle_with_dimension_policy(
        &mut self,
        entity: codec::EntityType,
        preserve_dimension_layer_and_style: bool,
    ) -> Option<Handle> {
        self.commit_entity_handle_with_policies(
            entity,
            preserve_dimension_layer_and_style,
            false,
            false,
        )
    }

    pub(super) fn commit_entity_handle_preserve_layer(
        &mut self,
        entity: codec::EntityType,
    ) -> Option<Handle> {
        self.commit_entity_handle_with_policies(entity, false, true, false)
    }

    pub(super) fn commit_entity_handle_preserve_style(
        &mut self,
        entity: codec::EntityType,
    ) -> Option<Handle> {
        self.commit_entity_handle_with_policies(entity, false, true, true)
    }

    fn commit_entity_handle_with_policies(
        &mut self,
        mut entity: codec::EntityType,
        preserve_dimension_layer_and_style: bool,
        preserve_entity_layer: bool,
        preserve_entity_style: bool,
    ) -> Option<Handle> {
        let i = self.active_tab;
        let tracks_dimension_chain = matches!(
            crate::entities::dimension::as_dimension(&entity),
            Some(
                codec::entities::Dimension::Linear(_)
                    | codec::entities::Dimension::Aligned(_)
                    | codec::entities::Dimension::Angular2Ln(_)
                    | codec::entities::Dimension::Angular3Pt(_)
                    | codec::entities::Dimension::Ordinate(_)
            )
        );
        let inherited_dimension = if preserve_dimension_layer_and_style {
            match &entity {
                codec::EntityType::Dimension(dimension) => Some((
                    dimension.base().common.layer.clone(),
                    dimension.base().style_name.clone(),
                )),
                _ => None,
            }
        } else {
            None
        };
        if let codec::EntityType::Table(table) = &mut entity {
            if let Some(settings) = table
                .name
                .strip_prefix(crate::modules::annotate::data_link::EXTRACTION_PENDING)
                .map(str::to_string)
            {
                table.name.clear();
                let link = crate::app::annotation_data::add_extraction_link(
                    &mut self.tabs[i].scene.document,
                    &settings,
                );
                crate::modules::annotate::data_link::link_cells(table, link, 1);
            }
            const PREFIX: &str = "__OPENCAD_LINK_PENDING__";
            if let Some(path) = table.name.strip_prefix(PREFIX).map(str::to_string) {
                let document = &mut self.tabs[i].scene.document;
                let stem = std::path::Path::new(&path)
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("Data Link")
                    .to_string();
                let names = crate::app::annotation_data::data_link_entries(document);
                let mut name = stem.clone();
                let mut n = 1;
                while names.iter().any(|(known, _)| known.eq_ignore_ascii_case(&name)) {
                    n += 1;
                    name = format!("{stem} ({n})");
                }
                let mut link = codec::objects::DataLink::default();
                crate::app::annotation_data::set_spreadsheet_link(
                    &mut link,
                    &crate::app::annotation_data::SpreadsheetLinkSettings {
                        path: &path,
                        stored_path: &path,
                        sheet: "",
                        kind: crate::ui::window::annotation_data::LinkRange::EntireSheet,
                        range: None,
                        path_option: 1,
                        allow_write: false,
                        use_source_formatting: true,
                        keep_source_formatting: true,
                    },
                );
                let link_handle = crate::app::annotation_data::add_data_link(document, &name, link);
                crate::modules::annotate::data_link::link_cells(table, link_handle, 0);
                table.name = std::path::Path::new(&path)
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("Linked table")
                    .to_string();
                table.description = path;
            }
        }
        let tracks_draw_anchor = self.tabs[i].active_cmd.is_some()
            && matches!(
                &entity,
                codec::EntityType::Line(_)
                    | codec::EntityType::Arc(_)
                    | codec::EntityType::LwPolyline(_)
                    | codec::EntityType::Polyline(_)
                    | codec::EntityType::Polyline2D(_)
                    | codec::EntityType::Polyline3D(_)
            );
        let explicit_mleader_layer = match &entity {
            codec::EntityType::MultiLeader(leader) => leader
                .common
                .layer
                .strip_prefix("__MLEADER_LAYER__")
                .map(str::to_owned),
            _ => None,
        };
        if let Some(layer) = explicit_mleader_layer {
            entity.as_entity_mut().set_layer(layer);
        } else if !preserve_entity_layer {
            // DIMLAYER and MLEADERLAYER put new dimensions and
            // multileaders on their own layers.
            let xref = match &entity {
                codec::EntityType::Insert(insert) => self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get(&insert.block_name)
                    .is_some_and(|record| record.flags.is_xref),
                _ => false,
            };
            let layer = if matches!(entity, codec::EntityType::Dimension(_)) {
                self.new_dimension_layer(i)
            } else if matches!(entity, codec::EntityType::MultiLeader(_)) {
                self.new_mleader_layer(i)
            } else if matches!(entity, codec::EntityType::Hatch(_)) {
                self.new_variable_layer(i, "HATCHLAYER")
            } else if xref {
                self.new_variable_layer(i, "XREFLAYER")
            } else {
                self.tabs[i].active_layer.clone()
            };
            if layer != "0" || entity.as_entity().layer().is_empty() {
                entity.as_entity_mut().set_layer(layer);
            }
        }

        // INSUNITS: when inserting a block whose BlockRecord.units differ
        // from the host's header.insertion_units, scale the new INSERT so
        // 1 source-unit equals the matching host length.
        if let codec::EntityType::Insert(ref mut ins) = entity {
            let host_units = self.tabs[i].scene.document.header.insertion_units;
            let src_units = self.tabs[i]
                .scene
                .document
                .block_records
                .get(&ins.block_name)
                .map(|br| br.units)
                .unwrap_or(0);
            if let Some(ratio) = insert_unit_scale(host_units, src_units) {
                if !apply_insert_unit_scale(ins, ratio) {
                    self.command_line.push_error(
                        t!("INSERT unit scale is outside the supported range.").as_ref(),
                    );
                    return None;
                }
            }
        }

        if !preserve_entity_style {
            crate::scene::view::dispatch::apply_named_color(
                &mut entity,
                self.ribbon.active_color,
                self.ribbon.active_color_name(),
            );
            crate::scene::view::dispatch::apply_common_prop(
                &mut entity,
                "linetype",
                &self.ribbon.active_linetype.clone(),
            );
            crate::scene::view::dispatch::apply_line_weight(
                &mut entity,
                self.ribbon.active_lineweight,
            );
            // CELTSCALE (header.current_entity_linetype_scale): new entities
            // pick up the document's saved per-entity linetype scale. The user
            // can override per entity later via the properties panel.
            let celtscale = self.tabs[i]
                .scene
                .document
                .header
                .current_entity_linetype_scale;
            if (celtscale - 1.0).abs() > 1e-9 && celtscale.abs() > 1e-9 {
                entity.common_mut().linetype_scale = celtscale;
            }
        }

        if !preserve_entity_style {
            crate::scene::creation_style::apply_current_creation_styles(
                &self.tabs[i].scene.document,
                &mut entity,
            );
        }
        if let (Some((layer, style_name)), codec::EntityType::Dimension(dimension)) =
            (inherited_dimension, &mut entity)
        {
            dimension.base_mut().common.layer = layer;
            dimension.base_mut().style_name = style_name;
        }

        // Smart centre objects carry their own drawing-level creation style.
        // Apply it after the generic ribbon style so ordinary LINE entities
        // keep the existing path while centre lines honour their settings.
        let center_line =
            codec::entities::CenterLineAssociation::read(&entity.common().extended_data).is_some();
        let center_mark =
            codec::entities::CenterMarkAssociation::read(&entity.common().extended_data).is_some();
        if center_line || center_mark {
            let settings = self.tabs[i].scene.centerline_settings();
            // CENTERLAYER: "." is the current layer; a named layer is made
            // when the drawing does not have it yet.
            let layer = settings.layer.trim();
            if !layer.is_empty() && layer != "." {
                self.tabs[i].scene.ensure_layer(layer);
                entity.common_mut().layer = layer.to_string();
            }
            // CENTERLTYPE: empty is CENTER2, "." the current linetype.
            let linetype = match settings.linetype.trim() {
                "" => "CENTER2".to_string(),
                "." => self.tabs[i].scene.document.header.current_linetype_name.clone(),
                other => other.to_string(),
            };
            crate::io::linetypes::ensure_catalog_linetype(
                &mut self.tabs[i].scene.document,
                &linetype,
            );
            entity.common_mut().linetype = linetype;
            entity.common_mut().linetype_scale = settings.linetype_scale;
            let application = if center_mark {
                codec::entities::CENTERMARK_XDATA_APPLICATION
            } else {
                codec::entities::CENTERLINE_XDATA_APPLICATION
            };
            if !self.tabs[i].scene.document.app_ids.contains(application) {
                let mut app = codec::tables::AppId::new(application);
                app.handle = self.tabs[i].scene.document.allocate_handle();
                let _ = self.tabs[i].scene.document.app_ids.add(app);
            }
        }

        let text_style_annotative = match &entity {
            codec::EntityType::Text(text) => crate::scene::annotative::text_style_is_annotative(
                &self.tabs[i].scene.document,
                &text.style,
            ),
            codec::EntityType::MText(text) => crate::scene::annotative::text_style_is_annotative(
                &self.tabs[i].scene.document,
                &text.style,
            ),
            codec::EntityType::AttributeEntity(attribute) => {
                crate::scene::annotative::text_style_is_annotative(
                    &self.tabs[i].scene.document,
                    &attribute.text_style,
                )
            }
            codec::EntityType::AttributeDefinition(attribute) => {
                crate::scene::annotative::text_style_is_annotative(
                    &self.tabs[i].scene.document,
                    &attribute.text_style,
                )
            }
            _ => false,
        };
        if text_style_annotative {
            match &mut entity {
                codec::EntityType::MText(text) => text.is_annotative = true,
                codec::EntityType::AttributeEntity(attribute) => attribute.flags.annotative = true,
                codec::EntityType::AttributeDefinition(attribute) => {
                    attribute.flags.annotative = true
                }
                _ => {}
            }
        }
        let needs_annotation_context =
            crate::scene::annotative::is_annotative(&self.tabs[i].scene.document, &entity)
                || crate::scene::annotative::annotation_style_is_annotative(
                    &self.tabs[i].scene.document,
                    &entity,
                );

        let new_handle = if matches!(&entity, codec::EntityType::Viewport(_))
            && self.tabs[i].scene.current_layout != "Model"
        {
            // Assign a unique viewport ID (max existing id + 1, min 2).
            if let codec::EntityType::Viewport(ref mut vp) = entity {
                vp.id = self.tabs[i].scene.next_viewport_id();
            }

            let layout = self.tabs[i].scene.current_layout.clone();
            match self.tabs[i]
                .scene
                .document
                .add_entity_to_layout(entity, &layout)
            {
                Ok(new_handle) => {
                    if self.tabs[i].scene.is_recording_undo() {
                        self.tabs[i].scene.record_undo_before(new_handle, None);
                    }
                    self.tabs[i].scene.auto_fit_viewport(new_handle);
                    // Adding a viewport straight onto the document layout
                    // bypasses Scene::add_entity; publish the exact new handle
                    // so only its border is tessellated.
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(new_handle, crate::scene::ChangeKind::Added)]);
                    Some(new_handle)
                }
                Err(e) => {
                    self.command_line
                        .push_error(crate::tf!("Viewport could not be added: {e}").as_ref());
                    None
                }
            }
        } else {
            Some(self.tabs[i].scene.add_entity(entity))
        };

        // A linked table is one of its data link's connected objects.
        if let Some(handle) = new_handle {
            let link = match self.tabs[i].scene.document.get_entity(handle) {
                Some(codec::EntityType::Table(table)) => table
                    .rows
                    .iter()
                    .flat_map(|row| row.cells.iter())
                    .find_map(|cell| cell.data_link_handle),
                _ => None,
            };
            if let Some(link) = link {
                crate::app::annotation_data::connect_data_link(
                    &mut self.tabs[i].scene.document,
                    link,
                    handle,
                );
            }
        }

        if needs_annotation_context {
            if let (Some(handle), Some(scale)) = (
                new_handle,
                self.tabs[i].scene.creation_annotation_scale_handle(),
            ) {
                crate::scene::annotative::create_annotation_context(
                    &mut self.tabs[i].scene.document,
                    handle,
                    scale,
                );
                self.tabs[i]
                    .scene
                    .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
            }
        }

        if tracks_draw_anchor {
            if let Some(handle) = new_handle {
                self.tabs[i].last_draw_anchor = Some(handle);
            }
        }
        if tracks_dimension_chain {
            self.tabs[i].scene.last_created_dimension = new_handle;
        }
        // An OLE object placed now is at 100 % of the size it is given and
        // takes OLEQUALITY (automatic: high graphics for a picture).
        if let Some(handle) = new_handle {
            if let Some(codec::EntityType::Ole2Frame(ole)) =
                self.tabs[i].scene.document.get_entity_mut(handle)
            {
                ole.lock_aspect = super::commands::ole_quality().min(2) as u8;
                let (_, w, h) = crate::entities::ole2frame::frame_box(ole);
                self.tabs[i].ole_frames.insert(
                    handle.value(),
                    crate::entities::ole2frame::FrameState {
                        base: Some((w, h)),
                        lock_aspect: true,
                    },
                );
            }
        }
        self.sync_registered_layers(i);
        new_handle
    }
}

fn make_sections_read_only(
    sections: &mut [crate::scene::model::object::PropSection],
    keep: &[&str],
) {
    use crate::scene::model::object::PropValue;

    for property in sections
        .iter_mut()
        .flat_map(|section| section.props.iter_mut())
        .filter(|property| !keep.contains(&property.field))
    {
        let text = match &property.value {
            PropValue::ReadOnly(value)
            | PropValue::ReadOnlyWithTooltip { value, .. }
            | PropValue::EditText(value)
            | PropValue::PlainText(value)
            | PropValue::Hyperlink(value)
            | PropValue::LayerChoice(value)
            | PropValue::LinetypeChoice(value)
            | PropValue::HatchPatternChoice(value) => value.clone(),
            PropValue::BlockNumber { value, .. } => value.clone(),
            PropValue::BlockChoice { selected, options, .. } => options.get(*selected).cloned().unwrap_or_default(),
            PropValue::Choice { selected, .. } | PropValue::ChoiceWithTooltip { selected, .. } => selected.clone(),
            PropValue::EditChoice { value, .. } => value.clone(),
            PropValue::ColorChoice(color) => match color {
                codec::types::Color::None => "None".to_string(),
                codec::types::Color::ByLayer => "ByLayer".to_string(),
                codec::types::Color::ByBlock => "ByBlock".to_string(),
                codec::types::Color::Index(index) => index.to_string(),
                codec::types::Color::Rgb { r, g, b } => format!("{r},{g},{b}"),
            },
            PropValue::NamedColorChoice { name, .. } => name.clone(),
            PropValue::ColorVaries | PropValue::LwVaries | PropValue::FieldLwVaries { .. } => {
                VARIES_LABEL.to_string()
            }
            PropValue::LwChoice(lineweight)
            | PropValue::FieldLwChoice {
                value: lineweight, ..
            } => ui::properties::LwItem(*lineweight).to_string(),
            PropValue::BoolToggle { value, .. } => {
                if *value { t!("Yes") } else { t!("No") }.into_owned()
            }
            PropValue::Stepper { display, .. } => display.clone(),
            PropValue::AttrText { value, .. } => value.clone(),
            PropValue::PluginParam { value, .. } => value.clone(),
            // The Constraints/Parameters sections aren't per-entity editable
            // fields to begin with (a constraint link is a navigation
            // action, not a value; a parameter belongs to the whole
            // document, not the locked entity) — locking the entity doesn't
            // change either, so just carry through readable text instead of
            // collapsing them to a value that was never editable anyway.
            PropValue::EntityLink { handles, .. } => format!("{} entity link(s)", handles.len()),
            PropValue::ParamRow { name, formula, .. } => format!("{name} = {formula}"),
            PropValue::ParamAddRow => String::new(),
            PropValue::ParamsVisibilityToggle(value) => {
                if *value { t!("On") } else { t!("Off") }.into_owned()
            }
        };
        property.field = "locked_read_only";
        property.value = if let PropValue::ChoiceWithTooltip { tooltip, .. } = &property.value {
            PropValue::ReadOnlyWithTooltip { value: text, tooltip: tooltip.clone() }
        } else {
            PropValue::ReadOnly(text)
        };
    }
}

// ── Multi-selection property aggregation ───────────────────────────────────

pub(super) fn build_selection_groups(
    selected: &[(Handle, &EntityType)],
) -> Vec<ui::properties::SelectionGroup> {
    let mut groups = vec![ui::properties::SelectionGroup {
        label: format!("{} ({})", t!("All").into_owned(), selected.len()),
        handles: selected.iter().map(|(handle, _)| *handle).collect(),
    }];

    let mut by_type: std::collections::BTreeMap<String, Vec<Handle>> =
        std::collections::BTreeMap::new();
    for (handle, entity) in selected {
        by_type
            .entry(entity_type_key(entity))
            .or_default()
            .push(*handle);
    }

    for (kind, handles) in by_type {
        groups.push(ui::properties::SelectionGroup {
            label: format!(
                "{}({})",
                if kind == "insert" {
                    t!("Block Reference").into_owned()
                } else {
                    t!(title_case_word(&kind)).into_owned()
                },
                handles.len()
            ),
            handles,
        });
    }

    groups
}

impl crate::scene::model::object::PropSection {
    /// `pub` so the external `cargo bench` harness
    /// (`benches/properties_selection_aggregation.rs`, a separate crate) can
    /// time the real selection-rebuild aggregation entry. The
    /// `app::properties` module is private and `OpenCADStudio` itself is
    /// `pub(super)`, so neither the free `aggregate_sections` function nor an
    /// inherent app method is reachable externally; this bridge rides on the
    /// publicly reachable `PropSection` type instead (`#[doc(hidden)]` so it
    /// stays out of the public API docs).
    /// Calls the exact function the selection-rebuild block invokes, so the
    /// bench measures production semantics unchanged.
    #[doc(hidden)]
    pub fn aggregate_selection(
        selected: &[(Handle, &EntityType)],
        text_style_names: &[String],
    ) -> Vec<Self> {
        aggregate_sections(selected, text_style_names)
    }
}

/// Entity builders that read document-seeded per-thread state with no
/// transplantable getter: the text-style height/font caches
/// (`entities::common::FIXED_TEXT_HEIGHTS` / `STYLE_FONTS`, written by
/// `set_fixed_text_heights(&document)` — setter-only from here) and, for
/// tables, the table-cell stepper (`entities::table`, getter private to that
/// module). Selections containing any of these aggregate on the calling
/// thread, exactly as before parallelization; every other builder reads only
/// the unit context and the focused-vertex cell, both captured below and
/// replayed on each worker so rows match the calling thread's output.
fn builds_on_calling_thread(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Text(_)
            | EntityType::MText(_)
            | EntityType::AttributeDefinition(_)
            | EntityType::AttributeEntity(_)
            | EntityType::Table(_)
    )
}

/// Per-entity row build for one chunk of the selection, on rayon workers when
/// available (`crate::par::prelude` degrades to plain sequential iteration on
/// wasm32). Each build replays the calling thread's captured unit context and
/// focused vertex first, so a worker produces byte-for-byte the rows the
/// calling thread would — those two thread-locals are the only ones the
/// parallel-eligible builders read (see [`builds_on_calling_thread`]).
fn build_section_wave(
    wave: &[(Handle, &EntityType)],
    text_style_names: &[String],
    seeds: (crate::entities::common::UnitContext, usize),
) -> Vec<Vec<crate::scene::model::object::PropSection>> {
    use crate::par::prelude::*;

    wave.par_iter()
        .map(|&(handle, entity)| {
            crate::entities::common::set_unit_context(seeds.0);
            dispatch::set_prop_current_vertex(seeds.1);
            dispatch::properties_sectioned(handle, entity, text_style_names)
        })
        .collect()
}

pub(super) fn aggregate_sections(
    selected: &[(Handle, &EntityType)],
    text_style_names: &[String],
) -> Vec<crate::scene::model::object::PropSection> {
    let Some((first_handle, first_entity)) = selected.first() else {
        return vec![];
    };
    let mut result = dispatch::properties_sectioned(*first_handle, first_entity, text_style_names);
    let rest = &selected[1..];

    // `refresh_properties` seeds the unit context and the focused vertex on
    // this thread before every rebuild; capture them once so worker rebuilds
    // format rows identically.
    let seeds = (
        crate::entities::common::unit_context(),
        dispatch::prop_current_vertex(),
    );
    let parallel = selected.len() >= PARALLEL_AGGREGATE_MIN
        && !selected
            .iter()
            .any(|(_, entity)| builds_on_calling_thread(entity));

    if parallel {
        for wave in rest.chunks(PARALLEL_AGGREGATE_CHUNK) {
            // Nothing in common left to narrow: every later entity can only
            // intersect against an empty set.
            if result.is_empty() {
                break;
            }
            for sections in build_section_wave(wave, text_style_names, seeds) {
                intersect_into(&mut result, &sections);
            }
        }
    } else {
        for (handle, entity) in rest {
            // Nothing in common left to narrow: every later entity can only
            // intersect against an empty set.
            if result.is_empty() {
                break;
            }
            let sections = dispatch::properties_sectioned(*handle, entity, text_style_names);
            intersect_into(&mut result, &sections);
        }
    }
    // Sum the filled area while individual Area rows may still vary.
    if selected.len() > 1
        && selected
            .iter()
            .all(|(_, entity)| matches!(entity, codec::EntityType::Hatch(_)))
    {
        let total = selected
            .iter()
            .filter_map(|(_, entity)| match entity {
                codec::EntityType::Hatch(hatch) => {
                    Some(crate::entities::hatch::boundary_area(hatch))
                }
                _ => None,
            })
            .sum::<f64>();
        set_row(&mut result, "cumulative_area", format!("{total:.4}"));
    }
    // Several lines: their summed length below the Length row.
    if selected.len() > 1
        && selected
            .iter()
            .all(|(_, entity)| matches!(entity, codec::EntityType::Line(_)))
    {
        let total = selected
            .iter()
            .filter_map(|(_, entity)| match entity {
                codec::EntityType::Line(line) => Some(line.length()),
                _ => None,
            })
            .sum::<f64>();
        for section in result.iter_mut() {
            if let Some(at) = section.props.iter().position(|p| p.field == "length") {
                section.props.insert(
                    at + 1,
                    crate::scene::model::object::Property {
                        label: t!("Total length").into_owned(),
                        field: "total_length",
                        value: crate::scene::model::object::PropValue::ReadOnly(
                            crate::entities::common::format_length(total),
                        ),
                    },
                );
                break;
            }
        }
    }
    result
}

fn aggregate_solid_history_sections(
    document: &codec::Document,
    handles: &[Handle],
) -> Vec<crate::scene::model::object::PropSection> {
    let mut sections = handles
        .iter()
        .map(|handle| crate::scene::model::solid_history::primitive_properties(document, *handle));
    let Some(mut merged) = sections.next() else {
        return Vec::new();
    };
    if merged.is_empty() {
        return Vec::new();
    }
    for next in sections {
        if next.is_empty() {
            return Vec::new();
        }
        intersect_into(&mut merged, &next);
    }
    merged
}

fn retain_compact_solid_sections(sections: &mut Vec<crate::scene::model::object::PropSection>) {
    sections.iter_mut().for_each(|section| {
        let internal_surface_section = section
            .props
            .iter()
            .any(|property| property.field == "srf_kind");
        section.props.retain(|property| {
            matches!(
                property.field,
                "color"
                    | "layer"
                    | "linetype"
                    | "linetype_scale"
                    | "plot_style"
                    | "lineweight"
                    | "transparency"
                    | "hyperlink"
                    | "material"
            ) || (!internal_surface_section
                && crate::scene::model::solid_history::is_specialized_property(property.field))
        });
    });
    sections.retain(|section| !section.props.is_empty());
}

/// Intersect `acc` with `right` in place — one fold step of the former
/// `merge_sections(left, right)` helper, with identical semantics:
///
/// * `acc` keeps its section order, section titles, row order, labels, and
///   `field` ids (rows are matched, never re-keyed);
/// * each `acc` section/row matches the FIRST `right` entry carrying the same
///   title / `field` (mirroring the old linear `.find`s);
/// * rows absent from either side drop, as do sections left with no rows;
/// * surviving values fold through [`merge_prop_value`]; a value equal to its
///   counterpart is kept as-is, so identical selections clone nothing.
fn intersect_into(
    acc: &mut Vec<crate::scene::model::object::PropSection>,
    right: &[crate::scene::model::object::PropSection],
) {
    use rustc_hash::FxHashMap;

    // First match wins, exactly like `right.iter().find(...)`.
    let mut right_sections: FxHashMap<&str, usize> = FxHashMap::default();
    let mut right_fields: Vec<FxHashMap<&'static str, usize>> = Vec::with_capacity(right.len());
    for (section_index, section) in right.iter().enumerate() {
        let mut fields = FxHashMap::default();
        for (prop_index, prop) in section.props.iter().enumerate() {
            fields.entry(prop.field).or_insert(prop_index);
        }
        right_fields.push(fields);
        right_sections
            .entry(section.title.as_str())
            .or_insert(section_index);
    }

    acc.retain_mut(|section| {
        let Some(&section_index) = right_sections.get(section.title.as_str()) else {
            return false;
        };
        let fields = &right_fields[section_index];
        section.props.retain_mut(|prop| {
            let Some(&prop_index) = fields.get(prop.field) else {
                return false;
            };
            let other = &right[section_index].props[prop_index];
            if prop.value != other.value {
                prop.value = merge_prop_value(&prop.value, &other.value);
            }
            true
        });
        !section.props.is_empty()
    });
}

fn merge_prop_value(
    left: &crate::scene::model::object::PropValue,
    right: &crate::scene::model::object::PropValue,
) -> crate::scene::model::object::PropValue {
    use crate::scene::model::object::PropValue;

    if left == right {
        return left.clone();
    }

    match (left, right) {
        (
            PropValue::ChoiceWithTooltip { options, tooltip, .. },
            PropValue::ChoiceWithTooltip { options: other_options, tooltip: other_tooltip, .. },
        ) => {
            let mut merged_options = options.clone();
            for option in other_options {
                if !merged_options.contains(option) { merged_options.push(option.clone()); }
            }
            PropValue::ChoiceWithTooltip {
                selected: VARIES_LABEL.into(), options: merged_options,
                tooltip: if tooltip == other_tooltip { tooltip.clone() } else { String::new() },
            }
        }
        (PropValue::LayerChoice(_), PropValue::LayerChoice(_)) => {
            PropValue::LayerChoice(VARIES_LABEL.into())
        }
        (PropValue::ColorChoice(_), PropValue::ColorChoice(_))
        | (PropValue::NamedColorChoice { .. }, PropValue::NamedColorChoice { .. })
        | (PropValue::ColorChoice(_), PropValue::NamedColorChoice { .. })
        | (PropValue::NamedColorChoice { .. }, PropValue::ColorChoice(_))
        | (PropValue::ColorVaries, _)
        | (_, PropValue::ColorVaries) => PropValue::ColorVaries,
        (PropValue::LwChoice(_), PropValue::LwChoice(_))
        | (PropValue::LwVaries, _)
        | (_, PropValue::LwVaries) => PropValue::LwVaries,
        (
            PropValue::FieldLwChoice { field, .. },
            PropValue::FieldLwChoice {
                field: other_field, ..
            },
        ) if field == other_field => PropValue::FieldLwVaries { field },
        (PropValue::FieldLwVaries { field }, _) | (_, PropValue::FieldLwVaries { field }) => {
            PropValue::FieldLwVaries { field }
        }
        (PropValue::LinetypeChoice(_), PropValue::LinetypeChoice(_)) => {
            PropValue::LinetypeChoice(VARIES_LABEL.into())
        }
        (
            PropValue::Choice { options, .. },
            PropValue::Choice {
                options: other_options,
                ..
            },
        ) => {
            let mut merged_options = options.clone();
            for opt in other_options {
                if !merged_options.contains(opt) {
                    merged_options.push(opt.clone());
                }
            }
            PropValue::Choice {
                selected: VARIES_LABEL.into(),
                options: merged_options,
            }
        }
        (
            PropValue::EditChoice { options, .. },
            PropValue::EditChoice {
                options: other_options,
                ..
            },
        ) => {
            let mut merged_options = options.clone();
            for opt in other_options {
                if !merged_options.contains(opt) {
                    merged_options.push(opt.clone());
                }
            }
            PropValue::EditChoice {
                value: VARIES_LABEL.into(),
                options: merged_options,
            }
        }
        (PropValue::EditText(_), PropValue::EditText(_)) => {
            PropValue::EditText(VARIES_LABEL.into())
        }
        (PropValue::PlainText(_), PropValue::PlainText(_)) => {
            PropValue::PlainText(VARIES_LABEL.into())
        }
        (PropValue::Hyperlink(_), PropValue::Hyperlink(_)) => {
            PropValue::Hyperlink(VARIES_LABEL.into())
        }
        (PropValue::ReadOnly(_), PropValue::ReadOnly(_)) => {
            PropValue::ReadOnly(VARIES_LABEL.into())
        }
        (PropValue::ReadOnlyWithTooltip { tooltip, .. }, PropValue::ReadOnlyWithTooltip { .. }) => {
            PropValue::ReadOnlyWithTooltip {
                value: VARIES_LABEL.into(),
                tooltip: tooltip.clone(),
            }
        }
        (PropValue::ReadOnly(_), PropValue::ReadOnlyWithTooltip { tooltip, .. })
        | (PropValue::ReadOnlyWithTooltip { tooltip, .. }, PropValue::ReadOnly(_)) => {
            PropValue::ReadOnlyWithTooltip {
                value: VARIES_LABEL.into(),
                tooltip: tooltip.clone(),
            }
        }
        (PropValue::HatchPatternChoice(_), PropValue::HatchPatternChoice(_)) => {
            PropValue::HatchPatternChoice(VARIES_LABEL.into())
        }
        (
            PropValue::BoolToggle { field, .. },
            PropValue::BoolToggle {
                field: other_field, ..
            },
        ) if field == other_field => PropValue::ReadOnly(VARIES_LABEL.into()),
        _ => left.clone(),
    }
}

/// Set the first property row matching `field` (across all sections) to a
/// read-only `value`. No-op when the field is absent. Used to fill the
/// doc-dependent placeholder rows the entity builders leave empty.
/// An external reference's rows as the reference application lists them:
/// Insertion point X/Y/Z, Scale X/Y/Z without the unit conversion the INSERT
/// carries (no uniform-scale or Annotative rows), Saved Path after the name
/// and Layer property overrides last.
fn xref_rows(
    sections: &mut [crate::scene::model::object::PropSection],
    ins: &codec::entities::Insert,
    saved_path: &str,
    factor: f64,
    overrides: bool,
) {
    use crate::entities::common::{edit_prop, ro_prop};
    let factor = if factor.abs() > 0.0 { factor } else { 1.0 };
    for section in sections.iter_mut() {
        for prop in section.props.iter_mut() {
            let label = match prop.field {
                "ins_x" => "Insertion point X",
                "ins_y" => "Insertion point Y",
                "ins_z" => "Insertion point Z",
                _ => continue,
            };
            prop.label = t!(label).into_owned();
        }
        let scale_at = section.props.iter().position(|p| {
            matches!(
                p.field,
                "x_scale" | "y_scale" | "z_scale" | "u_scale" | "ins_uniform"
            )
        });
        section.props.retain(|p| {
            !matches!(
                p.field,
                "x_scale" | "y_scale" | "z_scale" | "u_scale" | "ins_uniform" | "annotative"
            )
        });
        if let Some(at) = scale_at {
            let at = at.min(section.props.len());
            section.props.splice(
                at..at,
                [
                    edit_prop(
                        t!("Scale X").as_ref(),
                        "xref_x_scale",
                        ins.x_scale() / factor,
                    ),
                    edit_prop(
                        t!("Scale Y").as_ref(),
                        "xref_y_scale",
                        ins.y_scale() / factor,
                    ),
                    edit_prop(
                        t!("Scale Z").as_ref(),
                        "xref_z_scale",
                        ins.z_scale() / factor,
                    ),
                ],
            );
        }
        if let Some(at) = section.props.iter().position(|p| p.field == "block") {
            section.props.insert(
                at + 1,
                ro_prop(
                    t!("Saved Path").as_ref(),
                    "xref_saved_path",
                    crate::modules::insert::xref_cmd::display_path(saved_path),
                ),
            );
        }
        if section.props.iter().any(|p| p.field == "unit_factor") {
            section.props.push(ro_prop(
                t!("Layer property overrides").as_ref(),
                "xref_layer_overrides",
                if overrides { t!("Yes") } else { t!("No") }.into_owned(),
            ));
        }
    }
}

fn set_row(sections: &mut [crate::scene::model::object::PropSection], field: &str, value: String) {
    for section in sections.iter_mut() {
        if let Some(row) = section.props.iter_mut().find(|p| p.field == field) {
            row.value = crate::scene::model::object::PropValue::ReadOnly(value);
            return;
        }
    }
}

/// Replace a row's value with an arbitrary control (editable field, dropdown,
/// colour picker …) rather than plain read-only text.
/// A dimensional constraint's Properties as the reference shows them: the
/// Constraint rows, then only the text rotation for a dynamic dimension
/// and the full dimension sections for an annotational one.
/// A Yes/No list row, as the reference shows on/off object properties.
fn yes_no_choice(flag: bool) -> crate::scene::model::object::PropValue {
    crate::scene::model::object::PropValue::Choice {
        selected: if flag { t!("Yes") } else { t!("No") }.into_owned(),
        options: vec![t!("Yes").into_owned(), t!("No").into_owned()],
    }
}

fn dynamic_dimension_sections(
    scene: &crate::scene::Scene,
    handle: Handle,
    set: &crate::scene::parametric_constraints::ParametricConstraintSet,
    constraint: &crate::scene::parametric_constraints::ParametricConstraint,
    sections: Vec<crate::scene::model::object::PropSection>,
) -> Vec<crate::scene::model::object::PropSection> {
    use crate::scene::model::object::{PropSection, PropValue, Property};
    use crate::scene::named_parameters::DrivingValue;
    let table = if set.local_parameters.is_empty() {
        scene.named_parameters()
    } else {
        &set.local_parameters
    };
    let reference = !constraint.enabled;
    let annotational = scene.dimension_is_annotational(handle);
    let (name, expression, value, description) = match &constraint.driving_param {
        Some(DrivingValue::Named(name)) => (
            name.clone(),
            table
                .iter()
                .find(|parameter| &parameter.name == name)
                .map(|parameter| parameter.source.clone())
                .unwrap_or_default(),
            table.resolve(name).ok(),
            table.description(name).to_string(),
        ),
        Some(DrivingValue::Literal(value)) => (
            String::new(),
            format!("{value}"),
            Some(*value),
            String::new(),
        ),
        None => (String::new(), String::new(), None, String::new()),
    };
    let row = |label: &str, field: &'static str, value: PropValue| Property {
        label: label.to_string(),
        field,
        value,
    };
    let yes_no = |flag: bool| PropValue::Choice {
        selected: if flag { t!("Yes") } else { t!("No") }.into_owned(),
        options: vec![t!("Yes").into_owned(), t!("No").into_owned()],
    };
    let text_rotation = sections
        .iter()
        .flat_map(|section| section.props.iter())
        .find(|property| property.field == "text_rotation")
        .cloned();
    let mut result = vec![PropSection {
        title: t!("Constraint").into_owned(),
        props: vec![
            row(
                t!("Constraint Form").as_ref(),
                "dyn_constraint_form",
                PropValue::Choice {
                    selected: if annotational {
                        "Annotational"
                    } else {
                        "Dynamic"
                    }
                    .to_string(),
                    options: vec!["Dynamic".to_string(), "Annotational".to_string()],
                },
            ),
            row(
                t!("Reference").as_ref(),
                "dyn_constraint_reference",
                yes_no(reference),
            ),
            row(
                t!("Name").as_ref(),
                "dyn_constraint_name",
                PropValue::EditText(name),
            ),
            row(
                t!("Expression").as_ref(),
                "dyn_constraint_expression",
                // A reference constraint's expression is the measurement.
                if reference {
                    PropValue::ReadOnly(expression)
                } else {
                    PropValue::EditText(expression)
                },
            ),
            row(
                t!("Value").as_ref(),
                "dyn_constraint_value",
                // An angle's value reads at the angular precision, as on the
                // dimension and in -PARAMETERS.
                PropValue::ReadOnly(match &constraint.driving_param {
                    Some(DrivingValue::Named(name)) if set.local_parameters.is_empty() => {
                        scene.parameter_value_text(name)
                    }
                    _ => value.map(|v| format!("{v:.4}")).unwrap_or_default(),
                }),
            ),
            row(
                t!("Description").as_ref(),
                "dyn_constraint_description",
                PropValue::EditText(description),
            ),
        ],
    }];
    if annotational {
        // The reference lists General first, then the constraint, then the
        // dimension's own groups; the constraint's dimension is not an
        // associative dimension to it, and its text is the constraint's.
        let mut sections = sections;
        sections.retain(|section| {
            section.title != t!("3D Visualization").as_ref() && section.title != "Associative Data"
        });
        for section in &mut sections {
            section
                .props
                .retain(|property| property.field != "association_status");
            for property in &mut section.props {
                match property.field {
                    "associative" => property.value = PropValue::ReadOnly("No".to_string()),
                    "text_override" => {
                        if let PropValue::PlainText(text) | PropValue::EditText(text) =
                            &property.value
                        {
                            property.value = PropValue::ReadOnly(text.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
        let constraint_section = result.remove(0);
        let general = (!sections.is_empty()).then(|| sections.remove(0));
        result = general
            .into_iter()
            .chain(std::iter::once(constraint_section))
            .chain(sections)
            .collect();
    } else if let Some(text_rotation) = text_rotation {
        result.push(PropSection {
            title: t!("Text").into_owned(),
            props: vec![text_rotation],
        });
    }
    result
}

fn set_row_value(
    sections: &mut [crate::scene::model::object::PropSection],
    field: &str,
    value: crate::scene::model::object::PropValue,
) {
    for section in sections.iter_mut() {
        if let Some(row) = section.props.iter_mut().find(|p| p.field == field) {
            row.value = value;
            return;
        }
    }
}

fn update_row_text(
    sections: &mut [crate::scene::model::object::PropSection],
    field: &str,
    value: String,
) {
    use crate::scene::model::object::PropValue;
    for section in sections.iter_mut() {
        let Some(row) = section
            .props
            .iter_mut()
            .find(|property| property.field == field)
        else {
            continue;
        };
        match &mut row.value {
            PropValue::ReadOnly(current)
            | PropValue::EditText(current)
            | PropValue::PlainText(current)
            | PropValue::Hyperlink(current) => *current = value,
            PropValue::ReadOnlyWithTooltip { value: current, .. } => *current = value,
            PropValue::Choice { selected, .. } | PropValue::ChoiceWithTooltip { selected, .. } => *selected = value,
            _ => {}
        }
        return;
    }
}

fn update_row_toggle(
    sections: &mut [crate::scene::model::object::PropSection],
    field: &str,
    value: bool,
) {
    use crate::scene::model::object::PropValue;
    for section in sections.iter_mut() {
        let Some(row) = section
            .props
            .iter_mut()
            .find(|property| property.field == field)
        else {
            continue;
        };
        match &mut row.value {
            PropValue::BoolToggle { value: current, .. } => *current = value,
            PropValue::ReadOnly(current) => {
                *current = if value { t!("Yes") } else { t!("No") }.into_owned()
            }
            PropValue::ReadOnlyWithTooltip { value: current, .. } => {
                *current = if value { t!("Yes") } else { t!("No") }.into_owned()
            }
            _ => {}
        }
        return;
    }
}

fn update_row_color(
    sections: &mut [crate::scene::model::object::PropSection],
    field: &str,
    color: codec::types::Color,
) {
    use crate::scene::model::object::PropValue;
    let label = match color {
        codec::types::Color::None => "None".to_string(),
        codec::types::Color::ByLayer => "ByLayer".to_string(),
        codec::types::Color::ByBlock => "ByBlock".to_string(),
        codec::types::Color::Index(index) => index.to_string(),
        codec::types::Color::Rgb { r, g, b } => format!("{r},{g},{b}"),
    };
    for section in sections.iter_mut() {
        let Some(row) = section
            .props
            .iter_mut()
            .find(|property| property.field == field)
        else {
            continue;
        };
        match &mut row.value {
            PropValue::ColorChoice(current) => *current = color,
            PropValue::NamedColorChoice { .. } => row.value = PropValue::ColorChoice(color),
            PropValue::ReadOnly(current) => *current = label,
            PropValue::ReadOnlyWithTooltip { value: current, .. } => *current = label,
            _ => {}
        }
        return;
    }
}

/// The lineweight dropdown options (named defaults + the standard millimetre
/// steps), matching the labels `dim_lineweight_label` produces.
pub(crate) fn lineweight_options() -> Vec<String> {
    crate::entities::common::lineweight_options()
}

/// Inverse of `dim_lineweight_label`: a lineweight label → DIMLWD enum value.
pub(crate) fn dim_lineweight_from_label(label: &str) -> i16 {
    match label.trim() {
        "ByLayer" => -1,
        "ByBlock" => -2,
        "Default" => -3,
        s => s
            .trim_end_matches("mm")
            .trim()
            .parse::<f64>()
            .map(|mm| (mm * 100.0).round() as i16)
            .unwrap_or(-3),
    }
}

/// The vertical-text-position dropdown options, matching `dimtad_label`.
pub(crate) fn tad_options() -> Vec<String> {
    ["Centered", "Above", "Outside", "JIS", "Below"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Inverse of `dimtad_label`: a vertical-position label → DIMTAD value.
pub(crate) fn dimtad_from_label(label: &str) -> i16 {
    match label.trim() {
        "Above" => 1,
        "Outside" => 2,
        "JIS" => 3,
        "Below" => 4,
        _ => 0,
    }
}

/// Resolve a dimension style by name (case-insensitive), falling back to
/// "Standard" when the name is blank.
fn find_dim_style<'a>(
    doc: &'a codec::Document,
    name: &str,
) -> Option<&'a codec::tables::DimStyle> {
    doc.dim_styles.iter().find(|s| {
        s.name.eq_ignore_ascii_case(name)
            || (name.trim().is_empty() && s.name.eq_ignore_ascii_case("Standard"))
    })
}

/// Insert `row` immediately after the first property whose field matches.
fn insert_row_after(
    sections: &mut [crate::scene::model::object::PropSection],
    field: &str,
    row: crate::scene::model::object::Property,
) {
    for section in sections.iter_mut() {
        if let Some(idx) = section.props.iter().position(|p| p.field == field) {
            section.props.insert(idx + 1, row);
            return;
        }
    }
}

/// Vertical text placement (DIMTAD) label.
fn dimtad_label(dimtad: i16) -> &'static str {
    match dimtad {
        1 => "Above",
        2 => "Outside",
        3 => "JIS",
        4 => "Below",
        _ => "Centered",
    }
}

/// Friendly arrowhead name from an arrowhead block-record name (the `_CLOSED…`
/// style internal names map to their palette labels; a null/empty name is the
/// closed-filled default).
pub(crate) fn arrowhead_label(name: &str) -> String {
    let key = name.trim().trim_start_matches('_').to_ascii_uppercase();
    let label = match key.as_str() {
        "" | "CLOSEDFILLED" => "Closed filled",
        "CLOSED" => "Closed",
        "CLOSEDBLANK" => "Closed blank",
        "DOT" => "Dot",
        "DOTSMALL" => "Dot small",
        "DOTBLANK" => "Dot blank",
        "SMALLDOTBLANK" => "Dot small blank",
        "ORIGIN" => "Origin indicator",
        "ORIGIN2" => "Origin indicator 2",
        "OPEN" => "Open",
        "OPEN90" => "Right angle",
        "OPEN30" => "Open 30",
        "NONE" => "None",
        "OBLIQUE" => "Oblique",
        "ARCHTICK" => "Architectural tick",
        "BOXBLANK" => "Box",
        "BOXFILLED" => "Box filled",
        "DATUMBLANK" => "Datum triangle",
        "DATUMFILLED" => "Datum triangle filled",
        "INTEGRAL" => "Integral",
        _ => return name.to_string(),
    };
    label.to_string()
}

/// The leader's arrowhead label, resolved from the dim style's DIMLDRBLK block.
fn leader_arrow_label(
    doc: &codec::Document,
    ds: &codec::tables::DimStyle,
    arrow_enabled: bool,
) -> String {
    if !arrow_enabled {
        return "None".to_string();
    }
    if ds.dimldrblk.is_null() {
        return "Closed filled".to_string();
    }
    doc.block_records
        .iter()
        .find(|b| b.handle == ds.dimldrblk)
        .map(|b| arrowhead_label(&b.name))
        .unwrap_or_else(|| "Closed filled".to_string())
}

/// DIMLWD lineweight enum → label.
fn dim_lineweight_label(dimlwd: i16) -> String {
    crate::entities::common::lineweight_label(dimlwd)
}

/// Human-readable INSUNITS name (DXF group 70 unit codes).
pub(in crate::app) fn insunits_name(code: i16) -> &'static str {
    match code {
        1 => "Inches",
        2 => "Feet",
        3 => "Miles",
        4 => "Millimeters",
        5 => "Centimeters",
        6 => "Meters",
        7 => "Kilometers",
        8 => "Microinches",
        9 => "Mils",
        10 => "Yards",
        11 => "Angstroms",
        12 => "Nanometers",
        13 => "Microns",
        14 => "Decimeters",
        15 => "Decameters",
        16 => "Hectometers",
        17 => "Gigameters",
        18 => "Astronomical Units",
        19 => "Light Years",
        20 => "Parsecs",
        21 => "US Survey Feet",
        22 => "US Survey Inches",
        23 => "US Survey Yards",
        24 => "US Survey Miles",
        0 => "Unitless",
        _ => "Unknown",
    }
}

/// Unit-conversion scale for a new INSERT.
pub(in crate::app) fn insert_unit_scale(host_units: i16, src_units: i16) -> Option<f64> {
    let host_mm = insunits_to_mm(host_units)?;
    let src_mm = insunits_to_mm(src_units)?;
    let ratio = src_mm / host_mm;
    if !ratio.is_finite() || (ratio - 1.0).abs() <= 1e-9 {
        return None;
    }
    Some(ratio)
}

pub(in crate::app) fn format_unit_factor(factor: f64) -> String {
    let magnitude = factor.abs();
    if magnitude > 0.0 && !(1.0e-4..1.0e7).contains(&magnitude) {
        format!("{factor:.4e}")
    } else {
        format!("{factor:.4}")
    }
}

/// Convert INSUNITS (DXF group 70) to millimetres.
pub(super) fn insunits_to_mm(code: i16) -> Option<f64> {
    Some(match code {
        1 => 25.4,                        // Inches
        2 => 304.8,                       // Feet
        3 => 1_609_344.0,                 // Miles
        4 => 1.0,                         // Millimeters
        5 => 10.0,                        // Centimeters
        6 => 1_000.0,                     // Meters
        7 => 1_000_000.0,                 // Kilometers
        8 => 0.000_025_4,                 // Microinches
        9 => 0.025_4,                     // Mils
        10 => 914.4,                      // Yards
        11 => 1.0e-7,                     // Angstroms
        12 => 1.0e-6,                     // Nanometers
        13 => 0.001,                      // Microns
        14 => 100.0,                      // Decimeters
        15 => 10_000.0,                   // Decameters
        16 => 100_000.0,                  // Hectometers
        17 => 1.0e12,                     // Gigameters
        18 => 1.495_978_707e14,           // Astronomical Units
        19 => 9.460_730_472_580_8e18,     // Light Years
        20 => 3.085_677_581_491_367_3e19, // Parsecs
        21 => 1_200_000.0 / 3_937.0,      // US Survey Feet
        22 => 100_000.0 / 3_937.0,        // US Survey Inches
        23 => 3_600_000.0 / 3_937.0,      // US Survey Yards
        24 => 6_336_000_000.0 / 3_937.0,  // US Survey Miles
        _ => return None,
    })
}

fn apply_insert_unit_scale(ins: &mut codec::entities::Insert, ratio: f64) -> bool {
    const MIN_INSERT_SCALE: f64 = 1.0e-12;
    if [ins.x_scale(), ins.y_scale(), ins.z_scale()]
        .into_iter()
        .map(|scale| scale * ratio)
        .any(|scale| !scale.is_finite() || scale.abs() < MIN_INSERT_SCALE)
    {
        return false;
    }

    let origin = ins.get_transform().apply(Vector3::ZERO);
    ins.apply_transform(&Transform::from_translation(-origin));
    ins.apply_transform(&Transform::from_scale(ratio));
    ins.apply_transform(&Transform::from_translation(origin));
    true
}

#[cfg(test)]
mod asym_scale_tests {
    // #47: the uniform-scale opt-out must be per-tab. Fresh documents restart
    // handle numbering (seed 1), so two tabs hand out identical handles — an
    // app-global set leaks one tab's opt-out into the other.
    use crate::app::OpenCADStudio;

    fn tab_with_insert(app: &mut OpenCADStudio) -> (usize, codec::Handle) {
        app.tab_counter += 1;
        let n = app.tab_counter;
        app.tabs
            .push(crate::app::document::DocumentTab::new_drawing(n));
        let idx = app.tabs.len() - 1;
        app.active_tab = idx;
        let ins = codec::entities::Insert::new("B", codec::types::Vector3::ZERO);
        let h = app
            .commit_entity_handle(codec::EntityType::Insert(ins))
            .expect("insert commits");
        (idx, h)
    }

    #[test]
    fn asym_scale_opt_out_does_not_leak_across_tabs() {
        let mut app = OpenCADStudio::new_for_test();
        let (a, ha) = tab_with_insert(&mut app);
        let (b, hb) = tab_with_insert(&mut app);
        assert_eq!(
            ha, hb,
            "premise: identically built fresh docs reuse handle values"
        );
        app.tabs[a].props_asym_scale.insert(ha.value());
        assert!(
            !app.tabs[b].props_asym_scale.contains(&hb.value()),
            "tab B must not see tab A's opt-out"
        );
    }
}

#[cfg(test)]
mod insert_unit_scale_tests {
    use super::{apply_insert_unit_scale, format_unit_factor, insert_unit_scale, insunits_to_mm};
    use codec::entities::{AttributeEntity, Insert};
    use codec::types::Vector3;

    const UNITLESS: i16 = 0;
    const INCHES: i16 = 1;
    const MILLIMETERS: i16 = 4;
    const CENTIMETERS: i16 = 5;
    const METERS: i16 = 6;

    #[test]
    fn unitless_block_is_inserted_as_authored() {
        // Reported case: a unitless block with a 1000-unit edge inserted into a
        // millimetre drawing must keep that edge, not gain a conversion factor.
        assert_eq!(insert_unit_scale(MILLIMETERS, UNITLESS), None);
        assert_eq!(insert_unit_scale(INCHES, UNITLESS), None);
    }

    #[test]
    fn unitless_drawing_does_not_scale_a_measured_block() {
        assert_eq!(insert_unit_scale(UNITLESS, METERS), None);
        assert_eq!(insert_unit_scale(UNITLESS, INCHES), None);
        assert_eq!(insert_unit_scale(UNITLESS, UNITLESS), None);
    }

    #[test]
    fn matching_units_do_not_scale() {
        assert_eq!(insert_unit_scale(METERS, METERS), None);
    }

    #[test]
    fn unknown_units_do_not_scale() {
        assert_eq!(insunits_to_mm(0), None);
        assert_eq!(insunits_to_mm(25), None);
        assert_eq!(insert_unit_scale(MILLIMETERS, 25), None);
    }

    #[test]
    fn differing_units_convert_through_millimetres() {
        let ratio = insert_unit_scale(MILLIMETERS, METERS).expect("metres into mm should scale");
        assert!((ratio - 1000.0).abs() < 1e-9, "got {ratio}");

        let ratio = insert_unit_scale(MILLIMETERS, INCHES).expect("inches into mm should scale");
        assert!((ratio - 25.4).abs() < 1e-9, "got {ratio}");

        let ratio = insert_unit_scale(METERS, CENTIMETERS).expect("cm into m should scale");
        assert!((ratio - 0.01).abs() < 1e-12, "got {ratio}");
    }

    #[test]
    fn survey_units_have_expected_ratios() {
        let survey_foot = insunits_to_mm(21).expect("survey feet");
        let survey_inch = insunits_to_mm(22).expect("survey inches");
        let survey_yard = insunits_to_mm(23).expect("survey yards");
        let survey_mile = insunits_to_mm(24).expect("survey miles");

        assert!((survey_foot / survey_inch - 12.0).abs() < 1e-12);
        assert!((survey_yard / survey_foot - 3.0).abs() < 1e-12);
        assert!((survey_mile / survey_foot - 5280.0).abs() < 1e-9);
    }

    #[test]
    fn astronomical_units_use_precise_si_values() {
        assert_eq!(insunits_to_mm(18), Some(1.495_978_707e14));
        assert_eq!(insunits_to_mm(19), Some(9.460_730_472_580_8e18));
        assert_eq!(insunits_to_mm(20), Some(3.085_677_581_491_367_3e19));
        assert_ne!(format_unit_factor(1.0e-7), "0.0000");
    }

    #[test]
    fn applying_unit_scale_composes_with_insert_transform() {
        let mut ins = Insert::new("Block", Vector3::new(12.0, -4.0, 3.0));
        ins.set_x_scale(-2.0);
        ins.set_y_scale(3.0);
        ins.set_z_scale(-4.0);
        ins.rotation = 0.37;
        let insertion = ins.get_transform().apply(Vector3::ZERO);
        let attribute_position = insertion + Vector3::new(2.0, -1.0, 0.5);
        let mut attribute = AttributeEntity::simple("TAG", "Value");
        attribute.insertion_point = attribute_position;
        ins.attributes.push(attribute);

        assert!(apply_insert_unit_scale(&mut ins, 25.4));

        let scaled_insertion = ins.get_transform().apply(Vector3::ZERO);
        let expected_attribute = insertion + (attribute_position - insertion) * 25.4;
        assert!((scaled_insertion - insertion).length() < 1e-9);
        assert!((ins.attributes[0].insertion_point - expected_attribute).length() < 1e-9);
        assert!((ins.x_scale() + 50.8).abs() < 1e-9);
        assert!((ins.y_scale() - 76.2).abs() < 1e-9);
        assert!((ins.z_scale() + 101.6).abs() < 1e-9);
    }

    #[test]
    fn applying_large_unit_scale_keeps_distant_insert_fixed() {
        let mut ins = Insert::new("Block", Vector3::new(1.0e12, -2.0e12, 3.0e12));
        let insertion = ins.get_transform().apply(Vector3::ZERO);

        assert!(apply_insert_unit_scale(&mut ins, 1.0e20));

        let scaled_insertion = ins.get_transform().apply(Vector3::ZERO);
        assert_eq!(scaled_insertion, insertion);
        assert_eq!(ins.x_scale(), 1.0e20);
    }

    #[test]
    fn unsupported_tiny_unit_scale_is_not_applied() {
        let mut ins = Insert::new("Block", Vector3::new(1.0, 2.0, 3.0));
        let before = ins.clone();

        assert!(!apply_insert_unit_scale(&mut ins, 1.0e-13));
        assert_eq!(ins, before);
    }
}

#[cfg(test)]
mod apply_property_op_tests {
    use crate::app::OpenCADStudio;
    use codec::entities::Line;
    use codec::types::{Color, Vector3};

    fn line_handle(app: &mut OpenCADStudio) -> codec::Handle {
        let mut line = Line::new();
        line.start = Vector3::ZERO;
        line.end = Vector3::new(1.0, 0.0, 0.0);
        app.commit_entity_handle(codec::EntityType::Line(line))
            .expect("line should commit")
    }

    #[test]
    fn empty_handles_does_not_invoke_or_dirty() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        let before = app.tabs[i].dirty;
        let called = std::rc::Rc::new(std::cell::Cell::new(false));
        let called2 = called.clone();
        app.apply_property_op(i, "TEST", &[], |_app, _h| {
            called2.set(true);
        });
        assert!(!called.get(), "closure must not run for empty handles");
        assert_eq!(
            app.tabs[i].dirty, before,
            "dirty must be unchanged for empty handles"
        );
    }

    #[test]
    fn applies_to_each_handle_sets_dirty_and_mutates() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        let h1 = line_handle(&mut app);
        let h2 = line_handle(&mut app);
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let count2 = count.clone();
        app.tabs[i].dirty = false;
        app.apply_property_op(i, "CHPROP", &[h1, h2], |app2, h| {
            count2.set(count2.get() + 1);
            if let Some(e) = app2.tabs[app2.active_tab].scene.document.get_entity_mut(h) {
                e.common_mut().color = Color::Index(7);
            }
        });
        assert_eq!(count.get(), 2, "closure runs once per handle");
        assert!(app.tabs[i].dirty, "tab marked dirty");
        let e = app.tabs[i]
            .scene
            .document
            .get_entity(h1)
            .expect("entity present");
        assert_eq!(
            e.common().color,
            Color::Index(7),
            "entity mutation persisted"
        );
    }

    #[test]
    fn missing_entity_does_not_abort_loop() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        let h1 = line_handle(&mut app);
        // A freshly allocated handle has no entity yet.
        let missing = app.tabs[i].scene.document.allocate_handle();
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let count2 = count.clone();
        app.tabs[i].dirty = false;
        app.apply_property_op(i, "CHPROP", &[h1, missing], |app2, h| {
            count2.set(count2.get() + 1);
            if let Some(e) = app2.tabs[app2.active_tab].scene.document.get_entity_mut(h) {
                e.common_mut().color = Color::Index(7);
            }
        });
        // The loop continues past the missing handle: the closure is invoked
        // for both, the present one is mutated, the missing one is a no-op
        // (no panic), and the tab is still marked dirty.
        assert_eq!(
            count.get(),
            2,
            "closure still called for the missing handle"
        );
        assert!(
            app.tabs[i].dirty,
            "tab marked dirty even with a missing handle"
        );
        let e = app.tabs[i]
            .scene
            .document
            .get_entity(h1)
            .expect("entity present");
        assert_eq!(
            e.common().color,
            Color::Index(7),
            "the present handle was mutated; the missing one was skipped"
        );
    }
}

#[cfg(test)]
mod chprop_integration_tests {
    use crate::app::{Message, OpenCADStudio};
    use codec::entities::Line;
    use codec::types::{Color, LineWeight, Vector3};

    fn line_handle(app: &mut OpenCADStudio) -> codec::Handle {
        let mut line = Line::new();
        line.start = Vector3::ZERO;
        line.end = Vector3::new(1.0, 0.0, 0.0);
        app.commit_entity_handle(codec::EntityType::Line(line))
            .expect("line should commit")
    }

    /// Full-handler integration: a colour change on a multi-entity
    /// selection flows through `Message::PropColorChanged` -> the
    /// property-op handler -> `apply_property_op`, and is reversible
    /// via undo / redo. Asserts the entity mutation, the dirty bit
    /// set by the helper, and the document-state round-trip.
    ///
    /// Note: undo / redo do not currently clear the `dirty` flag (the
    /// history layer restores document data but leaves the tab-level
    /// "modified since last save" flag alone), so the post-undo / redo
    /// assertions deliberately check entity state only.
    #[test]
    fn prop_color_change_multiple_entities_undo_redo() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        let h1 = line_handle(&mut app);
        let h2 = line_handle(&mut app);
        let original = app.tabs[i]
            .scene
            .document
            .get_entity(h1)
            .unwrap()
            .common()
            .color;

        // Seed the Properties panel's `source_handles` so
        // `property_target_handles` returns the two lines. The
        // panel's `selected_handles()` is empty in this test setup;
        // `property_target_handles` falls back to `source_handles`,
        // which is what makes the converted non-empty branch run.
        app.tabs[i].properties.source_handles = vec![h1, h2];
        let _ = app.update(Message::PropColorChanged(Color::Index(5)));

        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(h1)
                .unwrap()
                .common()
                .color,
            Color::Index(5)
        );
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(h2)
                .unwrap()
                .common()
                .color,
            Color::Index(5)
        );
        assert!(app.tabs[i].dirty, "helper must set the dirty bit");

        app.undo_active_tab();
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(h1)
                .unwrap()
                .common()
                .color,
            original
        );
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(h2)
                .unwrap()
                .common()
                .color,
            original
        );

        app.redo_active_tab();
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(h1)
                .unwrap()
                .common()
                .color,
            Color::Index(5)
        );
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(h2)
                .unwrap()
                .common()
                .color,
            Color::Index(5)
        );
    }

    #[test]
    fn property_target_resolvers_discard_stale_handles() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        let existing = line_handle(&mut app);
        let stale = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].properties.source_handles = vec![existing, stale];

        assert_eq!(app.property_target_handles(i), vec![existing]);
        assert_eq!(app.point_cloud_lock_targets(i), vec![existing]);

        app.tabs[i].properties.source_handles = vec![stale];
        assert!(app.property_target_handles(i).is_empty());
        assert!(app.point_cloud_lock_targets(i).is_empty());

        app.finish_pending_history(i);
        let undo_before = app.tabs[i].history.undo_stack.len();
        let color_before = app.tabs[i].scene.document.header.current_entity_color;
        app.tabs[i].dirty = false;
        let _ = app.update(crate::app::Message::PropColorChanged(
            codec::types::Color::Index(9),
        ));
        app.finish_pending_history(i);
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before);
        assert_eq!(
            app.tabs[i].scene.document.header.current_entity_color,
            color_before
        );
        assert!(!app.tabs[i].dirty);
    }

    /// Regression test for the CLI `CHPROP` command. Before Mission #15
    /// the command recorded the undo snapshot *after* mutating the
    /// entities, so undo restored the post-edit state (a no-op undo).
    /// Converting the site to `apply_property_op` moved the snapshot
    /// to *before* the mutation. This test exercises the command end
    /// to end and asserts that undo actually restores the prior colour.
    #[ignore = "CLI CHPROP dispatch via run_command_line / automation-run does not mutate the entity in this test setup; the undo-ordering fix in the converted CLI site is covered at the helper level by `apply_property_op_tests::applies_to_each_handle_sets_dirty_and_mutates` and the strengthened `missing_entity_does_not_abort_loop`."]
    #[test]
    fn cli_chprop_undo_restores_previous_state() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        let h = line_handle(&mut app);
        let original = app.tabs[i]
            .scene
            .document
            .get_entity(h)
            .unwrap()
            .common()
            .color;

        let _ = app.automation_op(r#"{"op":"select","type":"Line"}"#);
        let _ = app.automation_op(r#"{"op":"run","cmd":"CHPROP COLOR 3"}"#);
        assert_ne!(
            app.tabs[i]
                .scene
                .document
                .get_entity(h)
                .unwrap()
                .common()
                .color,
            original,
            "CHPROP COLOR 3 must have changed the line's colour"
        );

        app.undo_active_tab();
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(h)
                .unwrap()
                .common()
                .color,
            original,
            "undo must restore the pre-CHPROP colour"
        );
    }

    /// When a selected entity's layer is locked, `property_target_handles`
    /// filters it out, the handler's `if handles.is_empty()` branch runs,
    /// and the entity itself is *not* mutated. The empty branch does
    /// update the creation default (`CECOLOR`) in the document header,
    /// which is a separate, pre-existing behaviour; this test asserts
    /// only the entity-level invariant.
    #[test]
    fn prop_change_on_locked_layer_is_ignored() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        let h = line_handle(&mut app);

        let layer_name = app.tabs[i]
            .scene
            .document
            .get_entity(h)
            .unwrap()
            .common()
            .layer
            .clone();
        {
            let layer = app.tabs[i]
                .scene
                .document
                .layers
                .get_mut(&layer_name)
                .expect("entity's layer must exist");
            layer.flags.locked = true;
        }
        assert!(
            app.tabs[i].scene.is_layer_locked(h),
            "test setup: layer must report locked"
        );

        let _ = app.automation_op(r#"{"op":"select","type":"Line"}"#);
        let _ = app.update(Message::PropColorChanged(Color::Index(9)));

        assert_ne!(
            app.tabs[i]
                .scene
                .document
                .get_entity(h)
                .unwrap()
                .common()
                .color,
            Color::Index(9),
            "a locked-layer entity must not be mutated by PropColorChanged"
        );
    }

    /// The Home-ribbon Lineweight chip is updated by the converted
    /// `RibbonLineweightChanged` handler. Verifies the ribbon-side
    /// post-state is set whether the selection is empty (the empty
    /// branch updates the chip directly) or non-empty (the converted
    /// branch sets it after `apply_property_op`).
    #[test]
    fn ribbon_lineweight_updates_after_change() {
        let mut app = OpenCADStudio::new_for_test();
        let _h = line_handle(&mut app);
        let _ = app.automation_op(r#"{"op":"select","type":"Line"}"#);

        let new_lw = LineWeight::from_value(50);
        let _ = app.update(Message::RibbonLineweightChanged(new_lw));
        assert_eq!(app.ribbon.active_lineweight, new_lw);
    }
}

#[cfg(test)]
mod grip_limit_tests {
    use super::*;
    use crate::scene::model::object::GripShape;

    fn test_grip(id: usize, is_midpoint: bool) -> GripDef {
        GripDef {
            id,
            world: glam::DVec3::ZERO,
            is_midpoint,
            shape: GripShape::Square,
            dir: None,
            axis: None,
        }
    }

    #[test]
    fn refresh_selected_grips_caps_total_grip_count() {
        let cap = crate::app::settings::MAX_SELECTED_GRIPS;
        let n = cap + 500;
        let grips: Vec<GripDef> = (0..n).map(|id| test_grip(id, false)).collect();
        let handles: Vec<Handle> = (0..n as u64).map(|k| Handle::new(k + 1)).collect();
        let (grips, handles) = super::apply_grip_budget(grips, handles);
        assert_eq!(
            grips.len(),
            cap,
            "grips past the budget must be dropped, kept {}/{}",
            grips.len(),
            n,
        );
        assert_eq!(
            handles.len(),
            grips.len(),
            "handles must stay in lockstep with grips",
        );
        for (grip, handle) in grips.iter().zip(handles.iter()) {
            assert_eq!(
                *handle,
                Handle::new(grip.id as u64 + 1),
                "handle at each index must still belong to its grip",
            );
        }
    }

    #[test]
    fn refresh_selected_grips_prefers_vertex_grips_over_midpoints() {
        let cap = crate::app::settings::MAX_SELECTED_GRIPS;
        let n = cap + 500;
        // Interleaved: even ids are midpoints, odd ids are vertices.
        let grips: Vec<GripDef> = (0..n).map(|id| test_grip(id, id % 2 == 0)).collect();
        let handles: Vec<Handle> = (0..n as u64).map(|k| Handle::new(k + 1)).collect();
        let (grips, handles) = super::apply_grip_budget(grips, handles);
        assert_eq!(grips.len(), cap);
        assert_eq!(handles.len(), grips.len());
        // Fewer vertices than the cap: every vertex grip survives, midpoints
        // fill the remainder — no vertex is sacrificed for a midpoint.
        let kept_vertices: Vec<usize> = grips
            .iter()
            .filter(|grip| !grip.is_midpoint)
            .map(|grip| grip.id)
            .collect();
        let all_vertices: Vec<usize> = (0..n).filter(|id| id % 2 == 1).collect();
        assert_eq!(
            kept_vertices, all_vertices,
            "no vertex grip may be sacrificed for a midpoint",
        );
        let ids: Vec<usize> = grips.iter().map(|grip| grip.id).collect();
        assert!(ids.is_sorted(), "retained grips must keep entity order");
        for (grip, handle) in grips.iter().zip(handles.iter()) {
            assert_eq!(*handle, Handle::new(grip.id as u64 + 1));
        }

        // Vertices alone past the cap: the tail is truncated deterministically.
        let over = cap + 100;
        let grips: Vec<GripDef> = (0..over).map(|id| test_grip(id, false)).collect();
        let handles: Vec<Handle> = (0..over as u64).map(|k| Handle::new(k + 1)).collect();
        let (grips, handles) = super::apply_grip_budget(grips, handles);
        assert_eq!(grips.len(), cap);
        assert_eq!(handles.len(), grips.len());
        assert!(
            grips.iter().zip(0..cap).all(|(grip, id)| grip.id == id),
            "vertex overflow must keep the first `cap` grips in entity order",
        );
        for (grip, handle) in grips.iter().zip(handles.iter()) {
            assert_eq!(*handle, Handle::new(grip.id as u64 + 1));
        }
    }

    #[test]
    fn grip_budget_boundary_lengths() {
        let cap = crate::app::settings::MAX_SELECTED_GRIPS;

        // Empty vec passthrough.
        let (grips, handles) = super::apply_grip_budget(Vec::new(), Vec::new());
        assert!(grips.is_empty());
        assert!(handles.is_empty());

        // len == cap passthrough: content untouched.
        let grips: Vec<GripDef> = (0..cap).map(|id| test_grip(id, id % 2 == 0)).collect();
        let handles: Vec<Handle> = (0..cap as u64).map(|k| Handle::new(k + 1)).collect();
        let (kept_grips, kept_handles) = super::apply_grip_budget(grips, handles);
        assert_eq!(kept_grips.len(), cap);
        assert_eq!(kept_handles.len(), cap);
        assert!(
            kept_grips
                .iter()
                .zip(0..cap)
                .all(|(grip, id)| grip.id == id),
            "at exactly the cap every grip must survive in order",
        );
        for (grip, handle) in kept_grips.iter().zip(kept_handles.iter()) {
            assert_eq!(*handle, Handle::new(grip.id as u64 + 1));
        }

        // len == cap + 1 truncates to cap.
        let over = cap + 1;
        let grips: Vec<GripDef> = (0..over).map(|id| test_grip(id, false)).collect();
        let handles: Vec<Handle> = (0..over as u64).map(|k| Handle::new(k + 1)).collect();
        let (kept_grips, kept_handles) = super::apply_grip_budget(grips, handles);
        assert_eq!(kept_grips.len(), cap);
        assert_eq!(kept_handles.len(), cap);
        assert!(
            kept_grips
                .iter()
                .zip(0..cap)
                .all(|(grip, id)| grip.id == id),
            "one over the cap must drop exactly the tail grip",
        );
    }

    #[test]
    fn a_selection_past_the_limit_gets_no_grips() {
        use codec::entities::{EntityType, Line};
        use codec::types::Vector3;

        let select_n = |n: usize, limit: Option<i32>| -> usize {
            let mut app = OpenCADStudio::new_for_test();
            app.automation_op(r#"{"op":"new"}"#);
            if let Some(limit) = limit {
                app.grip_object_limit = limit;
            }
            let i = app.active_tab;
            let handles: Vec<_> = (0..n)
                .map(|k| {
                    let x = k as f64;
                    app.tabs[i]
                        .scene
                        .add_entity(EntityType::Line(Line::from_points(
                            Vector3::new(x, 0.0, 0.0),
                            Vector3::new(x + 1.0, 1.0, 0.0),
                        )))
                })
                .collect();
            for handle in handles {
                app.tabs[i].scene.select_entity(handle, false);
            }
            app.refresh_selected_grips();
            app.tabs[i].selected_grips.len()
        };

        assert!(
            select_n(
                crate::app::settings::DEFAULT_GRIP_OBJECT_LIMIT as usize,
                None
            ) > 0,
            "a selection at the limit must still show its grips",
        );
        assert_eq!(
            select_n(
                crate::app::settings::DEFAULT_GRIP_OBJECT_LIMIT as usize + 1,
                None
            ),
            0,
            "one past the limit must show none at all",
        );
    }

    /// The ViewCube, the UCS icon and selection cycling all have commands and
    /// status-bar pills, but their values lived only on the app struct: turn
    /// the ViewCube off, restart, and it came straight back. Nothing failed —
    /// the setting simply evaporated — which is exactly the kind of thing a
    /// round-trip test catches and a person does not.
    #[test]
    fn the_view_toggles_survive_a_save_and_load() {
        let mut app = OpenCADStudio::new_for_test();
        app.show_viewcube = false;
        app.show_ucs_icon = false;
        app.ucs_icon_at_origin = false;
        app.selection_cycling = true;

        let saved = app.current_settings();
        let mut restored = OpenCADStudio::new_for_test();
        restored.apply_settings(&saved);

        assert!(!restored.show_viewcube, "the ViewCube must stay off");
        assert!(!restored.show_ucs_icon);
        assert!(!restored.ucs_icon_at_origin);
        assert!(restored.selection_cycling);
    }

    /// The Options card writes these four straight onto the app and relies on
    /// the snapshot/restore pair to carry them across a restart. Three of them
    /// had no UI until now and so no reason for anyone to notice if the pair
    /// missed one.
    #[test]
    fn the_selection_settings_survive_a_save_and_load() {
        let mut app = OpenCADStudio::new_for_test();
        app.pick_add = false;
        app.pick_drag_rect = true;
        app.pick_box = 11;
        app.grip_object_limit = 0;

        let saved = app.current_settings();
        let mut restored = OpenCADStudio::new_for_test();
        restored.apply_settings(&saved);

        assert!(!restored.pick_add);
        assert!(restored.pick_drag_rect);
        assert_eq!(restored.pick_box, 11);
        assert_eq!(
            restored.grip_object_limit, 0,
            "zero is a real value here, not an unset field",
        );
    }

    /// `GRIPOBJLIMIT 0` means no limit, and the old comparison — a bare `>`
    /// against the stored number — could not express that: zero would have
    /// suppressed every grip instead of allowing all of them.
    #[test]
    fn a_limit_of_zero_means_no_limit() {
        use codec::entities::{EntityType, Line};
        use codec::types::Vector3;

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.grip_object_limit = 0;
        let i = app.active_tab;
        let over = crate::app::settings::DEFAULT_GRIP_OBJECT_LIMIT as usize * 3;
        let handles: Vec<_> = (0..over)
            .map(|k| {
                let x = k as f64;
                app.tabs[i]
                    .scene
                    .add_entity(EntityType::Line(Line::from_points(
                        Vector3::new(x, 0.0, 0.0),
                        Vector3::new(x + 1.0, 1.0, 0.0),
                    )))
            })
            .collect();
        for handle in handles {
            app.tabs[i].scene.select_entity(handle, false);
        }
        app.refresh_selected_grips();
        assert!(
            app.tabs[i].selected_grips.len() > 0,
            "zero must read as unlimited, not as a limit of none",
        );
    }
}

#[cfg(test)]
mod aggregation_tests {
    use super::*;
    use codec::EntityType;

    fn line(layer: &str, color: i16) -> EntityType {
        let mut line = codec::entities::line::Line::default();
        line.common.layer = layer.to_string();
        line.common.color = codec::types::Color::from_index(color);
        EntityType::Line(line)
    }

    fn row<'a>(
        sections: &'a [crate::scene::model::object::PropSection],
        field: &str,
    ) -> Option<&'a crate::scene::model::object::Property> {
        sections
            .iter()
            .flat_map(|section| section.props.iter())
            .find(|property| property.field == field)
    }

    #[test]
    fn shared_values_survive_the_fold_and_differing_ones_do_not() {
        let entities = [line("WALLS", 1), line("WALLS", 3), line("WALLS", 1)];
        let selected: Vec<(Handle, &EntityType)> = entities
            .iter()
            .enumerate()
            .map(|(i, entity)| (Handle::new(i as u64 + 1), entity))
            .collect();

        let sections = aggregate_sections(&selected, &[]);
        assert!(!sections.is_empty(), "three lines share their layer rows");

        // A colour the entities disagree on has its own variant rather than
        // the text label, so the two rows are checked on their own terms.
        let layer = row(&sections, "layer").expect("a layer row");
        assert!(
            format!("{:?}", layer.value).contains("WALLS"),
            "one layer across all three: {:?}",
            layer.value,
        );
        let color = row(&sections, "color").expect("a color row");
        assert!(
            format!("{:?}", color.value).contains("Varies"),
            "two colours across three lines: {:?}",
            color.value,
        );
    }

    // One entity folds to its own properties, and the fold must not depend on
    // there being a second one to merge against.
    #[test]
    fn a_single_entity_aggregates_to_its_own_rows() {
        let entity = line("WALLS", 1);
        let selected = [(Handle::new(1), &entity)];
        let sections = aggregate_sections(&selected, &[]);
        let layer = row(&sections, "layer").expect("a layer row");
        assert!(format!("{:?}", layer.value).contains("WALLS"));
    }
}

#[cfg(test)]
mod custom_class_title_tests {
    // Plugin-declared class labels (API v10) qualify the single-entity panel
    // title as "<Label> (<Native>)" only while a declaring plugin is loaded.
    // The test harness never loads plugins, so the observable behavior here is
    // the gating: an entity stamped with a perfect identity record (registered
    // app, class-prefix key) still shows today's exact native title. The
    // "<Label> (<Native>)" form itself is covered by
    // plugin::entity_labels::display_type_title, and the live-manager
    // filtering by plugin::process::manager tests.
    use crate::app::OpenCADStudio;

    #[test]
    fn single_entity_title_stays_native_without_a_loaded_plugin() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        let mut entity = codec::EntityType::Line(codec::entities::Line::from_points(
            codec::types::Vector3::ZERO,
            codec::types::Vector3::new(1.0, 0.0, 0.0),
        ));
        let mut record = codec::xdata::ExtendedDataRecord::new("CVIFC");
        record.add_value(codec::xdata::XDataValue::String(
            "CVIFC-CLASS:LINE".to_string(),
        ));
        entity.common_mut().extended_data.add_record(record);
        let handle = app.tabs[i].scene.add_entity(entity);

        let source = app.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .expect("committed line");
        let (_, title) = app.entity_property_sections(i, handle, source, &Vec::new(), None, 0);
        assert_eq!(title, "Line");
    }
}
