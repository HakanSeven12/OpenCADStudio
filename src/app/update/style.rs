//! `style` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{
    parse_coord, polar_constrain_near, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis,
    CoordKind,
};
use crate::app::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
use crate::scene::pick::grip::{find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit};
use crate::scene::model::object::GripApply;
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::PropertiesPanel;
use acadrust::types::Color as AcadColor;
use acadrust::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};


impl OpenCADStudio {
    pub(in crate::app) fn mlstyle_mut(
        &mut self,
        tab: usize,
    ) -> Option<&mut acadrust::objects::MLineStyle> {
        use acadrust::objects::ObjectType;
        let name = self.mlstyle_selected.clone();
        self.tabs[tab]
            .scene
            .document
            .objects
            .values_mut()
            .find_map(|object| match object {
                ObjectType::MLineStyle(style) if style.name == name => Some(style),
                _ => None,
            })
    }

    pub(in crate::app) fn load_mlstyle_bufs(&mut self, tab: usize) {
        use acadrust::objects::ObjectType;
        let name = self.mlstyle_selected.clone();
        let Some((description, start_angle, end_angle, fill_color, elements)) = self.tabs[tab]
            .scene
            .document
            .objects
            .values()
            .find_map(|object| match object {
                ObjectType::MLineStyle(style) if style.name == name => Some((
                    style.description.clone(),
                    style.start_angle.to_degrees(),
                    style.end_angle.to_degrees(),
                    style.fill_color.index().unwrap_or(256),
                    style
                        .elements
                        .iter()
                        .map(|element| {
                            [
                                format!("{}", element.offset),
                                element.color.index().unwrap_or(256).to_string(),
                            element.linetype.clone(),
                            ]
                        })
                        .collect::<Vec<_>>(),
                )),
                _ => None,
            })
        else {
            return;
        };
        self.mln_description = description;
        self.mln_start_angle = format!("{start_angle:.4}");
        self.mln_end_angle = format!("{end_angle:.4}");
        self.mln_fill_color = fill_color.to_string();
        self.mln_elements = elements;
    }

    pub(in crate::app) fn stage_mlstyle_bufs(&mut self) {
        let tab = self.active_tab;
        let description = self.mln_description.clone();
        let start_angle = self.mln_start_angle.trim().parse::<f64>().ok();
        let end_angle = self.mln_end_angle.trim().parse::<f64>().ok();
        let fill_color = self.mln_fill_color.trim().parse::<i16>().ok();
        let elements = self.mln_elements.clone();
        if let Some(style) = self.mlstyle_mut(tab) {
            style.description = description;
            if let Some(value) = start_angle {
                style.start_angle = value.to_radians();
            }
            if let Some(value) = end_angle {
                style.end_angle = value.to_radians();
            }
            if let Some(value) = fill_color {
                style.fill_color = acadrust::types::Color::from_index(value);
            }
            for (element, values) in style.elements.iter_mut().zip(elements) {
                if let Ok(value) = values[0].trim().parse::<f64>() {
                    element.offset = value;
                }
                if let Ok(value) = values[1].trim().parse::<i16>() {
                    element.color = acadrust::types::Color::from_index(value);
                }
                element.linetype = values[2].clone();
            }
        }
    }

    pub(in crate::app) fn tablestyle_mut(&mut self, tab: usize) -> Option<&mut acadrust::objects::TableStyle> {
        use acadrust::objects::ObjectType;
        let name = self.tablestyle_selected.clone();
        self.tabs[tab]
            .scene
            .document
            .objects
            .values_mut()
            .find_map(|o| match o {
                ObjectType::TableStyle(s) if s.name == name => Some(s),
                _ => None,
            })
    }

    /// Mutable access to a table style's cell style by row (0=Data,1=Header,2=Title).

    pub(in crate::app) fn ts_cell_of(
        s: &mut acadrust::objects::TableStyle,
        row: u8,
    ) -> Option<&mut acadrust::objects::RowCellStyle> {
        match row {
            0 => Some(&mut s.data_row_style),
            1 => Some(&mut s.header_row_style),
            2 => Some(&mut s.title_row_style),
            _ => None,
        }
    }

    /// Mutable access to a cell's border by index
    /// (0=left 1=right 2=top 3=bottom 4=horizontal-inside 5=vertical-inside).

    pub(in crate::app) fn ts_border_of(
        c: &mut acadrust::objects::RowCellStyle,
        border: u8,
    ) -> Option<&mut acadrust::objects::TableCellBorder> {
        match border {
            0 => Some(&mut c.left_border),
            1 => Some(&mut c.right_border),
            2 => Some(&mut c.top_border),
            3 => Some(&mut c.bottom_border),
            4 => Some(&mut c.horizontal_inside_border),
            5 => Some(&mut c.vertical_inside_border),
            _ => None,
        }
    }

    /// Populate margin + per-cell edit buffers from the selected table style.

    pub(in crate::app) fn load_tablestyle_bufs(&mut self, tab: usize) {
        use acadrust::objects::ObjectType;
        let name = self.tablestyle_selected.clone();
        let Some(s) = self.tabs[tab]
            .scene
            .document
            .objects
            .values()
            .find_map(|o| match o {
                ObjectType::TableStyle(s) if s.name == name => Some(s),
                _ => None,
            })
        else {
            return;
        };
        self.ts_hmargin = format!("{:.4}", s.horizontal_margin);
        self.ts_vmargin = format!("{:.4}", s.vertical_margin);
        self.ts_description = s.description.clone();
        for (r, c) in [&s.data_row_style, &s.header_row_style, &s.title_row_style]
            .into_iter()
            .enumerate()
        {
            self.ts_cell_textstyle[r] = c.text_style_name.clone();
            self.ts_cell_height[r] = format!("{:.4}", c.text_height);
            self.ts_cell_textcolor[r] = c
                .text_color
                .index()
                .map(|v| v.to_string())
                .unwrap_or_default();
            self.ts_cell_fillcolor[r] = c
                .fill_color
                .index()
                .map(|v| v.to_string())
                .unwrap_or_default();
            self.ts_cell_datatype[r] = c.data_type.to_string();
            self.ts_cell_unittype[r] = c.unit_type.to_string();
            self.ts_cell_format[r] = c.format_string.clone();
            let borders = [
                &c.left_border,
                &c.right_border,
                &c.top_border,
                &c.bottom_border,
                &c.horizontal_inside_border,
                &c.vertical_inside_border,
            ];
            for (b, bd) in borders.into_iter().enumerate() {
                self.ts_border_lw[r][b] = bd.line_weight.value().to_string();
                self.ts_border_color[r][b] =
                    bd.color.index().map(|v| v.to_string()).unwrap_or_default();
                self.ts_border_spacing[r][b] = format!("{:.4}", bd.double_line_spacing);
            }
        }
    }

    /// Mutable access to the currently selected multileader style.

    pub(in crate::app) fn mleaderstyle_mut(&mut self, tab: usize) -> Option<&mut acadrust::objects::MultiLeaderStyle> {
        use acadrust::objects::ObjectType;
        let name = self.mleaderstyle_selected.clone();
        self.tabs[tab]
            .scene
            .document
            .objects
            .values_mut()
            .find_map(|o| match o {
                ObjectType::MultiLeaderStyle(s) if s.name == name => Some(s),
                _ => None,
            })
    }

    /// Populate all edit buffers from the currently selected multileader style.

    pub(in crate::app) fn load_mleaderstyle_bufs(&mut self, tab: usize) {
        use acadrust::objects::ObjectType;
        let name = self.mleaderstyle_selected.clone();
        let Some(s) = self.tabs[tab]
            .scene
            .document
            .objects
            .values()
            .find_map(|o| match o {
                ObjectType::MultiLeaderStyle(s) if s.name == name => Some(s),
                _ => None,
            })
        else {
            return;
        };
        self.mls_landing_distance = format!("{:.4}", s.landing_distance);
        self.mls_landing_gap = format!("{:.4}", s.landing_gap);
        self.mls_arrowhead_size = format!("{:.4}", s.arrowhead_size);
        self.mls_text_height = format!("{:.4}", s.text_height);
        self.mls_scale_factor = format!("{:.4}", s.scale_factor);
        self.mls_break_gap = format!("{:.4}", s.break_gap_size);
        self.mls_first_seg_angle = format!("{:.4}", s.first_segment_angle.to_degrees());
        self.mls_second_seg_angle = format!("{:.4}", s.second_segment_angle.to_degrees());
        self.mls_max_points = s.max_leader_points.to_string();
        self.mls_default_text = s.default_text.clone();
        self.mls_line_color = s
            .line_color
            .index()
            .map(|c| c.to_string())
            .unwrap_or_default();
        self.mls_text_color = s
            .text_color
            .index()
            .map(|c| c.to_string())
            .unwrap_or_default();
        self.mls_description = s.description.clone();
        self.mls_align_space = format!("{:.4}", s.align_space);
        self.mls_block_color = s
            .block_content_color
            .index()
            .map(|c| c.to_string())
            .unwrap_or_default();
        self.mls_block_rotation = format!("{:.4}", s.block_content_rotation.to_degrees());
        self.mls_block_scale_x = format!("{:.4}", s.block_content_scale_x);
        self.mls_block_scale_y = format!("{:.4}", s.block_content_scale_y);
        self.mls_block_scale_z = format!("{:.4}", s.block_content_scale_z);
    }

    /// Populate all edit buffers from the currently selected dim style.

    pub(in crate::app) fn load_dimstyle_bufs(&mut self, tab: usize) {
        let doc = &self.tabs[tab].scene.document;
        let Some(ds) = doc.dim_styles.get(&self.dimstyle_selected) else {
            return;
        };
        self.ds_dimdle = format!("{}", ds.dimdle);
        self.ds_dimdli = format!("{}", ds.dimdli);
        self.ds_dimgap = format!("{}", ds.dimgap);
        self.ds_dimexe = format!("{}", ds.dimexe);
        self.ds_dimexo = format!("{}", ds.dimexo);
        self.ds_dimsd1 = ds.dimsd1;
        self.ds_dimsd2 = ds.dimsd2;
        self.ds_dimse1 = ds.dimse1;
        self.ds_dimse2 = ds.dimse2;
        self.ds_dimasz = format!("{}", ds.dimasz);
        self.ds_dimcen = format!("{}", ds.dimcen);
        self.ds_dimtsz = format!("{}", ds.dimtsz);
        self.ds_dimtxt = format!("{}", ds.dimtxt);
        self.ds_dimtxsty = ds.dimtxsty.clone();
        self.ds_dimtad = format!("{}", ds.dimtad);
        self.ds_dimtih = ds.dimtih;
        self.ds_dimtoh = ds.dimtoh;
        self.ds_dimscale = format!("{}", ds.dimscale);
        self.ds_dimlfac = format!("{}", ds.dimlfac);
        self.ds_dimlunit = format!("{}", ds.dimlunit);
        self.ds_dimdec = format!("{}", ds.dimdec);
        self.ds_dimpost = ds.dimpost.clone();
        self.ds_dimtol = ds.dimtol;
        self.ds_dimlim = ds.dimlim;
        self.ds_dimtp = format!("{}", ds.dimtp);
        self.ds_dimtm = format!("{}", ds.dimtm);
        self.ds_dimtdec = format!("{}", ds.dimtdec);
        self.ds_dimtfac = format!("{}", ds.dimtfac);
        self.ds_annotative = ds.annotative;
        self.ds_dimclrd = format!("{}", ds.dimclrd);
        self.ds_dimlwd = format!("{}", ds.dimlwd);
        self.ds_dimclre = format!("{}", ds.dimclre);
        self.ds_dimlwe = format!("{}", ds.dimlwe);
        self.ds_dimfxl = format!("{}", ds.dimfxl);
        self.ds_dimfxlon = ds.dimfxlon;
        self.ds_dimsah = ds.dimsah;
        self.ds_dimarcsym = format!("{}", ds.dimarcsym);
        self.ds_dimjogang = format!("{}", ds.dimjogang.to_degrees());
        self.ds_dimclrt = format!("{}", ds.dimclrt);
        self.ds_dimjust = format!("{}", ds.dimjust);
        self.ds_dimtvp = format!("{}", ds.dimtvp);
        self.ds_dimtfill = format!("{}", ds.dimtfill);
        self.ds_dimtfillclr = format!("{}", ds.dimtfillclr);
        self.ds_dimtxtdirection = ds.dimtxtdirection;
        self.ds_dimatfit = format!("{}", ds.dimatfit);
        self.ds_dimtix = ds.dimtix;
        self.ds_dimsoxd = ds.dimsoxd;
        self.ds_dimtmove = format!("{}", ds.dimtmove);
        self.ds_dimupt = ds.dimupt;
        self.ds_dimtofl = ds.dimtofl;
        self.ds_dimfit = format!("{}", ds.dimfit);
        self.ds_dimdsep = format!("{}", ds.dimdsep);
        self.ds_dimrnd = format!("{}", ds.dimrnd);
        self.ds_dimzin = format!("{}", ds.dimzin);
        self.ds_dimfrac = format!("{}", ds.dimfrac);
        self.ds_dimaunit = format!("{}", ds.dimaunit);
        self.ds_dimadec = format!("{}", ds.dimadec);
        self.ds_dimunit = format!("{}", ds.dimunit);
        self.ds_dimazin = format!("{}", ds.dimazin);
        self.ds_dimalt = ds.dimalt;
        self.ds_dimaltf = format!("{}", ds.dimaltf);
        self.ds_dimaltd = format!("{}", ds.dimaltd);
        self.ds_dimaltu = format!("{}", ds.dimaltu);
        self.ds_dimalttd = format!("{}", ds.dimalttd);
        self.ds_dimaltrnd = format!("{}", ds.dimaltrnd);
        self.ds_dimapost = ds.dimapost.clone();
        self.ds_dimaltz = format!("{}", ds.dimaltz);
        self.ds_dimalttz = format!("{}", ds.dimalttz);
        self.ds_dimtolj = format!("{}", ds.dimtolj);
        self.ds_dimtzin = format!("{}", ds.dimtzin);
    }

    /// Write edit buffers back into the selected dim style document entry.

    pub(in crate::app) fn apply_dimstyle_bufs(&mut self, tab: usize) {
        let doc = &mut self.tabs[tab].scene.document;

        let text_style_handle = doc
            .text_styles
            .get(&self.ds_dimtxsty)
            .map(|style| style.handle)
            .unwrap_or(acadrust::types::Handle::NULL);
        let Some(ds) = doc.dim_styles.get_mut(&self.dimstyle_selected) else {
            return;
        };
        macro_rules! set_f64 {
            ($field:ident, $buf:expr) => {
                if let Ok(v) = $buf.trim().parse::<f64>() {
                    ds.$field = v;
                }
            };
        }
        macro_rules! set_i16 {
            ($field:ident, $buf:expr) => {
                if let Ok(v) = $buf.trim().parse::<i16>() {
                    ds.$field = v;
                }
            };
        }
        set_f64!(dimdle, self.ds_dimdle);
        set_f64!(dimdli, self.ds_dimdli);
        set_f64!(dimgap, self.ds_dimgap);
        set_f64!(dimexe, self.ds_dimexe);
        set_f64!(dimexo, self.ds_dimexo);
        set_f64!(dimasz, self.ds_dimasz);
        set_f64!(dimcen, self.ds_dimcen);
        set_f64!(dimtsz, self.ds_dimtsz);
        set_f64!(dimtxt, self.ds_dimtxt);
        if self.ds_annotative {
            ds.dimscale = 0.0;
            self.ds_dimscale = "0".to_string();
        } else {
            set_f64!(dimscale, self.ds_dimscale);
        }
        set_f64!(dimlfac, self.ds_dimlfac);
        set_f64!(dimtp, self.ds_dimtp);
        set_f64!(dimtm, self.ds_dimtm);
        set_f64!(dimtfac, self.ds_dimtfac);
        set_i16!(dimtad, self.ds_dimtad);
        set_i16!(dimlunit, self.ds_dimlunit);
        set_i16!(dimdec, self.ds_dimdec);
        set_i16!(dimtdec, self.ds_dimtdec);
        ds.dimsd1 = self.ds_dimsd1;
        ds.dimsd2 = self.ds_dimsd2;
        ds.dimse1 = self.ds_dimse1;
        ds.dimse2 = self.ds_dimse2;
        ds.dimtih = self.ds_dimtih;
        ds.dimtoh = self.ds_dimtoh;
        ds.dimtol = self.ds_dimtol;
        ds.dimlim = self.ds_dimlim;
        ds.dimpost = self.ds_dimpost.clone();
        ds.dimtxsty = self.ds_dimtxsty.clone();
        ds.dimtxsty_handle = text_style_handle;
        ds.annotative = self.ds_annotative;
        set_i16!(dimclrd, self.ds_dimclrd);
        set_i16!(dimlwd, self.ds_dimlwd);
        set_i16!(dimclre, self.ds_dimclre);
        set_i16!(dimlwe, self.ds_dimlwe);
        set_f64!(dimfxl, self.ds_dimfxl);
        set_i16!(dimarcsym, self.ds_dimarcsym);
        set_i16!(dimclrt, self.ds_dimclrt);
        set_i16!(dimjust, self.ds_dimjust);
        set_f64!(dimtvp, self.ds_dimtvp);
        set_i16!(dimtfill, self.ds_dimtfill);
        set_i16!(dimtfillclr, self.ds_dimtfillclr);
        set_i16!(dimatfit, self.ds_dimatfit);
        set_i16!(dimtmove, self.ds_dimtmove);
        set_i16!(dimfit, self.ds_dimfit);
        set_i16!(dimdsep, self.ds_dimdsep);
        set_f64!(dimrnd, self.ds_dimrnd);
        set_i16!(dimzin, self.ds_dimzin);
        set_i16!(dimfrac, self.ds_dimfrac);
        set_i16!(dimaunit, self.ds_dimaunit);
        set_i16!(dimadec, self.ds_dimadec);
        set_i16!(dimunit, self.ds_dimunit);
        set_i16!(dimazin, self.ds_dimazin);
        set_f64!(dimaltf, self.ds_dimaltf);
        set_i16!(dimaltd, self.ds_dimaltd);
        set_i16!(dimaltu, self.ds_dimaltu);
        set_i16!(dimalttd, self.ds_dimalttd);
        set_f64!(dimaltrnd, self.ds_dimaltrnd);
        set_i16!(dimaltz, self.ds_dimaltz);
        set_i16!(dimalttz, self.ds_dimalttz);
        set_i16!(dimtolj, self.ds_dimtolj);
        set_i16!(dimtzin, self.ds_dimtzin);
        if let Ok(v) = self.ds_dimjogang.trim().parse::<f64>() {
            ds.dimjogang = v.to_radians();
        }
        ds.dimfxlon = self.ds_dimfxlon;
        ds.dimsah = self.ds_dimsah;
        ds.dimtxtdirection = self.ds_dimtxtdirection;
        ds.dimtix = self.ds_dimtix;
        ds.dimsoxd = self.ds_dimsoxd;
        ds.dimupt = self.ds_dimupt;
        ds.dimtofl = self.ds_dimtofl;
        ds.dimalt = self.ds_dimalt;
        ds.dimapost = self.ds_dimapost.clone();

        // Same reason as a per-dimension edit: every dimension on this style is
        // drawn from a block made under the settings that just changed, so the
        // pictures are stale. Drop them and let each be drawn again. Without
        // this an edit here would move the numbers and leave the drawing alone.
        let edited = self.dimstyle_selected.clone();
        let stale: Vec<acadrust::Handle> = self.tabs[tab]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                acadrust::EntityType::Dimension(dim)
                    if dim.base().style_name.eq_ignore_ascii_case(&edited) =>
                {
                    Some(entity.common().handle)
                }
                _ => None,
            })
            .collect();
        for handle in stale {
            self.tabs[tab].scene.invalidate_dim_block_recorded(handle);
        }

        self.command_line
            .push_output(crate::tf!("DimStyle '{}' updated.", self.dimstyle_selected).as_ref());
    }

    /// Update a single string buffer field.

    pub(in crate::app) fn apply_ds_edit(&mut self, field: crate::app::DsField, val: String) {
        use crate::app::DsField::*;
        match field {
            Dimdle => self.ds_dimdle = val,
            Dimdli => self.ds_dimdli = val,
            Dimgap => self.ds_dimgap = val,
            Dimexe => self.ds_dimexe = val,
            Dimexo => self.ds_dimexo = val,
            Dimasz => self.ds_dimasz = val,
            Dimcen => self.ds_dimcen = val,
            Dimtsz => self.ds_dimtsz = val,
            Dimtxt => self.ds_dimtxt = val,
            Dimtxsty => self.ds_dimtxsty = val,
            Dimtad => self.ds_dimtad = val,
            Dimscale => self.ds_dimscale = val,
            Dimlfac => self.ds_dimlfac = val,
            Dimlunit => self.ds_dimlunit = val,
            Dimdec => self.ds_dimdec = val,
            Dimpost => self.ds_dimpost = val,
            Dimtp => {
                let symmetrical = self.ds_dimtol
                    && !self.ds_dimlim
                    && self.ds_dimtp.trim() == self.ds_dimtm.trim();
                self.ds_dimtp = val.clone();
                if symmetrical {
                    self.ds_dimtm = val;
                }
            }
            Dimtm => self.ds_dimtm = val,
            Dimtdec => self.ds_dimtdec = val,
            Dimtfac => self.ds_dimtfac = val,
            Dimclrd => self.ds_dimclrd = val,
            Dimlwd => self.ds_dimlwd = val,
            Dimclre => self.ds_dimclre = val,
            Dimlwe => self.ds_dimlwe = val,
            Dimfxl => self.ds_dimfxl = val,
            Dimarcsym => self.ds_dimarcsym = val,
            Dimjogang => self.ds_dimjogang = val,
            Dimclrt => self.ds_dimclrt = val,
            Dimjust => self.ds_dimjust = val,
            Dimtvp => self.ds_dimtvp = val,
            Dimtfill => self.ds_dimtfill = val,
            Dimtfillclr => self.ds_dimtfillclr = val,
            Dimatfit => self.ds_dimatfit = val,
            Dimtmove => self.ds_dimtmove = val,
            Dimfit => self.ds_dimfit = val,
            Dimdsep => self.ds_dimdsep = val,
            Dimrnd => self.ds_dimrnd = val,
            Dimzin => self.ds_dimzin = val,
            Dimfrac => self.ds_dimfrac = val,
            Dimaunit => self.ds_dimaunit = val,
            Dimadec => self.ds_dimadec = val,
            Dimunit => self.ds_dimunit = val,
            Dimazin => self.ds_dimazin = val,
            Dimaltf => self.ds_dimaltf = val,
            Dimaltd => self.ds_dimaltd = val,
            Dimaltu => self.ds_dimaltu = val,
            Dimalttd => self.ds_dimalttd = val,
            Dimaltrnd => self.ds_dimaltrnd = val,
            Dimapost => self.ds_dimapost = val,
            Dimaltz => self.ds_dimaltz = val,
            Dimalttz => self.ds_dimalttz = val,
            Dimtolj => self.ds_dimtolj = val,
            Dimtzin => self.ds_dimtzin = val,
            // Bool fields — no-op for string edit
            _ => {}
        }
    }

    /// Toggle a boolean buffer field.

    pub(in crate::app) fn apply_ds_toggle(&mut self, field: crate::app::DsField) {
        use crate::app::DsField::*;
        match field {
            Dimsd1 => self.ds_dimsd1 = !self.ds_dimsd1,
            Dimsd2 => self.ds_dimsd2 = !self.ds_dimsd2,
            Dimse1 => self.ds_dimse1 = !self.ds_dimse1,
            Dimse2 => self.ds_dimse2 = !self.ds_dimse2,
            Dimtih => self.ds_dimtih = !self.ds_dimtih,
            Dimtoh => self.ds_dimtoh = !self.ds_dimtoh,
            Dimtol => self.ds_dimtol = !self.ds_dimtol,
            Dimlim => self.ds_dimlim = !self.ds_dimlim,
            Annotative => {
                self.ds_annotative = !self.ds_annotative;
                if self.ds_annotative {
                    self.ds_dimscale = "0".to_string();
                }
            }
            Dimfxlon => self.ds_dimfxlon = !self.ds_dimfxlon,
            Dimsah => self.ds_dimsah = !self.ds_dimsah,
            Dimtxtdirection => self.ds_dimtxtdirection = !self.ds_dimtxtdirection,
            Dimtix => self.ds_dimtix = !self.ds_dimtix,
            Dimsoxd => self.ds_dimsoxd = !self.ds_dimsoxd,
            Dimupt => self.ds_dimupt = !self.ds_dimupt,
            Dimtofl => self.ds_dimtofl = !self.ds_dimtofl,
            Dimalt => self.ds_dimalt = !self.ds_dimalt,
            _ => {}
        }
    }

    /// Populate edit buffers from the currently selected text style.

    pub(in crate::app) fn load_textstyle_bufs(&mut self, tab: usize) {
        let doc = &self.tabs[tab].scene.document;
        if let Some(s) = doc.text_styles.get(&self.textstyle_selected) {
            self.textstyle_font = s.font_file.clone();
            self.textstyle_width = format!("{:.4}", s.width_factor);
            self.textstyle_oblique = format!("{:.2}", s.oblique_angle.to_degrees());
            self.textstyle_height = format!("{:.4}", s.height);
            self.textstyle_bigfont = s.big_font_file.clone();
            self.textstyle_ttf = s.true_type_font.clone();
        }
    }

pub(super) fn on_text_style_dialog_open(&mut self) -> Task<Message> {
                let i = self.active_tab;
                let cur = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_text_style_name
                    .clone();
                let exists = self.tabs[i].scene.document.text_styles.get(&cur).is_some();
                self.textstyle_selected = if exists {
                    cur
                } else {
                    self.tabs[i]
                        .scene
                        .document
                        .text_styles
                        .iter()
                        .next()
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "Standard".to_string())
                };
                self.load_textstyle_bufs(i);
                self.active_modal = Some(crate::app::ModalKind::TextStyle);
                self.style_stage_begin();
                Task::none()
    }

    /// Write the editor buffers into the selected text style (staged, no
    /// commit), so edits survive switching to another style as well as Apply.
    pub(super) fn stage_textstyle_bufs(&mut self) {
                let i = self.active_tab;
                let name = self.textstyle_selected.clone();
                let font = self.textstyle_font.clone();
                let width_str = self.textstyle_width.clone();
                let oblique_str = self.textstyle_oblique.clone();
                let height_str = self.textstyle_height.clone();
                let bigfont = self.textstyle_bigfont.clone();
                let ttf = self.textstyle_ttf.clone();
                if let Some(s) = self.tabs[i].scene.document.text_styles.get_mut(&name) {
                    if s.xref_dependent {
                        return;
                    }
                    s.font_file = font;
                    s.big_font_file = bigfont;
                    s.true_type_font = ttf;
                    if let Ok(w) = width_str.trim().parse::<f64>() {
                        s.width_factor = w;
                    }
                    if let Ok(a) = oblique_str.trim().parse::<f64>() {
                        s.oblique_angle = a.to_radians();
                    }
                    if let Ok(h) = height_str.trim().parse::<f64>() {
                        s.height = h.max(0.0);
                    }
                }
    }

    pub(super) fn on_text_style_apply(&mut self) -> Task<Message> {
                self.stage_textstyle_bufs();
                self.style_stage_commit();
                Task::none()
    }

    pub(super) fn on_table_style_cell_apply(&mut self, row: u8) -> Task<Message> {
                let i = self.active_tab;
                let r = row as usize;
                if r >= 3 {
                    return Task::none();
                }
                let ts = self.ts_cell_textstyle[r].trim().to_string();
                let height: Option<f64> = self.ts_cell_height[r].trim().parse().ok();
                let tc: Option<i16> = self.ts_cell_textcolor[r].trim().parse().ok();
                let fc: Option<i16> = self.ts_cell_fillcolor[r].trim().parse().ok();
                let dtype: Option<i32> = self.ts_cell_datatype[r].trim().parse().ok();
                let utype: Option<i32> = self.ts_cell_unittype[r].trim().parse().ok();
                let fmt = self.ts_cell_format[r].clone();
                // Per-border numeric edits for this cell.
                let border_vals: [(Option<i16>, Option<i16>, Option<f64>); 6] =
                    std::array::from_fn(|b| {
                        (
                            self.ts_border_lw[r][b].trim().parse().ok(),
                            self.ts_border_color[r][b].trim().parse().ok(),
                            self.ts_border_spacing[r][b].trim().parse().ok(),
                        )
                    });
                if let Some(c) = self
                    .tablestyle_mut(i)
                    .and_then(|s| Self::ts_cell_of(s, row))
                {
                    if !ts.is_empty() {
                        c.text_style_name = ts;
                    }
                    if let Some(h) = height {
                        c.text_height = h;
                    }
                    if let Some(v) = tc {
                        c.text_color = acadrust::types::Color::from_index(v);
                    }
                    if let Some(v) = fc {
                        c.fill_color = acadrust::types::Color::from_index(v);
                    }
                    if let Some(v) = dtype {
                        c.data_type = v;
                    }
                    if let Some(v) = utype {
                        c.unit_type = v;
                    }
                    c.format_string = fmt;
                    for (b, (lw, color, spacing)) in border_vals.into_iter().enumerate() {
                        if let Some(bd) = Self::ts_border_of(c, b as u8) {
                            if let Some(v) = lw {
                                bd.line_weight = acadrust::types::LineWeight::from_value(v);
                            }
                            if let Some(v) = color {
                                bd.color = acadrust::types::Color::from_index(v);
                            }
                            if let Some(v) = spacing {
                                bd.double_line_spacing = v;
                            }
                        }
                    }
                }
                Task::none()
    }

    pub(super) fn on_ml_style_dialog_open(&mut self) -> Task<Message> {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let cur = self.tabs[i].scene.document.header.multiline_style.clone();
                let exists = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .values()
                    .any(|o| matches!(o, ObjectType::MLineStyle(s) if s.name == cur));
                self.mlstyle_selected = if exists {
                    cur
                } else {
                    self.tabs[i]
                        .scene
                        .document
                        .objects
                        .values()
                        .find_map(|o| {
                            if let ObjectType::MLineStyle(s) = o {
                                Some(s.name.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| "Standard".to_string())
                };
                self.active_modal = Some(crate::app::ModalKind::MlStyle);
                self.load_mlstyle_bufs(i);
                self.style_stage_begin();
                Task::none()
    }

    pub(super) fn on_mleader_style_dialog_open(&mut self) -> Task<Message> {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let cur = self.tabs[i].active_mleader_style.clone();
                let exists = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .values()
                    .any(|o| matches!(o, ObjectType::MultiLeaderStyle(s) if s.name == cur));
                self.mleaderstyle_selected = if exists {
                    cur
                } else {
                    self.tabs[i]
                        .scene
                        .document
                        .objects
                        .values()
                        .find_map(|o| {
                            if let ObjectType::MultiLeaderStyle(s) = o {
                                Some(s.name.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| "Standard".to_string())
                };
                self.load_mleaderstyle_bufs(i);
                self.active_modal = Some(crate::app::ModalKind::MLeaderStyle);
                self.style_stage_begin();
                Task::none()
    }

    pub(super) fn on_mleader_style_dialog_set_current(&mut self) -> Task<Message> {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let name = self.mleaderstyle_selected.clone();
                let exists = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .values()
                    .any(|o| matches!(o, ObjectType::MultiLeaderStyle(s) if s.name == name));
                if exists {
                    // Staged: header field is the round-trip source of truth
                    // ($CMLEADERSTYLE); the ribbon/tab mirror it.
                    self.tabs[i]
                        .scene
                        .document
                        .header
                        .current_mleader_style_name = name.clone();
                    self.tabs[i].active_mleader_style = name.clone();
                    self.ribbon.active_mleader_style = name.clone();
                    self.command_line
                        .push_output(crate::tf!("Current multileader style: {}", name).as_ref());
                }
                Task::none()
    }

    pub(super) fn on_mleader_style_edit(&mut self, field: &'static str, value: String) -> Task<Message> {
                self.mls_color_open = None;
                match field {
                    "landing_distance" => self.mls_landing_distance = value,
                    "landing_gap" => self.mls_landing_gap = value,
                    "arrowhead_size" => self.mls_arrowhead_size = value,
                    "text_height" => self.mls_text_height = value,
                    "scale_factor" => self.mls_scale_factor = value,
                    "break_gap" => self.mls_break_gap = value,
                    "first_seg_angle" => self.mls_first_seg_angle = value,
                    "second_seg_angle" => self.mls_second_seg_angle = value,
                    "max_points" => self.mls_max_points = value,
                    "default_text" => self.mls_default_text = value,
                    "line_color" => self.mls_line_color = value,
                    "text_color" => self.mls_text_color = value,
                    "description" => self.mls_description = value,
                    "align_space" => self.mls_align_space = value,
                    "block_color" => self.mls_block_color = value,
                    "block_rotation" => self.mls_block_rotation = value,
                    "block_scale_x" => self.mls_block_scale_x = value,
                    "block_scale_y" => self.mls_block_scale_y = value,
                    "block_scale_z" => self.mls_block_scale_z = value,
                    _ => {}
                }
                Task::none()
    }

    pub(super) fn on_mleader_style_set_enum(&mut self, field: &'static str, value: String) -> Task<Message> {
                use acadrust::objects::{
                    BlockContentConnectionType, LeaderContentType, LeaderDrawOrderType,
                    MultiLeaderDrawOrderType, MultiLeaderPathType, TextAlignmentType,
                    TextAngleType, TextAttachmentDirectionType, TextAttachmentType,
                };
                // Parse a TextAttachmentType from its debug name.
                fn parse_att(v: &str) -> TextAttachmentType {
                    match v {
                        "TopOfTopLine" => TextAttachmentType::TopOfTopLine,
                        "MiddleOfText" => TextAttachmentType::MiddleOfText,
                        "MiddleOfBottomLine" => TextAttachmentType::MiddleOfBottomLine,
                        "BottomOfBottomLine" => TextAttachmentType::BottomOfBottomLine,
                        "BottomLine" => TextAttachmentType::BottomLine,
                        "BottomOfTopLineUnderlineBottomLine" => {
                            TextAttachmentType::BottomOfTopLineUnderlineBottomLine
                        }
                        "BottomOfTopLineUnderlineTopLine" => {
                            TextAttachmentType::BottomOfTopLineUnderlineTopLine
                        }
                        "BottomOfTopLineUnderlineAll" => {
                            TextAttachmentType::BottomOfTopLineUnderlineAll
                        }
                        "CenterOfText" => TextAttachmentType::CenterOfText,
                        "CenterOfTextOverline" => TextAttachmentType::CenterOfTextOverline,
                        _ => TextAttachmentType::MiddleOfTopLine,
                    }
                }
                let i = self.active_tab;
                if let Some(s) = self.mleaderstyle_mut(i) {
                    match field {
                        "path_type" => {
                            s.path_type = match value.as_str() {
                                "Invisible" => MultiLeaderPathType::Invisible,
                                "Spline" => MultiLeaderPathType::Spline,
                                _ => MultiLeaderPathType::StraightLineSegments,
                            };
                        }
                        "content_type" => {
                            s.content_type = match value.as_str() {
                                "None" => LeaderContentType::None,
                                "Block" => LeaderContentType::Block,
                                "Tolerance" => LeaderContentType::Tolerance,
                                _ => LeaderContentType::MText,
                            };
                        }
                        "text_angle_type" => {
                            s.text_angle_type = match value.as_str() {
                                "ParallelToLastLeaderLine" => {
                                    TextAngleType::ParallelToLastLeaderLine
                                }
                                "Optimized" => TextAngleType::Optimized,
                                _ => TextAngleType::Horizontal,
                            };
                        }
                        "text_alignment" => {
                            s.text_alignment = match value.as_str() {
                                "Center" => TextAlignmentType::Center,
                                "Right" => TextAlignmentType::Right,
                                _ => TextAlignmentType::Left,
                            };
                        }
                        "text_left_attachment" => s.text_left_attachment = parse_att(&value),
                        "text_right_attachment" => s.text_right_attachment = parse_att(&value),
                        "text_top_attachment" => s.text_top_attachment = parse_att(&value),
                        "text_bottom_attachment" => s.text_bottom_attachment = parse_att(&value),
                        "text_attachment_direction" => {
                            s.text_attachment_direction = match value.as_str() {
                                "Vertical" => TextAttachmentDirectionType::Vertical,
                                _ => TextAttachmentDirectionType::Horizontal,
                            };
                        }
                        "block_content_connection" => {
                            s.block_content_connection = match value.as_str() {
                                "BasePoint" => BlockContentConnectionType::BasePoint,
                                _ => BlockContentConnectionType::BlockExtents,
                            };
                        }
                        "leader_draw_order" => {
                            s.leader_draw_order = match value.as_str() {
                                "LeaderTailFirst" => LeaderDrawOrderType::LeaderTailFirst,
                                _ => LeaderDrawOrderType::LeaderHeadFirst,
                            };
                        }
                        "multileader_draw_order" => {
                            s.multileader_draw_order = match value.as_str() {
                                "LeaderFirst" => MultiLeaderDrawOrderType::LeaderFirst,
                                _ => MultiLeaderDrawOrderType::ContentFirst,
                            };
                        }
                        _ => {}
                    }
                }
                Task::none()
    }

    pub(super) fn on_mleader_style_set_handle(&mut self, field: &'static str, value: String) -> Task<Message> {
                let i = self.active_tab;
                let doc = &self.tabs[i].scene.document;
                let handle: Option<acadrust::types::Handle> = if value == "None"
                    || value == "ByBlock"
                    || value == "Closed filled"
                {
                    None
                } else {
                    match field {
                        "line_type_handle" => doc
                            .line_types
                            .iter()
                            .find(|lt| lt.name == value)
                            .map(|lt| lt.handle),
                        "text_style_handle" => doc
                            .text_styles
                            .iter()
                            .find(|t| t.name == value)
                            .map(|t| t.handle),
                        "arrowhead_handle" | "block_content_handle" => doc
                            .block_records
                            .iter()
                            .find(|b| b.name == value)
                            .map(|b| b.handle),
                        _ => None,
                    }
                };
                if let Some(s) = self.mleaderstyle_mut(i) {
                    match field {
                        "line_type_handle" => s.line_type_handle = handle,
                        "text_style_handle" => s.text_style_handle = handle,
                        "arrowhead_handle" => s.arrowhead_handle = handle,
                        "block_content_handle" => s.block_content_handle = handle,
                        _ => {}
                    }
                }
                Task::none()
    }

    /// Write the editor buffers into the selected multileader style (staged, no
    /// commit), so edits survive switching to another style as well as Apply.
    pub(super) fn stage_mleaderstyle_bufs(&mut self) {
                let i = self.active_tab;
                let (ld, lg, asz, th, sf, bg, fsa, ssa, mp, dt, lc, tc) = (
                    self.mls_landing_distance.parse::<f64>().ok(),
                    self.mls_landing_gap.parse::<f64>().ok(),
                    self.mls_arrowhead_size.parse::<f64>().ok(),
                    self.mls_text_height.parse::<f64>().ok(),
                    self.mls_scale_factor.parse::<f64>().ok(),
                    self.mls_break_gap.parse::<f64>().ok(),
                    self.mls_first_seg_angle.parse::<f64>().ok(),
                    self.mls_second_seg_angle.parse::<f64>().ok(),
                    self.mls_max_points.parse::<i32>().ok(),
                    self.mls_default_text.clone(),
                    self.mls_line_color.parse::<i16>().ok(),
                    self.mls_text_color.parse::<i16>().ok(),
                );
                let desc = self.mls_description.clone();
                let align = self.mls_align_space.parse::<f64>().ok();
                let bclr = self.mls_block_color.parse::<i16>().ok();
                let brot = self.mls_block_rotation.parse::<f64>().ok();
                let bsx = self.mls_block_scale_x.parse::<f64>().ok();
                let bsy = self.mls_block_scale_y.parse::<f64>().ok();
                let bsz = self.mls_block_scale_z.parse::<f64>().ok();
                if let Some(s) = self.mleaderstyle_mut(i) {
                    if let Some(v) = ld {
                        s.landing_distance = v;
                    }
                    if let Some(v) = lg {
                        s.landing_gap = v;
                    }
                    if let Some(v) = asz {
                        s.arrowhead_size = v;
                    }
                    if let Some(v) = th {
                        s.text_height = v;
                    }
                    if let Some(v) = sf {
                        s.scale_factor = v;
                    }
                    if let Some(v) = bg {
                        s.break_gap_size = v;
                    }
                    if let Some(v) = fsa {
                        s.first_segment_angle = v.to_radians();
                    }
                    if let Some(v) = ssa {
                        s.second_segment_angle = v.to_radians();
                    }
                    if let Some(v) = mp {
                        s.max_leader_points = v;
                    }
                    s.default_text = dt;
                    if let Some(v) = lc {
                        s.line_color = acadrust::types::Color::from_index(v);
                    }
                    if let Some(v) = tc {
                        s.text_color = acadrust::types::Color::from_index(v);
                    }
                    s.description = desc;
                    if let Some(v) = align {
                        s.align_space = v;
                    }
                    if let Some(v) = bclr {
                        s.block_content_color = acadrust::types::Color::from_index(v);
                    }
                    if let Some(v) = brot {
                        s.block_content_rotation = v.to_radians();
                    }
                    if let Some(v) = bsx {
                        s.block_content_scale_x = v;
                    }
                    if let Some(v) = bsy {
                        s.block_content_scale_y = v;
                    }
                    if let Some(v) = bsz {
                        s.block_content_scale_z = v;
                    }
                }
    }

    pub(super) fn on_mleader_style_apply(&mut self) -> Task<Message> {
                self.stage_mleaderstyle_bufs();
                self.style_stage_commit();
                Task::none()
    }

    pub(super) fn on_dim_style_dialog_open(&mut self) -> Task<Message> {
                let i = self.active_tab;
                // Pick the document's current dim style or "Standard".
                let cur = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_dimstyle_name
                    .clone();
                let selected = if self.tabs[i].scene.document.dim_styles.get(&cur).is_some() {
                    cur
                } else {
                    self.tabs[i]
                        .scene
                        .document
                        .dim_styles
                        .iter()
                        .next()
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "Standard".to_string())
                };
                self.dimstyle_selected = selected.clone();
                self.dimstyle_compare = self.tabs[i]
                    .scene
                    .document
                    .dim_styles
                    .iter()
                    .map(|style| style.name.clone())
                    .find(|name| !name.eq_ignore_ascii_case(&selected))
                    .unwrap_or_default();
                self.load_dimstyle_bufs(i);
                self.active_modal = Some(crate::app::ModalKind::DimStyle);
                self.style_stage_begin();
                Task::none()
    }

    pub(super) fn on_color_window_pick(&mut self, color: acadrust::types::Color) -> Task<Message> {
                if matches!(
                    self.color_pick_target.as_ref().map(|(target, _)| target),
                    Some(crate::app::ColorPickTarget::PlotStyle)
                ) {
                    self.color_pick_target = None;

                    let rgb = match color {
                        acadrust::types::Color::Rgb { r, g, b } => Some((r, g, b)),
                        acadrust::types::Color::Index(index) => {
                            acadrust::types::aci_table::aci_to_rgb(index)
                        }
                        _ => None,
                    };

                    if let Some((r, g, b)) = rgb {
                        self.ps_color_buf = format!("#{r:02X}{g:02X}{b:02X}");
                    }

                    return self.on_plot_style_panel_apply();
                }
                let s = crate::ui::color_select::color_to_aci_string(color);
                let edit = match self.color_pick_target.take().map(|(target, _)| target) {
                    Some(crate::app::ColorPickTarget::DimStyle(f)) => Some(Message::DsEdit(f, s)),
                    Some(crate::app::ColorPickTarget::MLeader(f)) => {
                        Some(Message::MLeaderStyleEdit { field: f, value: s })
                    }
                    Some(crate::app::ColorPickTarget::Table(r, f)) => {
                        Some(Message::TableStyleCellEdit {
                            row: r,
                            field: f,
                            value: s,
                        })
                    }
                    Some(crate::app::ColorPickTarget::Properties) => {
                        Some(Message::PropColorChanged(color))
                    }
                    Some(crate::app::ColorPickTarget::PropertiesBg) => {
                        Some(Message::PropBgColorChanged(color))
                    }
                    Some(crate::app::ColorPickTarget::PropertiesField(field)) => {
                        Some(Message::PropColorFieldChanged { field, color })
                    }
                    Some(crate::app::ColorPickTarget::MText) => {
                        Some(Message::MTextColorChanged(color))
                    }
                    Some(crate::app::ColorPickTarget::Ribbon) => {
                        Some(Message::RibbonColorChanged(color))
                    }
                    Some(crate::app::ColorPickTarget::Layer(idx)) => {
                        self.tabs[self.active_tab].layers.selected = Some(idx);
                        Some(Message::LayerColorSet(color))
                    }
                    Some(crate::app::ColorPickTarget::LayerState(idx)) => {
                        Some(Message::LayerStateEditorLayerColor(idx, color))
                    }
                    Some(crate::app::ColorPickTarget::PlotStyle) => None,
                    None => None,
                };
                if let Some(m) = edit {
                    self.update(m)
                } else {
                    Task::none()
                }
    }

    pub(super) fn on_ds_set_handle(&mut self, field: &'static str, value: String) -> Task<Message> {
                let i = self.active_tab;
                let name = self.dimstyle_selected.clone();
                let is_lt = matches!(
                    field,
                    "dimltex_handle" | "dimltex1_handle" | "dimltex2_handle"
                );
                let doc = &self.tabs[i].scene.document;
                let handle = if value == "Default" || value == "ByBlock" {
                    acadrust::types::Handle::NULL
                } else if is_lt {
                    doc.line_types
                        .iter()
                        .find(|lt| lt.name == value)
                        .map(|lt| lt.handle)
                        .unwrap_or(acadrust::types::Handle::NULL)
                } else {
                    doc.block_records
                        .iter()
                        .find(|b| b.name == value)
                        .map(|b| b.handle)
                        .unwrap_or(acadrust::types::Handle::NULL)
                };
                // Staged: persists on Apply.
                if let Some(ds) = self.tabs[i].scene.document.dim_styles.get_mut(&name) {
                    match field {
                        "dimblk" => ds.dimblk = handle,
                        "dimblk1" => {
                            ds.dimblk1 = handle;
                            ds.dimblk2 = handle;
                        }
                        "dimblk2" => ds.dimblk2 = handle,
                        "dimldrblk" => ds.dimldrblk = handle,
                        "dimltex_handle" => ds.dimltex_handle = handle,
                        "dimltex1_handle" => ds.dimltex1_handle = handle,
                        "dimltex2_handle" => ds.dimltex2_handle = handle,
                        _ => {}
                    }
                }
                Task::none()
    }
}
