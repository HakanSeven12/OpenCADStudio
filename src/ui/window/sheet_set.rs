//! Sheet Set Manager (SHEETSET): the open sheet sets as a tree of subsets
//! and sheets, the New Sheet Set wizard (NEWSHEETSET), the Sheet Set /
//! Subset / Sheet Properties dialog and the small New Sheet, New Subset,
//! Import Layout and Rename & Renumber forms. The tree is read from the
//! sheet set database (`.dst`) itself on every view.

use crate::app::Message;
use crate::t;
use crate::ui::dock::PanelId;
use crate::ui::kit;
use crate::ui::kit::menu::{self, Item};
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, field_style};
use crate::ui::style::{chrome, kit_metrics as km, text as text_size};
use crate::ui::window::pdf_dialogs::{accent_text, card_style};
use document::sheet_set::{self as ss, ComponentKind, Element as SsElement, SheetSetDatabase};
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::Padding;
use iced::{Background, Border, Element, Fill, Length, Theme};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const SET_ICON: &[u8] = include_bytes!("../../../assets/icons/sheetset.svg");

/// SSMAUTOOPEN, SSLOCATE, SSMPOLLTIME and SSMSHEETSTATUS: profile settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SheetSetSettings {
    pub auto_open: u8,
    pub locate: u8,
    pub poll_time: u16,
    pub sheet_status: u8,
}

impl Default for SheetSetSettings {
    fn default() -> Self {
        Self {
            auto_open: 1,
            locate: 1,
            poll_time: 60,
            sheet_status: 2,
        }
    }
}

/// The palette's three pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SsmTab {
    #[default]
    SheetList,
    SheetViews,
    ModelViews,
}

/// The palette and the open sheet sets.
#[derive(Debug, Default)]
pub struct SheetSetManager {
    pub show: bool,
    pub settings: SheetSetSettings,
    /// Open sheet sets (each read from its `.dst`).
    pub sets: Vec<SheetSetDatabase>,
    /// The set the palette shows.
    pub current: Option<usize>,
    pub tab: SsmTab,
    /// Folded subset / set ids.
    pub collapsed: HashSet<String>,
    /// Highlighted component id.
    pub selected: Option<String>,
    /// SSFOUND: the `.dst` the last opened sheet drawing named, when found.
    pub found: String,
    /// The open dialog (shown in the `SheetSet` modal).
    pub dialog: Option<SsDialog>,
    /// Last seen modification time of each open `.dst` (by path key).
    pub seen: std::collections::HashMap<String, std::time::SystemTime>,
    /// Sheet status by sheet id (SSMSHEETSTATUS) and when it was taken.
    pub status: std::collections::HashMap<String, SheetStatus>,
    pub status_at: Option<iced::time::Instant>,
    /// Sheet Views: by category (else by sheet).
    pub by_category: bool,
    /// Model Views: location id, folder and its drawings.
    pub locations: Vec<(String, String, Vec<String>)>,
    /// Drawings this application holds `.dwl` / `.dwl2` lock files for.
    pub locks: HashSet<std::path::PathBuf>,
    /// Model Views: drawings shown open (path keys) and their named model
    /// views, read when a drawing is opened in the tree.
    pub open_drawings: HashSet<String>,
    pub model_views: std::collections::HashMap<String, Vec<String>>,
    /// What the pending insertion point places.
    pub placing: Option<Placing>,
    /// A named view to show once the sheet drawing being opened is up.
    pub pending_view: Option<String>,
    /// The fly-out submenu open in a row menu (its key).
    pub submenu: Option<String>,
    /// Block previews (`file|name`; no name: the whole drawing).
    pub previews:
        std::collections::HashMap<String, Vec<crate::scene::model::wire_model::WireModel>>,
    /// Drawings read for Model Views, placement and blocks, by path key.
    pub drawings: DrawingCache,
}

/// Drawings read from disk with their modification time; a changed file is
/// read again (off the UI thread, see [`read_drawing`]).
#[derive(Default)]
pub struct DrawingCache(
    pub  std::collections::HashMap<
        String,
        (std::time::SystemTime, std::sync::Arc<document::Document>),
    >,
);

impl std::fmt::Debug for DrawingCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DrawingCache({} drawings)", self.0.len())
    }
}

impl DrawingCache {
    /// The drawing at `path` when it was read and is unchanged since.
    pub fn get(&self, path: &str) -> Option<std::sync::Arc<document::Document>> {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        let (t, doc) = self.0.get(&ss::path_key(path))?;
        (*t == modified).then(|| doc.clone())
    }

    pub fn insert(
        &mut self,
        path: &str,
        modified: std::time::SystemTime,
        doc: std::sync::Arc<document::Document>,
    ) {
        self.0.insert(ss::path_key(path), (modified, doc));
    }

    /// Take the drawing at `path` out (to lend it to a scene); put it back
    /// with [`DrawingCache::insert`].
    pub fn take(
        &mut self,
        path: &str,
    ) -> Option<(std::time::SystemTime, std::sync::Arc<document::Document>)> {
        self.0.remove(&ss::path_key(path))
    }
}

/// A drawing read for the cache (its modification time and document).
#[derive(Clone)]
pub struct ReadDrawing(
    pub Result<(std::time::SystemTime, std::sync::Arc<document::Document>), String>,
);

impl std::fmt::Debug for ReadDrawing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_ok() {
            "ReadDrawing(Ok)"
        } else {
            "ReadDrawing(Err)"
        })
    }
}

/// Read the drawing at `path` (run on a worker thread).
pub fn read_drawing(path: &str) -> ReadDrawing {
    let modified = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| e.to_string());
    ReadDrawing(modified.and_then(|t| {
        let doc = crate::io::load_file(std::path::Path::new(path)).map_err(|e| e.to_string())?;
        Ok((t, std::sync::Arc::new(doc)))
    }))
}

/// The paper-space layouts (name, handle) and the sheet link of a drawing
/// picked for Import Layout as Sheet, or why it could not be read.
pub type ImportRead = Result<
    (
        Vec<(String, String)>,
        Option<document::sheet_set::SheetSetData>,
    ),
    String,
>;

/// A row menu item: an entry, a separator, or a fly-out submenu (label, key, items).
#[derive(Debug, Clone)]
pub enum Mi {
    Entry(String, Option<Message>),
    Sep,
    Sub(String, String, Vec<(String, Option<Message>)>),
}

/// What a sheet set point pick places on the current sheet.
#[derive(Debug, Clone)]
pub enum Placing {
    /// Place on Sheet: a model drawing and its named view (`None`: the
    /// drawing's extents).
    View {
        set: usize,
        sheet: String,
        drawing: String,
        view: Option<String>,
        /// The model drawing, read on a worker for its reference (`None`:
        /// the sheet has that reference already).
        read: Option<Box<crate::io::xref::XrefRead>>,
    },
    /// A callout or view label block (file, block name) for sheet view `view`.
    Block {
        set: usize,
        sheet: String,
        view: String,
        file: String,
        name: String,
    },
}

/// Commands of a sheet view row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewAction {
    Display,
    Rename,
    /// Set the view's category.
    Category(String),
    /// Place a callout block (its id in the set's callout blocks).
    Callout(String),
    Label,
}

/// A sheet whose drawing is missing, or open (a `.dwl` lock file, or a tab here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetStatus {
    Missing,
    Locked,
}

impl SheetSetManager {
    pub fn db(&self) -> Option<&SheetSetDatabase> {
        self.sets.get(self.current?)
    }

    pub fn db_mut(&mut self) -> Option<&mut SheetSetDatabase> {
        self.sets.get_mut(self.current?)
    }
}

/// Right-click commands of a tree row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Open,
    NewSheet,
    NewSubset,
    ImportLayout,
    Rename,
    Remove,
    Properties,
    Close,
}

#[derive(Debug, Clone)]
pub enum SheetSetMsg {
    /// The set combo: a set by index, or `None` for "Open...".
    PickSet(Option<usize>),
    Tab(SsmTab),
    Expand(String),
    Select(String),
    /// Double-click: open the sheet's drawing at its layout.
    Activate(String),
    Menu(String, MenuAction),
    Refresh,
    /// Once a second: reload sets whose `.dst` changed on disk.
    Poll,
    /// Import: Browse for Drawings and the drawings picked.
    BrowseDrawings,
    ImportAdd(Vec<std::path::PathBuf>),
    /// The drawings picked for import, read: into a new Import dialog
    /// (`true`) or replacing the open dialog's rows.
    ImportRead(bool, Vec<(std::path::PathBuf, ImportRead)>),
    /// A drawing read for the cache, then the message that needed it.
    DrawingRead(String, ReadDrawing, Box<SheetSetMsg>),
    /// Place on Sheet: the model drawing (and view) read on a worker for its
    /// reference, for the sheet drawing at that path.
    ViewReferenceRead(
        String,
        Option<String>,
        Option<std::path::PathBuf>,
        Box<crate::io::xref::XrefRead>,
    ),
    /// The wizard's folder of existing drawings, scanned for layouts.
    FolderScanned(std::path::PathBuf, Vec<FoundLayout>),
    /// Rename & Renumber: apply and go to the previous / next sheet.
    Previous,
    Next,
    /// Rename & Renumber: the closed sheet drawing's layout renamed in its
    /// file on a worker (or why not); then OK (`None`) or Previous / Next.
    RenameWritten(Box<Form>, Option<bool>, Result<(), String>),
    /// New Sheet: its drawing written on a worker (its layout handle, or
    /// why not).
    SheetWritten(Box<Form>, Box<NewSheetWrite>, Result<String, String>),
    /// Sheet Views: by category or by sheet.
    ViewsByCategory(bool),
    NewCategory,
    CategoryProperties(String),
    CategoryRemove(String),
    AddBlocks,
    BlocksPicked(Option<std::path::PathBuf>),
    /// Model Views.
    AddLocation,
    LocationPicked(Option<std::path::PathBuf>),
    RemoveLocation(String),
    OpenDrawing(String),
    /// Model Views: open / fold a drawing (its named views), See Model Space
    /// Views, Place on Sheet (drawing, named view).
    ToggleDrawing(String),
    SeeViews(String),
    PlaceOnSheet(String, Option<String>),
    /// Sheet Views: a view row's command.
    ViewMenu(String, ViewAction),
    /// List of Blocks / Select Block.
    BlockListSelect(usize),
    /// A row menu's fly-out submenu opened (hovered) or closed.
    Submenu(Option<String>),
    BlockListAdd,
    BlockListDelete,
    SelectBlockBrowse,
    SelectBlockPicked(Option<std::path::PathBuf>),
    SelectBlockWhole(bool),
    SelectBlockCheck(usize, bool),
    /// A `.dst` picked by Open (or given by automation).
    OpenPicked(Option<std::path::PathBuf>),
    /// A drawing picked by Import Layout as Sheet.
    ImportPicked(Option<std::path::PathBuf>),
    /// A folder picked for a dialog field.
    FolderPicked(FieldId, Option<std::path::PathBuf>),
    /// A template drawing picked for a sheet creation template.
    TemplatePicked(Option<std::path::PathBuf>),
    Browse(FieldId),
    BrowseTemplate,
    /// Edit a text field of the open dialog.
    Input(FieldId, String),
    Toggle(FieldId, bool),
    Choose(FieldId, usize),
    Wizard(WizardMsg),
    /// Open the Properties dialog from the wizard.
    WizardProperties,
    AddCustom,
    AddCustomOk,
    AddCustomCancel,
    /// A custom property row's value.
    CustomValue(usize, String),
    Ok,
    Cancel,
    Help,
}

#[derive(Debug, Clone)]
pub enum WizardMsg {
    Step(u8),
    Existing(bool),
    /// Toggle one drawing layout of the "existing drawings" page.
    LayoutOn(usize, bool),
    RemoveFolder(usize),
}

/// Text fields, toggles and lists of the dialogs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldId {
    /// A Properties row by index.
    Row(usize),
    Name,
    Description,
    Folder,
    Hierarchy,
    Number,
    Title,
    FileName,
    /// The layout list of Import Layout as Sheet.
    Layout,
    /// Add Custom Property: name, default value, owner (0 set, 1 sheet).
    CustomName,
    CustomDefault,
    CustomOwner,
    /// A wizard "existing drawings" folder to add.
    AddFolder,
    /// Prompt for template (Properties).
    PromptTemplate,
    OpenAfter,
    Publish,
    LayoutName,
    /// Rename option 0..4.
    RenameOption(usize),
    ImportRow(usize),
    ImportPrefix,
    /// View Category: a callout block's check box.
    CategoryBlock(usize),
}

/// What a Properties row edits.
#[derive(Debug, Clone, PartialEq)]
pub enum RowKey {
    /// An `AcSmProp` of the component.
    Prop(&'static str),
    /// A file reference property (`NewSheetLocation`).
    Folder(&'static str),
    /// The sheet creation template (`DefDwtLayout`): the value shows
    /// `Layout (file)`; `template` holds the picked drawing and layout.
    Template,
    /// Yes / No: prompt for template.
    PromptTemplate,
    /// Publish Sheets in Subset (`OverrideSheetPublish`).
    Publish,
    /// Shown only.
    ReadOnly,
}

#[derive(Debug, Clone)]
pub struct PropRow {
    pub group: &'static str,
    pub label: &'static str,
    pub key: RowKey,
    pub value: String,
}

/// The Sheet Set / Subset / Sheet Properties dialog.
#[derive(Debug, Clone)]
pub struct Properties {
    /// `None` edits the wizard's draft set.
    pub set: Option<usize>,
    pub component: String,
    pub kind: ComponentKind,
    pub title: String,
    pub rows: Vec<PropRow>,
    /// Name, value, flags.
    pub custom: Vec<(String, String, i32)>,
    /// Add Custom Property being filled: name, default value, owner is sheet.
    pub adding: Option<(String, String, bool)>,
    /// The template picked in this dialog: drawing and layout.
    pub template: Option<(String, String)>,
    /// Back to the wizard on OK / Cancel.
    pub wizard: Option<Box<Wizard>>,
    pub error: Option<String>,
}

/// One layout found by the "existing drawings" wizard page.
#[derive(Debug, Clone)]
pub struct FoundLayout {
    pub folder: String,
    pub file: String,
    pub layout: String,
    pub handle: String,
    pub on: bool,
}

/// NEWSHEETSET.
#[derive(Debug, Clone)]
pub struct Wizard {
    /// 0 Begin, 1 Sheet Set Details, 2 Choose Layouts (existing drawings), 3 Confirm.
    pub step: u8,
    pub existing: bool,
    pub name: String,
    pub description: String,
    pub folder: String,
    pub hierarchy: bool,
    pub folders: Vec<String>,
    pub layouts: Vec<FoundLayout>,
    /// The set being built: name, description, properties and custom
    /// properties go into it; Finish writes it.
    pub draft: SheetSetDatabase,
    pub error: Option<String>,
}

/// New Sheet, New Subset, Rename & Renumber and Import Layout as Sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormKind {
    #[default]
    NewSheet,
    NewSubset,
    Rename,
    ImportLayout,
    /// Rename & Renumber View.
    RenameView,
}

/// One layout of Import Layouts as Sheets.
#[derive(Debug, Clone)]
pub struct ImportRow {
    pub drawing: String,
    pub layout: String,
    pub handle: String,
    /// Already a sheet of this set: cannot be imported.
    pub taken: bool,
    /// The drawing's sheet link names another sheet set (importable, warned).
    pub warn: bool,
    pub on: bool,
}

/// What New Sheet writes: its drawing at `path` (the set's `file` for it),
/// from layout `template` of a drawing (none: a new drawing's first
/// layout), with one layout `layout_name`, linked to the set `dst` at
/// revision `version`.
#[derive(Debug, Clone)]
pub struct NewSheetWrite {
    pub path: std::path::PathBuf,
    pub file: String,
    pub template: Option<(String, String)>,
    pub layout_name: String,
    pub dst: String,
    pub version: i32,
}

#[derive(Debug, Clone, Default)]
pub struct Form {
    pub kind: FormKind,
    pub set: usize,
    /// Parent (new sheet / subset / import) or the sheet / subset renamed.
    pub component: String,
    pub number: String,
    pub title: String,
    pub file_name: String,
    pub folder: String,
    /// The sheet template picked when the subset prompts for one (file, layout).
    pub drawing: String,
    pub template: Option<(String, String)>,
    /// New Sheet: open the new sheet afterwards.
    pub open_after: bool,
    /// New Subset: folder hierarchy; publish sheets by their own setting.
    pub hierarchy: bool,
    pub publish: bool,
    /// Rename & Renumber: layout name and the four rename options
    /// (layout = title, layout prefix, file = title, file prefix).
    pub layout_name: String,
    pub rename: [bool; 4],
    /// Import: the layouts found and "prefix sheet titles with file name".
    pub rows: Vec<ImportRow>,
    pub prefix: bool,
    pub error: Option<String>,
}

/// Select Layout as Sheet Template (a subset that prompts for its template).
#[derive(Debug, Clone)]
pub struct TemplatePick {
    pub form: Form,
    pub file: String,
    pub layouts: Vec<String>,
    pub layout: usize,
}

/// The View Category dialog.
#[derive(Debug, Clone)]
pub struct Category {
    pub set: usize,
    /// `None` creates a category.
    pub id: Option<String>,
    pub name: String,
    /// Callout block id (`new:file|name|handle` until saved), label, used.
    pub blocks: Vec<(String, String, bool)>,
    /// Callout blocks deleted in List of Blocks.
    pub removed: Vec<String>,
    pub error: Option<String>,
}

/// List of Blocks: the set's callout blocks (id, label).
#[derive(Debug, Clone)]
pub struct BlockList {
    pub category: Box<Category>,
    pub blocks: Vec<(String, String)>,
    pub selected: Option<usize>,
    /// Preview of the selected block (`file|name`).
    pub preview: Option<String>,
}

/// Select Block: a drawing, taken whole or by its named blocks.
#[derive(Debug, Clone)]
pub struct SelectBlock {
    pub list: Box<BlockList>,
    pub file: String,
    pub whole: bool,
    /// Block name, handle, checked.
    pub names: Vec<(String, String, bool)>,
    /// Preview shown (`file|name`).
    pub preview: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SsDialog {
    Wizard(Wizard),
    Properties(Properties),
    Form(Form),
    /// Remove Sheets confirmation: set, sheet id, question.
    Confirm(usize, String, String),
    Template(TemplatePick),
    Category(Category),
    BlockList(BlockList),
    SelectBlock(SelectBlock),
}

impl SsDialog {
    pub fn title(&self) -> String {
        match self {
            SsDialog::Wizard(_) => t!("Create Sheet Set").into_owned(),
            SsDialog::Properties(p) => p.title.clone(),
            SsDialog::Form(f) => match f.kind {
                FormKind::NewSheet => t!("New Sheet").into_owned(),
                FormKind::NewSubset => t!("Subset Properties").into_owned(),
                FormKind::Rename => t!("Rename & Renumber Sheet").into_owned(),
                FormKind::ImportLayout => t!("Import Layouts as Sheets").into_owned(),
                FormKind::RenameView => t!("Rename & Renumber View").into_owned(),
            },
            SsDialog::BlockList(_) => t!("List of Blocks").into_owned(),
            SsDialog::SelectBlock(_) => t!("Select Block").into_owned(),
            SsDialog::Confirm(..) => t!("Remove Sheets").into_owned(),
            SsDialog::Template(_) => t!("Select Layout as Sheet Template").into_owned(),
            SsDialog::Category(_) => t!("View Category").into_owned(),
        }
    }
}

fn msg(m: SheetSetMsg) -> Message {
    Message::SheetSet(m)
}

// ── palette ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pick(Option<usize>, String);

impl std::fmt::Display for Pick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

/// A palette-menu item; `None` shows it disabled.
fn menu_item(label: String, m: Option<Message>) -> Item {
    match m {
        Some(m) => Item::new(label, m),
        None => Item::new(label, Message::Noop).enabled(false),
    }
}

/// A row's right-click menu.
/// The reference's order per row kind; a sheet's New Sheet / Import work on
/// its subset, and only an empty subset can be removed.
fn row_menu(id: String, kind: ComponentKind, empty: bool) -> Element<'static, Message> {
    let entry = |label: &str, action: MenuAction, on: bool| {
        menu_item(
            t!(label).into_owned(),
            on.then(|| msg(SheetSetMsg::Menu(id.clone(), action))),
        )
    };
    let mut items = Vec::new();
    match kind {
        ComponentKind::SheetSet => items.extend([
            entry("Close Sheet Set", MenuAction::Close, true),
            Item::Separator,
        ]),
        ComponentKind::Sheet => {
            items.extend([entry("Open", MenuAction::Open, true), Item::Separator])
        }
        ComponentKind::Subset => {}
    }
    items.push(entry("New Sheet...", MenuAction::NewSheet, true));
    if kind != ComponentKind::Sheet {
        items.push(entry("New Subset...", MenuAction::NewSubset, true));
    }
    items.push(entry(
        "Import Layout as Sheet...",
        MenuAction::ImportLayout,
        true,
    ));
    match kind {
        ComponentKind::SheetSet => {}
        ComponentKind::Subset => items.extend([
            Item::Separator,
            entry("Rename Subset...", MenuAction::Properties, true),
            entry("Remove Subset", MenuAction::Remove, empty),
        ]),
        ComponentKind::Sheet => items.extend([
            Item::Separator,
            entry("Rename & Renumber...", MenuAction::Rename, true),
            entry("Remove Sheet", MenuAction::Remove, true),
        ]),
    }
    items.push(Item::Separator);
    items.push(entry("Properties...", MenuAction::Properties, true));
    menu::view(items)
}

/// A small status badge after a sheet name.
fn badge<'a>(prefix: &str, label: &str, warn: bool) -> Element<'a, Message> {
    container(
        text(format!("{prefix}{}", t!(label)))
            .size(text_size::UI_SMALL)
            .line_height(text_size::line()),
    )
    .padding(Padding::ZERO.left(km::CELL_PAD_X).right(km::CELL_PAD_X))
    .style(move |theme: &Theme| container::Style {
        text_color: Some(if warn {
            chrome::danger(theme)
        } else {
            chrome::muted(theme)
        }),
        border: Border {
            color: chrome::border(theme),
            width: km::line(),
            radius: (km::CHECK / 2.0).into(),
        },
        ..Default::default()
    })
    .into()
}

/// A tree row of the Sheet Views / Model Views pages with its own menu.
fn plain_row<'a>(
    depth: u16,
    icon: &'static [u8],
    label: String,
    fold: Option<(bool, Message)>,
    double: Option<Message>,
    menu: Vec<Mi>,
    open_sub: Option<String>,
) -> Element<'a, Message> {
    let mut item = kit::tree_row(label)
        .depth(depth)
        .icon(crate::ui::icons::semantic(icon, km::ROW_ICON));
    item = match fold {
        Some((open, m)) => item.fold(open, m),
        None => item.caret_room(),
    };
    if let Some(m) = double {
        item = item.on_double_click(m);
    }
    let area = item.view();
    if menu.is_empty() {
        return area;
    }
    iced_aw::ContextMenu::new(area, move || menu_panel(&menu, open_sub.as_deref())).into()
}

/// A row's right-click menu. A submenu opens beside its entry while
/// hovered (a click would close the menu).
fn menu_panel(menu: &[Mi], open: Option<&str>) -> Element<'static, Message> {
    let items = menu
        .iter()
        .map(|item| match item {
            Mi::Sep => Item::Separator,
            Mi::Entry(label, m) => menu_item(label.clone(), m.clone()),
            Mi::Sub(label, key, sub) => Item::submenu(
                label.clone(),
                msg(SheetSetMsg::Submenu(Some(key.clone()))),
                sub.iter()
                    .map(|(l, m)| menu_item(l.clone(), m.clone()))
                    .collect(),
            )
            .open(open == Some(key.as_str())),
        })
        .collect();
    menu::with_submenus(items, msg(SheetSetMsg::Submenu(None)))
}

/// A menu item: translated label and message (an empty label is a separator).
fn mi(label: &str, m: Option<Message>) -> Mi {
    if label.is_empty() {
        Mi::Sep
    } else {
        Mi::Entry(t!(label).into_owned(), m)
    }
}

fn tree_row<'a>(
    state: &SheetSetManager,
    el: &SsElement,
    depth: u16,
    icon: &'static [u8],
    label: String,
    fold: Option<bool>,
) -> Element<'a, Message> {
    let id = el.id().to_string();
    let kind = ComponentKind::of(el).unwrap_or(ComponentKind::Sheet);
    let selected = state.selected.as_deref() == Some(id.as_str());
    let status = state.status.get(&id).copied();
    let mut item = kit::tree_row(label)
        .depth(depth)
        .icon(crate::ui::icons::semantic(icon, km::ROW_ICON))
        .selected(selected)
        .on_press(msg(SheetSetMsg::Select(id.clone())))
        .on_double_click(msg(SheetSetMsg::Activate(id.clone())));
    item = match fold {
        Some(open) => item.fold(open, msg(SheetSetMsg::Expand(id.clone()))),
        None => item.caret_room(),
    };
    // A missing drawing: grey with a "? missing" badge; an open one: a lock badge.
    item = match status {
        Some(SheetStatus::Missing) => item.muted(true).trailing(badge("? ", "missing", true)),
        Some(SheetStatus::Locked) => item.trailing(badge("", "open", false)),
        None => item,
    };
    let area = item.view();
    let empty = !el.children.iter().any(|c| {
        matches!(
            ComponentKind::of(c),
            Some(ComponentKind::Subset | ComponentKind::Sheet)
        )
    });
    iced_aw::ContextMenu::new(area, move || row_menu(id.clone(), kind, empty)).into()
}

fn push_tree<'a>(
    state: &SheetSetManager,
    el: &SsElement,
    depth: u16,
    out: &mut Vec<Element<'a, Message>>,
) {
    for c in &el.children {
        match ComponentKind::of(c) {
            Some(ComponentKind::Subset) => {
                let open = !state.collapsed.contains(c.id());
                out.push(tree_row(
                    state,
                    c,
                    depth,
                    crate::ui::icons::FOLDER_OPEN,
                    c.prop("Name").unwrap_or("").to_string(),
                    Some(open),
                ));
                if open {
                    push_tree(state, c, depth + 1, out);
                }
            }
            Some(ComponentKind::Sheet) => {
                out.push(tree_row(
                    state,
                    c,
                    depth,
                    crate::ui::icons::DOC,
                    ss::number_and_title(c),
                    None,
                ));
            }
            _ => {}
        }
    }
}

/// A sheet view row and its menu: Display, Rename & Renumber, Set category
/// (the categories as sub-items), Place Callout Block (the set's callout
/// blocks as sub-items) and Place View Label Block.
fn view_row<'a>(
    state: &SheetSetManager,
    db: &SheetSetDatabase,
    view: &SsElement,
    depth: u16,
) -> Element<'a, Message> {
    let id = view.id().to_string();
    let act = |a: ViewAction| Some(msg(SheetSetMsg::ViewMenu(id.clone(), a)));
    let categories = db
        .view_categories()
        .iter()
        .map(|c| {
            (
                c.prop("Name").unwrap_or("").to_string(),
                act(ViewAction::Category(c.id().to_string())),
            )
        })
        .collect();
    let callouts = db
        .callout_blocks()
        .iter()
        .map(|b| {
            (
                b.prop("Name").unwrap_or("").to_string(),
                act(ViewAction::Callout(b.id().to_string())),
            )
        })
        .collect();
    let menu = vec![
        mi("Display", act(ViewAction::Display)),
        mi("Rename & Renumber...", act(ViewAction::Rename)),
        Mi::Sep,
        Mi::Sub(
            t!("Set category").into_owned(),
            format!("{id}|category"),
            categories,
        ),
        Mi::Sub(
            t!("Place Callout Block").into_owned(),
            format!("{id}|callout"),
            callouts,
        ),
        mi("Place View Label Block", act(ViewAction::Label)),
    ];
    plain_row(
        depth,
        crate::ui::icons::DOC,
        ss::number_and_title(view),
        None,
        act(ViewAction::Display),
        menu,
        state.submenu.clone(),
    )
}

/// The Sheet Views page: the set, then its view categories (or sheets) and their views.
fn sheet_views_rows<'a>(
    state: &SheetSetManager,
    db: &SheetSetDatabase,
) -> Vec<Element<'a, Message>> {
    let new_category = || mi("New View Category...", Some(msg(SheetSetMsg::NewCategory)));
    let mut rows = vec![plain_row(
        0,
        SET_ICON,
        db.name().to_string(),
        None,
        None,
        vec![new_category()],
        None,
    )];
    let views: Vec<&SsElement> = db
        .sheets()
        .into_iter()
        .flat_map(|s| db.sheet_views(s))
        .collect();
    if state.by_category {
        let named = db.view_categories();
        // Views of the unnamed (default) category sit directly under the set.
        for v in views.iter().filter(|v| {
            !SheetSetDatabase::view_category_of(v)
                .is_some_and(|c| named.iter().any(|n| n.id() == c))
        }) {
            rows.push(view_row(state, db, v, 1));
        }
        for c in named {
            let id = c.id().to_string();
            rows.push(plain_row(
                1,
                crate::ui::icons::FOLDER_OPEN,
                c.prop("Name").unwrap_or("").to_string(),
                None,
                Some(msg(SheetSetMsg::CategoryProperties(id.clone()))),
                vec![
                    new_category(),
                    mi(
                        "Properties...",
                        Some(msg(SheetSetMsg::CategoryProperties(id.clone()))),
                    ),
                    mi("Remove", Some(msg(SheetSetMsg::CategoryRemove(id.clone())))),
                ],
                None,
            ));
            for v in views
                .iter()
                .filter(|v| SheetSetDatabase::view_category_of(v) == Some(id.as_str()))
            {
                rows.push(view_row(state, db, v, 2));
            }
        }
    } else {
        for sheet in db.sheets() {
            let views = db.sheet_views(sheet);
            if views.is_empty() {
                continue;
            }
            rows.push(plain_row(
                1,
                crate::ui::icons::DOC,
                ss::number_and_title(sheet),
                None,
                None,
                vec![],
                None,
            ));
            for v in views {
                rows.push(view_row(state, db, v, 2));
            }
        }
    }
    rows
}

/// The Model Views page: each location folder, its drawings and, for an
/// opened drawing, its named model views.
fn model_views_rows<'a>(state: &SheetSetManager) -> Vec<Element<'a, Message>> {
    let mut rows = Vec::new();
    // A location shows relative to the set's folder when it can.
    let base = state
        .db()
        .and_then(|db| db.path.as_deref())
        .and_then(|p| std::path::Path::new(p).parent().map(|p| p.to_path_buf()));
    for (id, folder, files) in &state.locations {
        let label = base
            .as_deref()
            .and_then(|b| ss::relative_path(b, std::path::Path::new(folder)))
            .unwrap_or_else(|| folder.clone());
        rows.push(plain_row(
            0,
            crate::ui::icons::FOLDER_OPEN,
            label,
            None,
            None,
            vec![
                mi("Add New Location...", Some(msg(SheetSetMsg::AddLocation))),
                mi(
                    "Remove Location",
                    Some(msg(SheetSetMsg::RemoveLocation(id.clone()))),
                ),
            ],
            None,
        ));
        for file in files {
            let path = std::path::Path::new(folder)
                .join(file)
                .to_string_lossy()
                .to_string();
            let key = ss::path_key(&path);
            let open = state.open_drawings.contains(&key);
            rows.push(plain_row(
                1,
                crate::ui::icons::DOC,
                file.clone(),
                Some((open, msg(SheetSetMsg::ToggleDrawing(path.clone())))),
                Some(msg(SheetSetMsg::OpenDrawing(path.clone()))),
                vec![
                    mi("Open", Some(msg(SheetSetMsg::OpenDrawing(path.clone())))),
                    mi("", None),
                    mi(
                        "Place on Sheet",
                        Some(msg(SheetSetMsg::PlaceOnSheet(path.clone(), None))),
                    ),
                    mi(
                        "See Model Space Views",
                        Some(msg(SheetSetMsg::SeeViews(path.clone()))),
                    ),
                ],
                None,
            ));
            if open {
                for v in state.model_views.get(&key).cloned().unwrap_or_default() {
                    rows.push(plain_row(
                        2,
                        crate::ui::icons::DOC,
                        v.clone(),
                        None,
                        None,
                        vec![
                            mi("Open", Some(msg(SheetSetMsg::OpenDrawing(path.clone())))),
                            mi("", None),
                            mi(
                                "Place on Sheet",
                                Some(msg(SheetSetMsg::PlaceOnSheet(path.clone(), Some(v)))),
                            ),
                        ],
                        None,
                    ));
                }
            }
        }
    }
    rows.push(
        kit::tree_row(format!("+ {}", t!("Add New Location...")))
            .muted(true)
            .on_press(msg(SheetSetMsg::AddLocation))
            .view(),
    );
    rows
}

/// A page's heading: a section bar with its buttons.
fn heading<'a>(label: &str, buttons: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    kit::section_bar(t!(label).into_owned(), buttons)
}

pub fn view<'a>(
    state: &'a SheetSetManager,
    width: f32,
    chrome: crate::ui::dock::Chrome,
) -> Element<'a, Message> {
    let title_bar = crate::ui::dock::title_bar(
        PanelId::SheetSetManager,
        t!("Sheet Set Manager").into_owned(),
        chrome,
    );
    let mut picks: Vec<Pick> = state
        .sets
        .iter()
        .enumerate()
        .map(|(i, db)| Pick(Some(i), db.name().to_string()))
        .collect();
    picks.push(Pick(None, t!("Open...").into_owned()));
    let current = state.current.and_then(|i| picks.get(i).cloned());
    let combo = pick_list(current, picks, |p: &Pick| p.1.clone())
        .placeholder(t!("Open...").into_owned())
        .on_select(|p: Pick| msg(SheetSetMsg::PickSet(p.0)))
        .text_size(text_size::UI)
        .padding(kit::BOX_PAD)
        .style(kit::pick_list_style)
        .menu_style(kit::pick_menu_style)
        .width(Fill);
    let tabs = kit::tabs(
        vec![
            (
                t!("Sheet List").into_owned(),
                msg(SheetSetMsg::Tab(SsmTab::SheetList)),
            ),
            (
                t!("Sheet Views").into_owned(),
                msg(SheetSetMsg::Tab(SsmTab::SheetViews)),
            ),
            (
                t!("Model Views").into_owned(),
                msg(SheetSetMsg::Tab(SsmTab::ModelViews)),
            ),
        ],
        match state.tab {
            SsmTab::SheetList => 0,
            SsmTab::SheetViews => 1,
            SsmTab::ModelViews => 2,
        },
    );

    let refresh = || {
        kit::bar_button(
            crate::ui::icons::REFRESH,
            t!("Refresh"),
            msg(SheetSetMsg::Refresh),
        )
    };
    let (header, rows): (Element<'a, Message>, Vec<Element<'a, Message>>) =
        match (state.db(), state.tab) {
            (None, _) => (
                Space::new().into(),
                vec![kit::empty_state(
                    t!("Open a sheet set or create one with NEWSHEETSET.").into_owned(),
                )],
            ),
            (Some(db), SsmTab::SheetList) => {
                let set = db.sheet_set();
                let open = !state.collapsed.contains(set.id());
                let mut rows = vec![tree_row(
                    state,
                    set,
                    0,
                    SET_ICON,
                    db.name().to_string(),
                    Some(open),
                )];
                if open {
                    push_tree(state, set, 1, &mut rows);
                }
                (heading("Sheets", vec![refresh()]), rows)
            }
            (Some(db), SsmTab::SheetViews) => {
                let by = state.by_category;
                let header = heading(
                    if by {
                        "View by category"
                    } else {
                        "View by sheet"
                    },
                    vec![
                        kit::bar_button(
                            crate::ui::icons::SWAP,
                            t!(if by {
                                "View by sheet"
                            } else {
                                "View by category"
                            }),
                            msg(SheetSetMsg::ViewsByCategory(!by)),
                        ),
                        kit::bar_button(
                            crate::ui::icons::PLUS,
                            t!("New View Category..."),
                            msg(SheetSetMsg::NewCategory),
                        ),
                    ],
                );
                (header, sheet_views_rows(state, db))
            }
            (Some(_), SsmTab::ModelViews) => (
                heading(
                    "Locations",
                    vec![
                        refresh(),
                        kit::bar_button(
                            crate::ui::icons::PLUS,
                            t!("Add New Location..."),
                            msg(SheetSetMsg::AddLocation),
                        ),
                    ],
                ),
                model_views_rows(state),
            ),
        };
    let hint = kit::footer(kit::footer_text(
        t!("Double-click opens a sheet. Right-click for more commands.").into_owned(),
    ));
    kit::palette(
        title_bar,
        column![
            kit::band(combo),
            tabs,
            kit::scroll_area(column![header, iced::widget::Column::with_children(rows)]),
            hint,
        ],
        width,
    )
}

// ── dialogs ──────────────────────────────────────────────────────────────────

fn card<'a>(title: &str, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        column![
            text(t!(title).to_uppercase())
                .size(crate::ui::style::text::UI_SMALL)
                .style(accent_text),
            content.into()
        ]
        .spacing(8),
    )
    .padding([10, 12])
    .width(Fill)
    .style(card_style)
    .into()
}

fn input<'a>(value: &str, field: FieldId) -> Element<'a, Message> {
    text_input("", value)
        .size(crate::ui::style::text::UI_LARGE)
        .padding([5, 8])
        .style(field_style)
        .on_input(move |v| msg(SheetSetMsg::Input(field, v)))
        .into()
}

fn labeled<'a>(label: &str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![
        text(t!(label).into_owned())
            .size(crate::ui::style::text::UI_LARGE)
            .width(Length::Fixed(170.0)),
        control.into()
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn browse<'a>(m: SheetSetMsg) -> Element<'a, Message> {
    button(text("...").size(crate::ui::style::text::UI_LARGE))
        .on_press(msg(m))
        .style(button_style(false))
        .padding([5, 10])
        .into()
}

fn error_band<'a>(error: &Option<String>) -> Option<Element<'a, Message>> {
    error.as_ref().map(|e| {
        container(text(e.clone()).size(crate::ui::style::text::UI_LARGE))
            .padding([6, 10])
            .width(Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(crate::ui::style::chrome::mix(
                    crate::ui::style::chrome::surface(theme),
                    crate::ui::style::chrome::danger(theme),
                    0.2,
                ))),
                text_color: Some(crate::ui::style::chrome::strong(theme)),
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    })
}

fn footer<'a>(
    left: Vec<Element<'a, Message>>,
    right: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut r = row![button(text("?").size(crate::ui::style::text::UI_LARGE))
        .on_press(msg(SheetSetMsg::Help))
        .style(button_style(false))
        .padding([5, 11])]
    .spacing(6)
    .align_y(iced::Center);
    for e in left {
        r = r.push(e);
    }
    r = r.push(Space::new().width(Fill));
    for e in right {
        r = r.push(e);
    }
    r.into()
}

fn wizard_view<'a>(w: &'a Wizard) -> Element<'a, Message> {
    let steps: Vec<(u8, &str)> = if w.existing {
        vec![
            (0, "Begin"),
            (1, "Details"),
            (2, "Choose Layouts"),
            (3, "Confirm"),
        ]
    } else {
        vec![(0, "Begin"), (1, "Details"), (3, "Confirm")]
    };
    let chips =
        iced::widget::Row::with_children(steps.iter().enumerate().map(|(n, (s, label))| {
            button(text(format!("{} {}", n + 1, t!(*label))).size(crate::ui::style::text::UI_LARGE))
                .on_press(msg(SheetSetMsg::Wizard(WizardMsg::Step(*s))))
                .style(button_style(w.step == *s))
                .padding([4, 10])
                .into()
        }))
        .spacing(6);
    let page: Element<'a, Message> = match w.step {
        0 => card(
            "Create a sheet set using",
            column![
                checkbox(!w.existing)
                    .label(t!("An empty sheet set").into_owned())
                    .text_size(crate::ui::style::text::UI_LARGE)
                    .size(14)
                    .on_toggle(|_| msg(SheetSetMsg::Wizard(WizardMsg::Existing(false)))),
                checkbox(w.existing)
                    .label(t!("Existing drawings").into_owned())
                    .text_size(crate::ui::style::text::UI_LARGE)
                    .size(14)
                    .on_toggle(|_| msg(SheetSetMsg::Wizard(WizardMsg::Existing(true)))),
                text(
                    t!(if w.existing {
                        "Lets you specify one or more folders containing drawings. The layouts from these drawings can be automatically imported into the sheet set."
                    } else {
                        "Creates a sheet set with no subsets or sheets."
                    })
                    .into_owned()
                )
                .size(crate::ui::style::text::UI)
                .style(muted_style),
            ]
            .spacing(8),
        ),
        1 => card(
            "Sheet Set",
            column![
                labeled("Name of new sheet set:", input(&w.name, FieldId::Name)),
                labeled("Description (optional):", input(&w.description, FieldId::Description)),
                labeled(
                    "Store sheet set data file (.dst) here:",
                    row![input(&w.folder, FieldId::Folder), browse(SheetSetMsg::Browse(FieldId::Folder))].spacing(6)
                ),
                checkbox(w.hierarchy)
                    .label(t!("Create a folder hierarchy based on subsets").into_owned())
                    .text_size(crate::ui::style::text::UI_LARGE)
                    .size(14)
                    .on_toggle(|v| msg(SheetSetMsg::Toggle(FieldId::Hierarchy, v))),
                row![
                    Space::new().width(Fill),
                    button(text(t!("Sheet Set Properties...").into_owned()).size(crate::ui::style::text::UI_LARGE))
                        .on_press(msg(SheetSetMsg::WizardProperties))
                        .style(button_style(false))
                        .padding([5, 12]),
                ],
            ]
            .spacing(8),
        ),
        2 => {
            let rows = w.layouts.iter().enumerate().map(|(i, l)| {
                checkbox(l.on)
                    .label(format!("{} — {}", l.file, l.layout))
                    .text_size(crate::ui::style::text::UI_LARGE)
                    .size(14)
                    .on_toggle(move |v| msg(SheetSetMsg::Wizard(WizardMsg::LayoutOn(i, v))))
                    .into()
            });
            let folders = w.folders.iter().enumerate().map(|(i, f)| {
                row![
                    text(f.clone()).size(crate::ui::style::text::UI_LARGE).width(Fill),
                    button(crate::ui::icons::themed_secondary(crate::ui::icons::CLOSE, 12.0))
                        .on_press(msg(SheetSetMsg::Wizard(WizardMsg::RemoveFolder(i))))
                        .style(button::text)
                        .padding([0, 6]),
                ]
                .into()
            });
            card(
                "Choose Layouts",
                column![
                    row![
                        text(t!("Select folders containing drawings. Layouts in the drawings can be added to the sheet set.").into_owned())
                            .size(crate::ui::style::text::UI)
                            .style(muted_style)
                            .width(Fill),
                        button(text(t!("Browse...").into_owned()).size(crate::ui::style::text::UI_LARGE))
                            .on_press(msg(SheetSetMsg::Browse(FieldId::AddFolder)))
                            .style(button_style(false))
                            .padding([5, 12]),
                    ]
                    .spacing(8)
                    .align_y(iced::Center),
                    iced::widget::Column::with_children(folders.collect::<Vec<_>>()).spacing(2),
                    container(scrollable(iced::widget::Column::with_children(rows.collect::<Vec<_>>()).spacing(3)))
                        .height(Length::Fixed(180.0)),
                ]
                .spacing(8),
            )
        }
        _ => {
            let mut lines = vec![
                format!("{}: {}", t!("Sheet Set"), w.name),
                format!("{}: {}", t!("Sheet set data file"), wizard_dst_path(w)),
                format!("{}: {}", t!("Description"), w.description),
            ];
            if w.existing {
                let n = w.layouts.iter().filter(|l| l.on).count();
                lines.push(format!("{}: {n}", t!("Sheets to import")));
            }
            for (name, value, _) in ss::custom_properties(w.draft.sheet_set()) {
                lines.push(format!("{name}: {value}"));
            }
            card(
                "Confirm",
                iced::widget::Column::with_children(lines.into_iter().map(|l| text(l).size(crate::ui::style::text::UI_LARGE).into())).spacing(4),
            )
        }
    };
    let next_step = match (w.step, w.existing) {
        (0, _) => Some(1),
        (1, true) => Some(2),
        (1, false) | (2, _) => Some(3),
        _ => None,
    };
    let back_step = match (w.step, w.existing) {
        (3, true) => Some(2),
        (3, false) | (2, _) => Some(1),
        (1, _) => Some(0),
        _ => None,
    };
    let back = button(text(t!("Back").into_owned()).size(crate::ui::style::text::UI_LARGE))
        .on_press_maybe(back_step.map(|s| msg(SheetSetMsg::Wizard(WizardMsg::Step(s)))))
        .style(button_style(false))
        .padding([6, 14]);
    let next: Element<'a, Message> = match next_step {
        Some(s) => button(text(t!("Next").into_owned()).size(crate::ui::style::text::UI_LARGE))
            .on_press(msg(SheetSetMsg::Wizard(WizardMsg::Step(s))))
            .style(button_style(true))
            .padding([6, 14])
            .into(),
        None => dialog_button(t!("Finish"), msg(SheetSetMsg::Ok), true).into(),
    };
    let mut col = column![chips, page].spacing(10);
    if let Some(e) = error_band(&w.error) {
        col = col.push(e);
    }
    col.push(footer(
        vec![],
        vec![
            back.into(),
            next,
            dialog_button(t!("Cancel"), msg(SheetSetMsg::Cancel), false).into(),
        ],
    ))
    .into()
}

/// Where the wizard writes the `.dst`.
pub fn wizard_dst_path(w: &Wizard) -> String {
    ss::native_path(
        &std::path::Path::new(&w.folder)
            .join(format!("{}.dst", w.name.trim()))
            .to_string_lossy(),
    )
}

fn properties_view<'a>(p: &'a Properties) -> Element<'a, Message> {
    let mut groups: Vec<Element<'a, Message>> = Vec::new();
    let mut current: Option<&'static str> = None;
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let flush = |group: Option<&'static str>,
                 rows: &mut Vec<Element<'a, Message>>,
                 groups: &mut Vec<Element<'a, Message>>| {
        if let Some(g) = group {
            groups.push(card(
                g,
                iced::widget::Column::with_children(std::mem::take(rows)).spacing(6),
            ));
        }
    };
    for (i, r) in p.rows.iter().enumerate() {
        if current != Some(r.group) {
            flush(current, &mut rows, &mut groups);
            current = Some(r.group);
        }
        let control: Element<'a, Message> = match r.key {
            RowKey::ReadOnly => container(
                text(r.value.clone())
                    .size(crate::ui::style::text::UI_LARGE)
                    .style(muted_style),
            )
            .padding([5, 8])
            .width(Fill)
            .into(),
            RowKey::Folder(_) => row![
                input(&r.value, FieldId::Row(i)),
                browse(SheetSetMsg::Browse(FieldId::Row(i)))
            ]
            .spacing(6)
            .into(),
            RowKey::Template => row![
                container(text(r.value.clone()).size(crate::ui::style::text::UI_LARGE))
                    .padding([5, 8])
                    .width(Fill),
                browse(SheetSetMsg::BrowseTemplate)
            ]
            .spacing(6)
            .align_y(iced::Center)
            .into(),
            RowKey::PromptTemplate => {
                let yes = r.value == "Yes";
                let opts = vec![t!("Yes").into_owned(), t!("No").into_owned()];
                pick_list(
                    Some(opts[if yes { 0 } else { 1 }].clone()),
                    opts.clone(),
                    |s: &String| s.clone(),
                )
                .on_select(move |s: String| {
                    msg(SheetSetMsg::Choose(
                        FieldId::Row(i),
                        usize::from(s != t!("Yes")),
                    ))
                })
                .text_size(crate::ui::style::text::UI_LARGE)
                .padding([5, 8])
                .width(Fill)
                .into()
            }
            RowKey::Publish => yes_no(
                r.value != "Do Not Publish Sheets",
                FieldId::Row(i),
                "Publish by Sheet 'Include for Publish' Setting",
                "Do Not Publish Sheets",
            ),
            RowKey::Prop(_) => input(&r.value, FieldId::Row(i)),
        };
        rows.push(labeled(r.label, control));
    }
    flush(current, &mut rows, &mut groups);
    if p.kind != ComponentKind::Subset {
        let owner = |f: i32| {
            if f & ss::CUSTOM_SHEET_PROP != 0 {
                t!("Sheet")
            } else {
                t!("Sheet Set")
            }
        };
        let mut custom: Vec<Element<'a, Message>> = p
            .custom
            .iter()
            .enumerate()
            .map(|(i, (name, value, flags))| {
                let label = if p.kind == ComponentKind::SheetSet {
                    format!("{name} ({})", owner(*flags))
                } else {
                    name.clone()
                };
                row![
                    text(label)
                        .size(crate::ui::style::text::UI_LARGE)
                        .width(Length::Fixed(170.0)),
                    text_input("", value)
                        .size(crate::ui::style::text::UI_LARGE)
                        .padding([5, 8])
                        .style(field_style)
                        .on_input(move |v| msg(SheetSetMsg::CustomValue(i, v))),
                ]
                .spacing(8)
                .align_y(iced::Center)
                .into()
            })
            .collect();
        if p.kind == ComponentKind::SheetSet {
            match &p.adding {
                Some((name, value, sheet)) => custom.push(
                    container(
                        column![
                            labeled("Name:", input(name, FieldId::CustomName)),
                            labeled("Default value:", input(value, FieldId::CustomDefault)),
                            labeled(
                                "Owner",
                                row![
                                    checkbox(!*sheet)
                                        .label(t!("Sheet Set").into_owned())
                                        .text_size(crate::ui::style::text::UI_LARGE)
                                        .size(14)
                                        .on_toggle(|_| msg(SheetSetMsg::Choose(
                                            FieldId::CustomOwner,
                                            0
                                        ))),
                                    checkbox(*sheet)
                                        .label(t!("Sheet").into_owned())
                                        .text_size(crate::ui::style::text::UI_LARGE)
                                        .size(14)
                                        .on_toggle(|_| msg(SheetSetMsg::Choose(
                                            FieldId::CustomOwner,
                                            1
                                        ))),
                                ]
                                .spacing(12)
                            ),
                            row![
                                Space::new().width(Fill),
                                dialog_button(
                                    t!("Cancel"),
                                    msg(SheetSetMsg::AddCustomCancel),
                                    false
                                ),
                                dialog_button(t!("OK"), msg(SheetSetMsg::AddCustomOk), true),
                            ]
                            .spacing(6),
                        ]
                        .spacing(6),
                    )
                    .padding(8)
                    .style(|theme: &Theme| container::Style {
                        border: Border {
                            color: chrome::border(theme),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    })
                    .into(),
                ),
                None => custom.push(
                    row![
                        Space::new().width(Fill),
                        button(
                            text(t!("Add Custom Property...").into_owned())
                                .size(crate::ui::style::text::UI_LARGE)
                        )
                        .on_press(msg(SheetSetMsg::AddCustom))
                        .style(button_style(false))
                        .padding([5, 12]),
                    ]
                    .into(),
                ),
            }
        }
        groups.push(card(
            "Custom Properties",
            iced::widget::Column::with_children(custom).spacing(6),
        ));
    }
    let mut col =
        column![scrollable(iced::widget::Column::with_children(groups).spacing(10)).height(Fill)]
            .spacing(10);
    if let Some(e) = error_band(&p.error) {
        col = col.push(e);
    }
    col.push(footer(
        vec![],
        vec![
            dialog_button(t!("Cancel"), msg(SheetSetMsg::Cancel), false).into(),
            dialog_button(t!("OK"), msg(SheetSetMsg::Ok), true).into(),
        ],
    ))
    .into()
}

fn check<'a>(on: bool, label: &str, field: FieldId, enabled: bool) -> Element<'a, Message> {
    let c = checkbox(on)
        .label(t!(label).into_owned())
        .text_size(crate::ui::style::text::UI_LARGE)
        .size(14);
    if enabled {
        c.on_toggle(move |v| msg(SheetSetMsg::Toggle(field, v)))
            .into()
    } else {
        c.into()
    }
}

fn read_only<'a>(value: &str) -> Element<'a, Message> {
    container(
        text(value.to_string())
            .size(crate::ui::style::text::UI_LARGE)
            .style(muted_style),
    )
    .padding([5, 8])
    .width(Fill)
    .into()
}

fn ok_cancel<'a>(ok: &str) -> Element<'a, Message> {
    footer(
        vec![],
        vec![
            dialog_button(t!("Cancel"), msg(SheetSetMsg::Cancel), false).into(),
            dialog_button(t!(ok), msg(SheetSetMsg::Ok), true).into(),
        ],
    )
}

fn yes_no<'a>(yes: bool, field: FieldId, a: &str, b: &str) -> Element<'a, Message> {
    let opts = vec![t!(a).into_owned(), t!(b).into_owned()];
    let first = opts[0].clone();
    pick_list(
        Some(opts[if yes { 0 } else { 1 }].clone()),
        opts,
        |s: &String| s.clone(),
    )
    .on_select(move |s: String| msg(SheetSetMsg::Toggle(field, s == first)))
    .text_size(crate::ui::style::text::UI_LARGE)
    .padding([5, 8])
    .width(Fill)
    .into()
}

fn form_view<'a>(f: &'a Form) -> Element<'a, Message> {
    let number_title = row![
        text(t!("Number").into_owned())
            .size(crate::ui::style::text::UI_LARGE)
            .width(Length::Fixed(96.0)),
        container(input(&f.number, FieldId::Number)).width(Length::Fixed(110.0)),
        text(t!("Sheet title").into_owned()).size(crate::ui::style::text::UI_LARGE),
        input(&f.title, FieldId::Title),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let short = |label: &str, control: Element<'a, Message>| -> Element<'a, Message> {
        row![
            text(t!(label).into_owned())
                .size(crate::ui::style::text::UI_LARGE)
                .width(Length::Fixed(96.0)),
            control
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    };
    let cards: Vec<Element<'a, Message>> = match f.kind {
        FormKind::NewSheet => vec![card(
            "Sheet",
            column![
                number_title,
                short("File name", input(&f.file_name, FieldId::FileName)),
                short("Folder path", read_only(&f.folder)),
                short("Sheet template", read_only(&f.drawing)),
                row![
                    Space::new().width(Length::Fixed(104.0)),
                    check(
                        f.open_after,
                        "Open in drawing editor",
                        FieldId::OpenAfter,
                        true
                    )
                ],
            ]
            .spacing(8),
        )],
        FormKind::NewSubset => vec![card(
            "Subset",
            column![
                labeled("Subset name", input(&f.title, FieldId::Title)),
                labeled(
                    "Create folder hierarchy",
                    yes_no(f.hierarchy, FieldId::Hierarchy, "Yes", "No")
                ),
                labeled(
                    "Publish sheets in subset",
                    yes_no(
                        f.publish,
                        FieldId::Publish,
                        "Publish by Sheet 'Include for Publish' Setting",
                        "Do Not Publish Sheets"
                    )
                ),
                labeled(
                    "New sheet location",
                    if f.hierarchy {
                        read_only(&f.folder)
                    } else {
                        row![
                            input(&f.folder, FieldId::Folder),
                            browse(SheetSetMsg::Browse(FieldId::Folder))
                        ]
                        .spacing(6)
                        .into()
                    }
                ),
            ]
            .spacing(8),
        )],
        FormKind::Rename => {
            let layout: Element<'a, Message> = if f.rename[0] {
                read_only(&f.layout_name)
            } else {
                input(&f.layout_name, FieldId::LayoutName)
            };
            let file: Element<'a, Message> = if f.rename[2] {
                read_only(&f.file_name)
            } else {
                input(&f.file_name, FieldId::FileName)
            };
            vec![
                card(
                    "Sheet",
                    column![
                        number_title,
                        short("Layout name", layout),
                        short("File name", file),
                        short("Folder path", read_only(&f.folder))
                    ]
                    .spacing(8),
                ),
                card(
                    "Rename options",
                    row![
                        column![
                            text(t!("Rename layout to match:").into_owned())
                                .size(crate::ui::style::text::UI_LARGE),
                            check(f.rename[0], "Sheet title", FieldId::RenameOption(0), true),
                            row![
                                Space::new().width(22),
                                check(
                                    f.rename[1],
                                    "Prefix with sheet number",
                                    FieldId::RenameOption(1),
                                    f.rename[0]
                                )
                            ],
                        ]
                        .spacing(6)
                        .width(Fill),
                        column![
                            text(t!("Rename drawing file to match:").into_owned())
                                .size(crate::ui::style::text::UI_LARGE),
                            check(f.rename[2], "Sheet title", FieldId::RenameOption(2), true),
                            row![
                                Space::new().width(22),
                                check(
                                    f.rename[3],
                                    "Prefix with sheet number",
                                    FieldId::RenameOption(3),
                                    f.rename[2]
                                )
                            ],
                        ]
                        .spacing(6)
                        .width(Fill),
                    ]
                    .spacing(16),
                ),
            ]
        }
        FormKind::RenameView => vec![card(
            "View",
            row![
                text(t!("Number").into_owned())
                    .size(crate::ui::style::text::UI_LARGE)
                    .width(Length::Fixed(70.0)),
                container(input(&f.number, FieldId::Number)).width(Length::Fixed(90.0)),
                text(t!("View title").into_owned()).size(crate::ui::style::text::UI_LARGE),
                input(&f.title, FieldId::Title),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )],
        FormKind::ImportLayout => {
            let head = row![
                Space::new().width(22),
                text(t!("Layout name").into_owned())
                    .size(crate::ui::style::text::UI)
                    .width(Fill),
                text(t!("Drawing").into_owned())
                    .size(crate::ui::style::text::UI)
                    .width(Fill),
                text(t!("Status").into_owned())
                    .size(crate::ui::style::text::UI)
                    .width(Fill),
            ]
            .spacing(6);
            let mut list = vec![head.into()];
            for (k, r) in f.rows.iter().enumerate() {
                let file = std::path::Path::new(&r.drawing)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let (label, warn) = if r.taken {
                    (
                        "This layout is already part of a sheet set - not available for import.",
                        true,
                    )
                } else if r.warn {
                    (
                        "Warning: this layout may belong to another sheet set.",
                        true,
                    )
                } else {
                    ("Available for import", false)
                };
                let status: Element<'a, Message> = text(t!(label).into_owned())
                    .size(crate::ui::style::text::UI_LARGE)
                    .style(move |t: &Theme| text::Style {
                        color: Some(if warn {
                            t.palette().warning.base.color
                        } else {
                            t.palette().success.base.color
                        }),
                    })
                    .into();
                let enabled = !r.taken;
                let cb = checkbox(r.on).size(14);
                let cb: Element<'a, Message> = if enabled {
                    cb.on_toggle(move |v| msg(SheetSetMsg::Toggle(FieldId::ImportRow(k), v)))
                        .into()
                } else {
                    cb.into()
                };
                let name = text(r.layout.clone())
                    .size(crate::ui::style::text::UI_LARGE)
                    .width(Fill);
                let file = text(file)
                    .size(crate::ui::style::text::UI_LARGE)
                    .width(Fill);
                list.push(
                    row![
                        cb,
                        if enabled {
                            name
                        } else {
                            name.style(muted_style)
                        },
                        if enabled {
                            file
                        } else {
                            file.style(muted_style)
                        },
                        container(status).width(Fill)
                    ]
                    .spacing(6)
                    .align_y(iced::Center)
                    .into(),
                );
            }
            vec![card(
                "Drawings",
                column![
                    row![
                        text(t!("Select drawing files containing layouts").into_owned()).size(crate::ui::style::text::UI_LARGE).width(Fill),
                        button(text(t!("Browse for Drawings...").into_owned()).size(crate::ui::style::text::UI_LARGE))
                            .on_press(msg(SheetSetMsg::BrowseDrawings))
                            .style(button_style(false))
                            .padding([5, 12]),
                    ]
                    .align_y(iced::Center),
                    text(t!("A layout can belong to only one sheet set. If a layout already belongs to a sheet set, you must create a copy of the layout to import it.").into_owned())
                        .size(crate::ui::style::text::UI)
                        .style(muted_style),
                    container(scrollable(iced::widget::Column::with_children(list).spacing(4)))
                        .padding(6)
                        .height(Length::Fixed(150.0))
                        .style(card_style),
                    check(f.prefix, "Prefix sheet titles with file name", FieldId::ImportPrefix, true),
                ]
                .spacing(8),
            )]
        }
    };
    let mut col = iced::widget::Column::with_children(cards).spacing(10);
    if let Some(e) = error_band(&f.error) {
        col = col.push(e);
    }
    let foot = match f.kind {
        FormKind::Rename | FormKind::RenameView => footer(
            vec![
                button(text(t!("Previous").into_owned()).size(crate::ui::style::text::UI_LARGE))
                    .on_press(msg(SheetSetMsg::Previous))
                    .style(button_style(false))
                    .padding([5, 12])
                    .into(),
                button(text(t!("Next").into_owned()).size(crate::ui::style::text::UI_LARGE))
                    .on_press(msg(SheetSetMsg::Next))
                    .style(button_style(false))
                    .padding([5, 12])
                    .into(),
            ],
            vec![
                dialog_button(t!("Cancel"), msg(SheetSetMsg::Cancel), false).into(),
                dialog_button(t!("OK"), msg(SheetSetMsg::Ok), true).into(),
            ],
        ),
        FormKind::ImportLayout => ok_cancel("Import Checked"),
        _ => ok_cancel("OK"),
    };
    col.push(foot).into()
}

fn template_view<'a>(t: &'a TemplatePick) -> Element<'a, Message> {
    let rows = t.layouts.iter().enumerate().map(|(i, name)| {
        button(text(name.clone()).size(crate::ui::style::text::UI_LARGE))
            .on_press(msg(SheetSetMsg::Choose(FieldId::Layout, i)))
            .style(button_style(t.layout == i))
            .padding([3, 8])
            .width(Fill)
            .into()
    });
    column![
        card(
            "Template",
            column![
                text(t!("Drawing template file name").into_owned())
                    .size(crate::ui::style::text::UI_LARGE),
                row![read_only(&t.file), browse(SheetSetMsg::BrowseTemplate)]
                    .spacing(6)
                    .align_y(iced::Center),
                text(t!("Select a layout to create new sheets").into_owned())
                    .size(crate::ui::style::text::UI_LARGE),
                container(scrollable(
                    iced::widget::Column::with_children(rows.collect::<Vec<_>>()).spacing(1)
                ))
                .padding(4)
                .height(Length::Fixed(130.0))
                .style(card_style),
            ]
            .spacing(8)
        ),
        ok_cancel("OK"),
    ]
    .spacing(10)
    .into()
}

fn category_view<'a>(c: &'a Category) -> Element<'a, Message> {
    let blocks = c
        .blocks
        .iter()
        .enumerate()
        .map(|(i, (_, label, on))| check(*on, label, FieldId::CategoryBlock(i), true));
    let mut col = column![card(
        "Category",
        column![
            labeled("Category name", input(&c.name, FieldId::Name)),
            text(t!("Select the callout blocks to be used in this category").into_owned()).size(crate::ui::style::text::UI_LARGE),
            container(scrollable(iced::widget::Column::with_children(blocks.collect::<Vec<_>>()).spacing(4)))
                .padding(6)
                .height(Length::Fixed(90.0))
                .width(Fill)
                .style(card_style),
            text(t!("Selected blocks will be available for selection when you insert a callout block from the view tab.").into_owned())
                .size(crate::ui::style::text::UI)
                .style(muted_style),
            button(text(t!("Add Blocks...").into_owned()).size(crate::ui::style::text::UI_LARGE))
                .on_press(msg(SheetSetMsg::AddBlocks))
                .style(button_style(false))
                .padding([5, 12]),
        ]
        .spacing(8)
    )]
    .spacing(10);
    if let Some(e) = error_band(&c.error) {
        col = col.push(e);
    }
    col.push(ok_cancel("OK")).into()
}

/// A block preview (the Blocks palette's), or the empty preview box.
fn preview_box<'a>(
    previews: &'a std::collections::HashMap<
        String,
        Vec<crate::scene::model::wire_model::WireModel>,
    >,
    key: Option<&String>,
    height: f32,
) -> Element<'a, Message> {
    let inner: Element<'a, Message> = match key.and_then(|k| previews.get(k)) {
        Some(w) if !w.is_empty() => {
            crate::ui::window::block_palette::preview_canvas(w, height - 8.0)
        }
        _ => text(t!("preview").into_owned())
            .size(crate::ui::style::text::UI)
            .style(muted_style)
            .into(),
    };
    container(inner)
        .center_x(Fill)
        .center_y(Length::Fixed(height))
        .padding(4)
        .style(card_style)
        .into()
}

fn block_list_view<'a>(b: &'a BlockList, state: &'a SheetSetManager) -> Element<'a, Message> {
    let rows = b.blocks.iter().enumerate().map(|(i, (_, label))| {
        button(text(label.clone()).size(crate::ui::style::text::UI_LARGE))
            .on_press(msg(SheetSetMsg::BlockListSelect(i)))
            .style(button_style(b.selected == Some(i)))
            .padding([3, 8])
            .width(Fill)
            .into()
    });
    let side = column![
        button(
            text(t!("Add...").into_owned())
                .size(crate::ui::style::text::UI_LARGE)
                .center()
                .width(Fill)
        )
        .on_press(msg(SheetSetMsg::BlockListAdd))
        .style(button_style(false))
        .padding([5, 12])
        .width(Fill),
        button(
            text(t!("Delete").into_owned())
                .size(crate::ui::style::text::UI_LARGE)
                .center()
                .width(Fill)
        )
        .on_press_maybe(b.selected.map(|_| msg(SheetSetMsg::BlockListDelete)))
        .style(button_style(false))
        .padding([5, 12])
        .width(Fill),
        preview_box(&state.previews, b.preview.as_ref(), 84.0),
    ]
    .spacing(6)
    .width(Length::Fixed(130.0));
    column![
        card(
            "Blocks",
            column![
                text(t!("The following blocks are associated with this sheet set:").into_owned())
                    .size(crate::ui::style::text::UI_LARGE),
                row![
                    container(scrollable(
                        iced::widget::Column::with_children(rows.collect::<Vec<_>>()).spacing(1)
                    ))
                    .padding(4)
                    .height(Length::Fixed(150.0))
                    .width(Fill)
                    .style(card_style),
                    side,
                ]
                .spacing(10)
                .height(Length::Fixed(150.0)),
            ]
            .spacing(8)
        ),
        ok_cancel("OK"),
    ]
    .spacing(10)
    .into()
}

fn select_block_view<'a>(b: &'a SelectBlock, state: &'a SheetSetManager) -> Element<'a, Message> {
    let names = b.names.iter().enumerate().map(|(i, (name, _, on))| {
        let c = checkbox(*on)
            .label(name.clone())
            .text_size(crate::ui::style::text::UI_LARGE)
            .size(14);
        if b.whole {
            c.into()
        } else {
            c.on_toggle(move |v| msg(SheetSetMsg::SelectBlockCheck(i, v)))
                .into()
        }
    });
    let radio = |on: bool, label: &str, whole: bool| {
        checkbox(on)
            .label(t!(label).into_owned())
            .text_size(crate::ui::style::text::UI_LARGE)
            .size(14)
            .on_toggle(move |_| msg(SheetSetMsg::SelectBlockWhole(whole)))
    };
    let mut col = column![card(
        "Drawing",
        column![
            text(t!("Enter the drawing file name:").into_owned())
                .size(crate::ui::style::text::UI_LARGE),
            row![read_only(&b.file), browse(SheetSetMsg::SelectBlockBrowse)]
                .spacing(6)
                .align_y(iced::Center),
            radio(b.whole, "Select the drawing file as a block", true),
            radio(!b.whole, "Choose blocks in the drawing file:", false),
            row![
                Space::new().width(22),
                container(scrollable(
                    iced::widget::Column::with_children(names.collect::<Vec<_>>()).spacing(4)
                ))
                .padding(6)
                .height(Length::Fixed(110.0))
                .width(Fill)
                .style(card_style),
                container(preview_box(&state.previews, b.preview.as_ref(), 110.0))
                    .width(Length::Fixed(130.0)),
            ]
            .spacing(10),
        ]
        .spacing(8)
    )]
    .spacing(10);
    if let Some(e) = error_band(&b.error) {
        col = col.push(e);
    }
    col.push(ok_cancel("OK")).into()
}

fn confirm_view<'a>(question: &'a str) -> Element<'a, Message> {
    column![
        row![
            text("?").size(22).style(accent_text),
            column![
                text(question.to_string()).size(crate::ui::style::text::UI_LARGE),
                text(t!("(The layout and the drawing file will not be deleted.)").into_owned())
                    .size(crate::ui::style::text::UI_LARGE)
                    .style(muted_style),
            ]
            .spacing(4),
        ]
        .spacing(14),
        row![
            Space::new().width(Fill),
            dialog_button(t!("Cancel"), msg(SheetSetMsg::Cancel), false),
            dialog_button(t!("Remove"), msg(SheetSetMsg::Ok), true),
        ]
        .spacing(6),
    ]
    .spacing(14)
    .into()
}

pub fn dialog_view<'a>(
    state: &'a SheetSetManager,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let body = match state.dialog.as_ref() {
        Some(SsDialog::Wizard(w)) => wizard_view(w),
        Some(SsDialog::Properties(p)) => properties_view(p),
        Some(SsDialog::Form(f)) => form_view(f),
        Some(SsDialog::Confirm(_, _, q)) => confirm_view(q),
        Some(SsDialog::Template(t)) => template_view(t),
        Some(SsDialog::Category(c)) => category_view(c),
        Some(SsDialog::BlockList(b)) => block_list_view(b, state),
        Some(SsDialog::SelectBlock(b)) => select_block_view(b, state),
        None => Space::new().into(),
    };
    container(body).padding([10, 12]).width(sizing.width).into()
}
