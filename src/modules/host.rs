use std::marker::PhantomData;

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{utils, Powerline, Style};

use super::Module;

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
    fn append_segments(&mut self, powerline: &mut Powerline) {
        if self.show_on_local || utils::is_remote_shell() {
            if let Some(host) = current_hostname() {
                powerline.add_segment(host, Style::simple(S::hostname_fg(), S::hostname_bg()));
            }
        }
    }
}

fn current_hostname() -> Option<String> {
    hostname::get().ok().and_then(hostname_text)
}

fn hostname_text(host: std::ffi::OsString) -> Option<String> {
    let host = host.to_string_lossy().into_owned();
    (!host.is_empty()).then_some(host)
}
