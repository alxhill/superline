use std::marker::PhantomData;
use std::time::Duration;

use crate::colors::Color;
use crate::modules::Module;
use crate::themes::DefaultColors;
use crate::{Segments, Style};

pub struct LastCmdDuration<S> {
    min_display_time: Duration,
    cmd_duration: Option<Duration>,
    scheme: PhantomData<S>,
}

pub trait LastCmdDurationScheme: DefaultColors {
    const DEFAULT_TIME_ICON: &'static str = "\u{f1acc}";
    fn time_bg() -> Color {
        Self::default_bg()
    }
    fn time_fg() -> Color {
        Self::default_fg()
    }

    fn time_icon() -> &'static str {
        Self::DEFAULT_TIME_ICON // clock with ! after
    }
}

impl<S: LastCmdDurationScheme> LastCmdDuration<S> {
    pub fn new(cmd_duration: Option<Duration>, min_duration: Duration) -> LastCmdDuration<S> {
        LastCmdDuration {
            min_display_time: min_duration,
            cmd_duration,
            scheme: PhantomData,
        }
    }
}

impl<S: LastCmdDurationScheme> Module for LastCmdDuration<S> {
    fn append_segments(&mut self, segments: &mut Segments) {
        if let Some(cmd_dur) = self.cmd_duration {
            if cmd_dur > self.min_display_time {
                segments.add_short_segment(
                    format!(" {}{}", nice_duration(cmd_dur), S::time_icon()),
                    Style::simple(S::time_fg(), S::time_bg()),
                );
            }
        }
    }
}

fn nice_duration(dur: Duration) -> String {
    if dur > Duration::from_secs(60) {
        return format!("{}m{}s", dur.as_secs() / 60, dur.as_secs() % 60);
    }

    if dur > Duration::from_secs(1) {
        return format!("{:.2}s", dur.as_millis() as f32 / 1000f32);
    }

    if dur > Duration::from_millis(1) {
        return format!("{}ms", dur.as_millis());
    }

    format!("{}µs", dur.as_millis())
}
