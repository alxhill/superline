use std::marker::PhantomData;

use crate::colors::Color;
use crate::modules::Module;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

pub struct Unknown<S: UnknownScheme> {
    name: String,
    scheme: PhantomData<S>,
}

pub trait UnknownScheme: DefaultColors {
    fn unknown_fg() -> Color {
        Self::alert_fg()
    }

    fn unknown_bg() -> Color {
        Self::alert_bg()
    }
}

impl<S: UnknownScheme> Unknown<S> {
    pub fn new(name: String) -> Unknown<S> {
        Unknown {
            name,
            scheme: PhantomData,
        }
    }
}

impl<S: UnknownScheme> Module for Unknown<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        powerline.add_segment(
            format!("unknown module: {}", self.name),
            Style::simple(S::unknown_fg(), S::unknown_bg()),
        );
    }
}
