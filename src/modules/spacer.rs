use std::marker::PhantomData;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::modules::{DefaultPadding, Module};
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

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
    fn default_padding(&self) -> DefaultPadding {
        if self.large {
            SegmentPadding::Large.into()
        } else {
            SegmentPadding::Small.into()
        }
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        powerline.add_segment("", Style::simple(S::color_fg(), S::color_bg()));
    }
}
