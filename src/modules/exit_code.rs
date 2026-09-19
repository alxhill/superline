use std::env;
use std::marker::PhantomData;

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{Segments, Style};

use super::Module;

pub struct ExitCode<S: ExitCodeScheme> {
    scheme: PhantomData<S>,
}

pub trait ExitCodeScheme: DefaultColors {
    fn exit_code_bg() -> Color {
        Self::default_bg()
    }
    fn exit_code_fg() -> Color {
        Self::default_fg()
    }
}

impl<S: ExitCodeScheme> Default for ExitCode<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: ExitCodeScheme> ExitCode<S> {
    pub fn new() -> ExitCode<S> {
        ExitCode {
            scheme: PhantomData,
        }
    }
}

impl<S: ExitCodeScheme> Module for ExitCode<S> {
    fn append_segments(&mut self, segments: &mut Segments) {
        if let Some(exit_code) = env::args().nth(1).as_deref() {
            if exit_code != "0" {
                segments.add_segment(
                    exit_code,
                    Style::simple(S::exit_code_fg(), S::exit_code_bg()),
                )
            }
        }
    }
}
