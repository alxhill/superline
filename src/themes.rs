pub(crate) use custom::{color_code, infer_theme_property_kind, validate_theme, ThemePropertyKind};
pub(crate) use custom::{text_attribute_color, text_attribute_key, TEXT_ATTRIBUTES};
pub use custom::{CustomTheme, CustomThemeError};

use crate::colors::Color;
use crate::modules::{
    BatteryScheme, CargoScheme, ClaudeCodeScheme, CmdScheme, CwdScheme, ErrorMessageScheme,
    ExitCodeScheme, GitScheme, HostScheme, JavaScheme, JobsScheme, KubernetesScheme,
    LastCmdDurationScheme, LocalIpScheme, MemoryUsageScheme, NodeScheme, OsScheme, PrScheme,
    PythonScheme, ReadOnlyScheme, ShellScheme, SpacerScheme, SudoScheme, TimeScheme, UnknownScheme,
    UsageScheme, UserScheme,
};
use crate::update::UpdateScheme;
use std::path::{Path, PathBuf};

mod custom;

/// The themes built into superline, by the file name a config uses for them.
/// One is read from the binary unless its file is in the config directory.
pub(crate) const BUNDLED_THEMES: &[(&str, &str)] = &[
    ("rainbow.json", RAINBOW),
    ("simple.json", include_str!("../themes/simple.json")),
    ("gruvbox.json", include_str!("../themes/gruvbox.json")),
];

/// The bundled rainbow theme, also what a prompt falls back to when its config
/// or theme can't be loaded.
pub(crate) const RAINBOW: &str = include_str!("../themes/rainbow.json");

/// The theme file a config's `theme` value names. `.json` is optional, and a
/// path is relative to the config directory unless it starts with `/`.
pub fn theme_path(config_dir: &Path, theme: &str) -> PathBuf {
    let mut name = PathBuf::from(theme);
    if name.extension().is_none() {
        name.set_extension("json");
    }
    match theme.as_bytes() {
        [b'/', ..] => name,
        _ => config_dir.join(name),
    }
}

/// The bundled theme whose file in `config_dir` is `path`.
pub(crate) fn bundled_theme(config_dir: &Path, path: &Path) -> Option<&'static str> {
    BUNDLED_THEMES
        .iter()
        .find(|(file, _)| config_dir.join(file) == path)
        .map(|(_, text)| *text)
}

pub trait DefaultColors {
    fn default_bg() -> Color;
    fn default_fg() -> Color;

    fn secondary_bg() -> Color {
        Self::default_bg()
    }

    fn secondary_fg() -> Color {
        Self::default_fg()
    }

    fn alert_bg() -> Color {
        Self::default_bg()
    }

    fn alert_fg() -> Color {
        Self::default_fg()
    }
}

pub trait CompleteTheme:
    DefaultColors
    + BatteryScheme
    + CmdScheme
    + CwdScheme
    + LastCmdDurationScheme
    + ExitCodeScheme
    + GitScheme
    + PrScheme
    + PythonScheme
    + ReadOnlyScheme
    + SpacerScheme
    + HostScheme
    + JobsScheme
    + KubernetesScheme
    + LocalIpScheme
    + OsScheme
    + MemoryUsageScheme
    + SudoScheme
    + ShellScheme
    + UserScheme
    + CargoScheme
    + ClaudeCodeScheme
    + TimeScheme
    + UsageScheme
    + NodeScheme
    + JavaScheme
    + ErrorMessageScheme
    + UnknownScheme
    + UpdateScheme
{
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_themes_are_valid_theme_files() {
        for (file, text) in BUNDLED_THEMES {
            let value: serde_json::Value = serde_json::from_str(text)
                .unwrap_or_else(|e| panic!("themes/{file} does not parse: {e}"));
            validate_theme(&value).unwrap_or_else(|e| panic!("themes/{file}: {e}"));
        }
    }

    #[test]
    fn theme_names_resolve_with_or_without_json() {
        let dir = Path::new("/home/me/.config/superline");
        let rainbow = dir.join("rainbow.json");
        assert_eq!(theme_path(dir, "rainbow"), rainbow);
        assert_eq!(theme_path(dir, "rainbow.json"), rainbow);
        assert_eq!(
            theme_path(dir, "themes/ocean"),
            dir.join("themes/ocean.json")
        );
        assert_eq!(theme_path(dir, "ocean.theme"), dir.join("ocean.theme"));
        assert_eq!(theme_path(dir, "/etc/ocean"), Path::new("/etc/ocean.json"));
        assert_eq!(
            theme_path(dir, "/etc/ocean.json"),
            Path::new("/etc/ocean.json")
        );
        assert_eq!(theme_path(dir, ""), dir.join(""));
    }

    #[test]
    fn only_bundled_theme_files_in_the_config_dir_are_bundled() {
        let dir = Path::new("/home/me/.config/superline");
        assert_eq!(bundled_theme(dir, &dir.join("rainbow.json")), Some(RAINBOW));
        assert_eq!(
            bundled_theme(dir, &dir.join("simple.json")),
            Some(BUNDLED_THEMES[1].1)
        );
        assert_eq!(bundled_theme(dir, &dir.join("ocean.json")), None);
        assert_eq!(bundled_theme(dir, Path::new("/etc/rainbow.json")), None);
    }
}
