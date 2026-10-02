use std::fmt;
use std::fmt::{Display, Write};
use std::time::Duration;

use unicode_width::UnicodeWidthStr;

use crate::colors::Color;
use crate::config;
use crate::config::{LineSegment, SegmentPadding, SeparatorStyle, TerminalRuntimeMetadata, Widget};
use crate::debug;
use crate::modules::{
    Battery, Cargo, ClaudeAgent, ClaudeCache, ClaudeContext, ClaudeCost, ClaudeDuration,
    ClaudeLines, ClaudeModel, ClaudeSession, ClaudeVim, Cmd, Cwd, DefaultPadding, ErrorMessage,
    Git, Hostname, Java, Jobs, Kubernetes, LastCmdDuration, LocalIp, MemoryUsage, Module, Node, Os,
    Pr, Python, ReadOnly, ShellName, Spacer, Sudo, Text, Time, Unknown, Usage, UsageWindows,
    Username,
};
use crate::terminal::*;
use crate::themes::{CompleteTheme, DefaultColors};

#[derive(Clone)]
pub struct Style {
    pub fg: FgColor,
    pub bg: BgColor,
    pub sep_fg: FgColor,
}

impl Style {
    /// Text in `fg`, with its text attributes, on `bg`. The separator is drawn
    /// in `bg` without attributes.
    pub fn simple(fg: Color, bg: Color) -> Style {
        Style {
            fg: fg.into(),
            bg: bg.into(),
            sep_fg: BgColor::from(bg).transpose(),
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
    /// Whether any segment has been drawn.
    drawn: bool,
    /// The configured padding of the widget being drawn, which replaces the
    /// default each of its segments asks for.
    widget_padding: Option<SegmentPadding>,
    /// The padding the module being drawn declares for its segments.
    module_padding: SegmentPadding,
    /// Set by [`default_padding`], which asks a widget's module for its
    /// padding without drawing it.
    probe: Option<Option<DefaultPadding>>,
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
            drawn: false,
            widget_padding: None,
            module_padding: SegmentPadding::Large,
            probe: None,
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

        // Text attributes end with the segment, before any separator.
        write!(self.left_buffer, "{}", style.fg.attrs_off())?;

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

        // Text attributes end with the segment, before any separator.
        write!(self.right_buffer, "{}", style.fg.attrs_off())?;

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
        self.drawn = true;
        let _ = match self.direction {
            Direction::Left => self.write_segment(seg, style, padding, visible_width),
            Direction::Right => self.write_segment_right(seg, style, padding, visible_width),
        };
    }

    /// Adds a segment with the padding its module declares, unless the
    /// widget's padding is configured.
    pub fn add_segment<D: Display>(&mut self, seg: D, style: Style) {
        self.push_segment(seg, style, self.module_padding, None);
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
                format!("{} {}", link, tinted(glyph, color, &style))
            }
            None => link,
        };
        self.push_segment(seg, style, self.module_padding, Some(visible_width));
    }

    /// Adds a segment of `text` followed by `suffix` in its own colour, both
    /// on this segment's background, e.g. the model name and its effort level.
    pub fn add_two_tone_segment(
        &mut self,
        text: &str,
        suffix: &str,
        suffix_fg: Color,
        style: Style,
    ) {
        let visible_width = text.width() + 1 + suffix.width();
        let seg = format!("{} {}", text, tinted(suffix, suffix_fg, &style));
        self.push_segment(seg, style, self.module_padding, Some(visible_width));
    }

    /// Whether the row has any segments, as opposed to only layout entries
    /// or widgets with nothing to show.
    pub fn has_segments(&self) -> bool {
        self.drawn
    }

    /// Adds a segment made of `pieces`, each with an optional note that
    /// iTerm2 shows when hovering over it. The annotation escapes and their
    /// notes are invisible, so the visible width is that of the text alone.
    pub fn add_annotated_segment<'a>(
        &mut self,
        pieces: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
        style: Style,
    ) {
        let mut seg = String::new();
        let mut visible_width = 0;
        for (text, note) in pieces {
            let cells = text.width();
            if let Some(message) = note.filter(|_| cells > 0) {
                let _ = write!(seg, "{}", Annotation { cells, message });
            }
            seg.push_str(text);
            visible_width += cells;
        }
        self.push_segment(seg, style, self.module_padding, Some(visible_width));
    }

    pub fn start_right(&mut self) {
        assert_eq!(self.direction, Direction::Left);
        self.close_left_buffer();
        self.direction = Direction::Right;
    }

    pub fn add_module<M: Module>(&mut self, mut module: M) {
        if let Some(probe) = &mut self.probe {
            *probe = Some(module.default_padding());
            return;
        }
        let span = debug::span(debug::type_label(std::any::type_name::<M>()));
        let outer = std::mem::replace(&mut self.module_padding, module.default_padding().padding);
        module.append_segments(self);
        self.module_padding = outer;
        span.finish();
    }

    fn add_conf_modules<T: CompleteTheme>(
        &mut self,
        widgets: &[Widget],
        runtime_data: &impl TerminalRuntimeMetadata,
    ) {
        let claude = runtime_data.claude_code();
        for widget in widgets {
            self.widget_padding = widget.padding;
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
                    worktrees,
                    remote_link,
                } => self.add_module(Git::<T>::with_config(
                    Duration::from_millis(*status_timeout_ms),
                    *backend,
                    *worktrees,
                    *remote_link,
                )),
                LineSegment::Pr { status } => self.add_module(
                    Pr::<T>::new(*status).with_claude_code_pr(claude.and_then(|s| s.pr.as_ref())),
                ),
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
                    hover,
                    width,
                } => self.add_module(
                    Usage::<T>::new(
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
                        *hover && runtime_data.hover_text(),
                    )
                    .with_claude_code_limits(claude.and_then(|s| s.rate_limits.as_ref()))
                    .with_width(*width),
                ),
                LineSegment::ClaudeModel { effort, fast_mode } => {
                    self.add_module(ClaudeModel::<T>::new(claude, *effort, *fast_mode))
                }
                LineSegment::ClaudeContext {
                    display,
                    tokens,
                    threshold,
                    width,
                } => self.add_module(ClaudeContext::<T>::new(
                    claude, *display, *width, *tokens, *threshold,
                )),
                LineSegment::ClaudeCost => self.add_module(ClaudeCost::<T>::new(claude)),
                LineSegment::ClaudeDuration { api } => {
                    self.add_module(ClaudeDuration::<T>::new(claude, *api))
                }
                LineSegment::ClaudeLines => self.add_module(ClaudeLines::<T>::new(claude)),
                LineSegment::ClaudeCache => self.add_module(ClaudeCache::<T>::new(claude)),
                LineSegment::ClaudeVim => self.add_module(ClaudeVim::<T>::new(claude)),
                LineSegment::ClaudeAgent => self.add_module(ClaudeAgent::<T>::new(claude)),
                LineSegment::ClaudeSession { max_length } => {
                    self.add_module(ClaudeSession::<T>::new(claude, *max_length))
                }
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

/// `text` in its own colour and attributes inside a segment drawn in `style`,
/// which is restored after it so the terminal state matches what the renderer
/// records for the segment.
fn tinted(text: &str, color: Color, style: &Style) -> String {
    let fg = FgColor::from(color);
    format!(
        "{}{}{}{}{}",
        style.fg.attrs_off(),
        fg,
        text,
        fg.attrs_off(),
        style.fg
    )
}

/// The padding a widget's module declares, as it would draw the widget. `None`
/// for layout entries that draw no segment.
pub fn default_padding(widget: &Widget) -> Option<DefaultPadding> {
    if matches!(
        widget.segment,
        LineSegment::Separator(_) | LineSegment::Padding(_)
    ) {
        return None;
    }
    let mut probe = Powerline::new();
    probe.probe = Some(None);
    probe.add_conf_modules::<ProbeTheme>(std::slice::from_ref(widget), &NoRuntimeData);
    probe.probe.flatten()
}

/// Every scheme's defaults, for modules that are built but not drawn.
struct ProbeTheme;

impl DefaultColors for ProbeTheme {
    fn default_bg() -> Color {
        Color::from_u8(0)
    }

    fn default_fg() -> Color {
        Color::from_u8(15)
    }
}

macro_rules! probe_schemes {
    ($($scheme:path),* $(,)?) => {
        $(impl $scheme for ProbeTheme {})*
    };
}

probe_schemes!(
    crate::modules::BatteryScheme,
    crate::modules::CargoScheme,
    crate::modules::ClaudeCodeScheme,
    crate::modules::CmdScheme,
    crate::modules::ErrorMessageScheme,
    crate::modules::ExitCodeScheme,
    crate::modules::GitScheme,
    crate::modules::HostScheme,
    crate::modules::JavaScheme,
    crate::modules::JobsScheme,
    crate::modules::KubernetesScheme,
    crate::modules::LastCmdDurationScheme,
    crate::modules::LocalIpScheme,
    crate::modules::MemoryUsageScheme,
    crate::modules::NodeScheme,
    crate::modules::OsScheme,
    crate::modules::PrScheme,
    crate::modules::PythonScheme,
    crate::modules::ReadOnlyScheme,
    crate::modules::ShellScheme,
    crate::modules::SpacerScheme,
    crate::modules::SudoScheme,
    crate::modules::TimeScheme,
    crate::modules::UnknownScheme,
    crate::modules::UsageScheme,
    crate::modules::UserScheme,
    crate::update::UpdateScheme,
);

impl crate::modules::CwdScheme for ProbeTheme {
    fn path_bg_colors() -> Vec<Color> {
        vec![Self::default_bg()]
    }
}

impl CompleteTheme for ProbeTheme {}

/// Stands in for the shell when a module is built but not drawn.
struct NoRuntimeData;

impl TerminalRuntimeMetadata for NoRuntimeData {
    fn shell_name(&self) -> String {
        String::new()
    }

    fn total_columns(&self) -> usize {
        0
    }

    fn last_command_duration(&self) -> Option<Duration> {
        None
    }

    fn last_command_status(&self) -> &str {
        "0"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colors::{Color, TextAttrs};

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

    /// The text a terminal shows for `buffer`, without its colour, link and
    /// annotation escapes.
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
                        if c == '\x07' || (c == '\x1b' && chars.next() == Some('\\')) {
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

    /// Draws a plain segment, one with its own padding, and a link.
    struct Declares(DefaultPadding);

    impl Module for Declares {
        fn default_padding(&self) -> DefaultPadding {
            self.0
        }

        fn append_segments(&mut self, powerline: &mut Powerline) {
            let style = Style::simple(Color::from_u8(15), Color::from_u8(0));
            powerline.add_segment("one", style.clone());
            powerline.add_padded_segment("two", style.clone(), SegmentPadding::Right);
            powerline.add_hyperlink_segment("#1", "https://example.com/1", style, None);
        }
    }

    #[test]
    fn segments_get_the_padding_their_module_declares() {
        let style = Style::simple(Color::from_u8(15), Color::from_u8(0));
        for (padding, before, after) in PADDINGS {
            for side in 0..2 {
                let mut powerline = flush_powerline();
                if side == 1 {
                    powerline.start_right();
                }
                powerline.add_module(Declares(padding.into()));
                // Outside a module, a segment is large again.
                powerline.add_segment("out", style.clone());

                let (buffer, columns) = if side == 0 {
                    (&powerline.left_buffer, powerline.left_columns)
                } else {
                    (&powerline.right_buffer, powerline.right_columns)
                };
                let expected = format!("{before}one{after}two {before}#1{after} out ");
                assert_eq!(visible(buffer), expected, "{padding:?}");
                assert_eq!(columns, expected.chars().count(), "{padding:?}");
            }
        }
    }

    #[test]
    fn widget_padding_wins_over_what_the_module_declares() {
        let mut powerline = flush_powerline();
        powerline.widget_padding = Some(SegmentPadding::Left);
        powerline.add_module(Declares(SegmentPadding::Small.into()));
        assert_eq!(visible(&powerline.left_buffer), " one two #1");
    }

    #[test]
    fn default_padding_asks_the_module_without_drawing_it() {
        let widget = |json: &str| serde_json::from_str::<Widget>(json).unwrap();
        for (json, padding) in [
            (r#"{"cwd":{"max_length":9,"wanted_seg_num":2}}"#, "left"),
            (r#"{"last_cmd_duration":{"min_run_time":5}}"#, "left"),
            (r#""cmd""#, "small"),
            (r#""shell""#, "small"),
            (r#""small_spacer""#, "small"),
            (r#""large_spacer""#, "large"),
            (r#"{"git":{"padding":"small"}}"#, "large"),
            (r#""pr""#, "large"),
            (r#"{"text":"hi"}"#, "large"),
            (r#""python""#, "large; venv label right"),
            (r#""future_widget""#, "large"),
        ] {
            let declared = default_padding(&widget(json)).map(|p| p.to_string());
            assert_eq!(declared.as_deref(), Some(padding), "{json}");
        }
        assert_eq!(default_padding(&widget(r#"{"padding":2}"#)), None);
        assert_eq!(default_padding(&widget(r#"{"separator":"round"}"#)), None);
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
    fn annotation_notes_take_no_columns() {
        let style = Style::simple(Color::from_u8(15), Color::from_u8(0));
        let pieces = [
            ("5h", None),
            ("▅", Some("5h: 61% used")),
            (" 7d", None),
            ("▃", Some("7d: 41% used")),
        ];
        for (padding, before, after) in PADDINGS {
            let mut powerline = flush_powerline();
            powerline.widget_padding = Some(padding);
            powerline.add_annotated_segment(pieces, style.clone());
            powerline.start_right();
            powerline.add_annotated_segment(pieces, style.clone());

            let expected = format!("{before}5h▅ 7d▃{after}");
            let width = 7 + before.len() + after.len();
            for (buffer, columns) in [
                (&powerline.left_buffer, powerline.left_columns),
                (&powerline.right_buffer, powerline.right_columns),
            ] {
                assert!(
                    buffer.contains("\x1b]1337;AddHiddenAnnotation=1|5h: 61% used\x07▅"),
                    "{buffer:?}"
                );
                assert_eq!(visible(buffer), expected, "{padding:?}");
                assert_eq!(columns, width, "{padding:?}");
            }
        }
    }

    #[test]
    fn annotated_segment_without_notes_matches_a_plain_one() {
        let style = Style::simple(Color::from_u8(15), Color::from_u8(0));
        let mut annotated = flush_powerline();
        let mut plain = flush_powerline();
        annotated.add_annotated_segment([("5h ", None), ("12%", None)], style.clone());
        plain.add_segment("5h 12%", style);
        assert_eq!(annotated.left_buffer, plain.left_buffer);
        assert_eq!(annotated.left_columns, plain.left_columns);
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

    const BOLD_UNDERLINE: TextAttrs = TextAttrs {
        bold: true,
        italic: false,
        underline: true,
    };

    const ITALIC: TextAttrs = TextAttrs {
        bold: false,
        italic: true,
        underline: false,
    };

    /// Each printed character with the attributes a terminal would draw it
    /// in, reading bare escapes, and the attributes still on at the end.
    fn drawn_attrs(buffer: &str) -> (Vec<(char, TextAttrs)>, TextAttrs) {
        let mut attrs = TextAttrs::NONE;
        let mut drawn = Vec::new();
        let mut chars = buffer.chars();
        while let Some(c) = chars.next() {
            if c != '\x1b' {
                drawn.push((c, attrs));
                continue;
            }
            match chars.next() {
                Some('[') => {
                    let params: String = chars.by_ref().take_while(|c| *c != 'm').collect();
                    let mut codes = params.split(';');
                    while let Some(code) = codes.next() {
                        match code {
                            "" | "0" => attrs = TextAttrs::NONE,
                            "1" => attrs.bold = true,
                            "3" => attrs.italic = true,
                            "4" => attrs.underline = true,
                            "22" => attrs.bold = false,
                            "23" => attrs.italic = false,
                            "24" => attrs.underline = false,
                            "38" | "48" => {
                                codes.next();
                                codes.next();
                            }
                            _ => {}
                        }
                    }
                }
                // OSC 8 hyperlinks run to ST (ESC \).
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\x1b' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        (drawn, attrs)
    }

    /// Only `styled` is drawn with `attrs`; every other visible character,
    /// separators included, is drawn without any, and nothing is left on.
    fn assert_attrs_stay_in(buffer: &str, styled: char, attrs: TextAttrs) {
        let (drawn, end) = drawn_attrs(buffer);
        assert!(drawn.iter().any(|(c, _)| *c == styled), "{buffer:?}");
        for (c, drawn_with) in drawn {
            match c {
                c if c == styled => assert_eq!(drawn_with, attrs, "{buffer:?}"),
                // A segment's own padding spaces share its attributes.
                ' ' => {}
                c => assert!(
                    drawn_with.is_empty(),
                    "{c:?} drawn with {drawn_with:?} in {buffer:?}"
                ),
            }
        }
        assert!(end.is_empty(), "attributes left on after {buffer:?}");
    }

    fn bold_style() -> Style {
        Style::simple(Color(15).with_attrs(BOLD_UNDERLINE), Color(31))
    }

    #[test]
    fn text_attributes_stay_inside_their_left_segment() {
        let _ = SHELL.set(Shell::Bare);
        let mut powerline = Powerline::new();
        let plain = Style::simple(Color(15), Color(31));
        powerline.add_segment("a", plain.clone());
        powerline.add_segment("B", bold_style());
        powerline.add_segment("B", bold_style());
        powerline.add_padding(2);
        powerline.add_segment("B", bold_style());
        powerline.add_short_segment("c", plain);
        powerline.add_segment("B", bold_style());
        powerline.close_left_buffer();

        assert_attrs_stay_in(&powerline.left_buffer, 'B', BOLD_UNDERLINE);
    }

    #[test]
    fn text_attributes_stay_inside_their_right_segment() {
        let _ = SHELL.set(Shell::Bare);
        let mut powerline = Powerline::new();
        powerline.start_right();
        powerline.add_segment("B", bold_style());
        powerline.add_segment("a", Style::simple(Color(15), Color(31)));
        powerline.add_segment("B", bold_style());
        powerline.add_padding(1);
        powerline.add_segment("B", bold_style());

        assert_attrs_stay_in(&powerline.right_buffer, 'B', BOLD_UNDERLINE);
    }

    #[test]
    fn a_marker_glyph_gets_its_own_attributes_and_restores_the_segments() {
        let _ = SHELL.set(Shell::Bare);
        let mut powerline = Powerline::new();
        powerline.add_hyperlink_segment(
            "BB",
            "https://example.com",
            bold_style(),
            Some(("m", Color(2).with_attrs(ITALIC))),
        );
        powerline.add_segment("a", Style::simple(Color(15), Color(31)));
        powerline.close_left_buffer();

        let (drawn, end) = drawn_attrs(&powerline.left_buffer);
        let text: String = drawn.iter().map(|(c, _)| c).collect();
        assert!(text.starts_with(" BB m "), "{text:?}");
        let attrs: Vec<TextAttrs> = drawn.iter().map(|(_, attrs)| *attrs).collect();
        assert_eq!(attrs[1..3], [BOLD_UNDERLINE, BOLD_UNDERLINE]);
        assert_eq!(attrs[4], ITALIC);
        // Back to the segment's attributes for its closing space.
        assert_eq!(attrs[5], BOLD_UNDERLINE);
        assert!(attrs[6..].iter().all(|attrs| attrs.is_empty()));
        assert!(end.is_empty());
    }

    #[test]
    fn text_attributes_do_not_change_the_column_count() {
        let _ = SHELL.set(Shell::Bare);
        let mut plain = Powerline::new();
        plain.add_segment("one", Style::simple(Color(15), Color(31)));
        plain.start_right();
        plain.add_segment("two", Style::simple(Color(15), Color(31)));

        let mut styled = Powerline::new();
        styled.add_segment("one", bold_style());
        styled.start_right();
        styled.add_segment("two", bold_style());

        assert_eq!(styled.left_columns, plain.left_columns);
        assert_eq!(styled.right_columns, plain.right_columns);
    }

    #[test]
    fn plain_styles_emit_no_attribute_escapes() {
        let _ = SHELL.set(Shell::Bare);
        let mut powerline = Powerline::new();
        let style = Style::simple(Color(15), Color(31));
        powerline.add_hyperlink_segment(
            "#1",
            "https://example.com",
            style.clone(),
            Some(("m", Color(2))),
        );
        powerline.add_segment("a", style.clone());
        powerline.close_left_buffer();
        powerline.start_right();
        powerline.add_segment("b", style);

        for buffer in [&powerline.left_buffer, &powerline.right_buffer] {
            for code in ["1", "3", "4", "22", "23", "24"] {
                assert!(!buffer.contains(&format!("\x1b[{code}m")), "{buffer:?}");
            }
        }
    }
}
