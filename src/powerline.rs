use std::fmt;
use std::fmt::{Display, Write};
use std::time::Duration;

use crate::colors::Color;
use crate::config;
use crate::config::{LineSegment, SeparatorStyle, TerminalRuntimeMetadata};
use crate::modules::{
    Cargo, Cmd, Cwd, ErrorMessage, Git, Host, LastCmdDuration, Module, Nvm, Pr, PythonEnv,
    ReadOnly, SdkmanJava, ShellName, Spacer, Time, Unknown, User,
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
        let _ = match self.direction {
            Direction::Left => self.write_segment(seg, style, true, None),
            Direction::Right => self.write_segment_right(seg, style, true, None),
        };
    }

    pub fn add_short_segment<D: Display>(&mut self, seg: D, style: Style) {
        let _ = match self.direction {
            Direction::Left => self.write_segment(seg, style, false, None),
            Direction::Right => self.write_segment_right(seg, style, false, None),
        };
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
        let mut visible_width = label.chars().count();
        let link = format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url, label);
        let seg = match marker {
            Some((glyph, color)) => {
                // separating space + the glyph itself
                visible_width += 1 + glyph.chars().count();
                // Colour the glyph, then restore the segment's foreground so the
                // terminal state matches what the renderer records for it.
                format!("{} {}{}{}", link, FgColor::from(color), glyph, style.fg)
            }
            None => link,
        };
        let _ = match self.direction {
            Direction::Left => self.write_segment(seg, style, true, Some(visible_width)),
            Direction::Right => self.write_segment_right(seg, style, true, Some(visible_width)),
        };
    }

    pub fn start_right(&mut self) {
        assert_eq!(self.direction, Direction::Left);
        self.close_left_buffer();
        self.direction = Direction::Right;
    }

    pub fn add_module<M: Module>(&mut self, mut module: M) {
        module.append_segments(self);
    }

    fn add_conf_modules<T: CompleteTheme>(
        &mut self,
        modules: &Vec<LineSegment>,
        runtime_data: &impl TerminalRuntimeMetadata,
    ) {
        for module in modules {
            match module {
                LineSegment::SmallSpacer => self.add_module(Spacer::<T>::small()),
                LineSegment::LargeSpacer => self.add_module(Spacer::<T>::large()),
                LineSegment::PythonEnv => self.add_module(PythonEnv::<T>::new()),
                LineSegment::Cmd => {
                    self.add_module(Cmd::<T>::new(runtime_data.last_command_status()))
                }
                LineSegment::Cargo => self.add_module(Cargo::<T>::new()),
                LineSegment::Git { status_timeout_ms } => self.add_module(
                    Git::<T>::with_status_timeout(Duration::from_millis(*status_timeout_ms)),
                ),
                LineSegment::Pr { status } => self.add_module(Pr::<T>::new(*status)),
                LineSegment::Separator(style) => self.set_separator(style.into()),
                LineSegment::ReadOnly => self.add_module(ReadOnly::<T>::new()),
                LineSegment::Host => self.add_module(Host::<T>::new()),
                LineSegment::Shell => {
                    self.add_module(ShellName::<T>::new(runtime_data.shell_name()))
                }
                LineSegment::User => self.add_module(User::<T>::new()),
                LineSegment::Padding(size) => self.add_padding(*size),
                LineSegment::Time { format } => match format {
                    Some(format) => self.add_module(Time::<T>::with_time_format(format.clone())),
                    None => self.add_module(Time::<T>::new()),
                },
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
                LineSegment::Nvm => self.add_module(Nvm::<T>::new()),
                LineSegment::Sdkman => self.add_module(SdkmanJava::<T>::new()),
                LineSegment::Error { message } => {
                    self.add_module(ErrorMessage::<T>::new(message.clone()))
                }
                LineSegment::Unknown { name } => self.add_module(Unknown::<T>::new(name.clone())),
            };
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
