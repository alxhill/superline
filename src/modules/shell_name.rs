use crate::themes::DefaultColors;
use crate::{Color, Segments, Style};
use std::marker::PhantomData;

use super::Module;

pub struct ShellName<S: ShellScheme> {
    name: String,
    scheme: PhantomData<S>,
}

pub trait ShellScheme: DefaultColors {
    fn shellname_fg() -> Color {
        Self::default_fg()
    }

    fn shellname_bg() -> Color {
        Self::default_bg()
    }
}

impl<S: ShellScheme> ShellName<S> {
    pub fn new(name: String) -> ShellName<S> {
        ShellName {
            name,
            scheme: PhantomData,
        }
    }
}

impl<S: ShellScheme> Module for ShellName<S> {
    fn append_segments(&mut self, segments: &mut Segments) {
        segments.add_short_segment(
            &self.name,
            Style::simple(S::default_fg(), S::shellname_bg()),
        );
    }
}
