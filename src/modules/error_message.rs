use std::marker::PhantomData;

use crate::colors::Color;
use crate::modules::Module;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

pub struct ErrorMessage<S: ErrorMessageScheme> {
    message: String,
    scheme: PhantomData<S>,
}

pub trait ErrorMessageScheme: DefaultColors {
    fn error_message_fg() -> Color {
        Self::alert_fg()
    }

    fn error_message_bg() -> Color {
        Self::alert_bg()
    }
}

impl<S: ErrorMessageScheme> ErrorMessage<S> {
    pub fn new(message: String) -> ErrorMessage<S> {
        ErrorMessage {
            message,
            scheme: PhantomData,
        }
    }
}

impl<S: ErrorMessageScheme> Module for ErrorMessage<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        powerline.add_segment(
            &self.message,
            Style::simple(S::error_message_fg(), S::error_message_bg()),
        );
    }
}
