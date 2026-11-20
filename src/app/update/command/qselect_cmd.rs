use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn on_qselect_open(&mut self) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].scene.selection.borrow_mut().menu.close();
        let remembered = self.qselect_settings.clone();
        let scope = remembered
            .as_ref()
            .map_or(crate::app::QSelectScope::CurrentSpace, |settings| {
                settings.scope
            });
        let available_types = self.tabs[i].scene.qselect_type_counts(scope);
        let type_filter = if let Some(settings) = remembered.as_ref() {
            settings
                .type_filter
                .as_ref()
                .filter(|selected| available_types.iter().any(|(item, _)| item == *selected))
                .cloned()
        } else {
            self.tabs[i]
                .scene
                .selected
                .iter()
                .next()
                .and_then(|handle| self.tabs[i].scene.document.get_entity(*handle))
                .map(crate::entities::traits::entity_type_name)
                .map(str::to_string)
        };
        let available_properties = self.tabs[i]
            .scene
            .qselect_properties(type_filter.as_deref(), scope);
        let candidate_count = self.tabs[i].scene.qselect_candidate_count(scope);
        let property = remembered
            .as_ref()
            .and_then(|settings| settings.property_field.as_ref())
            .and_then(|field| {
                available_properties
                    .iter()
                    .find(|property| property.field == *field)
                    .cloned()
            });
        let mut operator = remembered
            .as_ref()
            .map_or(crate::app::QSelectOp::Eq, |settings| settings.operator);
        if operator.needs_number()
            && !property.as_ref().is_some_and(|property| {
                matches!(property.editor, crate::app::QSelectValueEditor::Number)
            })
        {
            operator = crate::app::QSelectOp::Eq;
        }
        let value = remembered.as_ref().map_or_else(String::new, |settings| {
            if settings.property_field.is_none() || property.is_some() {
                settings.value.clone()
            } else {
                String::new()
            }
        });
        let mode = remembered
            .as_ref()
            .map_or(crate::app::QSelectMode::Include, |settings| settings.mode);
        let result = remembered
            .as_ref()
            .map_or_else(Default::default, |settings| settings.result);
        // Highlighting the matches starts off until it is turned on, then
        // stays as it was left (#2611).
        let show = remembered.as_ref().is_some_and(|settings| settings.show);
        self.qselect = Some(crate::app::QSelectState {
            conditions: remembered
                .as_ref()
                .map_or_else(Vec::new, |settings| settings.conditions.clone()),
            editing_condition: None,
            scope,
            available_types,
            available_properties,
            candidate_count,
            type_filter,
            property,
            operator,
            value,
            mode,
            result,
            error: None,
            live_count: 0,
            live_by_type: Vec::new(),
            show,
            value_counts: Vec::new(),
            sample: None,
        });
        self.reset_modal_geometry();
        self.qselect_refresh_values();
        self.qselect_refresh_live();
        Task::none()
    }

}
