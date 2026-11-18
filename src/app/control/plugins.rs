//! `plugins` read: the installed packages and the store, with the status the
//! Plugin Manager shows for each. A first read starts the catalog fetch.
//! `plugin_call` execute: run one of a plugin's AI tools (API v25).

use super::failure;
use crate::app::{Message, OpenCADStudio};
use iced::Task;
use serde_json::{json, Value};

impl OpenCADStudio {
    #[cfg(target_arch = "wasm32")]
    pub(super) fn control_plugins(&mut self) -> (Value, Task<Message>) {
        (
            json!({"ok":true,"installed":[],"available":[],"catalog":"unavailable",
                "note":"Plugins are native packages; they run in the desktop app."}),
            Task::none(),
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn control_plugins(&mut self) -> (Value, Task<Message>) {
        use crate::ui::window::plugin_manager::{
            release_compatible, repository_is_installed, InstalledStatus,
        };

        self.external_plugins = crate::plugin::external::discover();
        self.refresh_plugin_tools();
        let installed: Vec<Value> = self
            .external_plugins
            .iter()
            .map(|p| {
                let error = self.plugin_load_errors.get(&p.id);
                let status = InstalledStatus::of(
                    p,
                    self.loaded_plugin_ids.contains(&p.id),
                    self.disabled_plugins.contains(&p.id),
                    error.map(String::as_str),
                );
                json!({
                    "id":p.id,"name":p.name,"version":p.version,"api_version":p.api_version,
                    "repository":p.repository,"description":p.description,
                    "commands":p.command_prefixes,"bundled":p.bundled,
                    "status":status.key(),"error":error,
                    "tools":self.plugin_tools.get(&p.id).map(|tools| tools.iter().map(|tool| json!({
                        "name":tool.name,"description":tool.description,
                        "input_schema":serde_json::from_str::<Value>(&tool.input_schema)
                            .unwrap_or(Value::Null),
                    })).collect::<Vec<_>>()).unwrap_or_default(),
                })
            })
            .collect();

        // The catalog loads with the Plugin Manager; a read before it opened
        // starts the same fetches.
        let mut tasks = Vec::new();
        if self.plugin_registry.is_empty()
            && !self.plugin_registry_loading
            && self.plugin_registry_error.is_none()
        {
            self.plugin_registry_loading = true;
            tasks.push(self.fetch_registry_task());
        }
        let linked: Vec<String> = self
            .plugin_repos
            .iter()
            .filter(|repo| {
                !self.repo_release_tags.contains_key(*repo)
                    && !self.plugin_releases_loading.contains(*repo)
            })
            .cloned()
            .collect();
        for repo in linked {
            tasks.push(self.fetch_releases_task(repo));
        }
        let catalog = if self.plugin_registry_loading {
            "loading"
        } else if self.plugin_registry_error.is_some() {
            "error"
        } else {
            "ready"
        };

        let entries = self
            .plugin_registry
            .iter()
            .map(|e| (e.repo.as_str(), Some(e.name.as_str()), e.description.as_str(), "registry"))
            .chain(
                self.plugin_repos
                    .iter()
                    .filter(|repo| !self.plugin_registry.iter().any(|e| &e.repo == *repo))
                    .map(|repo| (repo.as_str(), None, "", "linked")),
            );
        let available: Vec<Value> = entries
            .map(|(repo, name, description, source)| {
                let releases = self.repo_release_tags.get(repo);
                let status = if self.plugin_installing.contains(repo) {
                    "installing"
                } else if self.plugin_installed_now.contains_key(repo) {
                    "restart_to_load"
                } else if repository_is_installed(repo, name, &self.external_plugins) {
                    "installed"
                } else if self.plugin_releases_loading.contains(repo) {
                    "checking_releases"
                } else {
                    match releases {
                        Some(list) if list.iter().any(release_compatible) => "installable",
                        Some(_) => "incompatible",
                        None => "unknown",
                    }
                };
                json!({
                    "repository":repo,"name":name.unwrap_or(repo),"description":description,
                    "source":source,"status":status,
                    "releases":releases.map(|list| list.iter().map(|r| json!({
                        "tag":r.tag,"api_version":r.api_version,"compatible":release_compatible(r),
                    })).collect::<Vec<_>>()),
                })
            })
            .collect();

        let mut response = json!({
            "ok":true,"catalog":catalog,"host_api_version":ocs_plugin_api::API_VERSION,
            "installed":installed,"available":available,
        });
        if catalog == "loading" || !self.plugin_releases_loading.is_empty() {
            response["hint"] = json!("The catalog is still loading; read plugins again shortly.");
        }
        if let Some(error) = &self.plugin_registry_error {
            response["catalog_error"] = json!(error);
        }
        (response, Task::batch(tasks))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl OpenCADStudio {
    /// Ask every loaded plugin for its AI tools; a plugin that fails to
    /// answer simply offers none.
    pub(in crate::app) fn refresh_plugin_tools(&mut self) {
        let processes = crate::plugin::external::with_manager(|manager| {
            manager
                .alive_ids()
                .into_iter()
                .filter_map(|id| manager.process(&id).map(|process| (id, process)))
                .collect::<Vec<_>>()
        });
        self.plugin_tools = processes
            .into_iter()
            .map(|(id, process)| (id, process.tools().unwrap_or_default()))
            .filter(|(_, tools)| !tools.is_empty())
            .collect();
    }

    /// `{"op":"plugin_call","plugin":id,"tool":name,"arguments":{…}}`: the
    /// plugin must be loaded and enabled; the arguments must match the
    /// tool's schema. The whole call is one undo
    /// step named after the tool.
    pub(super) fn control_plugin_call(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let plugin = super::string(req, "plugin")?.to_string();
        let tool = super::string(req, "tool")?.to_string();
        let arguments = match &req["arguments"] {
            Value::Null => json!({}),
            Value::Object(_) => req["arguments"].clone(),
            _ => return Err(failure("invalid_request", "arguments must be an object")),
        };
        if self.disabled_plugins.contains(&plugin) {
            return Err(failure("plugin_disabled", format!("Plugin '{plugin}' is disabled")));
        }
        let Some(process) =
            crate::plugin::external::with_manager(|manager| manager.process(&plugin))
                .filter(|process| process.is_alive())
        else {
            return Err(failure("plugin_not_loaded", format!("Plugin '{plugin}' is not running")));
        };
        if !self.plugin_tools.contains_key(&plugin) {
            self.refresh_plugin_tools();
        }
        let Some(definition) = self
            .plugin_tools
            .get(&plugin)
            .and_then(|tools| tools.iter().find(|t| t.name == tool))
            .cloned()
        else {
            return Err(failure(
                "unknown_tool",
                format!("Plugin '{plugin}' offers no tool '{tool}'; read op plugins for its tools"),
            ));
        };
        // The schema is the plugin's, not ours: a broken one refuses the
        // call instead of panicking the host.
        let schema: Value = serde_json::from_str(&definition.input_schema).map_err(|e| {
            failure("invalid_tool_schema", format!("Tool '{tool}' has an unreadable schema: {e}"))
        })?;
        let validator = jsonschema::validator_for(&schema).map_err(|e| {
            failure("invalid_tool_schema", format!("Tool '{tool}' has an invalid schema: {e}"))
        })?;
        if let Err(error) = validator.validate(&arguments) {
            return Err(failure(
                "invalid_arguments",
                format!("arguments{}: {error}", error.instance_path()),
            ));
        }

        let tab = self.active_tab;
        let label = format!("{} {}", plugin, tool);
        let outcome = {
            let mut host = crate::plugin::host::HostSession::new(self, tab);
            host.begin_undo_group(&label);
            let outcome = process.call_tool(&mut host, &tool, &arguments.to_string());
            host.end_undo_group();
            outcome
        };
        self.refresh_properties();
        match outcome {
            Ok(Ok(text)) => {
                let result = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
                self.set_control_result(json!({"plugin":plugin,"tool":tool,"result":result}));
                Ok(Task::none())
            }
            Ok(Err(message)) => Err(failure("tool_failed", message)),
            Err(error) => Err(failure("plugin_error", error.to_string())),
        }
    }
}
