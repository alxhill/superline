pub use custom::{CustomTheme, CustomThemeError};
pub use rainbow::RainbowTheme;
pub use simple::SimpleTheme;

use crate::colors::Color;
use crate::modules::{
    CargoScheme, CmdScheme, CwdScheme, ErrorMessageScheme, ExitCodeScheme, GitScheme, HostScheme,
    JavaScheme, LastCmdDurationScheme, NvmScheme, PrScheme, PythonEnvScheme, ReadOnlyScheme,
    ShellScheme, SpacerScheme, TimeScheme, UnknownScheme, UsageScheme, UserScheme,
};

mod custom;
mod rainbow;
mod simple;

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
    + CmdScheme
    + CwdScheme
    + LastCmdDurationScheme
    + ExitCodeScheme
    + GitScheme
    + PrScheme
    + PythonEnvScheme
    + ReadOnlyScheme
    + SpacerScheme
    + HostScheme
    + ShellScheme
    + UserScheme
    + CargoScheme
    + TimeScheme
    + UsageScheme
    + NvmScheme
    + JavaScheme
    + ErrorMessageScheme
    + UnknownScheme
{
}
