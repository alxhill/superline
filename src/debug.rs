//! Opt-in timing for a prompt render, enabled with `SUPERLINE_DEBUG=1`.
//!
//! Spans nest: `render` holds a row, a row holds the modules it draws, and a
//! module holds any cached lookup it went through. [`report`] prints the whole
//! tree to stderr once the prompt has been written, so stdout is untouched.
//!
//! Every entry point is a no-op when the variable is unset, so the instrumented
//! paths cost nothing in a normal prompt.

use std::borrow::Cow;
use std::env;
use std::fmt::{self, Display, Write as _};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Whether `SUPERLINE_DEBUG` asks for a timing report.
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| match env::var("SUPERLINE_DEBUG") {
        Ok(value) => !matches!(value.trim(), "" | "0" | "false" | "no"),
        Err(_) => false,
    })
}

/// Marks the start of the process. Call it first thing in `main` so the report
/// can attribute the time spent before the first span.
pub fn init() {
    let _ = start();
}

fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

struct Event {
    depth: usize,
    /// Offset from [`start`] at which this was recorded.
    at: Duration,
    label: Cow<'static, str>,
    /// `None` for a [`note`], which explains a decision rather than timing
    /// one and so leaves the report's time column blank.
    elapsed: Option<Duration>,
    detail: Option<String>,
}

struct Collector {
    events: Vec<Event>,
    depth: usize,
}

static STATE: Mutex<Collector> = Mutex::new(Collector {
    events: Vec::new(),
    depth: 0,
});

fn with_state<T>(f: impl FnOnce(&mut Collector) -> T) -> T {
    let mut state = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut state)
}

/// An open timing span. Recording happens in [`Span::finish`].
#[must_use = "a span records nothing until it is finished"]
pub struct Span(Option<Open>);

struct Open {
    label: Cow<'static, str>,
    depth: usize,
    /// Where the span's own line goes, ahead of the children it collected.
    mark: usize,
    at: Duration,
    started: Instant,
}

/// Opens a span. Anything recorded before it is finished nests underneath it.
pub fn span(label: impl Into<Cow<'static, str>>) -> Span {
    if !enabled() {
        return Span(None);
    }
    let at = start().elapsed();
    with_state(|state| {
        let open = Open {
            label: label.into(),
            depth: state.depth,
            mark: state.events.len(),
            at,
            started: Instant::now(),
        };
        state.depth += 1;
        Span(Some(open))
    })
}

impl Span {
    pub fn finish(self) {
        let Some(open) = self.0 else {
            return;
        };
        let elapsed = open.started.elapsed();
        with_state(|state| {
            state.depth = state.depth.saturating_sub(1);
            let event = Event {
                depth: open.depth,
                at: open.at,
                label: open.label,
                elapsed: Some(elapsed),
                detail: None,
            };
            let mark = open.mark.min(state.events.len());
            state.events.insert(mark, event);
        });
    }
}

/// How a [`crate::cache::Cached`] lookup was served.
pub enum CacheStatus {
    /// Served from the cache, still within the source's TTL.
    Fresh(Duration),
    /// Served from the cache past its TTL; `refreshing` says whether a
    /// background refresh was started (or was already in flight).
    Stale { age: Duration, refreshing: bool },
    /// Nothing cached; a refresh was started for a later prompt.
    Miss,
    /// A refresh finished while the prompt waited for it.
    Waited { served: bool },
    /// The wait ran out; whatever was cached (if anything) was served instead.
    TimedOut { age: Option<Duration> },
    /// Nothing cached and no refresh possible.
    Unavailable,
}

impl Display for CacheStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheStatus::Fresh(age) => write!(f, "hit fresh (age {})", Age(*age)),
            CacheStatus::Stale {
                age,
                refreshing: true,
            } => write!(f, "hit stale (age {}), refresh spawned", Age(*age)),
            CacheStatus::Stale {
                age,
                refreshing: false,
            } => write!(f, "hit stale (age {}), no refresh", Age(*age)),
            CacheStatus::Miss => write!(f, "miss, refresh spawned"),
            CacheStatus::Waited { served: true } => write!(f, "refresh finished during wait"),
            CacheStatus::Waited { served: false } => write!(f, "refresh finished, no value"),
            CacheStatus::TimedOut { age: Some(age) } => {
                write!(f, "wait timed out, serving cache (age {})", Age(*age))
            }
            CacheStatus::TimedOut { age: None } => write!(f, "wait timed out, nothing cached"),
            CacheStatus::Unavailable => write!(f, "unavailable"),
        }
    }
}

/// Records one cached lookup under whichever span is currently open.
pub fn cache(kind: &'static str, status: CacheStatus, elapsed: Duration) {
    if !enabled() {
        return;
    }
    let at = start().elapsed();
    with_state(|state| {
        let event = Event {
            depth: state.depth,
            at,
            label: Cow::Borrowed(kind),
            elapsed: Some(elapsed),
            detail: Some(status.to_string()),
        };
        state.events.push(event);
    });
}

/// Records a decision the prompt made, under whichever span is currently open.
///
/// Unlike a [`span`] or a [`cache`] lookup a note has no duration: it says why
/// the work that surrounds it took the shape it did, so it prints with an
/// empty time column.
pub fn note(label: impl Into<Cow<'static, str>>, detail: impl Into<String>) {
    if !enabled() {
        return;
    }
    let at = start().elapsed();
    with_state(|state| {
        let event = Event {
            depth: state.depth,
            at,
            label: label.into(),
            elapsed: None,
            detail: Some(detail.into()),
        };
        state.events.push(event);
    });
}

/// Writes the collected timings to stderr and clears them.
pub fn report() {
    if !enabled() {
        return;
    }
    let total = start().elapsed();
    let events = with_state(|state| {
        state.depth = 0;
        std::mem::take(&mut state.events)
    });
    eprint!("{}", render(&events, total));
}

fn render(events: &[Event], total: Duration) -> String {
    let startup = events.first().map(|event| event.at).unwrap_or(total);
    let mut rows: Vec<(usize, &str, Option<Duration>, Option<&str>)> =
        vec![(0, "startup", Some(startup), None)];
    rows.extend(events.iter().map(|event| {
        (
            event.depth,
            event.label.as_ref(),
            event.elapsed,
            event.detail.as_deref(),
        )
    }));
    rows.push((0, "total", Some(total), None));

    let width = rows
        .iter()
        .map(|(depth, label, ..)| depth * 2 + label.chars().count())
        .max()
        .unwrap_or(0);

    let mut out = String::from("superline debug\n");
    for (depth, label, elapsed, detail) in rows {
        let pad = width - depth * 2 - label.chars().count();
        let timing = match elapsed {
            Some(elapsed) => Millis(elapsed).to_string(),
            None => String::new(),
        };
        let _ = write!(
            out,
            "  {:indent$}{label}{:pad$}  {timing:>8}",
            "",
            "",
            indent = depth * 2,
            pad = pad,
        );
        if let Some(detail) = detail {
            let _ = write!(out, "  {detail}");
        }
        out.push('\n');
    }
    out
}

/// An elapsed time, in whichever unit keeps the number readable.
struct Millis(Duration);

impl Display for Millis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let millis = self.0.as_secs_f64() * 1000.0;
        if millis >= 1000.0 {
            write!(f, "{:.2}s", millis / 1000.0)
        } else {
            write!(f, "{:.1}ms", millis)
        }
    }
}

/// The age of a cached value, rounded to something a human can scan.
struct Age(Duration);

impl Display for Age {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secs = self.0.as_secs();
        match secs {
            0..=59 => write!(f, "{secs}s"),
            60..=3599 => write!(f, "{}m", secs / 60),
            _ => write!(f, "{}h", secs / 3600),
        }
    }
}

/// The bare name of a module type, e.g. `Git` for
/// `superline::modules::git::Git<superline::themes::RainbowTheme>`.
pub fn type_label(name: &'static str) -> &'static str {
    let base = name.split('<').next().unwrap_or(name);
    base.rsplit("::").next().unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_label_drops_path_and_theme_parameter() {
        assert_eq!(
            type_label("superline::modules::git::Git<superline::themes::RainbowTheme>"),
            "Git"
        );
        assert_eq!(type_label("superline::modules::cwd::Cwd"), "Cwd");
        assert_eq!(type_label("Spacer"), "Spacer");
    }

    #[test]
    fn durations_switch_unit_above_a_second() {
        assert_eq!(Millis(Duration::from_micros(1500)).to_string(), "1.5ms");
        assert_eq!(Millis(Duration::from_millis(1250)).to_string(), "1.25s");
    }

    #[test]
    fn ages_round_to_the_largest_useful_unit() {
        assert_eq!(Age(Duration::from_secs(9)).to_string(), "9s");
        assert_eq!(Age(Duration::from_secs(150)).to_string(), "2m");
        assert_eq!(Age(Duration::from_secs(7200)).to_string(), "2h");
    }

    #[test]
    fn report_nests_children_and_bookends_with_startup_and_total() {
        let events = vec![
            Event {
                depth: 0,
                at: Duration::from_millis(2),
                label: "render".into(),
                elapsed: Some(Duration::from_millis(50)),
                detail: None,
            },
            Event {
                depth: 1,
                at: Duration::from_millis(2),
                label: "Git".into(),
                elapsed: Some(Duration::from_millis(49)),
                detail: None,
            },
            Event {
                depth: 2,
                at: Duration::from_millis(3),
                label: "backend".into(),
                elapsed: None,
                detail: Some(String::from("gitoxide (configured)")),
            },
            Event {
                depth: 2,
                at: Duration::from_millis(3),
                label: "git".into(),
                elapsed: Some(Duration::from_millis(48)),
                detail: Some(
                    CacheStatus::TimedOut {
                        age: Some(Duration::from_secs(120)),
                    }
                    .to_string(),
                ),
            },
        ];

        let report = render(&events, Duration::from_millis(55));
        let lines: Vec<&str> = report.lines().collect();

        assert_eq!(lines[0], "superline debug");
        assert!(lines[1].starts_with("  startup"), "{}", lines[1]);
        assert!(lines[1].ends_with("2.0ms"), "{}", lines[1]);
        assert!(lines[3].starts_with("    Git"), "{}", lines[3]);
        assert!(lines[5].starts_with("      git"), "{}", lines[5]);
        assert!(
            lines[5].ends_with("wait timed out, serving cache (age 2m)"),
            "{}",
            lines[5]
        );
        assert!(lines[6].starts_with("  total"), "{}", lines[6]);
        assert!(lines[6].ends_with("55.0ms"), "{}", lines[6]);
    }

    /// A note has no duration, so its time column is blank while staying
    /// aligned with the timed rows around it.
    #[test]
    fn a_note_leaves_the_time_column_empty() {
        let events = vec![Event {
            depth: 0,
            at: Duration::from_millis(1),
            label: "backend".into(),
            elapsed: None,
            detail: Some(String::from("cli (configured)")),
        }];

        // The note's blank time column keeps the same width as a timed row's,
        // so the labels and the values below stay in their columns.
        assert_eq!(
            render(&events, Duration::from_millis(9)),
            "superline debug\n\
             \x20 startup     1.0ms\n\
             \x20 backend            cli (configured)\n\
             \x20 total       9.0ms\n",
        );
    }
}
