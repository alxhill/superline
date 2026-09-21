use std::env;
use std::marker::PhantomData;

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

/// The operating-system families that the prompt can identify without
/// consulting the filesystem or spawning a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsKind {
    Android,
    Linux,
    Macos,
    Windows,
    FreeBsd,
    OpenBsd,
    NetBsd,
    DragonFly,
    Illumos,
    Solaris,
    Haiku,
    Unknown,
}

impl OsKind {
    /// Maps Rust's target OS value to the small set of families shown by the
    /// widget. Keeping this as a pure function makes the platform matrix easy
    /// to exercise on one host in unit tests.
    pub fn from_target(target: &str) -> Self {
        match target {
            "android" => Self::Android,
            "linux" => Self::Linux,
            "macos" => Self::Macos,
            "windows" => Self::Windows,
            "freebsd" => Self::FreeBsd,
            "openbsd" => Self::OpenBsd,
            "netbsd" => Self::NetBsd,
            "dragonfly" => Self::DragonFly,
            "illumos" => Self::Illumos,
            "solaris" => Self::Solaris,
            "haiku" => Self::Haiku,
            _ => Self::Unknown,
        }
    }

    pub fn current() -> Self {
        Self::from_target(env::consts::OS)
    }

    /// The key used by a custom theme to override this operating system's
    /// symbol. A generic symbol key remains available as a simple fallback.
    pub fn theme_key(self) -> &'static str {
        match self {
            Self::Android => "android_symbol",
            Self::Linux => "linux_symbol",
            Self::Macos => "macos_symbol",
            Self::Windows => "windows_symbol",
            Self::FreeBsd => "freebsd_symbol",
            Self::OpenBsd => "openbsd_symbol",
            Self::NetBsd => "netbsd_symbol",
            Self::DragonFly => "dragonfly_symbol",
            Self::Illumos => "illumos_symbol",
            Self::Solaris => "solaris_symbol",
            Self::Haiku => "haiku_symbol",
            Self::Unknown => "unknown_symbol",
        }
    }

    /// Nerd Font symbols used by the built-in themes. These are deliberately
    /// family-level defaults rather than a distro database: the widget stays
    /// instant and predictable on Linux distributions and BSD variants alike.
    ///
    /// The BSD and Illumos values use their dedicated Nerd Font glyphs.
    /// Solaris and Haiku use the neutral platform marker instead of borrowing
    /// the Linux logo.
    pub fn default_symbol(self) -> &'static str {
        match self {
            Self::Android => "\u{f17b}",
            Self::Linux => "\u{f17c}",
            Self::Macos => "\u{f179}",
            Self::Windows => "\u{f17a}",
            Self::FreeBsd => "\u{f30c}",
            Self::OpenBsd => "\u{f023a}",
            Self::NetBsd => "\u{f0244}",
            Self::DragonFly => "\u{e28e}",
            Self::Illumos => "\u{f0238}",
            Self::Solaris | Self::Haiku | Self::Unknown => GENERIC_UNIX_SYMBOL,
        }
    }
}

/// A neutral, widely supported Nerd Font marker for OS families without a
/// dedicated symbol.
const GENERIC_UNIX_SYMBOL: &str = "\u{f0ac}";

/// Shows a compact icon for the current operating system.
pub struct Os<S: OsScheme> {
    kind: OsKind,
    scheme: PhantomData<S>,
}

pub trait OsScheme: DefaultColors {
    fn os_fg() -> Color {
        Self::default_fg()
    }

    fn os_bg() -> Color {
        Self::default_bg()
    }

    fn os_symbol(kind: OsKind) -> &'static str {
        kind.default_symbol()
    }
}

impl<S: OsScheme> Default for Os<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: OsScheme> Os<S> {
    pub fn new() -> Self {
        Self::from_kind(OsKind::current())
    }

    pub fn from_kind(kind: OsKind) -> Self {
        Self {
            kind,
            scheme: PhantomData,
        }
    }
}

impl<S: OsScheme> Module for Os<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        powerline.add_segment(
            S::os_symbol(self.kind),
            Style::simple(S::os_fg(), S::os_bg()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_supported_target_names() {
        assert_eq!(OsKind::from_target("linux"), OsKind::Linux);
        assert_eq!(OsKind::from_target("macos"), OsKind::Macos);
        assert_eq!(OsKind::from_target("windows"), OsKind::Windows);
        assert_eq!(OsKind::from_target("android"), OsKind::Android);
        assert_eq!(OsKind::from_target("freebsd"), OsKind::FreeBsd);
        assert_eq!(OsKind::from_target("openbsd"), OsKind::OpenBsd);
        assert_eq!(OsKind::from_target("netbsd"), OsKind::NetBsd);
        assert_eq!(OsKind::from_target("dragonfly"), OsKind::DragonFly);
        assert_eq!(OsKind::from_target("illumos"), OsKind::Illumos);
        assert_eq!(OsKind::from_target("solaris"), OsKind::Solaris);
        assert_eq!(OsKind::from_target("haiku"), OsKind::Haiku);
    }

    #[test]
    fn unknown_targets_use_a_safe_fallback() {
        assert_eq!(OsKind::from_target("plan9"), OsKind::Unknown);
        assert_eq!(OsKind::Unknown.default_symbol(), GENERIC_UNIX_SYMBOL);
    }

    #[test]
    fn common_platforms_have_distinct_symbols() {
        assert_ne!(
            OsKind::Linux.default_symbol(),
            OsKind::Macos.default_symbol()
        );
        assert_ne!(
            OsKind::Linux.default_symbol(),
            OsKind::Windows.default_symbol()
        );
        assert_ne!(
            OsKind::Macos.default_symbol(),
            OsKind::Windows.default_symbol()
        );
    }

    #[test]
    fn known_bsd_and_illumos_targets_do_not_use_linux_symbol() {
        let linux = OsKind::Linux.default_symbol();
        for kind in [
            OsKind::OpenBsd,
            OsKind::NetBsd,
            OsKind::DragonFly,
            OsKind::Illumos,
        ] {
            assert_ne!(kind.default_symbol(), linux);
        }

        assert_eq!(OsKind::OpenBsd.default_symbol(), "\u{f023a}");
        assert_eq!(OsKind::NetBsd.default_symbol(), "\u{f0244}");
        assert_eq!(OsKind::DragonFly.default_symbol(), "\u{e28e}");
        assert_eq!(OsKind::Illumos.default_symbol(), "\u{f0238}");
    }

    #[test]
    fn targets_without_a_dedicated_symbol_use_the_neutral_marker() {
        for kind in [OsKind::Solaris, OsKind::Haiku, OsKind::Unknown] {
            assert_eq!(kind.default_symbol(), GENERIC_UNIX_SYMBOL);
            assert_ne!(kind.default_symbol(), "?");
            assert_ne!(kind.default_symbol(), OsKind::Linux.default_symbol());
        }
    }
}
