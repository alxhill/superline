//! Widgets that show the session data Claude Code passes to its status line.
//! They draw nothing outside `superline claude-code`.

use std::marker::PhantomData;

use crate::claude_code::ClaudeCodeStatus;
use crate::colors::Color;
use crate::config::{SegmentPadding, UsageDisplay};
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::usage::format_window;
use super::{DefaultPadding, Module};

pub trait ClaudeCodeScheme: DefaultColors {
    const CLAUDE_MODEL_ICON: &'static str = "\u{ec82}"; // nf-cod-claude
    const CLAUDE_FAST_ICON: &'static str = "\u{f140b}"; // nf-md-lightning_bolt
    const CLAUDE_CONTEXT_ICON: &'static str = "\u{f029a}"; // nf-md-gauge
    const CLAUDE_DURATION_ICON: &'static str = "\u{f051b}"; // nf-md-timer_outline
    const CLAUDE_CACHE_ICON: &'static str = "\u{f00e8}"; // nf-md-cached
    const CLAUDE_VIM_ICON: &'static str = "\u{e62b}"; // nf-custom-vim
    const CLAUDE_AGENT_ICON: &'static str = "\u{ec67}"; // nf-cod-agent
    const CLAUDE_SESSION_ICON: &'static str = "\u{ec4f}"; // nf-cod-chat_sparkle

    fn claude_model_fg() -> Color {
        Self::default_fg()
    }
    fn claude_model_bg() -> Color {
        Self::default_bg()
    }
    fn claude_model_effort_fg() -> Color {
        Self::claude_model_fg()
    }
    fn claude_model_icon() -> &'static str {
        Self::CLAUDE_MODEL_ICON
    }
    fn claude_fast_icon() -> &'static str {
        Self::CLAUDE_FAST_ICON
    }

    fn claude_context_fg() -> Color {
        Self::default_fg()
    }
    fn claude_context_bg() -> Color {
        Self::default_bg()
    }
    fn claude_context_threshold_bg() -> Color {
        Self::alert_bg()
    }
    fn claude_context_icon() -> &'static str {
        Self::CLAUDE_CONTEXT_ICON
    }

    fn claude_cost_fg() -> Color {
        Self::default_fg()
    }
    fn claude_cost_bg() -> Color {
        Self::default_bg()
    }

    fn claude_duration_fg() -> Color {
        Self::default_fg()
    }
    fn claude_duration_bg() -> Color {
        Self::default_bg()
    }
    fn claude_duration_icon() -> &'static str {
        Self::CLAUDE_DURATION_ICON
    }

    fn claude_lines_added_fg() -> Color {
        Self::default_fg()
    }
    fn claude_lines_removed_fg() -> Color {
        Self::default_fg()
    }
    fn claude_lines_bg() -> Color {
        Self::default_bg()
    }

    fn claude_cache_fg() -> Color {
        Self::default_fg()
    }
    fn claude_cache_warm_bg() -> Color {
        Self::default_bg()
    }
    fn claude_cache_cold_bg() -> Color {
        Self::claude_cache_warm_bg()
    }
    fn claude_cache_icon() -> &'static str {
        Self::CLAUDE_CACHE_ICON
    }

    fn claude_vim_fg() -> Color {
        Self::default_fg()
    }
    fn claude_vim_normal_bg() -> Color {
        Self::default_bg()
    }
    fn claude_vim_insert_bg() -> Color {
        Self::claude_vim_normal_bg()
    }
    fn claude_vim_visual_bg() -> Color {
        Self::claude_vim_normal_bg()
    }
    fn claude_vim_icon() -> &'static str {
        Self::CLAUDE_VIM_ICON
    }

    fn claude_agent_fg() -> Color {
        Self::default_fg()
    }
    fn claude_agent_bg() -> Color {
        Self::default_bg()
    }
    fn claude_agent_icon() -> &'static str {
        Self::CLAUDE_AGENT_ICON
    }

    fn claude_session_fg() -> Color {
        Self::default_fg()
    }
    fn claude_session_bg() -> Color {
        Self::default_bg()
    }
    fn claude_session_icon() -> &'static str {
        Self::CLAUDE_SESSION_ICON
    }
}

/// An icon followed by a space, or nothing when a theme hides the icon.
fn with_icon(icon: &str, text: &str) -> String {
    if icon.is_empty() {
        text.to_string()
    } else {
        format!("{icon} {text}")
    }
}

/// Text that came from Claude Code, with control characters dropped so a
/// session or agent name cannot emit terminal escapes.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

pub struct ClaudeModel<'a, S> {
    status: Option<&'a ClaudeCodeStatus>,
    effort: bool,
    fast_mode: bool,
    scheme: PhantomData<S>,
}

impl<'a, S: ClaudeCodeScheme> ClaudeModel<'a, S> {
    pub fn new(status: Option<&'a ClaudeCodeStatus>, effort: bool, fast_mode: bool) -> Self {
        ClaudeModel {
            status,
            effort,
            fast_mode,
            scheme: PhantomData,
        }
    }
}

impl<S: ClaudeCodeScheme> Module for ClaudeModel<'_, S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(status) = self.status else {
            return;
        };
        let Some(name) = status
            .model
            .display_name
            .as_deref()
            .or(status.model.id.as_deref())
        else {
            return;
        };
        let fg = S::claude_model_fg();
        let mut label = with_icon(S::claude_model_icon(), &clean(name));
        if self.fast_mode && status.fast_mode && !S::claude_fast_icon().is_empty() {
            label.push(' ');
            label.push_str(S::claude_fast_icon());
        }
        let style = Style::simple(fg, S::claude_model_bg());
        let effort = status.effort.as_ref().and_then(|e| e.level.as_deref());
        match effort.filter(|_| self.effort) {
            Some(level) => powerline.add_two_tone_segment(
                &label,
                &clean(level),
                S::claude_model_effort_fg(),
                style,
            ),
            None => powerline.add_segment(label, style),
        }
    }
}

pub struct ClaudeContext<'a, S> {
    status: Option<&'a ClaudeCodeStatus>,
    display: UsageDisplay,
    tokens: bool,
    threshold: Option<f64>,
    scheme: PhantomData<S>,
}

impl<'a, S: ClaudeCodeScheme> ClaudeContext<'a, S> {
    pub fn new(
        status: Option<&'a ClaudeCodeStatus>,
        display: UsageDisplay,
        tokens: bool,
        threshold: Option<f64>,
    ) -> Self {
        ClaudeContext {
            status,
            display,
            tokens,
            threshold,
            scheme: PhantomData,
        }
    }
}

impl<S: ClaudeCodeScheme> Module for ClaudeContext<'_, S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(window) = self.status.and_then(|s| s.context_window.as_ref()) else {
            return;
        };
        let percent = window.used_percent();
        let mut label = with_icon(
            S::claude_context_icon(),
            &format_window("", percent, self.display),
        );
        if self.tokens {
            if let (Some(used), Some(size)) =
                (window.total_input_tokens, window.context_window_size)
            {
                label.push_str(&format!(" {}/{}", format_tokens(used), format_tokens(size)));
            }
        }
        let over = percent.is_some_and(|p| self.threshold.is_some_and(|t| p >= t));
        let bg = if over {
            S::claude_context_threshold_bg()
        } else {
            S::claude_context_bg()
        };
        powerline.add_segment(label, Style::simple(S::claude_context_fg(), bg));
    }
}

/// A token count in thousands or millions, e.g. `92k` or `1M`.
fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        let millions = tokens as f64 / 1_000_000.0;
        if millions.fract() < 0.05 {
            format!("{millions:.0}M")
        } else {
            format!("{millions:.1}M")
        }
    } else if tokens >= 1_000 {
        format!("{}k", (tokens as f64 / 1_000.0).round() as u64)
    } else {
        tokens.to_string()
    }
}

pub struct ClaudeCost<'a, S> {
    status: Option<&'a ClaudeCodeStatus>,
    scheme: PhantomData<S>,
}

impl<'a, S: ClaudeCodeScheme> ClaudeCost<'a, S> {
    pub fn new(status: Option<&'a ClaudeCodeStatus>) -> Self {
        ClaudeCost {
            status,
            scheme: PhantomData,
        }
    }
}

impl<S: ClaudeCodeScheme> Module for ClaudeCost<'_, S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(cost) = self
            .status
            .and_then(|s| s.cost.total_cost_usd)
            .filter(|cost| cost.is_finite())
        else {
            return;
        };
        powerline.add_segment(
            format_cost(cost),
            Style::simple(S::claude_cost_fg(), S::claude_cost_bg()),
        );
    }
}

fn format_cost(cost: f64) -> String {
    if cost >= 100.0 {
        format!("${cost:.0}")
    } else {
        format!("${cost:.2}")
    }
}

pub struct ClaudeDuration<'a, S> {
    status: Option<&'a ClaudeCodeStatus>,
    api: bool,
    scheme: PhantomData<S>,
}

impl<'a, S: ClaudeCodeScheme> ClaudeDuration<'a, S> {
    pub fn new(status: Option<&'a ClaudeCodeStatus>, api: bool) -> Self {
        ClaudeDuration {
            status,
            api,
            scheme: PhantomData,
        }
    }
}

impl<S: ClaudeCodeScheme> Module for ClaudeDuration<'_, S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(cost) = self.status.map(|s| &s.cost) else {
            return;
        };
        let millis = if self.api {
            cost.total_api_duration_ms
        } else {
            cost.total_duration_ms
        };
        let Some(millis) = millis else {
            return;
        };
        powerline.add_segment(
            with_icon(S::claude_duration_icon(), &format_duration(millis / 1000)),
            Style::simple(S::claude_duration_fg(), S::claude_duration_bg()),
        );
    }
}

/// The two largest units of a duration, e.g. `1h 5m` or `42s`.
fn format_duration(seconds: u64) -> String {
    let (days, hours) = (seconds / 86_400, seconds / 3_600 % 24);
    let (minutes, seconds) = (seconds / 60 % 60, seconds % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

pub struct ClaudeLines<'a, S> {
    status: Option<&'a ClaudeCodeStatus>,
    scheme: PhantomData<S>,
}

impl<'a, S: ClaudeCodeScheme> ClaudeLines<'a, S> {
    pub fn new(status: Option<&'a ClaudeCodeStatus>) -> Self {
        ClaudeLines {
            status,
            scheme: PhantomData,
        }
    }
}

impl<S: ClaudeCodeScheme> Module for ClaudeLines<'_, S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(cost) = self.status.map(|s| &s.cost) else {
            return;
        };
        let added = cost.total_lines_added.unwrap_or(0);
        let removed = cost.total_lines_removed.unwrap_or(0);
        if added == 0 && removed == 0 {
            return;
        }
        powerline.add_two_tone_segment(
            &format!("+{added}"),
            &format!("-{removed}"),
            S::claude_lines_removed_fg(),
            Style::simple(S::claude_lines_added_fg(), S::claude_lines_bg()),
        );
    }
}

pub struct ClaudeCache<'a, S> {
    status: Option<&'a ClaudeCodeStatus>,
    scheme: PhantomData<S>,
}

impl<'a, S: ClaudeCodeScheme> ClaudeCache<'a, S> {
    pub fn new(status: Option<&'a ClaudeCodeStatus>) -> Self {
        ClaudeCache {
            status,
            scheme: PhantomData,
        }
    }
}

impl<S: ClaudeCodeScheme> Module for ClaudeCache<'_, S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(cache) = self
            .status
            .and_then(|s| s.prompt_cache.as_ref())
            .filter(|cache| cache.caching_observed)
        else {
            return;
        };
        let ratio = cache
            .hit_ratio
            .filter(|ratio| ratio.is_finite())
            .map(|ratio| format!("{:.0}%", (ratio * 100.0).clamp(0.0, 100.0)))
            .unwrap_or_else(|| "–".to_string());
        let bg = if cache.warm {
            S::claude_cache_warm_bg()
        } else {
            S::claude_cache_cold_bg()
        };
        powerline.add_segment(
            with_icon(S::claude_cache_icon(), &ratio),
            Style::simple(S::claude_cache_fg(), bg),
        );
    }
}

pub struct ClaudeVim<'a, S> {
    status: Option<&'a ClaudeCodeStatus>,
    scheme: PhantomData<S>,
}

impl<'a, S: ClaudeCodeScheme> ClaudeVim<'a, S> {
    pub fn new(status: Option<&'a ClaudeCodeStatus>) -> Self {
        ClaudeVim {
            status,
            scheme: PhantomData,
        }
    }
}

impl<S: ClaudeCodeScheme> Module for ClaudeVim<'_, S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(mode) = self
            .status
            .and_then(|s| s.vim.as_ref())
            .and_then(|vim| vim.mode.as_deref())
        else {
            return;
        };
        let bg = match mode {
            "INSERT" => S::claude_vim_insert_bg(),
            mode if mode.starts_with("VISUAL") => S::claude_vim_visual_bg(),
            _ => S::claude_vim_normal_bg(),
        };
        powerline.add_segment(
            with_icon(S::claude_vim_icon(), &clean(mode)),
            Style::simple(S::claude_vim_fg(), bg),
        );
    }
}

pub struct ClaudeAgent<'a, S> {
    status: Option<&'a ClaudeCodeStatus>,
    scheme: PhantomData<S>,
}

impl<'a, S: ClaudeCodeScheme> ClaudeAgent<'a, S> {
    pub fn new(status: Option<&'a ClaudeCodeStatus>) -> Self {
        ClaudeAgent {
            status,
            scheme: PhantomData,
        }
    }
}

impl<S: ClaudeCodeScheme> Module for ClaudeAgent<'_, S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(name) = self
            .status
            .and_then(|s| s.agent.as_ref())
            .and_then(|agent| agent.name.as_deref())
            .filter(|name| !name.is_empty())
        else {
            return;
        };
        powerline.add_segment(
            with_icon(S::claude_agent_icon(), &clean(name)),
            Style::simple(S::claude_agent_fg(), S::claude_agent_bg()),
        );
    }
}

pub struct ClaudeSession<'a, S> {
    status: Option<&'a ClaudeCodeStatus>,
    max_length: usize,
    scheme: PhantomData<S>,
}

impl<'a, S: ClaudeCodeScheme> ClaudeSession<'a, S> {
    pub fn new(status: Option<&'a ClaudeCodeStatus>, max_length: usize) -> Self {
        ClaudeSession {
            status,
            max_length,
            scheme: PhantomData,
        }
    }
}

impl<S: ClaudeCodeScheme> Module for ClaudeSession<'_, S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(name) = self
            .status
            .and_then(|s| s.session_name.as_deref())
            .filter(|name| !name.is_empty())
        else {
            return;
        };
        powerline.add_segment(
            with_icon(
                S::claude_session_icon(),
                &truncate(&clean(name), self.max_length),
            ),
            Style::simple(S::claude_session_fg(), S::claude_session_bg()),
        );
    }
}

/// At most `max` characters, the last one an ellipsis when it was cut. `0`
/// leaves the text alone.
fn truncate(text: &str, max: usize) -> String {
    if max == 0 || text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_read_in_thousands_and_millions() {
        assert_eq!(format_tokens(950), "950");
        assert_eq!(format_tokens(92_400), "92k");
        assert_eq!(format_tokens(200_000), "200k");
        assert_eq!(format_tokens(1_000_000), "1M");
        assert_eq!(format_tokens(1_250_000), "1.2M");
    }

    #[test]
    fn costs_keep_cents_until_they_get_large() {
        assert_eq!(format_cost(0.01234), "$0.01");
        assert_eq!(format_cost(3.4721), "$3.47");
        assert_eq!(format_cost(142.6), "$143");
    }

    #[test]
    fn durations_show_their_two_largest_units() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(2843), "47m 23s");
        assert_eq!(format_duration(3_900), "1h 5m");
        assert_eq!(format_duration(90_000), "1d 1h");
    }

    #[test]
    fn long_session_names_are_cut_with_an_ellipsis() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("exactly-ten", 11), "exactly-ten");
        assert_eq!(truncate("a-much-longer-name", 8), "a-much-…");
        assert_eq!(truncate("unlimited", 0), "unlimited");
    }

    #[test]
    fn names_from_claude_code_cannot_emit_escapes() {
        assert_eq!(clean("evil\x1b[31mname\n"), "evil[31mname");
    }
}
