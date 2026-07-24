use std::time::Duration;

use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub const DEFAULT_GIT_STATUS_TIMEOUT_MS: u64 = 250;

pub trait TerminalRuntimeMetadata {
    fn shell_name(&self) -> String;
    fn total_columns(&self) -> usize;
    fn last_command_duration(&self) -> Option<Duration>;
    fn last_command_status(&self) -> &str;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub theme: String,
    pub rows: Vec<CommandLine>,
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
    },
    Pr {
        /// Append a coloured dot reflecting the PR's CI check status. On by
        /// default; set to `false` to show just the PR number.
        #[serde(default = "default_true")]
        status: bool,
    },
    PythonEnv,
    Nvm,
    Sdkman,
    Cargo,
    Host,
    Shell,
    Time {
        format: Option<String>,
    },
    User,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum KnownLineSegment {
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
    },
    Pr {
        #[serde(default = "default_true")]
        status: bool,
    },
    PythonEnv,
    Nvm,
    Sdkman,
    Cargo,
    Host,
    Shell,
    Time {
        format: Option<String>,
    },
    User,
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
            KnownLineSegment::Git { status_timeout_ms } => LineSegment::Git { status_timeout_ms },
            KnownLineSegment::Pr { status } => LineSegment::Pr { status },
            KnownLineSegment::PythonEnv => LineSegment::PythonEnv,
            KnownLineSegment::Nvm => LineSegment::Nvm,
            KnownLineSegment::Sdkman => LineSegment::Sdkman,
            KnownLineSegment::Cargo => LineSegment::Cargo,
            KnownLineSegment::Host => LineSegment::Host,
            KnownLineSegment::Shell => LineSegment::Shell,
            KnownLineSegment::Time { format } => LineSegment::Time { format },
            KnownLineSegment::User => LineSegment::User,
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

        // Preserve the original string shorthand while also accepting the
        // object form needed to configure the timeout.
        if value == Value::String("git".to_string()) {
            return Ok(LineSegment::Git {
                status_timeout_ms: DEFAULT_GIT_STATUS_TIMEOUT_MS,
            });
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
        "small_spacer"
            | "large_spacer"
            | "separator"
            | "cwd"
            | "read_only"
            | "git"
            | "pr"
            | "python_env"
            | "nvm"
            | "sdkman"
            | "cargo"
            | "host"
            | "shell"
            | "time"
            | "user"
            | "cmd"
            | "last_cmd_duration"
            | "padding"
            | "error"
            | "unknown"
    )
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
            rows: vec![
                CommandLine {
                    left: vec![
                        LineSegment::Padding(2),
                        LineSegment::Separator(SeparatorStyle::Round),
                        LineSegment::ReadOnly,
                        LineSegment::Cwd {
                            max_length: 60,
                            wanted_seg_num: 5,
                            resolve_symlinks: false,
                        },
                        LineSegment::Padding(2),
                        LineSegment::Git {
                            status_timeout_ms: DEFAULT_GIT_STATUS_TIMEOUT_MS,
                        },
                        LineSegment::Pr { status: true },
                    ],
                    right: Some(vec![]),
                },
                CommandLine {
                    left: vec![
                        LineSegment::Shell,
                        LineSegment::LastCmdDuration { min_run_time: 50 },
                        LineSegment::Cmd,
                        LineSegment::Padding(1),
                    ],
                    right: Some(vec![
                        LineSegment::Separator(SeparatorStyle::Round),
                        LineSegment::Nvm,
                        LineSegment::Sdkman,
                        LineSegment::PythonEnv,
                        LineSegment::Cargo,
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
    fn git_string_shorthand_uses_default_status_timeout() {
        let parsed: LineSegment =
            serde_json::from_str(r#""git""#).expect("git shorthand should parse");

        assert_eq!(
            parsed,
            LineSegment::Git {
                status_timeout_ms: DEFAULT_GIT_STATUS_TIMEOUT_MS,
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
