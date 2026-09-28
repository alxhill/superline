use std::marker::PhantomData;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::themes::DefaultColors;
use crate::{platform, Powerline, Style};

use super::{DefaultPadding, Module};

pub struct ReadOnly<S>(PhantomData<S>);

pub trait ReadOnlyScheme: DefaultColors {
    fn readonly_fg() -> Color {
        Self::default_fg()
    }
    fn readonly_bg() -> Color {
        Self::default_bg()
    }

    fn readonly_symbol() -> &'static str {
        ""
    }
}

impl<S: ReadOnlyScheme> Default for ReadOnly<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: ReadOnlyScheme> ReadOnly<S> {
    pub fn new() -> ReadOnly<S> {
        ReadOnly(PhantomData)
    }
}

impl<S: ReadOnlyScheme> Module for ReadOnly<S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        if platform::cwd_is_readonly() {
            powerline.add_segment(
                S::readonly_symbol(),
                Style::simple(S::readonly_fg(), S::readonly_bg()),
            );
        }
    }
}
