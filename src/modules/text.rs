use std::marker::PhantomData;

use crate::terminal::{Shell, SHELL};
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

/// Adds literal, user-supplied text to the prompt.
///
/// Text uses the theme's default colours. It deliberately has no formatting
/// options: if a prompt needs a different colour, the surrounding separator
/// and theme configuration should provide it rather than the text value
/// carrying terminal escape sequences of its own.
pub struct Text<S: DefaultColors> {
    text: String,
    scheme: PhantomData<S>,
}

impl<S: DefaultColors> Text<S> {
    pub fn new(text: String) -> Text<S> {
        Text {
            text,
            scheme: PhantomData,
        }
    }
}

impl<S: DefaultColors> Module for Text<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        powerline.add_segment(
            escape_for_shell(&sanitize(&self.text), SHELL.get()),
            Style::simple(S::default_fg(), S::default_bg()),
        );
    }
}

/// Keep config text from emitting terminal controls into a prompt.
///
/// Printable Unicode is passed through unchanged. C0/C1 controls and line
/// separators are written as a visible escaped form, so a value copied from a
/// config file cannot reset colours, move the cursor, or inject a new prompt
/// line. `escape_default` also gives the less common controls an unambiguous
/// representation without dropping data entirely.
fn sanitize(text: &str) -> String {
    let mut sanitized = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\u{2028}' => sanitized.push_str(r#"\u{2028}"#),
            '\u{2029}' => sanitized.push_str(r#"\u{2029}"#),
            character if character.is_control() => sanitized.extend(character.escape_default()),
            character => sanitized.push(character),
        }
    }
    sanitized
}

/// Escape prompt-language syntax after the terminal controls are gone.
///
/// Bash expands `$`, backticks and backslash sequences in `PS1`; zsh expands
/// `%` sequences. The other supported shells receive the rendered prompt as
/// ordinary text, so their values need no additional quoting here.
fn escape_for_shell(text: &str, shell: Option<&Shell>) -> String {
    match shell {
        Some(Shell::Bash) => text
            .replace('\\', "\\\\")
            .replace('$', "\\$")
            .replace('`', "\\`"),
        Some(Shell::Zsh) => text.replace('%', "%%"),
        Some(Shell::Bare) | None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{escape_for_shell, sanitize};
    use crate::terminal::Shell;

    #[test]
    fn preserves_unicode_and_printable_special_characters() {
        let text = r#"café 🌈 $HOME 100% \ [brackets]"#;
        assert_eq!(sanitize(text), text);
    }

    #[test]
    fn escapes_terminal_controls_and_line_separators() {
        let text = "before\x1b[31m\r\nnext\x07bell\0nul\u{2028}last";
        assert_eq!(
            sanitize(text),
            r#"before\u{1b}[31m\r\nnext\u{7}bell\u{0}nul\u{2028}last"#
        );
        assert!(sanitize(text)
            .chars()
            .all(|character| !character.is_control()));
    }

    #[test]
    fn escapes_bash_prompt_syntax() {
        assert_eq!(
            escape_for_shell(
                r#"$(printf INJECT) `printf INJECT` 100% \ [brackets]"#,
                Some(&Shell::Bash),
            ),
            r#"\$(printf INJECT) \`printf INJECT\` 100% \\ [brackets]"#
        );
    }

    #[test]
    fn escapes_zsh_prompt_syntax() {
        assert_eq!(
            escape_for_shell(
                "$(printf INJECT) `printf INJECT` 100% %n ! ",
                Some(&Shell::Zsh)
            ),
            "$(printf INJECT) `printf INJECT` 100%% %%n ! "
        );
    }

    #[test]
    fn leaves_bare_shell_text_unchanged() {
        let text = r#"$(printf INJECT) `printf INJECT` 100% %n ! \ [brackets]"#;
        assert_eq!(escape_for_shell(text, Some(&Shell::Bare)), text);
    }
}
