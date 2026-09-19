use std::marker::PhantomData;

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{utils, Segments, Style};

use super::Module;

pub struct Host<S: HostScheme> {
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

impl<S: HostScheme> Default for Host<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: HostScheme> Host<S> {
    pub fn new() -> Host<S> {
        Host {
            show_on_local: true,
            scheme: PhantomData,
        }
    }

    pub fn show_on_remote_shell() -> Host<S> {
        Host {
            show_on_local: false,
            scheme: PhantomData,
        }
    }
}

impl<S: HostScheme> Module for Host<S> {
    fn append_segments(&mut self, segments: &mut Segments) {
        if self.show_on_local || utils::is_remote_shell() {
            if let Ok(host) = hostname::get() {
                segments.add_segment(
                    host.to_str().unwrap(),
                    Style::simple(S::hostname_fg(), S::hostname_bg()),
                );
            }
        }
    }
}
