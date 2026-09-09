use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write as _};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, LocalResult, TimeZone};
use fs2::FileExt;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::colors::Color;
use crate::config::{UsageDisplay, UsageProvider};
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

const CACHE_TTL: Duration = Duration::from_secs(60);
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const BAR_WIDTH: usize = 5;
const SPARKLINE: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const SPARKLINE_EMPTY: char = ' ';
const MAX_CAPTURE_BYTES: usize = 256 * 1024;
const CODEX_APP_SERVER_TIMEOUT: Duration = Duration::from_secs(15);
// A stable, disposable Claude CLI probe session prevents creating a new local
// conversation on every refresh; its transcript is removed after each probe.
const CLAUDE_PROBE_SESSION_ID: &str = "b450f1cc-67ae-4f33-89fb-867a0d0fb522";
const OPENAI_ICON: &str = "\u{ec81}";
const CLAUDE_ICON: &str = "\u{ec82}";
// spaces added manually to allow for compact display
const DEFAULT_SESSION_LABEL: &str = "5h ";
const DEFAULT_WEEKLY_LABEL: &str = " 7d ";
const DEFAULT_FABLE_LABEL: &str = " F ";
const DEFAULT_CREDITS_LABEL: &str = " C ";

pub struct Usage<S> {
    provider: UsageProvider,
    windows: UsageWindows,
    display: UsageDisplay,
    threshold: Option<f64>,
    show_session_time_remaining: bool,
    session_time_remaining_only_at_limit: f64,
    scheme: PhantomData<S>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageWindow {
    pub enabled: bool,
    pub label: String,
}

impl UsageWindow {
    pub fn new(enabled: bool, label: Option<String>, default_label: &str) -> Self {
        Self {
            enabled,
            label: label.unwrap_or_else(|| default_label.to_string()),
        }
    }
}

/// The credits lane has its own display style and can hide itself until a
/// rate-limit window is exhausted, which is when providers start billing
/// against credits.
#[derive(Debug, Clone, PartialEq)]
pub struct CreditsWindow {
    pub window: UsageWindow,
    pub display: UsageDisplay,
    pub only_when_limited: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageWindows {
    pub session: UsageWindow,
    pub weekly: UsageWindow,
    pub fable: UsageWindow,
    pub credits: CreditsWindow,
}

impl UsageWindows {
    pub fn new(
        session: UsageWindow,
        weekly: UsageWindow,
        fable: UsageWindow,
        credits: CreditsWindow,
        provider: UsageProvider,
    ) -> Self {
        Self {
            session,
            weekly,
            // Only the Claude CLI reports a Fable-specific weekly window.
            fable: UsageWindow {
                enabled: fable.enabled && provider == UsageProvider::Claude,
                ..fable
            },
            credits,
        }
    }

    pub fn credits(
        enabled: bool,
        label: Option<String>,
        display: UsageDisplay,
        only_when_limited: bool,
    ) -> CreditsWindow {
        CreditsWindow {
            window: UsageWindow::new(enabled, label, DEFAULT_CREDITS_LABEL),
            display,
            only_when_limited,
        }
    }

    pub fn session(enabled: bool, label: Option<String>) -> UsageWindow {
        UsageWindow::new(enabled, label, DEFAULT_SESSION_LABEL)
    }

    pub fn weekly(enabled: bool, label: Option<String>) -> UsageWindow {
        UsageWindow::new(enabled, label, DEFAULT_WEEKLY_LABEL)
    }

    pub fn fable(enabled: bool, label: Option<String>) -> UsageWindow {
        UsageWindow::new(enabled, label, DEFAULT_FABLE_LABEL)
    }

    fn any_enabled(&self) -> bool {
        self.session.enabled
            || self.weekly.enabled
            || self.fable.enabled
            || self.credits.window.enabled
    }

    fn credits_visible(&self, cache: &UsageCache) -> bool {
        self.credits.window.enabled && (!self.credits.only_when_limited || cache.limit_reached())
    }
}

pub trait UsageScheme: DefaultColors {
    fn claude_usage_fg() -> Color {
        Self::default_fg()
    }
    fn claude_usage_bg() -> Color {
        Self::default_bg()
    }
    fn codex_usage_fg() -> Color {
        Self::default_fg()
    }
    fn codex_usage_bg() -> Color {
        Self::default_bg()
    }
    fn usage_threshold_bg() -> Color {
        Self::alert_bg()
    }
}

impl<S: UsageScheme> Usage<S> {
    pub fn new(
        provider: UsageProvider,
        windows: UsageWindows,
        display: UsageDisplay,
        threshold: Option<f64>,
        show_session_time_remaining: bool,
        session_time_remaining_only_at_limit: f64,
    ) -> Self {
        Self {
            provider,
            windows,
            display,
            threshold: threshold.filter(|threshold| threshold.is_finite()),
            show_session_time_remaining,
            session_time_remaining_only_at_limit,
            scheme: PhantomData,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct UsageCache {
    session: Option<f64>,
    weekly: Option<f64>,
    #[serde(default)]
    fable: Option<f64>,
    #[serde(default)]
    credits: Option<CreditsUsage>,
    #[serde(default)]
    session_resets_at: Option<u64>,
    fetched_at: u64,
}

impl UsageCache {
    /// Providers bill against credits once any rate-limit window is exhausted.
    fn limit_reached(&self) -> bool {
        [self.session, self.weekly, self.fable]
            .into_iter()
            .flatten()
            .any(|percent| percent >= 100.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CreditsUnit {
    Dollars,
    Count,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct CreditsUsage {
    used: f64,
    limit: f64,
    unit: CreditsUnit,
}

impl CreditsUsage {
    fn percent_used(&self) -> Option<f64> {
        (self.limit > 0.0).then(|| (self.used / self.limit * 100.0).clamp(0.0, 100.0))
    }

    fn numeric(&self) -> String {
        format!(
            "{}/{}",
            format_amount(self.used, self.unit),
            format_amount(self.limit, self.unit)
        )
    }
}

fn format_amount(amount: f64, unit: CreditsUnit) -> String {
    let amount = amount.max(0.0);
    match unit {
        CreditsUnit::Dollars if amount.fract() == 0.0 => format!("${amount:.0}"),
        CreditsUnit::Dollars => format!("${amount:.2}"),
        CreditsUnit::Count => format!("{amount:.0}"),
    }
}

impl<S: UsageScheme> Module for Usage<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        if !self.windows.any_enabled() && !self.show_session_time_remaining {
            return;
        }

        let Some(cache_path) = cache_path_for(self.provider) else {
            return;
        };
        let cache = read_cache(&cache_path);

        if cache
            .as_ref()
            .is_none_or(|cache| is_stale(cache.fetched_at))
        {
            spawn_refresh(self.provider, &cache_path);
        }

        let (default_fg, bg) = provider_style::<S>(self.provider);
        let label = cache
            .as_ref()
            .map(|cache| {
                format_usage(
                    self.provider,
                    cache,
                    &self.windows,
                    self.display,
                    self.show_session_time_remaining,
                    self.session_time_remaining_only_at_limit,
                )
            })
            .unwrap_or_else(|| format!("{} …", provider_label(self.provider)));
        let bg = cache
            .as_ref()
            .filter(|cache| threshold_reached(cache, &self.windows, self.threshold))
            .map(|_| S::usage_threshold_bg())
            .unwrap_or(bg);
        powerline.add_segment(label, Style::simple(default_fg, bg));
    }
}

fn provider_label(provider: UsageProvider) -> &'static str {
    match provider {
        UsageProvider::Claude => CLAUDE_ICON,
        UsageProvider::Codex => OPENAI_ICON,
    }
}

fn provider_style<S: UsageScheme>(provider: UsageProvider) -> (Color, Color) {
    match provider {
        UsageProvider::Claude => (S::claude_usage_fg(), S::claude_usage_bg()),
        UsageProvider::Codex => (S::codex_usage_fg(), S::codex_usage_bg()),
    }
}

fn format_usage(
    provider: UsageProvider,
    cache: &UsageCache,
    windows: &UsageWindows,
    display: UsageDisplay,
    show_session_time_remaining: bool,
    session_time_remaining_only_at_limit: f64,
) -> String {
    let mut parts = vec![provider_label(provider).to_string(), " ".to_string()];
    for (window, used_percent) in [
        (&windows.session, cache.session),
        (&windows.weekly, cache.weekly),
        (&windows.fable, cache.fable),
    ] {
        if window.enabled {
            parts.push(format_window(&window.label, used_percent, display));
        }
    }
    if windows.credits_visible(cache) {
        parts.push(format_credits(
            &windows.credits.window.label,
            cache.credits.as_ref(),
            windows.credits.display,
        ));
    }
    if show_session_time_remaining
        && cache.session_resets_at.is_some()
        && cache
            .session
            .is_some_and(|percent| percent >= session_time_remaining_only_at_limit * 100.0)
    {
        parts.push(format!(
            " ↻ {}",
            format_time_remaining(cache.session_resets_at)
        ));
    }
    parts.join("")
}

fn format_time_remaining(resets_at: Option<u64>) -> String {
    let Some(resets_at) = resets_at else {
        return "–".to_string();
    };
    format_remaining_duration(resets_at.saturating_sub(now_secs()))
}

fn format_remaining_duration(remaining: u64) -> String {
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "now".to_string()
    }
}

fn format_credits(label: &str, credits: Option<&CreditsUsage>, display: UsageDisplay) -> String {
    match (display, credits) {
        (UsageDisplay::Numeric, Some(credits)) => format!("{label}{}", credits.numeric()),
        _ => format_window(label, credits.and_then(CreditsUsage::percent_used), display),
    }
}

fn format_window(label: &str, used_percent: Option<f64>, display: UsageDisplay) -> String {
    let (prefix, value) = format_window_parts(label, used_percent, display);
    format!("{prefix}{value}")
}

fn format_window_parts(
    label: &str,
    used_percent: Option<f64>,
    display: UsageDisplay,
) -> (String, String) {
    let prefix = if !label.is_empty() {
        label.to_string()
    } else {
        Default::default()
    };
    let Some(percent) = used_percent.filter(|percent| percent.is_finite()) else {
        return (prefix, "–".to_string());
    };
    let percent = percent.clamp(0.0, 100.0);
    let value = match display {
        // Rate-limit windows only report a percentage, so numeric falls back to it.
        UsageDisplay::Percentage | UsageDisplay::Numeric => format!("{percent:.0}%"),
        UsageDisplay::Bar => {
            let filled = ((percent / 100.0) * BAR_WIDTH as f64).round() as usize;
            format!("{}{}", "▓".repeat(filled), "░".repeat(BAR_WIDTH - filled))
        }
        UsageDisplay::Sparkline if percent == 0.0 => SPARKLINE_EMPTY.to_string(),
        UsageDisplay::Sparkline => {
            let index = ((percent / 100.0) * (SPARKLINE.len() - 1) as f64).round() as usize;
            SPARKLINE[index].to_string()
        }
    };
    (prefix, value)
}

fn threshold_reached(cache: &UsageCache, windows: &UsageWindows, threshold: Option<f64>) -> bool {
    (windows.session.enabled && exceeds_threshold(cache.session, threshold))
        || (windows.weekly.enabled && exceeds_threshold(cache.weekly, threshold))
        || (windows.fable.enabled && exceeds_threshold(cache.fable, threshold))
        || (windows.credits_visible(cache)
            && exceeds_threshold(
                cache.credits.as_ref().and_then(CreditsUsage::percent_used),
                threshold,
            ))
}

fn exceeds_threshold(percent: Option<f64>, threshold: Option<f64>) -> bool {
    percent.is_some_and(|percent| {
        percent.is_finite() && threshold.is_some_and(|threshold| percent >= threshold)
    })
}

fn cache_path_for(provider: UsageProvider) -> Option<PathBuf> {
    Some(
        crate::platform::cache_dir()?
            .join("superline")
            .join(format!("usage-{}.json", provider.as_str())),
    )
}

fn read_cache(path: &Path) -> Option<UsageCache> {
    serde_json::from_reader(File::open(path).ok()?).ok()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn is_stale(fetched_at: u64) -> bool {
    now_secs().saturating_sub(fetched_at) >= CACHE_TTL.as_secs()
}

#[cfg(test)]
fn refresh_attempt_is_recent(marker_path: &Path) -> bool {
    let Ok(timestamp) = fs::read_to_string(marker_path) else {
        return false;
    };
    timestamp
        .trim()
        .parse::<u128>()
        .ok()
        .is_some_and(|then| now_millis().saturating_sub(then) < REFRESH_INTERVAL.as_millis())
}

/// Atomically claim this provider's refresh slot. The marker remains after the
/// child finishes so failed lookups are rate-limited too. Locking the marker
/// prevents concurrent prompt processes from both winning when it expires.
fn claim_refresh(marker_path: &Path) -> bool {
    let Ok(mut marker) = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(marker_path)
    else {
        return false;
    };
    if FileExt::try_lock_exclusive(&marker).is_err() {
        return false;
    }

    let mut timestamp = String::new();
    if marker.read_to_string(&mut timestamp).is_err() {
        return false;
    }
    let now = now_millis();
    if timestamp
        .trim()
        .parse::<u128>()
        .ok()
        .is_some_and(|then| now.saturating_sub(then) < REFRESH_INTERVAL.as_millis())
    {
        return false;
    }

    marker.set_len(0).is_ok()
        && marker.seek(SeekFrom::Start(0)).is_ok()
        && write!(marker, "{now}").is_ok()
}

fn spawn_refresh(provider: UsageProvider, cache_path: &Path) {
    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let marker_path = cache_path.with_extension("refresh");
    if !claim_refresh(&marker_path) {
        return;
    }

    let Ok(exe) = std::env::current_exe() else {
        let _ = fs::remove_file(marker_path);
        return;
    };
    if Command::new(exe)
        .arg("refresh-usage")
        .args(["--provider", provider.as_str()])
        .arg("--cache")
        .arg(cache_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_err()
    {
        let _ = fs::remove_file(marker_path);
    }
}

/// Refresh a provider cache through its own interactive CLI. A failed refresh
/// leaves the last good cache intact.
pub fn refresh_usage(provider: UsageProvider, cache_path: &Path) {
    if let Some(cache) = fetch_usage(provider) {
        write_cache(cache_path, &cache);
    }
}

fn fetch_usage(provider: UsageProvider) -> Option<UsageCache> {
    let parsed = match provider {
        UsageProvider::Claude => parse_claude_usage(&capture_claude_cli()?),
        UsageProvider::Codex => fetch_codex_rate_limits()?,
    };
    parsed.session?;

    Some(UsageCache {
        session: parsed.session,
        weekly: parsed.weekly,
        fable: parsed.fable,
        credits: parsed.credits,
        session_resets_at: parsed.session_resets_at,
        fetched_at: now_secs(),
    })
}

/// Codex exposes the same rate-limit read its `/status` card uses over the
/// app-server JSON-RPC protocol, so no terminal emulation or keystrokes are
/// needed. The TUI is unsafe to drive: it treats a burst of keystrokes as a
/// paste and can hand the slash command to the model as a prompt, and its
/// first `/status` after launch only says "refresh requested".
fn fetch_codex_rate_limits() -> Option<ParsedUsage> {
    let binary = resolve_binary("codex")?;
    let mut child = Command::new(binary)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let result = (|| {
        let mut stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let requests = [
            json!({
                "id": 1,
                "method": "initialize",
                "params": {
                    "clientInfo": {
                        "name": "superline",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                },
            }),
            json!({"method": "initialized", "params": {}}),
            json!({"id": 2, "method": "account/rateLimits/read", "params": {}}),
        ];
        for request in requests {
            writeln!(stdin, "{request}").ok()?;
        }
        stdin.flush().ok()?;

        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let deadline = Instant::now() + CODEX_APP_SERVER_TIMEOUT;
        loop {
            let remaining = deadline.checked_duration_since(Instant::now())?;
            let line = receiver.recv_timeout(remaining).ok()?;
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message.get("id").and_then(Value::as_u64) == Some(2) {
                return parse_codex_rate_limits(&message);
            }
        }
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}

/// Codex reports the five-hour window as `primary` and the weekly window as
/// `secondary`; the durations settle it when both are present. Credits come
/// from `individualLimit`, the per-seat credit budget, as a plain count.
fn parse_codex_rate_limits(message: &Value) -> Option<ParsedUsage> {
    let limits = message.get("result")?.get("rateLimits")?;
    let window = |name: &str| {
        let window = limits.get(name)?;
        let used = window.get("usedPercent")?.as_f64()?;
        Some((
            window.get("windowDurationMins").and_then(Value::as_u64),
            used,
            window.get("resetsAt").and_then(Value::as_u64),
        ))
    };
    let (mut session, mut weekly) = (window("primary"), window("secondary"));
    if let (Some((Some(short), _, _)), Some((Some(long), _, _))) = (session, weekly) {
        if short > long {
            std::mem::swap(&mut session, &mut weekly);
        }
    }
    let credits = limits.get("individualLimit").and_then(|limit| {
        Some(CreditsUsage {
            used: number_field(limit, "used")?,
            limit: number_field(limit, "limit")?,
            unit: CreditsUnit::Count,
        })
    });
    Some(ParsedUsage {
        session: session.map(|(_, used, _)| used.clamp(0.0, 100.0)),
        weekly: weekly.map(|(_, used, _)| used.clamp(0.0, 100.0)),
        fable: None,
        credits,
        session_resets_at: session.and_then(|(_, _, resets_at)| resets_at),
    })
}

/// Codex serialises the credit budget figures as strings.
fn number_field(value: &Value, key: &str) -> Option<f64> {
    match value.get(key)? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// Claude renders its quota panel only when connected to a terminal, so run it
/// in a small pseudo-terminal and issue the slash command
fn capture_claude_cli() -> Option<String> {
    let provider = UsageProvider::Claude;
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 60,
            cols: 200,
            pixel_width: 0,
            pixel_height: 0,
        })
        .ok()?;

    let binary = resolve_binary(provider.as_str())?;
    let mut command = CommandBuilder::new(binary);
    command.env("TERM", "xterm-256color");
    command.env("DISABLE_AUTOUPDATER", "1");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("ANTHROPIC_") {
            command.env_remove(key);
        }
    }
    command.args([
        "--allowed-tools",
        "",
        "--strict-mcp-config",
        "--session-id",
        CLAUDE_PROBE_SESSION_ID,
    ]);
    let probe_directory = probe_directory(provider)?;
    cleanup_claude_probe_sessions(&probe_directory);
    command.cwd(&probe_directory);

    let mut child = pair.slave.spawn_command(command).ok()?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().ok()?;
    let mut writer = pair.master.take_writer().ok()?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if sender.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut output = Vec::new();
    thread::sleep(Duration::from_secs(2));
    while let Ok(chunk) = receiver.try_recv() {
        output.extend_from_slice(&chunk);
    }
    // A fresh, private probe directory may show Claude's one-time trust
    // prompt. Only accept it when the captured screen proves that exact prompt
    // is active; this directory contains no user project files.
    let initial_screen = strip_terminal_sequences(&String::from_utf8_lossy(&output));
    let normalized_initial: String = initial_screen
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    if normalized_initial.contains("quicksafetycheck:") {
        // The first row is "No, exit" and the second is "Yes, I trust this
        // folder". Move to the latter before confirming.
        if writer.write_all(b"\x1b[B\r").is_err() || writer.flush().is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(500));
    }
    if writer.write_all(b"/usage\r").is_err() || writer.flush().is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut last_enter = Instant::now();
    let mut parsed_at = None;
    while Instant::now() < deadline && output.len() < MAX_CAPTURE_BYTES {
        if let Ok(chunk) = receiver.recv_timeout(Duration::from_millis(100)) {
            output.extend_from_slice(&chunk);
            if chunk.windows(4).any(|window| window == b"\x1b[6n") {
                let _ = writer.write_all(b"\x1b[1;1R");
            }
            let text = String::from_utf8_lossy(&output);
            let parsed = parse_claude_usage(&text);
            if parsed.session.is_some() && parsed.weekly.is_some() {
                parsed_at.get_or_insert_with(Instant::now);
            }
        }

        if last_enter.elapsed() >= Duration::from_millis(800) {
            let _ = writer.write_all(b"\r");
            let _ = writer.flush();
            last_enter = Instant::now();
        }
        if parsed_at.is_some_and(|at| at.elapsed() >= Duration::from_millis(750)) {
            break;
        }
    }

    let _ = writer.write_all(b"/exit\r");
    let _ = writer.flush();
    let _ = child.kill();
    let _ = child.wait();
    cleanup_claude_probe_sessions(&probe_directory);
    let output = String::from_utf8(output).ok()?;
    Some(output)
}

/// Use a private, tool-owned directory so accepting Claude's one-time trust
/// prompt never grants access to a user project.
fn probe_directory(provider: UsageProvider) -> Option<PathBuf> {
    let directory = crate::platform::cache_dir()?
        .join("superline")
        .join(format!("usage-probe-{}", provider.as_str()));
    fs::create_dir_all(&directory).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&directory, fs::Permissions::from_mode(0o700));
    }
    Some(directory)
}

fn cleanup_claude_probe_sessions(probe_directory: &Path) {
    let Some(home) = crate::platform::home_dir() else {
        return;
    };
    let project_name: String = probe_directory
        .to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let project_directory = home.join(".claude/projects").join(project_name);
    let Ok(entries) = fs::read_dir(&project_directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
        {
            let _ = fs::remove_file(path);
        }
    }
}

fn resolve_binary(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        for extension in ["exe", "cmd", "bat"] {
            let candidate = directory.join(format!("{name}.{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[derive(Debug, PartialEq)]
struct ParsedUsage {
    session: Option<f64>,
    weekly: Option<f64>,
    fable: Option<f64>,
    credits: Option<CreditsUsage>,
    session_resets_at: Option<u64>,
}

fn parse_claude_usage(text: &str) -> ParsedUsage {
    let clean = strip_terminal_sequences(text);
    ParsedUsage {
        session: percent_near_label(&clean, r"current\s*session"),
        weekly: percent_near_label(
            &clean,
            r"current\s*week\s*\(\s*all\s*m\s*o\s*d\s*e\s*l\s*s\s*\)",
        ),
        fable: percent_near_label(&clean, r"current\s*week\s*\(\s*f\s*a\s*b\s*l\s*e\s*\)"),
        credits: parse_claude_credits(&clean),
        session_resets_at: parse_claude_session_reset(&clean),
    }
}

/// Claude reports the session reset as a local clock time (for example,
/// `Resets 2:50pm`). A clock-only value is the next matching local time.
fn parse_claude_session_reset(text: &str) -> Option<u64> {
    parse_claude_session_reset_at(text, Local::now())
}

fn parse_claude_session_reset_at(text: &str, now: DateTime<Local>) -> Option<u64> {
    let session_section = Regex::new(
        r"(?is)current\s*session(?P<section>.{0,500}?)(?:current\s*week|usage\s*credits|$)",
    )
    .ok()?
    .captures_iter(text)
    .last()?
    .name("section")?
    .as_str();
    let reset =
        Regex::new(r"(?is)resets\s+(?:at\s+)?([0-9]{1,2})(?::([0-9]{2}))?\s*([ap])\.?m\.?").ok()?;
    let captures = reset.captures_iter(session_section).last()?;
    let hour: u32 = captures.get(1)?.as_str().parse().ok()?;
    let minute: u32 = captures
        .get(2)
        .map_or(Some(0), |capture| capture.as_str().parse().ok())?;
    if !(1..=12).contains(&hour) || minute >= 60 {
        return None;
    }
    let hour = match captures.get(3)?.as_str().to_ascii_lowercase().as_str() {
        "a" if hour == 12 => 0,
        "a" => hour,
        "p" if hour == 12 => 12,
        "p" => hour + 12,
        _ => return None,
    };
    let timestamp_for = |date: chrono::NaiveDate| match Local
        .from_local_datetime(&date.and_hms_opt(hour, minute, 0)?)
    {
        LocalResult::Single(reset) => Some(reset.timestamp()),
        LocalResult::Ambiguous(first, _) => Some(first.timestamp()),
        LocalResult::None => None,
    };
    let reset = timestamp_for(now.date_naive())?;
    let reset = if reset <= now.timestamp() {
        timestamp_for(now.date_naive().succ_opt()?)?
    } else {
        reset
    };
    u64::try_from(reset).ok()
}

/// The credits row reads `Usage credits … $12.50 / $500.00 spent`.
fn parse_claude_credits(text: &str) -> Option<CreditsUsage> {
    let pattern = Regex::new(
        r"(?is)usage\s*credits.{0,500}?\$\s*([0-9][0-9,]*(?:\.[0-9]+)?)\s*/\s*\$\s*([0-9][0-9,]*(?:\.[0-9]+)?)\s*(spent|used)",
    )
    .ok()?;
    let captures = pattern.captures_iter(text).last()?;
    let amount = |index: usize| -> Option<f64> {
        captures.get(index)?.as_str().replace(',', "").parse().ok()
    };
    Some(CreditsUsage {
        used: amount(1)?,
        limit: amount(2)?,
        unit: CreditsUnit::Dollars,
    })
}

fn percent_near_label(text: &str, label: &str) -> Option<f64> {
    let pattern = format!(
        r"(?is){label}.{{0,500}}?([0-9]{{1,3}}(?:\.[0-9]+)?)\s*%\s*(used|spent|consumed|left|remaining|available)"
    );
    let captures = Regex::new(&pattern).ok()?.captures_iter(text).last()?;
    let percent: f64 = captures.get(1)?.as_str().parse().ok()?;
    let qualifier = captures.get(2)?.as_str().to_ascii_lowercase();
    let used = match qualifier.as_str() {
        "used" | "spent" | "consumed" => percent,
        "left" | "remaining" | "available" => 100.0 - percent,
        _ => return None,
    };
    Some(used.clamp(0.0, 100.0))
}

fn strip_terminal_sequences(text: &str) -> String {
    let osc = Regex::new(r"\x1b\][^\x07]*(?:\x07|\x1b\\)").expect("valid OSC regex");
    let csi = Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("valid CSI regex");
    let without_osc = osc.replace_all(text, "");
    csi.replace_all(&without_osc, "").into_owned()
}

fn write_cache(path: &Path, cache: &UsageCache) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("tmp");
    if let Ok(mut file) = File::create(&tmp) {
        if serde_json::to_writer(&mut file, cache).is_ok() && file.flush().is_ok() {
            let _ = fs::rename(tmp, path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_rate_limit_windows_from_app_server_response() {
        let message = json!({
            "id": 2,
            "result": {
                "rateLimits": {
                    "primary": {"usedPercent": 17, "windowDurationMins": 300, "resetsAt": 1},
                    "secondary": {"usedPercent": 37, "windowDurationMins": 10080, "resetsAt": 2},
                    "planType": "business"
                }
            }
        });
        assert_eq!(
            parse_codex_rate_limits(&message),
            Some(ParsedUsage {
                session: Some(17.0),
                weekly: Some(37.0),
                fable: None,
                credits: None,
                session_resets_at: Some(1),
            })
        );

        let swapped = json!({
            "id": 2,
            "result": {
                "rateLimits": {
                    "primary": {"usedPercent": 37, "windowDurationMins": 10080},
                    "secondary": {"usedPercent": 17, "windowDurationMins": 300}
                }
            }
        });
        assert_eq!(
            parse_codex_rate_limits(&swapped),
            Some(ParsedUsage {
                session: Some(17.0),
                weekly: Some(37.0),
                fable: None,
                credits: None,
                session_resets_at: None,
            })
        );

        let session_only = json!({
            "id": 2,
            "result": {"rateLimits": {"primary": {"usedPercent": 120}, "secondary": null}}
        });
        assert_eq!(
            parse_codex_rate_limits(&session_only),
            Some(ParsedUsage {
                session: Some(100.0),
                weekly: None,
                fable: None,
                credits: None,
                session_resets_at: None,
            })
        );

        let with_credits = json!({
            "id": 2,
            "result": {
                "rateLimits": {
                    "primary": {"usedPercent": 100, "windowDurationMins": 300},
                    "secondary": {"usedPercent": 29, "windowDurationMins": 10080},
                    "credits": {"hasCredits": true, "unlimited": false, "balance": null},
                    "individualLimit": {"limit": "12000", "used": "410.7836902141571", "remainingPercent": 97}
                }
            }
        });
        assert_eq!(
            parse_codex_rate_limits(&with_credits).and_then(|parsed| parsed.credits),
            Some(CreditsUsage {
                used: 410.7836902141571,
                limit: 12000.0,
                unit: CreditsUnit::Count,
            })
        );

        let error = json!({
            "id": 2,
            "error": {"code": -32603, "message": "failed to fetch codex rate limits"}
        });
        assert_eq!(parse_codex_rate_limits(&error), None);
    }

    #[test]
    fn parses_claude_used_percentages_from_ansi_output() {
        let text = "\x1b[2JSettings: Usage\nCurrent session\n17% used\n\
                    Current week (all models)\n42% used\x1b[0m";
        assert_eq!(
            parse_claude_usage(text),
            ParsedUsage {
                session: Some(17.0),
                weekly: Some(42.0),
                fable: None,
                credits: None,
                session_resets_at: None,
            }
        );
    }

    #[test]
    fn parses_claude_weekly_label_split_by_terminal_repaints() {
        let text = "Current session 5% used\nCurrent week (all m odels) 10% used";
        assert_eq!(
            parse_claude_usage(text),
            ParsedUsage {
                session: Some(5.0),
                weekly: Some(10.0),
                fable: None,
                credits: None,
                session_resets_at: None,
            }
        );
    }

    #[test]
    fn parses_claude_fable_window_when_present() {
        let text = "Current session\n███████ 14%used\n\
                    Current week (all models)\n██████▌ 13% used\n\
                    Current week (Fable)\n████████ 16% used";
        assert_eq!(
            parse_claude_usage(text),
            ParsedUsage {
                session: Some(14.0),
                weekly: Some(13.0),
                fable: Some(16.0),
                credits: None,
                session_resets_at: None,
            }
        );
    }

    #[test]
    fn parses_claude_usage_credits_row() {
        let text = "Current session\n████ 25%used\n\
                    Current week (all models)\n████ 32%used\n\
                    Current week (Fable)\n████ 39%used\n\
                    Usage credits                     3%used\
                    $12.50 / $500.00 spent · Resets Oct 1 (America/New_York)";
        assert_eq!(
            parse_claude_usage(text),
            ParsedUsage {
                session: Some(25.0),
                weekly: Some(32.0),
                fable: Some(39.0),
                credits: Some(CreditsUsage {
                    used: 12.5,
                    limit: 500.0,
                    unit: CreditsUnit::Dollars,
                }),
                session_resets_at: None,
            }
        );
    }

    #[test]
    fn parses_claude_session_reset_as_the_next_local_clock_time() {
        let now = Local
            .with_ymd_and_hms(2026, 9, 9, 10, 0, 0)
            .single()
            .expect("valid local test time");
        let text = "Current session\n17% used\nResets 2:50pm\n\
                    Current week (all models)\n42% used\nResets 8pm";
        let expected = Local
            .with_ymd_and_hms(2026, 9, 9, 14, 50, 0)
            .single()
            .expect("valid local test time")
            .timestamp() as u64;
        assert_eq!(parse_claude_session_reset_at(text, now), Some(expected));

        let now = Local
            .with_ymd_and_hms(2026, 9, 9, 18, 0, 0)
            .single()
            .expect("valid local test time");
        let expected = Local
            .with_ymd_and_hms(2026, 9, 10, 14, 50, 0)
            .single()
            .expect("valid local test time")
            .timestamp() as u64;
        assert_eq!(parse_claude_session_reset_at(text, now), Some(expected));
    }

    #[test]
    fn cache_without_fable_field_still_loads() {
        let cache: UsageCache =
            serde_json::from_str(r#"{"session":12.0,"weekly":13.0,"fetched_at":1}"#)
                .expect("pre-fable cache should deserialize");
        assert_eq!(cache.fable, None);
        assert_eq!(cache.credits, None);
    }

    fn windows(
        provider: UsageProvider,
        session: bool,
        weekly: bool,
        fable: bool,
        labels: Option<&str>,
    ) -> UsageWindows {
        let label = labels.map(str::to_string);
        UsageWindows::new(
            UsageWindows::session(session, label.clone()),
            UsageWindows::weekly(weekly, label.clone()),
            UsageWindows::fable(fable, label),
            UsageWindows::credits(false, None, UsageDisplay::Numeric, false),
            provider,
        )
    }

    fn with_credits(
        mut windows: UsageWindows,
        display: UsageDisplay,
        only_when_limited: bool,
    ) -> UsageWindows {
        windows.credits = UsageWindows::credits(true, None, display, only_when_limited);
        windows
    }

    fn cache(session: f64, credits: Option<CreditsUsage>) -> UsageCache {
        UsageCache {
            session: Some(session),
            weekly: Some(20.0),
            fable: None,
            credits,
            session_resets_at: None,
            fetched_at: 0,
        }
    }

    const DOLLARS: CreditsUsage = CreditsUsage {
        used: 50.0,
        limit: 100.0,
        unit: CreditsUnit::Dollars,
    };

    #[test]
    fn credits_display_is_configured_separately_from_the_windows() {
        let windows = with_credits(
            windows(UsageProvider::Claude, true, false, false, None),
            UsageDisplay::Numeric,
            false,
        );
        assert_eq!(
            format_usage(
                UsageProvider::Claude,
                &cache(12.0, Some(DOLLARS)),
                &windows,
                UsageDisplay::Sparkline,
                false,
                0.0,
            ),
            "\u{ec82} 5h ▂ C $50/$100"
        );

        let count = CreditsUsage {
            used: 410.78,
            limit: 12000.0,
            unit: CreditsUnit::Count,
        };
        assert_eq!(
            format_credits("", Some(&count), UsageDisplay::Numeric),
            "411/12000"
        );
        assert_eq!(
            format_credits("", Some(&DOLLARS), UsageDisplay::Percentage),
            "50%"
        );
        assert_eq!(
            format_credits("", Some(&DOLLARS), UsageDisplay::Bar),
            "▓▓▓░░"
        );
        assert_eq!(
            format_credits("", Some(&DOLLARS), UsageDisplay::Sparkline),
            "▅"
        );
        assert_eq!(format_credits("C", None, UsageDisplay::Numeric), "C–");
        assert_eq!(format_amount(12.5, CreditsUnit::Dollars), "$12.50");
        assert_eq!(format_amount(1234.0, CreditsUnit::Dollars), "$1234");
    }

    #[test]
    fn credits_can_be_hidden_until_a_window_is_exhausted() {
        let windows = with_credits(
            windows(UsageProvider::Codex, true, false, false, Some("")),
            UsageDisplay::Numeric,
            true,
        );
        assert_eq!(
            format_usage(
                UsageProvider::Codex,
                &cache(40.0, Some(DOLLARS)),
                &windows,
                UsageDisplay::Percentage,
                false,
                0.0,
            ),
            "\u{ec81} 40%"
        );
        assert_eq!(
            format_usage(
                UsageProvider::Codex,
                &cache(100.0, Some(DOLLARS)),
                &windows,
                UsageDisplay::Percentage,
                false,
                0.0,
            ),
            "\u{ec81} 100% C $50/$100"
        );
    }

    #[test]
    fn session_reset_countdown_can_be_limited_to_a_fullness_threshold() {
        let cache = UsageCache {
            session: Some(79.0),
            weekly: Some(67.8),
            fable: None,
            credits: None,
            session_resets_at: Some(now_secs().saturating_add(3600)),
            fetched_at: 0,
        };
        let windows = windows(UsageProvider::Codex, true, false, false, None);
        let format = |cache: &UsageCache| {
            format_usage(
                UsageProvider::Codex,
                cache,
                &windows,
                UsageDisplay::Percentage,
                true,
                0.8,
            )
        };

        assert!(!format(&cache).contains('↻'));
        assert!(format(&UsageCache {
            session: Some(80.0),
            ..cache
        })
        .contains('↻'));
    }

    #[test]
    fn remaining_duration_is_compact() {
        assert_eq!(format_remaining_duration(2 * 3600 + 34 * 60), "2h 34m");
        assert_eq!(format_remaining_duration(59 * 60), "59m");
        assert_eq!(format_remaining_duration(59), "now");
    }

    #[test]
    fn numeric_display_falls_back_to_percentage_for_rate_limit_windows() {
        assert_eq!(
            format_window("5h", Some(61.0), UsageDisplay::Numeric),
            "5h61%"
        );
    }

    #[test]
    fn threshold_considers_visible_credits() {
        let windows = with_credits(
            windows(UsageProvider::Claude, true, false, false, None),
            UsageDisplay::Numeric,
            true,
        );
        let credits = CreditsUsage {
            used: 90.0,
            limit: 100.0,
            unit: CreditsUnit::Dollars,
        };
        assert!(!threshold_reached(
            &cache(10.0, Some(credits)),
            &windows,
            Some(85.0)
        ));
        assert!(threshold_reached(
            &cache(100.0, Some(credits)),
            &windows,
            Some(85.0)
        ));
    }

    #[test]
    fn percentage_display_can_select_windows() {
        let cache = UsageCache {
            session: Some(12.4),
            weekly: Some(67.8),
            fable: Some(33.3),
            credits: None,
            session_resets_at: None,
            fetched_at: 0,
        };
        assert_eq!(
            format_usage(
                UsageProvider::Claude,
                &cache,
                &windows(UsageProvider::Claude, true, false, false, None),
                UsageDisplay::Percentage,
                false,
                0.0,
            ),
            "\u{ec82} 5h 12%"
        );
        assert_eq!(
            format_usage(
                UsageProvider::Codex,
                &cache,
                &windows(UsageProvider::Codex, false, true, false, None),
                UsageDisplay::Percentage,
                false,
                0.0,
            ),
            "\u{ec81}  7d 68%"
        );
        assert_eq!(
            format_usage(
                UsageProvider::Claude,
                &cache,
                &windows(UsageProvider::Claude, true, true, true, None),
                UsageDisplay::Percentage,
                false,
                0.0,
            ),
            "\u{ec82} 5h 12% 7d 68% F 33%"
        );
        assert_eq!(
            format_usage(
                UsageProvider::Claude,
                &cache,
                &windows(UsageProvider::Claude, true, true, false, Some("")),
                UsageDisplay::Sparkline,
                false,
                0.0,
            ),
            "\u{ec82} ▂▆"
        );
    }

    #[test]
    fn fable_window_is_only_enabled_for_claude() {
        assert!(
            !windows(UsageProvider::Codex, true, true, true, None)
                .fable
                .enabled
        );
        assert!(
            windows(UsageProvider::Claude, true, true, true, None)
                .fable
                .enabled
        );
    }

    #[test]
    fn bar_display_is_clamped_and_fixed_width() {
        assert_eq!(
            format_window("5h", Some(61.0), UsageDisplay::Bar),
            "5h▓▓▓░░"
        );
        assert_eq!(
            format_window("7d", Some(120.0), UsageDisplay::Bar),
            "7d▓▓▓▓▓"
        );
        assert_eq!(format_window("7d", None, UsageDisplay::Bar), "7d–");
    }

    #[test]
    fn sparkline_display_uses_one_glyph_per_window() {
        assert_eq!(
            format_window("5h", Some(0.0), UsageDisplay::Sparkline),
            "5h "
        );
        assert_eq!(
            format_window("5h", Some(61.0), UsageDisplay::Sparkline),
            "5h▅"
        );
        assert_eq!(
            format_window("7d", Some(100.0), UsageDisplay::Sparkline),
            "7d█"
        );
    }

    #[test]
    fn threshold_checks_the_visible_windows() {
        let cache = UsageCache {
            session: Some(81.0),
            weekly: Some(79.0),
            fable: Some(95.0),
            credits: None,
            session_resets_at: None,
            fetched_at: 0,
        };

        let claude = UsageProvider::Claude;
        assert!(threshold_reached(
            &cache,
            &windows(claude, true, false, false, None),
            Some(80.0)
        ));
        assert!(!threshold_reached(
            &cache,
            &windows(claude, false, true, false, None),
            Some(80.0)
        ));
        assert!(threshold_reached(
            &cache,
            &windows(claude, true, true, false, None),
            Some(80.0)
        ));
        assert!(threshold_reached(
            &cache,
            &windows(claude, false, false, true, None),
            Some(80.0)
        ));
        assert!(!threshold_reached(
            &cache,
            &windows(claude, false, true, false, None),
            Some(90.0)
        ));
    }

    #[test]
    fn refresh_claim_is_atomic_and_rate_limits_failed_attempts() {
        let directory = std::env::temp_dir().join(format!(
            "superline-usage-refresh-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("create test directory");
        let marker = directory.join("usage-codex.refresh");

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let claims = (0..8)
            .map(|_| {
                let barrier = barrier.clone();
                let marker = marker.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    claim_refresh(&marker)
                })
            })
            .collect::<Vec<_>>();
        let winners = claims
            .into_iter()
            .map(|claim| claim.join().expect("refresh claim thread"))
            .filter(|claimed| *claimed)
            .count();

        assert_eq!(winners, 1);
        assert!(!claim_refresh(&marker));
        assert!(refresh_attempt_is_recent(&marker));

        let _ = fs::remove_dir_all(directory);
    }
}
