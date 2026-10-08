//! Python scripting plugin for Open CAD Studio.
//!
//! Phase 1.2 of `ROADMAP.md`: a read-only `ocs` module (`ocs.selection()`,
//! `ocs.get(handle)`) plus `PY_EVAL <expr>` to evaluate one inline Python
//! expression. Phase 1.3: 2D write access (`ocs.add_line`/`ocs.add_circle`)
//! plus `PY_RUN <path.py>` to run a script file. The experimental
//! `ocs.command("...")` / `ocs.select(handles)` functions remain disabled in
//! the bundled build because nested command replay can hang.

#[cfg(feature = "experimental-host-model")]
mod event_cache;
mod host_ctx;
#[cfg(feature = "experimental-host-model")]
mod input_cache;
mod ocs_module;
mod selection_cache;

use ocs_plugin_api::host::{BuiltinPlugin, HostApi, HostNotification};
use ocs_plugin_api::manifest::{ApiVersion, PluginManifest};
use ocs_plugin_api::ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub(crate) const PLUGIN_ID: &str = "opencad.python";

// Keep these fields in sync with `plugin.toml`.
static MANIFEST: PluginManifest = PluginManifest {
    id: PLUGIN_ID,
    name: "Python Scripting",
    version: env!("CARGO_PKG_VERSION"),
    description: "Embedded Python scripting (RustPython) for Open CAD Studio.",
    api_version: ApiVersion::CURRENT,
    ribbon_order: 60,
    xdata_apps: &[],
    command_prefixes: &["PY_"],
};

/// The ribbon tab.
struct PythonModule;

impl CadModule for PythonModule {
    fn id(&self) -> &'static str {
        "opencad_python"
    }
    fn title(&self) -> &'static str {
        "Python Scripting"
    }
    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![RibbonGroup {
                title: "Tools",
                tools: vec![
                    RibbonItem::LargeTool(ToolDef {
                        id: "PY_EVAL",
                        label: "Eval 1+1",
                        icon: IconKind::Glyph("★"),
                        event: ModuleEvent::Command("PY_EVAL 1 + 1".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "PY_RUN",
                        label: "Run Script…",
                        icon: IconKind::Glyph("▶"),
                        event: ModuleEvent::PluginFileDialog {
                            command: "PY_RUN".to_string(),
                            title: "Run Python Script".to_string(),
                            filter_name: "Python Scripts".to_string(),
                            extensions: vec!["py".to_string()],
                        },
                    }),
                ],
            }]
        })
    }
}

/// The plugin entry point.
struct PythonPlugin;

impl BuiltinPlugin for PythonPlugin {
    fn manifest(&self) -> &'static PluginManifest {
        &MANIFEST
    }
    fn ribbon(&self) -> Box<dyn CadModule> {
        Box::new(PythonModule)
    }
    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool {
        if let Some(rest) = cmd.strip_prefix("PY_EVAL") {
            let source = rest.trim();
            if source.is_empty() {
                host.push_error("PY_EVAL: expected a Python expression, e.g. PY_EVAL 1 + 1");
            } else {
                run_eval(host, source);
            }
            return true;
        }
        if let Some(rest) = cmd.strip_prefix("PY_RUN") {
            let path = rest.trim();
            if path.is_empty() {
                host.push_error("PY_RUN: expected a file path, e.g. PY_RUN script.py");
            } else {
                run_file(host, path);
            }
            return true;
        }
        false
    }

    /// The runner's own event loop drains `HostApi::try_recv_notification()`
    /// *before* any `Dispatch` request reaches `dispatch()` — calling it from
    /// there always sees an empty queue. This callback is the only place a
    /// V4+ plugin actually observes notifications; see `selection_cache.rs`.
    fn on_notification(&mut self, _command_id: Option<u64>, notification: HostNotification) {
        #[cfg(feature = "experimental-host-model")]
        event_cache::observe(&notification);
        match notification {
            HostNotification::SelectionChanged { handles } => selection_cache::set(handles),
            HostNotification::SelectionChangedV4 { handles, .. } => selection_cache::set(handles),
            _ => {}
        }
    }
}

/// Evaluate one Python expression against a fresh interpreter (no session
/// persistence yet — that's a Phase 3 open question) and report the result or
/// error via the command line. A script bug must never look like a silent
/// no-op, so both paths always push something.
fn run_eval(host: &mut dyn HostApi, source: &str) {
    let guard = host_ctx::HostGuard::set(host, "PY_EVAL");

    let builder = rustpython_vm::Interpreter::builder(Default::default());
    let ocs_def = ocs_module::module_def(&builder.ctx);
    let interp = builder.add_native_module(ocs_def).build();

    let outcome = interp.enter(|vm| -> Result<(String, String), String> {
        let output = install_output_capture(vm)?;
        let scope = vm.new_scope_with_builtins();
        // Pre-bind `ocs` in scope so a one-expression `PY_EVAL` can call
        // `ocs.selection()` directly — `Mode::Eval` only accepts a single
        // expression, so a script has no way to run an `import` statement
        // itself.
        let ocs = vm
            .import("ocs", 0)
            .map_err(|exc| exception_to_string(vm, &exc))?;
        scope
            .globals
            .set_item("ocs", ocs, vm)
            .map_err(|exc| exception_to_string(vm, &exc))?;
        #[cfg(feature = "experimental-host-model")]
        install_document_model(vm, &scope)?;
        let code_obj = vm
            .compile(
                source,
                rustpython_vm::compiler::Mode::Eval,
                "<py_eval>".to_string(),
            )
            .map_err(|err| err.to_string())?;
        let result = match vm.run_code_obj(code_obj, scope) {
            Ok(result) => result
                .str(vm)
                .map(|s| s.to_string())
                .map_err(|exc| exception_to_string(vm, &exc)),
            Err(exc) => Err(exception_to_string(vm, &exc)),
        }?;
        Ok((result, captured_output(vm, &output)?))
    });

    drop(guard);

    match outcome {
        Ok((text, printed)) => {
            if !printed.is_empty() {
                host.push_output(&printed);
            }
            host.push_output(&text);
        }
        Err(err) => host.push_error(&err),
    }
}

/// Run a `.py` file (`PY_RUN <path>`) — same interpreter setup as `PY_EVAL`,
/// but `Mode::Exec` (statements, not a single expression) since a file can
/// hold a whole script. Reports errors the same way as `PY_EVAL`; on success
/// reports that the script ran rather than a value (`Mode::Exec` doesn't
/// produce one).
fn run_file(host: &mut dyn HostApi, path: &str) {
    let source = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            host.push_error(&format!("PY_RUN: cannot read {path}: {err}"));
            return;
        }
    };

    let guard = host_ctx::HostGuard::set(host, "PY_RUN");

    let builder = rustpython_vm::Interpreter::builder(Default::default());
    let ocs_def = ocs_module::module_def(&builder.ctx);
    let interp = builder.add_native_module(ocs_def).build();

    let outcome = interp.enter(|vm| -> Result<String, String> {
        let output = install_output_capture(vm)?;
        let scope = vm.new_scope_with_builtins();
        let ocs = vm
            .import("ocs", 0)
            .map_err(|exc| exception_to_string(vm, &exc))?;
        scope
            .globals
            .set_item("ocs", ocs, vm)
            .map_err(|exc| exception_to_string(vm, &exc))?;
        #[cfg(feature = "experimental-host-model")]
        install_document_model(vm, &scope)?;
        let code_obj = vm
            .compile(
                &source,
                rustpython_vm::compiler::Mode::Exec,
                path.to_string(),
            )
            .map_err(|err| err.to_string())?;
        vm.run_code_obj(code_obj, scope)
            .map_err(|exc| exception_to_string(vm, &exc))?;
        captured_output(vm, &output)
    });

    drop(guard);

    match outcome {
        Ok(printed) => {
            if !printed.is_empty() {
                host.push_output(&printed);
            }
            host.push_output(&format!("PY_RUN: {path} completed"));
        }
        Err(err) => host.push_error(&err),
    }
}

#[cfg(feature = "experimental-host-model")]
fn install_document_model(
    vm: &rustpython_vm::VirtualMachine,
    scope: &rustpython_vm::scope::Scope,
) -> Result<(), String> {
    let code = vm
        .compile(
            include_str!("document_model.py"),
            rustpython_vm::compiler::Mode::Exec,
            "<ocs_document_model>".to_owned(),
        )
        .map_err(|error| error.to_string())?;
    vm.run_code_obj(code, scope.clone())
        .map_err(|exc| exception_to_string(vm, &exc))?;
    Ok(())
}

#[cfg(all(test, feature = "experimental-host-model"))]
#[test]
fn document_model_bootstraps_in_rustpython() {
    let builder = rustpython_vm::Interpreter::builder(Default::default());
    let ocs_def = ocs_module::module_def(&builder.ctx);
    let interp = builder.add_native_module(ocs_def).build();
    interp.enter(|vm| {
        let scope = vm.new_scope_with_builtins();
        let ocs = vm.import("ocs", 0).unwrap();
        scope.globals.set_item("ocs", ocs, vm).unwrap();
        install_document_model(vm, &scope).unwrap();
        let code = vm
            .compile(
                "ocs.active_document.transaction('Move line').label",
                rustpython_vm::compiler::Mode::Eval,
                "<model_test>".to_owned(),
            )
            .unwrap();
        let label = vm.run_code_obj(code, scope).unwrap();
        assert_eq!(label.str(vm).unwrap().to_string(), "Move line");
    });
}

/// RustPython is built without its stdio feature, so sys.stdout/stderr are
/// None by default. Give each fresh interpreter a memory stream for print().
fn install_output_capture(
    vm: &rustpython_vm::VirtualMachine,
) -> Result<rustpython_vm::PyObjectRef, String> {
    let sys = vm
        .import("sys", 0)
        .map_err(|exc| exception_to_string(vm, &exc))?;
    let io = vm
        .import("_io", 0)
        .map_err(|exc| exception_to_string(vm, &exc))?;
    let output = vm
        .call_method(&io, "StringIO", ())
        .map_err(|exc| exception_to_string(vm, &exc))?;
    sys.set_attr("stdout", output.clone(), vm)
        .map_err(|exc| exception_to_string(vm, &exc))?;
    sys.set_attr("stderr", output.clone(), vm)
        .map_err(|exc| exception_to_string(vm, &exc))?;
    Ok(output)
}

fn captured_output(
    vm: &rustpython_vm::VirtualMachine,
    output: &rustpython_vm::PyObjectRef,
) -> Result<String, String> {
    vm.call_method(output, "getvalue", ())
        .and_then(|value| value.str(vm))
        .map(|value| value.to_string().trim_end_matches('\n').to_owned())
        .map_err(|exc| exception_to_string(vm, &exc))
}

/// `"<TypeName>: <message>"`, the same shape `str(exception)` plus its type
/// name produces in CPython — no full traceback yet (Phase 1.2 is minimal;
/// see the roadmap's error-handling note).
fn exception_to_string(
    vm: &rustpython_vm::VirtualMachine,
    exc: &rustpython_vm::builtins::PyBaseExceptionRef,
) -> String {
    let exc_obj: rustpython_vm::PyObjectRef = exc.clone().into();
    let type_name = exc_obj.class().name().to_string();
    let message = exc_obj.str(vm).map(|s| s.to_string()).unwrap_or_default();
    format!("{type_name}: {message}")
}

// Emit the C-ABI symbols the host loader looks for.
ocs_plugin_api::export_plugin!(PythonPlugin);
