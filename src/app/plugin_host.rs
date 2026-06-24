// HostSession — plugin-facing API implemented inside `app` (private field access).

use std::any::Any;

use acadrust::tables::AppId;
use acadrust::xdata::ExtendedDataRecord;
use acadrust::{CadDocument, EntityType, Handle};
use ocs_plugin_api::host::HostApi;

use super::OpenCADStudio;

/// Session adapter: one active document tab, command line, undo.
pub(crate) struct HostSession<'a> {
    app: &'a mut OpenCADStudio,
    tab: usize,
    doc_store: Option<ocs_plugin_api::shm::DocumentSnapshotStore>,
}

impl<'a> HostSession<'a> {
    pub(crate) fn new(app: &'a mut OpenCADStudio, tab: usize) -> Self {
        Self {
            app,
            tab,
            doc_store: None,
        }
    }

    pub fn tab_index(&self) -> usize {
        self.tab
    }

    pub fn document(&self) -> &CadDocument {
        &self.app.tabs[self.tab].scene.document
    }

    pub fn document_mut(&mut self) -> &mut CadDocument {
        &mut self.app.tabs[self.tab].scene.document
    }

    pub fn document_view(&mut self) -> Option<ocs_plugin_api::shm::DocumentViewInfo> {
        use ocs_plugin_api::shm::DocumentSnapshotStore;
        if self.doc_store.is_none() {
            let mut store = DocumentSnapshotStore::new(self.tab, 8 * 1024 * 1024).ok()?;
            store.publish(self.document()).ok()?;
            self.doc_store = Some(store);
        }
        let store = self.doc_store.as_ref()?;
        Some(ocs_plugin_api::shm::DocumentViewInfo {
            path: store.path().to_string_lossy().to_string(),
            version: store.version(),
        })
    }

    fn publish_document_view(&mut self) {
        let doc = &self.app.tabs[self.tab].scene.document;
        if let Some(store) = self.doc_store.as_mut() {
            if let Err(e) = store.publish(doc) {
                eprintln!(
                    "[host] failed to publish document view for tab {}: {e}",
                    self.tab
                );
            }
        }
    }

    pub fn add_entity(&mut self, entity: EntityType) -> Handle {
        let handle = self.app.tabs[self.tab].scene.add_entity(entity);
        self.publish_document_view();
        handle
    }

    pub fn bump_geometry(&mut self) {
        self.app.tabs[self.tab].scene.bump_geometry();
    }

    // ── XDATA convenience ──────────────────────────────────────────────────
    // Plugins persist domain data as XDATA on plain entities so it round-trips
    // through DWG/DXF. These wrap the `acadrust::xdata` API keyed by entity
    // handle and keep the APPID table in sync.

    /// Read the XDATA record for `app_name` attached to entity `handle`, if any.
    pub fn read_record(&self, handle: Handle, app_name: &str) -> Option<&ExtendedDataRecord> {
        self.document()
            .get_entity(handle)?
            .common()
            .extended_data
            .get_record(app_name)
    }

    /// Attach `record` to entity `handle`, replacing any existing record for the
    /// same application. Registers the application in the APPID table when
    /// missing so the file stays valid for other CAD apps. Returns `false` when
    /// the entity does not exist.
    pub fn write_record(&mut self, handle: Handle, record: ExtendedDataRecord) -> bool {
        let app = record.application_name.clone();
        self.ensure_app_id(&app);
        let Some(entity) = self.document_mut().get_entity_mut(handle) else {
            return false;
        };
        let xd = &mut entity.common_mut().extended_data;
        // Drop any existing record for this app, then append the new one.
        let kept: Vec<_> = xd
            .records()
            .iter()
            .filter(|r| r.application_name != app)
            .cloned()
            .collect();
        xd.clear();
        for r in kept {
            xd.add_record(r);
        }
        xd.add_record(record);
        self.publish_document_view();
        true
    }

    /// Remove the XDATA record for `app_name` from entity `handle`. Returns
    /// `true` when a record was actually removed.
    pub fn remove_record(&mut self, handle: Handle, app_name: &str) -> bool {
        let Some(entity) = self.document_mut().get_entity_mut(handle) else {
            return false;
        };
        let xd = &mut entity.common_mut().extended_data;
        let kept: Vec<_> = xd
            .records()
            .iter()
            .filter(|r| r.application_name != app_name)
            .cloned()
            .collect();
        if kept.len() == xd.records().len() {
            return false;
        }
        xd.clear();
        for r in kept {
            xd.add_record(r);
        }
        self.publish_document_view();
        true
    }

    /// Register `name` in the APPID table if it is not already present, so XDATA
    /// written under it survives a DWG/DXF round-trip.
    fn ensure_app_id(&mut self, name: &str) {
        let doc = self.document_mut();
        if !doc.app_ids.contains(name) {
            let _ = doc.app_ids.add(AppId::new(name));
        }
    }

    pub fn push_undo(&mut self, label: &str) {
        self.app.push_undo_snapshot(self.tab, label);
    }

    pub fn set_dirty(&mut self) {
        self.app.tabs[self.tab].dirty = true;
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

/// The stable contract a plugin's `dispatch` sees. Each method forwards to the
/// inherent `HostSession` method of the same name (inherent methods take
/// resolution priority, so this is plain delegation, not recursion). The
/// per-tab plugin-state accessors expose the raw `Any` box; the typed
/// `ocs_plugin_api::host::plugin_state*` helpers wrap them.
impl HostApi for HostSession<'_> {
    fn tab_index(&self) -> usize {
        self.tab_index()
    }
    fn document(&self) -> &CadDocument {
        self.document()
    }
    fn document_mut(&mut self) -> &mut CadDocument {
        self.document_mut()
    }
    fn add_entity(&mut self, entity: EntityType) -> Handle {
        self.add_entity(entity)
    }
    fn bump_geometry(&mut self) {
        self.bump_geometry()
    }
    fn read_record(&self, handle: Handle, app_name: &str) -> Option<&ExtendedDataRecord> {
        self.read_record(handle, app_name)
    }
    fn write_record(&mut self, handle: Handle, record: ExtendedDataRecord) -> bool {
        self.write_record(handle, record)
    }
    fn remove_record(&mut self, handle: Handle, app_name: &str) -> bool {
        self.remove_record(handle, app_name)
    }
    fn push_undo(&mut self, label: &str) {
        self.push_undo(label)
    }
    fn set_dirty(&mut self) {
        self.set_dirty()
    }
    fn push_info(&mut self, msg: &str) {
        self.push_info(msg)
    }
    fn push_output(&mut self, msg: &str) {
        self.push_output(msg)
    }
    fn push_error(&mut self, msg: &str) {
        self.push_error(msg)
    }
    fn start_interactive(&mut self, command: Box<dyn ocs_plugin_api::host::InteractiveCommand>) {
        self.app.tabs[self.tab].active_cmd =
            Some(Box::new(PluginInteractiveAdapter { inner: command }));
    }
    fn plugin_state_any(&self, plugin_id: &str) -> Option<&(dyn Any + Send + Sync)> {
        self.app.tabs[self.tab]
            .plugin_state
            .get(plugin_id)
            .map(|b| b.as_ref())
    }
    fn plugin_state_any_mut(&mut self, plugin_id: &str) -> Option<&mut (dyn Any + Send + Sync)> {
        self.app.tabs[self.tab]
            .plugin_state
            .get_mut(plugin_id)
            .map(|b| b.as_mut())
    }
    fn ensure_plugin_state_any(
        &mut self,
        plugin_id: &'static str,
        init: &mut dyn FnMut() -> Box<dyn Any + Send + Sync>,
    ) -> &mut (dyn Any + Send + Sync) {
        self.app.tabs[self.tab]
            .plugin_state
            .entry(plugin_id)
            .or_insert_with(|| init())
            .as_mut()
    }
    fn document_reader(&self) -> Box<dyn ocs_plugin_api::host::DocumentReader + '_> {
        Box::new(ocs_plugin_api::host::CadDocumentReader(self.document()))
    }
    fn document_view(&mut self) -> Option<ocs_plugin_api::shm::DocumentViewInfo> {
        self.document_view()
    }
}

/// Bridges a plugin's [`InteractiveCommand`](ocs_plugin_api::host::InteractiveCommand)
/// to the host's internal `CadCommand`, so a plugin tool drives the host's
/// point-collection flow (viewport clicks or `--serve` coordinates) just like a
/// built-in tool.
struct PluginInteractiveAdapter {
    inner: Box<dyn ocs_plugin_api::host::InteractiveCommand>,
}

impl crate::command::CadCommand for PluginInteractiveAdapter {
    fn name(&self) -> &'static str {
        "PLUGIN"
    }
    // Every call into the plugin runs under a panic guard (#145): a buggy plugin
    // that panics mid-command leaves the host running — the command just ends.
    fn prompt(&self) -> String {
        crate::plugin::guard("InteractiveCommand::prompt", || self.inner.prompt())
            .unwrap_or_default()
    }
    fn on_point(&mut self, pt: glam::Vec3) -> crate::command::CmdResult {
        crate::plugin::guard("InteractiveCommand::on_point", || {
            self.inner.on_point([pt.x as f64, pt.y as f64, pt.z as f64])
        })
        .map(plugin_step_to_result)
        .unwrap_or(crate::command::CmdResult::Cancel)
    }
    fn on_enter(&mut self) -> crate::command::CmdResult {
        crate::plugin::guard("InteractiveCommand::on_enter", || self.inner.on_enter())
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel)
    }
    fn needs_entity_pick(&self) -> bool {
        crate::plugin::guard("InteractiveCommand::needs_object_pick", || {
            self.inner.needs_object_pick()
        })
        .unwrap_or(false)
    }
    fn on_entity_pick(&mut self, handle: Handle, pt: glam::Vec3) -> crate::command::CmdResult {
        crate::plugin::guard("InteractiveCommand::on_object_pick", || {
            self.inner
                .on_object_pick(handle, [pt.x as f64, pt.y as f64, pt.z as f64])
        })
        .map(plugin_step_to_result)
        .unwrap_or(crate::command::CmdResult::Cancel)
    }
}

/// Bridges an out-of-process plugin's interactive command to the host's
/// `CadCommand`. Events are sent over IPC and the returned `CommandStep` is
/// translated into a `CmdResult`. Prompt and object-pick mode are cached and
/// refreshed after each event.
pub(crate) struct PluginProcessInteractiveAdapter {
    pub process: std::sync::Arc<ocs_plugin_api::process::PluginProcess>,
    pub command_id: u64,
    prompt: Option<String>,
    needs_entity_pick: Option<bool>,
}

impl PluginProcessInteractiveAdapter {
    pub(crate) fn new(
        process: std::sync::Arc<ocs_plugin_api::process::PluginProcess>,
        command_id: u64,
    ) -> Self {
        let prompt = process.get_prompt(command_id).ok();
        let needs_entity_pick = process.needs_entity_pick(command_id).ok();
        Self {
            process,
            command_id,
            prompt,
            needs_entity_pick,
        }
    }

    fn refresh(&mut self) {
        self.prompt = self.process.get_prompt(self.command_id).ok();
        self.needs_entity_pick = self.process.needs_entity_pick(self.command_id).ok();
    }
}

impl crate::command::CadCommand for PluginProcessInteractiveAdapter {
    fn name(&self) -> &'static str {
        "PLUGIN"
    }
    fn prompt(&self) -> String {
        self.prompt.clone().unwrap_or_default()
    }
    fn on_point(&mut self, pt: glam::Vec3) -> crate::command::CmdResult {
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(
                self.command_id,
                InteractiveEvent::Point([pt.x as f64, pt.y as f64, pt.z as f64]),
            )
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        self.refresh();
        result
    }
    fn on_enter(&mut self) -> crate::command::CmdResult {
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(self.command_id, InteractiveEvent::Enter)
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        self.refresh();
        result
    }
    fn needs_entity_pick(&self) -> bool {
        self.needs_entity_pick.unwrap_or(false)
    }
    fn on_entity_pick(&mut self, handle: Handle, pt: glam::Vec3) -> crate::command::CmdResult {
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(
                self.command_id,
                InteractiveEvent::ObjectPick {
                    handle,
                    pt: [pt.x as f64, pt.y as f64, pt.z as f64],
                },
            )
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        self.refresh();
        result
    }
}

fn plugin_step_to_result(step: ocs_plugin_api::host::CommandStep) -> crate::command::CmdResult {
    use crate::command::CmdResult;
    use ocs_plugin_api::host::CommandStep;
    match step {
        CommandStep::NeedPoint => CmdResult::NeedPoint,
        CommandStep::Commit(e) => CmdResult::CommitEntity(e),
        CommandStep::CommitAndEnd(e) => CmdResult::CommitAndExit(e),
        CommandStep::Done | CommandStep::Cancel => CmdResult::Cancel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use acadrust::entities::Point;
    use acadrust::xdata::XDataValue;
    use ocs_plugin_api::host::DocumentReader;

    #[test]
    fn xdata_record_round_trips_and_registers_appid() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let h = host.add_entity(EntityType::Point(Point::new()));

        let mut rec = ExtendedDataRecord::new("DEMO_SURVEY");
        rec.add_value(XDataValue::String("PNT-1".to_string()));
        rec.add_value(XDataValue::Integer32(42));
        assert!(host.write_record(h, rec));

        let got = host.read_record(h, "DEMO_SURVEY").expect("record missing");
        assert_eq!(got.values.len(), 2);
        // APPID registered so the XDATA survives a DWG/DXF round-trip.
        assert!(host.document().app_ids.contains("DEMO_SURVEY"));

        // A second write replaces rather than duplicates the record.
        let mut rec2 = ExtendedDataRecord::new("DEMO_SURVEY");
        rec2.add_value(XDataValue::String("PNT-2".to_string()));
        assert!(host.write_record(h, rec2));
        let got = host.read_record(h, "DEMO_SURVEY").unwrap();
        assert_eq!(got.values.len(), 1);

        // Removal reports whether anything was dropped.
        assert!(host.remove_record(h, "DEMO_SURVEY"));
        assert!(host.read_record(h, "DEMO_SURVEY").is_none());
        assert!(!host.remove_record(h, "DEMO_SURVEY"));
    }

    #[test]
    fn plugin_state_round_trips_through_hostapi_trait() {
        use ocs_plugin_api::host::{self, HostApi};
        let mut app = OpenCADStudio::new_for_test();
        let mut session = HostSession::new(&mut app, 0);
        let host: &mut dyn HostApi = &mut session;

        // Absent before first use.
        assert!(host::plugin_state::<u32>(&*host, "opencad.demo").is_none());
        // Insert via ensure, then mutate.
        *host::ensure_plugin_state(host, "opencad.demo", || 7u32) += 1;
        assert_eq!(
            *host::plugin_state::<u32>(&*host, "opencad.demo").unwrap(),
            8
        );
        *host::plugin_state_mut::<u32>(host, "opencad.demo").unwrap() = 100;
        assert_eq!(
            *host::plugin_state::<u32>(&*host, "opencad.demo").unwrap(),
            100
        );
    }

    /// A plugin command: second point commits a Point and ends.
    struct PlacePoint {
        got_first: bool,
    }
    impl ocs_plugin_api::host::InteractiveCommand for PlacePoint {
        fn prompt(&self) -> String {
            "Pick a point".into()
        }
        fn on_point(&mut self, pt: [f64; 3]) -> ocs_plugin_api::host::CommandStep {
            use ocs_plugin_api::host::CommandStep;
            if self.got_first {
                let p = acadrust::entities::Point::at(acadrust::types::Vector3::new(
                    pt[0], pt[1], pt[2],
                ));
                CommandStep::CommitAndEnd(acadrust::EntityType::Point(p))
            } else {
                self.got_first = true;
                CommandStep::NeedPoint
            }
        }
    }

    #[test]
    fn plugin_interactive_command_drives_host_flow() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        {
            let mut host = HostSession::new(&mut app, 0);
            host.start_interactive(Box::new(PlacePoint { got_first: false }));
        }
        assert!(app.tabs[0].active_cmd.is_some());
        for pt in [glam::DVec3::new(0.0, 0.0, 0.0), glam::DVec3::new(5.0, 5.0, 0.0)] {
            let r = app.tabs[0].active_cmd.as_mut().unwrap().on_point(pt);
            let _ = app.apply_cmd_result(r);
        }
        assert_eq!(app.tabs[0].scene.document.entities().count(), 1);
        assert!(app.tabs[0].active_cmd.is_none(), "command should have ended");
    }

    /// A plugin command that picks an existing object, then marks it.
    struct PickThenMark;
    impl ocs_plugin_api::host::InteractiveCommand for PickThenMark {
        fn prompt(&self) -> String {
            "Pick an object".into()
        }
        fn on_point(&mut self, _pt: [f64; 3]) -> ocs_plugin_api::host::CommandStep {
            ocs_plugin_api::host::CommandStep::Cancel
        }
        fn needs_object_pick(&self) -> bool {
            true
        }
        fn on_object_pick(
            &mut self,
            _handle: acadrust::Handle,
            pt: [f64; 3],
        ) -> ocs_plugin_api::host::CommandStep {
            let p =
                acadrust::entities::Point::at(acadrust::types::Vector3::new(pt[0], pt[1], pt[2]));
            ocs_plugin_api::host::CommandStep::CommitAndEnd(acadrust::EntityType::Point(p))
        }
    }

    #[test]
    fn plugin_object_pick_routes_to_command() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let target = {
            let mut host = HostSession::new(&mut app, 0);
            let h = host.add_entity(acadrust::EntityType::Point(acadrust::entities::Point::at(
                acadrust::types::Vector3::new(3.0, 4.0, 0.0),
            )));
            host.start_interactive(Box::new(PickThenMark));
            h
        };
        // The command requested an entity pick, not a free point.
        assert!(app.tabs[0].active_cmd.as_ref().unwrap().needs_entity_pick());
        let r = app.tabs[0]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_entity_pick(target, glam::Vec3::new(3.0, 4.0, 0.0));
        let _ = app.apply_cmd_result(r);
        // Original point + the mark the command committed.
        assert_eq!(app.tabs[0].scene.document.entities().count(), 2);
    }

    #[test]
    fn host_document_reader_sees_entities() {
        use ocs_plugin_api::host::ReaderEntityKind;
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        host.add_entity(acadrust::EntityType::Point(acadrust::entities::Point::at(
            acadrust::types::Vector3::new(7.0, 8.0, 0.0),
        )));
        let reader = host.document_reader();
        assert_eq!(reader.entity_count(), 1);
        let mut kinds = Vec::new();
        reader.for_each_entity(&mut |e| kinds.push(e.kind));
        assert_eq!(kinds, vec![ReaderEntityKind::Point]);
    }

    #[test]
    fn host_document_view_publish_and_read_shared() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let info = host.document_view().unwrap();
        let reader =
            ocs_plugin_api::shm::SharedDocumentReader::open(std::path::Path::new(&info.path))
                .unwrap();
        assert_eq!(reader.entity_count(), 0);

        host.add_entity(acadrust::EntityType::Point(acadrust::entities::Point::at(
            acadrust::types::Vector3::new(1.0, 2.0, 0.0),
        )));

        assert_eq!(reader.entity_count(), 1);
    }

    /// Read an entity handle from the live document, write XDATA for that
    /// handle, read it back, and remove it.
    #[test]
    fn document_reader_to_xdata_roundtrip() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let h = host.add_entity(EntityType::Point(Point::at(acadrust::types::Vector3::new(
            7.0, 8.0, 0.0,
        ))));

        {
            let reader = host.document_reader();
            assert_eq!(reader.entity_count(), 1);
            let mut handles = Vec::new();
            reader.for_each_entity(&mut |e| handles.push(e.handle));
            assert_eq!(handles, vec![h]);
        }

        let mut rec = ExtendedDataRecord::new("ROUNDTRIP");
        rec.add_value(XDataValue::String("from-reader".to_string()));
        assert!(host.write_record(h, rec));

        let got = host.read_record(h, "ROUNDTRIP").expect("record missing");
        assert_eq!(got.values.len(), 1);
        assert!(matches!(got.values[0], XDataValue::String(ref s) if s == "from-reader"));

        assert!(host.remove_record(h, "ROUNDTRIP"));
        assert!(host.read_record(h, "ROUNDTRIP").is_none());
    }

    /// Publish a shared document view, read the entity handle from shared
    /// memory, then write and read-back XDATA through the normal HostApi RPCs.
    #[test]
    fn shared_document_view_read_then_write_xdata_roundtrip() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let info = host.document_view().unwrap();
        let reader =
            ocs_plugin_api::shm::SharedDocumentReader::open(std::path::Path::new(&info.path))
                .unwrap();

        let h = host.add_entity(EntityType::Point(Point::at(acadrust::types::Vector3::new(
            1.0, 2.0, 0.0,
        ))));
        assert_eq!(reader.entity_count(), 1);

        let mut handles = Vec::new();
        reader.for_each_entity(&mut |e| handles.push(e.handle));
        assert_eq!(handles, vec![h]);

        let mut rec = ExtendedDataRecord::new("SHM_ROUNDTRIP");
        rec.add_value(XDataValue::Integer32(123));
        assert!(host.write_record(h, rec));

        let got = host
            .read_record(h, "SHM_ROUNDTRIP")
            .expect("record missing");
        assert_eq!(got.values.len(), 1);
        assert!(matches!(got.values[0], XDataValue::Integer32(123)));
    }
}
