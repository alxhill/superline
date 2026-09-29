use std::env;
use std::fs::read_to_string;
use std::marker::PhantomData;
use std::path::Path;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::mise;
use crate::modules::{DefaultPadding, Module};
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

pub struct Cargo<S> {
    /// Whether to show the mise-pinned toolchain version after the icon.
    show_version: bool,
    scheme: PhantomData<S>,
}

pub trait CargoScheme: DefaultColors {
    const CARGO_ICON: &'static str = "\u{e68b}";

    fn cargo_fg() -> Color {
        Self::default_fg()
    }

    fn cargo_bg() -> Color {
        Self::default_bg()
    }

    fn icon() -> &'static str {
        Self::CARGO_ICON
    }

    /// Marks a toolchain version that a mise config pins for this project.
    fn mise_icon() -> &'static str {
        mise::DEFAULT_ICON
    }
}

impl<S: CargoScheme> Default for Cargo<S> {
    fn default() -> Self {
        Self::new(true)
    }
}

impl<S: CargoScheme> Cargo<S> {
    pub fn new(show_version: bool) -> Cargo<S> {
        Cargo {
            show_version,
            scheme: PhantomData,
        }
    }
}

/// A Rust project in the current directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoProject {
    /// Whether a mise config pins the rust toolchain.
    mise_managed: bool,
    /// The pinned toolchain version, when `show_version` is on.
    toolchain_version: Option<String>,
}

impl<S: CargoScheme> Module for Cargo<S> {
    /// The Rust project in the current directory, if there is one.
    type Data = Option<CargoProject>;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn fetch(&self) -> Option<CargoProject> {
        let cwd = env::current_dir().ok()?;
        if !cwd.join("Cargo.toml").exists() {
            return None;
        }

        // The icon alone says "rust project"; a pinned toolchain adds the
        // version that will actually build it. mise wins over rustup's
        // `rust-toolchain` file since it is what puts cargo on the path.
        let mise_version = mise::tool_version("rust");
        let toolchain_version = match (mise_version, self.show_version) {
            (Some(version), true) => Some(version.to_string()),
            (None, true) => rust_toolchain_channel(&cwd),
            (_, false) => None,
        };

        Some(CargoProject {
            mise_managed: mise_version.is_some(),
            toolchain_version,
        })
    }

    fn sample(&self) -> Option<CargoProject> {
        Some(CargoProject {
            mise_managed: false,
            toolchain_version: self.show_version.then(|| "1.93.0".to_string()),
        })
    }

    fn render(&self, project: Option<CargoProject>, powerline: &mut Powerline) {
        if let Some(label) = project.map(|project| cargo_label::<S>(&project)) {
            if !label.is_empty() {
                powerline.add_segment(label, Style::simple(S::cargo_fg(), S::cargo_bg()));
            }
        }
    }
}

/// The mise marker stays even with the version hidden, since it says who
/// manages the toolchain rather than which one it is.
fn cargo_label<S: CargoScheme>(project: &CargoProject) -> String {
    [
        project.mise_managed.then(|| S::mise_icon()),
        Some(S::icon()),
        project.toolchain_version.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

/// The channel pinned by the nearest `rust-toolchain.toml` or legacy
/// `rust-toolchain` file, searched from `cwd` upwards the way rustup does so a
/// workspace member picks up the pin at the workspace root.
fn rust_toolchain_channel(cwd: &Path) -> Option<String> {
    for dir in cwd.ancestors() {
        if let Ok(contents) = read_to_string(dir.join("rust-toolchain.toml")) {
            return toolchain_toml_channel(&contents);
        }
        if let Ok(contents) = read_to_string(dir.join("rust-toolchain")) {
            return legacy_toolchain_channel(&contents);
        }
    }
    None
}

/// Reads `channel` from the `[toolchain]` table. A `path` toolchain has no
/// channel and yields nothing.
fn toolchain_toml_channel(contents: &str) -> Option<String> {
    let mut in_toolchain = false;

    for line in contents.lines() {
        let line = line.trim();

        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_toolchain = header.trim() == "toolchain";
            continue;
        }
        if !in_toolchain || line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "channel" {
            let value = value.split('#').next().unwrap_or_default().trim();
            let channel = value.trim_matches(|c| c == '"' || c == '\'');
            return (!channel.is_empty()).then(|| channel.to_string());
        }
    }

    None
}

/// The legacy `rust-toolchain` file is either the bare channel name on a single
/// line or the same TOML as `rust-toolchain.toml`.
fn legacy_toolchain_channel(contents: &str) -> Option<String> {
    if contents.contains("[toolchain]") {
        return toolchain_toml_channel(contents);
    }

    contents
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
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

    impl CargoScheme for TestTheme {}

    #[test]
    fn label_shows_the_mise_marker_icon_and_version() {
        let project = CargoProject {
            mise_managed: true,
            toolchain_version: Some("1.93.0".to_string()),
        };
        assert_eq!(
            cargo_label::<TestTheme>(&project),
            format!("{} \u{e68b} 1.93.0", mise::DEFAULT_ICON)
        );
    }

    #[test]
    fn sample_follows_show_version() {
        let with_version = Cargo::<TestTheme>::new(true).sample().unwrap();
        assert_eq!(cargo_label::<TestTheme>(&with_version), "\u{e68b} 1.93.0");

        let without_version = Cargo::<TestTheme>::new(false).sample().unwrap();
        assert_eq!(cargo_label::<TestTheme>(&without_version), "\u{e68b}");
    }

    #[test]
    fn reads_the_channel_from_the_toolchain_table() {
        let contents = r#"
[toolchain]
channel = "1.93.0"  # keep in step with CI
components = ["rustfmt", "clippy"]
"#;
        assert_eq!(toolchain_toml_channel(contents), Some("1.93.0".to_string()));
        assert_eq!(
            toolchain_toml_channel("[toolchain]\nchannel = 'nightly-2025-01-15'\n"),
            Some("nightly-2025-01-15".to_string())
        );
    }

    #[test]
    fn ignores_channels_outside_the_toolchain_table_and_path_toolchains() {
        assert_eq!(
            toolchain_toml_channel("[other]\nchannel = \"stable\"\n"),
            None
        );
        assert_eq!(
            toolchain_toml_channel("[toolchain]\npath = \"/opt/rust\"\n"),
            None
        );
    }

    #[test]
    fn legacy_file_holds_a_bare_channel_or_toml() {
        assert_eq!(
            legacy_toolchain_channel("# pinned\nstable\n"),
            Some("stable".to_string())
        );
        assert_eq!(
            legacy_toolchain_channel("[toolchain]\nchannel = \"1.85\"\n"),
            Some("1.85".to_string())
        );
        assert_eq!(legacy_toolchain_channel("\n"), None);
    }
}
