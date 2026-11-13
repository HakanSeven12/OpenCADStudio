//! GEOGRAPHICLOCATION: edit the root drawing CRS as native GEODATA.

use super::{Message, ModalKind, OpenCADStudio};
use crate::io::crs::{self, CrsDefinition};
use crate::ui::window::geographic_location::{GeographicLocationMsg, State};
use document::objects::GeoData;
use iced::Task;

impl OpenCADStudio {
    pub(in crate::app) fn open_geographic_location(&mut self) {
        let document = &self.tabs[self.active_tab].scene.document;
        let definition = crs::model_space_geo_data(document)
            .map(|geo| {
                CrsDefinition::from_geodata(geo)
                    .ok()
                    .and_then(|crs| crs.code().map(|code| format!("EPSG:{code}")))
                    .unwrap_or_else(|| {
                        if geo.coordinate_system_definition.trim().is_empty() {
                            geo.coordinate_system_wkt.clone()
                        } else {
                            geo.coordinate_system_definition.clone()
                        }
                    })
            })
            .unwrap_or_default();
        self.geographic_location = Some(State {
            definition,
            error: None,
        });
        self.active_modal = Some(ModalKind::GeographicLocation);
        self.reset_modal_geometry();
    }

    pub(in crate::app) fn on_geographic_location(
        &mut self,
        msg: GeographicLocationMsg,
    ) -> Task<Message> {
        match msg {
            GeographicLocationMsg::Definition(value) => {
                if let Some(state) = self.geographic_location.as_mut() {
                    state.definition = value;
                    state.error = None;
                }
                Task::none()
            }
            GeographicLocationMsg::Apply => self.apply_geographic_location(),
        }
    }

    fn geographic_location_error(&mut self, error: String) {
        if let Some(state) = self.geographic_location.as_mut() {
            state.error = Some(crate::tr!(
                "geographic-location-dialog",
                "invalid",
                error = error
            ));
        }
    }

    fn apply_geographic_location(&mut self) -> Task<Message> {
        let Some(state) = self.geographic_location.as_ref() else {
            return Task::none();
        };
        let definition = state.definition.clone();
        let crs = match CrsDefinition::from_text(&definition) {
            Ok(crs) => crs,
            Err(error) => {
                self.geographic_location_error(error);
                return Task::none();
            }
        };

        let i = self.active_tab;
        let before = crs::model_space_geo_data(&self.tabs[i].scene.document).cloned();
        let mut edited: GeoData = before.clone().unwrap_or_default();
        if let Err(error) = crs.apply_to_geodata(&mut edited) {
            self.geographic_location_error(error);
            return Task::none();
        }
        if before.as_ref() == Some(&edited) {
            self.close_active_modal();
            return Task::none();
        }

        self.push_undo_snapshot(i, "GEOGRAPHICLOCATION");
        if let Err(error) = crs::set_document_crs(&mut self.tabs[i].scene.document, &crs) {
            self.drop_unchanged_pending_history(i);
            self.geographic_location_error(error);
            return Task::none();
        }
        self.tabs[i].scene.refresh_object_data_cache();
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
        self.close_active_modal();
        self.command_line.push_output(
            crate::tr!(
                "geographic-location-dialog",
                "set",
                name = crs.name().to_string()
            )
            .as_ref(),
        );
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::objects::ObjectType;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.tabs[app.active_tab].dirty = false;
        app
    }

    fn undo_len(app: &mut OpenCADStudio) -> usize {
        app.finish_all_pending_history();
        app.tabs[app.active_tab].history.undo_stack.len()
    }

    fn apply(app: &mut OpenCADStudio, definition: &str) {
        let _ = app.update(Message::GeographicLocation(
            GeographicLocationMsg::Definition(definition.to_string()),
        ));
        let _ = app.update(Message::GeographicLocation(GeographicLocationMsg::Apply));
    }

    fn open_and_apply(app: &mut OpenCADStudio, definition: &str) {
        let _ = app.dispatch_command("GEOGRAPHICLOCATION");
        apply(app, definition);
    }

    fn geo(app: &OpenCADStudio) -> &GeoData {
        crs::model_space_geo_data(&app.tabs[app.active_tab].scene.document)
            .expect("model-space GEODATA")
    }

    fn cache_has(app: &OpenCADStudio, handle: document::Handle) -> bool {
        let scene = &app.tabs[app.active_tab].scene;
        crate::entities::object_data::geo_objects(&scene.object_data_cache).contains(&handle)
    }

    #[test]
    fn command_creates_canonical_persistent_geodata() {
        assert!(crate::ui::menu_bar::implemented("GEOGRAPHICLOCATION"));
        let mut app = app();
        let i = app.active_tab;
        let undo = undo_len(&mut app);
        open_and_apply(&mut app, "EPSG:5514");
        assert!(app.active_modal.is_none());
        assert!(app.tabs[i].dirty);
        assert_eq!(undo_len(&mut app), undo + 1);

        let handle = crs::model_space_geo_data_handle(&app.tabs[i].scene.document).unwrap();
        assert!(handle.is_valid());
        assert_eq!(geo(&app).handle, handle);
        assert!(cache_has(&app, handle));
        assert_eq!(geo(&app).version, 3);
        assert_eq!(geo(&app).coordinate_type, 2);
        assert!(!geo(&app).coordinate_system_definition.is_empty());
        assert_eq!(
            geo(&app).coordinate_system_definition,
            geo(&app).coordinate_system_wkt
        );
        assert_eq!(
            CrsDefinition::from_geodata(geo(&app)).unwrap().code(),
            Some(5514)
        );
        assert_eq!(
            app.tabs[i].scene.geo_data().map(|geo| geo.handle),
            Some(handle)
        );

        let document = &app.tabs[i].scene.document;
        let model_space = document.block_records.get("*Model_Space").unwrap().handle;
        assert_eq!(geo(&app).host_block, model_space);
        assert_eq!(geo(&app).reactors, vec![geo(&app).owner]);
        let ObjectType::Dictionary(dictionary) = &document.objects[&geo(&app).owner] else {
            panic!("GEODATA owner is its extension dictionary")
        };
        assert!(dictionary.entries.iter().any(|(key, value)| {
            key.eq_ignore_ascii_case(crs::GEODATA_DICTIONARY_KEY) && *value == handle
        }));
    }

    #[test]
    fn create_and_update_round_trip_through_undo_and_redo() {
        let mut app = app();
        let i = app.active_tab;
        open_and_apply(&mut app, "EPSG:5514");
        let handle = crs::model_space_geo_data_handle(&app.tabs[i].scene.document).unwrap();
        let wkt = geo(&app).coordinate_system_wkt.clone();
        app.undo_steps(1);
        assert!(crs::model_space_geo_data(&app.tabs[i].scene.document).is_none());
        assert!(app.tabs[i].scene.geo_data().is_none());
        assert!(!cache_has(&app, handle));
        assert!(!app.tabs[i].dirty);
        app.redo_steps(1);
        assert_eq!(geo(&app).handle, handle);
        assert!(cache_has(&app, handle));
        assert_eq!(geo(&app).coordinate_system_wkt, wkt);
        assert!(app.tabs[i].scene.geo_data().is_some());
        assert!(app.tabs[i].dirty);

        let original = {
            let document = &mut app.tabs[i].scene.document;
            let handle = crs::model_space_geo_data_handle(document).unwrap();
            let Some(ObjectType::GeoData(geo)) = document.objects.get_mut(&handle) else {
                unreachable!()
            };
            geo.design_point.x = -742_925.25;
            geo.reference_point.y = 50.08804;
            geo.north_direction.x = 0.25;
            geo.up_direction.z = 1.0;
            geo.horizontal_unit_scale = 1.25;
            geo.coordinate_system_datum = "S-JTSK".to_string();
            geo.clone()
        };
        open_and_apply(&mut app, "EPSG:4326");
        assert_eq!(
            crs::model_space_geo_data_handle(&app.tabs[i].scene.document),
            Some(handle)
        );
        assert_eq!(geo(&app).coordinate_type, 3);
        assert_eq!(geo(&app).design_point, original.design_point);
        assert_eq!(geo(&app).reference_point, original.reference_point);
        assert_eq!(geo(&app).north_direction, original.north_direction);
        assert_eq!(geo(&app).up_direction, original.up_direction);
        assert_eq!(
            geo(&app).horizontal_unit_scale,
            original.horizontal_unit_scale
        );
        assert_eq!(
            geo(&app).coordinate_system_datum,
            original.coordinate_system_datum
        );
        let edited = geo(&app).clone();

        app.undo_steps(1);
        assert_eq!(geo(&app), &original);
        assert_eq!(app.tabs[i].scene.geo_data(), Some(&original));
        app.redo_steps(1);
        assert_eq!(geo(&app), &edited);
        assert_eq!(app.tabs[i].scene.geo_data(), Some(&edited));
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .objects
                .values()
                .filter(|object| matches!(object, ObjectType::GeoData(_)))
                .count(),
            1
        );
    }

    #[test]
    fn invalid_proj_only_and_cancel_leave_no_history_or_handles() {
        for definition in ["not-a-crs", "+proj=longlat +datum=WGS84"] {
            let mut app = app();
            let i = app.active_tab;
            let objects = app.tabs[i].scene.document.objects.len();
            let next_handle = app.tabs[i].scene.document.next_handle();
            let undo = undo_len(&mut app);
            open_and_apply(&mut app, definition);
            assert!(matches!(
                app.active_modal,
                Some(ModalKind::GeographicLocation)
            ));
            assert!(app
                .geographic_location
                .as_ref()
                .and_then(|state| state.error.as_ref())
                .is_some());
            assert_eq!(app.tabs[i].scene.document.objects.len(), objects);
            assert_eq!(app.tabs[i].scene.document.next_handle(), next_handle);
            assert_eq!(undo_len(&mut app), undo);
            assert!(!app.tabs[i].dirty);
            let _ = app.update(Message::CloseModal);
            assert!(app.geographic_location.is_none());
            assert_eq!(app.tabs[i].scene.document.objects.len(), objects);
            assert_eq!(undo_len(&mut app), undo);
        }
    }

    #[test]
    fn opening_prefills_and_identical_apply_is_a_no_op() {
        let mut app = app();
        let i = app.active_tab;
        open_and_apply(&mut app, "5514");
        app.tabs[i].dirty = false;
        let undo = undo_len(&mut app);
        let _ = app.dispatch_command("GEOGRAPHICLOCATION");
        assert_eq!(
            app.geographic_location.as_ref().unwrap().definition,
            "EPSG:5514"
        );
        let _ = app.update(Message::GeographicLocation(GeographicLocationMsg::Apply));
        assert!(app.active_modal.is_none());
        assert_eq!(undo_len(&mut app), undo);
        assert!(!app.tabs[i].dirty);
    }
}
