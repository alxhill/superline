use std::marker::PhantomData;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{Cached, Lookup, Source};
use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::platform;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::{DefaultPadding, Module};

/// How long a cached sudo credential check remains useful.
const CACHE_TTL: Duration = Duration::from_secs(10);
/// `-n` guarantees that a prompt can never be opened by the check. `-N`
/// prevents a successful validation from extending the cached timestamp, and
/// `-v` validates the timestamp without running a command as root. This is the
/// documented sudo credential-cache probe (`sudo -Nnv`).
const SUDO_ARGS: [&str; 3] = ["-N", "-n", "-v"];

/// Shows a marker when sudo credentials are already cached.
///
/// Checking sudo can involve a process launch and, depending on the platform,
/// a policy lookup, so it is always driven through the shared cache. A missing
/// or stale value is refreshed away from the prompt thread.
pub struct Sudo<S: SudoScheme> {
    scheme: PhantomData<S>,
}

pub trait SudoScheme: DefaultColors {
    const SUDO_SYMBOL: &'static str = "⚿";

    fn sudo_fg() -> Color {
        Self::default_fg()
    }

    fn sudo_bg() -> Color {
        Self::default_bg()
    }

    fn sudo_symbol() -> &'static str {
        Self::SUDO_SYMBOL
    }
}

impl<S: SudoScheme> Default for Sudo<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: SudoScheme> Sudo<S> {
    pub fn new() -> Self {
        Self {
            scheme: PhantomData,
        }
    }
}

/// The parameter-less lookup is kept public so the refresh dispatcher can
/// route the detached cache worker back to this module.
#[derive(Clone, Serialize, Deserialize)]
pub struct SudoLookup;

impl Source for SudoLookup {
    type Value = bool;
    const KIND: &'static str = "sudo";
    const TTL: Duration = CACHE_TTL;
    // A failed/non-cached probe should not run again on every prompt.
    const REFRESH_INTERVAL: Duration = CACHE_TTL;

    fn cache_id(&self) -> String {
        "default".to_string()
    }

    fn fetchable(&self) -> bool {
        !cfg!(windows) && platform::resolve_binary("sudo").is_some()
    }

    fn fetch(&self) -> Option<bool> {
        if cfg!(windows) {
            return None;
        }

        let sudo = platform::resolve_binary("sudo")?;
        let status = Command::new(sudo)
            .args(SUDO_ARGS)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()?;
        Some(status.success())
    }
}

impl<S: SudoScheme> Module for Sudo<S> {
    /// Whether sudo credentials are cached.
    type Data = bool;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn fetch(&self) -> bool {
        !S::sudo_symbol().is_empty()
            && matches!(Cached::new(SudoLookup).load(), Lookup::Ready(true))
    }

    fn sample(&self) -> bool {
        true
    }

    fn render(&self, cached: bool, powerline: &mut Powerline) {
        if let Some(symbol) = display_text::<S>(cached) {
            powerline.add_segment(symbol, Style::simple(S::sudo_fg(), S::sudo_bg()));
        }
    }
}

fn display_text<S: SudoScheme>(cached: bool) -> Option<&'static str> {
    let symbol = S::sudo_symbol();
    (cached && !symbol.is_empty()).then_some(symbol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colors::{black, blue, white};

    struct TestTheme;

    impl DefaultColors for TestTheme {
        fn default_bg() -> Color {
            black()
        }

        fn default_fg() -> Color {
            white()
        }
    }

    impl SudoScheme for TestTheme {
        fn sudo_bg() -> Color {
            blue()
        }
    }

    struct NoSymbolTheme;

    impl DefaultColors for NoSymbolTheme {
        fn default_bg() -> Color {
            black()
        }

        fn default_fg() -> Color {
            white()
        }
    }

    impl SudoScheme for NoSymbolTheme {
        fn sudo_symbol() -> &'static str {
            ""
        }
    }

    #[test]
    fn cached_credentials_show_the_marker() {
        assert_eq!(display_text::<TestTheme>(true), Some("⚿"));
    }

    #[test]
    fn missing_credentials_hide_the_marker() {
        assert_eq!(display_text::<TestTheme>(false), None);
    }

    #[test]
    fn an_empty_symbol_hides_the_marker() {
        assert_eq!(display_text::<NoSymbolTheme>(true), None);
    }

    #[test]
    fn sample_shows_the_marker() {
        let sudo = Sudo::<TestTheme>::new();
        assert_eq!(display_text::<TestTheme>(sudo.sample()), Some("⚿"));
    }

    #[test]
    fn check_is_non_interactive_and_does_not_update_or_run_a_command() {
        assert_eq!(SUDO_ARGS, ["-N", "-n", "-v"]);
    }

    #[test]
    fn lookup_uses_one_stable_cache_entry() {
        assert_eq!(SudoLookup.cache_id(), "default");
        assert_eq!(SudoLookup::KIND, "sudo");
        assert_eq!(SudoLookup::TTL, CACHE_TTL);
    }
}
