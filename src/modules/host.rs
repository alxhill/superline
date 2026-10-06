use std::marker::PhantomData;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::themes::DefaultColors;
use crate::{utils, Powerline, Style};

use super::{DefaultPadding, Module};

/// Displays the system hostname.
///
/// `Host` is retained as a type alias for source compatibility with the
/// original superline API. New code should use `Hostname`.
pub struct Hostname<S: HostScheme> {
    show_on_local: bool,
    scheme: PhantomData<S>,
}

pub trait HostScheme: DefaultColors {
    fn hostname_fg() -> Color {
        Self::default_fg()
    }
    fn hostname_bg() -> Color {
        Self::default_bg()
    }
}

/// The original name of [`Hostname`].
pub type Host<S> = Hostname<S>;

impl<S: HostScheme> Default for Hostname<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: HostScheme> Hostname<S> {
    pub fn new() -> Hostname<S> {
        Hostname {
            show_on_local: true,
            scheme: PhantomData,
        }
    }

    pub fn show_on_remote_shell() -> Hostname<S> {
        Hostname {
            show_on_local: false,
            scheme: PhantomData,
        }
    }
}

impl<S: HostScheme> Module for Hostname<S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        if self.show_on_local || utils::is_remote_shell() {
            if let Some(host) = current_hostname() {
                powerline.add_segment(host, Style::simple(S::hostname_fg(), S::hostname_bg()));
            }
        }
    }
}

fn current_hostname() -> Option<String> {
    local_hostname().or_else(|| hostname::get().ok().and_then(hostname_text))
}

/// The Bonjour name set in System Settings > Sharing (`scutil --get
/// LocalHostName`). Unlike the kernel hostname it does not change with the
/// network's DHCP or reverse DNS answers.
#[cfg(target_os = "macos")]
fn local_hostname() -> Option<String> {
    use system_configuration::core_foundation::base::TCFType;
    use system_configuration::core_foundation::string::CFString;
    use system_configuration::sys::dynamic_store_copy_specific::SCDynamicStoreCopyLocalHostName;

    // SAFETY: a null store asks SystemConfiguration for a temporary session.
    // The returned string follows the create rule, so it is owned here.
    let name = unsafe { SCDynamicStoreCopyLocalHostName(std::ptr::null()) };
    if name.is_null() {
        return None;
    }
    let name = unsafe { CFString::wrap_under_create_rule(name) }.to_string();
    (!name.is_empty()).then_some(name)
}

#[cfg(not(target_os = "macos"))]
fn local_hostname() -> Option<String> {
    None
}

fn hostname_text(host: std::ffi::OsString) -> Option<String> {
    let host = host.to_string_lossy();
    let host = if cfg!(target_os = "macos") {
        short_hostname(&host)
    } else {
        &host
    };
    (!host.is_empty()).then(|| host.to_owned())
}

/// Matches `hostname -s`: everything before the first dot.
fn short_hostname(host: &str) -> &str {
    host.split('.').next().unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_hostname_drops_domain() {
        assert_eq!(short_hostname("mac.local"), "mac");
        assert_eq!(short_hostname("mac.corp.example.com"), "mac");
        assert_eq!(short_hostname("mac"), "mac");
    }
}
