//! Ribbon description types — the plain-data vocabulary a [`CadModule`] uses to
//! declare its tab. No UI-framework dependency: the host renders these.

#[cfg(feature = "host")]
pub mod owned;

// ── Events ────────────────────────────────────────────────────────────────

/// Events a module tool can emit to the host application.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "host", derive(serde::Serialize, serde::Deserialize))]
pub enum ModuleEvent {
    /// Fire a named CAD command (e.g. "LINE", "CIRCLE").
    Command(String),
    /// Open the OS file dialog.
    OpenFileDialog,
    /// Remove all loaded models from the scene.
    #[allow(dead_code)]
    ClearModels,
    /// Put the active viewport in a named visual style.
    ///
    /// The name is one of the render modes the viewport can be drawn in:
    /// `WIREFRAME2D`, `WIREFRAME3D`, `HIDDENLINE`, `FLATSHADED`,
    /// `GOURAUDSHADED`, `FLATSHADEDWITHEDGES`, `GOURAUDSHADEDWITHEDGES`
    /// (case-insensitive). The host resolves it against the same list its own
    /// ribbon and commands read, so an add-on offers exactly the styles the
    /// application does. An unknown name is reported and nothing changes.
    SetVisualStyle(String),
    /// Toggle the layer manager panel.
    ToggleLayers,
    /// Ask the host to open a native file picker. On selection the host
    /// dispatches `"<command> <path>"` back to the plugin (full original case,
    /// bypassing the command line so case-sensitive paths/args survive); on
    /// cancel nothing happens. Lets an add-on import files without owning any
    /// dialog UI.
    PluginFileDialog {
        /// Plugin command to dispatch with the chosen path appended.
        command: String,
        /// Dialog window title.
        title: String,
        /// Human label for the file-type filter (e.g. "PNEZD Points").
        filter_name: String,
        /// Accepted extensions, without the dot (e.g. `["csv", "txt"]`).
        extensions: Vec<String>,
    },
}

// ── Data types ────────────────────────────────────────────────────────────

/// Icon source for a ribbon tool button.
#[derive(Clone, Copy)]
pub enum IconKind {
    /// Unicode glyph rendered as text (fast, no file needed).
    Glyph(&'static str),
    /// Raw SVG bytes embedded at compile time via `include_bytes!`.
    Svg(&'static [u8]),
}

/// A single tool button shown in the ribbon.
#[derive(Clone)]
pub struct ToolDef {
    /// Unique command id, e.g. "LINE".
    pub id: &'static str,
    /// Short label shown under the icon.
    pub label: &'static str,
    /// Icon — either a unicode glyph or embedded SVG bytes.
    pub icon: IconKind,
    /// Event emitted when the tool is clicked.
    pub event: ModuleEvent,
}

/// One item in a ribbon group — plain button or dropdown, in small (1-row) or large (3-row) size.
#[derive(Clone)]
pub enum RibbonItem {
    /// 1-row button — icon only, no label.
    Tool(ToolDef),
    /// 1-row button — icon and label.
    LabeledTool(ToolDef),
    /// 3-row button — icon + label below; full ribbon height.
    LargeTool(ToolDef),
    /// 1-row dropdown — icon + ▾ on right, no label.
    Dropdown {
        id: &'static str,
        icon: IconKind,
        items: Vec<(&'static str, &'static str, IconKind)>,
        default: &'static str,
    },
    /// 1-row split dropdown — icon and fixed label, plus a menu arrow.
    LabeledDropdown {
        id: &'static str,
        label: &'static str,
        icon: IconKind,
        items: Vec<(&'static str, &'static str, IconKind)>,
        default: &'static str,
    },
    /// 3-row dropdown — icon + label + ▾ below label; full ribbon height.
    LargeDropdown {
        id: &'static str,
        label: &'static str,
        icon: IconKind,
        items: Vec<(&'static str, &'static str, IconKind)>,
        default: &'static str,
    },
    /// Explicit columns of small icon-only buttons.
    ToolGrid { columns: Vec<Vec<ToolDef>> },
    /// Layer combo + two rows of small tools below.
    /// row2: operates on the layer of a selected object (off/freeze/lock/make-current)
    /// row3: all-layers operations + match (on/thaw/unlock/match)
    LayerComboGroup {
        row2: Vec<ToolDef>,
        row3: Vec<ToolDef>,
    },
    /// Match Properties (large button) + Color / Linetype / Lineweight combos on the right.
    PropertiesGroup { match_prop: ToolDef },
    /// A style selector combobox (text / dim / mleader / table style) with
    /// optional small tool rows below it.
    StyleComboGroup {
        /// Which style domain this combo controls.
        style_key: StyleKey,
        /// Unique dropdown id (must be unique across the ribbon).
        combo_id: &'static str,
        /// Optional command to run when the user opens the style manager.
        manager_cmd: Option<&'static str>,
        /// Small tool rows rendered below the combo (0–2 rows).
        rows: Vec<Vec<ToolDef>>,
    },
}

/// A [`RibbonItem::LargeDropdown`] label that shows the icon and name of the
/// item last chosen, which the face then runs.
pub const FOLLOW_CHOSEN: &str = "\u{1}";

/// Identifies which style list a `StyleComboGroup` refers to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "host", derive(serde::Serialize, serde::Deserialize))]
pub enum StyleKey {
    TextStyle,
    DimStyle,
    MLeaderStyle,
    TableStyle,
}

impl From<ToolDef> for RibbonItem {
    fn from(t: ToolDef) -> Self {
        RibbonItem::Tool(t)
    }
}

/// A named group of tool buttons shown together in the ribbon.
#[derive(Clone)]
pub struct RibbonGroup {
    pub title: &'static str,
    pub tools: Vec<RibbonItem>,
}

// ── Trait ─────────────────────────────────────────────────────────────────

/// A CAD module owns a set of ribbon groups shown when its tab is active.
/// Each module is a stateless unit struct — all UI state lives in Ribbon.
pub trait CadModule: Send + Sync {
    #[allow(dead_code)]
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn ribbon_groups(&self) -> &[RibbonGroup];
}

#[cfg(test)]
mod tests {
    use super::*;

    // Timing microbenchmark, not a correctness check: it intentionally
    // `Box::leak`s ~400k strings to defeat the allocator's reuse, so keep it out
    // of the default suite. Run manually with `cargo test -- --ignored`.
    // Correctness of the cache is covered by
    // `once_lock_produces_identical_pointer_on_subsequent_calls`.
    #[test]
    #[ignore = "microbenchmark: leaks strings by design; run with --ignored"]
    fn once_lock_eliminates_allocation_after_first_call() {
        // Helper to build a realistic module tree (2 groups, ~20 items)
        // Benchmark builds this manually for a clean before/after comparison.
        fn build_tree() -> Vec<RibbonGroup> {
            vec![
                RibbonGroup {
                    title: "Draw",
                    tools: (0..10)
                        .map(|i| {
                            RibbonItem::Tool(ToolDef {
                                id: &*Box::leak(format!("TOOL_{i}").into_boxed_str()),
                                label: &*Box::leak(format!("Tool {i}").into_boxed_str()),
                                icon: IconKind::Glyph("T"),
                                event: ModuleEvent::Command(format!("CMD_{i}")),
                            })
                        })
                        .collect(),
                },
                RibbonGroup {
                    title: "Modify",
                    tools: (0..10)
                        .map(|i| {
                            RibbonItem::Tool(ToolDef {
                                id: &*Box::leak(format!("MOD_{i}").into_boxed_str()),
                                label: &*Box::leak(format!("Mod {i}").into_boxed_str()),
                                icon: IconKind::Glyph("M"),
                                event: ModuleEvent::Command(format!("MODIFY_{i}")),
                            })
                        })
                        .collect(),
                },
            ]
        }

        // ── Before: rebuild the tree every call (what every frame used to pay) ──
        const N: usize = 10_000;
        let start = std::time::Instant::now();
        let mut total_len = 0usize;
        for _ in 0..N {
            let groups = std::hint::black_box(build_tree());
            total_len += std::hint::black_box(groups.len());
            // Prevent optimizer from reusing the allocation across iterations
            // by leaking the Vec — otherwise LLVM coalesces the Vec into a
            // single allocation for the whole loop, which under-counts the
            // real per-frame cost. black_box on .as_ptr() forces the box to
            // be materialised even when nothing else consumes it.
            std::hint::black_box(groups.as_ptr());
        }
        let before_elapsed = start.elapsed();
        eprintln!(
            "BEFORE (rebuild each call): {N} builds in {before_elapsed:?} \
             ({:.1} µs/build, total_len={total_len})",
            before_elapsed.as_secs_f64() * 1_000_000.0 / N as f64,
        );

        // ── After: cached via OnceLock ──
        struct Cached;
        impl CadModule for Cached {
            fn id(&self) -> &'static str {
                "cached"
            }
            fn title(&self) -> &'static str {
                "Cached"
            }
            fn ribbon_groups(&self) -> &[RibbonGroup] {
                static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
                GROUPS.get_or_init(build_tree)
            }
        }
        let m = Cached;
        let _ = m.ribbon_groups(); // warm-up — pays construction cost once

        let start = std::time::Instant::now();
        let mut total_len = 0usize;
        for _ in 0..N {
            let groups = std::hint::black_box(m.ribbon_groups());
            total_len += std::hint::black_box(groups.len());
        }
        let after_elapsed = start.elapsed();
        eprintln!(
            "AFTER (cached): {N} calls in {after_elapsed:?} \
             ({:.1} ns/call, total_len={total_len})",
            after_elapsed.as_secs_f64() * 1_000_000_000.0 / N as f64,
        );

        // Ratio: serialize cost in ns/call to avoid division by zero
        let before_ns = before_elapsed.as_nanos() as f64 / N as f64;
        let after_ns = after_elapsed.as_nanos() as f64 / N as f64;
        let ratio = if after_ns > 0.0 {
            before_ns / after_ns
        } else {
            f64::INFINITY
        };
        eprintln!(
            "BEFORE vs AFTER: {before_ns:.0} ns/call vs {after_ns:.1} ns/call ({ratio:.0}× faster)"
        );

        assert_eq!(total_len, 2 * N, "each call must return 2 groups");
    }

    #[test]
    fn tool_def_converts_into_small_tool() {
        let tool = ToolDef {
            id: "LINE",
            label: "Line",
            icon: IconKind::Glyph("／"),
            event: ModuleEvent::Command("LINE".to_string()),
        };
        assert!(matches!(RibbonItem::from(tool), RibbonItem::Tool(_)));
    }

    #[test]
    fn once_lock_produces_identical_pointer_on_subsequent_calls() {
        struct Demo;
        impl CadModule for Demo {
            fn id(&self) -> &'static str {
                "demo"
            }
            fn title(&self) -> &'static str {
                "Demo"
            }
            fn ribbon_groups(&self) -> &[RibbonGroup] {
                static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
                GROUPS.get_or_init(|| {
                    vec![RibbonGroup {
                        title: "Group",
                        tools: vec![RibbonItem::Tool(ToolDef {
                            id: "LINE",
                            label: "Line",
                            icon: IconKind::Glyph("／"),
                            event: ModuleEvent::Command("LINE".to_string()),
                        })],
                    }]
                })
            }
        }
        let m = Demo;
        let first: *const [RibbonGroup] = m.ribbon_groups();
        let second: *const [RibbonGroup] = m.ribbon_groups();
        assert_eq!(first, second, "cached calls must return the same pointer");
    }

    #[test]
    fn cad_module_is_object_safe() {
        struct Demo;
        impl CadModule for Demo {
            fn id(&self) -> &'static str {
                "demo"
            }
            fn title(&self) -> &'static str {
                "Demo"
            }
            fn ribbon_groups(&self) -> &[RibbonGroup] {
                static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
                GROUPS.get_or_init(|| {
                    vec![RibbonGroup {
                        title: "Group",
                        tools: vec![],
                    }]
                })
            }
        }
        let m: Box<dyn CadModule> = Box::new(Demo);
        assert_eq!(m.title(), "Demo");
        assert_eq!(m.ribbon_groups().len(), 1);
    }
}
