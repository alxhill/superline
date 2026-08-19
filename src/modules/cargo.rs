use std::env;
use std::marker::PhantomData;

use crate::colors::Color;
use crate::mise;
use crate::modules::Module;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

pub struct Cargo<S> {
    scheme: PhantomData<S>,
}

pub trait CargoScheme: DefaultColors {
    fn cargo_fg() -> Color {
        Self::default_fg()
    }

    fn cargo_bg() -> Color {
        Self::default_bg()
    }

    fn icon() -> &'static str {
        "\u{e68b}"
    }

    /// Marks a toolchain version that a mise config pins for this project.
    fn mise_icon() -> &'static str {
        mise::DEFAULT_ICON
    }
}

impl<S: CargoScheme> Default for Cargo<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: CargoScheme> Cargo<S> {
    pub fn new() -> Cargo<S> {
        Cargo {
            scheme: PhantomData,
        }
    }
}

impl<S: CargoScheme> Module for Cargo<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        if let Ok(cwd) = env::current_dir() {
            if cwd.join("Cargo.toml").exists() {
                // The icon alone says "rust project"; a mise-pinned toolchain
                // adds the version that will actually build it.
                let label = match mise::tool_version("rust") {
                    Some(version) => format!("{} {} {}", S::mise_icon(), S::icon(), version),
                    None => S::icon().to_string(),
                };

                powerline.add_segment(label, Style::simple(S::cargo_fg(), S::cargo_bg()));
            }
        }
    }
}
