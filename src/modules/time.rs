use std::marker::PhantomData;

use chrono::Local;

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{Segments, Style};

use super::Module;

pub struct Time<S: TimeScheme> {
    time_format: String,
    scheme: PhantomData<S>,
}

pub trait TimeScheme: DefaultColors {
    fn time_bg() -> Color {
        Self::default_bg()
    }
    fn time_fg() -> Color {
        Self::default_fg()
    }
}

impl<S: TimeScheme> Default for Time<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: TimeScheme> Time<S> {
    pub fn new() -> Time<S> {
        Time {
            time_format: "%H:%M:%S".into(),
            scheme: PhantomData,
        }
    }

    pub fn with_time_format(time_format: String) -> Time<S> {
        Time {
            time_format,
            scheme: PhantomData,
        }
    }
}

impl<S: TimeScheme> Module for Time<S> {
    fn append_segments(&mut self, segments: &mut Segments) {
        let now = Local::now().format(&self.time_format).to_string();
        segments.add_segment(now, Style::simple(S::time_fg(), S::time_bg()));
    }
}
