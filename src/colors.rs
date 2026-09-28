#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u8);

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
            pub fn from_name(name: &str) -> Option<Color> {
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
    pub fn to_u8(self) -> u8 {
        self.0
    }

    pub fn from_u8(val: u8) -> Color {
        Color(val)
    }
}
