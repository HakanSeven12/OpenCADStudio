//! Plugin identity, API version constants, and compatibility rules.
//!
//! This module defines the host ABI major version (`API_VERSION`), the oldest
//! major the host still loads (`API_VERSION_MIN_SUPPORTED`), and the runtime
//! `OCS_PLUGIN_MAX_API_VERSION` gate that lets operators disable newer API
//! majors without rebuilding.
//!
//! Compatibility rule: a plugin compiled against major `N` runs on any host
//! whose major is `>= N`, because new vtable entries and enum variants are
//! appended at the end. `host_accepts_plugin_version` enforces the supported
//! range after applying the runtime cap.
//!
//! See `ARCHITECTURE.md` in the crate root for the full versioning policy.

/// Host plugin API version. Bump when the host runtime surface breaks
/// compatibility. v2 added `HostApi::start_interactive`. v3 changes
/// `document()` / `document_mut()` to local cached copies for out-of-process
/// plugins and appends `document_reader` / `document_view` at the end of the
/// vtable so API v2 plugins keep working. v4 adds full-duplex notifications on
/// a multiplexed socket while leaving the V2/V3 ABI and protocol untouched. v5
/// adds the `BuiltinPlugin::on_load` lifecycle callback and
/// `HostApi::document_path`. v6 appends generic host system-variable access
/// (`HostApi::system_variable` / `set_system_variable`). v7 appends atomic,
/// undoable replacement of a set of existing entities. v8 appends name-addressed
/// XRecord object CRUD under named-object-dictionary paths
/// (`HostApi::create_xrecord` / `read_xrecord` / `update_xrecord` /
/// `delete_xrecord` / `list_xrecords`). v9 appends the reference-only entity
/// designation (`HostApi::set_reference_only`): display-only geometry a plugin
/// still owns and refreshes while the built-in edit paths treat it like a
/// locked layer. v10 appends `BuiltinPlugin::entity_classes`: plugin-declared
/// XDATA class-identity display labels, folded into the registered manifest.
/// Display-only; the host consults the declaration only while the declaring
/// plugin is loaded, and entities without one keep their native type names.
/// v11 appends the plugin grip surface: `HostApi::entity_grips` (built-in
/// grip enumeration with viewport parity), `HostApi::grip_drag` (the same
/// one-undo-step edit the interactive viewport performs), and
/// `HostApi::set_plugin_grips` plus `BuiltinPlugin::on_custom_grip_event`
/// (plugin-declared, plugin-owned custom grip points: session display state
/// the host renders, hit-tests and drags but never interprets). v12 lands
/// the consumer-validation hardening: the plugin-directed LIVE-GEOMETRY
/// channel (CustomGripResponse::live_entities, capped), grip deliveries
/// that never block the UI loop (bounded polls plus the settle watchdog
/// that tears a silent plugin connection down), nested COMMAND frames
/// refused inside grip deliveries (their execution would need the message
/// loop the blocked delivery cannot pump — the UI-freeze wedge), cheap
/// per-entity reads (HostApi::entities_batch) so refresh passes never
/// re-clone invalidated document snapshots, and the deterministic-boot
/// retry for plugin command dispatch. v13 closes the consumer's validation
/// round: live geometry is VISUAL-ONLY against the parametric solver too
/// (the restyled bump policy — bonded partners no longer move by host
/// action mid-drag), constraint creation is a first-class HostApi write
/// (`HostApi::constraint_create`, joining the caller's open scope like
/// every other write; the driver's `begin_undo` joins plugin scopes
/// instead of closing them — a re-bond composed of writes + constraints +
/// replays is ONE plugin-labeled step), `CommandRequest::RunQuiet` replays
/// commands with the command-line echo suppressed, the notification channel
/// attaches deterministically (`PluginNotification::ChannelReady` ack;
/// boot-window broadcasts defer then flush), boundary notifications carry
/// DIRTY REGIONS (`DocumentChangedRegions`: touched entities + record
/// pairs), and record walks batch (`read_xrecords` / `update_xrecords` /
/// `list_xrecords_with_entries` — one frame per pass, not per record).
///
/// **API v14** (the newest painted release; two arcs landed in it: the
/// runner's self-driving export — no Rust type crosses the library
/// boundary — and the parametric/class envelopes being reshaped IN PLACE
/// before release; api_version stays 14): `stage_parametric_chain` — a
/// declared alignment
/// chain whose driving ramps are declared DATA (the vocabulary any record
/// model restates its laws with) turns into reference-only display
/// geometry staged by the host (exact Line/Arc carriers for the constant
/// classes, the exact from-end-states Hermite for the profile's
/// at-most-quadratic polynomial slope class, measured chords within the
/// declared tolerance for every table law), audited seam-to-seam
/// before anything is staged and capped by
/// [`MAX_PLUGIN_LIVE_ENTITIES`](crate::host::MAX_PLUGIN_LIVE_ENTITIES); and
/// `sample_parametric` — the same engine exposed as a pure placement
/// evaluation. It adds plugin-scoped class objects —
/// `register_plugin_class` / `stage_plugin_class_instance`: a declared
/// entity class (schema version + grip schema + parameter schema) whose
/// instances carry only identity and a host-evaluated display descriptor
/// (the parametric engine is the first host-known kind; kinds append at
/// the end); the staged display set is reference-only and stamped with the
/// class identity key. Class declarations are session state (like the grip
/// sets); records stay the only file truth. `CustomGripResponse` gains the
/// `descriptor` field (serde default): a Begin answer can hand the host a
/// live design so the host re-derives preview geometry locally per move
/// instead of the plugin streaming it (older hosts read `None` and keep
/// the v12 semantics). The declaration's parameter schema —
/// [`PluginClassParameter`](crate::host::PluginClassParameter): name +
/// value kind + editability, serde default so declarations without the
/// field read as an empty schema — and the session's properties pair
/// (`plugin_class_instance_properties` / `update_plugin_class_instance_parameter`)
/// turn a stamped instance's record rows into readable/writable
/// properties: the reading surface renders the declared rows; for rows the
/// declaration marks editable the write path patches the (name, value)
/// entry pair on the instance's design record row through the existing
/// XRecord update machinery (one undo step, the record's dirty
/// notification re-stages the declaring plugin). The values stay the
/// records' truth; the host renders and writes only what the declaration
/// spells, never inventing semantics for a name or kind it does not know.
/// The host's properties panel rides the same core for selections whose
/// XDATA carries a registered class key, and the read half crosses the
/// wire additively (`PluginRequest::PluginClassInstanceProperties` /
/// `PluginResponse::PluginClassInstancePropertiesResult` with the declared
/// [`PluginClassParameterRow`](crate::host::PluginClassParameterRow) rows —
/// appended at the END of the request/response enums; the python `ocs`
/// surface exposes it as `plugin_class_instance_properties`). A curve's
/// driving quantity is declared as DATA — a
/// [`Ramp`](crate::host::Ramp) payload (polynomial, piecewise or sampled
/// table) per segment, the vocabulary any alignment library restates its
/// own laws with (the v14 envelope was reshaped IN PLACE to these generic
/// shapes before release; api_version stays 14 because no deployed
/// plugins pre-date the reshaping) — and the host's generic Gaussian
/// integrator executes it; new blend shapes join as declared data, never
/// as per-shape host code. The drag pipeline consumes
/// the Begin descriptor: [`PluginGripSchemaEntry`](crate::host::PluginGripSchemaEntry)
/// gains an optional `drives` row (a
/// [`PluginGripAnchor`](crate::host::PluginGripAnchor): the segment, the
/// end and the record-model anchor law the grip re-anchors; serde default
/// keeps earlier declarations streaming) and
/// [`CustomGripResponse`](crate::host::CustomGripResponse) gains the
/// accompanying per-move tolerance (the same value the plugin staged its
/// instance with). When a Begin answer hands the host a serviceable
/// descriptor design — a profile-chain END anchor — the host
/// re-derives the display set LOCALLY per coalesced cursor move through
/// the declared anchor law's own generic solve (the ramp's end value
/// re-derives per its arm's integral gain, the downstream joints
/// re-declare from the engine's own sampling, and the chain's own audit
/// gates every move): no Move frames cross the IPC, the per-move restage
/// patches the same staged handles through the visual-only phase (zero
/// history steps, the byte-identical restore before the settle), and the
/// plugin's settle callback stays the ONE write window (records first,
/// then the re-stage; exactly one labeled undo step). Unserved declarations
/// fall back to the v12 streaming path with one headline.
/// v14: the plugin runs the runner loop itself through `ocs_plugin_run`
/// (emitted by `export_plugin!`), so no Rust type crosses the library
/// boundary. The host no longer matches the plugin's compiler or codec commit;
/// it matches the codec types' wire shapes (`version_info::wire_schema`).
pub const API_VERSION: u32 = 14;

/// Oldest plugin API major the current host still loads. This keeps previously
/// compiled cdylibs usable as long as their vtable layout is a prefix of the
/// current `HostApi` trait. V2 is no longer loaded: its ribbon wire types
/// predate the current ones, so a V2 plugin passed the gate and then failed
/// silently while registering its ribbon (#1085, #987).
pub const API_VERSION_MIN_SUPPORTED: u32 = 3;

/// Environment variable that caps the API major accepted by the host at
/// runtime. Set to `5` to disable V6 plugins, `4` to disable V5, or `3` for
/// V3-only mode.
pub const MAX_API_VERSION_ENV: &str = "OCS_PLUGIN_MAX_API_VERSION";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiVersion {
    pub major: u32,
}

impl ApiVersion {
    pub const CURRENT: Self = Self { major: API_VERSION };

    /// True when this plugin version can run on `host`. A plugin is compatible
    /// with any host whose API major is the same or newer (new host methods are
    /// appended at the end of the vtable, so old plugins ignore them).
    pub fn is_compatible_with(&self, host: ApiVersion) -> bool {
        self.major <= host.major
    }
}

/// The effective maximum API major this host accepts, honoring
/// [`MAX_API_VERSION_ENV`]. This lets operators disable a new API major at
/// runtime without rebuilding.
pub fn effective_max_api_version() -> u32 {
    std::env::var(MAX_API_VERSION_ENV)
        .ok()
        .and_then(|s| s.parse().ok())
        .map(|max: u32| max.clamp(API_VERSION_MIN_SUPPORTED, API_VERSION))
        .unwrap_or(API_VERSION)
}

/// True when a plugin built against `plugin_major` can be loaded by this host.
/// The host supports majors from `API_VERSION_MIN_SUPPORTED` up to
/// [`effective_max_api_version`], which can be lowered at runtime via
/// `OCS_PLUGIN_MAX_API_VERSION`.
pub fn host_accepts_plugin_version(plugin_major: u32) -> bool {
    plugin_major >= API_VERSION_MIN_SUPPORTED && plugin_major <= effective_max_api_version()
}

/// A plugin's declaration of the entity classes it identifies via XDATA,
/// surfaced by the host as display-only type labels (the Properties panel
/// title and the automation `display_type` field).
///
/// The split is deliberate: the *identity* is a stable class KEY stamped into
/// the entity's XDATA (one string value per entity, e.g. `"MYAPP-CLASS:LINE"`)
/// and round-trips with the drawing; the *display labels* live in this
/// declaration, so they are plugin-localizable and fixable without rewriting
/// drawings. The declaration is consulted only while the declaring plugin is
/// loaded — a plugin that is not installed, disabled, failed to load, or whose
/// process has died contributes nothing, and its entities display as their
/// bare native type.
///
/// Declared through [`crate::host::BuiltinPlugin::entity_classes`] (API v10);
/// see `docs/plugin-architecture.md` for the full convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityClasses {
    /// XDATA application name whose records carry the class-identity keys
    /// (e.g. `"CVIFC"`). Should be one of the manifest's `xdata_apps` so the
    /// host knows the plugin writes under this id.
    pub app_id: &'static str,
    /// Prefix that marks a string XDATA value as a class-identity key
    /// (e.g. `"CVIFC-CLASS:"`). The remainder of the value is the class key.
    pub class_prefix: &'static str,
    /// Class key → display label (e.g. `"LINE"` → `"Linear Element"`), in
    /// declaration order. Display text never round-trips the drawing.
    pub labels: &'static [(&'static str, &'static str)],
}

#[cfg(test)]
mod tests {
    use crate::test_lock::ENV_LOCK;

    use super::*;

    #[test]
    fn current_matches_const() {
        assert_eq!(ApiVersion::CURRENT.major, API_VERSION);
    }

    #[test]
    fn same_major_is_compatible() {
        assert!(ApiVersion::CURRENT.is_compatible_with(ApiVersion::CURRENT));
        assert!(!ApiVersion::CURRENT.is_compatible_with(ApiVersion {
            major: API_VERSION - 1,
        }));
        // Forward compatibility: a plugin compiled today runs on a future host
        // that only appends new vtable entries.
        assert!(ApiVersion::CURRENT.is_compatible_with(ApiVersion {
            major: API_VERSION + 1,
        }));
    }

    #[test]
    fn api_v2_plugin_runs_on_api_v3_host() {
        assert!(ApiVersion { major: 2 }.is_compatible_with(ApiVersion { major: 3 }));
    }

    #[test]
    fn effective_max_defaults_to_api_version() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(MAX_API_VERSION_ENV);
        assert_eq!(effective_max_api_version(), API_VERSION);
    }

    #[test]
    fn effective_max_can_disable_v4() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(MAX_API_VERSION_ENV, "3");
        assert_eq!(effective_max_api_version(), 3);
        assert!(!host_accepts_plugin_version(4));
        assert!(host_accepts_plugin_version(3));
        assert!(!host_accepts_plugin_version(2));
        std::env::remove_var(MAX_API_VERSION_ENV);
    }

    #[test]
    fn effective_max_below_minimum_is_v3_only_mode() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(MAX_API_VERSION_ENV, "2");
        assert_eq!(effective_max_api_version(), 3);
        assert!(!host_accepts_plugin_version(4));
        assert!(host_accepts_plugin_version(3));
        assert!(!host_accepts_plugin_version(2));
        std::env::remove_var(MAX_API_VERSION_ENV);
    }

    #[test]
    fn effective_max_clamps_to_supported_range() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(MAX_API_VERSION_ENV, "99");
        assert_eq!(effective_max_api_version(), API_VERSION);
        std::env::set_var(MAX_API_VERSION_ENV, "1");
        assert_eq!(effective_max_api_version(), API_VERSION_MIN_SUPPORTED);
        std::env::remove_var(MAX_API_VERSION_ENV);
    }

    #[test]
    fn host_accepts_supported_range() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(MAX_API_VERSION_ENV);
        assert!(host_accepts_plugin_version(API_VERSION_MIN_SUPPORTED));
        assert!(host_accepts_plugin_version(API_VERSION));
        assert!(!host_accepts_plugin_version(API_VERSION + 1));
        assert!(!host_accepts_plugin_version(API_VERSION_MIN_SUPPORTED - 1));
    }
}

/// Static metadata every plugin supplies at registration time.
/// Keep fields in sync with `plugin.toml` beside the package.
#[derive(Clone, Copy, Debug)]
pub struct PluginManifest {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub api_version: ApiVersion,
    /// Sort key for add-on ribbon tabs (lower = further left among plugins).
    pub ribbon_order: i32,
    pub xdata_apps: &'static [&'static str],
    pub command_prefixes: &'static [&'static str],
}
