use std::env;
use std::fs::File;
use std::io::read_to_string;
use std::marker::PhantomData;

use crate::colors::Color;
use crate::mise;
use crate::modules::Module;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

pub struct Nvm<S> {
    scheme: PhantomData<S>,
}

pub trait NvmScheme: DefaultColors {
    fn nvm_fg() -> Color {
        Self::default_fg()
    }

    fn nvm_bg() -> Color {
        Self::default_bg()
    }

    fn nvm_inactive_bg() -> Color {
        Self::default_bg()
    }

    fn icon() -> &'static str {
        "\u{ed0d}"
    }

    /// Marks a version that came from a mise config rather than nvm.
    fn mise_icon() -> &'static str {
        mise::DEFAULT_ICON
    }
}

impl<S: NvmScheme> Default for Nvm<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: NvmScheme> Nvm<S> {
    pub fn new() -> Nvm<S> {
        Nvm {
            scheme: PhantomData,
        }
    }
}

impl<S: NvmScheme> Module for Nvm<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        let nvm_current_version = env::var("nvm_current_version").ok();

        let nvmrc_version = env::current_dir()
            .and_then(|cwd| File::open(cwd.join(".nvmrc")))
            .and_then(read_to_string)
            .ok();

        match (
            nvm_current_version,
            mise::tool_version("node"),
            nvmrc_version,
        ) {
            // todo: handle the case where active version != .nvmrc
            (Some(version), _, _) => {
                powerline.add_segment(
                    format!("{} {}", S::icon(), version),
                    Style::simple(S::nvm_fg(), S::nvm_bg()),
                );
            }
            // A mise config manages node for this directory, so its version is
            // the one in effect even though nvm never activated it.
            (None, Some(version), _) => {
                powerline.add_segment(
                    format!("{} {} {}", S::mise_icon(), S::icon(), version),
                    Style::simple(S::nvm_fg(), S::nvm_bg()),
                );
            }
            (None, None, Some(nvmrc)) => {
                powerline.add_segment(
                    format!("{} {}", S::icon(), nvmrc.trim()),
                    Style::simple(S::nvm_fg(), S::nvm_inactive_bg()),
                );
            }
            _ => {}
        }
    }
}
