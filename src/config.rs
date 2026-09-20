use std::time::Duration;

use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::Value;

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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub theme: String,
    pub rows: Vec<CommandLine>,
    /// Left out of the written default config: the check is on unless a
    /// config opts out.
    #[serde(default, skip_serializing_if = "UpdateConfig::is_default")]
    pub update: UpdateConfig,
}

/// The once-a-day check for a newer release, printed above the prompt.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateConfig {
    /// Skip the check and never show the notice.
    #[serde(default)]
    pub disable: bool,
}

impl UpdateConfig {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

// single line of a command terminal
#[derive(Debug, Serialize, Deserialize)]
pub struct CommandLine {
    pub left: Vec<LineSegment>,
    pub right: Option<Vec<LineSegment>>,
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
    Nats,
    Os,
    Shell,
    Time {
        format: Option<String>,
    },
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
    Nats,
    Os,
    Shell,
    Time {
        format: Option<String>,
    },
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
            } => LineSegment::Git {
                status_timeout_ms,
                backend,
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
            KnownLineSegment::Nats => LineSegment::Nats,
            KnownLineSegment::Os => LineSegment::Os,
            KnownLineSegment::Shell => LineSegment::Shell,
            KnownLineSegment::Time { format } => LineSegment::Time { format },
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

fn is_known_segment_name(name: &str) -> bool {
    matches!(
        name,
        "battery"
            | "small_spacer"
            | "large_spacer"
            | "separator"
            | "cwd"
            | "read_only"
            | "git"
            | "pr"
            | "python"
            | "python_env"
            | "node"
            | "nvm"
            | "java"
            | "sdkman"
            | "cargo"
            | "kubernetes"
            | "host"
            | "hostname"
            | "jobs"
            | "local_ip"
            | "localip"
            | "nats"
            | "os"
            | "shell"
            | "time"
            | "ai_usage"
            | "user"
            | "username"
            | "cmd"
            | "last_cmd_duration"
            | "padding"
            | "error"
            | "unknown"
    )
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
    AngleLine,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: "rainbow".into(),
            update: UpdateConfig::default(),
            rows: vec![
                CommandLine {
                    left: vec![
                        LineSegment::Padding(2),
                        LineSegment::Separator(SeparatorStyle::Round),
                        LineSegment::ReadOnly,
                        LineSegment::Username,
                        LineSegment::Hostname,
                        LineSegment::LocalIp,
                        LineSegment::Nats,
                        LineSegment::Os,
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
                        },
                        LineSegment::Pr { status: true },
                        LineSegment::Padding(2),
                        LineSegment::Battery,
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
                    ],
                    right: Some(vec![]),
                },
                CommandLine {
                    left: vec![
                        LineSegment::Shell,
                        LineSegment::LastCmdDuration { min_run_time: 50 },
                        LineSegment::Jobs,
                        LineSegment::Cmd,
                        LineSegment::Padding(1),
                    ],
                    right: Some(vec![
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
                    ]),
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
    fn battery_is_a_bare_segment() {
        let parsed: LineSegment =
            serde_json::from_str(r#""battery""#).expect("battery module should parse");

        assert_eq!(parsed, LineSegment::Battery);
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
            }
        );
    }

    #[test]
    fn jobs_string_shorthand_parses() {
        let parsed: LineSegment =
            serde_json::from_str(r#""jobs""#).expect("jobs shorthand should parse");

        assert_eq!(parsed, LineSegment::Jobs);
    }

    #[test]
    fn nats_string_shorthand_parses() {
        let parsed: LineSegment =
            serde_json::from_str(r#""nats""#).expect("nats shorthand should parse");

        assert_eq!(parsed, LineSegment::Nats);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), r#""nats""#);
    }

    #[test]
    fn os_string_shorthand_parses() {
        let parsed: LineSegment = serde_json::from_str(r#""os""#).expect("os module should parse");

        assert_eq!(parsed, LineSegment::Os);
    }

    #[test]
    fn default_config_includes_os() {
        assert!(Config::default().rows.iter().any(|row| {
            row.left
                .iter()
                .chain(row.right.iter().flatten())
                .any(|segment| matches!(segment, LineSegment::Os))
        }));
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
                }
            );
        }
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
    fn hostname_segment_uses_the_starship_aligned_name() {
        let parsed: LineSegment =
            serde_json::from_str(r#""hostname""#).expect("hostname segment should parse");

        assert_eq!(parsed, LineSegment::Hostname);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), r#""hostname""#);
    }

    #[test]
    fn host_segment_remains_a_compatibility_alias() {
        let parsed: LineSegment =
            serde_json::from_str(r#""host""#).expect("host segment should parse");

        assert_eq!(parsed, LineSegment::Host);
    }

    #[test]
    fn local_ip_segment_accepts_the_snake_case_name_and_starship_alias() {
        for name in ["local_ip", "localip"] {
            let parsed: LineSegment = serde_json::from_str(&format!(r#""{name}""#))
                .unwrap_or_else(|_| panic!("{name} segment should parse"));
            assert_eq!(parsed, LineSegment::LocalIp);
        }

        assert_eq!(
            serde_json::to_string(&LineSegment::LocalIp).unwrap(),
            r#""local_ip""#
        );
    }

    #[test]
    fn username_segment_uses_the_starship_aligned_name() {
        let parsed: LineSegment =
            serde_json::from_str(r#""username""#).expect("username segment should parse");

        assert_eq!(parsed, LineSegment::Username);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), r#""username""#);
    }

    #[test]
    fn user_segment_remains_a_compatibility_alias() {
        let parsed: LineSegment =
            serde_json::from_str(r#""user""#).expect("user segment should parse");

        assert_eq!(parsed, LineSegment::User);
    }

    #[test]
    fn default_config_includes_hostname() {
        assert!(Config::default().rows.iter().any(|row| {
            row.left
                .iter()
                .chain(row.right.iter().flatten())
                .any(|segment| matches!(segment, LineSegment::Hostname))
        }));
    }

    #[test]
    fn default_config_includes_local_ip() {
        assert!(Config::default().rows.iter().any(|row| {
            row.left
                .iter()
                .chain(row.right.iter().flatten())
                .any(|segment| matches!(segment, LineSegment::LocalIp))
        }));
    }

    #[test]
    fn default_config_includes_username() {
        assert!(Config::default().rows.iter().any(|row| {
            row.left
                .iter()
                .chain(row.right.iter().flatten())
                .any(|segment| matches!(segment, LineSegment::Username))
        }));
    }

    #[test]
    fn default_config_includes_nats() {
        assert!(Config::default().rows.iter().any(|row| {
            row.left
                .iter()
                .chain(row.right.iter().flatten())
                .any(|segment| matches!(segment, LineSegment::Nats))
        }));
    }

    #[test]
    fn update_notice_is_on_unless_disabled() {
        let config: Config = serde_json::from_str(r#"{"theme":"rainbow","rows":[]}"#)
            .expect("config without an update block should parse");
        assert!(!config.update.disable);

        let config: Config =
            serde_json::from_str(r#"{"theme":"rainbow","rows":[],"update":{"disable":true}}"#)
                .expect("config with the update block should parse");
        assert!(config.update.disable);

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
    fn invalid_known_segment_still_fails_to_parse() {
        let err = serde_json::from_str::<LineSegment>(r#"{"padding":"wide"}"#)
            .expect_err("bad known module config should remain invalid");

        assert!(
            err.to_string().contains("invalid type"),
            "unexpected error: {err}",
        );
    }
}
