use std::marker::PhantomData;

use chrono::{DateTime, Local, NaiveDate};

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::{DefaultPadding, Module};

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
    /// The time to show.
    type Data = DateTime<Local>;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn fetch(&self) -> DateTime<Local> {
        Local::now()
    }

    fn sample(&self) -> DateTime<Local> {
        sample_time()
    }

    fn render(&self, now: DateTime<Local>, powerline: &mut Powerline) {
        let now = now.format(&self.time_format).to_string();
        powerline.add_segment(now, Style::simple(S::time_fg(), S::time_bg()));
    }
}

/// 09:41:00 on a fixed date, in the local time zone.
fn sample_time() -> DateTime<Local> {
    NaiveDate::from_ymd_opt(2026, 1, 1)
        .and_then(|date| date.and_hms_opt(9, 41, 0))
        .and_then(|time| time.and_local_timezone(Local).earliest())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_is_nine_forty_one() {
        assert_eq!(sample_time().format("%H:%M:%S").to_string(), "09:41:00");
    }
}
