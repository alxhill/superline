use std::time::Duration;

use serde::{de, ser, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::claude_code::ClaudeCodeStatus;

pub const DEFAULT_GIT_STATUS_TIMEOUT_MS: u64 = 250;

pub trait TerminalRuntimeMetadata {
    fn shell_name(&self) -> String;
    fn total_columns(&self) -> usize;
    fn last_command_duration(&self) -> Option<Duration>;
    fn last_command_status(&self) -> &str;

    /// Number of background jobs reported by the interactive shell.
    fn job_count(&self) -> usize {
        0
    }

    /// The session data Claude Code passed to `superline claude-code`.
    fn claude_code(&self) -> Option<&ClaudeCodeStatus> {
        None
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub theme: String,
    pub rows: Vec<CommandLine>,
    /// Left out of the written default config: the check and automatic
    /// upgrades are on unless a config opts out.
    #[serde(default, skip_serializing_if = "UpdateConfig::is_default")]
    pub update: UpdateConfig,
}

/// The once-a-day check for a newer release, printed above the prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateConfig {
    /// Skip the check and never show the notice.
    #[serde(default)]
    pub disable: bool,
    /// Install a newer release in the background instead of only announcing
    /// it. Only prebuilt release binaries upgrade themselves.
    #[serde(default = "default_true")]
    pub auto: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        UpdateConfig {
            disable: false,
            auto: true,
        }
    }
}

impl UpdateConfig {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

// single line of a command terminal
#[derive(Debug, Serialize, Deserialize)]
pub struct CommandLine {
    pub left: Vec<Widget>,
    pub right: Option<Vec<Widget>>,
}

/// One entry of a row: the widget, plus the options every widget takes. These
/// are written among the widget's own options, as in
/// `{ "git": { "padding": "small" } }`.
#[derive(Debug, PartialEq)]
pub struct Widget {
    pub segment: LineSegment,
    /// Space around the text of the widget's segments, overriding the
    /// widget's own spacing.
    pub padding: Option<SegmentPadding>,
}

/// Where a segment gets a space beside its text. Written in the config as
/// its lowercase name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentPadding {
    /// No space on either side.
    Small,
    /// A space on each side.
    Large,
    /// A space before the text only.
    Left,
    /// A space after the text only.
    Right,
}

impl SegmentPadding {
    pub const ALL: [SegmentPadding; 4] = [
        SegmentPadding::Small,
        SegmentPadding::Large,
        SegmentPadding::Left,
        SegmentPadding::Right,
    ];

    /// How each value is written, in the order of [`SegmentPadding::ALL`].
    pub const NAMES: &'static [&'static str] = &["small", "large", "left", "right"];

    pub fn name(self) -> &'static str {
        match self {
            SegmentPadding::Small => "small",
            SegmentPadding::Large => "large",
            SegmentPadding::Left => "left",
            SegmentPadding::Right => "right",
        }
    }

    pub fn from_name(name: &str) -> Option<SegmentPadding> {
        SegmentPadding::ALL.into_iter().find(|p| p.name() == name)
    }
}

impl<'de> Deserialize<'de> for SegmentPadding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        value
            .as_str()
            .and_then(SegmentPadding::from_name)
            .ok_or_else(|| {
                de::Error::custom(format!(
                    "expected one of {}, got {value}",
                    SegmentPadding::NAMES.join(", ")
                ))
            })
    }
}

impl From<LineSegment> for Widget {
    fn from(segment: LineSegment) -> Self {
        Widget {
            segment,
            padding: None,
        }
    }
}

impl<'de> Deserialize<'de> for Widget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut value = Value::deserialize(deserializer)?;
        let padding = match options_mut(&mut value).and_then(|options| options.remove("padding")) {
            Some(padding) => Some(
                SegmentPadding::deserialize(padding)
                    .map_err(|e| de::Error::custom(format!("padding: {e}")))?,
            ),
            None => None,
        };
        let segment = LineSegment::deserialize(value).map_err(de::Error::custom)?;
        Ok(Widget { segment, padding })
    }
}

impl Serialize for Widget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let Some(padding) = self.padding else {
            return self.segment.serialize(serializer);
        };
        let mut value = serde_json::to_value(&self.segment).map_err(ser::Error::custom)?;
        if let Value::String(name) = &value {
            value = serde_json::json!({ name: {} });
        }
        options_mut(&mut value)
            .ok_or_else(|| ser::Error::custom("this widget takes no padding"))?
            .insert("padding".into(), padding.name().into());
        value.serialize(serializer)
    }
}

/// The options object of a segment written as `{ "name": { ... } }`.
fn options_mut(value: &mut Value) -> Option<&mut serde_json::Map<String, Value>> {
    match value {
        Value::Object(map) if map.len() == 1 => map.values_mut().next()?.as_object_mut(),
        _ => None,
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LineSegment {
    Battery,
    SmallSpacer,
    LargeSpacer,
    Separator(SeparatorStyle),
    Cwd {
        max_length: usize,
        wanted_seg_num: usize,
        #[serde(default)]
        resolve_symlinks: bool,
    },
    ReadOnly,
    Git {
        #[serde(default = "default_git_status_timeout_ms")]
        status_timeout_ms: u64,
        /// Which backend produces the status: `auto` (default) picks the CLI
        /// for large working trees and gitoxide for small ones, `cli` and
        /// `gitoxide` pin one backend.
        #[serde(default)]
        backend: GitBackend,
        /// Show the repository's linked-worktree count next to the branch, as
        /// `index/count` inside a linked worktree. On by default; nothing
        /// shows when there are none.
        #[serde(default = "default_true")]
        worktrees: bool,
    },
    Pr {
        /// Append a coloured dot reflecting the PR's CI check status. On by
        /// default; set to `false` to show just the PR number.
        #[serde(default = "default_true")]
        status: bool,
    },
    Python {
        /// Show the interpreter version. On by default: inside a virtual env
        /// it is read from the env's own files, and only envs without them ask
        /// the interpreter in the background and cache the answer.
        #[serde(default = "default_true")]
        version: bool,
        /// Show the active virtual env name. On by default.
        #[serde(default = "default_true")]
        venv: bool,
    },
    Node {
        /// Show the node version after the icon. On by default.
        #[serde(default = "default_true")]
        version: bool,
    },
    Java {
        /// Show the major java version. On by default.
        #[serde(default = "default_true")]
        version: bool,
        /// Show the JDK distribution (corretto, Temurin, ...). On by default.
        #[serde(default = "default_true")]
        jdk: bool,
    },
    Cargo {
        /// Show the mise-pinned toolchain version after the icon. On by default.
        #[serde(default = "default_true")]
        version: bool,
    },
    Kubernetes,
    Host,
    Hostname,
    Jobs,
    /// Show the primary non-loopback IPv4 address.
    LocalIp,
    /// Show used and total system memory, plus swap when it is available.
    MemoryUsage {
        /// Hide the segment below this percentage. With no threshold it is
        /// always shown.
        #[serde(default)]
        threshold: Option<u8>,
    },
    Os,
    Sudo,
    Shell,
    Time {
        format: Option<String>,
    },
    /// Add literal text to the prompt. Control characters are rendered as
    /// visible escapes so config values cannot inject terminal controls.
    Text(String),
    AiUsage {
        provider: UsageProvider,
        #[serde(default = "default_true")]
        session: bool,
        #[serde(default = "default_true")]
        weekly: bool,
        #[serde(default)]
        fable: bool,
        #[serde(default)]
        display: UsageDisplay,
        threshold: Option<f64>,
        session_label: Option<String>,
        weekly_label: Option<String>,
        fable_label: Option<String>,
        #[serde(default)]
        credits: bool,
        credits_display: Option<UsageDisplay>,
        credits_label: Option<String>,
        #[serde(default)]
        credits_only_when_limited: bool,
        #[serde(default)]
        session_time_remaining: bool,
        session_time_remaining_only_at_limit: f64,
    },
    /// Claude Code's model, with its effort level and fast mode. The
    /// `claude_*` widgets only draw in `superline claude-code`.
    ClaudeModel {
        /// Show the reasoning effort after the model. On by default.
        #[serde(default = "default_true")]
        effort: bool,
        /// Show an icon while fast mode is on. On by default.
        #[serde(default = "default_true")]
        fast_mode: bool,
    },
    /// How full the session's context window is.
    ClaudeContext {
        #[serde(default)]
        display: UsageDisplay,
        /// Also show the tokens used out of the window size.
        #[serde(default)]
        tokens: bool,
        /// Switch to the threshold background at this percentage.
        threshold: Option<f64>,
    },
    /// The session's estimated cost in USD.
    ClaudeCost,
    /// How long the session has been running.
    ClaudeDuration {
        /// Show the time spent waiting on the API instead.
        #[serde(default)]
        api: bool,
    },
    /// Lines added and removed this session.
    ClaudeLines,
    /// The prompt cache's hit ratio, on a background that shows whether it
    /// is still warm.
    ClaudeCache,
    /// The vim mode, while vim mode is on.
    ClaudeVim,
    /// The agent name, when running with `--agent`.
    ClaudeAgent,
    /// The session's name or title.
    ClaudeSession {
        /// Cut longer names short; 0 never cuts.
        #[serde(default = "default_session_max_length")]
        max_length: usize,
    },
    User,
    Username,
    Cmd,
    LastCmdDuration {
        min_run_time: u64, // milliseconds
    },
    Padding(usize),
    Error {
        message: String,
    },
    Unknown {
        name: String,
    },
}

fn default_true() -> bool {
    true
}

pub const DEFAULT_SESSION_MAX_LENGTH: usize = 30;

fn default_session_max_length() -> usize {
    DEFAULT_SESSION_MAX_LENGTH
}

fn default_git_status_timeout_ms() -> u64 {
    DEFAULT_GIT_STATUS_TIMEOUT_MS
}

fn deserialize_unit_interval<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = f64::deserialize(deserializer)?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(de::Error::custom("must be a finite number from 0 to 1"));
    }
    Ok(value)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum KnownLineSegment {
    Battery,
    SmallSpacer,
    LargeSpacer,
    Separator(SeparatorStyle),
    Cwd {
        max_length: usize,
        wanted_seg_num: usize,
        #[serde(default)]
        resolve_symlinks: bool,
    },
    ReadOnly,
    Git {
        #[serde(default = "default_git_status_timeout_ms")]
        status_timeout_ms: u64,
        #[serde(default)]
        backend: GitBackend,
        #[serde(default = "default_true")]
        worktrees: bool,
    },
    Pr {
        #[serde(default = "default_true")]
        status: bool,
    },
    /// Named `python_env` before the language modules were renamed; both
    /// names parse.
    #[serde(alias = "python_env")]
    Python {
        #[serde(default = "default_true")]
        version: bool,
        #[serde(default = "default_true")]
        venv: bool,
    },
    /// Named `nvm` before the language modules were renamed; both names parse.
    #[serde(alias = "nvm")]
    Node {
        #[serde(default = "default_true")]
        version: bool,
    },
    /// Named `sdkman` before it also read mise configs; both names parse.
    #[serde(alias = "sdkman")]
    Java {
        #[serde(default = "default_true")]
        version: bool,
        #[serde(default = "default_true")]
        jdk: bool,
    },
    Cargo {
        #[serde(default = "default_true")]
        version: bool,
    },
    Kubernetes,
    Host,
    Hostname,
    Jobs,
    #[serde(alias = "localip")]
    LocalIp,
    MemoryUsage {
        #[serde(default)]
        threshold: Option<u8>,
    },
    Os,
    Sudo,
    Shell,
    Time {
        format: Option<String>,
    },
    Text(String),
    AiUsage {
        provider: UsageProvider,
        #[serde(default = "default_true")]
        session: bool,
        #[serde(default = "default_true")]
        weekly: bool,
        #[serde(default)]
        fable: bool,
        #[serde(default)]
        display: UsageDisplay,
        threshold: Option<f64>,
        session_label: Option<String>,
        weekly_label: Option<String>,
        fable_label: Option<String>,
        #[serde(default)]
        credits: bool,
        credits_display: Option<UsageDisplay>,
        credits_label: Option<String>,
        #[serde(default)]
        credits_only_when_limited: bool,
        #[serde(default)]
        session_time_remaining: bool,
        #[serde(default, deserialize_with = "deserialize_unit_interval")]
        session_time_remaining_only_at_limit: f64,
    },
    ClaudeModel {
        #[serde(default = "default_true")]
        effort: bool,
        #[serde(default = "default_true")]
        fast_mode: bool,
    },
    ClaudeContext {
        #[serde(default)]
        display: UsageDisplay,
        #[serde(default)]
        tokens: bool,
        threshold: Option<f64>,
    },
    ClaudeCost,
    ClaudeDuration {
        #[serde(default)]
        api: bool,
    },
    ClaudeLines,
    ClaudeCache,
    ClaudeVim,
    ClaudeAgent,
    ClaudeSession {
        #[serde(default = "default_session_max_length")]
        max_length: usize,
    },
    User,
    Username,
    Cmd,
    LastCmdDuration {
        min_run_time: u64,
    },
    Padding(usize),
    Error {
        message: String,
    },
    Unknown {
        name: String,
    },
}

impl From<KnownLineSegment> for LineSegment {
    fn from(segment: KnownLineSegment) -> Self {
        match segment {
            KnownLineSegment::Battery => LineSegment::Battery,
            KnownLineSegment::SmallSpacer => LineSegment::SmallSpacer,
            KnownLineSegment::LargeSpacer => LineSegment::LargeSpacer,
            KnownLineSegment::Separator(style) => LineSegment::Separator(style),
            KnownLineSegment::Cwd {
                max_length,
                wanted_seg_num,
                resolve_symlinks,
            } => LineSegment::Cwd {
                max_length,
                wanted_seg_num,
                resolve_symlinks,
            },
            KnownLineSegment::ReadOnly => LineSegment::ReadOnly,
            KnownLineSegment::Git {
                status_timeout_ms,
                backend,
                worktrees,
            } => LineSegment::Git {
                status_timeout_ms,
                backend,
                worktrees,
            },
            KnownLineSegment::Pr { status } => LineSegment::Pr { status },
            KnownLineSegment::Python { version, venv } => LineSegment::Python { version, venv },
            KnownLineSegment::Node { version } => LineSegment::Node { version },
            KnownLineSegment::Java { version, jdk } => LineSegment::Java { version, jdk },
            KnownLineSegment::Cargo { version } => LineSegment::Cargo { version },
            KnownLineSegment::Kubernetes => LineSegment::Kubernetes,
            KnownLineSegment::Host => LineSegment::Host,
            KnownLineSegment::Hostname => LineSegment::Hostname,
            KnownLineSegment::Jobs => LineSegment::Jobs,
            KnownLineSegment::LocalIp => LineSegment::LocalIp,
            KnownLineSegment::MemoryUsage { threshold } => LineSegment::MemoryUsage { threshold },
            KnownLineSegment::Os => LineSegment::Os,
            KnownLineSegment::Sudo => LineSegment::Sudo,
            KnownLineSegment::Shell => LineSegment::Shell,
            KnownLineSegment::Time { format } => LineSegment::Time { format },
            KnownLineSegment::Text(text) => LineSegment::Text(text),
            KnownLineSegment::AiUsage {
                provider,
                session,
                weekly,
                fable,
                display,
                threshold,
                session_label,
                weekly_label,
                fable_label,
                credits,
                credits_display,
                credits_label,
                credits_only_when_limited,
                session_time_remaining,
                session_time_remaining_only_at_limit,
            } => LineSegment::AiUsage {
                provider,
                session,
                weekly,
                fable,
                display,
                threshold,
                session_label,
                weekly_label,
                fable_label,
                credits,
                credits_display,
                credits_label,
                credits_only_when_limited,
                session_time_remaining,
                session_time_remaining_only_at_limit,
            },
            KnownLineSegment::ClaudeModel { effort, fast_mode } => {
                LineSegment::ClaudeModel { effort, fast_mode }
            }
            KnownLineSegment::ClaudeContext {
                display,
                tokens,
                threshold,
            } => LineSegment::ClaudeContext {
                display,
                tokens,
                threshold,
            },
            KnownLineSegment::ClaudeCost => LineSegment::ClaudeCost,
            KnownLineSegment::ClaudeDuration { api } => LineSegment::ClaudeDuration { api },
            KnownLineSegment::ClaudeLines => LineSegment::ClaudeLines,
            KnownLineSegment::ClaudeCache => LineSegment::ClaudeCache,
            KnownLineSegment::ClaudeVim => LineSegment::ClaudeVim,
            KnownLineSegment::ClaudeAgent => LineSegment::ClaudeAgent,
            KnownLineSegment::ClaudeSession { max_length } => {
                LineSegment::ClaudeSession { max_length }
            }
            KnownLineSegment::User => LineSegment::User,
            KnownLineSegment::Username => LineSegment::Username,
            KnownLineSegment::Cmd => LineSegment::Cmd,
            KnownLineSegment::LastCmdDuration { min_run_time } => {
                LineSegment::LastCmdDuration { min_run_time }
            }
            KnownLineSegment::Padding(size) => LineSegment::Padding(size),
            KnownLineSegment::Error { message } => LineSegment::Error { message },
            KnownLineSegment::Unknown { name } => LineSegment::Unknown { name },
        }
    }
}

impl<'de> Deserialize<'de> for LineSegment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        // A bare `"git"` or `"java"` is shorthand for the object form with
        // every option at its default, so a segment can gain options without
        // breaking configs that name it as a plain string.
        if let Value::String(name) = &value {
            if is_known_segment_name(name) {
                let with_defaults = serde_json::json!({ name: {} });
                if let Ok(segment) = serde_json::from_value::<KnownLineSegment>(with_defaults) {
                    return Ok(segment.into());
                }
            }
        }

        // `{ "battery": {} }` is the object form of a segment without options.
        if let Value::Object(map) = &value {
            if let Some((name, Value::Object(options))) = map.iter().next() {
                if map.len() == 1 && options.is_empty() && is_known_segment_name(name) {
                    let name = Value::String(name.clone());
                    if let Ok(segment) = serde_json::from_value::<KnownLineSegment>(name) {
                        return Ok(segment.into());
                    }
                }
            }
        }

        match serde_json::from_value::<KnownLineSegment>(value.clone()) {
            Ok(segment) => Ok(segment.into()),
            Err(err) => match segment_name(&value) {
                Some(name) if !is_known_segment_name(&name) => Ok(LineSegment::Unknown { name }),
                Some(name) if name == "unknown" && value.is_string() => {
                    Ok(LineSegment::Unknown { name })
                }
                _ => Err(de::Error::custom(err)),
            },
        }
    }
}

fn segment_name(value: &Value) -> Option<String> {
    match value {
        Value::String(name) => Some(name.clone()),
        Value::Object(map) if map.len() == 1 => map.keys().next().cloned(),
        _ => None,
    }
}

/// Every name (and alias) a segment can be written with.
pub(crate) const KNOWN_SEGMENT_NAMES: &[&str] = &[
    "battery",
    "small_spacer",
    "large_spacer",
    "separator",
    "cwd",
    "read_only",
    "git",
    "pr",
    "python",
    "python_env",
    "node",
    "nvm",
    "java",
    "sdkman",
    "cargo",
    "kubernetes",
    "host",
    "hostname",
    "jobs",
    "local_ip",
    "localip",
    "memory_usage",
    "os",
    "sudo",
    "shell",
    "time",
    "text",
    "ai_usage",
    "user",
    "username",
    "claude_model",
    "claude_context",
    "claude_cost",
    "claude_duration",
    "claude_lines",
    "claude_cache",
    "claude_vim",
    "claude_agent",
    "claude_session",
    "cmd",
    "last_cmd_duration",
    "padding",
    "error",
    "unknown",
];

fn is_known_segment_name(name: &str) -> bool {
    KNOWN_SEGMENT_NAMES.contains(&name)
}

/// Which backend produces the `git` module's status. See `src/modules/git.rs`
/// for how `Auto` decides between the other two.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GitBackend {
    #[default]
    Auto,
    Cli,
    Gitoxide,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UsageProvider {
    Claude,
    Codex,
}

impl UsageProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UsageDisplay {
    #[default]
    #[serde(
        alias = "percent",
        alias = "percents",
        alias = "percentages",
        alias = "pct"
    )]
    Percentage,
    #[serde(alias = "bars")]
    Bar,
    #[serde(alias = "capped_bars", alias = "capped")]
    CappedBar,
    #[serde(alias = "blocks")]
    Block,
    #[serde(alias = "sparklines", alias = "spark", alias = "sparks")]
    Sparkline,
    #[serde(alias = "number", alias = "numbers", alias = "num")]
    Numeric,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SeparatorStyle {
    Chevron,
    Round,
    None,
}

fn widgets(segments: Vec<LineSegment>) -> Vec<Widget> {
    segments.into_iter().map(Widget::from).collect()
}

impl Config {
    /// The layout `superline claude-code` writes for a fresh install.
    pub fn claude_code_default() -> Config {
        Config {
            theme: "rainbow".into(),
            update: UpdateConfig::default(),
            rows: vec![CommandLine {
                left: widgets(vec![
                    LineSegment::Separator(SeparatorStyle::Round),
                    LineSegment::Padding(0),
                    LineSegment::ClaudeModel {
                        effort: true,
                        fast_mode: true,
                    },
                    LineSegment::ClaudeContext {
                        display: UsageDisplay::Percentage,
                        tokens: false,
                        threshold: Some(80.0),
                    },
                    LineSegment::Padding(1),
                    LineSegment::ReadOnly,
                    LineSegment::Cwd {
                        max_length: 40,
                        wanted_seg_num: 4,
                        resolve_symlinks: false,
                    },
                    LineSegment::Git {
                        status_timeout_ms: DEFAULT_GIT_STATUS_TIMEOUT_MS,
                        backend: GitBackend::Auto,
                        worktrees: true,
                    },
                    LineSegment::Pr { status: true },
                ]),
                right: Some(widgets(vec![
                    LineSegment::ClaudeLines,
                    LineSegment::ClaudeCost,
                    LineSegment::AiUsage {
                        provider: UsageProvider::Claude,
                        session: true,
                        weekly: true,
                        fable: false,
                        display: UsageDisplay::Percentage,
                        threshold: Some(90.0),
                        session_label: None,
                        weekly_label: None,
                        fable_label: None,
                        credits: false,
                        credits_display: None,
                        credits_label: None,
                        credits_only_when_limited: false,
                        session_time_remaining: false,
                        session_time_remaining_only_at_limit: 0.0,
                    },
                    LineSegment::Padding(0),
                ])),
            }],
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: "rainbow".into(),
            update: UpdateConfig::default(),
            rows: vec![
                CommandLine {
                    left: widgets(vec![
                        LineSegment::Padding(2),
                        LineSegment::Separator(SeparatorStyle::Round),
                        LineSegment::ReadOnly,
                        LineSegment::Cwd {
                            max_length: 60,
                            wanted_seg_num: 5,
                            resolve_symlinks: false,
                        },
                        LineSegment::Kubernetes,
                        LineSegment::Padding(2),
                        LineSegment::Git {
                            status_timeout_ms: DEFAULT_GIT_STATUS_TIMEOUT_MS,
                            backend: GitBackend::Auto,
                            worktrees: true,
                        },
                        LineSegment::Pr { status: true },
                        LineSegment::Padding(2),
                        LineSegment::AiUsage {
                            provider: UsageProvider::Claude,
                            session: true,
                            weekly: true,
                            fable: true,
                            display: UsageDisplay::Sparkline,
                            threshold: None,
                            session_label: None,
                            weekly_label: None,
                            fable_label: None,
                            credits: true,
                            credits_display: Some(UsageDisplay::Numeric),
                            credits_label: None,
                            credits_only_when_limited: true,
                            session_time_remaining: false,
                            session_time_remaining_only_at_limit: 0.0,
                        },
                        LineSegment::Padding(1),
                        LineSegment::AiUsage {
                            provider: UsageProvider::Codex,
                            session: true,
                            weekly: true,
                            fable: false,
                            display: UsageDisplay::Sparkline,
                            threshold: None,
                            session_label: None,
                            weekly_label: None,
                            fable_label: None,
                            credits: true,
                            credits_display: Some(UsageDisplay::Numeric),
                            credits_label: None,
                            credits_only_when_limited: true,
                            session_time_remaining: false,
                            session_time_remaining_only_at_limit: 0.0,
                        },
                    ]),
                    right: Some(widgets(vec![LineSegment::Sudo, LineSegment::Battery])),
                },
                CommandLine {
                    left: widgets(vec![
                        LineSegment::Shell,
                        LineSegment::LastCmdDuration { min_run_time: 50 },
                        LineSegment::Jobs,
                        LineSegment::Cmd,
                        LineSegment::Padding(1),
                    ]),
                    right: Some(widgets(vec![
                        LineSegment::Separator(SeparatorStyle::Round),
                        LineSegment::Node { version: true },
                        LineSegment::Java {
                            version: true,
                            jdk: true,
                        },
                        LineSegment::Python {
                            version: true,
                            venv: true,
                        },
                        LineSegment::Cargo { version: true },
                        LineSegment::Padding(0),
                    ])),
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The binary writes `Config::default()` to disk with `to_string_pretty`
    /// and then reads it back with `from_reader`. This guards that round-trip:
    /// the serialized default config must always deserialize into an equivalent
    /// `Config`, so a fresh install never ends up with an unparsable config.
    #[test]
    fn default_config_round_trips() {
        let default = Config::default();

        let json = serde_json::to_string_pretty(&default)
            .expect("default config should serialize to JSON");

        let parsed: Config =
            serde_json::from_str(&json).expect("serialized default config should parse back");

        // Compare via the canonical JSON form to confirm the round-trip is lossless.
        let reserialized = serde_json::to_string_pretty(&parsed)
            .expect("reparsed config should serialize to JSON");
        assert_eq!(json, reserialized);
    }

    #[test]
    fn default_claude_code_layout_round_trips() {
        let json = serde_json::to_string_pretty(&Config::claude_code_default()).unwrap();
        let parsed: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string_pretty(&parsed).unwrap());
        assert!(!json.contains("unknown"), "{json}");
    }

    #[test]
    fn claude_code_segments_parse_with_defaults_and_options() {
        let cases = [
            (
                r#""claude_model""#,
                LineSegment::ClaudeModel {
                    effort: true,
                    fast_mode: true,
                },
            ),
            (
                r#"{"claude_context":{"display":"bar","tokens":true,"threshold":75}}"#,
                LineSegment::ClaudeContext {
                    display: UsageDisplay::Bar,
                    tokens: true,
                    threshold: Some(75.0),
                },
            ),
            (
                r#""claude_context""#,
                LineSegment::ClaudeContext {
                    display: UsageDisplay::Percentage,
                    tokens: false,
                    threshold: None,
                },
            ),
            (r#""claude_cost""#, LineSegment::ClaudeCost),
            (
                r#"{"claude_duration":{"api":true}}"#,
                LineSegment::ClaudeDuration { api: true },
            ),
            (r#""claude_lines""#, LineSegment::ClaudeLines),
            (r#""claude_cache""#, LineSegment::ClaudeCache),
            (r#""claude_vim""#, LineSegment::ClaudeVim),
            (r#""claude_agent""#, LineSegment::ClaudeAgent),
            (
                r#""claude_session""#,
                LineSegment::ClaudeSession {
                    max_length: DEFAULT_SESSION_MAX_LENGTH,
                },
            ),
        ];
        for (json, expected) in cases {
            let parsed: LineSegment =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(parsed, expected, "{json}");
        }
    }

    #[test]
    fn git_string_shorthand_uses_default_status_timeout() {
        let parsed: LineSegment =
            serde_json::from_str(r#""git""#).expect("git shorthand should parse");

        assert_eq!(
            parsed,
            LineSegment::Git {
                status_timeout_ms: DEFAULT_GIT_STATUS_TIMEOUT_MS,
                backend: GitBackend::Auto,
                worktrees: true,
            }
        );
    }

    #[test]
    fn git_status_timeout_is_configurable() {
        let parsed: LineSegment = serde_json::from_str(r#"{"git":{"status_timeout_ms":250}}"#)
            .expect("configured git module should parse");

        assert_eq!(
            parsed,
            LineSegment::Git {
                status_timeout_ms: 250,
                backend: GitBackend::Auto,
                worktrees: true,
            }
        );
    }

    #[test]
    fn git_backend_is_configurable() {
        for (name, expected) in [
            ("auto", GitBackend::Auto),
            ("cli", GitBackend::Cli),
            ("gitoxide", GitBackend::Gitoxide),
        ] {
            let json = format!(r#"{{"git":{{"backend":"{name}"}}}}"#);
            let parsed: LineSegment =
                serde_json::from_str(&json).unwrap_or_else(|_| panic!("{json} should parse"));

            assert_eq!(
                parsed,
                LineSegment::Git {
                    status_timeout_ms: DEFAULT_GIT_STATUS_TIMEOUT_MS,
                    backend: expected,
                    worktrees: true,
                }
            );
        }
    }

    #[test]
    fn git_worktree_count_can_be_turned_off() {
        let parsed: LineSegment = serde_json::from_str(r#"{"git":{"worktrees":false}}"#)
            .expect("git module without worktrees should parse");

        assert_eq!(
            parsed,
            LineSegment::Git {
                status_timeout_ms: DEFAULT_GIT_STATUS_TIMEOUT_MS,
                backend: GitBackend::Auto,
                worktrees: false,
            }
        );
    }

    #[test]
    fn usage_defaults_to_both_windows_and_percentage() {
        let parsed: LineSegment = serde_json::from_str(r#"{"ai_usage":{"provider":"claude"}}"#)
            .expect("AI usage module should parse");

        assert_eq!(
            parsed,
            LineSegment::AiUsage {
                provider: UsageProvider::Claude,
                session: true,
                weekly: true,
                fable: false,
                display: UsageDisplay::Percentage,
                threshold: None,
                session_label: None,
                weekly_label: None,
                fable_label: None,
                credits: false,
                credits_display: None,
                credits_label: None,
                credits_only_when_limited: false,
                session_time_remaining: false,
                session_time_remaining_only_at_limit: 0.0,
            }
        );
    }

    #[test]
    fn usage_windows_and_display_are_configurable() {
        let parsed: LineSegment = serde_json::from_str(
            r#"{"ai_usage":{"provider":"codex","session":false,"weekly":true,"display":"sparkline","threshold":80,"session_label":"","weekly_label":"week","session_time_remaining":true,"session_time_remaining_only_at_limit":0.8}}"#,
        )
        .expect("configured AI usage module should parse");

        assert_eq!(
            parsed,
            LineSegment::AiUsage {
                provider: UsageProvider::Codex,
                session: false,
                weekly: true,
                fable: false,
                display: UsageDisplay::Sparkline,
                threshold: Some(80.0),
                session_label: Some(String::new()),
                weekly_label: Some("week".to_string()),
                fable_label: None,
                credits: false,
                credits_display: None,
                credits_label: None,
                credits_only_when_limited: false,
                session_time_remaining: true,
                session_time_remaining_only_at_limit: 0.8,
            }
        );
    }

    #[test]
    fn usage_session_reset_threshold_is_a_unit_interval() {
        for value in ["0", "1"] {
            let config = format!(
                r#"{{"ai_usage":{{"provider":"codex","session_time_remaining_only_at_limit":{value}}}}}"#
            );
            assert!(serde_json::from_str::<LineSegment>(&config).is_ok());
        }
        for value in ["-0.1", "1.1"] {
            let config = format!(
                r#"{{"ai_usage":{{"provider":"codex","session_time_remaining_only_at_limit":{value}}}}}"#
            );
            assert!(serde_json::from_str::<LineSegment>(&config).is_err());
        }
    }

    #[test]
    fn usage_fable_window_is_configurable() {
        let parsed: LineSegment = serde_json::from_str(
            r#"{"ai_usage":{"provider":"claude","fable":true,"fable_label":"fable "}}"#,
        )
        .expect("AI usage module with fable window should parse");

        assert_eq!(
            parsed,
            LineSegment::AiUsage {
                provider: UsageProvider::Claude,
                session: true,
                weekly: true,
                fable: true,
                display: UsageDisplay::Percentage,
                threshold: None,
                session_label: None,
                weekly_label: None,
                fable_label: Some("fable ".to_string()),
                credits: false,
                credits_display: None,
                credits_label: None,
                credits_only_when_limited: false,
                session_time_remaining: false,
                session_time_remaining_only_at_limit: 0.0,
            }
        );
    }

    #[test]
    fn usage_credits_window_is_configurable() {
        let parsed: LineSegment = serde_json::from_str(
            r#"{"ai_usage":{"provider":"codex","credits":true,"credits_display":"numeric","credits_label":"","credits_only_when_limited":true}}"#,
        )
        .expect("AI usage module with credits window should parse");

        assert_eq!(
            parsed,
            LineSegment::AiUsage {
                provider: UsageProvider::Codex,
                session: true,
                weekly: true,
                fable: false,
                display: UsageDisplay::Percentage,
                threshold: None,
                session_label: None,
                weekly_label: None,
                fable_label: None,
                credits: true,
                credits_display: Some(UsageDisplay::Numeric),
                credits_label: Some(String::new()),
                credits_only_when_limited: true,
                session_time_remaining: false,
                session_time_remaining_only_at_limit: 0.0,
            }
        );
    }

    #[test]
    fn usage_display_accepts_aliases() {
        let cases = [
            ("percentage", UsageDisplay::Percentage),
            ("percent", UsageDisplay::Percentage),
            ("percents", UsageDisplay::Percentage),
            ("percentages", UsageDisplay::Percentage),
            ("pct", UsageDisplay::Percentage),
            ("bar", UsageDisplay::Bar),
            ("bars", UsageDisplay::Bar),
            ("capped_bar", UsageDisplay::CappedBar),
            ("capped_bars", UsageDisplay::CappedBar),
            ("capped", UsageDisplay::CappedBar),
            ("block", UsageDisplay::Block),
            ("blocks", UsageDisplay::Block),
            ("sparkline", UsageDisplay::Sparkline),
            ("sparklines", UsageDisplay::Sparkline),
            ("spark", UsageDisplay::Sparkline),
            ("sparks", UsageDisplay::Sparkline),
            ("numeric", UsageDisplay::Numeric),
            ("number", UsageDisplay::Numeric),
            ("numbers", UsageDisplay::Numeric),
            ("num", UsageDisplay::Numeric),
        ];

        for (name, expected) in cases {
            let parsed: UsageDisplay = serde_json::from_str(&format!(r#""{name}""#))
                .unwrap_or_else(|_| panic!("{name} should parse as a usage display"));
            assert_eq!(parsed, expected, "{name}");
        }
    }

    #[test]
    fn java_segment_still_parses_under_its_old_sdkman_name() {
        for name in [r#""java""#, r#""sdkman""#] {
            let parsed: LineSegment =
                serde_json::from_str(name).unwrap_or_else(|_| panic!("{name} should parse"));

            assert_eq!(
                parsed,
                LineSegment::Java {
                    version: true,
                    jdk: true,
                }
            );
        }
    }

    #[test]
    fn java_version_and_jdk_are_configurable() {
        let parsed: LineSegment = serde_json::from_str(r#"{"java":{"jdk":false}}"#)
            .expect("configured java module should parse");
        assert_eq!(
            parsed,
            LineSegment::Java {
                version: true,
                jdk: false,
            }
        );

        let parsed: LineSegment =
            serde_json::from_str(r#"{"sdkman":{"version":false,"jdk":false}}"#)
                .expect("configured sdkman alias should parse");
        assert_eq!(
            parsed,
            LineSegment::Java {
                version: false,
                jdk: false,
            }
        );
    }

    #[test]
    fn old_language_module_names_still_parse() {
        let parsed: LineSegment = serde_json::from_str(r#""nvm""#).expect("nvm alias should parse");
        assert_eq!(parsed, LineSegment::Node { version: true });

        let parsed: LineSegment = serde_json::from_str(r#"{"nvm":{"version":false}}"#)
            .expect("configured nvm alias should parse");
        assert_eq!(parsed, LineSegment::Node { version: false });

        let parsed: LineSegment =
            serde_json::from_str(r#""python_env""#).expect("python_env alias should parse");
        assert_eq!(
            parsed,
            LineSegment::Python {
                version: true,
                venv: true,
            }
        );

        let parsed: LineSegment =
            serde_json::from_str(r#"{"python_env":{"version":false,"venv":false}}"#)
                .expect("configured python_env alias should parse");
        assert_eq!(
            parsed,
            LineSegment::Python {
                version: false,
                venv: false,
            }
        );
    }

    #[test]
    fn segments_with_options_parse_as_plain_strings_with_defaults() {
        let cases = [
            (r#""node""#, LineSegment::Node { version: true }),
            (
                r#""python""#,
                LineSegment::Python {
                    version: true,
                    venv: true,
                },
            ),
            (r#""cargo""#, LineSegment::Cargo { version: true }),
            (r#""pr""#, LineSegment::Pr { status: true }),
        ];

        for (json, expected) in cases {
            let parsed: LineSegment =
                serde_json::from_str(json).unwrap_or_else(|_| panic!("{json} should parse"));
            assert_eq!(parsed, expected, "{json}");
        }
    }

    #[test]
    fn update_notice_and_auto_upgrades_are_on_unless_disabled() {
        let config: Config = serde_json::from_str(r#"{"theme":"rainbow","rows":[]}"#)
            .expect("config without an update block should parse");
        assert_eq!(config.update, UpdateConfig::default());
        assert!(!config.update.disable);
        assert!(config.update.auto);

        let config: Config =
            serde_json::from_str(r#"{"theme":"rainbow","rows":[],"update":{"disable":true}}"#)
                .expect("config with the update block should parse");
        assert!(config.update.disable);
        assert!(config.update.auto);

        let config: Config =
            serde_json::from_str(r#"{"theme":"rainbow","rows":[],"update":{"auto":false}}"#)
                .expect("config opting out of auto upgrades should parse");
        assert!(!config.update.auto);
        assert!(!config.update.disable);

        let json = serde_json::to_string(&Config::default()).expect("default config serializes");
        assert!(!json.contains("update"), "{json}");
    }

    #[test]
    fn language_version_display_is_configurable() {
        let parsed: LineSegment = serde_json::from_str(r#"{"node":{"version":false}}"#)
            .expect("configured node module should parse");
        assert_eq!(parsed, LineSegment::Node { version: false });

        let parsed: LineSegment = serde_json::from_str(r#"{"cargo":{"version":false}}"#)
            .expect("configured cargo module should parse");
        assert_eq!(parsed, LineSegment::Cargo { version: false });

        let parsed: LineSegment =
            serde_json::from_str(r#"{"python":{"version":false,"venv":false}}"#)
                .expect("configured python module should parse");
        assert_eq!(
            parsed,
            LineSegment::Python {
                version: false,
                venv: false,
            }
        );
    }

    #[test]
    fn unknown_string_segment_deserializes_as_unknown_module() {
        let parsed: LineSegment =
            serde_json::from_str(r#""future_module""#).expect("unknown module should parse");

        assert_eq!(
            parsed,
            LineSegment::Unknown {
                name: "future_module".to_string()
            }
        );
    }

    #[test]
    fn unknown_object_segment_deserializes_as_unknown_module() {
        let parsed: LineSegment = serde_json::from_str(r#"{"future_module":{"enabled":true}}"#)
            .expect("unknown module with future config should parse");

        assert_eq!(
            parsed,
            LineSegment::Unknown {
                name: "future_module".to_string()
            }
        );
    }

    #[test]
    fn every_widget_takes_padding_among_its_options() {
        let cases = [
            (
                r#"{"git":{"padding":"small","backend":"cli"}}"#,
                LineSegment::Git {
                    status_timeout_ms: DEFAULT_GIT_STATUS_TIMEOUT_MS,
                    backend: GitBackend::Cli,
                    worktrees: true,
                },
                Some(SegmentPadding::Small),
            ),
            (
                r#"{"battery":{"padding":"large"}}"#,
                LineSegment::Battery,
                Some(SegmentPadding::Large),
            ),
            (
                r#"{"sdkman":{"padding":"left","jdk":false}}"#,
                LineSegment::Java {
                    version: true,
                    jdk: false,
                },
                Some(SegmentPadding::Left),
            ),
            (
                r#"{"cmd":{"padding":"right"}}"#,
                LineSegment::Cmd,
                Some(SegmentPadding::Right),
            ),
            (r#"{"battery":{}}"#, LineSegment::Battery, None),
            (r#""battery""#, LineSegment::Battery, None),
            (r#"{"padding":3}"#, LineSegment::Padding(3), None),
            (
                r#"{"future_module":{"padding":"left"}}"#,
                LineSegment::Unknown {
                    name: "future_module".to_string(),
                },
                Some(SegmentPadding::Left),
            ),
        ];

        for (json, segment, padding) in cases {
            let parsed: Widget =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("{json} should parse: {e}"));
            assert_eq!(parsed, Widget { segment, padding }, "{json}");
        }
    }

    #[test]
    fn padding_must_be_one_of_the_four_choices() {
        for json in [
            r#"{"git":{"padding":0}}"#,
            r#"{"git":{"padding":1}}"#,
            r#"{"git":{"padding":"Large"}}"#,
            r#"{"battery":{"padding":"wide"}}"#,
            r#"{"battery":{"padding":null}}"#,
        ] {
            let err = serde_json::from_str::<Widget>(json).expect_err(json);
            assert!(
                err.to_string()
                    .contains("padding: expected one of small, large, left, right"),
                "{json}: {err}"
            );
        }
        assert!(serde_json::from_str::<Widget>(r#"{"cwd":{"padding":"large"}}"#).is_err());
    }

    #[test]
    fn padding_round_trips() {
        for json in [
            r#"{"battery":{"padding":"small"}}"#,
            r#"{"git":{"status_timeout_ms":250,"backend":"auto","padding":"right"}}"#,
            r#""git""#,
        ] {
            let parsed: Widget = serde_json::from_str(json).unwrap();
            let written = serde_json::to_value(&parsed).unwrap();
            let reparsed: Widget = serde_json::from_value(written).unwrap();
            assert_eq!(parsed, reparsed, "{json}");
        }
        for padding in SegmentPadding::ALL {
            let written = serde_json::to_value(Widget {
                segment: LineSegment::Battery,
                padding: Some(padding),
            })
            .unwrap();
            assert_eq!(
                written,
                serde_json::json!({ "battery": { "padding": padding.name() } })
            );
        }
        assert_eq!(
            SegmentPadding::ALL.map(SegmentPadding::name),
            SegmentPadding::NAMES
        );
    }

    #[test]
    fn invalid_known_segment_still_fails_to_parse() {
        let err = serde_json::from_str::<LineSegment>(r#"{"padding":"wide"}"#)
            .expect_err("bad known module config should remain invalid");

        assert!(
            err.to_string().contains("invalid type"),
            "unexpected error: {err}",
        );
    }
}
