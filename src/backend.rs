//! Where widgets get data from outside the prompt, and the fixtures that can
//! stand in for it.
//!
//! Widgets that are not handed their data by the shell integration or Claude
//! Code's status line read it through one of two kinds of backend:
//!
//! - a [`Probe`] for quick, synchronous questions about the machine (the
//!   hostname, the battery, the clock), read with [`probe`];
//! - a [`crate::cache::Source`] for slow lookups (git status, a PR over the
//!   network, an AI provider's usage), loaded through [`crate::cache::Cached`].
//!
//! Setting `SUPERLINE_FIXTURES` to a JSON file, or to the JSON itself, swaps
//! any backend named in it for a canned value, so a prompt can be rendered,
//! screenshotted or recorded the same way on any machine, whatever is
//! installed or signed in there:
//!
//! ```json
//! {
//!   "clock": "2026-01-01T09:41:00",
//!   "hostname": "devbox",
//!   "battery": { "percent": 42.0, "state": "discharging" },
//!   "pr": { "feat/login": { "number": 148, "...": "..." }, "*": null },
//!   "usage": { "claude": { "session": 38.0 } },
//!   "git-status": { "$lookup": "loading" }
//! }
//! ```
//!
//! Each key is a [`Probe::KEY`] or a [`Source::KIND`], and its value is what
//! that backend would have produced after parsing. A source whose lookups
//! differ by a [`Source::fixture_key`] (a branch, a provider) takes an object
//! by that key instead, with `"*"` answering every other key. A source can
//! also be held in one of its other states with `{"$lookup": "loading"}` or
//! `{"$lookup": "unavailable"}`. A backend missing from the fixtures is asked
//! as usual, except those that phone home or change the installation (see
//! [`Source::LIVE_WITH_FIXTURES`]), which stay unavailable.
//!
//! A fixtured source never reads or writes the cache and never starts a
//! refresh, so its value is on the very first prompt.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::cache::{Lookup, Source};

/// The environment variable naming the fixtures file, or holding the JSON.
pub const FIXTURES_VAR: &str = "SUPERLINE_FIXTURES";

/// A quick, synchronous question about the machine the prompt runs on.
///
/// The implementing type holds whatever the question needs; most need
/// nothing. `probe` runs on the thread building the prompt, so anything slow
/// belongs in a [`Source`] instead.
pub trait Probe {
    /// What the question answers, as fixtures spell it.
    type Value: DeserializeOwned;

    /// Names this backend in the fixtures.
    const KEY: &'static str;

    /// Asks the machine. `None` means there is nothing to show, which a
    /// fixture spells `null`.
    fn probe(&self) -> Option<Self::Value>;
}

/// Answers `probe` from the fixtures when they name it, else from the
/// machine.
pub fn probe<P: Probe>(probe: P) -> Option<P::Value> {
    match fixtures().and_then(|fixtures| fixtures.get(P::KEY)) {
        Some(Value::Null) => None,
        Some(value) => parse(P::KEY, value),
        None => probe.probe(),
    }
}

/// What a fixture says a [`Source`] should load as, or `None` to load it as
/// usual.
pub(crate) fn source_lookup<S: Source>(source: &S) -> Option<Lookup<S::Value>> {
    let Some(fixtures) = fixtures() else {
        return None;
    };
    let Some(entry) = fixtures.get(S::KIND) else {
        return (!S::LIVE_WITH_FIXTURES).then_some(Lookup::Unavailable);
    };
    let value = match source.fixture_key() {
        Some(key) => match entry.as_object() {
            Some(by_key) => match by_key.get(&key).or_else(|| by_key.get("*")) {
                Some(value) => value,
                None => return Some(Lookup::Unavailable),
            },
            None => {
                warn(S::KIND, "expected an object keyed by fixture key");
                return Some(Lookup::Unavailable);
            }
        },
        None => entry,
    };
    if let Some(state) = value.get("$lookup") {
        return Some(match state.as_str() {
            Some("loading") => Lookup::Loading,
            Some("unavailable") => Lookup::Unavailable,
            _ => {
                warn(S::KIND, "`$lookup` must be \"loading\" or \"unavailable\"");
                Lookup::Unavailable
            }
        });
    }
    Some(match parse(S::KIND, value) {
        Some(value) => Lookup::Ready(value),
        None => Lookup::Unavailable,
    })
}

/// Whether fixtures are in use.
pub fn fixtures_active() -> bool {
    fixtures().is_some()
}

/// The current time, which the `clock` fixture can pin.
pub fn now() -> DateTime<Local> {
    probe(Clock).map_or_else(Local::now, |time| time.0)
}

/// [`now`] as seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    u64::try_from(now().timestamp()).unwrap_or(0)
}

/// The wall clock. Fixtures give it as RFC 3339 (`2026-01-01T09:41:00Z`) or
/// as a local time without an offset (`2026-01-01T09:41:00`).
struct Clock;

impl Probe for Clock {
    type Value = FixedTime;
    const KEY: &'static str = "clock";

    fn probe(&self) -> Option<FixedTime> {
        Some(FixedTime(Local::now()))
    }
}

struct FixedTime(DateTime<Local>);

impl<'de> serde::Deserialize<'de> for FixedTime {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_time(&text)
            .map(FixedTime)
            .ok_or_else(|| serde::de::Error::custom(format!("`{text}` is not a time")))
    }
}

fn parse_time(text: &str) -> Option<DateTime<Local>> {
    if let Ok(time) = DateTime::parse_from_rfc3339(text) {
        return Some(time.with_timezone(&Local));
    }
    let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S"))
        .ok()?;
    Local.from_local_datetime(&naive).earliest()
}

fn fixtures() -> Option<&'static Map<String, Value>> {
    static FIXTURES: OnceLock<Option<Map<String, Value>>> = OnceLock::new();
    FIXTURES
        .get_or_init(|| load(&std::env::var_os(FIXTURES_VAR)?.to_string_lossy()))
        .as_ref()
}

/// Reads the fixtures from the variable's value: JSON when it starts with
/// `{`, else a path to a JSON file.
fn load(setting: &str) -> Option<Map<String, Value>> {
    let setting = setting.trim();
    if setting.is_empty() {
        return None;
    }
    let text = if setting.starts_with('{') {
        setting.to_string()
    } else {
        match std::fs::read_to_string(Path::new(setting)) {
            Ok(text) => text,
            Err(err) => {
                warn(FIXTURES_VAR, &format!("cannot read {setting}: {err}"));
                return None;
            }
        }
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Some(map),
        Ok(_) => {
            warn(FIXTURES_VAR, "fixtures must be a JSON object");
            None
        }
        Err(err) => {
            warn(FIXTURES_VAR, &format!("not valid JSON: {err}"));
            None
        }
    }
}

fn parse<V: DeserializeOwned>(key: &str, value: &Value) -> Option<V> {
    match V::deserialize(value) {
        Ok(value) => Some(value),
        Err(err) => {
            warn(key, &err.to_string());
            None
        }
    }
}

/// A fixture that cannot be used is worth saying out loud: fixtures only
/// exist to make a prompt look a particular way.
fn warn(key: &str, message: &str) {
    static WARNED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let mut warned = WARNED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if warned.get_or_insert_with(HashSet::new).insert(key.to_string()) {
        eprintln!("superline: fixture `{key}`: {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn times_parse_with_and_without_an_offset() {
        let local = parse_time("2026-01-01T09:41:00").unwrap();
        assert_eq!((local.hour(), local.minute()), (9, 41));
        let utc = parse_time("2026-01-01T09:41:00Z").unwrap();
        assert_eq!(utc.timestamp(), 1_767_260_460);
        assert_eq!(parse_time("2026-01-01 09:41:00").unwrap().day(), 1);
        assert!(parse_time("tomorrow").is_none());
    }

    #[test]
    fn inline_and_file_fixtures_load() {
        assert_eq!(load(r#"{"hostname":"devbox"}"#).unwrap()["hostname"], "devbox");
        let dir = std::env::temp_dir().join(format!("superline-fixtures-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("fixtures.json");
        std::fs::write(&file, r#"{"os":"macos"}"#).unwrap();
        assert_eq!(load(file.to_str().unwrap()).unwrap()["os"], "macos");
        assert!(load("[]").is_none());
        assert!(load("").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
