use crate::config::SegmentPadding;
use crate::themes::DefaultColors;
use crate::{Color, Powerline, Style};
use std::marker::PhantomData;

use super::{DefaultPadding, Module};

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
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Small.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        powerline.add_segment(
            &self.name,
            Style::simple(S::shellname_fg(), S::shellname_bg()),
        );
    }
}
