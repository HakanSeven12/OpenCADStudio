// Undo groups, dirty state, system variables and the output stream.

use super::*;

impl<'a> HostSession<'a> {
    pub fn push_undo(&mut self, label: &str) {
        if self.undo_group_open() {
            return;
        }
        self.app.push_undo_snapshot(self.tab, label);
        if let Some(pending) = self.app.tabs[self.tab].history.pending.as_mut() {
            pending.plugin_owned = true;
        }
    }

    fn undo_group_open(&self) -> bool {
        self.app.tabs[self.tab]
            .history
            .pending
            .as_ref()
            .is_some_and(|pending| pending.held)
    }

    pub fn begin_undo_group(&mut self, label: &str) {
        self.push_undo(label);
        if let Some(pending) = self.app.tabs[self.tab].history.pending.as_mut() {
            pending.held = true;
        }
    }

    pub fn query_entities(
        &mut self,
        query: &str,
    ) -> Result<ocs_plugin_api::host::EntityPage, String> {
        let mut request: serde_json::Value =
            serde_json::from_str(query).map_err(|e| format!("query: {e}"))?;
        if !request.is_object() {
            return Err("query: expected a JSON object".to_owned());
        }
        request["detail"] = serde_json::json!("summary");
        request["fields"] = serde_json::json!([]);
        let answer = self.app.entity_query(self.tab, &request);
        if answer["ok"] != true {
            return Err(answer["error"]
                .as_str()
                .unwrap_or("query failed")
                .to_owned());
        }
        let entities = answer["entities"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entity| u64::from_str_radix(entity["handle"].as_str()?, 16).ok())
            .filter_map(|handle| self.document().get_entity(Handle::new(handle)).cloned())
            .collect();
        Ok(ocs_plugin_api::host::EntityPage {
            entities,
            count: answer["count"].as_u64().unwrap_or(0),
            next_offset: answer["next_offset"].as_u64(),
        })
    }

    pub fn end_undo_group(&mut self) {
        if self.undo_group_open() {
            if let Some(pending) = self.app.tabs[self.tab].history.pending.as_mut() {
                pending.held = false;
            }
            self.app.finish_pending_history(self.tab);
        }
    }

    pub fn set_dirty(&mut self) {
        self.app.tabs[self.tab].dirty = true;
    }

    pub fn system_variable(&self, name: &str) -> Option<HostSettingValue> {
        match name.to_ascii_uppercase().as_str() {
            "CLAYER" => Some(HostSettingValue::Text(
                self.document().header.current_layer_name.clone(),
            )),
            "SNAPANG" => Some(HostSettingValue::Number(self.app.snap_angle_deg as f64)),
            "CTAB" => Some(HostSettingValue::Text(
                self.app.tabs[self.tab].scene.current_layout.clone(),
            )),
            "CTEXTSTYLE" => Some(HostSettingValue::Text(
                self.document().header.current_text_style_name.clone(),
            )),
            "CELTYPE" => Some(HostSettingValue::Text(
                self.document().header.current_linetype_name.clone(),
            )),
            // Every numeric and point variable `getvar` reads; a point comes
            // as "x,y,z".
            other => {
                let read = crate::app::quick_calc::sysvars(&self.app.tabs[self.tab]);
                match (read.0)(&other.to_ascii_lowercase())? {
                    crate::app::calc::SysVar::Number(value) => {
                        Some(HostSettingValue::Number(value))
                    }
                    crate::app::calc::SysVar::Point(p) => {
                        Some(HostSettingValue::Text(format!("{},{},{}", p.x, p.y, p.z)))
                    }
                    crate::app::calc::SysVar::Text => None,
                }
            }
        }
    }

    pub fn set_system_variable(
        &mut self,
        name: &str,
        value: HostSettingValue,
    ) -> Result<HostSettingValue, String> {
        match (name.to_ascii_uppercase().as_str(), value) {
            ("CLAYER", HostSettingValue::Text(layer)) => {
                self.app.set_current_layer_name(self.tab, &layer)?;
                Ok(HostSettingValue::Text(layer))
            }
            ("SNAPANG", HostSettingValue::Number(angle)) if (angle as f32).is_finite() => {
                self.app.snap_angle_deg = (angle as f32).rem_euclid(360.0);
                Ok(HostSettingValue::Number(self.app.snap_angle_deg as f64))
            }
            ("SNAPANG", HostSettingValue::Number(_)) => {
                Err("SNAPANG: finite number required".to_owned())
            }
            ("CLAYER", _) | ("SNAPANG", _) => {
                Err(format!("{}: wrong value type", name.to_ascii_uppercase()))
            }
            _ => Err(format!("unsupported system variable {name:?}")),
        }
    }

    pub fn push_info(&mut self, msg: &str) {
        self.app.command_line.push_info(msg);
    }

    pub fn push_output(&mut self, msg: &str) {
        self.app.command_line.push_output(msg);
    }

    pub fn push_error(&mut self, msg: &str) {
        self.app.command_line.push_error(msg);
    }
}
