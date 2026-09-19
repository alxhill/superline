use std::marker::PhantomData;

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{platform, Segments, Style};

use super::Module;

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
    fn append_segments(&mut self, segments: &mut Segments) {
        if platform::cwd_is_readonly() {
            segments.add_segment(
                S::readonly_symbol(),
                Style::simple(S::readonly_fg(), S::readonly_bg()),
            );
        }
    }
}
