use std::fmt;
use std::fmt::{Display, Write};
use std::time::Duration;

use unicode_width::UnicodeWidthStr;

use crate::colors::Color;
use crate::config;
use crate::config::{LineSegment, SegmentPadding, SeparatorStyle, TerminalRuntimeMetadata, Widget};
use crate::debug;
use crate::modules::{
    Battery, Cargo, Cmd, Cwd, ErrorMessage, Git, Hostname, Java, Jobs, Kubernetes, LastCmdDuration,
    LocalIp, MemoryUsage, Module, Node, Os, Pr, Python, ReadOnly, ShellName, Spacer, Sudo, Text,
    Time, Unknown, Usage, UsageWindows, Username,
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

/// Spaces drawn on each side of a segment's text.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Padding {
    left: usize,
    right: usize,
}

impl From<SegmentPadding> for Padding {
    fn from(padding: SegmentPadding) -> Padding {
        let (left, right) = match padding {
            SegmentPadding::Small => (0, 0),
            SegmentPadding::Large => (1, 1),
            SegmentPadding::Left => (1, 0),
            SegmentPadding::Right => (0, 1),
        };
        Padding { left, right }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Separator {
    Chevron,
    Round,
    None,
}

#[derive(Debug, Eq, PartialEq)]
enum Direction {
    Left,
    Right,
}

impl Separator {
    fn for_direction(&self, direction: Direction) -> &'static str {
        match (self, direction) {
            (Separator::Chevron, Direction::Right) => "\u{e0b0}",
            (Separator::Chevron, Direction::Left) => "\u{e0b2}",
            (Separator::Round, Direction::Right) => "\u{e0b4}",
            (Separator::Round, Direction::Left) => "\u{e0b6}",
            (Separator::None, _) => "",
        }
    }

    /// Column width of the glyph itself, so a zero-character separator
    /// doesn't throw off left/right prompt alignment.
    fn width(&self) -> usize {
        self.for_direction(Direction::Left).chars().count()
    }
}

impl From<&SeparatorStyle> for Separator {
    fn from(style: &SeparatorStyle) -> Self {
        match style {
            SeparatorStyle::Chevron => Separator::Chevron,
            SeparatorStyle::Round => Separator::Round,
            SeparatorStyle::None => Separator::None,
        }
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
    /// The configured padding of the widget being drawn, which replaces the
    /// default each of its segments asks for.
    widget_padding: Option<SegmentPadding>,
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
            widget_padding: None,
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
        let mut powerline = Powerline::new();
        powerline.add_conf_modules::<T>(&conf.left, &runtime_data);

        if let Some(right_modules) = &conf.right {
            powerline.start_right();
            powerline.add_conf_modules::<T>(right_modules, &runtime_data);
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
        padding: Padding,
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
            self.left_columns += self.separator.width();
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
        write!(
            self.left_buffer,
            "{:left$}{}{:right$}",
            "",
            seg,
            "",
            left = padding.left,
            right = padding.right
        )?;

        // Count terminal cells, so wide characters take two columns. When the
        // segment carries invisible escapes (e.g. a hyperlink) the caller
        // passes the real visible width instead.
        self.left_columns += visible_width
            .map(|width| width + padding.left + padding.right)
            .unwrap_or_else(|| self.left_buffer[orig_len..].width());

        self.last_style = Some(style);
        Ok(())
    }

    fn write_segment_right<D: Display>(
        &mut self,
        seg: D,
        style: Style,
        padding: Padding,
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
        self.right_columns += self.separator.width();

        // The separator above painted this segment's background as the
        // foreground, so the foreground always needs restoring here. Skipping
        // it when it matched the previous segment's background left text
        // drawn in its own background color.
        write!(self.right_buffer, "{}", style.fg)?;

        let orig_len = self.right_buffer.len();
        write!(
            self.right_buffer,
            "{:left$}{}{:right$}",
            "",
            seg,
            "",
            left = padding.left,
            right = padding.right
        )?;

        // Count terminal cells, so wide characters take two columns. When the
        // segment carries invisible escapes (e.g. a hyperlink) the caller
        // passes the real visible width instead.
        self.right_columns += visible_width
            .map(|width| width + padding.left + padding.right)
            .unwrap_or_else(|| self.right_buffer[orig_len..].width());

        self.last_style_right = Some(style);
        Ok(())
    }

    fn push_segment<D: Display>(
        &mut self,
        seg: D,
        style: Style,
        default: SegmentPadding,
        visible_width: Option<usize>,
    ) {
        let padding = self.widget_padding.unwrap_or(default).into();
        let _ = match self.direction {
            Direction::Left => self.write_segment(seg, style, padding, visible_width),
            Direction::Right => self.write_segment_right(seg, style, padding, visible_width),
        };
    }

    /// Adds a segment with a space on each side, unless the widget's padding
    /// is configured.
    pub fn add_segment<D: Display>(&mut self, seg: D, style: Style) {
        self.push_segment(seg, style, SegmentPadding::Large, None);
    }

    /// Adds a segment with no padding, unless the widget's padding is
    /// configured.
    pub fn add_short_segment<D: Display>(&mut self, seg: D, style: Style) {
        self.push_segment(seg, style, SegmentPadding::Small, None);
    }

    /// Adds a segment with `default` padding around its text, unless the
    /// widget's padding is configured.
    pub fn add_padded_segment<D: Display>(
        &mut self,
        seg: D,
        style: Style,
        default: SegmentPadding,
    ) {
        self.push_segment(seg, style, default, None);
    }

    /// Adds a segment whose text is an OSC 8 terminal hyperlink, optionally
    /// followed by a coloured marker glyph (e.g. the PR status dot) that shares
    /// this segment's background instead of getting one of its own. The OSC and
    /// colour escapes are invisible, so the visible width is computed from
    /// `label` and the marker glyph alone to keep column accounting (and
    /// right-prompt padding) correct.
    pub fn add_hyperlink_segment(
        &mut self,
        label: &str,
        url: &str,
        style: Style,
        marker: Option<(&str, Color)>,
    ) {
        let mut visible_width = label.width();
        let link = Hyperlink { url, label }.to_string();
        let seg = match marker {
            Some((glyph, color)) => {
                // separating space + the glyph itself
                visible_width += 1 + glyph.width();
                // Colour the glyph, then restore the segment's foreground so the
                // terminal state matches what the renderer records for it.
                format!("{} {}{}{}", link, FgColor::from(color), glyph, style.fg)
            }
            None => link,
        };
        self.push_segment(seg, style, SegmentPadding::Large, Some(visible_width));
    }

    pub fn start_right(&mut self) {
        assert_eq!(self.direction, Direction::Left);
        self.close_left_buffer();
        self.direction = Direction::Right;
    }

    pub fn add_module<M: Module>(&mut self, mut module: M) {
        let span = debug::span(debug::type_label(std::any::type_name::<M>()));
        module.append_segments(self);
        span.finish();
    }

    fn add_conf_modules<T: CompleteTheme>(
        &mut self,
        widgets: &[Widget],
        runtime_data: &impl TerminalRuntimeMetadata,
    ) {
        for widget in widgets {
            // The config's padding wins over the theme's.
            self.widget_padding = widget
                .padding
                .or_else(|| theme_module(&widget.segment).and_then(T::padding));
            let module = &widget.segment;
            match module {
                LineSegment::Battery => self.add_module(Battery::<T>::new()),
                LineSegment::SmallSpacer => self.add_module(Spacer::<T>::small()),
                LineSegment::LargeSpacer => self.add_module(Spacer::<T>::large()),
                LineSegment::Python { version, venv } => {
                    self.add_module(Python::<T>::new(*version, *venv))
                }
                LineSegment::Cmd => {
                    self.add_module(Cmd::<T>::new(runtime_data.last_command_status()))
                }
                LineSegment::Cargo { version } => self.add_module(Cargo::<T>::new(*version)),
                LineSegment::Git {
                    status_timeout_ms,
                    backend,
                } => self.add_module(Git::<T>::with_config(
                    Duration::from_millis(*status_timeout_ms),
                    *backend,
                )),
                LineSegment::Pr { status } => self.add_module(Pr::<T>::new(*status)),
                LineSegment::Separator(style) => self.set_separator(style.into()),
                LineSegment::ReadOnly => self.add_module(ReadOnly::<T>::new()),
                LineSegment::Host | LineSegment::Hostname => self.add_module(Hostname::<T>::new()),
                LineSegment::Jobs => self.add_module(Jobs::<T>::new(runtime_data.job_count())),
                LineSegment::LocalIp => self.add_module(LocalIp::<T>::new()),
                LineSegment::Os => self.add_module(Os::<T>::new()),
                LineSegment::MemoryUsage { threshold } => {
                    self.add_module(MemoryUsage::<T>::new(*threshold))
                }
                LineSegment::Sudo => self.add_module(Sudo::<T>::new()),
                LineSegment::Kubernetes => self.add_module(Kubernetes::<T>::new()),
                LineSegment::Shell => {
                    self.add_module(ShellName::<T>::new(runtime_data.shell_name()))
                }
                LineSegment::Text(text) => self.add_module(Text::<T>::new(text.clone())),
                LineSegment::User | LineSegment::Username => self.add_module(Username::<T>::new()),
                LineSegment::Padding(size) => self.add_padding(*size),
                LineSegment::Time { format } => match format {
                    Some(format) => self.add_module(Time::<T>::with_time_format(format.clone())),
                    None => self.add_module(Time::<T>::new()),
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
                } => self.add_module(Usage::<T>::new(
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
                LineSegment::LastCmdDuration { min_run_time } => {
                    self.add_module(LastCmdDuration::<T>::new(
                        runtime_data.last_command_duration(),
                        Duration::from_millis(*min_run_time),
                    ))
                }
                LineSegment::Cwd {
                    max_length,
                    wanted_seg_num,
                    resolve_symlinks,
                } => self.add_module(Cwd::<T>::new(
                    *max_length,
                    *wanted_seg_num,
                    *resolve_symlinks,
                )),
                LineSegment::Node { version } => self.add_module(Node::<T>::new(*version)),
                LineSegment::Java { version, jdk } => {
                    self.add_module(Java::<T>::new(*version, *jdk))
                }
                LineSegment::Error { message } => {
                    self.add_module(ErrorMessage::<T>::new(message.clone()))
                }
                LineSegment::Unknown { name } => self.add_module(Unknown::<T>::new(name.clone())),
            };
        }
        self.widget_padding = None;
    }

    pub fn add_padding(&mut self, len: usize) {
        let padding = vec![" "; len].join("");
        match self.direction {
            Direction::Left => {
                // close out the buffer, write the padding, and leave the next write_segment
                // to handle adding the alternate separator
                self.close_left_buffer();
                self.left_columns += len + self.separator.width();
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
                    self.right_columns += self.separator.width();
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
            self.left_columns += self.separator.width();
        }
        self.last_style = None;
    }
}

/// The `modules` key a widget is themed under, for the properties every
/// module takes. `None` for layout entries that draw no segment.
fn theme_module(segment: &LineSegment) -> Option<&'static str> {
    Some(match segment {
        LineSegment::Battery => "battery",
        LineSegment::SmallSpacer | LineSegment::LargeSpacer => "spacer",
        LineSegment::Separator(_) | LineSegment::Padding(_) => return None,
        LineSegment::Cwd { .. } => "cwd",
        LineSegment::ReadOnly => "readonly",
        LineSegment::Git { .. } => "git",
        LineSegment::Pr { .. } => "pr",
        LineSegment::Python { .. } => "python",
        LineSegment::Node { .. } => "node",
        LineSegment::Java { .. } => "java",
        LineSegment::Cargo { .. } => "cargo",
        LineSegment::Kubernetes => "kubernetes",
        LineSegment::Host | LineSegment::Hostname => "hostname",
        LineSegment::Jobs => "jobs",
        LineSegment::LocalIp => "local_ip",
        LineSegment::MemoryUsage { .. } => "memory_usage",
        LineSegment::Os => "os",
        LineSegment::Sudo => "sudo",
        LineSegment::Shell => "shell",
        LineSegment::Time { .. } => "time",
        LineSegment::Text(_) => "text",
        LineSegment::AiUsage { .. } => "ai_usage",
        LineSegment::User | LineSegment::Username => "username",
        LineSegment::Cmd => "cmd",
        LineSegment::LastCmdDuration { .. } => "last_cmd_duration",
        LineSegment::Error { .. } => "error",
        LineSegment::Unknown { .. } => "unknown",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colors::Color;

    #[test]
    fn none_separator_has_no_glyph() {
        assert_eq!(Separator::None.for_direction(Direction::Left), "");
        assert_eq!(Separator::None.for_direction(Direction::Right), "");
        assert_eq!(Separator::None.width(), 0);
    }

    #[test]
    fn other_separators_are_a_single_column_wide() {
        for sep in [Separator::Chevron, Separator::Round] {
            assert_eq!(sep.width(), 1);
        }
    }

    #[test]
    fn right_segment_restores_its_foreground_after_the_separator() {
        let _ = SHELL.set(Shell::Bare);
        let mut powerline = Powerline::new();
        powerline.start_right();
        let dark = Color::from_u8(235);
        let orange = Color::from_u8(208);
        powerline.add_segment("user", Style::simple(Color::from_u8(223), dark));
        // The second segment's text color equals the first segment's
        // background, which used to be mistaken for an already-active color.
        powerline.add_segment("cargo", Style::simple(dark, orange));

        let buffer = &powerline.right_buffer;
        let before_cargo = &buffer[..buffer.rfind(" cargo ").unwrap()];
        let separator = powerline.separator.for_direction(Direction::Left);
        let after_separator = &before_cargo[before_cargo.rfind(separator).unwrap()..];
        assert!(
            after_separator.contains(&FgColor::from(dark).to_string()),
            "cargo segment must set its foreground after the separator; got {after_separator:?}"
        );
    }

    #[test]
    fn none_separator_does_not_widen_the_left_prompt() {
        let _ = SHELL.set(Shell::Bare);
        let mut powerline = Powerline::new();
        powerline.set_separator(Separator::None);
        let style = Style::simple(Color::from_u8(15), Color::from_u8(0));
        powerline.add_segment("one", style.clone());
        powerline.add_segment("two", style);

        // " one " (5) + " two " (5), no separator glyph counted between them
        assert_eq!(powerline.left_columns, 10);
    }

    #[test]
    fn wide_characters_count_two_columns() {
        let _ = SHELL.set(Shell::Bare);
        let mut powerline = Powerline::new();
        powerline.set_separator(Separator::None);
        let style = Style::simple(Color::from_u8(15), Color::from_u8(0));
        powerline.add_segment("データ", style.clone());
        powerline.start_right();
        powerline.add_segment("日本語", style);

        // " データ " and " 日本語 ": three double-width characters plus padding
        assert_eq!(powerline.left_columns, 8);
        assert_eq!(powerline.right_columns, 8);
    }

    fn flush_powerline() -> Powerline {
        let _ = SHELL.set(Shell::Bare);
        let mut powerline = Powerline::new();
        powerline.set_separator(Separator::None);
        powerline
    }

    /// The text a terminal shows for `buffer`, without its colour and link
    /// escapes.
    fn visible(buffer: &str) -> String {
        let mut out = String::new();
        let mut chars = buffer.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, chars.peek()) {
                ('\x1b', Some('[')) => {
                    chars.find(|c| c.is_ascii_alphabetic());
                }
                ('\x1b', Some(']')) => {
                    while let Some(c) = chars.next() {
                        if c == '\x1b' && chars.next() == Some('\\') {
                            break;
                        }
                    }
                }
                _ => out.push(c),
            }
        }
        out
    }

    /// Each padding with the spaces it draws before and after the text.
    const PADDINGS: [(SegmentPadding, &str, &str); 4] = [
        (SegmentPadding::Small, "", ""),
        (SegmentPadding::Large, " ", " "),
        (SegmentPadding::Left, " ", ""),
        (SegmentPadding::Right, "", " "),
    ];

    #[test]
    fn widget_padding_replaces_every_segment_default_on_both_sides() {
        let style = Style::simple(Color::from_u8(15), Color::from_u8(0));
        for (padding, before, after) in PADDINGS {
            let mut powerline = flush_powerline();
            powerline.widget_padding = Some(padding);
            for side in 0..2 {
                if side == 1 {
                    powerline.start_right();
                }
                powerline.add_segment("one", style.clone());
                powerline.add_short_segment("two", style.clone());
                powerline.add_padded_segment("six", style.clone(), SegmentPadding::Left);
                powerline.add_padded_segment("ten", style.clone(), SegmentPadding::Right);
            }

            let expected: String = ["one", "two", "six", "ten"]
                .map(|text| format!("{before}{text}{after}"))
                .concat();
            let columns = 4 * (3 + before.len() + after.len());
            assert_eq!(visible(&powerline.left_buffer), expected, "{padding:?}");
            assert_eq!(visible(&powerline.right_buffer), expected, "{padding:?}");
            assert_eq!(powerline.left_columns, columns, "{padding:?}");
            assert_eq!(powerline.right_columns, columns, "{padding:?}");
        }
    }

    #[test]
    fn padded_segment_matches_text_padded_by_hand() {
        let style = Style::simple(Color::from_u8(15), Color::from_u8(0));
        let mut padded = flush_powerline();
        let mut by_hand = flush_powerline();
        for side in 0..2 {
            if side == 1 {
                padded.start_right();
                by_hand.start_right();
            }
            for (padding, before, after) in PADDINGS {
                padded.add_padded_segment("env", style.clone(), padding);
                by_hand.add_short_segment(format!("{before}env{after}"), style.clone());
            }
        }

        assert_eq!(padded.left_buffer, by_hand.left_buffer);
        assert_eq!(padded.right_buffer, by_hand.right_buffer);
        assert_eq!(visible(&padded.left_buffer), "env env  envenv ");
        // "env" four times, plus a space for small, two for large, and one
        // each for left and right.
        for columns in [
            padded.left_columns,
            padded.right_columns,
            by_hand.left_columns,
            by_hand.right_columns,
        ] {
            assert_eq!(columns, 4 * 3 + 4);
        }
    }

    #[test]
    fn default_segment_padding_is_unchanged() {
        let style = Style::simple(Color::from_u8(15), Color::from_u8(0));
        let mut powerline = flush_powerline();
        powerline.add_segment("one", style.clone());
        powerline.add_short_segment("two", style.clone());
        powerline.add_hyperlink_segment("#12", "https://example.com/pr/12", style, None);
        assert_eq!(visible(&powerline.left_buffer), " one two #12 ");
        assert_eq!(powerline.left_columns, 5 + 3 + 5);
    }

    #[test]
    fn hyperlink_width_includes_the_widget_padding() {
        let style = Style::simple(Color::from_u8(15), Color::from_u8(0));
        let marker = Some(("●", Color::from_u8(2)));
        let cases = PADDINGS
            .map(|(padding, before, after)| (Some(padding), before, after))
            .into_iter()
            .chain([(None, " ", " ")]);
        for (padding, before, after) in cases {
            let mut powerline = flush_powerline();
            powerline.widget_padding = padding;
            powerline.add_hyperlink_segment(
                "#12",
                "https://example.com/pr/12",
                style.clone(),
                marker,
            );
            powerline.start_right();
            powerline.add_hyperlink_segment(
                "#12",
                "https://example.com/pr/12",
                style.clone(),
                marker,
            );

            // "#12" plus a space and the marker glyph is five cells.
            let expected = format!("{before}#12 ●{after}");
            let width = 5 + before.len() + after.len();
            assert_eq!(visible(&powerline.left_buffer), expected, "{padding:?}");
            assert_eq!(visible(&powerline.right_buffer), expected, "{padding:?}");
            assert_eq!(powerline.left_columns, width, "{padding:?}");
            assert_eq!(powerline.right_columns, width, "{padding:?}");
        }
    }
}
