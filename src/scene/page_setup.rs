//! Named page setups — the reusable plot configurations a drawing stores in
//! its `ACAD_PLOTSETTINGS` dictionary (each a standalone `PlotSettings` object
//! keyed by name). These are distinct from a layout's own embedded plot
//! settings: a named setup can be applied to any layout. CRUD here keeps the
//! dictionary + objects consistent so the result round-trips through DWG/DXF.

use super::Scene;
use acadrust::objects::{Dictionary, Layout, ObjectType, PlotSettings};
use acadrust::{CadDocument, Handle};

/// Give a freshly created paper layout the page setup a new drawing starts
/// with — ISO A4 landscape on no plotter, plotted 1:1 as a layout — stored the
/// way the file format expects it: the portrait medium under its canonical
/// name with the orientation in the rotation code. `plot_style` names the
/// plot style table to attach, or is empty.
pub fn apply_default_page_setup(layout: &mut Layout, plot_style: &str) {
    use acadrust::objects::{PlotPaperUnits, PlotRotation, PlotType, ScaledType};
    let a4 = crate::io::paper_catalog::default_paper();
    let (paper_width, paper_height) = a4.portrait_mm();
    layout.min_limits = (0.0, 0.0);
    layout.max_limits = (paper_height, paper_width);
    layout.min_extents = (0.0, 0.0, 0.0);
    layout.max_extents = (paper_height, paper_width, 0.0);
    layout.paper_width = paper_width;
    layout.paper_height = paper_height;
    layout.plot_rotation = PlotRotation::Degrees90.to_code();
    layout.plot_paper_units = PlotPaperUnits::Millimeters.to_code();
    layout.plot_type = PlotType::Layout.to_code();
    layout.plot_scale_type = ScaledType::OneToOne.to_code();
    layout.plot_scale_numerator = 1.0;
    layout.plot_scale_denominator = 1.0;
    layout.plot_scale_factor = 1.0;
    layout.plot_flags.use_standard_scale = true;
    layout.plot_flags.print_lineweights = true;
    layout.plot_flags.draw_viewports_first = true;
    layout.plot_flags.plot_plot_styles = !plot_style.is_empty();
    layout.plot_flags.show_plot_styles = !plot_style.is_empty();
    layout.plot_style_sheet = plot_style.to_string();
    layout.paper_size = a4.canonical.to_string();
    layout.plot_printer_name = crate::io::plot_device::PlotDevice::None.canonical_name();
}

/// The unprintable margins of a sheet as they appear on the rotated layout,
/// `(left, bottom, right, top)`. Stored margins belong to the medium's own
/// edges; a plot rotated by a quarter turn (`rotation` is the file's 0–3
/// code) shows the medium turned on the layout, so each margin moves to the
/// edge its side lands on — verified against limits written by the
/// commercial application: at 90° the medium's top margin is the layout's
/// left one and its left margin the layout's bottom one.
pub fn rotated_margins(
    (left, bottom, right, top): (f64, f64, f64, f64),
    rotation: i16,
) -> (f64, f64, f64, f64) {
    match rotation {
        1 => (top, left, bottom, right),
        2 => (right, top, left, bottom),
        3 => (bottom, right, top, left),
        _ => (left, bottom, right, top),
    }
}

/// Handle of a document's `ACAD_PLOTSETTINGS` dictionary, located robustly.
///
/// The canonical path is the header's `acad_plotsettings_dict_handle`, but
/// DWGs written by other programs don't always leave that pointer resolvable
/// (the header handle points at no loaded dictionary — see
/// [`crate::scene::annotative::root_named_dict_handle`]). In that case fall
/// back to the dictionary that owns the drawing's `PlotSettings` objects,
/// mirroring [`Scene::scalelist_dict_handle`]. Returns `None` when the
/// drawing genuinely has no named page setups.
fn plotsettings_dict_handle_in(document: &CadDocument) -> Option<Handle> {
    let dh = document.header.acad_plotsettings_dict_handle;
    if matches!(document.objects.get(&dh), Some(ObjectType::Dictionary(_))) {
        return Some(dh);
    }
    let owner = document.objects.values().find_map(|o| match o {
        ObjectType::PlotSettings(ps) => Some(ps.owner),
        _ => None,
    })?;
    matches!(document.objects.get(&owner), Some(ObjectType::Dictionary(_))).then_some(owner)
}

/// The named page setups of any document — `(name, settings)` in dictionary
/// order. Reads a drawing that is not open in the editor (the source of a
/// `PSETUPIN` import) the same way the editor reads its own.
pub fn document_page_setups(document: &CadDocument) -> Vec<(String, PlotSettings)> {
    let Some(dh) = plotsettings_dict_handle_in(document) else {
        return Vec::new();
    };
    let Some(ObjectType::Dictionary(dict)) = document.objects.get(&dh) else {
        return Vec::new();
    };
    dict.entries
        .iter()
        .filter_map(|(name, handle)| match document.objects.get(handle) {
            Some(ObjectType::PlotSettings(ps)) => Some((name.clone(), ps.clone())),
            _ => None,
        })
        .collect()
}

/// A page setup copied out of another drawing, made safe for this one: the
/// handles it pointed at (a plot view, a visual style) meant something only
/// in its source, so they are dropped while the names, which travel, stay.
pub fn detach_page_setup(ps: &mut PlotSettings) {
    ps.plot_view_handle = Handle::NULL;
    ps.visual_style_handle = Handle::NULL;
    ps.reactors.clear();
    ps.xdictionary_handle = None;
}

impl Scene {
    fn plotsettings_dict_handle(&self) -> Option<Handle> {
        plotsettings_dict_handle_in(&self.document)
    }

    /// Names of the document's named page setups, in dictionary order.
    pub fn page_setup_names(&self) -> Vec<String> {
        match self.plotsettings_dict_handle().and_then(|h| self.document.objects.get(&h)) {
            Some(ObjectType::Dictionary(d)) => d.entries.iter().map(|(k, _)| k.clone()).collect(),
            _ => Vec::new(),
        }
    }

    /// Bring a page setup from another drawing in under `name`, replacing
    /// one of that name the way `PSETUPIN` redefines it.
    pub fn import_page_setup(&mut self, name: &str, mut ps: PlotSettings) {
        detach_page_setup(&mut ps);
        self.page_setup_save(name, ps);
    }

    /// Clone the named page setup's `PlotSettings`, or `None` if absent.
    pub fn page_setup_get(&self, name: &str) -> Option<PlotSettings> {
        let dh = self.plotsettings_dict_handle()?;
        let ObjectType::Dictionary(d) = self.document.objects.get(&dh)? else {
            return None;
        };
        let h = d.entries.iter().find(|(k, _)| k == name).map(|(_, h)| *h)?;
        match self.document.objects.get(&h)? {
            ObjectType::PlotSettings(ps) => Some(ps.clone()),
            _ => None,
        }
    }

    /// Handle of the `ACAD_PLOTSETTINGS` dictionary, creating it (and its entry
    /// in the root named-objects dictionary) if the drawing has none yet. The
    /// root itself is resolved (or synthesised) robustly so registration
    /// persists even on drawings whose header root pointer is unresolvable.
    fn ensure_plotsettings_dict(&mut self) -> Handle {
        if let Some(dh) = self.plotsettings_dict_handle() {
            // Keep the header pointer in sync with the located dictionary so the
            // writer and later reads agree on it.
            self.document.header.acad_plotsettings_dict_handle = dh;
            return dh;
        }
        let root = crate::scene::annotative::root_named_dict_handle(&mut self.document);
        let mut dict = Dictionary::new();
        dict.handle = self.document.allocate_handle();
        dict.owner = root;
        let new_handle = dict.handle;
        self.document
            .objects
            .insert(new_handle, ObjectType::Dictionary(dict));
        self.document.header.acad_plotsettings_dict_handle = new_handle;
        if let Some(ObjectType::Dictionary(rd)) = self.document.objects.get_mut(&root) {
            rd.entries.retain(|(k, _)| k != "ACAD_PLOTSETTINGS");
            rd.entries.push(("ACAD_PLOTSETTINGS".to_string(), new_handle));
        }
        new_handle
    }

    /// Handle of the named page setup, if it exists.
    fn page_setup_handle(&self, name: &str) -> Option<Handle> {
        let dh = self.plotsettings_dict_handle()?;
        let ObjectType::Dictionary(d) = self.document.objects.get(&dh)? else {
            return None;
        };
        d.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, h)| *h)
    }

    /// The dictionary's own spelling of `name` (page-setup names compare
    /// case-insensitively, like every named-object dictionary key).
    fn page_setup_key(&self, name: &str) -> Option<String> {
        let dh = self.plotsettings_dict_handle()?;
        let ObjectType::Dictionary(d) = self.document.objects.get(&dh)? else {
            return None;
        };
        d.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(k, _)| k.clone())
    }

    /// Create or update the named page setup from `ps` (its `page_name` and
    /// `owner` are set here). Existing entries are updated in place; new ones
    /// are inserted and registered in the dictionary.
    pub fn page_setup_save(&mut self, name: &str, mut ps: PlotSettings) {
        let dict_handle = self.ensure_plotsettings_dict();
        // Replacing keeps the entry's stored spelling so the dictionary key
        // and the object's page_name stay one name.
        ps.page_name = self.page_setup_key(name).unwrap_or_else(|| name.to_string());
        ps.owner = dict_handle;
        if let Some(h) = self.page_setup_handle(name) {
            ps.handle = h;
            self.document.objects.insert(h, ObjectType::PlotSettings(ps));
        } else {
            ps.handle = self.document.allocate_handle();
            let h = ps.handle;
            self.document.objects.insert(h, ObjectType::PlotSettings(ps));
            if let Some(ObjectType::Dictionary(d)) = self.document.objects.get_mut(&dict_handle) {
                d.entries.push((name.to_string(), h));
            }
        }
    }

    /// Remove the named page setup (object + dictionary entry). No-op if absent.
    pub fn page_setup_delete(&mut self, name: &str) {
        let Some(dict_handle) = self.plotsettings_dict_handle() else {
            return;
        };
        let handle = if let Some(ObjectType::Dictionary(d)) =
            self.document.objects.get_mut(&dict_handle)
        {
            let h = d
                .entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, h)| *h);
            d.entries.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
            h
        } else {
            None
        };
        if let Some(h) = handle {
            self.document.objects.remove(&h);
        }
    }

    /// Rename a named page setup (dictionary key + the object's `page_name`).
    /// No-op if `old` is absent or `new` already exists.
    pub fn page_setup_rename(&mut self, old: &str, new: &str) {
        if old == new || new.trim().is_empty() {
            return;
        }
        // A different spelling of the same name is a rename, not a collision.
        if !old.eq_ignore_ascii_case(new) && self.page_setup_handle(new).is_some() {
            return; // name collision
        }
        let Some(dict_handle) = self.plotsettings_dict_handle() else {
            return;
        };
        let mut renamed = None;
        if let Some(ObjectType::Dictionary(d)) = self.document.objects.get_mut(&dict_handle) {
            if let Some(e) = d.entries.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(old)) {
                e.0 = new.to_string();
                renamed = Some(e.1);
            }
        }
        if let Some(h) = renamed {
            if let Some(ObjectType::PlotSettings(ps)) = self.document.objects.get_mut(&h) {
                ps.page_name = new.to_string();
            }
        }
    }
}
