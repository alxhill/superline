use std::marker::PhantomData;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::themes::DefaultColors;
use crate::{platform, Powerline, Style};

use super::{DefaultPadding, Module};

pub struct ReadOnly<S>(PhantomData<S>);

pub trait ReadOnlyScheme: DefaultColors {
    const READONLY_ICON: &'static str = "\u{e0a2}"; // the Powerline padlock

    fn readonly_fg() -> Color {
        Self::default_fg()
    }
    fn readonly_bg() -> Color {
        Self::default_bg()
    }

    fn readonly_symbol() -> &'static str {
        Self::READONLY_ICON
    }
}

impl<S: ReadOnlyScheme> Default for ReadOnly<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: ReadOnlyScheme> ReadOnly<S> {
    pub fn new() -> ReadOnly<S> {
        ReadOnly(PhantomData)
    }
}

impl<S: ReadOnlyScheme> Module for ReadOnly<S> {
    /// Whether the current directory is read-only.
    type Data = bool;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn fetch(&self) -> bool {
        // An empty symbol never shows, so skip the check.
        !S::readonly_symbol().is_empty() && platform::cwd_is_readonly()
    }

    fn sample(&self) -> bool {
        true
    }

    fn render(&self, readonly: bool, powerline: &mut Powerline) {
        let symbol = S::readonly_symbol();
        if !symbol.is_empty() && readonly {
            powerline.add_segment(symbol, Style::simple(S::readonly_fg(), S::readonly_bg()));
        }
    }
}
