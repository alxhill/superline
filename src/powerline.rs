use std::fmt;
use std::fmt::{Display, Write};
use std::panic;
use std::thread;
use std::time::Duration;

use crate::colors::Color;
use crate::config;
use crate::config::{LineSegment, SeparatorStyle, TerminalRuntimeMetadata};
use crate::modules::{
    Cargo, Cmd, Cwd, ErrorMessage, Git, Host, Java, LastCmdDuration, Module, Node, Pr, Python,
    ReadOnly, ShellName, Spacer, Time, Unknown, Usage, UsageWindows, User,
};
use crate::terminal::*;
use crate::themes::CompleteTheme;

#[derive(Clone)]
pub struct Style {
    pub fg: FgColor,
    pub bg: BgColor,
    pub sep_fg: FgColor,
}

impl Style {
    pub fn simple(fg: Color, bg: Color) -> Style {
        Style {
            fg: fg.into(),
            bg: bg.into(),
            sep_fg: bg.into(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Separator {
    Chevron,
    Round,
    AngleLine,
}

#[derive(Debug, Eq, PartialEq)]
enum Direction {
    Left,
    Right,
}

impl Separator {
    fn for_direction(&self, direction: Direction) -> char {
        match (self, direction) {
            (Separator::Chevron, Direction::Right) => '\u{e0b0}',
            (Separator::Chevron, Direction::Left) => '\u{e0b2}',
            (Separator::Round, Direction::Right) => '\u{e0b4}',
            (Separator::Round, Direction::Left) => '\u{e0b6}',
            (Separator::AngleLine, Direction::Right) => '\u{e0b1}',
            (Separator::AngleLine, Direction::Left) => '\u{e0b3}',
        }
    }
}

impl From<&SeparatorStyle> for Separator {
    fn from(style: &SeparatorStyle) -> Self {
        match style {
            SeparatorStyle::Chevron => Separator::Chevron,
            SeparatorStyle::Round => Separator::Round,
            SeparatorStyle::AngleLine => Separator::AngleLine,
        }
    }
}

/// One segment as a module describes it, before it is laid out. Everything
/// that depends on the neighbouring segments or on the shell (separators,
/// escape wrapping) is decided when it is pushed onto a [`Powerline`].
enum Segment {
    Text {
        text: String,
        style: Style,
        /// Whether to pad the text with a space on each side.
        spaces: bool,
    },
    Hyperlink {
        label: String,
        url: String,
        style: Style,
        marker: Option<(String, Color)>,
    },
}

/// The segments one module contributes, in order.
///
/// Modules build their output here rather than writing into a [`Powerline`]
/// directly, so that every module of a row can run on its own thread and the
/// row is assembled afterwards in config order.
#[derive(Default)]
pub struct Segments {
    items: Vec<Segment>,
}

impl Segments {
    pub fn add_segment<D: Display>(&mut self, seg: D, style: Style) {
        self.items.push(Segment::Text {
            text: seg.to_string(),
            style,
            spaces: true,
        });
    }

    pub fn add_short_segment<D: Display>(&mut self, seg: D, style: Style) {
        self.items.push(Segment::Text {
            text: seg.to_string(),
            style,
            spaces: false,
        });
    }

    /// Adds a segment whose text is an OSC 8 terminal hyperlink, optionally
    /// followed by a coloured marker glyph (e.g. the PR status dot) that shares
    /// this segment's background instead of getting one of its own.
    pub fn add_hyperlink_segment(
        &mut self,
        label: &str,
        url: &str,
        style: Style,
        marker: Option<(&str, Color)>,
    ) {
        self.items.push(Segment::Hyperlink {
            label: label.to_string(),
            url: url.to_string(),
            style,
            marker: marker.map(|(glyph, color)| (glyph.to_string(), color)),
        });
    }
}

/// One entry of a row's config, ready to be applied to a [`Powerline`].
/// Module steps carry the module itself so it can be run ahead of the layout
/// pass; the other steps only affect layout and are applied in order.
enum Step {
    Module(Box<dyn Module + Send>),
    Separator(Separator),
    Padding(usize),
}

impl Step {
    fn module<M: Module + Send + 'static>(module: M) -> Step {
        Step::Module(Box::new(module))
    }
}

pub struct PowerlineBuilder {
    powerline: Powerline,
}

pub trait PowerlineShellBuilder {
    fn set_shell(self, shell: Shell) -> impl PowerlineLeftBuilder;
}

pub trait PowerlineLeftBuilder: PowerlineRightBuilder {
    fn start_right(self) -> impl PowerlineRightBuilder;
}

pub trait PowerlineRightBuilder {
    fn add_module<M: Module>(self, module: M) -> Self;
    fn change_separator(self, separator: Separator) -> Self;
    fn add_padding(self, padding: usize) -> Self;

    fn render(self, columns: usize);
}

impl PowerlineShellBuilder for PowerlineBuilder {
    fn set_shell(self, shell: Shell) -> impl PowerlineLeftBuilder {
        SHELL.set(shell).expect("Failed to set shell");
        self
    }
}

impl PowerlineRightBuilder for PowerlineBuilder {
    fn add_module<M: Module>(mut self, module: M) -> Self {
        self.powerline.add_module(module);
        self
    }

    fn change_separator(mut self, separator: Separator) -> Self {
        self.powerline.set_separator(separator);
        self
    }

    fn add_padding(mut self, padding: usize) -> Self {
        self.powerline.add_padding(padding);
        self
    }

    fn render(mut self, columns: usize) {
        self.powerline.print_left();
        self.powerline.print_padding(columns);
        self.powerline.print_right();
        println!();
    }
}

impl PowerlineLeftBuilder for PowerlineBuilder {
    fn start_right(mut self) -> impl PowerlineRightBuilder {
        self.powerline.start_right();
        self
    }
}

pub struct Powerline {
    left_buffer: String,
    left_columns: usize, // counting only visible characters...hopefully
    right_buffer: String,
    right_columns: usize, // likewise for the right buffer
    last_style: Option<Style>,
    last_style_right: Option<Style>,
    separator: Separator,
    direction: Direction,
    last_padding: bool,
}

impl Default for Powerline {
    fn default() -> Self {
        Self::new()
    }
}

impl Powerline {
    pub fn new() -> Powerline {
        Powerline {
            left_buffer: String::with_capacity(512),
            left_columns: 0,
            right_buffer: String::with_capacity(512),
            right_columns: 0,
            last_style: None,
            last_style_right: None,
            separator: Separator::Chevron,
            direction: Direction::Left,
            last_padding: false,
        }
    }

    pub fn builder() -> impl PowerlineShellBuilder {
        PowerlineBuilder {
            powerline: Default::default(),
        }
    }

    pub fn from_conf<T: CompleteTheme>(
        conf: &config::CommandLine,
        runtime_data: impl TerminalRuntimeMetadata,
    ) -> Self {
        Self::from_row::<T>(conf, &runtime_data)
    }

    /// Builds one powerline per row. Rows are independent, so they are built
    /// side by side, and within each row the modules run side by side too.
    pub fn from_conf_rows<T: CompleteTheme>(
        rows: &[config::CommandLine],
        runtime_data: &(impl TerminalRuntimeMetadata + Sync),
    ) -> Vec<Self> {
        if rows.len() < 2 {
            return rows
                .iter()
                .map(|row| Self::from_row::<T>(row, runtime_data))
                .collect();
        }
        thread::scope(|scope| {
            let handles = rows
                .iter()
                .map(|row| scope.spawn(move || Self::from_row::<T>(row, runtime_data)))
                .collect::<Vec<_>>();
            handles.into_iter().map(join).collect()
        })
    }

    fn from_row<T: CompleteTheme>(
        conf: &config::CommandLine,
        runtime_data: &impl TerminalRuntimeMetadata,
    ) -> Self {
        let mut powerline = Powerline::new();
        powerline.add_conf_modules::<T>(&conf.left, runtime_data);

        if let Some(right_modules) = &conf.right {
            powerline.start_right();
            powerline.add_conf_modules::<T>(right_modules, runtime_data);
        }

        powerline
    }

    pub fn set_separator(&mut self, separator: Separator) {
        self.separator = separator;
    }

    #[inline(always)]
    fn write_segment<D: Display>(
        &mut self,
        seg: D,
        style: Style,
        spaces: bool,
        visible_width: Option<usize>,
    ) -> fmt::Result {
        // write the last style's separator on the new style's background
        if self.last_padding {
            write!(
                self.left_buffer,
                "{}{}{}",
                style.sep_fg,
                self.separator.for_direction(Direction::Left),
                style.bg
            )?;
            self.last_padding = false;
        }

        if let Some(Style { sep_fg, .. }) = self.last_style {
            self.left_columns += 1;
            write!(
                self.left_buffer,
                "{}{}{}",
                style.bg,
                sep_fg,
                self.separator.for_direction(Direction::Right)
            )?;
        } else {
            write!(self.left_buffer, "{}", style.bg)?;
        };

        if self.last_style.as_ref().map(|s| s.sep_fg) != Some(style.fg) {
            write!(self.left_buffer, "{}", style.fg)?;
        }

        let orig_len = self.left_buffer.len();
        if spaces {
            write!(self.left_buffer, " {} ", seg)?;
        } else {
            write!(self.left_buffer, "{}", seg)?;
        };

        // attempt to account for symbols in the segment by assuming all chars
        // printed are of length 1. When the segment carries invisible escapes
        // (e.g. a hyperlink) the caller passes the real visible width instead.
        self.left_columns += visible_width
            .map(|width| width + if spaces { 2 } else { 0 })
            .unwrap_or_else(|| self.left_buffer[orig_len..].chars().count());

        self.last_style = Some(style);
        Ok(())
    }

    fn write_segment_right<D: Display>(
        &mut self,
        seg: D,
        style: Style,
        spaces: bool,
        visible_width: Option<usize>,
    ) -> fmt::Result {
        // write the separator directly onto the current background
        write!(
            self.right_buffer,
            "{}{}{}",
            style.bg.transpose(),
            self.separator.for_direction(Direction::Left),
            style.bg
        )?;
        self.right_columns += 1;

        if self.last_style_right.as_ref().map(|s| s.sep_fg) != Some(style.fg) {
            write!(self.right_buffer, "{}", style.fg)?;
        }

        let orig_len = self.right_buffer.len();
        if spaces {
            write!(self.right_buffer, " {} ", seg)?;
        } else {
            write!(self.right_buffer, "{}", seg)?;
        };

        // attempt to account for symbols in the segment by assuming all chars
        // printed are of length 1 (so multi-byte chars don't over-inflate the
        // size). When the segment carries invisible escapes (e.g. a hyperlink)
        // the caller passes the real visible width instead.
        self.right_columns += visible_width
            .map(|width| width + if spaces { 2 } else { 0 })
            .unwrap_or_else(|| self.right_buffer[orig_len..].chars().count());

        self.last_style_right = Some(style);
        Ok(())
    }

    pub fn add_segment<D: Display>(&mut self, seg: D, style: Style) {
        self.push(Segment::Text {
            text: seg.to_string(),
            style,
            spaces: true,
        });
    }

    pub fn add_short_segment<D: Display>(&mut self, seg: D, style: Style) {
        self.push(Segment::Text {
            text: seg.to_string(),
            style,
            spaces: false,
        });
    }

    /// See [`Segments::add_hyperlink_segment`].
    pub fn add_hyperlink_segment(
        &mut self,
        label: &str,
        url: &str,
        style: Style,
        marker: Option<(&str, Color)>,
    ) {
        let mut segments = Segments::default();
        segments.add_hyperlink_segment(label, url, style, marker);
        self.add_segments(segments);
    }

    /// Lays out `segments` on the current side, in order.
    pub fn add_segments(&mut self, segments: Segments) {
        for segment in segments.items {
            self.push(segment);
        }
    }

    fn push(&mut self, segment: Segment) {
        let (seg, style, spaces, visible_width) = match segment {
            Segment::Text {
                text,
                style,
                spaces,
            } => (text, style, spaces, None),
            Segment::Hyperlink {
                label,
                url,
                style,
                marker,
            } => {
                // The OSC and colour escapes are invisible, so the visible
                // width is computed from the label and the marker glyph alone
                // to keep column accounting (and right-prompt padding) correct.
                let mut visible_width = label.chars().count();
                let link = format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url, label);
                let seg = match marker {
                    Some((glyph, color)) => {
                        // separating space + the glyph itself
                        visible_width += 1 + glyph.chars().count();
                        // Colour the glyph, then restore the segment's
                        // foreground so the terminal state matches what the
                        // renderer records for it.
                        format!("{} {}{}{}", link, FgColor::from(color), glyph, style.fg)
                    }
                    None => link,
                };
                (seg, style, true, Some(visible_width))
            }
        };
        let _ = match self.direction {
            Direction::Left => self.write_segment(seg, style, spaces, visible_width),
            Direction::Right => self.write_segment_right(seg, style, spaces, visible_width),
        };
    }

    pub fn start_right(&mut self) {
        assert_eq!(self.direction, Direction::Left);
        self.close_left_buffer();
        self.direction = Direction::Right;
    }

    pub fn add_module<M: Module>(&mut self, mut module: M) {
        let mut segments = Segments::default();
        module.append_segments(&mut segments);
        self.add_segments(segments);
    }

    fn add_conf_modules<T: CompleteTheme>(
        &mut self,
        modules: &[LineSegment],
        runtime_data: &impl TerminalRuntimeMetadata,
    ) {
        let steps = modules
            .iter()
            .map(|module| plan_step::<T>(module, runtime_data))
            .collect::<Vec<_>>();
        self.add_steps(steps);
    }

    /// Runs the modules among `steps` side by side, then applies every step in
    /// order so the row comes out exactly as a sequential build would have it.
    fn add_steps(&mut self, mut steps: Vec<Step>) {
        let mut rendered = run_modules(&mut steps).into_iter();
        for step in steps {
            match step {
                Step::Module(_) => {
                    self.add_segments(rendered.next().expect("one output per module"))
                }
                Step::Separator(separator) => self.set_separator(separator),
                Step::Padding(size) => self.add_padding(size),
            }
        }
    }

    pub fn add_padding(&mut self, len: usize) {
        let padding = vec![" "; len].join("");
        match self.direction {
            Direction::Left => {
                // close out the buffer, write the padding, and leave the next write_segment
                // to handle adding the alternate separator
                self.close_left_buffer();
                self.left_columns += len + 1;
                let _ = write!(self.left_buffer, "{}{}", Reset, padding);
            }
            Direction::Right => {
                // close out the current blob and write the padding
                if let Some(Style { sep_fg, .. }) = self.last_style_right {
                    write!(
                        self.right_buffer,
                        "{}{}{}{}{}",
                        Reset,
                        sep_fg,
                        self.separator.for_direction(Direction::Right),
                        Reset,
                        padding
                    )
                    .unwrap();
                    self.right_columns += 1;
                } else {
                    write!(self.right_buffer, "{}", padding).unwrap();
                }
                self.right_columns += len;
                self.last_style = None;
            }
        }

        self.last_padding = true;
    }

    pub fn print_left(&mut self) {
        if let Direction::Left = self.direction {
            self.close_left_buffer();
        }

        print!("{}{}", self.left_buffer, Reset);
    }

    pub fn print_padding(&self, total_columns: usize) {
        // no padding if there's no right buffer
        if self.direction == Direction::Left || self.right_buffer.is_empty() {
            return;
        }

        // careful not to underflow
        let padding = total_columns
            .checked_sub(self.left_columns)
            .and_then(|cols| cols.checked_sub(self.right_columns))
            .and_then(|cols| cols.checked_sub(1)) // extra padding for safety
            .unwrap_or(0);

        let padding = vec![" "; padding].join("");

        print!("{}", padding);
    }

    pub fn print_right(&self) {
        // no right buffer
        if self.direction == Direction::Left {
            return;
        }

        print!("{}{}", self.right_buffer, Reset);
    }

    fn close_left_buffer(&mut self) {
        // close out the left buffer with the right separator
        if let Some(Style { sep_fg, .. }) = self.last_style {
            write!(
                self.left_buffer,
                "{}{}{}{}",
                Reset,
                sep_fg,
                self.separator.for_direction(Direction::Right),
                Reset
            )
            .unwrap();
            self.left_columns += 1;
        }
        self.last_style = None;
    }
}

/// Turns one config entry into the step that realises it.
fn plan_step<T: CompleteTheme>(
    module: &LineSegment,
    runtime_data: &impl TerminalRuntimeMetadata,
) -> Step {
    match module {
        LineSegment::SmallSpacer => Step::module(Spacer::<T>::small()),
        LineSegment::LargeSpacer => Step::module(Spacer::<T>::large()),
        LineSegment::Python { version, venv } => Step::module(Python::<T>::new(*version, *venv)),
        LineSegment::Cmd => Step::module(Cmd::<T>::new(runtime_data.last_command_status())),
        LineSegment::Cargo { version } => Step::module(Cargo::<T>::new(*version)),
        LineSegment::Git { status_timeout_ms } => Step::module(Git::<T>::with_status_timeout(
            Duration::from_millis(*status_timeout_ms),
        )),
        LineSegment::Pr { status } => Step::module(Pr::<T>::new(*status)),
        LineSegment::Separator(style) => Step::Separator(style.into()),
        LineSegment::ReadOnly => Step::module(ReadOnly::<T>::new()),
        LineSegment::Host => Step::module(Host::<T>::new()),
        LineSegment::Shell => Step::module(ShellName::<T>::new(runtime_data.shell_name())),
        LineSegment::User => Step::module(User::<T>::new()),
        LineSegment::Padding(size) => Step::Padding(*size),
        LineSegment::Time { format } => match format {
            Some(format) => Step::module(Time::<T>::with_time_format(format.clone())),
            None => Step::module(Time::<T>::new()),
        },
        LineSegment::AiUsage {
            provider,
            session,
            weekly,
            fable,
            display,
            threshold,
            session_label,
            weekly_label,
            fable_label,
            credits,
            credits_display,
            credits_label,
            credits_only_when_limited,
            session_time_remaining,
            session_time_remaining_only_at_limit,
        } => Step::module(Usage::<T>::new(
            *provider,
            UsageWindows::new(
                UsageWindows::session(*session, session_label.clone()),
                UsageWindows::weekly(*weekly, weekly_label.clone()),
                UsageWindows::fable(*fable, fable_label.clone()),
                UsageWindows::credits(
                    *credits,
                    credits_label.clone(),
                    credits_display.unwrap_or(*display),
                    *credits_only_when_limited,
                ),
                *provider,
            ),
            *display,
            *threshold,
            *session_time_remaining,
            *session_time_remaining_only_at_limit,
        )),
        LineSegment::LastCmdDuration { min_run_time } => Step::module(LastCmdDuration::<T>::new(
            runtime_data.last_command_duration(),
            Duration::from_millis(*min_run_time),
        )),
        LineSegment::Cwd {
            max_length,
            wanted_seg_num,
            resolve_symlinks,
        } => Step::module(Cwd::<T>::new(
            *max_length,
            *wanted_seg_num,
            *resolve_symlinks,
        )),
        LineSegment::Node { version } => Step::module(Node::<T>::new(*version)),
        LineSegment::Java { version, jdk } => Step::module(Java::<T>::new(*version, *jdk)),
        LineSegment::Error { message } => Step::module(ErrorMessage::<T>::new(message.clone())),
        LineSegment::Unknown { name } => Step::module(Unknown::<T>::new(name.clone())),
    }
}

/// Runs every module among `steps`, each on its own thread when there is more
/// than one, and returns their segments in step order.
///
/// Module work is dominated by waiting: on the filesystem, on a child process,
/// on a cache refresh. Run side by side, a row takes as long as its slowest
/// module rather than the sum of all of them, and a git status walk that is
/// allowed to block for a while no longer holds back the segments after it.
fn run_modules(steps: &mut [Step]) -> Vec<Segments> {
    let count = steps
        .iter()
        .filter(|step| matches!(step, Step::Module(_)))
        .count();
    let modules = steps.iter_mut().filter_map(|step| match step {
        Step::Module(module) => Some(module),
        Step::Separator(_) | Step::Padding(_) => None,
    });
    if count < 2 {
        return modules
            .map(|module| collect_segments(module.as_mut()))
            .collect();
    }
    thread::scope(|scope| {
        let handles = modules
            .map(|module| scope.spawn(move || collect_segments(module.as_mut())))
            .collect::<Vec<_>>();
        handles.into_iter().map(join).collect()
    })
}

fn collect_segments(module: &mut (dyn Module + Send)) -> Segments {
    let mut segments = Segments::default();
    module.append_segments(&mut segments);
    segments
}

/// Waits for a worker and surfaces its panic on the caller, exactly as if the
/// work had run inline.
fn join<R>(handle: thread::ScopedJoinHandle<'_, R>) -> R {
    handle
        .join()
        .unwrap_or_else(|payload| panic::resume_unwind(payload))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;
    use crate::terminal::{Shell, SHELL};

    /// How many probes are inside `append_segments` at once, and the most
    /// there have ever been.
    #[derive(Default)]
    struct Concurrency {
        running: AtomicUsize,
        peak: AtomicUsize,
    }

    /// A module that takes `delay` to produce one segment.
    struct Probe {
        text: &'static str,
        delay: Duration,
        concurrency: Arc<Concurrency>,
    }

    impl Module for Probe {
        fn append_segments(&mut self, segments: &mut Segments) {
            let running = self.concurrency.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.concurrency.peak.fetch_max(running, Ordering::SeqCst);
            thread::sleep(self.delay);
            self.concurrency.running.fetch_sub(1, Ordering::SeqCst);
            segments.add_segment(self.text, Style::simple(Color(15), Color(4)));
        }
    }

    fn probe(text: &'static str, delay_ms: u64, concurrency: &Arc<Concurrency>) -> Probe {
        Probe {
            text,
            delay: Duration::from_millis(delay_ms),
            concurrency: concurrency.clone(),
        }
    }

    fn bare_shell() {
        let _ = SHELL.set(Shell::Bare);
    }

    #[test]
    fn modules_run_side_by_side() {
        bare_shell();
        let concurrency = Arc::new(Concurrency::default());
        let steps = ["a", "b", "c"]
            .into_iter()
            .map(|text| Step::module(probe(text, 200, &concurrency)))
            .collect();

        Powerline::new().add_steps(steps);

        assert_eq!(concurrency.peak.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn a_row_built_side_by_side_matches_a_sequential_build() {
        bare_shell();
        let concurrency = Arc::new(Concurrency::default());
        // The slowest module comes first on each side, so any ordering by
        // completion time rather than by config position would show.
        let left = |c: &Arc<Concurrency>| {
            vec![
                Step::module(probe("slow", 100, c)),
                Step::Padding(1),
                Step::module(probe("quick", 0, c)),
                Step::Separator(Separator::Round),
                Step::module(probe("last", 20, c)),
            ]
        };
        let right = |c: &Arc<Concurrency>| {
            vec![
                Step::module(probe("r-slow", 60, c)),
                Step::Padding(2),
                Step::module(probe("r-quick", 0, c)),
            ]
        };

        let mut parallel = Powerline::new();
        parallel.add_steps(left(&concurrency));
        parallel.start_right();
        parallel.add_steps(right(&concurrency));

        let mut sequential = Powerline::new();
        sequential.add_module(probe("slow", 0, &concurrency));
        sequential.add_padding(1);
        sequential.add_module(probe("quick", 0, &concurrency));
        sequential.set_separator(Separator::Round);
        sequential.add_module(probe("last", 0, &concurrency));
        sequential.start_right();
        sequential.add_module(probe("r-slow", 0, &concurrency));
        sequential.add_padding(2);
        sequential.add_module(probe("r-quick", 0, &concurrency));

        assert_eq!(parallel.left_buffer, sequential.left_buffer);
        assert_eq!(parallel.left_columns, sequential.left_columns);
        assert_eq!(parallel.right_buffer, sequential.right_buffer);
        assert_eq!(parallel.right_columns, sequential.right_columns);
    }

    #[test]
    fn hyperlink_segments_lay_out_the_same_from_a_module() {
        bare_shell();
        let style = Style::simple(Color(15), Color(2));

        let mut direct = Powerline::new();
        direct.add_hyperlink_segment(
            "#12",
            "https://example.com/12",
            style.clone(),
            Some(("*", Color(9))),
        );

        let mut segments = Segments::default();
        segments.add_hyperlink_segment(
            "#12",
            "https://example.com/12",
            style,
            Some(("*", Color(9))),
        );
        let mut deferred = Powerline::new();
        deferred.add_segments(segments);

        assert_eq!(direct.left_buffer, deferred.left_buffer);
        assert_eq!(direct.left_columns, deferred.left_columns);
    }

    #[test]
    fn a_panicking_module_still_panics_the_build() {
        struct Boom;
        impl Module for Boom {
            fn append_segments(&mut self, _: &mut Segments) {
                panic!("module failed");
            }
        }
        let concurrency = Arc::new(Concurrency::default());
        let steps = vec![
            Step::module(Boom),
            Step::module(probe("ok", 0, &concurrency)),
        ];

        let result = panic::catch_unwind(panic::AssertUnwindSafe(move || {
            Powerline::new().add_steps(steps)
        }));

        let payload = result.expect_err("the module's panic should surface");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"module failed"));
    }
}
