use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::colors::Color;
use crate::modules::{
    CargoScheme, CmdScheme, CwdScheme, ErrorMessageScheme, ExitCodeScheme, GitScheme, HostScheme,
    LastCmdDurationScheme, NvmScheme, PrScheme, PythonEnvScheme, ReadOnlyScheme, SdkmanScheme,
    ShellScheme, SpacerScheme, TimeScheme, UnknownScheme, UserScheme,
};
use crate::themes::{CompleteTheme, DefaultColors};

#[derive(Clone)]
pub struct CustomTheme;

static THEME: OnceLock<CustomThemeImpl> = OnceLock::new();

#[derive(Debug, Error)]
pub enum CustomThemeError {
    #[error("could not open theme file {}", path.display())]
    Open {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("theme file {} could not be parsed", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("theme file {} has invalid values: {details}", path.display())]
    Invalid { path: PathBuf, details: String },
}

#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum ColorsJson {
    Named(String),
    Code(u8),
}

#[derive(Deserialize)]
struct DefaultColorsJson {
    fg: ColorsJson,
    bg: ColorsJson,
}

#[derive(Deserialize)]
struct CustomThemeImpl {
    defaults: DefaultColorsJson,
    modules: HashMap<String, HashMap<String, Value>>,
}

impl CustomThemeImpl {
    fn get_property(&self, module: &str, property: &str) -> Option<&Value> {
        self.modules
            .get(module)
            .and_then(|module| module.get(property))
    }

    fn validate(&self) -> Result<(), String> {
        validate_color_json("defaults.fg", &self.defaults.fg)?;
        validate_color_json("defaults.bg", &self.defaults.bg)?;

        for (module, properties) in &self.modules {
            for (property, value) in properties {
                let path = format!("modules.{module}.{property}");
                match theme_property_kind(module, property) {
                    Some(ThemePropertyKind::Color) => validate_color_value(&path, value)?,
                    Some(ThemePropertyKind::ColorList) => validate_color_list(&path, value)?,
                    Some(ThemePropertyKind::String) => validate_string(&path, value)?,
                    None => {}
                }
            }
        }

        Ok(())
    }
}

impl CustomTheme {
    pub fn load(path: impl AsRef<Path>) -> Result<(), CustomThemeError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|source| CustomThemeError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        let theme: CustomThemeImpl =
            serde_json::from_reader(file).map_err(|source| CustomThemeError::Parse {
                path: path.to_path_buf(),
                source,
            })?;

        theme
            .validate()
            .map_err(|details| CustomThemeError::Invalid {
                path: path.to_path_buf(),
                details,
            })?;

        let _ = THEME.set(theme);
        Ok(())

        // todo: figure out why this is being set twice...
        // match THEME.set(theme) {
        //     Ok(()) => {
        //         println!("{:?} | finish custom theme load {x}", thread_id);
        //     }
        //     Err(e) => {
        //         println!("{:?} | failed to set custom theme? {:?}, {x}", thread_id, e);
        //     }
        // }
    }

    pub fn get_color(module: &str, color: &str) -> Option<Color> {
        let theme = THEME.get().expect("custom theme not set");
        theme.get_property(module, color).and_then(color_from_value)
    }

    pub fn get_colors(module: &str, property: &str) -> Option<Vec<Color>> {
        let theme = THEME.get().expect("custom theme not set");
        let value = theme.get_property(module, property);

        value.and_then(|value| {
            value
                .as_array()
                .and_then(|array| array.iter().map(color_from_value).collect())
        })
    }

    pub fn get_str(module: &str, property: &str) -> Option<String> {
        let theme = THEME.get().expect("custom theme not set");
        theme
            .get_property(module, property)
            .and_then(|value| value.as_str())
            .map(|s| s.to_string())
    }
}

impl DefaultColors for CustomTheme {
    fn default_bg() -> Color {
        let theme = THEME.get().expect("custom theme not set");
        color_from_json(&theme.defaults.bg).unwrap_or(Color(0))
    }

    fn default_fg() -> Color {
        let theme = THEME.get().expect("custom theme not set");
        color_from_json(&theme.defaults.fg).unwrap_or(Color(15))
    }
}

impl CompleteTheme for CustomTheme {}

macro_rules! color_from_json {
    ($function:ident, $module:ident, $property:ident, $default:ident) => {
        fn $function() -> Color {
            Self::get_color(stringify!($module), stringify!($property))
                .unwrap_or_else(Self::$default)
        }
    };
}

impl SdkmanScheme for CustomTheme {
    color_from_json!(sdkman_fg, sdkman, fg, default_fg);
    color_from_json!(sdkman_bg, sdkman, bg, default_bg);
}

impl NvmScheme for CustomTheme {
    color_from_json!(nvm_fg, nvm, fg, default_fg);
    color_from_json!(nvm_bg, nvm, bg, default_bg);
    color_from_json!(nvm_inactive_bg, nvm, inactive_bg, default_bg);
}

impl CargoScheme for CustomTheme {
    color_from_json!(cargo_fg, cargo, fg, default_fg);
    color_from_json!(cargo_bg, cargo, bg, default_bg);
}

impl ErrorMessageScheme for CustomTheme {
    color_from_json!(error_message_fg, error, fg, alert_fg);
    color_from_json!(error_message_bg, error, bg, alert_bg);
}

impl UnknownScheme for CustomTheme {
    color_from_json!(unknown_fg, unknown, fg, alert_fg);
    color_from_json!(unknown_bg, unknown, bg, alert_bg);
}

impl CmdScheme for CustomTheme {
    color_from_json!(cmd_passed_fg, cmd, passed_fg, default_fg);
    color_from_json!(cmd_passed_bg, cmd, passed_bg, default_bg);

    color_from_json!(cmd_failed_bg, cmd, failed_fg, default_fg);
    color_from_json!(cmd_failed_fg, cmd, failed_bg, default_bg);

    fn cmd_user_symbol() -> &'static str {
        Self::get_str("cmd", "user_symbol")
            .map(|str| str.leak() as &'static str)
            .unwrap_or(Self::DEFAULT_USER_SYMBOL)
    }
}

impl CwdScheme for CustomTheme {
    color_from_json!(path_fg, cwd, path_fg, default_fg);
    fn path_bg_colors() -> Vec<Color> {
        Self::get_colors("cwd", "bg_colors").unwrap_or(vec![Self::default_bg()])
    }
}

impl LastCmdDurationScheme for CustomTheme {
    color_from_json!(time_bg, last_cmd_duration, bg, default_bg);
    color_from_json!(time_fg, last_cmd_duration, fg, default_fg);

    fn time_icon() -> &'static str {
        Self::get_str("last_cmd_duration", "time_icon")
            .map(|str| str.leak() as &'static str)
            .unwrap_or(Self::DEFAULT_TIME_ICON)
    }
}

impl ExitCodeScheme for CustomTheme {
    color_from_json!(exit_code_bg, exit_code, bg, default_bg);
    color_from_json!(exit_code_fg, exit_code, fg, default_fg);
}

impl GitScheme for CustomTheme {
    color_from_json!(git_remote_bg, git, remote_bg, default_bg);
    color_from_json!(git_remote_fg, git, remote_fg, default_fg);
    color_from_json!(git_staged_bg, git, staged_bg, default_bg);
    color_from_json!(git_staged_fg, git, staged_fg, default_fg);
    color_from_json!(git_notstaged_bg, git, notstaged_bg, default_bg);
    color_from_json!(git_notstaged_fg, git, notstaged_fg, default_fg);
    color_from_json!(git_untracked_bg, git, untracked_bg, default_bg);
    color_from_json!(git_untracked_fg, git, untracked_fg, default_fg);
    color_from_json!(git_conflicted_bg, git, conflicted_bg, default_bg);
    color_from_json!(git_conflicted_fg, git, conflicted_fg, default_fg);
    color_from_json!(git_repo_clean_bg, git, clean_bg, default_bg);
    color_from_json!(git_repo_clean_fg, git, clean_fg, default_fg);
    color_from_json!(git_repo_dirty_bg, git, dirty_bg, default_bg);
    color_from_json!(git_repo_dirty_fg, git, dirty_fg, default_fg);
}

impl PrScheme for CustomTheme {
    color_from_json!(pr_draft_bg, pr, draft_bg, default_bg);
    color_from_json!(pr_draft_fg, pr, draft_fg, default_fg);
    color_from_json!(pr_open_bg, pr, open_bg, default_bg);
    color_from_json!(pr_open_fg, pr, open_fg, default_fg);
    color_from_json!(pr_merged_bg, pr, merged_bg, default_bg);
    color_from_json!(pr_merged_fg, pr, merged_fg, default_fg);
    color_from_json!(pr_closed_bg, pr, closed_bg, default_bg);
    color_from_json!(pr_closed_fg, pr, closed_fg, default_fg);

    color_from_json!(pr_status_success_fg, pr, status_success_fg, default_fg);
    color_from_json!(pr_status_failure_fg, pr, status_failure_fg, default_fg);
    color_from_json!(pr_status_pending_fg, pr, status_pending_fg, default_fg);

    fn pr_icon() -> &'static str {
        Self::get_str("pr", "icon")
            .map(|str| str.leak() as &'static str)
            .unwrap_or("\u{ea64}")
    }

    fn pr_status_icon() -> &'static str {
        Self::get_str("pr", "status_icon")
            .map(|str| str.leak() as &'static str)
            .unwrap_or("\u{25cf}")
    }
}

impl PythonEnvScheme for CustomTheme {
    color_from_json!(pyenv_fg, py, env_fg, default_fg);
    color_from_json!(pyenv_bg, py, env_bg, default_bg);

    color_from_json!(pyver_fg, py, version_fg, default_fg);
    color_from_json!(pyver_bg, py, version_bg, default_bg);
}

impl ReadOnlyScheme for CustomTheme {
    color_from_json!(readonly_fg, readonly, fg, default_fg);
    color_from_json!(readonly_bg, readonly, bg, default_bg);
}

impl SpacerScheme for CustomTheme {
    color_from_json!(color_fg, spacer, fg, default_fg);
    color_from_json!(color_bg, spacer, bg, default_bg);
}

impl HostScheme for CustomTheme {
    color_from_json!(hostname_bg, hostname, bg, default_bg);
    color_from_json!(hostname_fg, hostname, fg, default_fg);
}

impl ShellScheme for CustomTheme {
    color_from_json!(shellname_bg, shell, bg, default_bg);
    color_from_json!(shellname_fg, shell, fg, default_fg);
}

impl UserScheme for CustomTheme {
    color_from_json!(username_root_bg, username, root_bg, default_bg);
    color_from_json!(username_bg, username, bg, default_bg);
    color_from_json!(username_fg, username, fg, default_fg);
}

impl TimeScheme for CustomTheme {
    color_from_json!(time_bg, time, bg, default_bg);
    color_from_json!(time_fg, time, fg, default_fg);
}

#[derive(Clone, Copy)]
enum ThemePropertyKind {
    Color,
    ColorList,
    String,
}

fn theme_property_kind(module: &str, property: &str) -> Option<ThemePropertyKind> {
    match (module, property) {
        ("cmd", "user_symbol")
        | ("last_cmd_duration", "time_icon")
        | ("pr", "icon")
        | ("pr", "status_icon") => Some(ThemePropertyKind::String),
        ("cwd", "bg_colors") => Some(ThemePropertyKind::ColorList),
        (
            "cargo" | "error" | "exit_code" | "readonly" | "sdkman" | "spacer" | "time" | "unknown",
            "fg" | "bg",
        )
        | ("nvm", "fg" | "bg" | "inactive_bg")
        | ("cmd", "passed_fg" | "passed_bg" | "failed_fg" | "failed_bg")
        | ("cwd", "path_fg")
        | ("last_cmd_duration", "fg" | "bg")
        | (
            "git",
            "remote_bg" | "remote_fg" | "staged_bg" | "staged_fg" | "notstaged_bg" | "notstaged_fg"
            | "untracked_bg" | "untracked_fg" | "conflicted_bg" | "conflicted_fg" | "clean_bg"
            | "clean_fg" | "dirty_bg" | "dirty_fg",
        )
        | (
            "pr",
            "draft_bg" | "draft_fg" | "open_bg" | "open_fg" | "merged_bg" | "merged_fg"
            | "closed_bg" | "closed_fg" | "status_success_fg" | "status_failure_fg"
            | "status_pending_fg",
        )
        | ("py", "env_fg" | "env_bg" | "version_fg" | "version_bg")
        | ("hostname" | "shell", "fg" | "bg")
        | ("username", "root_bg" | "bg" | "fg") => Some(ThemePropertyKind::Color),
        _ => None,
    }
}

fn validate_color_value(path: &str, value: &Value) -> Result<(), String> {
    let color = serde_json::from_value::<ColorsJson>(value.to_owned())
        .map_err(|_| format!("expected color name or 0-255 color code at {path}"))?;
    validate_color_json(path, &color)
}

fn validate_color_list(path: &str, value: &Value) -> Result<(), String> {
    let colors = value
        .as_array()
        .ok_or_else(|| format!("expected an array of colors at {path}"))?;

    if colors.is_empty() {
        return Err(format!("expected at least one color at {path}"));
    }

    for (idx, color) in colors.iter().enumerate() {
        validate_color_value(&format!("{path}[{idx}]"), color)?;
    }

    Ok(())
}

fn validate_color_json(path: &str, color: &ColorsJson) -> Result<(), String> {
    match color {
        ColorsJson::Named(name) if Color::from_name(name).is_none() => {
            Err(format!("unknown color '{name}' at {path}"))
        }
        ColorsJson::Named(_) | ColorsJson::Code(_) => Ok(()),
    }
}

fn validate_string(path: &str, value: &Value) -> Result<(), String> {
    value
        .as_str()
        .map(|_| ())
        .ok_or_else(|| format!("expected string at {path}"))
}

fn color_from_value(value: &Value) -> Option<Color> {
    let color = serde_json::from_value::<ColorsJson>(value.to_owned()).ok()?;
    color_from_json(&color)
}

fn color_from_json(color: &ColorsJson) -> Option<Color> {
    match color {
        ColorsJson::Named(name) => Color::from_name(name),
        ColorsJson::Code(code) => Some(Color(*code)),
    }
}
