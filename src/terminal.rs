use std::sync::OnceLock;

use crate::colors::Color;

pub static SHELL: OnceLock<Shell> = OnceLock::new();

#[derive(Debug)]
pub enum Shell {
    Bash,
    Bare,
    Zsh,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BgColor(u8);

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FgColor(u8);

pub struct Reset;

impl FgColor {
    pub fn transpose(self) -> BgColor {
        BgColor(self.0)
    }
}

impl From<Color> for FgColor {
    fn from(c: Color) -> Self {
        FgColor(c.0)
    }
}

impl BgColor {
    pub fn transpose(self) -> FgColor {
        FgColor(self.0)
    }
}

impl From<Color> for BgColor {
    fn from(c: Color) -> Self {
        BgColor(c.0)
    }
}

impl std::fmt::Display for BgColor {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match SHELL.get().expect("shell not specified!") {
            Shell::Bash => write!(f, r#"\[\e[48;5;{}m\]"#, self.0),
            Shell::Bare => write!(f, "\x1b[48;5;{}m", self.0),
            Shell::Zsh => write!(f, "%{{\x1b[48;5;{}m%}}", self.0),
        }
    }
}

impl std::fmt::Display for FgColor {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match SHELL.get().expect("shell not specified!") {
            Shell::Bash => write!(f, r#"\[\e[38;5;{}m\]"#, self.0),
            Shell::Bare => write!(f, "\x1b[38;5;{}m", self.0),
            Shell::Zsh => write!(f, "%{{\x1b[38;5;{}m%}}", self.0),
        }
    }
}

/// An OSC 8 terminal hyperlink around `label`. The opening and closing
/// sequences are non-printing, so they get the same per-shell wrapping as the
/// colour escapes; the label itself is printed bare.
pub struct Hyperlink<'a> {
    pub url: &'a str,
    pub label: &'a str,
}

impl std::fmt::Display for Hyperlink<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match SHELL.get().expect("shell not specified!") {
            Shell::Bash => write!(
                f,
                r#"\[\e]8;;{}\e\\\]{}\[\e]8;;\e\\\]"#,
                self.url, self.label
            ),
            Shell::Bare => write!(f, "\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", self.url, self.label),
            // `%` starts a prompt escape in zsh, so a percent-encoded URL has to
            // be doubled to survive prompt expansion.
            Shell::Zsh => write!(
                f,
                "%{{\x1b]8;;{}\x1b\\%}}{}%{{\x1b]8;;\x1b\\%}}",
                self.url.replace('%', "%%"),
                self.label
            ),
        }
    }
}

impl std::fmt::Display for Reset {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match SHELL.get().expect("shell not specified!") {
            Shell::Bash => f.write_str(r#"\[\e[0m\]"#),
            Shell::Bare => f.write_str("\x1b[0m"),
            Shell::Zsh => f.write_str("%{\x1b[39m%}%{\x1b[49m%}"),
        }
    }
}
