use std::env;
use std::fs::File;
use std::io::read_to_string;
use std::marker::PhantomData;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::mise;
use crate::modules::{DefaultPadding, Module};
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

pub struct Node<S> {
    /// Whether to show the node version after the icon.
    show_version: bool,
    scheme: PhantomData<S>,
}

pub trait NodeScheme: DefaultColors {
    const NODE_ICON: &'static str = "\u{ed0d}";

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
        Self::NODE_ICON
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

    /// The segment's label and background.
    fn segment(&self, version: &NodeVersion) -> (String, Color) {
        match version {
            NodeVersion::Nvm(version) => (self.label("", version.trim()), S::node_bg()),
            NodeVersion::Mise(version) => (self.label(S::mise_icon(), version), S::node_bg()),
            NodeVersion::Nvmrc(nvmrc) => (self.label("", nvmrc.trim()), S::node_inactive_bg()),
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

/// Where the node version for the current directory comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeVersion {
    /// The version nvm activated.
    Nvm(String),
    /// A mise config manages node for this directory, so its version is the
    /// one in effect even though nvm never activated it.
    Mise(String),
    /// The version a `.nvmrc` asks for, which nothing has activated.
    Nvmrc(String),
}

impl<S: NodeScheme> Module for Node<S> {
    /// The node version in effect or requested, if any.
    type Data = Option<NodeVersion>;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn fetch(&self) -> Option<NodeVersion> {
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
            (Some(version), _, _) => Some(NodeVersion::Nvm(version)),
            (None, Some(version), _) => Some(NodeVersion::Mise(version.to_string())),
            (None, None, Some(nvmrc)) => Some(NodeVersion::Nvmrc(nvmrc)),
            _ => None,
        }
    }

    fn sample(&self) -> Option<NodeVersion> {
        Some(NodeVersion::Nvm("v22.11.0".to_string()))
    }

    fn render(&self, version: Option<NodeVersion>, powerline: &mut Powerline) {
        let segment = version.map(|version| self.segment(&version));

        if let Some((label, bg)) = segment.filter(|(label, _)| !label.is_empty()) {
            powerline.add_segment(label, Style::simple(S::node_fg(), bg));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colors::{black, white};

    struct TestTheme;

    impl DefaultColors for TestTheme {
        fn default_bg() -> Color {
            black()
        }

        fn default_fg() -> Color {
            white()
        }
    }

    impl NodeScheme for TestTheme {}

    #[test]
    fn labels_each_version_source() {
        let node = Node::<TestTheme>::new(true);
        assert_eq!(
            node.segment(&NodeVersion::Nvm("v22.11.0\n".to_string())).0,
            "\u{ed0d} v22.11.0"
        );
        assert_eq!(
            node.segment(&NodeVersion::Mise("22".to_string())).0,
            format!("{} \u{ed0d} 22", mise::DEFAULT_ICON)
        );
        assert_eq!(
            node.segment(&NodeVersion::Nvmrc("lts/iron\n".to_string()))
                .0,
            "\u{ed0d} lts/iron"
        );
    }

    #[test]
    fn hides_the_version_when_turned_off() {
        let node = Node::<TestTheme>::new(false);
        let sample = node.sample().expect("the sample has a version");
        assert_eq!(node.segment(&sample).0, "\u{ed0d}");
    }
}
