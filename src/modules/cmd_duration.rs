use std::marker::PhantomData;
use std::time::Duration;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::modules::{DefaultPadding, Module};
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

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
    /// How long the last command ran, if the shell reported it.
    type Data = Option<Duration>;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Left.into()
    }

    fn fetch(&self) -> Option<Duration> {
        self.cmd_duration
    }

    fn sample(&self) -> Option<Duration> {
        let sample = Duration::from_millis(1234);
        if sample > self.min_display_time {
            Some(sample)
        } else {
            Some(self.min_display_time + sample)
        }
    }

    fn render(&self, cmd_duration: Option<Duration>, powerline: &mut Powerline) {
        if let Some(text) = display_text::<S>(cmd_duration, self.min_display_time) {
            powerline.add_segment(text, Style::simple(S::time_fg(), S::time_bg()));
        }
    }
}

fn display_text<S: LastCmdDurationScheme>(
    cmd_duration: Option<Duration>,
    min_display_time: Duration,
) -> Option<String> {
    cmd_duration
        .filter(|cmd_dur| *cmd_dur > min_display_time)
        .map(|cmd_dur| format!("{}{}", nice_duration(cmd_dur), S::time_icon()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colors::{black, green};

    struct TestTheme;

    impl DefaultColors for TestTheme {
        fn default_bg() -> Color {
            black()
        }

        fn default_fg() -> Color {
            green()
        }
    }

    impl LastCmdDurationScheme for TestTheme {
        fn time_icon() -> &'static str {
            ""
        }
    }

    #[test]
    fn hides_durations_up_to_the_minimum() {
        let min = Duration::from_secs(2);
        assert_eq!(display_text::<TestTheme>(None, min), None);
        assert_eq!(display_text::<TestTheme>(Some(min), min), None);
    }

    #[test]
    fn shows_durations_over_the_minimum() {
        assert_eq!(
            display_text::<TestTheme>(Some(Duration::from_millis(1234)), Duration::ZERO).as_deref(),
            Some("1.23s")
        );
    }

    #[test]
    fn sample_clears_the_minimum() {
        for min in [Duration::ZERO, Duration::from_secs(5)] {
            let module = LastCmdDuration::<TestTheme>::new(None, min);
            assert!(display_text::<TestTheme>(module.sample(), min).is_some());
        }
    }
}
