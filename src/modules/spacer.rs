use std::marker::PhantomData;

use crate::colors::Color;
use crate::modules::Module;
use crate::themes::DefaultColors;
use crate::{Segments, Style};

#[derive(Copy, Clone)]
pub struct Spacer<S: SpacerScheme> {
    scheme: PhantomData<S>,
    large: bool,
}

pub trait SpacerScheme: DefaultColors {
    fn color_fg() -> Color {
        Self::default_fg()
    }
    fn color_bg() -> Color {
        Self::default_bg()
    }
}

impl<S: SpacerScheme> Spacer<S> {
    pub fn large() -> Spacer<S> {
        Spacer {
            scheme: PhantomData,
            large: true,
        }
    }

    pub fn small() -> Spacer<S> {
        Spacer {
            scheme: PhantomData,
            large: false,
        }
    }
}

impl<S: SpacerScheme> Module for Spacer<S> {
    fn append_segments(&mut self, segments: &mut Segments) {
        if self.large {
            segments.add_segment("", Style::simple(S::color_fg(), S::color_bg()));
        } else {
            segments.add_short_segment("", Style::simple(S::color_fg(), S::color_bg()));
        }
    }
}
