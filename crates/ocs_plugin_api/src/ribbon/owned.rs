//! Owned, serializable versions of the ribbon vocabulary for IPC.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, StyleKey, ToolDef};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedToolDef {
    pub id: String,
    pub label: String,
    pub icon: OwnedIconKind,
    pub event: ModuleEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OwnedIconKind {
    Glyph(String),
    Svg(Vec<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OwnedRibbonItem {
    Tool(OwnedToolDef),
    LargeTool(OwnedToolDef),
    Dropdown {
        id: String,
        icon: OwnedIconKind,
        items: Vec<(String, String, OwnedIconKind)>,
        default: String,
    },
    LargeDropdown {
        id: String,
        label: String,
        icon: OwnedIconKind,
        items: Vec<(String, String, OwnedIconKind)>,
        default: String,
    },
    LayerComboGroup {
        row2: Vec<OwnedToolDef>,
        row3: Vec<OwnedToolDef>,
    },
    PropertiesGroup {
        match_prop: OwnedToolDef,
    },
    StyleComboGroup {
        style_key: StyleKey,
        combo_id: String,
        manager_cmd: Option<String>,
        rows: Vec<Vec<OwnedToolDef>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedRibbonGroup {
    pub title: String,
    pub tools: Vec<OwnedRibbonItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedPluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub api_version: u32,
    pub ribbon_order: i32,
    pub xdata_apps: Vec<String>,
    pub command_prefixes: Vec<String>,
}

impl From<IconKind> for OwnedIconKind {
    fn from(i: IconKind) -> Self {
        match i {
            IconKind::Glyph(g) => OwnedIconKind::Glyph(g.to_string()),
            IconKind::Svg(b) => OwnedIconKind::Svg(b.to_vec()),
        }
    }
}

impl OwnedIconKind {
    /// Leak the owned data to reconstruct an `IconKind` with `&'static` lifetime.
    pub fn to_static(self) -> IconKind {
        match self {
            OwnedIconKind::Glyph(g) => IconKind::Glyph(&*Box::leak(g.into_boxed_str())),
            OwnedIconKind::Svg(b) => IconKind::Svg(&*Box::leak(b.into_boxed_slice())),
        }
    }
}

impl From<ToolDef> for OwnedToolDef {
    fn from(t: ToolDef) -> Self {
        Self {
            id: t.id.to_string(),
            label: t.label.to_string(),
            icon: t.icon.into(),
            event: t.event,
        }
    }
}

impl OwnedToolDef {
    pub fn to_static(self) -> ToolDef {
        ToolDef {
            id: &*Box::leak(self.id.into_boxed_str()),
            label: &*Box::leak(self.label.into_boxed_str()),
            icon: self.icon.to_static(),
            event: self.event,
        }
    }
}

impl From<RibbonItem> for OwnedRibbonItem {
    fn from(item: RibbonItem) -> Self {
        match item {
            RibbonItem::Tool(t) => OwnedRibbonItem::Tool(t.into()),
            RibbonItem::LargeTool(t) => OwnedRibbonItem::LargeTool(t.into()),
            RibbonItem::Dropdown {
                id,
                icon,
                items,
                default,
            } => OwnedRibbonItem::Dropdown {
                id: id.to_string(),
                icon: icon.into(),
                items: items
                    .into_iter()
                    .map(|(a, b, i)| (a.to_string(), b.to_string(), i.into()))
                    .collect(),
                default: default.to_string(),
            },
            RibbonItem::LargeDropdown {
                id,
                label,
                icon,
                items,
                default,
            } => OwnedRibbonItem::LargeDropdown {
                id: id.to_string(),
                label: label.to_string(),
                icon: icon.into(),
                items: items
                    .into_iter()
                    .map(|(a, b, i)| (a.to_string(), b.to_string(), i.into()))
                    .collect(),
                default: default.to_string(),
            },
            RibbonItem::LayerComboGroup { row2, row3 } => OwnedRibbonItem::LayerComboGroup {
                row2: row2.into_iter().map(Into::into).collect(),
                row3: row3.into_iter().map(Into::into).collect(),
            },
            RibbonItem::PropertiesGroup { match_prop } => OwnedRibbonItem::PropertiesGroup {
                match_prop: match_prop.into(),
            },
            RibbonItem::StyleComboGroup {
                style_key,
                combo_id,
                manager_cmd,
                rows,
            } => OwnedRibbonItem::StyleComboGroup {
                style_key,
                combo_id: combo_id.to_string(),
                manager_cmd: manager_cmd.map(|s| s.to_string()),
                rows: rows
                    .into_iter()
                    .map(|r| r.into_iter().map(Into::into).collect())
                    .collect(),
            },
        }
    }
}

impl OwnedRibbonItem {
    pub fn to_static(self) -> RibbonItem {
        match self {
            OwnedRibbonItem::Tool(t) => RibbonItem::Tool(t.to_static()),
            OwnedRibbonItem::LargeTool(t) => RibbonItem::LargeTool(t.to_static()),
            OwnedRibbonItem::Dropdown {
                id,
                icon,
                items,
                default,
            } => RibbonItem::Dropdown {
                id: &*Box::leak(id.into_boxed_str()),
                icon: icon.to_static(),
                items: items
                    .into_iter()
                    .map(|(a, b, i)| {
                        (
                            &*Box::leak(a.into_boxed_str()),
                            &*Box::leak(b.into_boxed_str()),
                            i.to_static(),
                        )
                    })
                    .collect(),
                default: &*Box::leak(default.into_boxed_str()),
            },
            OwnedRibbonItem::LargeDropdown {
                id,
                label,
                icon,
                items,
                default,
            } => RibbonItem::LargeDropdown {
                id: &*Box::leak(id.into_boxed_str()),
                label: &*Box::leak(label.into_boxed_str()),
                icon: icon.to_static(),
                items: items
                    .into_iter()
                    .map(|(a, b, i)| {
                        (
                            &*Box::leak(a.into_boxed_str()),
                            &*Box::leak(b.into_boxed_str()),
                            i.to_static(),
                        )
                    })
                    .collect(),
                default: &*Box::leak(default.into_boxed_str()),
            },
            OwnedRibbonItem::LayerComboGroup { row2, row3 } => RibbonItem::LayerComboGroup {
                row2: row2.into_iter().map(|t| t.to_static()).collect(),
                row3: row3.into_iter().map(|t| t.to_static()).collect(),
            },
            OwnedRibbonItem::PropertiesGroup { match_prop } => RibbonItem::PropertiesGroup {
                match_prop: match_prop.to_static(),
            },
            OwnedRibbonItem::StyleComboGroup {
                style_key,
                combo_id,
                manager_cmd,
                rows,
            } => RibbonItem::StyleComboGroup {
                style_key,
                combo_id: &*Box::leak(combo_id.into_boxed_str()),
                manager_cmd: manager_cmd.map(|s| &*Box::leak(s.into_boxed_str())),
                rows: rows
                    .into_iter()
                    .map(|r| r.into_iter().map(|t| t.to_static()).collect())
                    .collect(),
            },
        }
    }
}

impl From<RibbonGroup> for OwnedRibbonGroup {
    fn from(g: RibbonGroup) -> Self {
        Self {
            title: g.title.to_string(),
            tools: g.tools.into_iter().map(Into::into).collect(),
        }
    }
}

impl OwnedRibbonGroup {
    pub fn to_static(self) -> RibbonGroup {
        RibbonGroup {
            title: &*Box::leak(self.title.into_boxed_str()),
            tools: self.tools.into_iter().map(|t| t.to_static()).collect(),
        }
    }
}

/// Convert owned ribbon groups into a `CadModule` by leaking the strings once.
pub fn to_module(id: String, title: String, groups: Vec<OwnedRibbonGroup>) -> Box<dyn CadModule> {
    struct M {
        id: &'static str,
        title: &'static str,
        groups: Vec<RibbonGroup>,
    }
    impl CadModule for M {
        fn id(&self) -> &'static str {
            self.id
        }
        fn title(&self) -> &'static str {
            self.title
        }
        fn ribbon_groups(&self) -> Vec<RibbonGroup> {
            self.groups.clone()
        }
    }
    let id = &*Box::leak(id.into_boxed_str());
    let title = &*Box::leak(title.into_boxed_str());
    Box::new(M {
        id,
        title,
        groups: groups
            .into_iter()
            .map(OwnedRibbonGroup::to_static)
            .collect(),
    })
}

/// A cheaply-cloneable `CadModule` wrapper for plugin ribbon data.
#[derive(Clone)]
pub struct SharedCadModule(Arc<dyn CadModule>);

impl CadModule for SharedCadModule {
    fn id(&self) -> &'static str {
        self.0.id()
    }
    fn title(&self) -> &'static str {
        self.0.title()
    }
    fn ribbon_groups(&self) -> Vec<RibbonGroup> {
        self.0.ribbon_groups()
    }
}

/// Convert owned ribbon groups into a shareable `CadModule`.
pub fn to_shared_module(
    id: String,
    title: String,
    groups: Vec<OwnedRibbonGroup>,
) -> SharedCadModule {
    let module = to_module(id, title, groups);
    SharedCadModule(Arc::from(module))
}

#[cfg(all(test, feature = "host"))]
mod tests {
    use super::*;

    #[test]
    fn owned_ribbon_group_round_trips_through_static() {
        let owned = OwnedRibbonGroup {
            title: "Geometry".to_string(),
            tools: vec![OwnedRibbonItem::Tool(OwnedToolDef {
                id: "line".to_string(),
                label: "Line".to_string(),
                icon: OwnedIconKind::Glyph("L".to_string()),
                event: ModuleEvent::Command("LINE".to_string()),
            })],
        };
        let static_group = owned.clone().to_static();
        assert_eq!(static_group.title, "Geometry");
        assert_eq!(static_group.tools.len(), 1);
        let back: OwnedRibbonGroup = static_group.into();
        assert_eq!(back.title, owned.title);
    }

    #[test]
    fn shared_module_clones_without_re_leaking() {
        let owned = vec![OwnedRibbonGroup {
            title: "Draw".to_string(),
            tools: vec![OwnedRibbonItem::Tool(OwnedToolDef {
                id: "circle".to_string(),
                label: "Circle".to_string(),
                icon: OwnedIconKind::Glyph("C".to_string()),
                event: ModuleEvent::Command("CIRCLE".to_string()),
            })],
        }];
        let shared = to_shared_module("opencad.demo".to_string(), "Demo".to_string(), owned);
        let cloned = shared.clone();
        assert_eq!(shared.title(), "Demo");
        assert_eq!(shared.id(), "opencad.demo");
        assert_eq!(cloned.title(), shared.title());
        assert_eq!(shared.ribbon_groups().len(), cloned.ribbon_groups().len());
    }
}
