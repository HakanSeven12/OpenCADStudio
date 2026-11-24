//! The civil Create Points menu: a panel at the canvas's right edge, opened
//! from the ribbon's Create Points button. Its point creation groups fold
//! open one at a time; under them sit the other point tools. Each entry runs
//! its command through [`Message::RibbonToolClick`], like a ribbon button.

use iced::widget::{button, column, container, row, scrollable, text, tooltip, Space};
use iced::{Background, Border, Element, Length, Theme};

use crate::app::Message;
use crate::modules::ModuleEvent;

/// What the ribbon's Create Points button runs: it opens or closes this menu.
pub const OPEN_COMMAND: &str = "_CIVILPOINTSMENU";

macro_rules! icon {
    ($name:literal) => {
        include_bytes!(concat!("../../assets/icons/civil/points/", $name, ".svg"))
    };
}

/// One menu entry: its label, icon and the command it runs.
pub struct Entry {
    pub label: &'static str,
    pub icon: &'static [u8],
    pub command: &'static str,
}

/// A rule between entries of a group.
const RULE: Option<Entry> = None;

const fn e(label: &'static str, icon: &'static [u8], command: &'static str) -> Option<Entry> {
    Some(Entry { label, icon, command })
}

/// A foldable group of point creation commands.
pub struct Group {
    pub label: &'static str,
    pub icon: &'static [u8],
    pub entries: &'static [Option<Entry>],
}

pub const GROUPS: &[Group] = &[
    Group {
        label: "Create Points - Miscellaneous",
        icon: icon!("misc"),
        entries: &[
            e("Manual", icon!("manual"), "CREATEPOINTMANUAL"),
            e("Northing/Easting", icon!("ne"), "CREATEPOINTNORTHINGEASTING"),
            e("Bearing", icon!("bearing"), "CREATEPOINTBEARING"),
            e("Azimuth", icon!("azimuth"), "CREATEPOINTAZIMUTH"),
            e("Angle", icon!("angle"), "CREATEPOINTANGLE"),
            e("Deflection", icon!("deflect"), "CREATEPOINTDEFLECTION"),
            e("Geodetic Direct", icon!("geodir"), "CREATEPTGEODETICDIR"),
            e("Resection", icon!("resect"), "CREATEPOINTRESECTION"),
            e("Station/Offset Object", icon!("staoff"), "CREATEPTSTATIONOFFSETOBJ"),
            RULE,
            e("Automatic", icon!("auto"), "CREATEPOINTAUTOMATIC"),
            e("Along Line/Curve", icon!("along"), "CREATEPTALONGLNCURVESPIR"),
            e("On Line/Curve", icon!("online"), "CREATEPTONLINECURVESPIRAL"),
            e("Divide Object", icon!("divide"), "CREATEPOINTDIVIDEOBJECT"),
            e("Measure Object", icon!("measure"), "CREATEPOINTMEASUREOBJECT"),
            RULE,
            e("Polyline Vertices - Manual", icon!("plvman"), "CREATEPTPLYLINECTRVERTMAN"),
            e("Polyline Vertices - Automatic", icon!("plvauto"), "CREATEPTPLYLNCTRVERTAUTO"),
        ],
    },
    Group {
        label: "Create Points - Intersections",
        icon: icon!("inter"),
        entries: &[
            e("Direction/Direction", icon!("dirdir"), "CREATEPOINTDIRECTIONDIR"),
            e("Distance/Distance", icon!("distdist"), "CREATEPOINTDISTANCEDIST"),
            e("Direction/Distance", icon!("dirdist"), "CREATEPOINTDIRECTIONDIST"),
            e("Direction/Perpendicular", icon!("dirperp"), "CREATEPOINTDIRECTIONPERP"),
            e("Distance/Perpendicular", icon!("distperp"), "CREATEPOINTDISTANCEPERP"),
            RULE,
            e("Direction/Object", icon!("dirobj"), "CREATEPOINTDIRECTIONOBJ"),
            e("Distance/Object", icon!("distobj"), "CREATEPOINTDISTANCEOBJECT"),
            e("Object/Object", icon!("objobj"), "CREATEPOINTOBJECTOBJECT"),
            e("Perpendicular", icon!("perp"), "CREATEPTINTERSECTPERP"),
            RULE,
            e("Direction/Alignment", icon!("diralign"), "CREATEPOINTDIRECTIONALIGN"),
            e("Distance/Alignment", icon!("distalign"), "CREATEPOINTDISTANCEALIGN"),
            e("Object/Alignment", icon!("objalign"), "CREATEPOINTOBJECTALIGN"),
            e("Alignment/Alignment", icon!("alal"), "CREATEPOINTALIGNALIGN"),
        ],
    },
    Group {
        label: "Create Points - Alignments",
        icon: icon!("align"),
        entries: &[
            e("Station/Offset", icon!("staoff2"), "CREATEPOINTSTATIONOFFSET"),
            e("Divide Alignment", icon!("divalign"), "CREATEPOINTDIVIDEALIGN"),
            e("Measure Alignment", icon!("measalign"), "CREATEPOINTMEASUREALIGN"),
            e("At Geometry Points", icon!("geompts"), "CREATEPOINTATGEOMETRYPOINTS"),
            e("Radial or Perpendicular", icon!("radial"), "CREATEPOINTRADIALORPERP"),
            e("Import from File", icon!("impfile"), "CREATEPOINTIMPORTFROMFILE"),
            e("Profile Geometry Points", icon!("profgeom"), "CREATEPROFILEGEOMPOINTS"),
        ],
    },
    Group {
        label: "Create Points - Surface",
        icon: icon!("surf"),
        entries: &[
            e("Random Points", icon!("random"), "CREATEPOINTRANDOMPOINTS"),
            e("On Grid", icon!("grid"), "CREATEPOINTONGRID"),
            RULE,
            e("Along Polyline/Contour", icon!("alongpc"), "CREATEPTALONGPOLYCONTOUR"),
            e("Polyline/Contour Vertices", icon!("pcvert"), "CREATEPTPOLYCONTOURVERT"),
        ],
    },
    Group {
        label: "Create Points - Slope",
        icon: icon!("slope"),
        entries: &[
            e("High/Low Point", icon!("highlow"), "CREATEPOINTHIGHLOWPOINT"),
            e("Slope/Grade - Distance", icon!("slopedist"), "CREATEPOINTSLOPEGRADEDIST"),
            e("Slope/Grade - Elevation", icon!("slopeelev"), "CREATEPOINTSLOPEGRADEELEV"),
        ],
    },
    Group {
        label: "Create Points - Interpolate",
        icon: icon!("interp"),
        entries: &[
            e("Interpolate", icon!("interp1"), "CREATEPOINTINTERPOLATE"),
            RULE,
            e("By Relative Location", icon!("rel_loc"), "CREATEPOINTBYRELLOCATION"),
            e("By Relative Elevation", icon!("rel_elev"), "CREATEPOINTBYRELELEV"),
            e("Number by Distance", icon!("numdist"), "CREATEPOINTNUMBYDIST"),
            e("Perpendicular", icon!("iperp"), "CREATEPTINTERPPERPENDIC"),
            e("Incremental Distance", icon!("incdist"), "CREATEPTINCREMENTALDIST"),
            e("Incremental Elevation", icon!("incelev"), "CREATEPTINCRMNTLELEV"),
            e("Intersection", icon!("iinter"), "CREATEPTINTERPOLINTERSEC"),
        ],
    },
];

/// The point tools under the groups.
pub const TOOLS: &[Entry] = &[
    Entry { label: "Create Point Group", icon: icon!("group"), command: "CREATEPOINTGROUP" },
    Entry { label: "Create COGO Points from Corridor", icon: icon!("corr"), command: "CREATEPOINTSFROMCORRIDOR" },
    Entry { label: "Create Blocks from COGO Points", icon: icon!("blocks"), command: "CREATEBLOCKSFROMPOINTS" },
    Entry { label: "Convert CAD Points", icon: icon!("convert"), command: "CONVERTCADPOINTS" },
    Entry { label: "Replace Legacy Point Blocks", icon: icon!("replace"), command: "REPLACELEGACYPOINTBLOCKS" },
];

/// Whether `cmd` is a menu entry whose command is not written yet.
// ponytail: entries answer "not yet implemented" until their command lands;
// drop each from here as it does.
pub fn is_pending(cmd: &str) -> bool {
    !matches!(cmd, "CREATEPOINTMANUAL" | "CREATEPOINTGROUP")
        && (GROUPS.iter().flat_map(|g| g.entries.iter().flatten()).any(|e| e.command == cmd)
            || TOOLS.iter().any(|e| e.command == cmd))
}

/// Whether the menu is open and which group is folded open.
#[derive(Debug, Clone)]
pub struct PointsMenu {
    pub open: bool,
    pub expanded: Option<usize>,
}

impl Default for PointsMenu {
    fn default() -> Self {
        Self { open: false, expanded: Some(0) }
    }
}

#[derive(Debug, Clone)]
pub enum PointsMenuMsg {
    Close,
    /// Fold group `i` open, or shut it when it already is.
    Expand(usize),
}

impl PointsMenu {
    pub fn update(&mut self, msg: PointsMenuMsg) {
        match msg {
            PointsMenuMsg::Close => self.open = false,
            PointsMenuMsg::Expand(i) => {
                self.expanded = (self.expanded != Some(i)).then_some(i);
            }
        }
    }
}

const WIDTH: f32 = 300.0;
const EDGE_MARGIN: f32 = 8.0;

fn run(cmd: &'static str) -> Message {
    Message::RibbonToolClick { tool_id: cmd.to_string(), event: ModuleEvent::Command(cmd.to_string()) }
}

fn label(s: &'static str) -> String {
    crate::t!(s).to_string()
}

fn bold() -> iced::Font {
    iced::Font { weight: iced::font::Weight::Semibold, ..iced::Font::DEFAULT }
}

fn rule<'a>() -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.neutral.color)),
            ..Default::default()
        })
        .into()
}

/// A flat row button: highlighted on hover, `on` filled.
fn row_style(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = theme.palette();
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: if hovered {
                Some(Background::Color(p.background.strong.color))
            } else if on {
                Some(Background::Color(p.background.base.color))
            } else {
                None
            },
            border: Border { radius: 3.0.into(), ..Default::default() },
            text_color: p.background.base.text,
            ..Default::default()
        }
    }
}

fn entry_button(entry: &'static Entry) -> Element<'static, Message> {
    button(
        row![crate::ui::icons::semantic(entry.icon, 16.0), text(label(entry.label)).size(12)]
            .spacing(8)
            .align_y(iced::Center),
    )
    .padding([3, 10])
    .width(Length::Fill)
    .on_press(run(entry.command))
    .style(row_style(false))
    .into()
}

fn group_view(i: usize, group: &'static Group, open: bool) -> Element<'static, Message> {
    let title = text(label(group.label)).size(12);
    let head = button(
        row![
            crate::ui::icons::semantic(group.icon, 18.0),
            if open { title.font(bold()) } else { title },
            Space::new().width(Length::Fill),
            crate::ui::icons::themed_secondary(if open { crate::ui::icons::TRI_DOWN } else { crate::ui::icons::TRI_RIGHT }, 10.0),
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .padding([5, 12])
    .width(Length::Fill)
    .on_press(Message::PointsMenu(PointsMenuMsg::Expand(i)))
    .style(row_style(open));
    if !open {
        return head.into();
    }
    let mut items = column![].spacing(1).padding([2, 0]);
    for entry in group.entries {
        items = match entry {
            Some(entry) => items.push(entry_button(entry)),
            None => items.push(container(rule()).padding([3, 8])),
        };
    }
    // An accent bar down the left marks the folded-open group.
    let accent = container(Space::new())
        .width(Length::Fixed(2.0))
        .height(Length::Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().primary.base.color)),
            ..Default::default()
        });
    let body = container(row![accent, container(items).padding(iced::Padding { left: 20.0, ..iced::Padding::ZERO })].height(Length::Shrink))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weakest.color)),
            ..Default::default()
        });
    column![head, body].into()
}

fn tool_button(entry: &'static Entry) -> Element<'static, Message> {
    let btn = button(
        column![crate::ui::icons::semantic(entry.icon, 20.0), text(label(entry.label)).size(9).center()]
            .spacing(3)
            .align_x(iced::Center),
    )
    .padding([5, 2])
    .width(Length::FillPortion(1))
    .on_press(run(entry.command))
    .style(row_style(false));
    tooltip(btn, container(text(label(entry.label)).size(11)).padding([2, 6]).style(container::rounded_box), tooltip::Position::Top)
        .into()
}

/// The menu, pinned to the canvas's right edge, its foot `bottom` above the
/// canvas's (clear of the command line).
pub fn view(state: &PointsMenu, bottom: f32) -> Element<'static, Message> {
    let close = button(crate::ui::icons::themed_secondary(crate::ui::icons::CLOSE, 12.0))
        .padding([1, 6])
        .on_press(Message::PointsMenu(PointsMenuMsg::Close))
        .style(row_style(false));
    let header = container(
        row![
            crate::ui::icons::semantic(icon!("misc"), 18.0),
            text(label("Create Points")).size(12).font(bold()),
            Space::new().width(Length::Fill),
            close,
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .padding([6, 8])
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.strong.color)),
        ..Default::default()
    });

    let mut groups = column![].padding([4, 0]);
    for (i, group) in GROUPS.iter().enumerate() {
        groups = groups.push(group_view(i, group, state.expanded == Some(i)));
    }

    let mut tools = row![].spacing(2);
    for entry in TOOLS {
        tools = tools.push(tool_button(entry));
    }
    let footer = container(tools).padding(6).width(Length::Fill).style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.strong.color)),
        ..Default::default()
    });

    let card = container(column![header, rule(), scrollable(groups).height(Length::Fill), rule(), footer])
        .width(Length::Fixed(WIDTH))
        .height(Length::Fill)
        .style(|theme: &Theme| {
            let p = theme.palette();
            container::Style {
                background: Some(Background::Color(p.background.weak.color)),
                border: Border { color: p.background.neutral.color, width: 1.0, radius: 5.0.into() },
                ..Default::default()
            }
        });

    container(iced::widget::opaque(card))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::Right)
        .padding(iced::Padding { bottom: bottom.max(EDGE_MARGIN), ..iced::Padding::new(EDGE_MARGIN) })
        .into()
}
