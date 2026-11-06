//! HYPERLINK (the Insert / Edit Hyperlink window), -HYPERLINK,
//! HYPERLINKOPTIONS and HYPERLINKBASE.

use crate::app::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::insert::hyperlink_cmd::{
    DashHyperlinkCommand, HyperlinkOptionsCommand, HyperlinkRemovePick, HyperlinkSelectCommand,
};
use crate::scene::hyperlink::{self, Link};
use crate::ui::window::hyperlink_dialog::{HyperlinkMsg, HyperlinkOptions};
use document::Handle;
use iced::Task;

/// The layer -HYPERLINK Area draws its rectangle on (red).
const AREA_LAYER: &str = "URLLAYER";

fn parse_handles(text: &str) -> Vec<Handle> {
    text.split(',')
        .filter_map(|s| u64::from_str_radix(s.trim(), 16).ok().map(Handle::new))
        .collect()
}

/// HYPERLINKOPTIONS: show the hyperlink cursor, tooltip and shortcut menu.
static OPTIONS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub(crate) fn hyperlink_options() -> bool {
    OPTIONS.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn set_hyperlink_options(on: bool) {
    OPTIONS.store(on, std::sync::atomic::Ordering::Relaxed);
}

impl OpenCADStudio {
    pub(super) fn dispatch_hyperlink(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        let rest = rest.trim();
        match verb.trim_start_matches('.').to_ascii_uppercase().as_str() {
            "HYPERLINK" if rest.is_empty() => {
                let preselected = self.tabs[i].scene.selected_handles_in_order();
                if !preselected.is_empty() {
                    self.open_hyperlink_dialog(i, preselected);
                    return Some(Task::none());
                }
                Some(self.start_hyperlink_command(i, Box::new(HyperlinkSelectCommand)))
            }
            // HYPERLINK <url>: the address of the selected objects' links;
            // their text and named location stay.
            "HYPERLINK" => {
                let handles: Vec<Handle> = self.tabs[i]
                    .scene
                    .selected_handles_in_order()
                    .into_iter()
                    .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                    .collect();
                if handles.is_empty() {
                    self.command_line
                        .push_error(crate::t!("HYPERLINK: select objects first.").as_ref());
                    return Some(Task::none());
                }
                let url = rest.to_string();
                self.apply_property_op(i, "HYPERLINK", &handles, |app, handle| {
                    hyperlink::set_url(&mut app.tabs[i].scene.document, handle, &url);
                });
                let n = handles.len();
                self.command_line
                    .push_output(crate::tf!("HYPERLINK: attached to {n} object(s).").as_ref());
                Some(Task::none())
            }
            "-HYPERLINK" => Some(self.start_hyperlink_command(i, Box::new(DashHyperlinkCommand::new()))),
            "_HYPERLINKDIALOG" => {
                let handles = parse_handles(rest);
                self.tabs[i].scene.deselect_all();
                self.open_hyperlink_dialog(i, handles);
                Some(Task::none())
            }
            "_HYPERLINKAPPLY" => {
                self.apply_dash_hyperlink(i, rest);
                Some(Task::none())
            }
            "_HYPERLINKREMOVELIST" => {
                let entries: Vec<(Handle, Link)> = parse_handles(rest)
                    .into_iter()
                    .filter_map(|handle| {
                        let link = self.tabs[i].scene.document.get_entity(handle).and_then(Link::of)?;
                        Some((handle, link))
                    })
                    .collect();
                self.tabs[i].scene.deselect_all();
                // Objects without hyperlinks end the command without a word.
                if entries.is_empty() {
                    return Some(Task::none());
                }
                for (n, (_, link)) in entries.iter().enumerate() {
                    self.command_line
                        .push_output(&format!("{}.  {} ({})", n + 1, link.url, link.text));
                }
                let entries = entries.into_iter().map(|(h, link)| (h, link.url)).collect();
                Some(self.start_hyperlink_command(i, Box::new(HyperlinkRemovePick::new(entries))))
            }
            "_HYPERLINKREMOVE" => {
                let handles = parse_handles(rest);
                self.apply_property_op(i, "-HYPERLINK", &handles, |app, handle| {
                    hyperlink::set(&mut app.tabs[i].scene.document, handle, None);
                });
                let message = if handles.len() == 1 {
                    crate::tr!("hyperlink-cmd", "deleted-one")
                } else {
                    format!("{} {}", handles.len(), crate::tr!("hyperlink-cmd", "deleted-many"))
                };
                self.command_line.push_output(&message);
                Some(Task::none())
            }
            "HYPERLINKOPTIONS" if rest.is_empty() => Some(self.start_hyperlink_command(
                i,
                Box::new(HyperlinkOptionsCommand::new(hyperlink_options())),
            )),
            "HYPERLINKOPTIONS" | "_HYPERLINKOPTIONSSET" => {
                let mut command = HyperlinkOptionsCommand::new(hyperlink_options());
                let value = if verb.eq_ignore_ascii_case("_HYPERLINKOPTIONSSET") {
                    Some(rest == "1")
                } else {
                    match command.on_text_input(rest) {
                        Some(crate::command::CmdResult::Dispatch(set)) => Some(set.ends_with('1')),
                        _ => None,
                    }
                };
                match value {
                    Some(on) => {
                        if on != hyperlink_options() {
                            set_hyperlink_options(on);
                            self.save_config();
                        }
                    }
                    None => {
                        self.command_line
                            .push_rejection(&crate::tr!("hyperlink-cmd", "invalid-keyword"));
                    }
                }
                Some(Task::none())
            }
            "HYPERLINKBASE" => {
                let current = self.tabs[i].scene.document.header.hyperlink_base.clone();
                if rest.is_empty() {
                    self.command_line.push_output(&format!(
                        "{} <\"{current}\">:",
                        crate::tr!("hyperlink-cmd", "base-prompt")
                    ));
                    self.pending_setvar = Some("HYPERLINKBASE".into());
                    return Some(Task::none());
                }
                let value = if rest == "." { String::new() } else { rest.to_string() };
                if value != current {
                    self.push_undo_snapshot(i, "HYPERLINKBASE");
                    let document = &mut self.tabs[i].scene.document;
                    document.header.hyperlink_base = value.clone();
                    document.summary_info.hyperlink_base = value;
                    self.tabs[i].dirty = true;
                }
                Some(Task::none())
            }
            "SETVAR" if rest.to_ascii_uppercase().starts_with("HYPERLINKBASE") => {
                let value = rest["HYPERLINKBASE".len()..].trim();
                if value.is_empty() {
                    return Some(Task::none());
                }
                self.dispatch_hyperlink(&format!("HYPERLINKBASE {value}"), i)
            }
            _ => None,
        }
    }

    fn start_hyperlink_command(&mut self, i: usize, command: Box<dyn CadCommand>) -> Task<Message> {
        self.start_count_command(i, command)
    }

    /// The folder a relative address resolves against: HYPERLINKBASE, else
    /// the drawing's own folder (none while it is unsaved).
    fn hyperlink_base(&self, i: usize) -> String {
        let base = self.tabs[i].scene.document.header.hyperlink_base.trim();
        if !base.is_empty() {
            return base.to_string();
        }
        self.tabs[i]
            .current_path
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(|dir| dir.display().to_string())
            .unwrap_or_default()
    }

    /// Model, the layouts and the named views: what a link to this drawing
    /// can open.
    fn hyperlink_views(&self, i: usize) -> Vec<String> {
        let scene = &self.tabs[i].scene;
        let mut views = scene.layout_names();
        views.extend(scene.document.views.iter().map(|view| view.name.clone()));
        views
    }

    /// Opens the Insert / Edit Hyperlink window on `handles`: Edit when one of
    /// them has a link, which the window then shows.
    pub(in crate::app) fn open_hyperlink_dialog(&mut self, i: usize, handles: Vec<Handle>) {
        if handles.is_empty() {
            return;
        }
        let document = &self.tabs[i].scene.document;
        let links: Vec<Option<Link>> = handles
            .iter()
            .map(|handle| document.get_entity(*handle).and_then(Link::of))
            .collect();
        let with_links = links.iter().filter(|link| link.is_some()).count();
        let mixed = links.windows(2).any(|pair| pair[0] != pair[1]);
        let link = if mixed {
            Link::default()
        } else {
            links.into_iter().flatten().next().unwrap_or_default()
        };
        let view_target = link.url.is_empty() && !link.location.is_empty();
        let base = self.hyperlink_base(i);
        self.hyperlink_editor_block = None;
        self.hyperlink_editor_handles = handles;
        self.hyperlink_editor_url = if view_target { String::new() } else { link.address() };
        self.hyperlink_editor_description = link.text.clone();
        self.hyperlink_editor_mixed = mixed;
        self.hyperlink_editor_dirty = false;
        self.hyperlink_options = HyperlinkOptions {
            view_target,
            view: if view_target { link.location.clone() } else { String::new() },
            views: self.hyperlink_views(i),
            // A new link is stored relative where it can be; an existing one
            // keeps the form it has.
            relative: with_links == 0,
            relative_enabled: !base.is_empty(),
            base,
            // A new link converts DWG links to DWF; an existing one shows its flag.
            dwf: if with_links == 0 {
                true
            } else {
                link.flags & hyperlink::CONVERT_TO_DWF != 0
            },
            edit: with_links > 0,
            selected: self.hyperlink_editor_handles.len(),
            with_links,
            address_only: false,
        };
        self.active_modal = Some(crate::app::ModalKind::Hyperlink);
    }

    /// The link the window describes, for the target objects.
    pub(in crate::app) fn hyperlink_from_dialog(&self) -> Link {
        let options = &self.hyperlink_options;
        let text = self.hyperlink_editor_description.trim().to_string();
        if options.view_target {
            return Link {
                url: String::new(),
                text,
                location: options.view.clone(),
                flags: 0,
            };
        }
        let (mut url, location) = Link::split_address(&self.hyperlink_editor_url);
        if options.relative && options.relative_enabled {
            url = relative_address(&url, &options.base);
        }
        Link {
            url,
            text,
            location,
            flags: if options.dwf {
                hyperlink::CONVERT_TO_DWF
            } else {
                0
            },
        }
    }

    pub(in crate::app) fn update_hyperlink_dialog(&mut self, message: HyperlinkMsg) -> Task<Message> {
        let options = &mut self.hyperlink_options;
        match message {
            HyperlinkMsg::ViewTarget(on) => {
                options.view_target = on;
                if on && options.view.is_empty() {
                    options.view = options.views.first().cloned().unwrap_or_default();
                }
            }
            HyperlinkMsg::View(name) => options.view = name,
            HyperlinkMsg::Relative(on) => options.relative = on,
            HyperlinkMsg::Dwf(on) => options.dwf = on,
            HyperlinkMsg::Help => {
                self.command_line
                    .push_info(&crate::tr!("hyperlink-dialog", "help"));
                return Task::none();
            }
            HyperlinkMsg::Browse => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title(crate::t!("Select a file to link").as_ref())
                            .pick_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h))
                    },
                    |path| match path {
                        Some(path) => {
                            Message::HyperlinkUrlChanged(path.to_string_lossy().into_owned())
                        }
                        None => Message::Noop,
                    },
                );
            }
        }
        self.hyperlink_editor_dirty = true;
        Task::none()
    }

    /// -HYPERLINK Insert: the link on the objects, or on a new rectangle on
    /// URLLAYER for Area. An empty address and location removes the links.
    fn apply_dash_hyperlink(&mut self, i: usize, request: &str) {
        let Ok(request) = serde_json::from_str::<serde_json::Value>(request) else {
            return;
        };
        let field = |key: &str| request[key].as_str().unwrap_or_default().to_string();
        let link = Link {
            url: field("url"),
            text: field("text"),
            location: field("location"),
            flags: 0,
        };
        let mut handles = parse_handles(&field("handles"));
        let corners: Vec<[f64; 3]> =
            serde_json::from_value(request["area"].clone()).unwrap_or_default();
        self.push_undo_snapshot(i, "-HYPERLINK");
        if let [a, b] = corners[..] {
            let scene = &mut self.tabs[i].scene;
            // Created red the first time; an existing one keeps its settings.
            if !scene.document.layers.contains(AREA_LAYER) {
                scene.ensure_layer(AREA_LAYER);
                if let Some(layer) = scene.document.layers.get_mut(AREA_LAYER) {
                    layer.color = document::types::Color::from_index(1);
                }
            }
            let z = a[2];
            let mut rectangle = document::entities::Polyline3D::from_points(vec![
                document::types::Vector3::new(a[0], a[1], z),
                document::types::Vector3::new(b[0], a[1], z),
                document::types::Vector3::new(b[0], b[1], z),
                document::types::Vector3::new(a[0], b[1], z),
            ]);
            rectangle.close();
            rectangle.common.layer = AREA_LAYER.to_string();
            if let Some(handle) = self.commit_entity_handle_preserve_layer(document::EntityType::Polyline3D(rectangle)) {
                handles = vec![handle];
            }
        }
        for handle in &handles {
            let document = &mut self.tabs[i].scene.document;
            if link.is_empty() {
                hyperlink::set(document, *handle, None);
            } else {
                hyperlink::set(document, *handle, Some(&link));
            }
        }
        self.tabs[i].scene.deselect_all();
        self.invalidate_property_targets(i, &handles);
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }
}

/// A file address relative to `base` where it can be (same drive), else as
/// it is. Web addresses stay.
fn relative_address(url: &str, base: &str) -> String {
    if base.is_empty() || url.contains("://") || !std::path::Path::new(url).is_absolute() {
        return url.to_string();
    }
    crate::io::xref_model::to_pathtype_result(
        url,
        &std::path::Path::new(base).join("_"),
        crate::io::xref_model::Pathtype::Relative,
    )
    .unwrap_or_else(|_| url.to_string())
}
