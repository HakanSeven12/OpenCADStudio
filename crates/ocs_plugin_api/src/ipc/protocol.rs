//! Request/response envelopes exchanged between the host and a plugin process.
//!
//! A single bidirectional socket is used. Each side sends either a request
//! (expecting a response) or a response (to a previous request). This lets the
//! host handle plugin RPCs inline while it waits for the result of a host→plugin
//! request such as `Dispatch`, avoiding the need for two sockets or threads.
//!
//! Compatibility rule: new variants are appended at the end of every public enum
//! (`HostRequest`, `HostResponse`, `PluginRequest`, `PluginResponse`,
//! `RunnerHandshake`) so older plugins keep their bincode discriminant indices.
//! The V4 additions are a separate frame layer in [`crate::ipc::v4`] and do not
//! alter these enums.
//!
//! The pre-shared runner token is delivered through [`PLUGIN_TOKEN_ENV`]
//! (`OCS_PLUGIN_TOKEN`). The runner must present the same token immediately
//! after connecting or the host closes the connection.

use serde::{Deserialize, Serialize};

use crate::host::{CommandSource, CommandStep};
use crate::manifest::ApiVersion;
use crate::ribbon::owned::{OwnedPluginManifest, OwnedRibbonGroup};

pub use acadrust::xdata::{ExtendedDataRecord, XDataValue};
pub use acadrust::{CadDocument, EntityType, Handle};

/// Events the host forwards to an active plugin `InteractiveCommand`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InteractiveEvent {
    Point([f64; 3]),
    Enter,
    ObjectPick { handle: Handle, pt: [f64; 3] },
}

/// Initial handshake sent by the plugin runner immediately after connecting.
///
/// The runner proves it was spawned by this host by presenting a pre-shared
/// token delivered through the `OCS_PLUGIN_TOKEN` environment variable. A
/// mismatch causes the host to close the connection.
///
/// `TokenV4` is appended as the last variant so the existing `Token(String)`
/// variant keeps its bincode discriminant index (0), preserving V2/V3 wire
/// compatibility.
#[derive(Debug, Serialize, Deserialize)]
pub enum RunnerHandshake {
    Token(String),
    TokenV4 { token: String, protocol_version: u32 },
}

/// Environment variable through which the host passes the pre-shared
/// authentication token to the plugin runner child process.
pub const PLUGIN_TOKEN_ENV: &str = "OCS_PLUGIN_TOKEN";

/// Requests the host sends to the plugin runner.
#[derive(Debug, Serialize, Deserialize)]
pub enum HostRequest {
    GetManifest,
    GetRibbon,
    Dispatch {
        cmd: String,
    },
    InteractiveEvent {
        command_id: u64,
        event: InteractiveEvent,
    },
    GetPrompt {
        command_id: u64,
    },
    NeedsEntityPick {
        command_id: u64,
    },
    Shutdown,
    ExecuteCode {
        command_id: u64,
        source: CommandSource,
        code: String,
        tab_index: usize,
    },
}

/// Responses the plugin runner sends back for `HostRequest`.
#[derive(Debug, Serialize, Deserialize)]
pub enum HostResponse {
    Bool(bool),
    CommandStep(Box<CommandStep>),
    Text(String),
    Ribbon(Vec<OwnedRibbonGroup>),
    Manifest(OwnedPluginManifest),
    Error(String),
    CodeExecutionResult(crate::host::ExecutionResult),
}

/// Requests the plugin runner sends to the host.
#[derive(Debug, Serialize, Deserialize)]
pub enum PluginRequest {
    PushInfo(String),
    PushOutput(String),
    PushError(String),
    AddEntity(EntityType),
    /// Replace the existing entity carrying this entity's handle in place.
    UpdateEntity(EntityType),
    /// Delete the entity with `handle`.
    RemoveEntity {
        handle: Handle,
    },
    BumpGeometry,
    ReadRecord {
        handle: Handle,
        app_name: String,
    },
    WriteRecord {
        handle: Handle,
        record: ExtendedDataRecord,
    },
    RemoveRecord {
        handle: Handle,
        app_name: String,
    },
    PushUndo {
        label: String,
    },
    SetDirty,
    StartInteractive {
        command_id: u64,
    },
    DocumentSnapshot,
    /// Ask the host to create/refresh a shared-memory document view and return
    /// the file path + current version.
    OpenDocumentView,
    /// Add multiple entities in a single request.
    AddEntities(Vec<EntityType>),
    /// V4: ask the host to create/refresh a tab-keyed shared-memory document
    /// view and return the file path + current version.
    OpenDocumentViewV4 { tab_id: u64 },
    /// V4: ask the host to close the tab-keyed shared-memory document view.
    CloseDocumentViewV4 { tab_id: u64 },
    /// V4: ask the host for the stable tab identifier of the active tab.
    GetTabId,
    /// V5: ask the host for the filesystem path of the document in `tab_id`.
    DocumentPath { tab_id: u64 },
}

/// Responses the host sends back for `PluginRequest`.
#[derive(Debug, Serialize, Deserialize)]
pub enum PluginResponse {
    Ok,
    Bool(bool),
    Handle(Handle),
    Record(Option<ExtendedDataRecord>),
    Document(Box<CadDocument>),
    Error(String),
    /// Path to the memory-mapped file and the current snapshot version.
    DocumentView {
        path: String,
        version: u64,
    },
    Handles(Vec<Handle>),
    /// V4: path to the tab-keyed memory-mapped file and current version.
    DocumentViewV4 {
        path: String,
        version: u64,
    },
    /// V4: stable tab identifier of the active tab.
    TabId(u64),
    /// V5: filesystem path of the document in the requested tab, if any.
    DocumentPath(Option<std::ffi::OsString>),
}

/// Messages sent from the host to the plugin runner.
#[derive(Debug, Serialize, Deserialize)]
pub enum HostToPlugin {
    Request(HostRequest),
    Response(Box<PluginResponse>),
}

/// Messages sent from the plugin runner to the host.
#[derive(Debug, Serialize, Deserialize)]
pub enum PluginToHost {
    Request(Box<PluginRequest>),
    Response(HostResponse),
}

/// Convenience helper for manifest serialization.
impl From<&'static crate::manifest::PluginManifest> for OwnedPluginManifest {
    fn from(m: &'static crate::manifest::PluginManifest) -> Self {
        Self {
            id: m.id.to_string(),
            name: m.name.to_string(),
            version: m.version.to_string(),
            description: m.description.to_string(),
            api_version: m.api_version.major,
            ribbon_order: m.ribbon_order,
            xdata_apps: m.xdata_apps.iter().map(|s| s.to_string()).collect(),
            command_prefixes: m.command_prefixes.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl OwnedPluginManifest {
    pub fn api_version(&self) -> ApiVersion {
        ApiVersion {
            major: self.api_version,
        }
    }
}
