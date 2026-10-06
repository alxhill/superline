#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

/// What a theme colour draws with: a 256-colour palette code, or the
/// terminal's own default colour (a theme's `"none"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorCode {
    /// The terminal's default foreground or background (SGR `39` / `49`), so
    /// a segment shows the terminal's own background through it.
    Terminal,
    /// A code from the 256-colour palette (SGR `38;5;N` / `48;5;N`).
    Palette(u8),
}

impl ColorCode {
    /// The palette code, or `None` for the terminal's own colour.
    pub fn palette(self) -> Option<u8> {
        match self {
            ColorCode::Terminal => None,
            ColorCode::Palette(code) => Some(code),
        }
    }
}

/// The name a theme gives the terminal's own colour, e.g. `"bg": "none"`. No
/// palette colour is called this.
pub const NONE_NAME: &str = "none";

/// A theme colour: a 256-colour palette code or the terminal's own colour. A
/// colour a theme resolves for text (`fg` or `*_fg`) also carries the
/// attributes set next to it, which only apply when it is drawn as a
/// foreground.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Color {
    code: ColorCode,
    attrs: TextAttrs,
}

/// The plain colour with palette code `code`, written like a tuple-struct
/// constructor so `Color(31)` still builds one.
#[allow(non_snake_case)]
pub const fn Color(code: u8) -> Color {
    Color {
        code: ColorCode::Palette(code),
        attrs: TextAttrs::NONE,
    }
}

/// Text attributes a theme can set next to a text colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextAttrs {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl TextAttrs {
    pub const NONE: TextAttrs = TextAttrs {
        bold: false,
        italic: false,
        underline: false,
    };

    pub fn is_empty(self) -> bool {
        self == TextAttrs::NONE
    }

    /// The SGR parameters that turn the set attributes on, e.g. `1;4`.
    pub fn on_codes(self) -> String {
        self.codes(["1", "3", "4"])
    }

    /// The SGR parameters that turn the set attributes off again, leaving
    /// the colours alone.
    pub fn off_codes(self) -> String {
        self.codes(["22", "23", "24"])
    }

    fn codes(self, codes: [&str; 3]) -> String {
        [self.bold, self.italic, self.underline]
            .into_iter()
            .zip(codes)
            .filter_map(|(set, code)| set.then_some(code))
            .collect::<Vec<_>>()
            .join(";")
    }
}

macro_rules! define_colors {
    ($($name:ident => $code:expr),* $(,)?) => {
        $(
        pub const fn $name() -> Color {
            Color($code)
        }
        )*
        fn color_map() -> &'static HashMap<&'static str, Color> {
            static COLOR_MAP: OnceLock<HashMap<&'static str, Color>> = OnceLock::new();
            COLOR_MAP.get_or_init(|| {
                let mut m = HashMap::new();
                $(
                    m.insert(stringify!($name), $name());
                )*
                m
            })
        }

        /// Every named colour, in definition order.
        pub const NAMED_COLORS: &[(&str, Color)] = &[$((stringify!($name), Color($code))),*];

        impl Color {
            /// The colour a theme names, including [`NONE_NAME`] for the
            /// terminal's own colour.
            pub fn from_name(name: &str) -> Option<Color> {
                if name == NONE_NAME {
                    return Some(Color::NONE);
                }
                color_map().get(name).copied()
            }
        }
    };
}

define_colors! {
    black => 0,
    red => 1,
    light_red => 9,
    green => 2,
    light_green => 10,
    yellow => 3,
    light_yellow => 11,
    blue => 4,
    light_blue => 12,
    purple => 5,
    light_purple => 13,
    turquoise => 6,
    light_turquoise => 14,
    grey => 7,
    white => 15,
    dark_grey => 234,
    light_grey => 250,
    mid_grey => 240,

    turquoise_blue => 31,
    dark_green => 22,
    mid_green => 28,
    mid_red => 124,
    forest_green => 22,
    warning_red => 160,
    burgundy => 52,

    orange => 130,
    bright_orange => 202,
    dark_yellow => 136,
    dark_blue => 19,
    nice_purple => 93,
    burnt_orange => 214
}

impl Color {
    /// The terminal's own default colour: text drawn in it uses the
    /// terminal's foreground, and a segment drawn on it shows the terminal's
    /// background.
    pub const NONE: Color = Color {
        code: ColorCode::Terminal,
        attrs: TextAttrs::NONE,
    };

    pub fn code(self) -> ColorCode {
        self.code
    }

    /// Whether this is the terminal's own colour rather than a palette one.
    pub fn is_none(self) -> bool {
        self.code == ColorCode::Terminal
    }

    pub fn from_u8(val: u8) -> Color {
        Color(val)
    }

    pub fn attrs(self) -> TextAttrs {
        self.attrs
    }

    pub fn with_attrs(self, attrs: TextAttrs) -> Color {
        Color { attrs, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_names_the_terminals_own_colour() {
        let none = Color::from_name(NONE_NAME).unwrap();
        assert!(none.is_none());
        assert_eq!(none.code(), ColorCode::Terminal);
        assert_eq!(none.code().palette(), None);
        assert!(!Color::from_name("black").unwrap().is_none());
        assert_eq!(Color(0).code().palette(), Some(0));
    }

    #[test]
    fn no_palette_colour_is_called_none() {
        assert!(NAMED_COLORS.iter().all(|(name, _)| *name != NONE_NAME));
    }
}
