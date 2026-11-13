//! The Assistant pallet (ASSISTANT): a chat panel for an AI agent that drives
//! this OpenCAD Studio window.
//!
//! The panel is only the face. The agent is an external service on the same
//! machine (typically an LLM client using the `--mcp` endpoint), reached over
//! a loopback socket that carries one JSON object per line each way; the
//! protocol is described in `docs/assistant-panel.md`. When nothing listens
//! yet and `OCS_ASSISTANT_COMMAND` is set, the panel starts the service
//! itself, so opening the pallet is all the user does. No model, key or
//! vendor is built into the app.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use base64::Engine as _;
use iced::widget::{
    button, column, container, image, markdown, mouse_area, row, scrollable, text, text_input,
    Space,
};
use iced::{Background, Border, Color, Element, Fill, Font, Length, Task, Theme};

use crate::app::Message;
use crate::ui::dock::{Chrome, PanelId};

pub const ICON: &[u8] = include_bytes!("../../../assets/icons/assistant.svg");
pub const INPUT_ID: &str = "assistant-input";
const LOG_ID: &str = "assistant-log";

/// Where the service listens (`OCS_ASSISTANT_PORT` overrides).
const DEFAULT_PORT: u16 = 8766;
/// How long to keep trying while a service the panel started comes up
/// (it connects to OCS and to its model provider first).
const CONNECT_PATIENCE: Duration = Duration::from_secs(40);

/// Quick requests offered as chips under the conversation.
const CHIPS: [(&str, &str); 6] = [
    ("What's open?", "What drawing is open, and what's in it? Keep it short."),
    ("Selection?", "What is selected right now?"),
    ("Zoom extents", "Zoom to the whole drawing."),
    ("Show view", "Show me the current view."),
    ("Undo", "Undo the last change."),
    ("Save", "Save the drawing."),
];

#[derive(Debug, Clone)]
pub enum AssistantMsg {
    /// Timer: drain what the service sent.
    Poll,
    Input(String),
    Send,
    Chip(usize),
    Stop,
    NewChat,
    ToggleCard(usize),
    Reconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardState {
    Pending,
    Running,
    Ok,
    Failed,
}

struct Card {
    id: String,
    title: String,
    name: String,
    args: String,
    result: String,
    state: CardState,
    ms: u64,
    open: bool,
    images: Vec<image::Handle>,
}

enum Item {
    User(String),
    Bot { source: String, content: markdown::Content },
    Note(String),
    Card(Card),
    Error(String),
    Stopped,
}

#[derive(Default)]
struct Status {
    ocs: bool,
    drawing: String,
    path: String,
    model: String,
    effort: String,
    tools: u64,
    api_key: bool,
    cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
enum Link {
    /// Not started (the pallet has not been opened yet).
    Idle,
    /// Connecting, possibly after starting the service.
    Connecting(String),
    Connected,
    /// Gave up; the message says why.
    Down(String),
}

enum Incoming {
    Connected,
    Event(serde_json::Value),
    Note(String),
    Closed(String),
}

struct Conn {
    rx: Receiver<Incoming>,
    tx: Option<Sender<String>>,
}

pub struct AssistantPalette {
    pub show: bool,
    input: String,
    items: Vec<Item>,
    running: bool,
    status: Status,
    link: Link,
    conn: Option<Conn>,
    last_refresh: Option<Instant>,
}

impl Default for AssistantPalette {
    fn default() -> Self {
        Self {
            show: false,
            input: String::new(),
            items: Vec::new(),
            running: false,
            status: Status::default(),
            link: Link::Idle,
            conn: None,
            last_refresh: None,
        }
    }
}

impl AssistantPalette {
    /// How often the pallet wants a [`AssistantMsg::Poll`], if at all.
    pub fn poll_interval(&self) -> Option<Duration> {
        if !self.show && !self.running {
            return None;
        }
        Some(match self.link {
            Link::Idle | Link::Down(_) => return None,
            Link::Connecting(_) => Duration::from_millis(250),
            Link::Connected if self.running => Duration::from_millis(60),
            Link::Connected => Duration::from_millis(500),
        })
    }

    /// The pallet was shown: make sure there is a connection.
    pub fn opened(&mut self) {
        self.show = true;
        if self.conn.is_none() {
            self.connect();
        }
    }

    fn connect(&mut self) {
        self.link = Link::Connecting("Starting the assistant…".into());
        self.conn = Some(start_connection());
    }

    fn send(&mut self, value: serde_json::Value) -> bool {
        if let Some(tx) = self.conn.as_ref().and_then(|c| c.tx.as_ref()) {
            return tx.send(value.to_string()).is_ok();
        }
        false
    }

    pub fn update(&mut self, msg: AssistantMsg) -> Task<Message> {
        match msg {
            AssistantMsg::Poll => return self.drain(),
            AssistantMsg::Input(v) => self.input = v,
            AssistantMsg::Send => return self.submit(None),
            AssistantMsg::Chip(i) => {
                if let Some((_, say)) = CHIPS.get(i) {
                    return self.submit(Some(say.to_string()));
                }
            }
            AssistantMsg::Stop => {
                self.send(serde_json::json!({"type": "stop"}));
            }
            AssistantMsg::NewChat => {
                if !self.running {
                    self.send(serde_json::json!({"type": "reset"}));
                    self.items.clear();
                }
            }
            AssistantMsg::ToggleCard(i) => {
                if let Some(Item::Card(card)) = self.items.get_mut(i) {
                    card.open = !card.open;
                }
            }
            AssistantMsg::Reconnect => {
                self.conn = None;
                self.connect();
            }
        }
        Task::none()
    }

    fn submit(&mut self, text: Option<String>) -> Task<Message> {
        let text = text.unwrap_or_else(|| self.input.trim().to_string());
        if text.is_empty() || self.running {
            return Task::none();
        }
        if self.link != Link::Connected {
            self.items.push(Item::Error(
                "The assistant isn't connected yet. Wait a moment or press Reconnect.".into(),
            ));
            return scroll_to_end();
        }
        self.items.push(Item::User(text.clone()));
        self.input.clear();
        self.running = true;
        self.send(serde_json::json!({"type": "chat", "text": text}));
        scroll_to_end()
    }

    /// Apply everything the connection thread queued.
    fn drain(&mut self) -> Task<Message> {
        let mut events = Vec::new();
        let mut closed = None;
        if let Some(conn) = &self.conn {
            loop {
                match conn.rx.try_recv() {
                    Ok(Incoming::Connected) => self.link = Link::Connected,
                    Ok(Incoming::Note(n)) => self.link = Link::Connecting(n),
                    Ok(Incoming::Event(ev)) => events.push(ev),
                    Ok(Incoming::Closed(why)) => {
                        closed = Some(why);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        closed = Some("The assistant service stopped.".into());
                        break;
                    }
                }
            }
        }
        let grew = !events.is_empty();
        for ev in events {
            self.apply(ev);
        }
        if let Some(why) = closed {
            self.conn = None;
            if self.running {
                self.items.push(Item::Error(why.clone()));
                self.running = false;
            }
            self.status.ocs = false;
            self.link = Link::Down(why);
        }
        // Keep the drawing name and cost fresh while idle.
        if self.link == Link::Connected && !self.running {
            let due = self
                .last_refresh
                .is_none_or(|t| t.elapsed() > Duration::from_secs(10));
            if due {
                self.last_refresh = Some(Instant::now());
                self.send(serde_json::json!({"type": "refresh"}));
            }
        }
        if grew {
            scroll_to_end()
        } else {
            Task::none()
        }
    }

    fn apply(&mut self, ev: serde_json::Value) {
        let s = |k: &str| ev.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        match ev.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "status" => {
                self.status = Status {
                    ocs: ev.get("ocs").and_then(|v| v.as_bool()).unwrap_or(false),
                    drawing: s("drawing"),
                    path: s("path"),
                    model: s("model"),
                    effort: s("effort"),
                    tools: ev.get("tools").and_then(|v| v.as_u64()).unwrap_or(0),
                    api_key: ev.get("api_key").and_then(|v| v.as_bool()).unwrap_or(false),
                    cost: ev.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0),
                };
                if let Some(r) = ev.get("running").and_then(|v| v.as_bool()) {
                    self.running = r;
                }
            }
            "text_start" => self.push_text(""),
            "text" => self.push_text(&s("text")),
            "progress" => {
                let t = s("text");
                match self.items.last_mut() {
                    Some(Item::Note(n)) => n.push_str(&t),
                    _ => self.items.push(Item::Note(t)),
                }
            }
            "tool_pending" => self.items.push(Item::Card(Card {
                id: s("id"),
                title: s("name"),
                name: s("name"),
                args: String::new(),
                result: String::new(),
                state: CardState::Pending,
                ms: 0,
                open: false,
                images: Vec::new(),
            })),
            "tool_start" => {
                let id = s("id");
                let (title, name, args) = (s("title"), s("name"), s("args"));
                match self.card_mut(&id) {
                    Some(card) => {
                        card.title = title;
                        card.name = name;
                        card.args = args;
                        card.state = CardState::Running;
                    }
                    None => self.items.push(Item::Card(Card {
                        id,
                        title,
                        name,
                        args,
                        result: String::new(),
                        state: CardState::Running,
                        ms: 0,
                        open: false,
                        images: Vec::new(),
                    })),
                }
            }
            "tool_end" => {
                let id = s("id");
                let ok = ev.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
                let ms = ev.get("ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let preview = s("preview");
                let images: Vec<image::Handle> = ev
                    .get("images")
                    .and_then(|v| v.as_array())
                    .map(|list| list.iter().filter_map(|v| v.as_str()).filter_map(decode_image).collect())
                    .unwrap_or_default();
                if let Some(card) = self.card_mut(&id) {
                    card.state = if ok { CardState::Ok } else { CardState::Failed };
                    card.ms = ms;
                    card.result = preview;
                    card.open = !ok;
                    card.images = images;
                }
            }
            "error" => self.items.push(Item::Error(s("text"))),
            "cancelled" => self.items.push(Item::Stopped),
            "done" => {
                self.running = false;
                if let Some(c) = ev.get("cost_usd").and_then(|v| v.as_f64()) {
                    self.status.cost = c;
                }
                // Drop an empty reply bubble (a turn that only acted).
                if let Some(Item::Bot { source, .. }) = self.items.last() {
                    if source.trim().is_empty() {
                        self.items.pop();
                    }
                }
            }
            _ => {}
        }
    }

    fn push_text(&mut self, delta: &str) {
        if let Some(Item::Bot { source, content }) = self.items.last_mut() {
            if !delta.is_empty() {
                source.push_str(delta);
                *content = markdown::Content::parse(source);
            }
            return;
        }
        let source = delta.to_string();
        let content = markdown::Content::parse(&source);
        self.items.push(Item::Bot { source, content });
    }

    fn card_mut(&mut self, id: &str) -> Option<&mut Card> {
        self.items.iter_mut().rev().find_map(|item| match item {
            Item::Card(card) if card.id == id => Some(card),
            _ => None,
        })
    }
}

fn scroll_to_end() -> Task<Message> {
    iced::widget::operation::snap_to(
        iced::widget::Id::new(LOG_ID),
        scrollable::RelativeOffset::END,
    )
}

fn decode_image(data_url: &str) -> Option<image::Handle> {
    let b64 = data_url.split_once(",").map(|(_, b)| b).unwrap_or(data_url);
    base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()
        .map(image::Handle::from_bytes)
}

// ── The service connection (a thread per connection) ────────────────────

/// Split a command line into program and arguments: whitespace separates,
/// double quotes group (paths with spaces).
fn split_command(line: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    parts.push(std::mem::take(&mut current));
                    any = false;
                }
            }
            c => {
                current.push(c);
                any = true;
            }
        }
    }
    if any {
        parts.push(current);
    }
    parts
}

fn service_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("ocs-assistant-service.log")
}

fn service_port() -> u16 {
    std::env::var("OCS_ASSISTANT_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

#[cfg(target_arch = "wasm32")]
fn start_connection() -> Conn {
    let (tx_in, rx) = mpsc::channel();
    let _ = tx_in.send(Incoming::Closed(
        "The assistant needs the desktop app.".into(),
    ));
    Conn { rx, tx: None }
}

#[cfg(not(target_arch = "wasm32"))]
fn start_connection() -> Conn {
    let (tx_in, rx) = mpsc::channel::<Incoming>();
    let (tx_out, rx_out) = mpsc::channel::<String>();
    std::thread::Builder::new()
        .name("assistant-link".into())
        .spawn(move || run_link(tx_in, rx_out))
        .ok();
    Conn {
        rx,
        tx: Some(tx_out),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run_link(tx_in: Sender<Incoming>, rx_out: Receiver<String>) {
    let addr = SocketAddr::from(([127, 0, 0, 1], service_port()));
    let started = Instant::now();
    let mut launched = false;
    let stream = loop {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(400)) {
            Ok(stream) => break stream,
            Err(_) if !launched => {
                launched = true;
                match launch_service() {
                    Ok(()) => {
                        let _ = tx_in.send(Incoming::Note(
                            "Starting the assistant service…".into(),
                        ));
                    }
                    Err(why) => {
                        let _ = tx_in.send(Incoming::Closed(why));
                        return;
                    }
                }
            }
            Err(_) if started.elapsed() > CONNECT_PATIENCE => {
                let _ = tx_in.send(Incoming::Closed(format!(
                    "The assistant service didn't start. See {}.",
                    service_log_path().display()
                )));
                return;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    };
    let _ = stream.set_nodelay(true);
    let Ok(mut writer) = stream.try_clone() else {
        let _ = tx_in.send(Incoming::Closed("Couldn't open the assistant link.".into()));
        return;
    };
    let _ = tx_in.send(Incoming::Connected);
    // Writer: lines queued by the pallet. Ends when the pallet drops its sender.
    std::thread::Builder::new()
        .name("assistant-send".into())
        .spawn(move || {
            for line in rx_out {
                if writer.write_all(line.as_bytes()).is_err()
                    || writer.write_all(b"\n").is_err()
                    || writer.flush().is_err()
                {
                    break;
                }
            }
            let _ = writer.shutdown(std::net::Shutdown::Both);
        })
        .ok();
    // Reader: one JSON event per line.
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
            if tx_in.send(Incoming::Event(value)).is_err() {
                return; // the pallet went away
            }
        }
    }
    let _ = tx_in.send(Incoming::Closed(
        "The assistant service closed the connection.".into(),
    ));
}

/// Start the service named by `OCS_ASSISTANT_COMMAND` (in `OCS_ASSISTANT_DIR`
/// when set), hidden, with its output in the temp folder's
/// `ocs-assistant-service.log`. The service is told this executable's path in
/// `OCS_EXE` so its `--mcp` bridge attaches to this very build.
#[cfg(not(target_arch = "wasm32"))]
fn launch_service() -> Result<(), String> {
    let line = std::env::var("OCS_ASSISTANT_COMMAND").unwrap_or_default();
    let mut parts = split_command(&line).into_iter();
    let Some(program) = parts.next() else {
        return Err(format!(
            "No assistant service is listening on port {}. Start one, or set \
             OCS_ASSISTANT_COMMAND so OpenCAD Studio can start it, then press Reconnect.",
            service_port()
        ));
    };
    let log = std::fs::File::create(service_log_path())
        .map_err(|e| format!("Can't write the assistant log: {e}"))?;
    let log_err = log
        .try_clone()
        .map_err(|e| format!("Can't write the assistant log: {e}"))?;
    let mut cmd = std::process::Command::new(program);
    cmd.args(parts)
        .env("OCS_ASSISTANT_PORT", service_port().to_string())
        .stdin(std::process::Stdio::null())
        .stdout(log)
        .stderr(log_err);
    if let Some(dir) = std::env::var_os("OCS_ASSISTANT_DIR") {
        cmd.current_dir(dir);
    }
    if let Ok(exe) = std::env::current_exe() {
        cmd.env("OCS_EXE", exe);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't start the assistant service: {e}"))
}

// ── View ────────────────────────────────────────────────────────────────

const STRONG: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..Font::DEFAULT
};

fn m(msg: AssistantMsg) -> Message {
    Message::Assistant(msg)
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn muted(theme: &Theme) -> Color {
    let p = theme.palette();
    mix(p.background.base.color, p.background.base.text, 0.62)
}

fn muted_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(muted(theme)),
    }
}

fn bubble_style(theme: &Theme) -> container::Style {
    let p = theme.palette();
    container::Style {
        background: Some(Background::Color(p.primary.base.color.scale_alpha(0.18))),
        border: Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        text_color: Some(p.background.base.text),
        ..Default::default()
    }
}

fn card_style(state: CardState) -> impl Fn(&Theme) -> container::Style {
    move |theme: &Theme| {
        let p = theme.palette();
        let edge = match state {
            CardState::Ok => p.success.base.color,
            CardState::Failed => p.danger.base.color,
            _ => p.primary.base.color,
        };
        container::Style {
            background: Some(Background::Color(p.background.weak.color)),
            border: Border {
                color: edge.scale_alpha(0.55),
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        }
    }
}

fn chip_style(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.palette();
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if hovered {
            p.background.strong.color
        } else {
            p.background.weak.color
        })),
        border: Border {
            color: p.background.neutral.color,
            width: 1.0,
            radius: 10.0.into(),
        },
        text_color: p.background.base.text,
        ..Default::default()
    }
}

fn small_button<'a>(label: &'a str, msg: Message) -> Element<'a, Message> {
    button(text(label).size(11))
        .padding([2, 8])
        .style(button::subtle)
        .on_press(msg)
        .into()
}

pub fn view<'a>(p: &'a AssistantPalette, theme: &'a Theme, width: f32, chrome: Chrome) -> Element<'a, Message> {
    let title_bar = crate::ui::dock::title_bar(PanelId::Assistant, "Assistant".to_string(), chrome);
    crate::ui::dock::frame(title_bar, body(p, theme), width)
}

fn body<'a>(p: &'a AssistantPalette, theme: &'a Theme) -> Element<'a, Message> {
    // Header: connection dot, drawing, model and cost, New chat.
    let (dot_ok, line) = match &p.link {
        Link::Connected if p.status.ocs => (
            true,
            if p.status.drawing.is_empty() {
                "Connected to OpenCAD Studio".to_string()
            } else {
                p.status.drawing.clone()
            },
        ),
        Link::Connected => (false, "Service running, OpenCAD Studio not reached".to_string()),
        Link::Connecting(n) => (false, n.clone()),
        Link::Down(why) => (false, why.clone()),
        Link::Idle => (false, "Not started".to_string()),
    };
    let dot = container(Space::new().width(8).height(8)).style(move |theme: &Theme| {
        let pal = theme.palette();
        container::Style {
            background: Some(Background::Color(if dot_ok {
                pal.success.base.color
            } else {
                pal.danger.base.color.scale_alpha(0.8)
            })),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    });
    let mut head = row![
        dot,
        text(line).size(12).font(STRONG).width(Fill),
    ]
    .spacing(6)
    .align_y(iced::Center);
    if matches!(p.link, Link::Down(_)) {
        head = head.push(small_button("Reconnect", m(AssistantMsg::Reconnect)));
    }
    head = head.push(small_button("New chat", m(AssistantMsg::NewChat)));

    let mut meta = String::new();
    if !p.status.model.is_empty() {
        meta = format!(
            "{} · {} · {} tools · this chat ≈ ${:.2}",
            p.status.model, p.status.effort, p.status.tools, p.status.cost
        );
    }
    let mut header = column![head].spacing(2);
    if !p.status.path.is_empty() {
        header = header.push(text(p.status.path.clone()).size(10).style(muted_text));
    }
    if !meta.is_empty() {
        header = header.push(text(meta).size(10).style(muted_text));
    }
    if p.link == Link::Connected && !p.status.api_key && !p.status.model.is_empty() {
        header = header.push(
            text(crate::t!("The assistant service has no API key configured."))
                .size(11)
                .style(|t: &Theme| text::Style {
                    color: Some(t.palette().danger.base.color),
                }),
        );
    }

    // Conversation.
    let mut log = column![].spacing(8).padding([4, 10]).width(Fill);
    if p.items.is_empty() {
        log = log.push(
            container(
                column![
                    text("Tell me what to do in this drawing.").size(13).font(STRONG),
                    text("For example: “select all the doors”, “how many rooms are there?”, “move the selection 1 m right”, “zoom to the kitchen”.")
                        .size(12)
                        .style(muted_text),
                    text("Every step I take appears here as a card and happens live in the drawing. Ctrl+Z undoes it like any edit.")
                        .size(11)
                        .style(muted_text),
                ]
                .spacing(6),
            )
            .padding([18, 4]),
        );
    }
    for (i, item) in p.items.iter().enumerate() {
        log = log.push(item_view(i, item, theme));
    }
    if p.running {
        log = log.push(text("Working…").size(11).style(muted_text));
    }
    let log = scrollable(log)
        .id(iced::widget::Id::new(LOG_ID))
        .height(Fill)
        .width(Fill);

    // Chips.
    let mut chips = row![].spacing(4);
    for (i, (label, _)) in CHIPS.iter().enumerate() {
        let mut b = button(text(*label).size(11)).padding([2, 8]).style(chip_style);
        if !p.running {
            b = b.on_press(m(AssistantMsg::Chip(i)));
        }
        chips = chips.push(b);
    }
    let chips = chips.wrap();

    // Composer.
    let mut input = text_input("Ask or instruct… (Enter to send)", &p.input)
        .id(iced::widget::Id::new(INPUT_ID))
        .size(13)
        .padding([7, 9]);
    if !p.running {
        input = input
            .on_input(|v| m(AssistantMsg::Input(v)))
            .on_submit(m(AssistantMsg::Send));
    }
    let action = if p.running {
        button(text("Stop").size(12).font(STRONG))
            .padding([6, 12])
            .style(button::danger)
            .on_press(m(AssistantMsg::Stop))
    } else {
        button(text("Send").size(12).font(STRONG))
            .padding([6, 12])
            .style(button::primary)
            .on_press(m(AssistantMsg::Send))
    };
    let composer = row![input, action].spacing(6).align_y(iced::Center);

    column![
        container(header).padding([4, 4]),
        iced::widget::rule::horizontal(1),
        log,
        iced::widget::rule::horizontal(1),
        container(column![chips, composer].spacing(6)).padding([6, 4]),
    ]
    .spacing(4)
    .height(Fill)
    .into()
}

fn item_view<'a>(i: usize, item: &'a Item, theme: &'a Theme) -> Element<'a, Message> {
    match item {
        Item::User(t) => row![
            Space::new().width(Length::FillPortion(1)),
            container(text(t.as_str()).size(13))
                .padding([6, 10])
                .style(bubble_style),
        ]
        .width(Fill)
        .into(),
        Item::Bot { content, .. } => markdown::view(
            content.items(),
            markdown::Settings::with_text_size(13, theme),
        )
        .map(|uri| Message::OpenUrl(uri.to_string())),
        Item::Note(n) => {
            // Thinking summaries: keep the latest line short.
            let last = n.trim().lines().last().unwrap_or("").trim();
            let short: String = last.chars().take(160).collect();
            text(format!("› {short}")).size(11).style(muted_text).into()
        }
        Item::Card(card) => card_view(i, card),
        Item::Error(e) => text(e.as_str())
            .size(12)
            .style(|t: &Theme| text::Style {
                color: Some(t.palette().danger.base.color),
            })
            .into(),
        Item::Stopped => text("Stopped.").size(12).style(muted_text).into(),
    }
}

fn card_view<'a>(i: usize, card: &'a Card) -> Element<'a, Message> {
    let mark: Element<'a, Message> = match card.state {
        CardState::Pending | CardState::Running => text("◌").size(12).into(),
        CardState::Ok => text("✓").size(12).style(|t: &Theme| text::Style {
            color: Some(t.palette().success.base.color),
        }).into(),
        CardState::Failed => text("✗").size(12).style(|t: &Theme| text::Style {
            color: Some(t.palette().danger.base.color),
        }).into(),
    };
    let time = match card.state {
        CardState::Pending => "preparing…".to_string(),
        CardState::Running => "running…".to_string(),
        _ if card.ms >= 1000 => format!("{:.1} s", card.ms as f64 / 1000.0),
        _ => format!("{} ms", card.ms),
    };
    let summary = row![
        mark,
        text(card.title.as_str()).size(12).font(STRONG).width(Fill),
        text(time).size(10).style(muted_text),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let mut col = column![mouse_area(summary)
        .on_press(m(AssistantMsg::ToggleCard(i)))
        .interaction(iced::mouse::Interaction::Pointer)]
    .spacing(4);
    if card.open {
        let mono = |s: &'a str| {
            container(text(s).size(10).font(Font::MONOSPACE))
                .padding(4)
                .width(Fill)
                .style(|t: &Theme| container::Style {
                    background: Some(Background::Color(t.palette().background.base.color)),
                    border: Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
        };
        col = col.push(text(format!("{} · input", card.name)).size(10).style(muted_text));
        col = col.push(mono(card.args.as_str()));
        if !card.result.is_empty() {
            col = col.push(text("result").size(10).style(muted_text));
            col = col.push(mono(card.result.as_str()));
        }
    }
    for handle in &card.images {
        col = col.push(image(handle.clone()).width(Fill));
    }
    container(col)
        .padding([5, 8])
        .width(Fill)
        .style(card_style(card.state))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn palette() -> AssistantPalette {
        AssistantPalette {
            link: Link::Connected,
            ..Default::default()
        }
    }

    #[test]
    fn command_lines_split_on_spaces_and_keep_quoted_paths() {
        assert_eq!(
            split_command(r#""C:\My Tools\py.exe" -m  app.server"#),
            vec![r"C:\My Tools\py.exe", "-m", "app.server"]
        );
        assert_eq!(split_command("   "), Vec::<String>::new());
        assert_eq!(split_command(r#"run """#), vec!["run", ""]);
    }

    #[test]
    fn a_tool_step_goes_pending_running_done_on_one_card() {
        let mut p = palette();
        p.apply(json!({"type": "tool_pending", "id": "t1", "name": "ocs_select"}));
        p.apply(json!({"type": "tool_start", "id": "t1", "name": "ocs_select",
                       "title": "Select doors", "args": "{}"}));
        p.apply(json!({"type": "tool_end", "id": "t1", "ok": true, "ms": 42,
                       "preview": "3 selected", "images": []}));
        assert_eq!(p.items.len(), 1);
        let Item::Card(card) = &p.items[0] else { panic!("expected a card") };
        assert_eq!(card.title, "Select doors");
        assert_eq!(card.state, CardState::Ok);
        assert_eq!(card.ms, 42);
        assert!(!card.open, "a step that worked stays folded");
    }

    #[test]
    fn a_failed_step_opens_to_show_why() {
        let mut p = palette();
        p.apply(json!({"type": "tool_start", "id": "t2", "name": "ocs_run",
                       "title": "Run", "args": "{}"}));
        p.apply(json!({"type": "tool_end", "id": "t2", "ok": false, "ms": 3,
                       "preview": "unknown command", "images": []}));
        let Item::Card(card) = &p.items[0] else { panic!("expected a card") };
        assert_eq!(card.state, CardState::Failed);
        assert!(card.open);
        assert_eq!(card.result, "unknown command");
    }

    #[test]
    fn streamed_text_builds_one_reply_and_done_ends_the_run() {
        let mut p = palette();
        p.running = true;
        p.apply(json!({"type": "text_start"}));
        p.apply(json!({"type": "text", "text": "Selected "}));
        p.apply(json!({"type": "text", "text": "**3** doors."}));
        p.apply(json!({"type": "done", "cost_usd": 0.05}));
        assert!(!p.running);
        assert_eq!(p.items.len(), 1);
        let Item::Bot { source, .. } = &p.items[0] else { panic!("expected a reply") };
        assert_eq!(source, "Selected **3** doors.");
        assert!((p.status.cost - 0.05).abs() < 1e-9);
    }

    #[test]
    fn an_empty_reply_bubble_is_dropped_when_the_turn_only_acted() {
        let mut p = palette();
        p.apply(json!({"type": "text_start"}));
        p.apply(json!({"type": "done"}));
        assert!(p.items.is_empty());
    }

    #[test]
    fn sending_needs_a_connection_and_is_refused_while_running() {
        let mut p = AssistantPalette::default();
        p.input = "select the windows".into();
        let _ = p.submit(None);
        assert!(matches!(p.items.last(), Some(Item::Error(_))));
        assert!(!p.running);

        let mut p = palette();
        p.running = true;
        p.input = "again".into();
        let _ = p.submit(None);
        assert!(p.items.is_empty());
    }

    #[test]
    fn data_url_images_decode() {
        // 1×1 transparent PNG.
        let url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";
        assert!(decode_image(url).is_some());
        assert!(decode_image("data:image/png;base64,***").is_none());
    }
}
