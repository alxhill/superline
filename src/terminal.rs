use std::sync::OnceLock;

use crate::colors::{Color, TextAttrs};

pub static SHELL: OnceLock<Shell> = OnceLock::new();

#[derive(Debug)]
pub enum Shell {
    Bash,
    Bare,
    Zsh,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BgColor(u8);

/// A text colour and the attributes drawn with it. Printing it turns both on;
/// [`FgColor::attrs_off`] turns the attributes back off.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FgColor {
    code: u8,
    attrs: TextAttrs,
}

/// Resets the colours. Zsh's version leaves text attributes on, so whatever
/// turns one on also turns it off with [`AttrsOff`].
pub struct Reset;

/// Turns text attributes on, wrapped like the colour escapes. Prints nothing
/// when no attribute is set.
struct AttrsOn(TextAttrs);

/// Turns text attributes off without touching the colours. Prints nothing
/// when no attribute is set.
pub struct AttrsOff(pub TextAttrs);

impl FgColor {
    pub fn transpose(self) -> BgColor {
        BgColor(self.code)
    }

    pub fn attrs_off(self) -> AttrsOff {
        AttrsOff(self.attrs)
    }
}

impl From<Color> for FgColor {
    fn from(c: Color) -> Self {
        FgColor {
            code: c.to_u8(),
            attrs: c.attrs(),
        }
    }
}

impl BgColor {
    pub fn transpose(self) -> FgColor {
        FgColor {
            code: self.0,
            attrs: TextAttrs::NONE,
        }
    }
}

impl From<Color> for BgColor {
    fn from(c: Color) -> Self {
        BgColor(c.to_u8())
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
            Shell::Bash => write!(f, r#"\[\e[38;5;{}m\]"#, self.code),
            Shell::Bare => write!(f, "\x1b[38;5;{}m", self.code),
            Shell::Zsh => write!(f, "%{{\x1b[38;5;{}m%}}", self.code),
        }?;
        write!(f, "{}", AttrsOn(self.attrs))
    }
}

impl std::fmt::Display for AttrsOn {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write_sgr(
            f,
            SHELL.get().expect("shell not specified!"),
            &self.0.on_codes(),
        )
    }
}

impl std::fmt::Display for AttrsOff {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write_sgr(
            f,
            SHELL.get().expect("shell not specified!"),
            &self.0.off_codes(),
        )
    }
}

/// An SGR escape with `params`, wrapped the way `shell` marks non-printing
/// text.
fn write_sgr(f: &mut std::fmt::Formatter, shell: &Shell, params: &str) -> std::fmt::Result {
    if params.is_empty() {
        return Ok(());
    }
    match shell {
        Shell::Bash => write!(f, r#"\[\e[{params}m\]"#),
        Shell::Bare => write!(f, "\x1b[{params}m"),
        Shell::Zsh => write!(f, "%{{\x1b[{params}m%}}"),
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

/// An iTerm2 hidden annotation: hovering over the next `cells` columns shows
/// `message`. It is printed just before the text it covers and takes no
/// columns itself, so it gets the same per-shell wrapping as the colour
/// escapes. Terminals that don't know OSC 1337 ignore it.
pub struct Annotation<'a> {
    pub cells: usize,
    pub message: &'a str,
}

impl std::fmt::Display for Annotation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write_annotation(
            f,
            SHELL.get().expect("shell not specified!"),
            self.cells,
            self.message,
        )
    }
}

fn write_annotation(
    f: &mut std::fmt::Formatter,
    shell: &Shell,
    cells: usize,
    message: &str,
) -> std::fmt::Result {
    // Control characters could end the sequence early, and iTerm2 splits the
    // payload on `|`, so neither may reach the terminal.
    let message: String = message
        .chars()
        .filter(|c| !c.is_control() && *c != '|')
        .collect();
    let message = escape_for_shell(&message, Some(shell));
    match shell {
        Shell::Bash => write!(f, r#"\[\e]1337;AddHiddenAnnotation={cells}|{message}\a\]"#),
        Shell::Bare => write!(f, "\x1b]1337;AddHiddenAnnotation={cells}|{message}\x07"),
        Shell::Zsh => write!(
            f,
            "%{{\x1b]1337;AddHiddenAnnotation={cells}|{message}\x07%}}"
        ),
    }
}

/// Escape prompt-language syntax in text that contains no terminal controls.
///
/// Bash expands `$`, backticks and backslash sequences in `PS1`; zsh expands
/// `%` sequences. The other supported shells receive the rendered prompt as
/// ordinary text, so their values need no additional quoting here.
pub fn escape_for_shell(text: &str, shell: Option<&Shell>) -> String {
    match shell {
        Some(Shell::Bash) => text
            .replace('\\', "\\\\")
            .replace('$', "\\$")
            .replace('`', "\\`"),
        Some(Shell::Zsh) => text.replace('%', "%%"),
        Some(Shell::Bare) | None => text.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An SGR escape for an explicit shell, since `SHELL` is set once per
    /// process.
    struct Sgr<'a>(&'a Shell, String);

    impl std::fmt::Display for Sgr<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write_sgr(f, self.0, &self.1)
        }
    }

    /// An annotation for an explicit shell, since `SHELL` is set once per
    /// process.
    struct Note<'a>(&'a Shell, usize, &'a str);

    impl std::fmt::Display for Note<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write_annotation(f, self.0, self.1, self.2)
        }
    }

    #[test]
    fn annotations_are_wrapped_for_each_shell() {
        let note = |shell| Note(shell, 1, "5h: 61% used").to_string();
        assert_eq!(
            note(&Shell::Bash),
            r"\[\e]1337;AddHiddenAnnotation=1|5h: 61% used\a\]"
        );
        assert_eq!(
            note(&Shell::Zsh),
            "%{\x1b]1337;AddHiddenAnnotation=1|5h: 61%% used\x07%}"
        );
        assert_eq!(
            note(&Shell::Bare),
            "\x1b]1337;AddHiddenAnnotation=1|5h: 61% used\x07"
        );
    }

    #[test]
    fn annotation_messages_cannot_break_out_of_the_escape() {
        let message = "a\x07b\x1bc|d\ne\u{9b}f";
        for shell in [Shell::Bash, Shell::Zsh, Shell::Bare] {
            let note = Note(&shell, 2, message).to_string();
            assert!(note.contains("=2|abcdef"), "{shell:?}: {note:?}");
        }
        assert_eq!(
            Note(&Shell::Bare, 2, message).to_string(),
            "\x1b]1337;AddHiddenAnnotation=2|abcdef\x07"
        );
    }

    #[test]
    fn annotation_messages_escape_prompt_syntax() {
        assert_eq!(
            Note(&Shell::Bash, 1, r"$(x) `y` \z").to_string(),
            r"\[\e]1337;AddHiddenAnnotation=1|\$(x) \`y\` \\z\a\]"
        );
        assert_eq!(
            Note(&Shell::Zsh, 1, "100% %n").to_string(),
            "%{\x1b]1337;AddHiddenAnnotation=1|100%% %%n\x07%}"
        );
    }

    const BOLD_UNDERLINE: TextAttrs = TextAttrs {
        bold: true,
        italic: false,
        underline: true,
    };

    #[test]
    fn attributes_have_their_own_on_and_off_codes() {
        let all = TextAttrs {
            bold: true,
            italic: true,
            underline: true,
        };
        assert_eq!(all.on_codes(), "1;3;4");
        assert_eq!(all.off_codes(), "22;23;24");
        assert_eq!(BOLD_UNDERLINE.on_codes(), "1;4");
        assert_eq!(BOLD_UNDERLINE.off_codes(), "22;24");
    }

    #[test]
    fn attribute_escapes_are_wrapped_for_each_shell() {
        let on = |shell| Sgr(shell, BOLD_UNDERLINE.on_codes()).to_string();
        let off = |shell| Sgr(shell, BOLD_UNDERLINE.off_codes()).to_string();
        assert_eq!(on(&Shell::Bash), r"\[\e[1;4m\]");
        assert_eq!(off(&Shell::Bash), r"\[\e[22;24m\]");
        assert_eq!(on(&Shell::Zsh), "%{\x1b[1;4m%}");
        assert_eq!(off(&Shell::Zsh), "%{\x1b[22;24m%}");
        assert_eq!(on(&Shell::Bare), "\x1b[1;4m");
        assert_eq!(off(&Shell::Bare), "\x1b[22;24m");
    }

    #[test]
    fn no_attributes_print_no_escape() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Bare] {
            assert_eq!(Sgr(&shell, TextAttrs::NONE.on_codes()).to_string(), "");
            assert_eq!(Sgr(&shell, TextAttrs::NONE.off_codes()).to_string(), "");
        }
    }

    #[test]
    fn only_a_text_colour_keeps_its_attributes() {
        let bold = Color(31).with_attrs(BOLD_UNDERLINE);
        assert_eq!(FgColor::from(bold).attrs_off().0, BOLD_UNDERLINE);
        assert!(FgColor::from(bold) != FgColor::from(Color(31)));
        assert!(BgColor::from(bold).transpose() == FgColor::from(Color(31)));
    }
}
