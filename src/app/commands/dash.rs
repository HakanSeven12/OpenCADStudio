//! Command-line forms of the table commands (-LAYER, -PURGE, -STYLE,
//! -DIMSTYLE, -LAYOUT, -PLOT) in the prompt order scripts are written
//! against. Each one is a [`ScriptPromptCommand`] whose answers come back
//! here as `-NAME [answers…]` (a JSON array) and run on the same document
//! operations the dialogs and the inline `NAME <sub>` handlers use.

use super::*;
use crate::command::{PromptAnswers, PromptFlow, PromptStep, ScriptPromptCommand};
use crate::io::xref_model::wildcard_match;

/// Run a flow written as straight-line prompts (`?` returns the step).
fn flow(
    f: impl Fn(&mut PromptAnswers) -> Result<PromptStep, PromptStep> + Send + 'static,
) -> PromptFlow {
    Box::new(move |answers| f(&mut PromptAnswers::new(answers)).unwrap_or_else(|step| step))
}

/// The finished answers as the `-NAME [...]` line the dispatcher runs.
fn run(name: &str, args: &[&str]) -> Result<PromptStep, PromptStep> {
    Ok(PromptStep::Run(format!("{name} {}", serde_json::json!(args))))
}

/// `name` matches one of the comma-separated, wildcard-capable `list`.
fn listed(name: &str, list: &str) -> bool {
    list.split(',')
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .any(|pattern| wildcard_match(name, pattern))
}

const LAYER_OPTIONS: [(&str, &str); 15] = [
    ("?", "?"),
    ("Make", "MAKE"),
    ("Set", "SET"),
    ("New", "NEW"),
    ("Rename", "RENAME"),
    ("ON", "ON"),
    ("OFF", "OFF"),
    ("Color", "COLOR"),
    ("Ltype", "LTYPE"),
    ("LWeight", "LWEIGHT"),
    ("Plot", "PLOT"),
    ("Freeze", "FREEZE"),
    ("Thaw", "THAW"),
    ("LOck", "LOCK"),
    ("Unlock", "UNLOCK"),
];

fn layer_flow(current: String) -> PromptFlow {
    flow(move |q| {
        let option = q.keyword(
            crate::t!(
                "-LAYER  Enter an option [?/Make/Set/New/Rename/ON/OFF/Color/Ltype/LWeight/Plot/Freeze/Thaw/LOck/Unlock]:"
            )
            .into_owned(),
            None,
            &LAYER_OPTIONS,
        )?;
        let current_list = |q: &mut PromptAnswers| {
            q.text(
                crate::t!("-LAYER  Enter name list of layer(s) <%{name}>:", name = current)
                    .into_owned(),
                Some(current.clone()),
            )
        };
        match option {
            "?" => {
                let names = q.text(
                    crate::t!("-LAYER  Enter layer name(s) to list <*>:").into_owned(),
                    Some("*".into()),
                )?;
                run("-LAYER", &[option, &names])
            }
            "MAKE" | "SET" => {
                let prompt = if option == "MAKE" {
                    "-LAYER  Enter name for new layer (becomes the current layer) <%{name}>:"
                } else {
                    "-LAYER  Enter layer name to make current <%{name}>:"
                };
                let name = q.text(
                    crate::t!(prompt, name = current).into_owned(),
                    Some(current.clone()),
                )?;
                run("-LAYER", &[option, &name])
            }
            "NEW" => {
                let names = q.text(
                    crate::t!("-LAYER  Enter name list for new layer(s):").into_owned(),
                    None,
                )?;
                run("-LAYER", &[option, &names])
            }
            "RENAME" => {
                let old = q.text(crate::t!("-LAYER  Enter old layer name:").into_owned(), None)?;
                let new = q.text(crate::t!("-LAYER  Enter new layer name:").into_owned(), None)?;
                run("-LAYER", &[option, &old, &new])
            }
            "COLOR" => {
                let color = q.text(
                    crate::t!("-LAYER  Enter color name or number (1-255):").into_owned(),
                    None,
                )?;
                if !matches!(
                    crate::ui::window::attman_dialog::parse_color(&color),
                    Some(codec::types::Color::Index(_))
                ) {
                    return Err(PromptStep::Retry("Invalid input."));
                }
                let names = current_list(q)?;
                run("-LAYER", &[option, &color, &names])
            }
            "LTYPE" => {
                let linetype = q.text(
                    crate::t!("-LAYER  Enter a loaded linetype name <Continuous>:").into_owned(),
                    Some("Continuous".into()),
                )?;
                let names = current_list(q)?;
                run("-LAYER", &[option, &linetype, &names])
            }
            "LWEIGHT" => {
                let weight = q.text(
                    crate::t!("-LAYER  Enter lineweight (0.0mm - 2.11mm):").into_owned(),
                    None,
                )?;
                if crate::ui::window::attman_dialog::parse_lineweight(&weight).is_none() {
                    return Err(PromptStep::Retry("Invalid input."));
                }
                let names = current_list(q)?;
                run("-LAYER", &[option, &weight, &names])
            }
            "PLOT" => {
                let plot = q.keyword(
                    crate::t!("-LAYER  Enter a plotting preference [Plot/No plot] <Plot>:")
                        .into_owned(),
                    Some("PLOT"),
                    &[("Plot", "PLOT"), ("No plot", "NOPLOT")],
                )?;
                let names = current_list(q)?;
                run("-LAYER", &[option, plot, &names])
            }
            // ON / OFF / Freeze / Thaw / LOck / Unlock
            _ => {
                let names = q.text(
                    crate::t!("-LAYER  Enter name list of layer(s):").into_owned(),
                    None,
                )?;
                run("-LAYER", &[option, &names])
            }
        }
    })
}

const PURGE_TYPES: [(&str, &str); 19] = [
    ("Blocks", "BLOCKS"),
    ("DEtailviewstyles", "DETAILVIEWSTYLES"),
    ("Dimstyles", "DIMSTYLES"),
    ("Groups", "GROUPS"),
    ("LAyers", "LAYERS"),
    ("LTypes", "LINETYPES"),
    ("MAterials", "MATERIALS"),
    ("MLeaderstyles", "MLEADERSTYLES"),
    ("Mlinestyles", "MLINESTYLES"),
    ("Plotstyles", "PLOTSTYLES"),
    ("SHapes", "SHAPES"),
    ("textSTyles", "STYLES"),
    ("Tablestyles", "TABLESTYLES"),
    ("Visualstyles", "VISUALSTYLES"),
    ("Regapps", "REGAPPS"),
    ("Zero-length geometry", "ZEROLENGTH"),
    ("Empty text objects", "EMPTYTEXT"),
    ("Orphaned data", "ORPHANED"),
    ("All", "ALL"),
];

fn purge_flow() -> PromptFlow {
    flow(|q| {
        let kind = q.keyword(
            crate::t!(
                "-PURGE  Enter type of unused objects to purge [Blocks/DEtailviewstyles/Dimstyles/Groups/LAyers/LTypes/MAterials/MLeaderstyles/Mlinestyles/Plotstyles/SHapes/textSTyles/Tablestyles/Visualstyles/Regapps/Zero-length geometry/Empty text objects/Orphaned data/All]:"
            )
            .into_owned(),
            None,
            &PURGE_TYPES,
        )?;
        if matches!(kind, "REGAPPS" | "ZEROLENGTH" | "EMPTYTEXT" | "ORPHANED") {
            return run("-PURGE", &[kind, "*"]);
        }
        let names = q.text(
            crate::t!("-PURGE  Enter name(s) to purge <*>:").into_owned(),
            Some("*".into()),
        )?;
        q.yes_no(
            crate::t!("-PURGE  Verify each name to be purged? [Yes/No] <Y>:").into_owned(),
            true,
        )?;
        run("-PURGE", &[kind, &names])
    })
}

/// A text style's values as -STYLE offers them for defaults.
#[derive(Clone)]
struct StyleDefaults {
    name: String,
    font: String,
    height: f64,
    width: f64,
    oblique: f64,
}

fn style_flow(current: String, styles: Vec<StyleDefaults>) -> PromptFlow {
    flow(move |q| {
        let name = q.text(
            crate::t!("-STYLE  Enter name of text style or [?] <%{name}>:", name = current)
                .into_owned(),
            Some(current.clone()),
        )?;
        if name == "?" {
            return Ok(PromptStep::Run("STYLE LIST".into()));
        }
        let base = styles
            .iter()
            .find(|style| style.name.eq_ignore_ascii_case(&name))
            .or_else(|| styles.iter().find(|style| style.name == current))
            .cloned()
            .unwrap_or(StyleDefaults {
                name: String::new(),
                font: "txt.shx".into(),
                height: 0.0,
                width: 1.0,
                oblique: 0.0,
            });
        let font = q.text(
            crate::t!(
                "-STYLE  Specify full font name or font filename (TTF or SHX) <%{font}>:",
                font = base.font
            )
            .into_owned(),
            Some(base.font.clone()),
        )?;
        let height = q.number(
            crate::t!("-STYLE  Specify height of text <%{value}>:", value = base.height)
                .into_owned(),
            base.height,
        )?;
        let width = q.number(
            crate::t!("-STYLE  Specify width factor <%{value}>:", value = base.width).into_owned(),
            base.width,
        )?;
        let oblique = q.number(
            crate::t!("-STYLE  Specify obliquing angle <%{value}>:", value = base.oblique)
                .into_owned(),
            base.oblique,
        )?;
        let backwards = q.yes_no(
            crate::t!("-STYLE  Display text backwards? [Yes/No] <No>:").into_owned(),
            false,
        )?;
        let upside_down = q.yes_no(
            crate::t!("-STYLE  Display text upside-down? [Yes/No] <No>:").into_owned(),
            false,
        )?;
        // Only shape fonts can stand vertically, so only they ask.
        let vertical = shape_font(&font)
            && q.yes_no(
                crate::t!("-STYLE  Vertically? [Yes/No] <No>:").into_owned(),
                false,
            )?;
        let flag = |on: bool| if on { "Y" } else { "N" };
        run(
            "-STYLE",
            &[
                &name,
                &font,
                &height.to_string(),
                &width.to_string(),
                &oblique.to_string(),
                flag(backwards),
                flag(upside_down),
                flag(vertical),
            ],
        )
    })
}

/// A font named without an extension is a shape font (`romans` →
/// `romans.shx`), as a reference script expects.
fn shape_font(font: &str) -> bool {
    let lower = font.to_ascii_lowercase();
    lower.ends_with(".shx") || !lower.contains('.')
}

fn dimstyle_flow(current: String, existing: Vec<String>) -> PromptFlow {
    flow(move |q| {
        let option = q.keyword(
            crate::t!(
                "-DIMSTYLE  Enter a dimension style option [ANnotative/Save/Restore/STatus/Variables/Apply/?] <Restore>:"
            )
            .into_owned(),
            Some("RESTORE"),
            &[
                ("ANnotative", "ANNOTATIVE"),
                ("Save", "SAVE"),
                ("Restore", "RESTORE"),
                ("STatus", "STATUS"),
                ("Variables", "VARIABLES"),
                ("Apply", "APPLY"),
                ("?", "?"),
            ],
        )?;
        match option {
            // A new style, annotative or not; -DIMSTYLE asks again after it.
            "ANNOTATIVE" => {
                let annotative = q.yes_no(
                    crate::t!("-DIMSTYLE  Create annotative dimension style [Yes/No] <Yes>:")
                        .into_owned(),
                    true,
                )?;
                let name = q.text(
                    crate::t!("-DIMSTYLE  Enter name for new dimension style or [?]:")
                        .into_owned(),
                    None,
                )?;
                if name == "?" {
                    return run("-DIMSTYLE", &["?", "*"]);
                }
                run(
                    "-DIMSTYLE",
                    &["ANNOTATIVE", &name, if annotative { "1" } else { "0" }],
                )
            }
            "APPLY" => run("-DIMSTYLE", &["APPLY"]),
            "SAVE" => {
                let name = q.text(
                    crate::t!("-DIMSTYLE  Enter name for new dimension style or [?]:")
                        .into_owned(),
                    None,
                )?;
                if name == "?" {
                    return run("-DIMSTYLE", &["?", "*"]);
                }
                if existing.iter().any(|style| style.eq_ignore_ascii_case(&name))
                    && !q.yes_no(
                        crate::t!(
                            "-DIMSTYLE  That name is already in use, redefine it? [Yes/No] <No>:"
                        )
                        .into_owned(),
                        false,
                    )?
                {
                    return Ok(PromptStep::Finish);
                }
                run("-DIMSTYLE", &[option, &name])
            }
            "RESTORE" => {
                let name = q.text(
                    crate::t!("-DIMSTYLE  Enter a dimension style name, [?] or <select dimension>:")
                        .into_owned(),
                    Some(String::new()),
                )?;
                if name == "?" {
                    return run("-DIMSTYLE", &["?", "*"]);
                }
                if name.is_empty() {
                    return run("-DIMSTYLE", &["RESTORE_PICK"]);
                }
                run("-DIMSTYLE", &[option, &name])
            }
            "VARIABLES" => {
                let name = q.text(
                    crate::t!(
                        "-DIMSTYLE  Enter a dimension style name or [?] <%{name}>:",
                        name = current
                    )
                    .into_owned(),
                    Some(current.clone()),
                )?;
                if name == "?" {
                    return run("-DIMSTYLE", &["?", "*"]);
                }
                run("-DIMSTYLE", &[option, &name])
            }
            "STATUS" => run("-DIMSTYLE", &["VARIABLES", &current]),
            _ => {
                let names = q.text(
                    crate::t!("-DIMSTYLE  Enter dimension style(s) to list <*>:").into_owned(),
                    Some("*".into()),
                )?;
                run("-DIMSTYLE", &["?", &names])
            }
        }
    })
}

fn layout_flow(current: String, next: String) -> PromptFlow {
    flow(move |q| {
        let option = q.keyword(
            crate::t!("-LAYOUT  Enter layout option [Copy/Delete/New/Rename/Set/?] <Set>:")
                .into_owned(),
            Some("SET"),
            &[
                ("Copy", "COPY"),
                ("Delete", "DELETE"),
                ("New", "NEW"),
                ("Rename", "RENAME"),
                ("Set", "SET"),
                ("?", "?"),
            ],
        )?;
        let named = |q: &mut PromptAnswers, prompt: &str| {
            q.text(
                crate::t!(prompt, name = current).into_owned(),
                Some(current.clone()),
            )
        };
        match option {
            "COPY" => {
                let source = named(q, "-LAYOUT  Enter name of layout to copy <%{name}>:")?;
                let copy = format!("{source} (2)");
                let name = q.text(
                    crate::t!("-LAYOUT  Enter layout name for copy <%{name}>:", name = copy)
                        .into_owned(),
                    Some(copy.clone()),
                )?;
                run("-LAYOUT", &[option, &source, &name])
            }
            "DELETE" => {
                let name = named(q, "-LAYOUT  Enter name of layout to delete <%{name}>:")?;
                run("-LAYOUT", &[option, &name])
            }
            "NEW" => {
                let name = q.text(
                    crate::t!("-LAYOUT  Enter name of new layout <%{name}>:", name = next)
                        .into_owned(),
                    Some(next.clone()),
                )?;
                run("-LAYOUT", &[option, &name])
            }
            "RENAME" => {
                let old = named(q, "-LAYOUT  Enter layout to rename <%{name}>:")?;
                let new = q.text(crate::t!("-LAYOUT  Enter new layout name:").into_owned(), None)?;
                run("-LAYOUT", &[option, &old, &new])
            }
            "SET" => {
                let name = named(q, "-LAYOUT  Enter layout to make current <%{name}>:")?;
                run("-LAYOUT", &[option, &name])
            }
            _ => run("-LAYOUT", &["?"]),
        }
    })
}

/// What -PLOT starts from: the active layout and the plot dialog's setup.
#[cfg(not(target_arch = "wasm32"))]
struct PlotDefaults {
    layout: String,
    stem: String,
    paper: String,
    orientation: String,
    style: String,
}

#[cfg(not(target_arch = "wasm32"))]
fn plot_flow(d: PlotDefaults) -> PromptFlow {
    flow(move |q| {
        let mut req = serde_json::Map::new();
        let detailed = q.yes_no(
            crate::t!("-PLOT  Detailed plot configuration? [Yes/No] <No>:").into_owned(),
            false,
        )?;
        let layout = q.text(
            crate::t!("-PLOT  Enter a layout name <%{name}>:", name = d.layout).into_owned(),
            Some(d.layout.clone()),
        )?;
        let model = layout.eq_ignore_ascii_case("Model");
        req.insert("layout".into(), layout.clone().into());
        if !detailed {
            q.text(
                crate::t!("-PLOT  Enter a page setup name <>:").into_owned(),
                Some(String::new()),
            )?;
        }
        let device = q.text(
            crate::t!("-PLOT  Enter an output device name <%{name}>:", name = "PDF")
                .into_owned(),
            Some("PDF".into()),
        )?;
        if detailed {
            let paper = q.text(
                crate::t!("-PLOT  Enter paper size <%{name}>:", name = d.paper).into_owned(),
                Some(d.paper.clone()),
            )?;
            req.insert("paper".into(), paper.into());
            q.keyword(
                crate::t!("-PLOT  Enter paper units [Inches/Millimeters] <Millimeters>:")
                    .into_owned(),
                Some("MILLIMETERS"),
                &[("Inches", "INCHES"), ("Millimeters", "MILLIMETERS")],
            )?;
            let landscape = d.orientation.eq_ignore_ascii_case("Landscape");
            let orientation = q.keyword(
                crate::t!(
                    "-PLOT  Enter drawing orientation [Portrait/Landscape] <%{name}>:",
                    name = d.orientation
                )
                .into_owned(),
                Some(if landscape { "LANDSCAPE" } else { "PORTRAIT" }),
                &[("Portrait", "PORTRAIT"), ("Landscape", "LANDSCAPE")],
            )?;
            req.insert("orientation".into(), orientation.into());
            let upside_down = q.yes_no(
                crate::t!("-PLOT  Plot upside down? [Yes/No] <No>:").into_owned(),
                false,
            )?;
            req.insert("upside_down".into(), upside_down.into());
            let (areas, default): (&[(&str, &str)], &str) = if model {
                (
                    &[
                        ("Display", "DISPLAY"),
                        ("Extents", "EXTENTS"),
                        ("Limits", "LIMITS"),
                        ("Window", "WINDOW"),
                    ],
                    "EXTENTS",
                )
            } else {
                (
                    &[
                        ("Display", "DISPLAY"),
                        ("Extents", "EXTENTS"),
                        ("Layout", "LAYOUT"),
                        ("Window", "WINDOW"),
                    ],
                    "LAYOUT",
                )
            };
            let area = q.keyword(
                crate::t!(
                    "-PLOT  Enter plot area [Display/Extents/Limits/Layout/Window] <%{name}>:",
                    name = default
                )
                .into_owned(),
                Some(default),
                areas,
            )?;
            req.insert("area".into(), area.to_ascii_lowercase().into());
            if area == "WINDOW" {
                let mut window = Vec::new();
                for prompt in [
                    "-PLOT  Enter lower left corner of window <0,0>:",
                    "-PLOT  Enter upper right corner of window <0,0>:",
                ] {
                    let corner = q.text(crate::t!(prompt).into_owned(), Some("0,0".into()))?;
                    let xy = parse_xy(&corner).ok_or(PromptStep::Retry("Invalid input."))?;
                    window.extend(xy);
                }
                req.insert("window".into(), serde_json::json!(window));
            }
            let fit_default = if model { "Fit" } else { "1:1" };
            let scale = q.text(
                crate::t!(
                    "-PLOT  Enter plot scale (Plotted Millimeters=Drawing Units) or [Fit] <%{name}>:",
                    name = fit_default
                )
                .into_owned(),
                Some(fit_default.into()),
            )?;
            if scale.to_ascii_uppercase().starts_with('F') {
                req.insert("fit".into(), true.into());
            } else {
                let scale = scale.replace('=', ":");
                let ratio = if scale.contains(':') { scale } else { format!("1:{scale}") };
                req.insert("scale".into(), ratio.into());
            }
            let offset = q.text(
                crate::t!("-PLOT  Enter plot offset (x,y) or [Center] <Center>:").into_owned(),
                Some("Center".into()),
            )?;
            if offset.to_ascii_uppercase().starts_with('C') {
                req.insert("center".into(), true.into());
            } else {
                let [x, y] = parse_xy(&offset).ok_or(PromptStep::Retry("Invalid input."))?;
                req.insert("center".into(), false.into());
                req.insert("offset_x".into(), x.into());
                req.insert("offset_y".into(), y.into());
            }
            if q.yes_no(
                crate::t!("-PLOT  Plot with plot styles? [Yes/No] <Yes>:").into_owned(),
                true,
            )? {
                let style = q.text(
                    crate::t!(
                        "-PLOT  Enter plot style table name (enter . for none) <%{name}>:",
                        name = d.style
                    )
                    .into_owned(),
                    Some(d.style.clone()),
                )?;
                if !style.is_empty() && style != "." {
                    req.insert("plot_style".into(), style.into());
                }
            }
            let lineweights = q.yes_no(
                crate::t!("-PLOT  Plot with lineweights? [Yes/No] <Yes>:").into_owned(),
                true,
            )?;
            req.insert("lineweights".into(), lineweights.into());
            if model {
                q.keyword(
                    crate::t!(
                        "-PLOT  Enter shade plot setting [As displayed/legacy Wireframe/legacy Hidden/Visual styles/Rendered] <As displayed>:"
                    )
                    .into_owned(),
                    Some("ASDISPLAYED"),
                    &[
                        ("As displayed", "ASDISPLAYED"),
                        ("legacy Wireframe", "WIREFRAME"),
                        ("legacy Hidden", "HIDDEN"),
                        ("Visual styles", "VISUALSTYLES"),
                        ("Rendered", "RENDERED"),
                    ],
                )?;
            } else {
                for prompt in [
                    "-PLOT  Scale lineweights with plot scale? [Yes/No] <No>:",
                    "-PLOT  Plot paper space first? [Yes/No] <No>:",
                    "-PLOT  Hide paperspace objects? [Yes/No] <No>:",
                ] {
                    q.yes_no(crate::t!(prompt).into_owned(), false)?;
                }
            }
        }
        let lower = device.to_ascii_lowercase();
        let ext = if lower.contains("svg") { "svg" } else { "pdf" };
        let to_file = lower.contains("pdf")
            || lower.contains("svg")
            || q.yes_no(
                crate::t!("-PLOT  Write the plot to a file [Yes/No] <N>:").into_owned(),
                false,
            )?;
        if to_file {
            let safe: String = layout
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
                .collect();
            let file = format!("{}-{safe}.{ext}", d.stem);
            let path = q.text(
                crate::t!("-PLOT  Enter file name <%{name}>:", name = file).into_owned(),
                Some(file.clone()),
            )?;
            req.insert("path".into(), path.into());
        }
        q.yes_no(
            crate::t!("-PLOT  Save changes to page setup [Yes/No]? <N>").into_owned(),
            false,
        )?;
        if !q.yes_no(
            crate::t!("-PLOT  Proceed with plot [Yes/No] <Y>:").into_owned(),
            true,
        )? {
            return Ok(PromptStep::Finish);
        }
        Ok(PromptStep::Run(format!("-PLOT {}", serde_json::Value::Object(req))))
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_xy(text: &str) -> Option<[f64; 2]> {
    let (x, y) = text.split_once(',')?;
    Some([x.trim().parse().ok()?, y.trim().parse().ok()?])
}

impl OpenCADStudio {
    /// `-LAYER`, `-PURGE`, `-STYLE`, `-DIMSTYLE`, `-LAYOUT`, `-PLOT`: bare,
    /// they start the prompt sequence; with the answers they finish it.
    pub(super) fn dispatch_dash(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (verb, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
        let start = |app: &mut Self, command: ScriptPromptCommand| {
            app.command_line.push_info(&command.prompt());
            app.command_line.set_step_options(command.options());
            app.tabs[i].active_cmd = Some(Box::new(command));
        };
        let args: Vec<String> = serde_json::from_str(rest).unwrap_or_default();
        let arg = |n: usize| args.get(n).map(String::as_str).unwrap_or("");
        let mut task = Task::none();
        match verb {
            "-LAYER" => {
                if !rest.is_empty() {
                    task = self.dash_layer(i, &args);
                }
                // -LAYER keeps asking for options until an empty Enter.
                let current = self.tabs[i].scene.document.header.current_layer_name.clone();
                start(self, ScriptPromptCommand::new("-LAYER", layer_flow(current)));
            }
            "-PURGE" if rest.is_empty() => {
                start(self, ScriptPromptCommand::new("-PURGE", purge_flow()))
            }
            "-PURGE" => {
                if matches!(arg(0), "ALL" | "BLOCKS" | "LAYERS" | "LINETYPES" | "STYLES") {
                    return Some(self.dispatch_command(&format!("PURGE {} {}", arg(0), arg(1))));
                }
                // ponytail: only the tables the inline PURGE knows are purged;
                // the rest answer so a script keeps going.
                self.command_line
                    .push_output(crate::tf!("PURGE: {} are not purged yet.", arg(0)).as_ref());
            }
            "-STYLE" if rest.is_empty() => {
                let doc = &self.tabs[i].scene.document;
                let styles = doc
                    .text_styles
                    .iter()
                    .filter(|style| !style.name.is_empty())
                    .map(|style| StyleDefaults {
                        name: style.name.clone(),
                        font: if style.font_file.is_empty() {
                            style.true_type_font.clone()
                        } else {
                            style.font_file.clone()
                        },
                        height: style.height,
                        width: style.width_factor,
                        oblique: style.oblique_angle.to_degrees(),
                    })
                    .collect();
                let current = doc.header.current_text_style_name.clone();
                start(self, ScriptPromptCommand::new("-STYLE", style_flow(current, styles)));
            }
            "-STYLE" => self.dash_style(i, &args),
            "-DIMSTYLE" if rest.is_empty() => {
                self.push_current_dimstyle(i);
                let doc = &self.tabs[i].scene.document;
                let current = doc.header.current_dimstyle_name.clone();
                let existing = doc.dim_styles.iter().map(|s| s.name.clone()).collect();
                let flow = dimstyle_flow(current, existing);
                start(self, ScriptPromptCommand::new("-DIMSTYLE", flow));
            }
            "-DIMSTYLE" => {
                self.dash_dimstyle(i, &args);
                // After a new style from ANnotative the options come again.
                if arg(0) == "ANNOTATIVE" {
                    self.push_current_dimstyle(i);
                    let doc = &self.tabs[i].scene.document;
                    let current = doc.header.current_dimstyle_name.clone();
                    let existing = doc.dim_styles.iter().map(|s| s.name.clone()).collect();
                    let flow = dimstyle_flow(current, existing);
                    start(self, ScriptPromptCommand::new("-DIMSTYLE", flow));
                }
            }
            "-LAYOUT" if rest.is_empty() => {
                let current = self.tabs[i].scene.current_layout.clone();
                let next = Self::next_layout_name(&self.tabs[i].scene.layout_names());
                start(self, ScriptPromptCommand::new("-LAYOUT", layout_flow(current, next)));
            }
            "-LAYOUT" => return Some(self.dash_layout(i, &args)),
            // The web plots through the PLOT dialog's download.
            #[cfg(target_arch = "wasm32")]
            "-PLOT" => return Some(Task::done(Message::PlotDialogOpen)),
            #[cfg(not(target_arch = "wasm32"))]
            "-PLOT" if rest.is_empty() => {
                let stem = self.tabs[i]
                    .current_path
                    .as_ref()
                    .map(|path| path.with_extension("").to_string_lossy().into_owned())
                    .unwrap_or_else(|| self.tabs[i].tab_title.clone());
                let defaults = PlotDefaults {
                    layout: self.tabs[i].scene.current_layout.clone(),
                    stem,
                    paper: self.plot_dialog.paper.clone(),
                    orientation: self.plot_dialog.orientation.clone(),
                    style: self.plot_dialog.style_name.clone(),
                };
                start(self, ScriptPromptCommand::new("-PLOT", plot_flow(defaults)));
            }
            #[cfg(not(target_arch = "wasm32"))]
            "-PLOT" => self.dash_plot(rest),
            _ => return None,
        }
        // A repeated Enter starts the command over, not its last answers.
        Some(Task::batch([task, self.finish_dispatch(verb)]))
    }

    /// The document's layers named by a comma-separated, wildcard list;
    /// a plain name that matches nothing is reported.
    fn listed_layers(&mut self, i: usize, list: &str) -> Vec<String> {
        let names: Vec<String> = self.tabs[i]
            .scene
            .document
            .layers
            .iter()
            .filter(|layer| listed(&layer.name, list))
            .map(|layer| layer.name.clone())
            .collect();
        if names.is_empty() {
            self.command_line
                .push_error(crate::tf!("LAYER: '{}' not found.", list).as_ref());
        }
        names
    }

    /// Change `names` as one undo step and repaint what the change touches.
    fn edit_layers(
        &mut self,
        i: usize,
        label: &str,
        names: &[String],
        edit: impl Fn(&mut codec::tables::Layer),
    ) {
        if names.is_empty() {
            return;
        }
        let undo = self.begin_layer_undo(i, label, names);
        for name in names {
            if let Some(layer) = self.tabs[i].scene.document.layers.get_mut(name) {
                edit(layer);
            }
        }
        self.tabs[i].scene.invalidate_layer_visibility(names);
        self.tabs[i].scene.invalidate_layer_dependencies(names);
        self.tabs[i].dirty = true;
        self.commit_layer_undo(i, undo);
        self.refresh_layer_panel();
    }

    /// Add the layers in `list` that do not exist yet (one undo step).
    fn create_layers(&mut self, i: usize, list: &str) {
        let doc = &self.tabs[i].scene.document;
        let new: Vec<String> = list
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty() && !doc.layers.contains(name))
            .map(str::to_string)
            .collect();
        if new.is_empty() {
            return;
        }
        let undo = self.begin_layer_undo(i, "LAYER NEW", &new);
        for name in &new {
            let mut layer = codec::tables::Layer::new(name);
            // A layer needs a real handle or a DWG save drops it (#67).
            layer.handle = self.tabs[i].scene.document.allocate_handle();
            let _ = self.tabs[i].scene.document.layers.add(layer);
            self.command_line
                .push_output(crate::tf!("LAYER: '{}' created.", name).as_ref());
        }
        self.tabs[i].dirty = true;
        self.commit_layer_undo(i, undo);
        self.refresh_layer_panel();
    }

    fn dash_layer(&mut self, i: usize, args: &[String]) -> Task<Message> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or_default();
        let current = self.tabs[i].scene.document.header.current_layer_name.clone();
        match arg(0).as_str() {
            "?" => {
                let info: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
                    .filter(|layer| listed(&layer.name, &arg(1)))
                    .map(|layer| {
                        let state = if layer.flags.frozen {
                            "frozen"
                        } else if layer.flags.off {
                            "off"
                        } else if layer.flags.locked {
                            "locked"
                        } else {
                            "on"
                        };
                        format!("{}({})", layer.name, state)
                    })
                    .collect();
                self.command_line
                    .push_output(crate::tf!("Layers: {}", info.join(", ")).as_ref());
            }
            "MAKE" => {
                self.create_layers(i, &arg(1));
                if let Err(error) = self.set_current_layer_name(i, &arg(1)) {
                    self.command_line.push_error(&error);
                }
            }
            "SET" => {
                if self.tabs[i].scene.document.layers.contains(&arg(1)) {
                    let _ = self.set_current_layer_name(i, &arg(1));
                } else {
                    self.command_line
                        .push_error(crate::tf!("LAYER: '{}' not found.", arg(1)).as_ref());
                }
            }
            "NEW" => self.create_layers(i, &arg(1)),
            "RENAME" => {
                return self.dispatch_command(&format!("RENAME LAYER {} {}", arg(1), arg(2)))
            }
            "COLOR" => {
                let names = self.listed_layers(i, &arg(2));
                if let Some(color) = crate::ui::window::attman_dialog::parse_color(&arg(1)) {
                    self.edit_layers(i, "LAYER COLOR", &names, |layer| {
                        layer.color = color;
                        layer.color_name = None;
                        layer.book_name = None;
                    });
                }
            }
            "LTYPE" => {
                let linetype = self.tabs[i]
                    .scene
                    .document
                    .line_types
                    .iter()
                    .find(|lt| lt.name.eq_ignore_ascii_case(&arg(1)))
                    .map(|lt| lt.name.clone());
                let Some(linetype) = linetype else {
                    let name = arg(1);
                    self.command_line.push_error(
                        crate::tf!(
                            "LINETYPE: \"{name}\" is not loaded. Use LINETYPE LIST to see available linetypes."
                        )
                        .as_ref(),
                    );
                    return Task::none();
                };
                let names = self.listed_layers(i, &arg(2));
                self.edit_layers(i, "LAYER LINETYPE", &names, |layer| {
                    layer.line_type = linetype.clone()
                });
            }
            "LWEIGHT" => {
                let names = self.listed_layers(i, &arg(2));
                if let Some(weight) = crate::ui::window::attman_dialog::parse_lineweight(&arg(1)) {
                    self.edit_layers(i, "LAYER LINEWEIGHT", &names, |layer| {
                        layer.line_weight = weight
                    });
                }
            }
            "PLOT" => {
                let names = self.listed_layers(i, &arg(2));
                let plot = arg(1) == "PLOT";
                self.edit_layers(i, "LAYER PLOT/NOPLOT", &names, |layer| {
                    layer.is_plottable = plot
                });
            }
            option => {
                let mut names = self.listed_layers(i, &arg(1));
                if option == "FREEZE" && names.contains(&current) {
                    self.command_line.push_error(
                        crate::tf!("Cannot freeze layer \"{}\". It is the CURRENT layer.", current)
                            .as_ref(),
                    );
                    names.retain(|name| name != &current);
                }
                let edit: fn(&mut codec::tables::Layer) = match option {
                    "ON" => codec::tables::Layer::turn_on,
                    "OFF" => codec::tables::Layer::turn_off,
                    "FREEZE" => codec::tables::Layer::freeze,
                    "THAW" => codec::tables::Layer::thaw,
                    "LOCK" => codec::tables::Layer::lock,
                    _ => codec::tables::Layer::unlock,
                };
                let label = format!("LAYER {option}");
                self.edit_layers(i, &label, &names, edit);
            }
        }
        Task::none()
    }

    fn dash_style(&mut self, i: usize, args: &[String]) {
        let [name, font, height, width, oblique, backwards, upside_down, vertical] =
            std::array::from_fn(|n| args.get(n).cloned().unwrap_or_default());
        let undo = self.begin_text_style_undo(i, "STYLE", std::slice::from_ref(&name));
        let doc = &mut self.tabs[i].scene.document;
        if doc.text_styles.get(&name).is_none() {
            let mut style = codec::tables::TextStyle::new(&name);
            style.handle = doc.allocate_handle();
            let _ = doc.text_styles.add(style);
        }
        if let Some(style) = doc.text_styles.get_mut(&name) {
            let font = if font.contains('.') {
                font
            } else {
                format!("{font}.shx")
            };
            style.is_shape_file = false;
            style.true_type_font.clear();
            style.font_file = font;
            style.height = height.parse().unwrap_or(0.0);
            style.width_factor = width.parse().unwrap_or(1.0);
            style.oblique_angle = oblique.parse::<f64>().unwrap_or(0.0).to_radians();
            style.flags.backward = backwards == "Y";
            style.flags.upside_down = upside_down == "Y";
            style.is_vertical = vertical == "Y";
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.invalidate_text_style_dependencies(&name);
        self.commit_text_style_undo(i, undo);
        let _ = self.on_ribbon_style_changed(crate::modules::StyleKey::TextStyle, name.clone());
        self.command_line
            .push_output(crate::tf!("{} is now the current text style.", name).as_ref());
    }

    /// "Current dimension style: X   Annotative: No".
    fn push_current_dimstyle(&mut self, i: usize) {
        let doc = &self.tabs[i].scene.document;
        let current = doc.header.current_dimstyle_name.clone();
        let annotative = crate::scene::annotative::dim_style_is_annotative(doc, &current);
        let yes_no = if annotative { crate::t!("Yes") } else { crate::t!("No") };
        self.command_line.push_info(
            crate::t!(
                "-DIMSTYLE  Current dimension style: %{name}   Annotative: %{annotative}",
                name = current,
                annotative = yes_no
            )
            .as_ref(),
        );
    }

    fn dash_dimstyle(&mut self, i: usize, args: &[String]) {
        let arg = |n: usize| args.get(n).cloned().unwrap_or_default();
        let name = arg(1);
        match arg(0).as_str() {
            // ANnotative: the current style saved under a new name, with the
            // annotative setting asked for, and made current.
            "ANNOTATIVE" => {
                self.dash_dimstyle(i, &["SAVE".to_string(), name.clone()]);
                let doc = &mut self.tabs[i].scene.document;
                if let Some(style) = doc.dim_styles.get_mut(&name) {
                    style.annotative = arg(2) == "1";
                    self.tabs[i].scene.invalidate_dim_style_dependencies(&name);
                }
            }
            // Apply: the picked dimensions take the current style and lose
            // their overrides.
            "APPLY" => {
                use crate::modules::draw::select::SelectObjectsCommand;
                let select = SelectObjectsCommand::with_prompt(
                    "-DIMSTYLE",
                    r#"-DIMSTYLE ["APPLY_PICKED"]"#,
                    "Select objects:",
                );
                self.command_line.push_info(&select.prompt());
                self.tabs[i].active_cmd = Some(Box::new(select));
            }
            "APPLY_PICKED" => {
                let scene = &self.tabs[i].scene;
                let handles: Vec<codec::Handle> = scene
                    .selected_entities()
                    .into_iter()
                    .filter(|(handle, entity)| {
                        matches!(entity, codec::EntityType::Dimension(_))
                            && !scene.is_layer_locked(*handle)
                    })
                    .map(|(handle, _)| handle)
                    .collect();
                if handles.is_empty() {
                    return;
                }
                let current = scene.document.header.current_dimstyle_name.clone();
                self.push_undo_snapshot(i, "DIMSTYLE APPLY");
                for &handle in &handles {
                    let doc = &mut self.tabs[i].scene.document;
                    if let Some(codec::EntityType::Dimension(dimension)) =
                        doc.get_entity_mut(handle)
                    {
                        dimension.base_mut().style_name = current.clone();
                    }
                    crate::entities::dim_override::replace(doc, handle, Vec::new());
                }
                self.invalidate_property_targets(i, &handles);
                self.tabs[i].scene.deselect_all();
                self.tabs[i].dirty = true;
                self.refresh_properties();
            }
            // Restore from a picked dimension's style.
            "RESTORE_PICK" => {
                use crate::modules::draw::select::SelectObjectsCommand;
                let select = SelectObjectsCommand::with_prompt(
                    "-DIMSTYLE",
                    r#"-DIMSTYLE ["RESTORE_PICKED"]"#,
                    "Select dimension:",
                );
                self.command_line.push_info(&select.prompt());
                self.tabs[i].active_cmd = Some(Box::new(select));
            }
            "RESTORE_PICKED" => {
                let scene = &self.tabs[i].scene;
                let style = scene
                    .selected_entities()
                    .into_iter()
                    .find_map(|(_, entity)| match entity {
                        codec::EntityType::Dimension(dimension) => {
                            Some(dimension.base().style_name.clone())
                        }
                        _ => None,
                    });
                self.tabs[i].scene.deselect_all();
                if let Some(style) = style {
                    self.dash_dimstyle(i, &["RESTORE".to_string(), style]);
                }
            }
            "SAVE" => {
                let doc = &self.tabs[i].scene.document;
                let current = doc.header.current_dimstyle_name.clone();
                let Some(mut style) = doc.dim_styles.get(&current).cloned() else {
                    self.command_line
                        .push_error(crate::tf!("DIMSTYLE: '{}' not found.", current).as_ref());
                    return;
                };
                let undo =
                    self.begin_dim_style_undo(i, "DIMSTYLE SAVE", std::slice::from_ref(&name));
                let doc = &mut self.tabs[i].scene.document;
                match doc.dim_styles.get_mut(&name) {
                    Some(existing) => {
                        style.name = existing.name.clone();
                        style.handle = existing.handle;
                        *existing = style;
                    }
                    None => {
                        style.name = name.clone();
                        style.handle = doc.allocate_handle();
                        let _ = doc.dim_styles.add(style);
                    }
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.invalidate_dim_style_dependencies(&name);
                self.commit_dim_style_undo(i, undo);
                let _ = self.on_ribbon_style_changed(crate::modules::StyleKey::DimStyle, name);
            }
            "RESTORE" => {
                let found = self.tabs[i]
                    .scene
                    .document
                    .dim_styles
                    .iter()
                    .find(|style| style.name.eq_ignore_ascii_case(&name))
                    .map(|style| style.name.clone());
                match found {
                    Some(found) => {
                        let _ =
                            self.on_ribbon_style_changed(crate::modules::StyleKey::DimStyle, found);
                        self.push_current_dimstyle(i);
                    }
                    None => self
                        .command_line
                        .push_error(crate::tf!("DIMSTYLE: '{}' not found.", name).as_ref()),
                }
            }
            "VARIABLES" => {
                let doc = &self.tabs[i].scene.document;
                let Some(s) = doc.dim_styles.get(&name) else {
                    self.command_line
                        .push_error(crate::tf!("DIMSTYLE: '{}' not found.", name).as_ref());
                    return;
                };
                let lines: Vec<String> = [
                    ("DIMASZ", s.dimasz),
                    ("DIMCEN", s.dimcen),
                    ("DIMDLE", s.dimdle),
                    ("DIMDLI", s.dimdli),
                    ("DIMEXE", s.dimexe),
                    ("DIMEXO", s.dimexo),
                    ("DIMGAP", s.dimgap),
                    ("DIMLFAC", s.dimlfac),
                    ("DIMSCALE", s.dimscale),
                    ("DIMTSZ", s.dimtsz),
                    ("DIMTVP", s.dimtvp),
                    ("DIMTXT", s.dimtxt),
                ]
                .iter()
                .map(|(var, value)| format!("{var:<10}{value:.4}"))
                .chain(std::iter::once(format!("{:<10}{}", "DIMTXSTY", s.dimtxsty)))
                .collect();
                self.command_line.push_output(&s.name.clone());
                for line in lines {
                    self.command_line.push_output(&line);
                }
            }
            _ => {
                let names: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .dim_styles
                    .iter()
                    .filter(|style| listed(&style.name, &name))
                    .map(|style| style.name.clone())
                    .collect();
                self.command_line
                    .push_output(crate::tf!("DimStyles: {}", names.join(", ")).as_ref());
            }
        }
    }

    fn dash_layout(&mut self, i: usize, args: &[String]) -> Task<Message> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or_default();
        let layouts = self.tabs[i].scene.layout_names();
        // Layout names are matched without case, as typed in scripts.
        let find = |name: &str| layouts.iter().find(|l| l.eq_ignore_ascii_case(name)).cloned();
        let missing = |app: &mut Self, name: &str| {
            app.command_line
                .push_error(crate::tf!("Layout '{}' does not exist", name).as_ref());
            Task::none()
        };
        match arg(0).as_str() {
            "COPY" => match find(&arg(1)) {
                Some(source) => self.copy_layout_named(source, Some(arg(2))),
                None => missing(self, &arg(1)),
            },
            "DELETE" => match find(&arg(1)).filter(|name| name != "Model") {
                Some(name) => self.update(Message::LayoutDelete(name)),
                None => missing(self, &arg(1)),
            },
            "NEW" => self.create_layout_named(Some(arg(1))),
            "RENAME" => match find(&arg(1)).filter(|name| name != "Model") {
                Some(old) => {
                    self.layout_rename_state = Some((old, arg(2)));
                    self.on_layout_rename_commit()
                }
                None => missing(self, &arg(1)),
            },
            "SET" => match find(&arg(1)) {
                Some(name) => self.on_layout_switch(name),
                None => missing(self, &arg(1)),
            },
            _ => {
                let list = format!("{}: {}", crate::t!("Layouts"), layouts.join(", "));
                self.command_line.push_output(&list);
                Task::none()
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn dash_plot(&mut self, request: &str) {
        let Ok(request) = serde_json::from_str::<serde_json::Value>(request) else {
            return;
        };
        let Some(path) = request["path"].as_str().map(str::to_string) else {
            // ponytail: -PLOT writes files only; a printer job goes through PLOT.
            self.command_line.push_error(
                crate::t!("-PLOT sends to a file only; answer Yes to write the plot to a file.")
                    .as_ref(),
            );
            return;
        };
        match self.control_plot(&request) {
            Ok(_) => self
                .command_line
                .push_output(crate::tf!("Plot written to {}", path).as_ref()),
            Err(error) => self
                .command_line
                .push_error(error["error"].as_str().unwrap_or("plot failed")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};

    /// Script lines in the reference prompt order run unchanged.
    #[test]
    fn reference_script_lines_run() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        for line in [
            "-LAYER M SCANTEST C 1 SCANTEST",
            "-LAYER N TMP1,TMP2 OFF TMP1",
            "-PURGE LA TMP2 N",
            "-STYLE ROMANS romans.shx 0 1 0 N N N",
            "-RENAME LA TMP1 KEEP",
        ] {
            let _ = app.update(Message::ScriptLine(line.into()));
            assert!(app.tabs[app.active_tab].active_cmd.is_none(), "{line} still asks");
        }
        let doc = &app.tabs[app.active_tab].scene.document;
        let layer = doc.layers.get("SCANTEST").expect("-LAYER Make");
        assert_eq!(layer.color, codec::types::Color::Index(1));
        assert_eq!(doc.header.current_layer_name, "SCANTEST");
        assert!(doc.layers.get("KEEP").is_some_and(|l| l.flags.off));
        assert!(doc.layers.get("TMP2").is_none(), "-PURGE by name");
        assert_eq!(doc.text_styles.get("ROMANS").unwrap().font_file, "romans.shx");
        assert_eq!(doc.header.current_text_style_name, "ROMANS");
    }
}
