use std::marker::PhantomData;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::{DefaultPadding, Module};

pub struct Cmd<S: CmdScheme> {
    status: String,
    scheme: PhantomData<S>,
}

pub trait CmdScheme: DefaultColors {
    const DEFAULT_USER_SYMBOL: &'static str = "$";
    const DEFAULT_ROOT_SYMBOL: &'static str = "#";
    fn cmd_passed_fg() -> Color {
        Self::default_fg()
    }

    fn cmd_passed_bg() -> Color {
        Self::default_bg()
    }

    fn cmd_failed_bg() -> Color {
        Self::default_bg()
    }

    fn cmd_failed_fg() -> Color {
        Self::default_fg()
    }

    fn cmd_root_symbol() -> &'static str {
        Self::DEFAULT_ROOT_SYMBOL
    }

    fn cmd_user_symbol() -> &'static str {
        Self::DEFAULT_USER_SYMBOL
    }
}

impl<S: CmdScheme> Cmd<S> {
    pub fn new(status: &str) -> Cmd<S> {
        Cmd {
            status: status.into(),
            scheme: PhantomData,
        }
    }
}

/// The last command's exit status and whether the shell runs as root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdStatus {
    pub status: String,
    pub is_root: bool,
}

impl<S: CmdScheme> Module for Cmd<S> {
    type Data = CmdStatus;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Small.into()
    }

    fn fetch(&self) -> CmdStatus {
        CmdStatus {
            status: self.status.clone(),
            is_root: crate::platform::is_root(),
        }
    }

    fn sample(&self) -> CmdStatus {
        CmdStatus {
            status: "1".into(),
            is_root: false,
        }
    }

    fn render(&self, data: CmdStatus, powerline: &mut Powerline) {
        let (symbol, fg, bg) = segment::<S>(&data);
        powerline.add_segment(symbol, Style::simple(fg, bg));
    }
}

fn segment<S: CmdScheme>(data: &CmdStatus) -> (&str, Color, Color) {
    let user_symbol = if data.is_root {
        S::cmd_root_symbol()
    } else {
        S::cmd_user_symbol()
    };
    match data.status.as_ref() {
        "0" => (user_symbol, S::cmd_passed_fg(), S::cmd_passed_bg()),
        non_zero_code => (non_zero_code, S::cmd_failed_fg(), S::cmd_failed_bg()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colors::{black, green};

    struct TestTheme;

    impl DefaultColors for TestTheme {
        fn default_bg() -> Color {
            black()
        }

        fn default_fg() -> Color {
            green()
        }
    }

    impl CmdScheme for TestTheme {}

    fn symbol(status: &str, is_root: bool) -> String {
        let data = CmdStatus {
            status: status.into(),
            is_root,
        };
        segment::<TestTheme>(&data).0.to_owned()
    }

    #[test]
    fn success_shows_the_user_or_root_symbol() {
        assert_eq!(symbol("0", false), "$");
        assert_eq!(symbol("0", true), "#");
    }

    #[test]
    fn failure_shows_the_exit_code() {
        assert_eq!(symbol("127", false), "127");
    }
}
