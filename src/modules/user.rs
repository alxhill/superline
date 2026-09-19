use std::marker::PhantomData;

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{platform, utils, Segments, Style};

use super::Module;

pub struct User<S: UserScheme> {
    show_on_local: bool,
    scheme: PhantomData<S>,
}

pub trait UserScheme: DefaultColors {
    fn username_root_bg() -> Color {
        Self::default_bg()
    }
    fn username_bg() -> Color {
        Self::default_bg()
    }
    fn username_fg() -> Color {
        Self::default_fg()
    }
}

impl<S: UserScheme> Default for User<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: UserScheme> User<S> {
    pub fn new() -> User<S> {
        User {
            show_on_local: true,
            scheme: PhantomData,
        }
    }

    pub fn show_on_remote_shell() -> User<S> {
        User {
            show_on_local: false,
            scheme: PhantomData,
        }
    }
}

impl<S: UserScheme> Module for User<S> {
    fn append_segments(&mut self, segments: &mut Segments) {
        if self.show_on_local || utils::is_remote_shell() {
            let bg = if platform::is_root() {
                S::username_root_bg()
            } else {
                S::username_bg()
            };

            if let Some(name) = platform::current_username() {
                segments.add_segment(name, Style::simple(S::username_fg(), bg));
            }
        }
    }
}
