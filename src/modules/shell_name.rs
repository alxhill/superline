use crate::themes::DefaultColors;
use crate::{Color, Powerline, Style};
use std::marker::PhantomData;

use super::Module;

pub struct ShellName<S: ShellScheme> {
    name: String,
    scheme: PhantomData<S>,
}

pub trait ShellScheme: DefaultColors {
    fn shellname_fg() -> Color {
        Self::default_fg()
    }

    fn shellname_bg() -> Color {
        Self::default_bg()
    }
}

impl<S: ShellScheme> ShellName<S> {
    pub fn new(name: String) -> ShellName<S> {
        ShellName {
            name,
            scheme: PhantomData,
        }
    }
}

impl<S: ShellScheme> Module for ShellName<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        powerline.add_short_segment(&self.name, style::<S>());
    }
}

fn style<S: ShellScheme>() -> Style {
    Style::simple(S::shellname_fg(), S::shellname_bg())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colors::{black, green, red, white};
    use crate::terminal::{BgColor, FgColor};

    struct TestTheme;

    impl DefaultColors for TestTheme {
        fn default_bg() -> Color {
            black()
        }

        fn default_fg() -> Color {
            green()
        }
    }

    impl ShellScheme for TestTheme {
        fn shellname_fg() -> Color {
            white()
        }

        fn shellname_bg() -> Color {
            red()
        }
    }

    #[test]
    fn uses_the_themes_shell_colours() {
        let style = style::<TestTheme>();
        assert!(style.fg == FgColor::from(white()));
        assert!(style.bg == BgColor::from(red()));
    }
}
