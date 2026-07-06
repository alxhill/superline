use crate::colors::{black, dark_grey, grey, light_grey, Color};
use crate::modules::{
    CargoScheme, CmdScheme, CwdScheme, ExitCodeScheme, GitScheme, HostScheme,
    LastCmdDurationScheme, NvmScheme, PrScheme, PythonEnvScheme, ReadOnlyScheme, SdkmanScheme,
    ShellScheme, SpacerScheme, TimeScheme, UnknownScheme, UserScheme,
};
use crate::themes::{CompleteTheme, DefaultColors};

#[derive(Copy, Clone)]
pub struct SimpleTheme;

impl DefaultColors for SimpleTheme {
    fn default_bg() -> Color {
        black()
    }

    fn default_fg() -> Color {
        light_grey()
    }
}

impl CompleteTheme for SimpleTheme {}

impl ShellScheme for SimpleTheme {}

impl SdkmanScheme for SimpleTheme {}

impl UnknownScheme for SimpleTheme {
    fn unknown_fg() -> Color {
        Color(15)
    }

    fn unknown_bg() -> Color {
        Color(161)
    }
}

impl NvmScheme for SimpleTheme {}

impl CargoScheme for SimpleTheme {
    fn cargo_bg() -> Color {
        dark_grey()
    }
}

impl LastCmdDurationScheme for SimpleTheme {}

impl PythonEnvScheme for SimpleTheme {}

impl SpacerScheme for SimpleTheme {}

impl CmdScheme for SimpleTheme {
    fn cmd_passed_fg() -> Color {
        Color(15)
    }

    fn cmd_passed_bg() -> Color {
        Color(236)
    }

    fn cmd_failed_bg() -> Color {
        Color(161)
    }

    fn cmd_failed_fg() -> Color {
        Color(15)
    }
}

impl CwdScheme for SimpleTheme {
    fn path_bg_colors() -> Vec<Color> {
        vec![grey()]
    }
}

impl ExitCodeScheme for SimpleTheme {
    fn exit_code_bg() -> Color {
        Color(161)
    }
    fn exit_code_fg() -> Color {
        Color(15)
    }
}

impl UserScheme for SimpleTheme {
    fn username_root_bg() -> Color {
        Color(124)
    }
    fn username_bg() -> Color {
        Color(240)
    }
    fn username_fg() -> Color {
        Color(250)
    }
}

impl HostScheme for SimpleTheme {
    fn hostname_fg() -> Color {
        Color(250)
    }
    fn hostname_bg() -> Color {
        Color(238)
    }
}

impl ReadOnlyScheme for SimpleTheme {
    fn readonly_fg() -> Color {
        Color(254)
    }
    fn readonly_bg() -> Color {
        Color(124)
    }
}

impl TimeScheme for SimpleTheme {
    fn time_bg() -> Color {
        Color(238)
    }
    fn time_fg() -> Color {
        Color(250)
    }
}

impl PrScheme for SimpleTheme {
    fn pr_draft_bg() -> Color {
        Color(240)
    }
    fn pr_draft_fg() -> Color {
        Color(15)
    }
    fn pr_open_bg() -> Color {
        Color(28)
    }
    fn pr_open_fg() -> Color {
        Color(15)
    }
    fn pr_merged_bg() -> Color {
        Color(54)
    }
    fn pr_merged_fg() -> Color {
        Color(15)
    }
    fn pr_closed_bg() -> Color {
        Color(124)
    }
    fn pr_closed_fg() -> Color {
        Color(15)
    }

    fn pr_status_success_fg() -> Color {
        Color(40)
    }
    fn pr_status_failure_fg() -> Color {
        Color(161)
    }
    fn pr_status_pending_fg() -> Color {
        Color(178)
    }
}

impl GitScheme for SimpleTheme {
    fn git_remote_bg() -> Color {
        Color(240)
    }
    fn git_remote_fg() -> Color {
        Color(250)
    }
    fn git_staged_bg() -> Color {
        Color(22)
    }
    fn git_staged_fg() -> Color {
        Color(15)
    }
    fn git_notstaged_bg() -> Color {
        Color(130)
    }
    fn git_notstaged_fg() -> Color {
        Color(15)
    }
    fn git_untracked_bg() -> Color {
        Color(52)
    }
    fn git_untracked_fg() -> Color {
        Color(15)
    }
    fn git_conflicted_bg() -> Color {
        Color(9)
    }
    fn git_conflicted_fg() -> Color {
        Color(15)
    }
    fn git_repo_clean_bg() -> Color {
        Color(148)
    }
    fn git_repo_clean_fg() -> Color {
        Color(0)
    }
    fn git_repo_dirty_bg() -> Color {
        Color(161)
    }
    fn git_repo_dirty_fg() -> Color {
        Color(15)
    }
}
