//! PCEXTRACTEDGE, PCEXTRACTCORNER, PCEXTRACTCENTERLINE and
//! (-)PCEXTRACTSECTION: the commands get the drawing's shown cloud points
//! and its section objects when they start.

use super::*;
use crate::ui::window::pdf_dialogs::PdfDialogMsg;
use codec::entities::ExtendedEntityData;
use glam::DVec3;

use crate::modules::insert::pc_extract::{
    CenterlineCommand, Clouds, PlanesCommand, Section, SectionCommand,
};

fn dvec(v: codec::types::Vector3) -> DVec3 {
    DVec3::new(v.x, v.y, v.z)
}

impl OpenCADStudio {
    /// The points each point cloud shows (the renderer's placed set).
    fn extraction_clouds(&self, i: usize) -> Clouds {
        let document = &self.tabs[i].scene.document;
        let mut clouds = Vec::new();
        for entity in document.entities() {
            let codec::EntityType::Extended(extended) = entity else {
                continue;
            };
            let ExtendedEntityData::PointCloudEx(data) = &extended.data else {
                continue;
            };
            let common = entity.common();
            // The colour the renderer places with, so its cached points are reused.
            let color = if matches!(common.color, codec::types::Color::ByLayer) {
                document.layers.get(&common.layer).map(|layer| layer.color.clone()).unwrap_or(common.color.clone())
            } else {
                common.color.clone()
            };
            let color = color.rgb().map_or([255; 3], |(r, g, b)| [r, g, b]);
            if let Some(placed) = crate::scene::model::point_cloud::placed(
                document,
                data,
                color,
                None,
                &crate::scene::model::point_cloud::hidden(data),
            ) {
                clouds.push((common.handle, placed));
            }
        }
        Clouds(clouds)
    }

    fn extraction_sections(&self, i: usize) -> Vec<Section> {
        self.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| {
                let codec::EntityType::Extended(extended) = entity else {
                    return None;
                };
                let ExtendedEntityData::SectionObject(data) = &extended.data else {
                    return None;
                };
                let (first, last) = (dvec(*data.vertices.first()?), dvec(*data.vertices.last()?));
                let tangent = (last - first).normalize_or(DVec3::X);
                let vertical = dvec(data.vertical_direction).normalize_or(DVec3::Z);
                let viewing = vertical.cross(tangent).normalize_or(-DVec3::Z)
                    * if data.flags & 4 != 0 { 1.0 } else { -1.0 };
                Some(Section { origin: first, viewing, tangent, live: data.flags & 1 != 0 })
            })
            .collect()
    }

    pub(super) fn dispatch_pc_extract(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        use crate::command::CadCommand;
        let command: Box<dyn CadCommand> = match cmd {
            "PCEXTRACTEDGE" => Box::new(PlanesCommand::new(false, self.extraction_clouds(i))),
            "PCEXTRACTCORNER" => Box::new(PlanesCommand::new(true, self.extraction_clouds(i))),
            "PCEXTRACTCENTERLINE" => Box::new(CenterlineCommand::new(self.extraction_clouds(i))),
            "PCEXTRACTSECTION" | "-PCEXTRACTSECTION" => Box::new(SectionCommand::new(
                cmd == "PCEXTRACTSECTION",
                self.extraction_clouds(i),
                self.extraction_sections(i),
            )),
            "_PCSECTIONDLG" => {
                self.open_pc_section_dialog(i);
                return Some(self.finish_dispatch(cmd));
            }
            _ => return None,
        };
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(command);
        Some(self.finish_dispatch(cmd))
    }
}

impl OpenCADStudio {
    /// PCEXTRACTSECTION's settings dialog, filled from the session settings.
    fn open_pc_section_dialog(&mut self, i: usize) {
        use crate::modules::insert::pc_extract::{has_job, settings};
        use crate::ui::window::pdf_dialogs::{LineColor, PcSectionState};
        if !has_job() {
            return;
        }
        let s = settings();
        let current = crate::t!("Use Current").into_owned();
        let mut layers = vec![current.clone()];
        let mut names: Vec<String> = self.tabs[i].scene.document.layers.iter().map(|l| l.name.clone()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        layers.extend(names);
        let short = |v: f64| {
            let t = format!("{v:.4}");
            t.trim_end_matches('0').trim_end_matches('.').to_string()
        };
        self.pc_section = Some(PcSectionState {
            perimeter: s.perimeter,
            max_points: s.max_points.to_string(),
            layers,
            layer: s.layer.clone().unwrap_or(current),
            color: LineColor(s.color),
            polylines: s.polylines,
            width: short(s.width),
            min_length: short(s.min_length),
            connect: short(s.connect),
            angle: short(s.angle),
            preview: s.preview,
        });
        self.active_modal = Some(crate::app::ModalKind::PcSection);
    }

    pub(in crate::app) fn update_pc_section(&mut self, message: PdfDialogMsg) -> Task<Message> {
        use crate::modules::insert::pc_extract::{set_settings, settings, SectionCommand, SectionDistanceCommand};
        use crate::command::CadCommand;
        let i = self.active_tab;
        let Some(state) = self.pc_section.as_mut() else {
            return Task::none();
        };
        match message {
            PdfDialogMsg::SecPerimeter(on) => state.perimeter = on,
            PdfDialogMsg::SecMaxPoints(v) => state.max_points = v,
            PdfDialogMsg::SecLayer(v) => state.layer = v,
            PdfDialogMsg::SecColor(c) => state.color = c,
            PdfDialogMsg::SecPolylines(on) => state.polylines = on,
            PdfDialogMsg::SecWidth(v) => state.width = v,
            PdfDialogMsg::SecMinLength(v) => state.min_length = v,
            PdfDialogMsg::SecConnect(v) => state.connect = v,
            PdfDialogMsg::SecAngle(v) => state.angle = v,
            PdfDialogMsg::SecPreview(on) => state.preview = on,
            // Measure on screen, then back to the dialog (the extraction waits).
            PdfDialogMsg::SecPick(connect) => {
                self.pc_section = None;
                self.active_modal = None;
                let command = SectionDistanceCommand::new(connect);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            PdfDialogMsg::SecCreate => {
                let number = |t: &str| crate::entities::common::parse_f64(t.trim());
                let (min_length, connect, width) = (number(&state.min_length), number(&state.connect), number(&state.width));
                let (Some(min_length), Some(connect)) = (min_length.filter(|v| *v > 0.0), connect.filter(|v| *v > 0.0)) else {
                    self.command_line.push_error(crate::t!("Requires a positive number.").as_ref());
                    return Task::none();
                };
                let Some(angle) = state.angle.trim().parse::<i64>().ok().filter(|v| (0..=10).contains(v)) else {
                    self.command_line
                        .push_error("Colinear angle tolerance only accepts integer from 0 to 10.");
                    return Task::none();
                };
                let Some(max_points) = state.max_points.trim().parse::<usize>().ok().filter(|v| *v > 0) else {
                    self.command_line.push_error(crate::t!("Requires a positive integer.").as_ref());
                    return Task::none();
                };
                let current = crate::t!("Use Current").into_owned();
                let mut s = settings();
                s.perimeter = state.perimeter;
                s.max_points = max_points;
                s.layer = (state.layer != current).then(|| state.layer.clone());
                s.color = state.color.0;
                s.polylines = state.polylines;
                s.width = width.filter(|v| *v >= 0.0).unwrap_or(0.0);
                s.min_length = min_length;
                s.connect = connect;
                s.angle = angle as f64;
                s.preview = state.preview;
                set_settings(s);
                self.pc_section = None;
                self.active_modal = None;
                let Some(mut command) = SectionCommand::resume() else {
                    return Task::none();
                };
                let result = command.run();
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                return self.apply_cmd_result(result);
            }
            _ => {}
        }
        Task::none()
    }
}
