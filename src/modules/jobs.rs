use std::marker::PhantomData;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::themes::DefaultColors;
use crate::utils::join_non_empty;
use crate::{Powerline, Style};

use super::{DefaultPadding, Module};

/// Shows the number of running background jobs owned by the current shell.
///
/// The shell supplies the count when it invokes superline. Keeping the lookup
/// in the shell is important: a child process cannot see the parent's job
/// table, and the shell already has the most accurate view of job groups.
///
/// Stopped jobs are left out: some shells keep them in the job table long
/// after they are gone, which would pin the widget to the prompt forever.
pub struct Jobs<S: JobsScheme> {
    count: usize,
    scheme: PhantomData<S>,
}

pub trait JobsScheme: DefaultColors {
    const JOBS_ICON: &'static str = "\u{f085}"; // nf-fa-gears

    fn jobs_fg() -> Color {
        Self::default_fg()
    }

    fn jobs_bg() -> Color {
        Self::default_bg()
    }

    fn jobs_symbol() -> &'static str {
        Self::JOBS_ICON
    }
}

impl<S: JobsScheme> Jobs<S> {
    pub fn new(count: usize) -> Jobs<S> {
        Jobs {
            count,
            scheme: PhantomData,
        }
    }
}

impl<S: JobsScheme> Module for Jobs<S> {
    /// The number of running jobs.
    type Data = usize;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn fetch(&self) -> usize {
        self.count
    }

    fn sample(&self) -> usize {
        2
    }

    fn render(&self, count: usize, powerline: &mut Powerline) {
        if let Some(text) = display_text::<S>(count) {
            powerline.add_segment(text, Style::simple(S::jobs_fg(), S::jobs_bg()));
        }
    }
}

fn display_text<S: JobsScheme>(count: usize) -> Option<String> {
    (count > 0).then(|| join_non_empty([S::jobs_symbol(), count.to_string().as_str()]))
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

    impl JobsScheme for TestTheme {}

    struct NoIconTheme;

    impl DefaultColors for NoIconTheme {
        fn default_bg() -> Color {
            black()
        }

        fn default_fg() -> Color {
            green()
        }
    }

    impl JobsScheme for NoIconTheme {
        fn jobs_symbol() -> &'static str {
            ""
        }
    }

    #[test]
    fn an_empty_icon_leaves_only_the_count() {
        assert_eq!(display_text::<NoIconTheme>(2).as_deref(), Some("2"));
    }

    #[test]
    fn hides_when_no_jobs_are_running() {
        assert_eq!(display_text::<TestTheme>(0), None);
    }

    #[test]
    fn shows_the_count_for_a_single_job() {
        assert_eq!(display_text::<TestTheme>(1).as_deref(), Some("\u{f085} 1"));
    }

    #[test]
    fn shows_the_count_for_multiple_jobs() {
        assert_eq!(display_text::<TestTheme>(3).as_deref(), Some("\u{f085} 3"));
    }
}
