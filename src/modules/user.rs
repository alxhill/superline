use std::marker::PhantomData;

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{platform, utils, Powerline, Style};

use super::Module;

/// Displays the current username, using a distinct background for root.
///
/// `User` remains as a type alias for source compatibility with the original
/// superline API. New code should use `Username`, which matches the name used
/// by the configuration format.
pub struct Username<S: UserScheme> {
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

/// The original name of [`Username`].
pub type User<S> = Username<S>;

impl<S: UserScheme> Default for Username<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: UserScheme> Username<S> {
    pub fn new() -> Username<S> {
        Username {
            show_on_local: true,
            scheme: PhantomData,
        }
    }

    pub fn show_on_remote_shell() -> Username<S> {
        Username {
            show_on_local: false,
            scheme: PhantomData,
        }
    }
}

impl<S: UserScheme> Module for Username<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        if self.show_on_local || utils::is_remote_shell() {
            let bg = if platform::is_root() {
                S::username_root_bg()
            } else {
                S::username_bg()
            };

            if let Some(name) = platform::current_username() {
                powerline.add_segment(name, Style::simple(S::username_fg(), bg));
            }
        }
    }
}
