pub use custom::{CustomTheme, CustomThemeError};
pub use rainbow::RainbowTheme;
pub use simple::SimpleTheme;

use crate::colors::Color;
use crate::modules::{
    BatteryScheme, CargoScheme, CmdScheme, CwdScheme, ErrorMessageScheme, ExitCodeScheme,
    GitScheme, HostScheme, JavaScheme, JobsScheme, KubernetesScheme, LastCmdDurationScheme,
    LocalIpScheme, NodeScheme, PrScheme, PythonScheme,
    ReadOnlyScheme, ShellScheme, SpacerScheme, TimeScheme, UnknownScheme, UsageScheme, UserScheme,
};
use crate::update::UpdateScheme;

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
