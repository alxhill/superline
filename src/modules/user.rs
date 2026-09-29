use std::marker::PhantomData;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::themes::DefaultColors;
use crate::{platform, utils, Powerline, Style};

use super::{DefaultPadding, Module};

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

/// The user running the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUser {
    pub name: String,
    pub is_root: bool,
}

impl<S: UserScheme> Module for Username<S> {
    /// The current user, or `None` when hidden or unknown.
    type Data = Option<CurrentUser>;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn fetch(&self) -> Option<CurrentUser> {
        if !(self.show_on_local || utils::is_remote_shell()) {
            return None;
        }
        let is_root = platform::is_root();
        platform::current_username().map(|name| CurrentUser { name, is_root })
    }

    fn sample(&self) -> Option<CurrentUser> {
        Some(CurrentUser {
            name: "alex".into(),
            is_root: false,
        })
    }

    fn render(&self, user: Option<CurrentUser>, powerline: &mut Powerline) {
        if let Some(user) = user {
            let bg = if user.is_root {
                S::username_root_bg()
            } else {
                S::username_bg()
            };
            powerline.add_segment(user.name, Style::simple(S::username_fg(), bg));
        }
    }
}
