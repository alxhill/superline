use std::env;
use std::marker::PhantomData;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::{DefaultPadding, Module};

pub struct ExitCode<S: ExitCodeScheme> {
    scheme: PhantomData<S>,
}

pub trait ExitCodeScheme: DefaultColors {
    fn exit_code_bg() -> Color {
        Self::default_bg()
    }
    fn exit_code_fg() -> Color {
        Self::default_fg()
    }
}

impl<S: ExitCodeScheme> Default for ExitCode<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: ExitCodeScheme> ExitCode<S> {
    pub fn new() -> ExitCode<S> {
        ExitCode {
            scheme: PhantomData,
        }
    }
}

impl<S: ExitCodeScheme> Module for ExitCode<S> {
    /// The last command's exit code, as the shell passed it.
    type Data = Option<String>;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn fetch(&self) -> Option<String> {
        env::args().nth(1)
    }

    fn sample(&self) -> Option<String> {
        Some("1".into())
    }

    fn render(&self, exit_code: Option<String>, powerline: &mut Powerline) {
        if let Some(exit_code) = shown_code(exit_code.as_deref()) {
            powerline.add_segment(
                exit_code,
                Style::simple(S::exit_code_fg(), S::exit_code_bg()),
            )
        }
    }
}

fn shown_code(exit_code: Option<&str>) -> Option<&str> {
    exit_code.filter(|code| *code != "0")
}

#[cfg(test)]
mod tests {
    use super::shown_code;

    #[test]
    fn hides_success_and_missing_codes() {
        assert_eq!(shown_code(Some("0")), None);
        assert_eq!(shown_code(None), None);
    }

    #[test]
    fn shows_failing_codes() {
        assert_eq!(shown_code(Some("1")), Some("1"));
    }
}
