use crate::colors::Color;
use crate::colors::*;
use crate::modules::{
    CargoScheme, CmdScheme, CwdScheme, ErrorMessageScheme, ExitCodeScheme, GitScheme, HostScheme,
    LastCmdDurationScheme, NvmScheme, PrScheme, PythonEnvScheme, ReadOnlyScheme, SdkmanScheme,
    ShellScheme, SpacerScheme, TimeScheme, UnknownScheme, UserScheme,
};
use crate::themes::{CompleteTheme, DefaultColors};

#[derive(Copy, Clone)]
pub struct RainbowTheme;

impl DefaultColors for RainbowTheme {
    fn default_bg() -> Color {
        black()
    }

    fn default_fg() -> Color {
        green()
    }
}

impl CompleteTheme for RainbowTheme {}

impl SdkmanScheme for RainbowTheme {
    fn sdkman_fg() -> Color {
        Color(160)
    }

    fn sdkman_bg() -> Color {
        white()
    }
}

impl ErrorMessageScheme for RainbowTheme {
    fn error_message_fg() -> Color {
        white()
    }

    fn error_message_bg() -> Color {
        warning_red()
    }
}

impl UnknownScheme for RainbowTheme {
    fn unknown_fg() -> Color {
        white()
    }

    fn unknown_bg() -> Color {
        warning_red()
    }
}

impl NvmScheme for RainbowTheme {
    fn nvm_fg() -> Color {
        white()
    }

    fn nvm_bg() -> Color {
        forest_green()
    }

    fn nvm_inactive_bg() -> Color {
        burgundy()
    }
}

impl TimeScheme for RainbowTheme {
    fn time_bg() -> Color {
        dark_grey()
    }
    fn time_fg() -> Color {
        mid_grey()
    }
}

impl CargoScheme for RainbowTheme {
    fn cargo_fg() -> Color {
        black()
    }

    fn cargo_bg() -> Color {
        burnt_orange()
    }
}

impl UserScheme for RainbowTheme {
    fn username_root_bg() -> Color {
        red()
    }
    fn username_bg() -> Color {
        black()
    }
    fn username_fg() -> Color {
        green()
    }
}

impl HostScheme for RainbowTheme {
    fn hostname_fg() -> Color {
        grey()
    }
    fn hostname_bg() -> Color {
        dark_grey()
    }
}

impl ShellScheme for RainbowTheme {}

impl ExitCodeScheme for RainbowTheme {
    fn exit_code_bg() -> Color {
        red()
    }
    fn exit_code_fg() -> Color {
        white()
    }
}

impl CmdScheme for RainbowTheme {
    fn cmd_passed_fg() -> Color {
        green()
    }

    fn cmd_passed_bg() -> Color {
        black()
    }

    fn cmd_failed_bg() -> Color {
        warning_red()
    }

    fn cmd_failed_fg() -> Color {
        white()
    }

    fn cmd_user_symbol() -> &'static str {
        "\u{f105}"
    }
}

impl CwdScheme for RainbowTheme {
    fn path_fg() -> Color {
        white()
    }

    fn path_bg_colors() -> Vec<Color> {
        vec![red(), orange(), yellow(), green(), blue(), nice_purple()]
    }
}

impl GitScheme for RainbowTheme {
    fn git_remote_bg() -> Color {
        mid_grey()
    }
    fn git_remote_fg() -> Color {
        light_grey()
    }
    fn git_staged_bg() -> Color {
        forest_green()
    }
    fn git_staged_fg() -> Color {
        white()
    }
    fn git_notstaged_bg() -> Color {
        mid_red()
    }
    fn git_notstaged_fg() -> Color {
        white()
    }
    fn git_untracked_bg() -> Color {
        warning_red()
    }
    fn git_untracked_fg() -> Color {
        white()
    }
    fn git_conflicted_bg() -> Color {
        light_red()
    }
    fn git_conflicted_fg() -> Color {
        white()
    }
    fn git_repo_clean_bg() -> Color {
        blue()
    }
    fn git_repo_clean_fg() -> Color {
        white()
    }
    fn git_repo_dirty_bg() -> Color {
        bright_orange()
    }
    fn git_repo_dirty_fg() -> Color {
        white()
    }
}

impl PrScheme for RainbowTheme {
    fn pr_draft_fg() -> Color {
        white()
    }
    fn pr_draft_bg() -> Color {
        mid_grey()
    }
    fn pr_open_fg() -> Color {
        white()
    }
    fn pr_open_bg() -> Color {
        forest_green()
    }
    fn pr_merged_fg() -> Color {
        white()
    }
    fn pr_merged_bg() -> Color {
        nice_purple()
    }
    fn pr_closed_fg() -> Color {
        white()
    }
    fn pr_closed_bg() -> Color {
        mid_red()
    }

    fn pr_status_success_fg() -> Color {
        light_green()
    }
    fn pr_status_failure_fg() -> Color {
        warning_red()
    }
    fn pr_status_pending_fg() -> Color {
        dark_yellow()
    }
}

impl ReadOnlyScheme for RainbowTheme {
    fn readonly_fg() -> Color {
        Color(254)
    }
    fn readonly_bg() -> Color {
        Color(124)
    }

    fn readonly_symbol() -> &'static str {
        "\u{f0221}"
    }
}

impl PythonEnvScheme for RainbowTheme {
    fn pyenv_fg() -> Color {
        dark_grey()
    }

    fn pyenv_bg() -> Color {
        light_green()
    }

    fn pyver_fg() -> Color {
        dark_grey()
    }

    fn pyver_bg() -> Color {
        mid_green()
    }
}

impl SpacerScheme for RainbowTheme {}

impl LastCmdDurationScheme for RainbowTheme {
    fn time_bg() -> Color {
        black()
    }

    fn time_fg() -> Color {
        green()
    }

    fn time_icon() -> &'static str {
        ""
    }
}
