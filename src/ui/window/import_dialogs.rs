//! Import File (IMPORT: the format first, then the file, routed to the
//! format's importer), DGN Import (DGNIMPORT: model, target drawing,
//! conversion units and the mapping setup's effect on the file) and 3D Studio
//! Import (the objects of a .3ds file to bring in, their layers and what a
//! mesh with several materials becomes).

use iced::widget::{button, column, container, image, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::{card, well_style, PageThumb};

/// One edit in the Import File or DGN Import window.
#[derive(Debug, Clone)]
pub enum ImportDialogMsg {
    // Import File
    Path(String),
    Browse,
    /// A recent file chosen from its chip.
    Recent(usize),
    Format(usize),
    Search(String),
    OnlySupported(bool),
    Picked(Option<String>),
    ImportOk,
    // DGN Import
    DgnChange,
    DgnPicked(Option<String>),
    DgnModel(usize),
    DgnCurrent(bool),
    DgnPrefix(bool),
    DgnSub(bool),
    DgnExplode(bool),
    DgnReferences(crate::modules::insert::dgn_import::ReferenceMode),
    DgnExisting(crate::modules::insert::dgn_import::ExistingReference),
    DgnMappingTab(usize),
    DgnOk,
    // 3D Studio Import
    TdsChange,
    TdsPicked(Option<String>),
    TdsCheck(usize, bool),
    TdsAll,
    TdsClear,
    TdsFilter(usize),
    TdsSearch(String),
    TdsLayers(usize),
    TdsMulti(usize),
    TdsOk,
    // A mesh with several materials, asked during a 3D Studio import
    /// 0 split by material, 1 first material, 2 the material chosen below.
    TdsMatChoice(usize),
    TdsMatList(bool),
    /// 0 no material, k the mesh's k-th material.
    TdsMatPick(usize),
    TdsMatRemember(bool),
    TdsMatApply,
    Help(&'static str),
}

/// The colour of a format that is read in part or only written.
const AMBER: iced::Color = iced::Color::from_rgb8(0xd9, 0xa4, 0x00);

fn msg(m: ImportDialogMsg) -> Message {
    Message::ImportDialog(m)
}

/// Whether OCS imports a format, and why not when it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Yes,
    /// Any file: the importer follows the file's extension.
    ByExtension,
    No,
}

pub struct ImportFormat {
    pub name: &'static str,
    pub patterns: &'static str,
    pub extensions: &'static [&'static str],
    /// 0 2D / underlay, 1 3D model, 2 other.
    pub group: u8,
    pub support: Support,
}

const fn format(
    name: &'static str,
    patterns: &'static str,
    extensions: &'static [&'static str],
    group: u8,
    support: Support,
) -> ImportFormat {
    ImportFormat {
        name,
        patterns,
        extensions,
        group,
        support,
    }
}

/// The reference's Import File types, in its order of groups.
pub const FORMATS: [ImportFormat; 22] = [
    format("PDF Files", "*.pdf", &["pdf"], 0, Support::Yes),
    format("MicroStation DGN", "*.dgn", &["dgn"], 0, Support::Yes),
    format("All DGN Files", "*.*", &["*"], 0, Support::Yes),
    format("Metafile", "*.wmf", &["wmf"], 0, Support::Yes),
    format("SVG", "*.svg; *.svgz", &["svg", "svgz"], 0, Support::Yes),
    format("3D Studio", "*.3ds", &["3ds"], 1, Support::Yes),
    format("ACIS", "*.sat", &["sat"], 1, Support::Yes),
    format(
        "CATIA V4",
        "*.model; *.session; *.exp; *.dlv3",
        &["model", "session", "exp", "dlv3"],
        1,
        Support::No,
    ),
    format(
        "CATIA V5",
        "*.CATPart; *.CATProduct",
        &["CATPart", "CATProduct"],
        1,
        Support::No,
    ),
    format("IGES", "*.igs; *.iges", &["igs", "iges"], 1, Support::Yes),
    format("Inventor", "*.ipt; *.iam", &["ipt", "iam"], 1, Support::No),
    format("JT", "*.jt", &["jt"], 1, Support::No),
    format("NX", "*.prt", &["prt"], 1, Support::No),
    format("Parasolid Binary", "*.x_b", &["x_b"], 1, Support::No),
    format("Parasolid Text", "*.x_t", &["x_t"], 1, Support::No),
    format("Pro/ENGINEER", "*.prt*; *.asm*", &["prt", "asm"], 1, Support::No),
    format("Pro/ENGINEER Granite", "*.g", &["g"], 1, Support::No),
    format("Pro/ENGINEER Neutral", "*.neu*", &["neu"], 1, Support::No),
    format("Rhino", "*.3dm", &["3dm"], 1, Support::No),
    format(
        "SolidWorks",
        "*.prt; *.sldprt; *.asm; *.sldasm",
        &["prt", "sldprt", "asm", "sldasm"],
        1,
        Support::No,
    ),
    format("STEP", "*.ste; *.stp; *.step", &["ste", "stp", "step"], 1, Support::Yes),
    format("All Files", "*.*", &["*"], 2, Support::ByExtension),
];

pub struct ImportFileState {
    pub path: String,
    pub format: usize,
    pub search: String,
    pub only_supported: bool,
    pub error: Option<String>,
}

pub struct DgnImportState {
    pub path: String,
    pub version: &'static str,
    pub models: Vec<PageThumb>,
    pub model: usize,
    pub current_drawing: bool,
    pub prefix_duplicates: bool,
    pub sub_units: bool,
    pub explode_text_nodes: bool,
    pub references: crate::modules::insert::dgn_import::ReferenceMode,
    pub existing_reference: crate::modules::insert::dgn_import::ExistingReference,
    /// Master and sub unit names.
    pub units: (String, String),
    pub mapping_tab: usize,
    /// The chosen model's mapping rows per tab (layer, linetype,
    /// lineweight, colour).
    pub mapping: [Vec<(String, String)>; 4],
}

/// The objects of a 3D Studio file and the import's choices.
pub struct TdsImportState {
    pub path: String,
    /// "Global Settings" first, then the file's objects by name: each name,
    /// its kind (Mesh, Light, Camera or Global) and the materials it uses.
    pub objects: Vec<(String, &'static str, usize)>,
    pub materials: usize,
    pub checked: Vec<bool>,
    /// 0 all, 1 meshes, 2 lights, 3 cameras, 4 global.
    pub filter: usize,
    pub search: String,
    /// 0 by object, 1 by material, 2 by object colour, 3 single layer.
    pub layers: usize,
    /// 0 always ask, 1 split by material, 2 first material, 3 no material.
    pub multi: usize,
}

/// A 3D Studio import set to ask: the meshes with several materials still to
/// ask about, in the order the import processes them, and the answer being
/// given for the first one.
pub struct TdsMaterialState {
    /// The tab the import was started in.
    pub tab: usize,
    pub request: crate::modules::insert::tds_import::TdsImportRequest,
    /// Each mesh and its materials (with their diffuse colour), last
    /// assigned first.
    pub queue: Vec<(String, Vec<(String, Option<[u8; 3]>)>)>,
    /// 0 split by material, 1 first material, 2 a chosen one; the choice
    /// made for one mesh is offered for the next.
    pub choice: usize,
    /// 0 no material, k the k-th of the mesh's materials.
    pub material: usize,
    pub list_open: bool,
    /// The choice answers the remaining meshes too.
    pub remember: bool,
}

/// The windows' working copies and the files imported this session.
#[derive(Default)]
pub struct ImportDialogs {
    pub file: Option<ImportFileState>,
    pub dgn: Option<DgnImportState>,
    pub tds: Option<TdsImportState>,
    pub tds_material: Option<TdsMaterialState>,
    /// The texture maps the last 3D Studio import did not find, as (map
    /// file, material); the window shows the first, one at a time.
    pub tds_maps: Vec<(String, String)>,
    pub recent: Vec<String>,
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// A long path as its drive, an ellipsis and the last three parts.
fn short_path(path: &str) -> String {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if parts.len() <= 4 {
        return path.to_string();
    }
    format!("{}\\…\\{}", parts[0], parts[parts.len() - 3..].join("\\"))
}

fn badge<'a>(support: Support) -> Element<'a, Message> {
    let (label, tone) = match support {
        Support::Yes => (t!("Supported"), 0),
        Support::ByExtension => (t!("By extension"), 2),
        Support::No => (t!("None"), 2),
    };
    let color = move |theme: &Theme| match tone {
        0 => theme.palette().success.base.color,
        _ => theme.palette().background.base.text.scale_alpha(0.55),
    };
    container(
        text(label)
            .size(10)
            .style(move |theme: &Theme| text::Style {
                color: Some(color(theme)),
            }),
    )
    .padding([1, 7])
    .style(move |theme: &Theme| container::Style {
        border: Border {
            width: 1.0,
            radius: 9.0.into(),
            color: color(theme),
        },
        ..Default::default()
    })
    .into()
}

/// The reason a format cannot be imported.
pub fn unsupported_reason(support: Support) -> Option<std::borrow::Cow<'static, str>> {
    match support {
        Support::Yes | Support::ByExtension => None,
        Support::No => Some(t!("OpenCAD Studio has no importer for this format.")),
    }
}

fn tile_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status| {
        let palette = theme.palette();
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(if selected || hovered {
                palette.background.base.color
            } else {
                palette.background.weak.color
            })),
            text_color: palette.background.base.text,
            border: Border {
                width: if selected { 1.5 } else { 0.0 },
                radius: 6.0.into(),
                color: palette.primary.base.color,
            },
            ..Default::default()
        }
    }
}

/// Help, Cancel and the named action (inactive while `ok` is `None`).
fn footer<'a>(help: &'static str, action: std::borrow::Cow<'static, str>, ok: Option<Message>) -> Element<'a, Message> {
    row![
        button(text("?").size(12))
            .on_press(msg(ImportDialogMsg::Help(help)))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        dialog_button_styled_opt(action, ok, button::primary),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn chip<'a>(label: String, on: bool, message: Message) -> Element<'a, Message> {
    button(text(label).size(11))
        .on_press(message)
        .style(button_style(on))
        .padding([4, 10])
        .into()
}

/// One choice out of a few, as joined buttons; inactive ones are greyed.
fn segmented<'a>(options: Vec<(String, bool, Option<Message>)>) -> Element<'a, Message> {
    let buttons = options.into_iter().map(|(label, on, message)| {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press_maybe(message)
            .style(button_style(on))
            .padding([5, 6])
            .width(Fill)
            .into()
    });
    container(iced::widget::Row::with_children(buttons.collect::<Vec<_>>()).spacing(2))
        .padding(2)
        .width(Fill)
        .style(well_style)
        .into()
}

pub fn view_import_file<'a>(
    state: &'a ImportFileState,
    recent: &'a [String],
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let mut chips = row![text(t!("Recent:")).size(11).style(muted_style)]
        .spacing(6)
        .align_y(iced::Center);
    for (n, path) in recent.iter().enumerate() {
        chips = chips.push(chip(
            file_name(path),
            state.path == *path,
            msg(ImportDialogMsg::Recent(n)),
        ));
    }
    let mut file = column![row![
        text_input("", &state.path)
            .on_input(|s| msg(ImportDialogMsg::Path(s)))
            .on_submit(msg(ImportDialogMsg::ImportOk))
            .size(12)
            .padding([6, 8])
            .width(Fill)
            .style(field_style),
        button(text(t!("Browse...")).size(11))
            .on_press(msg(ImportDialogMsg::Browse))
            .style(button_style(false))
            .padding([6, 14]),
    ]
    .spacing(8)
    .align_y(iced::Center)]
    .spacing(8);
    if !recent.is_empty() {
        file = file.push(chips);
    }
    if let Some(error) = &state.error {
        file = file.push(text(error.clone()).size(11).style(|theme: &Theme| text::Style {
            color: Some(theme.palette().danger.base.color),
        }));
    }

    let search = state.search.trim().to_lowercase();
    let shown: Vec<usize> = (0..FORMATS.len())
        .filter(|&k| {
            let f = &FORMATS[k];
            (!state.only_supported || matches!(f.support, Support::Yes | Support::ByExtension))
                && (search.is_empty()
                    || f.name.to_lowercase().contains(&search)
                    || f.patterns.to_lowercase().contains(&search))
        })
        .collect();
    let mut groups = column![].spacing(6);
    for (group, title) in [
        (0u8, t!("2D / Underlay")),
        (1, t!("3D Model")),
        (2, t!("Other")),
    ] {
        let members: Vec<usize> = shown
            .iter()
            .copied()
            .filter(|&k| FORMATS[k].group == group)
            .collect();
        if members.is_empty() {
            continue;
        }
        groups = groups.push(text(title.to_uppercase()).size(10).style(muted_style));
        for line in members.chunks(3) {
            let mut r = row![].spacing(6);
            for &k in line {
                let f = &FORMATS[k];
                r = r.push(
                    button(
                        row![
                            column![
                                text(f.name).size(12),
                                text(f.patterns).size(10).style(muted_style),
                            ]
                            .spacing(1)
                            .width(Fill),
                            badge(f.support),
                        ]
                        .spacing(6)
                        .align_y(iced::Center),
                    )
                    .on_press(msg(ImportDialogMsg::Format(k)))
                    .style(tile_style(state.format == k))
                    .padding([5, 8])
                    .width(Length::FillPortion(1)),
                );
            }
            for _ in line.len()..3 {
                r = r.push(Space::new().width(Length::FillPortion(1)));
            }
            groups = groups.push(r);
        }
    }
    let selected = &FORMATS[state.format];
    let reason = unsupported_reason(selected.support);
    let formats = card(
        format!("{} · {}", t!("Format"), FORMATS.len()),
        column![
            row![
                text_input(&t!("Search formats..."), &state.search)
                    .on_input(|s| msg(ImportDialogMsg::Search(s)))
                    .size(11)
                    .padding([5, 8])
                    .width(Length::Fixed(220.0))
                    .style(field_style),
                Space::new().width(Fill),
                chip(
                    t!("All").into_owned(),
                    !state.only_supported,
                    msg(ImportDialogMsg::OnlySupported(false))
                ),
                chip(
                    t!("Only what OCS can open").into_owned(),
                    state.only_supported,
                    msg(ImportDialogMsg::OnlySupported(true))
                ),
            ]
            .spacing(6)
            .align_y(iced::Center),
            scrollable(groups).height(Length::Fixed(360.0)),
            text(match &reason {
                Some(r) => format!("{}: {r}", selected.name),
                None => String::new(),
            })
            .size(11)
            .style(|_: &Theme| text::Style {
                color: Some(AMBER),
            }),
        ]
        .spacing(8),
    );
    let ok = reason.is_none().then(|| msg(ImportDialogMsg::ImportOk));
    column![
        card(t!("File").into_owned(), file),
        formats,
        footer("import", t!("Import"), ok),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

/// One choice out of a few, as buttons stacked one above the other.
fn stacked<'a>(options: Vec<(String, bool, Message)>) -> Element<'a, Message> {
    let buttons = options.into_iter().map(|(label, on, message)| {
        button(text(label).size(11).width(Fill))
            .on_press(message)
            .style(button_style(on))
            .padding([5, 10])
            .width(Fill)
            .into()
    });
    container(iced::widget::Column::with_children(buttons.collect::<Vec<_>>()).spacing(2))
        .padding(2)
        .width(Fill)
        .style(well_style)
        .into()
}

/// The kind of a 3D Studio object in the window's words.
fn tds_kind(kind: &str) -> String {
    match kind {
        "Mesh" => crate::tr!("import-3ds", "mesh"),
        "Light" => crate::tr!("import-3ds", "light"),
        "Camera" => crate::tr!("import-3ds", "camera"),
        _ => crate::tr!("import-3ds", "general"),
    }
}

pub fn view_tds_import<'a>(
    state: &'a TdsImportState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let top = container(
        row![
            container(text(file_name(&state.path)).size(12))
                .padding([4, 10])
                .style(|theme: &Theme| container::Style {
                    border: Border {
                        width: 1.0,
                        radius: 12.0.into(),
                        color: theme.palette().primary.base.color,
                    },
                    ..Default::default()
                }),
            text(crate::tr!("import-3ds", "summary", objects = (state.objects.len().saturating_sub(1) as i64), materials = (state.materials as i64)))
            .size(11)
            .style(muted_style)
            .width(Fill),
            button(text(crate::tr!("import-3ds", "change")).size(11))
                .on_press(msg(ImportDialogMsg::TdsChange))
                .style(button::text)
                .padding([4, 6]),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .padding([8, 12])
    .width(Fill)
    .style(crate::ui::window::pdf_dialogs::card_style);

    let kinds = ["", "Mesh", "Light", "Camera", "Global"];
    let mut filters = row![].spacing(4).align_y(iced::Center);
    for (k, label) in [crate::tr!("import-3ds", "all"), crate::tr!("import-3ds", "mesh"), crate::tr!("import-3ds", "light"), crate::tr!("import-3ds", "camera"), crate::tr!("import-3ds", "general")]
        .into_iter()
        .enumerate()
    {
        filters = filters.push(chip(label, state.filter == k, msg(ImportDialogMsg::TdsFilter(k))));
    }
    let search = state.search.trim().to_lowercase();
    let mut rows = column![row![
        Space::new().width(Length::Fixed(22.0)),
        text(crate::tr!("import-3ds", "name")).size(11).style(muted_style).width(Fill),
        text(crate::tr!("import-3ds", "type")).size(11).style(muted_style).width(Length::Fixed(150.0)),
    ]]
    .spacing(2);
    for (k, (name, kind, materials)) in state.objects.iter().enumerate() {
        let global = *kind == "Global";
        if state.filter != 0 && kinds[state.filter] != *kind {
            continue;
        }
        if !search.is_empty() && !name.to_lowercase().contains(&search) {
            continue;
        }
        let shown_name = if global { crate::tr!("import-3ds", "global") } else { name.clone() };
        let shown_kind = if global {
            String::new()
        } else if *kind == "Mesh" {
            format!("{} · {}", tds_kind(kind), crate::tr!("import-3ds", "materials", n = (*materials as i64)))
        } else {
            tds_kind(kind)
        };
        let on = state.checked.get(k).copied().unwrap_or(false);
        rows = rows.push(
            row![
                iced::widget::checkbox(on)
                    .on_toggle(move |v| msg(ImportDialogMsg::TdsCheck(k, v)))
                    .size(14),
                text(shown_name).size(12).width(Fill),
                text(shown_kind).size(11).style(muted_style).width(Length::Fixed(150.0)),
            ]
            .spacing(8)
            .align_y(iced::Center),
        );
    }
    let objects = card(
        crate::tr!("import-3ds", "objects"),
        column![
            row![
                filters,
                Space::new().width(Fill),
                text_input(crate::tr!("import-3ds", "search").as_ref(), &state.search)
                    .on_input(|s| msg(ImportDialogMsg::TdsSearch(s)))
                    .size(11)
                    .padding([4, 8])
                    .width(Length::Fixed(170.0))
                    .style(field_style),
            ]
            .spacing(8)
            .align_y(iced::Center),
            container(scrollable(rows).height(Length::Fixed(300.0)))
                .padding(6)
                .width(Fill)
                .style(well_style),
            row![
                button(text(crate::tr!("import-3ds", "select-all")).size(11))
                    .on_press(msg(ImportDialogMsg::TdsAll))
                    .style(button_style(false))
                    .padding([4, 10]),
                button(text(crate::tr!("import-3ds", "clear")).size(11))
                    .on_press(msg(ImportDialogMsg::TdsClear))
                    .style(button_style(false))
                    .padding([4, 10]),
            ]
            .spacing(6),
        ]
        .spacing(8),
    );
    let layer_notes = [
        crate::tr!("import-3ds", "note-object"),
        crate::tr!("import-3ds", "note-material"),
        crate::tr!("import-3ds", "note-color"),
        crate::tr!("import-3ds", "note-single"),
    ];
    let layers = card(
        crate::tr!("import-3ds", "layers"),
        column![
            stacked(
                [crate::tr!("import-3ds", "by-object"), crate::tr!("import-3ds", "by-material"), crate::tr!("import-3ds", "by-color"), crate::tr!("import-3ds", "single")]
                    .into_iter()
                    .enumerate()
                    .map(|(k, label)| (label, state.layers == k, msg(ImportDialogMsg::TdsLayers(k))))
                    .collect()
            ),
            text(layer_notes[state.layers.min(3)].clone()).size(11).style(muted_style),
        ]
        .spacing(8),
    );
    let multi = card(
        crate::tr!("import-3ds", "multi"),
        stacked(
            [
                crate::tr!("import-3ds", "ask"),
                crate::tr!("import-3ds", "split"),
                crate::tr!("import-3ds", "first"),
                crate::tr!("import-3ds", "none"),
            ]
            .into_iter()
            .enumerate()
            .map(|(k, label)| (label, state.multi == k, msg(ImportDialogMsg::TdsMulti(k))))
            .collect(),
        ),
    );
    let any = state.checked.iter().any(|c| *c);
    let mut actions = row![
        button(text("?").size(12))
            .on_press(msg(ImportDialogMsg::Help("3ds")))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
    ]
    .spacing(6)
    .align_y(iced::Center);
    if !any {
        actions = actions.push(text(crate::tr!("import-3ds", "reason")).size(11).style(muted_style));
    }
    actions = actions
        .push(dialog_button(crate::tr!("import-3ds", "cancel"), Message::CloseModal, false))
        .push(dialog_button_styled_opt(
            crate::tr!("import-3ds", "import"),
            any.then(|| msg(ImportDialogMsg::TdsOk)),
            button::primary,
        ));
    column![
        top,
        row![
            container(objects).width(Length::FillPortion(3)),
            column![layers, multi].spacing(10).width(Length::FillPortion(2)),
        ]
        .spacing(10),
        actions,
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

fn material_swatch<'a>(color: Option<[u8; 3]>) -> Element<'a, Message> {
    let [r, g, b] = color.unwrap_or([0x80; 3]);
    container(Space::new().width(10).height(10))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(iced::Color::from_rgb8(r, g, b))),
            border: Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// The mesh with several materials and what it becomes.
pub fn view_tds_material<'a>(
    state: &'a TdsMaterialState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let Some((name, materials)) = state.queue.first() else {
        return Space::new().into();
    };
    let badge = container(crate::ui::icons::themed_primary(crate::ui::icons::MESH, 18.0))
        .width(34)
        .height(34)
        .center_x(34)
        .center_y(34)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().primary.base.color.scale_alpha(0.13),
            )),
            border: Border {
                radius: 17.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
    let mut chips = row![text(crate::tr!("import-3ds", "materials", n = (materials.len() as i64)))
        .size(11)
        .style(muted_style)]
    .spacing(6)
    .align_y(iced::Center);
    for (material, color) in materials {
        chips = chips.push(
            container(row![material_swatch(*color), text(material.as_str()).size(11)].spacing(4).align_y(iced::Center))
                .padding([1, 7])
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(theme.palette().background.base.color)),
                    border: Border {
                        width: 1.0,
                        radius: 9.0.into(),
                        color: theme.palette().background.strong.color,
                    },
                    ..Default::default()
                }),
        );
    }
    let object = container(
        row![
            badge,
            column![
                text(name.as_str()).size(13).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::DEFAULT
                }),
                chips
            ]
            .spacing(3)
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .padding([10, 12])
    .width(Fill)
    .style(crate::ui::window::pdf_dialogs::card_style);

    let choosing = state.choice == 2;
    let chosen = state.material.checked_sub(1).and_then(|k| materials.get(k));
    let head_label = |entry: Option<&(String, Option<[u8; 3]>)>| -> Element<'a, Message> {
        match entry {
            Some((material, color)) => row![material_swatch(*color), text(material.clone()).size(12)]
                .spacing(6)
                .align_y(iced::Center)
                .into(),
            None if choosing => text(crate::tr!("import-3ds-alert", "none")).size(12).into(),
            None => text(crate::tr!("import-3ds-alert", "none")).size(12).style(muted_style).into(),
        }
    };
    let head = button(
        row![
            head_label(if choosing { chosen } else { None }),
            Space::new().width(Fill),
            crate::ui::icons::themed_arrow_toggle(state.list_open, 9.0),
        ]
        .align_y(iced::Center),
    )
    .on_press_maybe(choosing.then(|| msg(ImportDialogMsg::TdsMatList(!state.list_open))))
    .style(button_style(false))
    .padding([4, 8])
    .width(Fill);
    let field: Element<'a, Message> = if choosing && state.list_open {
        let mut list = column![].spacing(1);
        for k in 0..=materials.len() {
            list = list.push(
                button(head_label(k.checked_sub(1).and_then(|j| materials.get(j))))
                    .on_press(msg(ImportDialogMsg::TdsMatPick(k)))
                    .style(crate::ui::color_select::list_row_style)
                    .padding([4, 8])
                    .width(Fill),
            );
        }
        iced_aw::DropDown::new(
            head,
            container(list).style(crate::ui::color_select::popup_panel_style).padding(2),
            true,
        )
        .alignment(iced_aw::drop_down::Alignment::Bottom)
        .offset(2.0)
        .on_dismiss(msg(ImportDialogMsg::TdsMatList(false)))
        .into()
    } else {
        head.into()
    };
    let note = match state.choice {
        0 => crate::tr!("import-3ds-alert", "note-split"),
        1 => crate::tr!("import-3ds-alert", "note-first"),
        _ => crate::tr!("import-3ds-alert", "note-select"),
    };
    let assignment = card(
        crate::tr!("import-3ds-alert", "assignment"),
        column![
            segmented(
                [
                    crate::tr!("import-3ds-alert", "split"),
                    crate::tr!("import-3ds-alert", "first"),
                    crate::tr!("import-3ds-alert", "select"),
                ]
                .into_iter()
                .enumerate()
                .map(|(k, label)| (label, state.choice == k, Some(msg(ImportDialogMsg::TdsMatChoice(k)))))
                .collect()
            ),
            row![
                text(crate::tr!("import-3ds-alert", "material"))
                    .size(11)
                    .style(muted_style)
                    .width(Length::Fixed(70.0)),
                field,
            ]
            .spacing(6)
            .align_y(iced::Center),
            text(note).size(11).style(muted_style),
        ]
        .spacing(8),
    );
    let remember = button(
        row![
            crate::ui::icons::themed_check_cell(state.remember),
            text(crate::tr!("import-3ds-alert", "remember")).size(11),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .on_press(msg(ImportDialogMsg::TdsMatRemember(!state.remember)))
    .style(button_style(state.remember))
    .padding([4, 10]);
    let mut remaining = column![row![remember]].spacing(8);
    if !state.remember {
        remaining = remaining.push(text(crate::tr!("import-3ds-alert", "remember-note")).size(11).style(muted_style));
    }
    column![
        object,
        assignment,
        card(crate::tr!("import-3ds-alert", "remaining"), remaining),
        footer(
            "3ds-material",
            std::borrow::Cow::Owned(crate::tr!("import-3ds-alert", "apply")),
            Some(msg(ImportDialogMsg::TdsMatApply)),
        ),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

/// The round amber warning mark the warning windows lead with.
pub(crate) fn warning_badge<'a>() -> Element<'a, Message> {
    container(
        iced::widget::svg(crate::ui::icons::themed_handle(crate::ui::icons::WARNING))
            .width(18.0)
            .height(18.0)
            .style(|_: &Theme, _| iced::widget::svg::Style { color: Some(AMBER) }),
    )
    .width(34)
    .height(34)
    .center_x(34)
    .center_y(34)
    .style(|_: &Theme| container::Style {
        background: Some(Background::Color(AMBER.scale_alpha(0.15))),
        border: Border {
            radius: 17.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

/// A texture map a 3D Studio import did not find: the file and the material
/// that comes in without it.
pub fn view_tds_map<'a>(map: &'a (String, String), sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message> {
    let (file, material) = map;
    let badge = warning_badge();
    let chip = container(
        row![
            crate::ui::icons::themed_secondary(crate::ui::icons::IMAGE, 12.0),
            text(file.as_str()).size(11)
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .padding([2, 8])
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        border: Border {
            width: 1.0,
            radius: 10.0.into(),
            color: theme.palette().background.strong.color,
        },
        ..Default::default()
    });
    column![
        row![
            badge,
            column![
                text(crate::tr!("import-3ds-map", "heading")).size(13).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::DEFAULT
                }),
                chip,
                text(crate::tr!("import-3ds-map", "note", material = material.as_str()))
                    .size(11)
                    .style(muted_style),
            ]
            .spacing(8),
        ]
        .spacing(12),
        iced::widget::rule::horizontal(1),
        row![
            Space::new().width(Fill),
            dialog_button_styled_opt(
                std::borrow::Cow::Owned(crate::tr!("import-3ds-map", "ok")),
                Some(Message::CloseModal),
                button::primary
            ),
        ],
    ]
    .spacing(12)
    .padding([12, 14])
    .width(sizing.width)
    .into()
}

pub fn view_dgn_import<'a>(
    state: &'a DgnImportState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let top = container(
        row![
            container(text(file_name(&state.path)).size(12))
                .padding([4, 10])
                .style(|theme: &Theme| container::Style {
                    border: Border {
                        width: 1.0,
                        radius: 12.0.into(),
                        color: theme.palette().primary.base.color,
                    },
                    ..Default::default()
                }),
            text(format!(
                "{} · {} · {}",
                short_path(&state.path),
                state.version,
                crate::tf!("{n} model(s)", n = state.models.len())
            ))
            .size(11)
            .style(muted_style)
            .width(Fill)
            .wrapping(iced::advanced::text::Wrapping::None),
            button(text(t!("Change file...")).size(11))
                .on_press(msg(ImportDialogMsg::DgnChange))
                .style(button::text)
                .padding([4, 6]),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .padding([8, 12])
    .width(Fill)
    .style(crate::ui::window::pdf_dialogs::card_style);

    let tiles = state.models.iter().enumerate().map(|(k, m)| {
        let chosen = k == state.model;
        let picture: Element<'a, Message> = match &m.image {
            Some(handle) => image(handle.clone()).width(Fill).height(Fill).into(),
            None => Space::new().width(Fill).height(Fill).into(),
        };
        button(
            column![
                container(picture)
                    .width(Length::Fixed(132.0))
                    .height(Length::Fixed(80.0))
                    .padding(4)
                    .style(well_style),
                text(m.label.clone())
                    .size(11)
                    .width(Length::Fixed(132.0))
                    .align_x(iced::Center),
            ]
            .spacing(4),
        )
        .on_press(msg(ImportDialogMsg::DgnModel(k)))
        .style(tile_style(chosen))
        .padding(5)
        .into()
    });
    let model = card(
        t!("Model").into_owned(),
        scrollable(
            iced::widget::Row::with_children(tiles.collect::<Vec<_>>())
                .spacing(6)
                .wrap()
                .vertical_spacing(6),
        )
        .height(Length::Fixed(118.0)),
    );
    let current = state.current_drawing;
    let target = card(
        t!("Target").into_owned(),
        column![
            segmented(vec![
                (
                    t!("New Drawing").into_owned(),
                    !current,
                    Some(msg(ImportDialogMsg::DgnCurrent(false)))
                ),
                (
                    t!("Current Drawing").into_owned(),
                    current,
                    Some(msg(ImportDialogMsg::DgnCurrent(true)))
                ),
            ]),
            text(t!("On name conflict")).size(10).style(muted_style),
            segmented(vec![
                (
                    t!("Add prefix to dependent definitions").into_owned(),
                    current && state.prefix_duplicates,
                    current.then(|| msg(ImportDialogMsg::DgnPrefix(true)))
                ),
                (
                    t!("Ignore duplicate names").into_owned(),
                    current && !state.prefix_duplicates,
                    current.then(|| msg(ImportDialogMsg::DgnPrefix(false)))
                ),
            ]),
        ]
        .spacing(6),
    );
    let unit = |name: &str| {
        let word = crate::modules::insert::dgn_import::unit_display(name);
        if word.is_empty() {
            String::new()
        } else {
            format!(" · {}", t!(word.as_str()))
        }
    };
    let units = card(
        t!("Units").into_owned(),
        column![
            segmented(vec![
                (
                    format!("{}{}", t!("Master unit"), unit(&state.units.0)),
                    !state.sub_units,
                    Some(msg(ImportDialogMsg::DgnSub(false)))
                ),
                (
                    format!("{}{}", t!("Sub unit"), unit(&state.units.1)),
                    state.sub_units,
                    Some(msg(ImportDialogMsg::DgnSub(true)))
                ),
            ]),
            row![
                iced::widget::toggler(state.explode_text_nodes)
                    .on_toggle(|on| msg(ImportDialogMsg::DgnExplode(on)))
                    .size(16),
                text(t!("Explode text nodes to text elements")).size(11),
            ]
            .spacing(8)
            .align_y(iced::Center),
        ]
        .spacing(8),
    );
    use crate::modules::insert::dgn_import::{ExistingReference as E, ReferenceMode as R};
    let translate = state.references == R::Translate;
    let references = card(
        t!("External DGN references").into_owned(),
        column![
            segmented(vec![
                (
                    t!("Bind as block / xref").into_owned(),
                    translate,
                    Some(msg(ImportDialogMsg::DgnReferences(R::Translate)))
                ),
                (
                    t!("Ignore").into_owned(),
                    state.references == R::Ignore,
                    Some(msg(ImportDialogMsg::DgnReferences(R::Ignore)))
                ),
                (
                    t!("Attach as DGN underlay").into_owned(),
                    state.references == R::Underlay,
                    Some(msg(ImportDialogMsg::DgnReferences(R::Underlay)))
                ),
            ]),
            text(t!("When the translated drawing exists"))
                .size(10)
                .style(muted_style),
            segmented(
                [
                    (t!("Prompt to overwrite"), E::Prompt),
                    (t!("Overwrite without prompting"), E::Overwrite),
                    (t!("Do not overwrite"), E::Keep),
                ]
                .into_iter()
                .map(|(label, value)| {
                    (
                        label.into_owned(),
                        translate && state.existing_reference == value,
                        translate.then(|| msg(ImportDialogMsg::DgnExisting(value))),
                    )
                })
                .collect()
            ),
        ]
        .spacing(6),
    );
    let tabs = [t!("Layer"), t!("Linetype"), t!("Lineweight"), t!("Color")];
    let mut table = column![row![
        text("DGN").size(11).style(muted_style).width(Fill),
        text("DWG").size(11).style(muted_style).width(Fill),
    ]]
    .spacing(4);
    for (dgn, dwg) in &state.mapping[state.mapping_tab.min(3)] {
        table = table.push(row![
            text(dgn.clone()).size(11).width(Fill),
            text("\u{2192}").size(11).style(muted_style).width(Length::Fixed(24.0)),
            text(dwg.clone()).size(11).width(Fill),
        ]);
    }
    let mapping = card(
        t!("Property mapping").into_owned(),
        column![
            row![chip(
                "Standard".to_string(),
                true,
                msg(ImportDialogMsg::DgnMappingTab(state.mapping_tab))
            )],
            text(t!("Standard mapping setup.")).size(11).style(muted_style),
            segmented(
                tabs.iter()
                    .enumerate()
                    .map(|(k, label)| {
                        (
                            label.to_string(),
                            state.mapping_tab == k,
                            Some(msg(ImportDialogMsg::DgnMappingTab(k))),
                        )
                    })
                    .collect()
            ),
            scrollable(table).height(Length::Fixed(250.0)),
            text(t!("The preview shows the selected setup's effect on this file."))
                .size(11)
                .style(muted_style),
        ]
        .spacing(8),
    );
    column![
        top,
        row![
            column![model, target, units, references]
                .spacing(10)
                .width(Length::FillPortion(3)),
            container(mapping).width(Length::FillPortion(2)),
        ]
        .spacing(10),
        footer("dgn", t!("Import"), Some(msg(ImportDialogMsg::DgnOk))),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}
