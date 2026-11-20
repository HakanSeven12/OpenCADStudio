use super::*;

impl Scene {
    pub(crate) fn prefers_imperial_scales(&self) -> Option<bool> {
        match self.document.header.insertion_units {
            1..=3 | 8..=10 | 21..=24 => Some(true),
            4..=7 | 11..=20 => Some(false),
            0 if self.document.header.measurement == 1 => Some(false),
            _ => None,
        }
    }

    pub(crate) fn default_scales(&self) -> &'static [(&'static str, f64)] {
        if self.prefers_imperial_scales() == Some(true) {
            Self::DEFAULT_IMPERIAL_SCALES
        } else {
            Self::DEFAULT_METRIC_SCALES
        }
    }

    /// Model-space annotation multiplier from the header's CANNOSCALEVALUE
    /// (paper/drawing factor). Every path that starts or loads a drawing, or
    /// changes its units, goes through here so it agrees with CANNOSCALE.
    ///
    /// Annotation sizes (text heights, DIMTXT, arrow sizes) are in drawing
    /// units, so the multiplier is the scale's own ratio whatever INSUNITS
    /// says: 0.2 text at 1:30 is 6 drawing units in a centimetre drawing
    /// (#2354). A template whose scales are stored already converted (a
    /// metre drawing's 1:50 as 1 = 0.05) gets its conversion from them.
    pub(crate) fn sync_annotation_scale_from_header(&mut self) {
        let cannoscale_value = self
            .scale_object_handle(&self.document.header.current_annotation_scale)
            .and_then(|handle| match self.document.objects.get(&handle) {
                Some(ObjectType::Scale(scale)) => Some(scale.factor()),
                _ => None,
            })
            .unwrap_or(self.document.header.annotation_scale_value);
        self.annotation_scale = if cannoscale_value > 1e-9 {
            (1.0 / cannoscale_value) as f32
        } else {
            1.0
        };
    }

    /// Conversion from one drawing unit to the paper unit of the standard
    /// scale family: millimetres for metric drawings, inches for imperial
    /// ones. Only the standard set the program offers is converted with it, so
    /// a metre drawing's "1:100" is stored as 1 = 0.1. A scale the drawing
    /// holds is used as its paper and drawing units say, whatever INSUNITS is.
    pub(crate) fn default_scale_unit_factor(&self) -> f64 {
        let paper_unit = if self.prefers_imperial_scales() == Some(true) {
            1 // Inches
        } else {
            4 // Millimeters
        };
        crate::modules::draw::units::conversion_factor(
            self.document.header.insertion_units,
            paper_unit,
        )
        .filter(|factor| factor.is_finite() && *factor > 0.0)
        .unwrap_or(1.0)
    }

    /// A standard scale ("1:200") means a physical paper-to-model ratio,
    /// even in older DWGs that store its unconverted 1/200 definition.
    ///
    /// Newer DWGs may already store the paper units converted to the model
    /// unit (1 paper = 0.2 drawing for a metre-based "1:200"). A custom
    /// scale, including "1:100 m", always keeps its own stored ratio.
    /// Normalize only exact standard names with exact legacy ratios.
    ///
    /// Viewport and plot scales only: the annotation multiplier is the
    /// scale's own ratio whatever the drawing units (#2354), so a centimetre
    /// drawing's 1:50 draws annotative text 50 times its paper height.
    pub(crate) fn effective_scale_factor(&self, scale: &document::objects::Scale) -> f64 {
        let stored = scale.factor();
        let legacy_standard = self.default_scales().iter().any(|&(name, base)| {
            scale.name.eq_ignore_ascii_case(name) && (stored - base).abs() <= 1.0e-9 * base.max(1.0)
        });
        if legacy_standard {
            stored * self.default_scale_unit_factor()
        } else {
            stored
        }
    }

    pub(crate) fn is_architectural_scale_name(name: &str) -> bool {
        name.contains('=') && name.contains('"') && name.contains('\'')
    }

    pub(crate) fn is_ratio_scale_name(name: &str) -> bool {
        name.split_once(':')
            .and_then(|(paper, drawing)| {
                Some((
                    paper.trim().parse::<f64>().ok()?,
                    drawing.trim().parse::<f64>().ok()?,
                ))
            })
            .is_some_and(|(paper, drawing)| paper > 0.0 && drawing > 0.0)
    }

    /// True when the drawing owns at least one annotation scale of its own
    /// (i.e. `scale_list` returns real objects rather than the fallback set).
    pub(crate) fn has_own_scales(&self) -> bool {
        self.document.objects.values().any(|o| {
            matches!(o, ObjectType::Scale(s)
                if !s.is_temporary
                    && !s.name.contains('|')
                    && !s.name.to_ascii_uppercase().ends_with("_XREF"))
        })
    }

    /// If the drawing has no annotation scales of its own, populate it with the
    /// standard fallback set as real `Scale` objects (and the `ACAD_SCALELIST`
    /// dictionary) so they can be selected, edited and renamed uniformly with a
    /// drawing that ships its own list. Returns `true` if any were created.
    ///
    /// The scale manager calls this on open and stages the result, so the set
    /// is discarded again unless the user applies an edit.
    ///
    /// `scale_handle_ensuring` calls it for real, before it writes the first
    /// scale a drawing has ever owned: one scale on its own would end the
    /// substitution and leave the drawing holding a list of one.
    pub fn ensure_real_scale_list(&mut self) -> bool {
        if self.has_own_scales() {
            return false;
        }
        self.add_default_scales();
        true
    }

    /// The drawing's insertion units changed from `from` to what the header
    /// now holds. The standard reducing scales stored in `from`'s unit (a
    /// metre drawing's 1:50 as 1 = 0.05) are converted to the new one, so a
    /// metre drawing switched to millimetres draws 1:50 annotations at 50x,
    /// not 0.05x (#1939). A scale edited away from that stored ratio keeps
    /// its own. The annotation multiplier follows.
    pub fn insertion_units_changed(&mut self, from: i16) {
        let to = self.document.header.insertion_units;
        self.document.header.insertion_units = from;
        let (old_factor, old_scales) = (self.default_scale_unit_factor(), self.default_scales());
        self.document.header.insertion_units = to;
        let (new_factor, new_scales) = (self.default_scale_unit_factor(), self.default_scales());
        let mut converted = false;
        if std::ptr::eq(old_scales, new_scales) && (old_factor - new_factor).abs() > 1e-12 {
            for object in self.document.objects.values_mut() {
                let ObjectType::Scale(scale) = object else {
                    continue;
                };
                let Some(&(label, factor)) = new_scales
                    .iter()
                    .find(|(label, _)| label.eq_ignore_ascii_case(&scale.name))
                else {
                    continue;
                };
                // The same paper and drawing sides `add_default_scales` wrote.
                let (paper, drawing) = label
                    .split_once(':')
                    .and_then(|(p, d)| {
                        Some((p.trim().parse::<f64>().ok()?, d.trim().parse::<f64>().ok()?))
                    })
                    .filter(|&(p, d)| p > 0.0 && d > 0.0)
                    .unwrap_or((1.0, 1.0 / factor));
                let stored = drawing / old_factor;
                if paper > drawing
                    || (scale.paper_units - paper).abs() > 1e-9 * paper
                    || (scale.drawing_units - stored).abs() > 1e-9 * stored.max(1.0)
                {
                    continue;
                }
                scale.drawing_units = drawing / new_factor;
                converted = true;
            }
        }
        if converted {
            self.bump_scale_epoch();
            let current = self.document.header.current_annotation_scale.clone();
            if self.set_annotation_scale_named(&current).is_some() {
                return;
            }
        }
        self.sync_annotation_scale_from_header();
        self.invalidate_annotation_dependencies();
    }

    /// Add the standard scale set, skipping names the drawing already has.
    pub(crate) fn add_default_scales(&mut self) {
        let unit_factor = self.default_scale_unit_factor();
        for &(label, factor) in self.default_scales() {
            // Fallback labels are "paper:drawing"; parse them for tidy whole
            // units, else derive drawing units from the factor (paper = 1).
            // The drawing side is in the drawing's own unit.
            let (paper, drawing) = label
                .split_once(':')
                .and_then(|(p, d)| {
                    Some((p.trim().parse::<f64>().ok()?, d.trim().parse::<f64>().ok()?))
                })
                .filter(|&(p, d)| p > 0.0 && d > 0.0)
                .unwrap_or((1.0, 1.0 / factor));
            // An enlarging scale ("2:1", "10:1") keeps its ratio as it is,
            // as the reference's new drawing stores it; the reducing ones are
            // converted to the drawing's unit.
            let drawing = if paper > drawing {
                drawing
            } else {
                drawing / unit_factor
            };
            self.add_scale(label, paper, drawing);
        }
        self.bump_scale_epoch();
    }

    /// `-SCALELISTEDIT` Reset: every scale except `keep` goes, then the
    /// standard set is added after the kept ones.
    pub fn reset_scale_list(&mut self, keep: &[String]) {
        let imperial = self.prefers_imperial_scales() == Some(true);
        self.reset_scale_list_to(keep, !imperial, imperial);
    }

    /// Reset to the metric list, the imperial one or both (the Edit
    /// Drawing Scales window asks which), keeping the names in `keep`.
    pub fn reset_scale_list_to(&mut self, keep: &[String], metric: bool, imperial: bool) {
        // The drawing's unit scale stays with the scales in use.
        let unit_scales = self.unit_scale_names();
        let names = self.scale_names_in_list_order();
        for name in names {
            let kept = keep.iter().any(|kept| kept.eq_ignore_ascii_case(&name))
                || unit_scales
                    .iter()
                    .any(|unit| unit.eq_ignore_ascii_case(&name));
            if !kept {
                self.remove_scale(&name);
            }
        }
        // The unit scale follows the scales in use (measured).
        for unit in &unit_scales {
            while self.move_scale(unit, 1) {}
        }
        match (metric, imperial) {
            (true, false) => self.add_metric_reset_scales(),
            // Imperial alone: 1:1 and the architectural set, inches to
            // twelve (a foot), as the reference's list stores them.
            (false, true) => {
                self.add_scale("1:1", 1.0, 1.0);
                for &(label, factor) in Self::DEFAULT_IMPERIAL_SCALES {
                    self.add_scale(label, factor * 12.0, 12.0);
                }
            }
            // Both: the civil (engineering) inch-to-feet set, then the metric
            // ratios.
            (true, true) => {
                self.add_scale("1\" = 1\"", 1.0, 1.0 / 12.0);
                for feet in Self::CIVIL_IMPERIAL_FEET {
                    self.add_scale(&format!("1\" = {feet}'"), 1.0, f64::from(*feet));
                }
                for &(name, paper, drawing) in Self::STANDARD_METRIC_RATIOS {
                    self.add_scale(name, paper, drawing);
                }
            }
            (false, false) => {}
        }
        self.bump_scale_epoch();
    }

    /// The feet one inch of paper shows in the civil imperial scale list.
    const CIVIL_IMPERIAL_FEET: &'static [u32] = &[
        5, 10, 20, 30, 40, 50, 60, 70, 75, 80, 100, 150, 200, 250, 500, 1000, 2000, 5000,
    ];

    /// The drawing's unit scale: the 1 : 1 entry named by the millimetres one
    /// drawing unit holds ("1:1000" in a metre drawing), as the metric list
    /// adds it. Any 1 : 1 scale flags itself, so the flag alone cannot tell.
    pub fn unit_scale_names(&self) -> Vec<String> {
        let name = Self::unit_scale_name_for(self.default_scale_unit_factor());
        self.scale_paper_drawing(&name)
            .filter(|(paper, drawing)| (paper - drawing).abs() < 1.0e-9)
            .map(|_| name)
            .into_iter()
            .collect()
    }

    pub(crate) fn unit_scale_name_for(unit: f64) -> String {
        let text = format!("{unit:.6}");
        format!("1:{}", text.trim_end_matches('0').trim_end_matches('.'))
    }

    pub(crate) fn add_metric_reset_scales(&mut self) {
        // The reference's metric default list: the drawing's unit scale,
        // named by the millimetres one drawing unit holds (1:1000 for
        // metres), then the standard ratios as they are; a name the list
        // keeps is not added again.
        let unit_name = Self::unit_scale_name_for(self.default_scale_unit_factor());
        if self.add_scale(&unit_name, 1.0, 1.0) {
            if let Some(handle) = self.scale_object_handle(&unit_name) {
                if let Some(ObjectType::Scale(scale)) = self.document.objects.get_mut(&handle) {
                    scale.is_unit_scale = true;
                }
            }
        }
        for &(name, paper, drawing) in Self::STANDARD_METRIC_RATIOS {
            self.add_scale(name, paper, drawing);
        }
    }

    /// Moves a scale one place up (`-1`) or down (`1`) in the drawing's list.
    pub fn move_scale(&mut self, name: &str, step: i32) -> bool {
        let Some(list) = self.scalelist_dict_handle() else {
            return false;
        };
        let Some(handle) = self.scale_object_handle(name) else {
            return false;
        };
        let Some(ObjectType::Dictionary(dictionary)) = self.document.objects.get_mut(&list) else {
            return false;
        };
        let Some(at) = dictionary.entries.iter().position(|(_, h)| *h == handle) else {
            return false;
        };
        let to = at as i64 + i64::from(step);
        if to < 0 || to as usize >= dictionary.entries.len() {
            return false;
        }
        dictionary.entries.swap(at, to as usize);
        self.bump_scale_epoch();
        true
    }

    /// The standard metric scale list, ratios as they are, in its order.
    const STANDARD_METRIC_RATIOS: &'static [(&'static str, f64, f64)] = &[
        ("1:1", 1.0, 1.0),
        ("1:2", 1.0, 2.0),
        ("1:4", 1.0, 4.0),
        ("1:5", 1.0, 5.0),
        ("1:8", 1.0, 8.0),
        ("1:10", 1.0, 10.0),
        ("1:16", 1.0, 16.0),
        ("1:20", 1.0, 20.0),
        ("1:30", 1.0, 30.0),
        ("1:40", 1.0, 40.0),
        ("1:50", 1.0, 50.0),
        ("1:100", 1.0, 100.0),
        ("2:1", 2.0, 1.0),
        ("4:1", 4.0, 1.0),
        ("8:1", 8.0, 1.0),
        ("10:1", 10.0, 1.0),
        ("100:1", 100.0, 1.0),
    ];

    /// The drawing's own scale names in the order its scale list holds them;
    /// the substituted standard set when it holds none.
    pub fn scale_names_in_list_order(&self) -> Vec<String> {
        let listed: Vec<String> = self
            .scalelist_dict_handle()
            .and_then(|handle| match self.document.objects.get(&handle) {
                Some(ObjectType::Dictionary(dictionary)) => Some(
                    dictionary
                        .entries
                        .iter()
                        .filter_map(|(_, handle)| match self.document.objects.get(handle) {
                            Some(ObjectType::Scale(scale))
                                if !scale.is_temporary
                                    && !scale.name.contains('|')
                                    && !scale.name.to_ascii_uppercase().ends_with("_XREF") =>
                            {
                                Some(scale.name.clone())
                            }
                            _ => None,
                        })
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default();
        if listed.is_empty() {
            self.scale_list()
                .into_iter()
                .map(|(name, _, _)| name)
                .collect()
        } else {
            listed
        }
    }

    /// Give an existing scale a new ratio.
    pub fn redefine_scale(&mut self, name: &str, paper: f64, drawing: f64) -> bool {
        let Some(handle) = self.scale_object_handle(name) else {
            return false;
        };
        if let Some(ObjectType::Scale(scale)) = self.document.objects.get_mut(&handle) {
            scale.paper_units = paper;
            scale.drawing_units = drawing;
        }
        self.bump_scale_epoch();
        self.invalidate_annotation_dependencies();
        true
    }

    pub fn scale_list(&self) -> Vec<(String, f32, f64)> {
        let mut list: Vec<(String, f32, f64)> = self
            .document
            .objects
            .values()
            .filter_map(|o| match o {
                // Skip xref-derived scales. Scales pulled in from an external
                // reference get an "_XREF" suffix ("1:50_XREF"); unbound
                // dependent symbols carry a "xref|name" prefix. Neither
                // belongs to this drawing's own scale list.
                ObjectType::Scale(s)
                    if !s.is_temporary
                        && !s.name.contains('|')
                        && !s.name.to_ascii_uppercase().ends_with("_XREF") =>
                {
                    let factor = self.effective_scale_factor(s);
                    Some((s.name.clone(), (1.0 / factor) as f32, factor))
                }
                _ => None,
            })
            .collect();
        list.sort_by(|a, b| a.2.total_cmp(&b.2));
        if list.is_empty() {
            // Many drawings (and minimal DXF/DWG exports) carry no
            // ACAD_SCALELIST Scale objects at all. Without a fallback the
            // annotation / viewport scale picker would be empty and so appear
            // broken. Substitute the standard ratio set — file scales still
            // win whenever the drawing actually defines any. (#154)

            let unit_factor = self.default_scale_unit_factor();
            list = self
                .default_scales()
                .iter()
                .map(|&(label, base_vp)| {
                    let vp = base_vp * unit_factor;
                    (label.to_string(), (1.0 / vp) as f32, vp)
                })
                .collect();
        }
        list
    }

    /// Scale list for the status-bar picker. Some DWGs carry both the standard
    /// metric and architectural sets in `ACAD_SCALELIST`; show only the family
    /// matching INSUNITS while retaining custom names. The complete file list
    /// remains available to the scale manager and is written back unchanged.
    pub fn scale_picker_list(&self) -> Vec<(String, f32, f64)> {
        let all = self.scale_list();
        let Some(imperial) = self.prefers_imperial_scales() else {
            return all;
        };

        let mut visible: Vec<_> = all
            .iter()
            .filter(|(name, _, _)| {
                let architectural = Self::is_architectural_scale_name(name);
                let ratio = Self::is_ratio_scale_name(name);
                if imperial {
                    architectural || (!architectural && !ratio)
                } else {
                    ratio || (!architectural && !ratio)
                }
            })
            .cloned()
            .collect();

        let has_matching_family = visible.iter().any(|(name, _, _)| {
            if imperial {
                Self::is_architectural_scale_name(name)
            } else {
                Self::is_ratio_scale_name(name)
            }
        });
        if !has_matching_family {
            let unit_factor = self.default_scale_unit_factor();

            visible.extend(self.default_scales().iter().map(|&(label, base_vp)| {
                let vp = base_vp * unit_factor;
                (label.to_string(), (1.0 / vp) as f32, vp)
            }));
        }

        // If the current scale belongs to the opposite family and has no
        // equivalent factor in the visible family, keep that one active entry
        // visible instead of silently hiding the drawing's current state.
        if let Some(active) = all.iter().find(|(name, _, _)| {
            name.eq_ignore_ascii_case(&self.document.header.current_annotation_scale)
        }) {
            let has_equivalent = visible
                .iter()
                .any(|(_, _, vp)| (vp - active.2).abs() < 0.001 * active.2.max(0.001));
            if !has_equivalent {
                visible.push(active.clone());
            }
        }

        visible.sort_by(|a, b| a.2.total_cmp(&b.2));
        visible
    }

    /// Handle of the drawing's `ACAD_SCALELIST` dictionary, located robustly.
    ///
    /// The canonical path is the root named-objects dict → `"ACAD_SCALELIST"`,
    /// but DWGs written by other programs don't always expose a resolvable root
    /// dict (the header handle points at no loaded object). In that case fall
    /// back to the owner dictionary shared by the drawing's `Scale` objects.
    /// Returns `None` only when the drawing genuinely has no scale list.
    pub(crate) fn scalelist_dict_handle(&self) -> Option<Handle> {
        use document::objects::ObjectType;
        let root_h = self.document.header.named_objects_dict_handle;
        if let Some(h) = crate::scene::annotative::as_dict(&self.document, root_h)
            .and_then(|d| d.get("ACAD_SCALELIST"))
        {
            return Some(h);
        }
        let owner = self.document.objects.values().find_map(|o| match o {
            ObjectType::Scale(s) if !s.is_temporary => Some(s.owner_handle),
            _ => None,
        })?;
        matches!(
            self.document.objects.get(&owner),
            Some(ObjectType::Dictionary(_))
        )
        .then_some(owner)
    }

    /// Millimetres in one unit of the paper side of this drawing's scale
    /// family: 25.4 for an imperial drawing (whose `1/8" = 1'-0"` style scales
    /// measure paper in inches), 1 for a metric one. Plot-scale factors built
    /// from [`Scene::scale_list`] are expressed per this unit; page setups
    /// store theirs per the layout's own paper unit, so the two are converted
    /// through this value.
    pub(crate) fn scale_family_unit_mm(&self) -> f64 {
        if self.prefers_imperial_scales() == Some(true) {
            25.4
        } else {
            1.0
        }
    }

    /// The `paper : drawing` ratio behind a scale-list name, from the
    /// drawing's own `Scale` object when it has one, otherwise from the
    /// built-in family. A ratio keeps the plot scale readable in a page setup
    /// (`1 : 250` rather than `0.004 : 1`).
    pub(crate) fn scale_ratio(&self, name: &str) -> Option<(f64, f64)> {
        use document::objects::ObjectType;
        let stored = self.document.objects.values().find_map(|o| match o {
            ObjectType::Scale(s) if !s.is_temporary && s.name.eq_ignore_ascii_case(name) => {
                Some((s.paper_units, s.drawing_units))
            }
            _ => None,
        });
        stored
            .or_else(|| Self::parse_scale_name_ratio(name))
            .filter(|(paper, drawing)| *paper > 0.0 && *drawing > 0.0)
    }

    /// The `paper : drawing` ratio spelled by a scale name: `1:250` gives
    /// `(1, 250)`, an architectural `1/8" = 1'-0"` gives `(0.125, 12)` in
    /// inches. Anything else is not a scale name.
    pub(crate) fn parse_scale_name_ratio(name: &str) -> Option<(f64, f64)> {
        if let Some((paper, drawing)) = name.split_once(':') {
            let paper: f64 = paper.trim().parse().ok()?;
            let drawing: f64 = drawing.trim().parse().ok()?;
            return (paper > 0.0 && drawing > 0.0).then_some((paper, drawing));
        }
        let (paper, drawing) = name.split_once('=')?;
        let paper = Self::parse_feet_inches(paper)?;
        let drawing = Self::parse_feet_inches(drawing)?;
        (paper > 0.0 && drawing > 0.0).then_some((paper, drawing))
    }

    /// Inches in a `F'-I"` / `I"` / `F'` length, where the inch part may be a
    /// fraction (`3/32"`).
    pub(crate) fn parse_feet_inches(text: &str) -> Option<f64> {
        let text = text.trim();
        let (feet, inches) = match text.split_once('\'') {
            Some((feet, rest)) => (
                feet.trim().parse::<f64>().ok()?,
                rest.trim_start_matches('-').trim(),
            ),
            None => (0.0, text),
        };
        let inches = inches.trim_end_matches('"').trim();
        let inches = if inches.is_empty() {
            0.0
        } else if let Some((num, den)) = inches.split_once('/') {
            let den: f64 = den.trim().parse().ok()?;
            if den <= 0.0 {
                return None;
            }
            num.trim().parse::<f64>().ok()? / den
        } else {
            inches.parse::<f64>().ok()?
        };
        Some(feet * 12.0 + inches)
    }

    /// Handle of the real (non-temporary) `Scale` object with this display name.
    /// Scales are matched by their `name` field, not the dictionary key — some
    /// files key the entries by an internal identifier (`*A1`, `A0`, …) that
    /// bears no relation to the scale name.
    pub(crate) fn scale_object_handle(&self, name: &str) -> Option<Handle> {
        use document::objects::ObjectType;
        self.document.objects.iter().find_map(|(h, o)| match o {
            ObjectType::Scale(s) if !s.is_temporary && s.name.eq_ignore_ascii_case(name) => {
                Some(*h)
            }
            _ => None,
        })
    }

    /// Resolve the current annotation scale (CANNOSCALE) to a real `Scale`
    /// object handle, materializing the object from the scale list when the
    /// drawing names the scale but has no `Scale` object for it. Returns `None`
    /// when there is no current annotation scale. Used when giving an object a
    /// per-object annotation context (the context's `340` must point at a real
    /// `AcDbScale`).
    pub(crate) fn current_annotation_scale_handle(&mut self) -> Option<Handle> {
        let name = self.document.header.current_annotation_scale.clone();
        if name.trim().is_empty() {
            return None;
        }
        self.scale_handle_ensuring(&name)
    }

    pub(crate) fn paper_annotation_scale_handle(&self) -> Option<Handle> {
        self.scale_object_handle("1:1").or_else(|| {
            self.document
                .objects
                .iter()
                .find_map(|(handle, object)| match object {
                    ObjectType::Scale(scale)
                        if !scale.is_temporary && (scale.factor() - 1.0).abs() <= 1.0e-9 =>
                    {
                        Some(*handle)
                    }
                    _ => None,
                })
        })
    }

    pub(crate) fn creation_annotation_scale_handle(&mut self) -> Option<Handle> {
        if self.current_layout == "Model" {
            return self.current_annotation_scale_handle();
        }
        if let Some(viewport) = self.active_viewport {
            if let Some(scale) = self.viewport_scale_handle(viewport) {
                return Some(scale);
            }
        }
        if let Some(scale) = self.paper_annotation_scale_handle() {
            return Some(scale);
        }
        self.scale_handle_ensuring("1:1")
    }

    pub(crate) fn creation_annotation_multiplier(&self) -> f64 {
        if self.current_layout == "Model" {
            return self.annotation_scale.max(1.0e-9) as f64;
        }
        let Some(handle) = self.active_viewport else {
            return 1.0;
        };
        let Some(EntityType::Viewport(viewport)) = self.document.get_entity(handle) else {
            return 1.0;
        };
        let paper_to_model =
            vp_effective_scale(viewport.custom_scale, viewport.view_height, viewport.height);
        if paper_to_model.is_finite() && paper_to_model > 1.0e-9 {
            1.0 / paper_to_model
        } else {
            1.0
        }
    }

    pub fn set_annotation_scale_named(&mut self, name: &str) -> Option<Handle> {
        let handle = self.scale_handle_ensuring(name)?;

        let (scale_name, scale_factor, multiplier) = {
            let ObjectType::Scale(scale) = self.document.objects.get(&handle)? else {
                return None;
            };

            let factor = scale.factor();
            (scale.name.clone(), factor, 1.0 / factor)
        };

        self.annotation_scale = multiplier as f32;
        self.document.header.current_annotation_scale = scale_name;
        self.document.header.annotation_scale_value = scale_factor;
        self.invalidate_annotation_dependencies();

        Some(handle)
    }

    pub(crate) fn current_layout_object_handle(&self) -> Option<Handle> {
        self.document
            .objects
            .iter()
            .find_map(|(handle, object)| match object {
                ObjectType::Layout(layout)
                    if layout.name.eq_ignore_ascii_case(&self.current_layout) =>
                {
                    Some(*handle)
                }
                _ => None,
            })
    }

    pub fn annotation_all_visible(&self) -> bool {
        use document::objects::XRecordValue;
        use document::xdata::XDataValue;
        // A value set here for this layout comes first. Past that, Model reads
        // the drawing's ANNOALLVISIBLE and is on without one. A paper layout
        // keeps its own value in the layout's xdata, and only an "on": it has
        // nothing to read otherwise, and is off.
        let layout = self.current_layout_object_handle();
        let set_here = layout
            .and_then(|layout| self.document.xrecord(layout, "OPEN_CAD_ANNOTATION_STATE"))
            .and_then(|record| {
                record.entries.iter().find_map(|entry| match entry.value {
                    XRecordValue::Bool(value) if entry.code == 290 => Some(value),
                    _ => None,
                })
            });
        if let Some(value) = set_here {
            return value;
        }
        if self.current_layout == "Model" {
            return self
                .document
                .drawing_variable("ANNOALLVISIBLE")
                .and_then(|value| value.trim().parse::<i32>().ok())
                .is_none_or(|value| value != 0);
        }
        layout
            .and_then(|layout| self.document.object_xdata.get(&layout))
            .and_then(|xdata| xdata.get_record(LAYOUT_ANNOTATION_VISIBILITY_APP))
            .is_some_and(|record| {
                record
                    .values
                    .iter()
                    .any(|value| matches!(value, XDataValue::Integer16(value) if *value != 0))
            })
    }

    /// MSLTSCALE: whether model-space linetypes follow the annotation scale.
    pub fn model_linetype_scaling(&self) -> bool {
        annotative::model_linetypes_follow_annotation(&self.document)
    }

    /// Set MSLTSCALE. The shader reads it every frame; complex linetypes have
    /// it baked into their wires, so those are re-tessellated.
    pub fn set_model_linetype_scaling(&mut self, on: bool) {
        crate::io::set_drawing_variable(
            &mut self.document,
            "MSLTSCALE",
            if on { "1" } else { "0" },
        );
        self.invalidate_annotation_dependencies();
    }

    pub fn set_annotation_all_visible(&mut self, value: bool) {
        use document::objects::{XRecordEntry, XRecordValue};
        let Some(layout) = self.current_layout_object_handle() else {
            return;
        };
        self.document
            .ensure_xrecord(layout, "OPEN_CAD_ANNOTATION_STATE");
        if let Some(record) = self
            .document
            .xrecord_mut(layout, "OPEN_CAD_ANNOTATION_STATE")
        {
            if let Some(entry) = record.entries.iter_mut().find(|entry| entry.code == 290) {
                entry.value = XRecordValue::Bool(value);
            } else {
                record.entries.push(XRecordEntry::bool(290, value));
            }
        }
        self.write_annotation_visibility_to_drawing(layout, value);
        self.invalidate_annotation_dependencies();
    }

    /// Store the value where the file format keeps it, so it stays with the
    /// drawing outside this program: see [`Self::annotation_all_visible`].
    pub(crate) fn write_annotation_visibility_to_drawing(&mut self, layout: Handle, value: bool) {
        use document::xdata::{ExtendedDataRecord, XDataValue};
        if self.current_layout == "Model" {
            let value = if value { "1" } else { "0" };
            crate::io::set_drawing_variable(&mut self.document, "ANNOALLVISIBLE", value);
            return;
        }
        if value {
            let _ = self.document.app_ids.add(document::tables::AppId::new(
                LAYOUT_ANNOTATION_VISIBILITY_APP,
            ));
            let mut record = ExtendedDataRecord::new(LAYOUT_ANNOTATION_VISIBILITY_APP);
            record.add_value(XDataValue::Integer16(1));
            let xdata = self.document.object_xdata.entry(layout).or_default();
            xdata.remove_record(LAYOUT_ANNOTATION_VISIBILITY_APP);
            xdata.add_record(record);
        } else if let Some(xdata) = self.document.object_xdata.get_mut(&layout) {
            xdata.remove_record(LAYOUT_ANNOTATION_VISIBILITY_APP);
            if xdata.is_empty() {
                self.document.object_xdata.remove(&layout);
            }
        }
    }

    pub fn add_annotation_scale_to_objects(
        &mut self,
        scale: Handle,
        previous_scale: Option<Handle>,
        mode: u8,
    ) -> usize {
        if previous_scale == Some(scale) || !(1..=4).contains(&mode) {
            return 0;
        }
        let viewport_frozen: HashSet<Handle> = self
            .target_viewport_handle()
            .and_then(|handle| match self.document.get_entity(handle) {
                Some(EntityType::Viewport(viewport)) => {
                    Some(viewport.frozen_layers.iter().copied().collect())
                }
                _ => None,
            })
            .unwrap_or_default();
        let handles: Vec<_> = self
            .document
            .entities()
            .filter(|entity| {
                if !crate::scene::annotative::is_annotative(&self.document, entity) {
                    return false;
                }
                let memberships = crate::scene::annotative::object_scale_memberships(
                    &self.document,
                    entity.common().handle,
                );
                if !previous_scale
                    .is_some_and(|current| memberships.iter().any(|(_, member)| *member == current))
                {
                    return false;
                }
                let layer = self.document.layers.get(&entity.common().layer);
                let (off, frozen, locked, viewport_frozen) = layer
                    .map(|layer| {
                        (
                            layer.flags.off,
                            layer.flags.frozen,
                            layer.flags.locked,
                            viewport_frozen.contains(&layer.handle),
                        )
                    })
                    .unwrap_or((false, false, false, false));
                match mode {
                    1 => !(off || frozen || locked || viewport_frozen),
                    2 => !(off || frozen || viewport_frozen),
                    3 => !locked,
                    4 => true,
                    _ => false,
                }
            })
            .map(|entity| entity.common().handle)
            .collect();
        let mut added = 0;
        for handle in handles {
            let existed =
                crate::scene::annotative::object_scale_memberships(&self.document, handle)
                    .iter()
                    .any(|(_, member)| *member == scale);
            if !existed
                && crate::scene::annotative::create_annotation_context(
                    &mut self.document,
                    handle,
                    scale,
                )
            {
                added += 1;
            }
        }
        if added > 0 {
            self.bump_geometry();
        }
        added
    }

    /// For an annotative TEXT/MTEXT, its stored height per unit of paper text
    /// height at the displayed scale (the stored height is the model height
    /// of its default representation). `None` for anything else.
    pub(crate) fn annotative_text_height_factor(&self, handle: Handle) -> Option<f64> {
        let entity = self.document.get_entity(handle)?;
        if !matches!(
            entity,
            EntityType::Text(_) | EntityType::MText(_) | EntityType::AttributeDefinition(_)
        ) || !crate::scene::annotative::is_annotative(&self.document, entity)
        {
            return None;
        }
        // The representation shown: the current scale's, else the object's
        // default one (a scale the object lacks shows it at that scale).
        let scale = crate::scene::annotative::active_object_context_for_scale(
            &self.document,
            handle,
            self.displayed_annotation_scale_handle(),
        )
        .map(|context| context.scale)
        .or_else(|| self.displayed_annotation_scale_handle());
        let model_per_paper = scale
            .and_then(|scale| match self.document.objects.get(&scale) {
                Some(ObjectType::Scale(scale)) => Some(scale.inverse_factor()),
                _ => None,
            })
            .unwrap_or(self.annotation_scale as f64);
        let ratio = crate::scene::annotative::text_context_ratio(&self.document, handle, scale)
            .unwrap_or(1.0);
        Some(model_per_paper / ratio)
    }

    /// For an annotative hatch, the model units per paper unit of the
    /// representation it shows (the current scale's, else its default
    /// one). Its Properties give the pattern scale on paper.
    pub(crate) fn annotative_hatch_model_per_paper(&self, handle: Handle) -> Option<f64> {
        let entity = self.document.get_entity(handle)?;
        if !matches!(entity, EntityType::Hatch(_))
            || !crate::scene::annotative::is_annotative(&self.document, entity)
        {
            return None;
        }
        let scale = crate::scene::annotative::active_object_context_for_scale(
            &self.document,
            handle,
            self.displayed_annotation_scale_handle(),
        )
        .map(|context| context.scale)
        .or_else(|| self.displayed_annotation_scale_handle())?;
        match self.document.objects.get(&scale) {
            Some(ObjectType::Scale(scale)) => {
                let factor = self.effective_scale_factor(scale);
                (factor > 1.0e-12).then(|| 1.0 / factor)
            }
            _ => None,
        }
    }

    pub(crate) fn displayed_annotation_scale_handle(&self) -> Option<Handle> {
        if self.current_layout == "Model" {
            return self.scale_object_handle(&self.document.header.current_annotation_scale);
        }
        self.explicit_viewport_handle()
            .and_then(|viewport| self.viewport_scale_handle(viewport))
            .or_else(|| self.paper_annotation_scale_handle())
    }

    /// Resolve a named annotation scale to a real `Scale` object handle,
    /// materializing the object from the scale list when the drawing names the
    /// scale (e.g. a virtual fallback scale) but has no `Scale` object for it.
    pub(crate) fn scale_handle_ensuring(&mut self, name: &str) -> Option<Handle> {
        if let Some(h) = self.scale_object_handle(name) {
            return Some(h);
        }
        // A drawing that owns no scales is being shown the standard set that
        // `scale_list` substitutes for an empty one. Writing only the scale
        // asked for would give it a list of its own — of exactly one entry —
        // and the substitution would stop, so every other scale would vanish
        // from the picker the moment anything needed a real one. Bring the
        // whole set in instead, which is what the drawing was already being
        // shown. (#666, #683)
        if self.ensure_real_scale_list() {
            if let Some(h) = self.scale_object_handle(name) {
                return Some(h);
            }
        }

        let fallback = self
            .default_scales()
            .iter()
            .find(|(label, _)| label.eq_ignore_ascii_case(name))
            .map(|(_, factor)| (1.0, 1.0 / factor / self.default_scale_unit_factor()))
            .or_else(|| {
                let (paper, drawing) = name.split_once(':')?;
                let paper = paper.trim().parse::<f64>().ok()?;
                let drawing = drawing.trim().parse::<f64>().ok()?;
                (paper > 0.0 && drawing > 0.0).then_some((paper, drawing))
            })
            .or_else(|| {
                let drawing = name.trim().parse::<f64>().ok()?;
                (drawing > 0.0).then_some((1.0, drawing))
            })?;
        let (paper, drawing) = self.scale_paper_drawing(name).unwrap_or(fallback);
        // `add_scale` already bumps `scale_epoch`; no extra bump here.
        self.add_scale(name, paper, drawing);
        self.scale_object_handle(name)
    }

    /// Add a named annotation scale to the drawing's `ACAD_SCALELIST`. Returns
    /// `false` if a scale with that name already exists. Creates the SCALELIST
    /// dictionary (and registers it in the root dictionary) when the drawing
    /// has none of its own — many minimal files carry no scale list at all.
    pub fn add_scale(&mut self, name: &str, paper: f64, drawing: f64) -> bool {
        use document::objects::{Dictionary, ObjectType, Scale};
        // Reject a duplicate by the scale's display name (the dictionary key may
        // be an unrelated internal identifier, so check the objects directly).
        if self.scale_object_handle(name).is_some() {
            return false;
        }
        let scalelist_h = match self.scalelist_dict_handle() {
            Some(h) => h,
            None => {
                // Resolve (or synthesise) the root robustly — a drawing whose
                // header root pointer is unresolvable would otherwise register
                // the new dictionary against a missing root and silently orphan
                // it. See `annotative::root_named_dict_handle`.
                let root_h = crate::scene::annotative::root_named_dict_handle(&mut self.document);
                let h = self.document.allocate_handle();
                let mut d = Dictionary::new();
                d.handle = h;
                d.owner = root_h;
                self.document.objects.insert(h, ObjectType::Dictionary(d));
                if let Some(ObjectType::Dictionary(root)) = self.document.objects.get_mut(&root_h) {
                    root.add_entry("ACAD_SCALELIST", h);
                }
                h
            }
        };
        let sh = self.document.allocate_handle();
        let mut s = Scale::new(name, paper, drawing);
        s.handle = sh;
        s.owner_handle = scalelist_h;
        self.document.objects.insert(sh, ObjectType::Scale(s));
        if let Some(ObjectType::Dictionary(sl)) = self.document.objects.get_mut(&scalelist_h) {
            sl.add_entry(name, sh);
        }
        self.bump_scale_epoch();
        true
    }

    /// How many annotative objects reference the named scale. Counts the
    /// object's own scale contexts; the current scale is handled separately by
    /// the command. Used to refuse a destructive delete.
    pub fn scale_in_use_count(&self, name: &str) -> usize {
        let Some(handle) = self.scale_object_handle(name) else {
            return 0;
        };
        self.document
            .entities()
            .filter(|entity| {
                crate::scene::annotative::object_scale_memberships(
                    &self.document,
                    entity.common().handle,
                )
                .iter()
                .any(|(_, member)| *member == handle)
            })
            .count()
    }

    /// Remove a named annotation scale from the drawing's `ACAD_SCALELIST`
    /// (both the `Scale` object and its dictionary entry). Returns `false` if
    /// no such scale exists. The current annotation scale should not be removed
    /// — the caller guards that.
    pub fn remove_scale(&mut self, name: &str) -> bool {
        use document::objects::ObjectType;
        let Some(sh) = self.scale_object_handle(name) else {
            return false;
        };
        if self
            .document
            .header
            .current_annotation_scale
            .eq_ignore_ascii_case(name)
        {
            return false;
        }
        let entity_handles: Vec<_> = self
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .collect();
        for entity in entity_handles {
            crate::scene::annotative::remove_annotation_context_for_scale(
                &mut self.document,
                entity,
                sh,
            );
        }
        let replacement = self
            .document
            .objects
            .iter()
            .find_map(|(handle, object)| match object {
                ObjectType::Scale(scale) if *handle != sh && !scale.is_temporary => Some(*handle),
                _ => None,
            });
        let viewports: Vec<_> = self
            .document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Viewport(viewport)
                    if self
                        .document
                        .viewport_annotation_scale(viewport.common.handle)
                        == Some(sh) =>
                {
                    Some(viewport.common.handle)
                }
                _ => None,
            })
            .collect();
        for viewport in viewports {
            if let Some(replacement) = replacement {
                self.document
                    .set_viewport_annotation_scale(viewport, replacement);
            } else if let Some(record) = self
                .document
                .xrecord_mut(viewport, "ASDK_XREC_ANNOTATION_SCALE_INFO")
            {
                record.entries.retain(|entry| entry.code != 340);
            }
        }
        // Resolve the dictionary before removing the object (the fallback lookup
        // reads a surviving scale's owner).
        let dict_h = self.scalelist_dict_handle();
        self.document.objects.remove(&sh);
        if let Some(dict_h) = dict_h {
            if let Some(ObjectType::Dictionary(sl)) = self.document.objects.get_mut(&dict_h) {
                // Match by handle — the entry key is not necessarily the name.
                sl.entries.retain(|(_, h)| *h != sh);
            }
        }
        self.invalidate_annotation_dependencies();
        self.bump_scale_epoch();
        true
    }

    /// Rename `old_name` and/or change its paper:drawing ratio. Returns `false`
    /// if the scale is missing or the new name collides with another scale.
    pub fn edit_scale(&mut self, old_name: &str, new_name: &str, paper: f64, drawing: f64) -> bool {
        use document::objects::ObjectType;
        let Some(sh) = self.scale_object_handle(old_name) else {
            return false;
        };
        // A new name that collides with a *different* scale object is rejected.
        if !new_name.eq_ignore_ascii_case(old_name) {
            if let Some(other) = self.scale_object_handle(new_name) {
                if other != sh {
                    return false;
                }
            }
        }
        if let Some(ObjectType::Scale(s)) = self.document.objects.get_mut(&sh) {
            s.name = new_name.to_string();
            s.paper_units = paper;
            s.drawing_units = drawing;
            s.is_unit_scale = (paper - drawing).abs() < 1e-10;
        }
        // Keep the owning dictionary's entry key in sync, matched by handle.
        if let Some(dict_h) = self.scalelist_dict_handle() {
            if let Some(ObjectType::Dictionary(sl)) = self.document.objects.get_mut(&dict_h) {
                if let Some(e) = sl.entries.iter_mut().find(|(_, h)| *h == sh) {
                    e.0 = new_name.to_string();
                }
            }
        }
        self.bump_scale_epoch();
        true
    }

    /// The (paper_units, drawing_units) of a named scale, for the editor.
    pub fn scale_paper_drawing(&self, name: &str) -> Option<(f64, f64)> {
        self.document.objects.values().find_map(|o| match o {
            ObjectType::Scale(s) if !s.is_temporary && s.name.eq_ignore_ascii_case(name) => {
                Some((s.paper_units, s.drawing_units))
            }
            _ => None,
        })
    }

    /// List of user viewports in the current layout: (handle, label, frozen_layer_handles).
    pub fn viewport_list(&self) -> Vec<crate::ui::window::layers::ViewportLayers> {
        if self.current_layout == "Model" {
            return vec![];
        }
        let (layout_block, _, content) = self.paper_viewport_handles();
        if layout_block.is_null() {
            return vec![];
        }
        let mut result: Vec<(document::Handle, String, Vec<document::Handle>)> = content
            .iter()
            .filter_map(|handle| {
                let Some(EntityType::Viewport(vp)) = self.document.get_entity(*handle) else {
                    return None;
                };
                Some((vp.common.handle, vp.id, vp.frozen_layers.clone()))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
            .map(|(i, (h, id, frozen))| {
                let label = if id > 1 {
                    format!("VP {}", id - 1)
                } else {
                    format!("VP {}", i + 1)
                };
                (h, label, frozen)
            })
            .collect();
        result.sort_by_key(|(_, label, _)| label.clone());
        result
            .into_iter()
            .map(|(handle, label, frozen)| {
                let colors = self
                    .document
                    .layers
                    .iter()
                    .filter_map(|layer| {
                        self.document
                            .layer_viewport_overrides(
                                layer.handle,
                                document::objects::KnownXRecordKind::LayerViewportColorOverride,
                            )
                            .into_iter()
                            .find(|(viewport, _)| *viewport == handle)
                            .and_then(|(_, value)| match value {
                                document::objects::XRecordValue::Int32(value) => Some((
                                    layer.handle,
                                    crate::scene::view::render::layer_override_color(value),
                                )),
                                _ => None,
                            })
                    })
                    .collect();
                let overridden = self
                    .document
                    .layers
                    .iter()
                    .filter(|layer| {
                        [
                            document::objects::KnownXRecordKind::LayerViewportColorOverride,
                            document::objects::KnownXRecordKind::LayerViewportLinetypeOverride,
                            document::objects::KnownXRecordKind::LayerViewportLineweightOverride,
                            document::objects::KnownXRecordKind::LayerViewportAlphaOverride,
                        ]
                        .into_iter()
                        .any(|kind| {
                            self.document
                                .layer_viewport_overrides(layer.handle, kind)
                                .into_iter()
                                .any(|(viewport, _)| viewport == handle)
                        })
                    })
                    .map(|layer| layer.handle)
                    .collect();
                crate::ui::window::layers::ViewportLayers {
                    handle,
                    label,
                    frozen,
                    colors,
                    overridden,
                }
            })
            .collect()
    }

    /// Count of user viewports (id > 1) in the current layout.
    pub fn viewport_count(&self) -> usize {
        if self.current_layout == "Model" {
            return 0;
        }
        let (layout_block, _, content) = self.paper_viewport_handles();
        if layout_block.is_null() {
            return 0;
        }
        content.len()
    }

    /// True when ISOLATEOBJECTS or HIDEOBJECTS has an active session filter.
    pub fn is_isolation_active(&self) -> bool {
        self.object_isolation.is_active()
    }

}
