use std::env;
use std::fs::File;
use std::io::read_to_string;
use std::marker::PhantomData;

use crate::colors::Color;
use crate::mise;
use crate::modules::Module;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

pub struct Node<S> {
    /// Whether to show the node version after the icon.
    show_version: bool,
    scheme: PhantomData<S>,
}

pub trait NodeScheme: DefaultColors {
    fn node_fg() -> Color {
        Self::default_fg()
    }

    fn node_bg() -> Color {
        Self::default_bg()
    }

    fn node_inactive_bg() -> Color {
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

impl<S: NodeScheme> Default for Node<S> {
    fn default() -> Self {
        Self::new(true)
    }
}

impl<S: NodeScheme> Node<S> {
    pub fn new(show_version: bool) -> Node<S> {
        Node {
            show_version,
            scheme: PhantomData,
        }
    }

    fn label(&self, source_icon: &str, version: &str) -> String {
        [
            Some(source_icon),
            Some(S::icon()),
            self.show_version.then_some(version),
        ]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
    }
}

impl<S: NodeScheme> Module for Node<S> {
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
                    self.label("", version.trim()),
                    Style::simple(S::node_fg(), S::node_bg()),
                );
            }
            // A mise config manages node for this directory, so its version is
            // the one in effect even though nvm never activated it.
            (None, Some(version), _) => {
                powerline.add_segment(
                    self.label(S::mise_icon(), version),
                    Style::simple(S::node_fg(), S::node_bg()),
                );
            }
            (None, None, Some(nvmrc)) => {
                powerline.add_segment(
                    self.label("", nvmrc.trim()),
                    Style::simple(S::node_fg(), S::node_inactive_bg()),
                );
            }
            _ => {}
        }
    }
}
