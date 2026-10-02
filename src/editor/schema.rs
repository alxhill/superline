//! What the editor knows about each widget: its options, their types and
//! defaults, and the JSON a freshly added one starts with. Keep this in step
//! with `LineSegment` in `src/config.rs`.

use serde_json::{json, Value};

use crate::config::SegmentPadding;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Bool {
        default: bool,
    },
    Int {
        default: Option<i64>,
        min: i64,
        max: i64,
    },
    Float {
        default: Option<f64>,
        min: f64,
        max: f64,
    },
    Str {
        default: Option<&'static str>,
    },
    Choice {
        variants: &'static [&'static str],
        default: Option<&'static str>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptionSpec {
    pub key: &'static str,
    pub kind: Kind,
    /// The config fails to parse without it.
    pub required: bool,
    pub help: &'static str,
}

impl OptionSpec {
    /// The value used when the key is absent, if there is one.
    pub fn default_value(&self) -> Option<Value> {
        match self.kind {
            Kind::Bool { default } => Some(Value::Bool(default)),
            Kind::Int { default, .. } => default.map(Value::from),
            Kind::Float { default, .. } => default.map(Value::from),
            Kind::Str { default } => default.map(Value::from),
            Kind::Choice { default, .. } => default.map(Value::from),
        }
    }

    /// Whether removing the key is a meaningful edit.
    pub fn can_unset(&self) -> bool {
        !self.required
    }

    /// Parses typed text into a value for this option.
    pub fn parse(&self, text: &str) -> Result<Option<Value>, String> {
        let text_trimmed = text.trim();
        match self.kind {
            Kind::Int { min, max, .. } => {
                if text_trimmed.is_empty() {
                    return self.unset_or_required();
                }
                let n: i64 = text_trimmed
                    .parse()
                    .map_err(|_| format!("{} must be a whole number", self.key))?;
                if n < min || n > max {
                    return Err(format!("{} must be between {min} and {max}", self.key));
                }
                Ok(Some(Value::from(n)))
            }
            Kind::Float { min, max, .. } => {
                if text_trimmed.is_empty() {
                    return self.unset_or_required();
                }
                let n: f64 = text_trimmed
                    .parse()
                    .map_err(|_| format!("{} must be a number", self.key))?;
                if !n.is_finite() || n < min || n > max {
                    return Err(format!("{} must be between {min} and {max}", self.key));
                }
                Ok(Some(Value::from(n)))
            }
            // An empty string is a real value for labels, so strings are kept
            // verbatim. Unsetting has its own key.
            Kind::Str { .. } => Ok(Some(Value::from(text))),
            Kind::Bool { .. } => match text_trimmed {
                "true" => Ok(Some(Value::Bool(true))),
                "false" => Ok(Some(Value::Bool(false))),
                _ => Err(format!("{} must be true or false", self.key)),
            },
            Kind::Choice { variants, .. } => {
                if text_trimmed.is_empty() {
                    return self.unset_or_required();
                }
                if variants.contains(&text_trimmed) {
                    Ok(Some(Value::from(text_trimmed)))
                } else {
                    Err(format!(
                        "{} must be one of {}",
                        self.key,
                        variants.join(", ")
                    ))
                }
            }
        }
    }

    fn unset_or_required(&self) -> Result<Option<Value>, String> {
        if self.required {
            Err(format!("{} is required", self.key))
        } else {
            Ok(None)
        }
    }

    /// Whether the option is edited by typing rather than toggling.
    pub fn is_text(&self) -> bool {
        matches!(
            self.kind,
            Kind::Int { .. } | Kind::Float { .. } | Kind::Str { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// No options of its own: written as a bare string, or as an object when
    /// it sets [`WIDGET_PADDING`].
    Unit,
    /// Named options: `{ "name": { ... } }`, or a bare string when every
    /// option has a default.
    Object(&'static [OptionSpec]),
    /// A single unnamed value: `{ "name": value }`.
    Value(&'static OptionSpec),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub summary: &'static str,
    pub shape: Shape,
    /// Offered in the add-widget picker.
    pub listed: bool,
    /// Which status line the widget draws in.
    pub scope: Scope,
}

/// Where a widget has something to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Everywhere,
    /// Only the shell prompt: it reads the shell's state.
    Prompt,
    /// Only `superline claude-code`: it reads Claude Code's session data.
    ClaudeCode,
}

impl Scope {
    /// Whether a widget of this scope belongs in a layout for `target`.
    pub fn fits(self, target: Scope) -> bool {
        self == Scope::Everywhere || self == target
    }
}

impl WidgetSpec {
    pub fn options(&self) -> &'static [OptionSpec] {
        match self.shape {
            Shape::Unit => &[],
            Shape::Object(options) => options,
            Shape::Value(option) => std::slice::from_ref(option),
        }
    }

    /// Whether the widget takes [`WIDGET_PADDING`]. A single-value widget has
    /// nowhere to write it.
    pub fn takes_padding(&self) -> bool {
        !matches!(self.shape, Shape::Value(_))
    }

    /// The JSON a newly added widget starts with: required options get a
    /// sensible starting value, everything else is left to its default.
    pub fn template(&self) -> Value {
        match &self.shape {
            Shape::Unit => Value::from(self.name),
            Shape::Value(_) => match self.name {
                "separator" => json!({ "separator": "round" }),
                "padding" => json!({ "padding": 1 }),
                "text" => json!({ "text": "text" }),
                _ => Value::from(self.name),
            },
            Shape::Object(_) => match self.name {
                "cwd" => json!({ "cwd": { "max_length": 60, "wanted_seg_num": 5 } }),
                "last_cmd_duration" => json!({ "last_cmd_duration": { "min_run_time": 50 } }),
                "ai_usage" => json!({ "ai_usage": { "provider": "claude" } }),
                _ => Value::from(self.name),
            },
        }
    }
}

const fn opt(key: &'static str, kind: Kind, help: &'static str) -> OptionSpec {
    OptionSpec {
        key,
        kind,
        required: false,
        help,
    }
}

const fn required(key: &'static str, kind: Kind, help: &'static str) -> OptionSpec {
    OptionSpec {
        key,
        kind,
        required: true,
        help,
    }
}

const fn boolean(key: &'static str, default: bool, help: &'static str) -> OptionSpec {
    opt(key, Kind::Bool { default }, help)
}

const fn label(key: &'static str, help: &'static str) -> OptionSpec {
    opt(key, Kind::Str { default: None }, help)
}

const DISPLAYS: &[&str] = &[
    "percentage",
    "bar",
    "capped_bar",
    "block",
    "sparkline",
    "numeric",
];

const SEPARATOR_VALUE: OptionSpec = required(
    "style",
    Kind::Choice {
        variants: &["chevron", "round", "none"],
        default: None,
    },
    "Shape drawn between segments. Applies to every following segment on the same side.",
);

const PADDING_VALUE: OptionSpec = required(
    "width",
    Kind::Int {
        default: None,
        min: 0,
        max: 500,
    },
    "Gap width in cells. Ends the current block and clears the background.",
);

/// Taken by every widget that has named options (or none), after its own.
pub const WIDGET_PADDING: OptionSpec = opt(
    "padding",
    Kind::Choice {
        variants: SegmentPadding::NAMES,
        default: None,
    },
    "Space beside the widget's text: small for none, large for one on each side, left or right for one on that side only. Unset: the widget's own.",
);

const TEXT_VALUE: OptionSpec = required(
    "text",
    Kind::Str { default: None },
    "Literal text drawn in the theme's default colours.",
);

const CWD: &[OptionSpec] = &[
    required(
        "max_length",
        Kind::Int {
            default: None,
            min: 0,
            max: 10_000,
        },
        "Longest path to show, in characters.",
    ),
    required(
        "wanted_seg_num",
        Kind::Int {
            default: None,
            min: 0,
            max: 1_000,
        },
        "How many trailing path components to keep.",
    ),
    boolean(
        "resolve_symlinks",
        false,
        "Show the real path instead of the one you cd'd into.",
    ),
];

const GIT: &[OptionSpec] = &[
    opt(
        "status_timeout_ms",
        Kind::Int {
            default: Some(crate::config::DEFAULT_GIT_STATUS_TIMEOUT_MS as i64),
            min: 0,
            max: 600_000,
        },
        "How long to wait for git status before serving the cached result.",
    ),
    opt(
        "backend",
        Kind::Choice {
            variants: &["auto", "cli", "gitoxide"],
            default: Some("auto"),
        },
        "cli shells out to git, gitoxide is in-process, auto picks by repo size.",
    ),
    boolean(
        "worktrees",
        true,
        "Show the linked-worktree count by the branch, index/count inside one.",
    ),
];

const PR: &[OptionSpec] = &[boolean(
    "status",
    true,
    "Append a coloured dot showing the PR's CI check status.",
)];

const PYTHON: &[OptionSpec] = &[
    boolean("version", true, "Show the interpreter version."),
    boolean("venv", true, "Show the active virtual env name."),
];

const VERSION: &[OptionSpec] = &[boolean(
    "version",
    true,
    "Show the pinned version after the icon.",
)];

const JAVA: &[OptionSpec] = &[
    boolean("version", true, "Show the major Java version."),
    boolean(
        "jdk",
        true,
        "Show the JDK distribution (corretto, Temurin, ...).",
    ),
];

const MEMORY_USAGE: &[OptionSpec] = &[opt(
    "threshold",
    Kind::Int {
        default: None,
        min: 0,
        max: 100,
    },
    "Hide the segment below this percentage. Unset to always show it.",
)];

const TIME: &[OptionSpec] = &[opt(
    "format",
    Kind::Str {
        default: Some("%H:%M:%S"),
    },
    "strftime format string.",
)];

const LAST_CMD_DURATION: &[OptionSpec] = &[required(
    "min_run_time",
    Kind::Int {
        default: None,
        min: 0,
        max: i64::MAX,
    },
    "Only show commands that ran at least this many milliseconds.",
)];

const METER_WIDTH: OptionSpec = opt(
    "width",
    Kind::Int {
        default: Some(crate::config::DEFAULT_METER_WIDTH as i64),
        min: 1,
        max: crate::config::MAX_METER_WIDTH as i64,
    },
    "Cells in a bar, capped_bar or block display.",
);

const AI_USAGE: &[OptionSpec] = &[
    required(
        "provider",
        Kind::Choice {
            variants: &["claude", "codex"],
            default: None,
        },
        "Whose subscription usage to show.",
    ),
    boolean("session", true, "Show the five-hour rate-limit window."),
    boolean("weekly", true, "Show the seven-day rate-limit window."),
    boolean(
        "fable",
        false,
        "Show the weekly Fable window (Claude only).",
    ),
    opt(
        "display",
        Kind::Choice {
            variants: DISPLAYS,
            default: Some("percentage"),
        },
        "How each window is drawn.",
    ),
    METER_WIDTH,
    opt(
        "threshold",
        Kind::Float {
            default: None,
            min: 0.0,
            max: 100.0,
        },
        "Percent used at which the widget switches to the theme's threshold_bg.",
    ),
    label(
        "session_label",
        "Label before the session window (default \"5h \").",
    ),
    label(
        "weekly_label",
        "Label before the weekly window (default \" 7d \").",
    ),
    label(
        "fable_label",
        "Label before the Fable window (default \" F \").",
    ),
    boolean("credits", false, "Add a lane for usage credits."),
    opt(
        "credits_display",
        Kind::Choice {
            variants: DISPLAYS,
            default: None,
        },
        "How the credits lane is drawn. Follows display when unset.",
    ),
    label(
        "credits_label",
        "Label before the credits lane (default \" C \").",
    ),
    boolean(
        "credits_only_when_limited",
        false,
        "Only show credits once a window has hit 100%.",
    ),
    boolean(
        "session_time_remaining",
        false,
        "Show how long until the session window resets.",
    ),
    opt(
        "session_time_remaining_only_at_limit",
        Kind::Float {
            default: Some(0.0),
            min: 0.0,
            max: 1.0,
        },
        "Only show the countdown once the session is this full (0 to 1).",
    ),
    boolean(
        "hover",
        true,
        "In iTerm2, hovering over the icon shows every window's percentage.",
    ),
];

const fn widget(name: &'static str, summary: &'static str, shape: Shape) -> WidgetSpec {
    WidgetSpec {
        name,
        aliases: &[],
        summary,
        shape,
        listed: true,
        scope: Scope::Everywhere,
    }
}

const fn prompt_widget(name: &'static str, summary: &'static str, shape: Shape) -> WidgetSpec {
    WidgetSpec {
        scope: Scope::Prompt,
        ..widget(name, summary, shape)
    }
}

const fn claude_widget(name: &'static str, summary: &'static str, shape: Shape) -> WidgetSpec {
    WidgetSpec {
        scope: Scope::ClaudeCode,
        ..widget(name, summary, shape)
    }
}

const CLAUDE_MODEL: &[OptionSpec] = &[
    boolean("effort", true, "Show the reasoning effort after the model."),
    boolean("fast_mode", true, "Show an icon while fast mode is on."),
];

const CLAUDE_CONTEXT: &[OptionSpec] = &[
    opt(
        "display",
        Kind::Choice {
            variants: DISPLAYS,
            default: Some("percentage"),
        },
        "How the share of the context window in use is drawn.",
    ),
    METER_WIDTH,
    boolean(
        "tokens",
        false,
        "Also show the tokens in use out of the window size.",
    ),
    opt(
        "threshold",
        Kind::Float {
            default: None,
            min: 0.0,
            max: 100.0,
        },
        "Percent used at which the widget switches to the theme's threshold_bg.",
    ),
];

const CLAUDE_DURATION: &[OptionSpec] = &[boolean(
    "api",
    false,
    "Show the time spent waiting on the API instead of the session's wall-clock time.",
)];

const CLAUDE_SESSION: &[OptionSpec] = &[opt(
    "max_length",
    Kind::Int {
        default: Some(crate::config::DEFAULT_SESSION_MAX_LENGTH as i64),
        min: 0,
        max: 500,
    },
    "Cut longer names short with an ellipsis. 0 never cuts.",
)];

pub const WIDGETS: &[WidgetSpec] = &[
    widget(
        "cwd",
        "Current working directory, shortened to fit.",
        Shape::Object(CWD),
    ),
    widget(
        "read_only",
        "Lock icon when the directory is not writable.",
        Shape::Unit,
    ),
    widget(
        "git",
        "Branch, working-tree status and ahead/behind counts.",
        Shape::Object(GIT),
    ),
    widget(
        "pr",
        "Link to the GitHub pull request for the branch.",
        Shape::Object(PR),
    ),
    widget(
        "kubernetes",
        "Active Kubernetes context and namespace.",
        Shape::Unit,
    ),
    widget(
        "ai_usage",
        "Claude or Codex subscription usage.",
        Shape::Object(AI_USAGE),
    ),
    prompt_widget(
        "cmd",
        "Prompt character; shows the exit code on failure.",
        Shape::Unit,
    ),
    prompt_widget(
        "last_cmd_duration",
        "How long the previous command took.",
        Shape::Object(LAST_CMD_DURATION),
    ),
    prompt_widget("shell", "Name of the running shell.", Shape::Unit),
    prompt_widget("jobs", "Number of running background jobs.", Shape::Unit),
    WidgetSpec {
        aliases: &["host"],
        ..widget("hostname", "The machine's hostname.", Shape::Unit)
    },
    WidgetSpec {
        aliases: &["user"],
        ..widget("username", "The current username.", Shape::Unit)
    },
    WidgetSpec {
        aliases: &["localip"],
        ..widget(
            "local_ip",
            "Primary non-loopback IPv4 address.",
            Shape::Unit,
        )
    },
    widget(
        "memory_usage",
        "Used RAM (and swap) percentage.",
        Shape::Object(MEMORY_USAGE),
    ),
    claude_widget(
        "claude_model",
        "Claude Code's model, effort level and fast mode.",
        Shape::Object(CLAUDE_MODEL),
    ),
    claude_widget(
        "claude_context",
        "How full Claude Code's context window is.",
        Shape::Object(CLAUDE_CONTEXT),
    ),
    claude_widget(
        "claude_cost",
        "The Claude Code session's estimated cost.",
        Shape::Unit,
    ),
    claude_widget(
        "claude_duration",
        "How long the Claude Code session has run.",
        Shape::Object(CLAUDE_DURATION),
    ),
    claude_widget(
        "claude_lines",
        "Lines added and removed this Claude Code session.",
        Shape::Unit,
    ),
    claude_widget(
        "claude_cache",
        "Prompt cache hit ratio, coloured by whether it is warm.",
        Shape::Unit,
    ),
    claude_widget(
        "claude_vim",
        "Claude Code's vim mode, while it is on.",
        Shape::Unit,
    ),
    claude_widget(
        "claude_agent",
        "The agent Claude Code runs as (--agent).",
        Shape::Unit,
    ),
    claude_widget(
        "claude_session",
        "The Claude Code session's name or title.",
        Shape::Object(CLAUDE_SESSION),
    ),
    widget("os", "Operating-system icon.", Shape::Unit),
    widget(
        "battery",
        "Low-battery warning at 10% or below.",
        Shape::Unit,
    ),
    widget(
        "sudo",
        "Marker while sudo credentials are cached.",
        Shape::Unit,
    ),
    widget("time", "The current time.", Shape::Object(TIME)),
    widget("text", "Literal text.", Shape::Value(&TEXT_VALUE)),
    WidgetSpec {
        aliases: &["python_env"],
        ..widget(
            "python",
            "Python version and virtual env.",
            Shape::Object(PYTHON),
        )
    },
    WidgetSpec {
        aliases: &["nvm"],
        ..widget("node", "Node.js version.", Shape::Object(VERSION))
    },
    WidgetSpec {
        aliases: &["sdkman"],
        ..widget("java", "Java version and JDK.", Shape::Object(JAVA))
    },
    widget(
        "cargo",
        "Rust toolchain for a Cargo project.",
        Shape::Object(VERSION),
    ),
    widget(
        "separator",
        "Layout: change the separator shape.",
        Shape::Value(&SEPARATOR_VALUE),
    ),
    widget(
        "padding",
        "Layout: end the block with a gap.",
        Shape::Value(&PADDING_VALUE),
    ),
    widget(
        "small_spacer",
        "Layout: a small blank segment.",
        Shape::Unit,
    ),
    widget(
        "large_spacer",
        "Layout: a large blank segment.",
        Shape::Unit,
    ),
    WidgetSpec {
        listed: false,
        ..widget(
            "error",
            "An error message.",
            Shape::Object(&[required(
                "message",
                Kind::Str { default: None },
                "Text of the error.",
            )]),
        )
    },
];

pub fn find(name: &str) -> Option<&'static WidgetSpec> {
    WIDGETS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))
}

/// The segment's widget name, whether written as `"name"` or `{ "name": ... }`.
pub fn segment_name(segment: &Value) -> Option<&str> {
    match segment {
        Value::String(name) => Some(name),
        Value::Object(map) if map.len() == 1 => map.keys().next().map(String::as_str),
        _ => None,
    }
}

/// Options of the global settings entry, keyed by their path under the root.
pub const THEME: OptionSpec = required(
    "theme",
    Kind::Str { default: None },
    "Theme file, relative to the config directory (.json optional). ⏎ picks one with a preview; \
     rainbow, simple and gruvbox are created on first use.",
);

pub const UPDATE: &[OptionSpec] = &[
    boolean(
        "disable",
        false,
        "Skip the daily release check and never show the update notice.",
    ),
    boolean(
        "auto",
        true,
        "Install newer releases in the background instead of only announcing them.",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LineSegment;

    #[test]
    fn every_template_parses() {
        for spec in WIDGETS.iter().filter(|spec| spec.listed) {
            let template = spec.template();
            serde_json::from_value::<LineSegment>(template.clone())
                .unwrap_or_else(|e| panic!("{} template {template} failed: {e}", spec.name));
        }
    }

    #[test]
    fn every_template_is_a_known_segment() {
        for spec in WIDGETS.iter().filter(|spec| spec.listed) {
            let parsed: LineSegment = serde_json::from_value(spec.template()).unwrap();
            assert!(
                !matches!(parsed, LineSegment::Unknown { .. }),
                "{} parsed as unknown",
                spec.name
            );
        }
    }

    #[test]
    fn every_config_segment_name_has_a_spec() {
        for name in crate::config::KNOWN_SEGMENT_NAMES {
            if *name != "unknown" {
                assert!(find(name).is_some(), "{name} is missing from WIDGETS");
            }
        }
    }

    #[test]
    fn aliases_resolve() {
        assert_eq!(find("host").unwrap().name, "hostname");
        assert_eq!(find("sdkman").unwrap().name, "java");
        assert!(find("future_widget").is_none());
    }

    #[test]
    fn padding_is_a_choice_of_the_values_the_config_takes() {
        assert_eq!(
            WIDGET_PADDING.kind,
            Kind::Choice {
                variants: &["small", "large", "left", "right"],
                default: None,
            }
        );
        assert_eq!(WIDGET_PADDING.parse("left"), Ok(Some(json!("left"))));
        assert_eq!(WIDGET_PADDING.parse(""), Ok(None));
        assert!(WIDGET_PADDING.parse("2").is_err());
    }

    #[test]
    fn padding_is_offered_to_every_widget_with_an_options_object() {
        assert!(find("battery").unwrap().takes_padding());
        assert!(find("git").unwrap().takes_padding());
        assert!(!find("text").unwrap().takes_padding());
        assert!(!find("padding").unwrap().takes_padding());
        for spec in WIDGETS {
            assert!(
                spec.options().iter().all(|o| o.key != WIDGET_PADDING.key),
                "{} lists padding itself",
                spec.name
            );
        }
    }

    /// The rows of the config reference's table of the padding each widget
    /// draws with when its `padding` is unset, as the widgets each row names
    /// and the padding with its markup removed. A row naming no widgets is
    /// for every other widget.
    fn documented_default_paddings() -> Vec<(Vec<&'static str>, String)> {
        let page = include_str!("../../site/config.html");
        let (_, table) = page
            .split_once(r#"<table class="opts" id="padding-defaults">"#)
            .expect("config.html has the padding defaults table");
        let (table, _) = table.split_once("</table>").unwrap();
        let (_, body) = table.split_once("<tbody>").unwrap();
        body.split("</tr>")
            .filter_map(|row| {
                let mut cells = row.split("<td>").skip(1);
                let (widgets, padding) = (cells.next()?, cells.next()?);
                let widgets = widgets
                    .split("<code>")
                    .skip(1)
                    .filter_map(|code| Some(code.split_once("</code>")?.0))
                    .collect();
                let mut text = String::new();
                let mut in_tag = false;
                for c in padding.chars() {
                    match c {
                        '<' => in_tag = true,
                        '>' => in_tag = false,
                        c if !in_tag => text.push(c),
                        _ => {}
                    }
                }
                Some((widgets, text.trim().to_string()))
            })
            .collect()
    }

    #[test]
    fn the_config_reference_lists_the_padding_every_widget_declares() {
        use crate::config::Widget;
        use crate::powerline::default_padding;

        let rows = documented_default_paddings();
        let documented = |name: &str| {
            let named = rows.iter().find(|(widgets, _)| widgets.contains(&name));
            let other = rows.iter().find(|(widgets, _)| widgets.is_empty());
            named.or(other).map(|(_, padding)| padding.as_str())
        };
        for (widgets, _) in &rows {
            for name in widgets {
                assert!(
                    find(name).is_some_and(WidgetSpec::takes_padding),
                    "config.html lists the padding of {name}, which takes none"
                );
            }
        }
        for spec in WIDGETS {
            let Ok(widget) = serde_json::from_value::<Widget>(spec.template()) else {
                assert!(!spec.listed, "{} template parses", spec.name);
                continue;
            };
            let layout = matches!(spec.name, "separator" | "padding");
            let declared = default_padding(&widget);
            assert_eq!(
                declared.is_none(),
                layout,
                "{} declares a padding",
                spec.name
            );
            if spec.listed && spec.takes_padding() {
                assert_eq!(
                    documented(spec.name),
                    declared.map(|padding| padding.to_string()).as_deref(),
                    "{}'s padding in config.html",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn claude_code_widgets_are_only_offered_for_claude_code_layouts() {
        for spec in WIDGETS {
            let claude = spec.name.starts_with("claude_");
            assert_eq!(spec.scope == Scope::ClaudeCode, claude, "{}", spec.name);
        }
        let model = find("claude_model").unwrap().scope;
        assert!(model.fits(Scope::ClaudeCode) && !model.fits(Scope::Prompt));
        let cmd = find("cmd").unwrap().scope;
        assert!(cmd.fits(Scope::Prompt) && !cmd.fits(Scope::ClaudeCode));
        let git = find("git").unwrap().scope;
        assert!(git.fits(Scope::Prompt) && git.fits(Scope::ClaudeCode));
    }

    #[test]
    fn value_widgets_expose_their_option() {
        assert_eq!(find("padding").unwrap().options()[0].key, "width");
        assert_eq!(find("battery").unwrap().options().len(), 0);
        assert_eq!(find("git").unwrap().options().len(), 3);
    }
}
