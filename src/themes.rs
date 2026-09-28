pub(crate) use custom::{color_code, infer_theme_property_kind, validate_theme, ThemePropertyKind};
pub use custom::{CustomTheme, CustomThemeError};

use crate::colors::Color;
use crate::modules::{
    BatteryScheme, CargoScheme, CmdScheme, CwdScheme, ErrorMessageScheme, ExitCodeScheme,
    GitScheme, HostScheme, JavaScheme, JobsScheme, KubernetesScheme, LastCmdDurationScheme,
    LocalIpScheme, MemoryUsageScheme, NodeScheme, OsScheme, PrScheme, PythonScheme, ReadOnlyScheme,
    ShellScheme, SpacerScheme, SudoScheme, TimeScheme, UnknownScheme, UsageScheme, UserScheme,
};
use crate::update::UpdateScheme;

mod custom;

/// The theme files bundled with superline, by the name a config uses for
/// them.
pub const BUILTIN_THEMES: &[(&str, &str)] = &[
    ("rainbow", include_str!("../themes/rainbow.json")),
    ("simple", include_str!("../themes/simple.json")),
];

/// The contents of the built-in theme with this name.
pub fn builtin_theme(name: &str) -> Option<&'static str> {
    BUILTIN_THEMES
        .iter()
        .find(|(builtin, _)| *builtin == name)
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
    fn builtin_themes_are_valid_theme_files() {
        for (name, text) in BUILTIN_THEMES {
            let value: serde_json::Value = serde_json::from_str(text)
                .unwrap_or_else(|e| panic!("themes/{name}.json does not parse: {e}"));
            validate_theme(&value).unwrap_or_else(|e| panic!("themes/{name}.json: {e}"));
        }
    }

    #[test]
    fn builtin_themes_are_found_by_name() {
        assert_eq!(builtin_theme("rainbow"), Some(BUILTIN_THEMES[0].1));
        assert_eq!(builtin_theme("simple"), Some(BUILTIN_THEMES[1].1));
        assert_eq!(builtin_theme("rainbow.json"), None);
    }
}
