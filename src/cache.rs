//! On-disk caches for slow lookups, refreshed by a detached child process.
//!
//! A prompt has to render in a few milliseconds, but some of what it shows
//! (git status on a large repo, a PR lookup over the network, an AI provider's
//! usage panel) takes far longer to compute. Modules describe such a lookup by
//! implementing [`Source`], and drive it through [`Cached`]:
//!
//! ```ignore
//! #[derive(Clone, Serialize, Deserialize)]
//! struct WeatherLookup { city: String }
//!
//! impl Source for WeatherLookup {
//!     type Value = Forecast;
//!     const KIND: &'static str = "weather";
//!     const TTL: Duration = Duration::from_secs(15 * 60);
//!
//!     fn cache_id(&self) -> String { hash_id(&self.city) }
//!     fn fetch(&self) -> Option<Forecast> { /* the slow part */ }
//! }
//!
//! // In `Module::append_segments`:
//! match Cached::new(WeatherLookup { city }).load() {
//!     Lookup::Ready(forecast) => powerline.add_segment(forecast.summary(), style),
//!     Lookup::Loading => powerline.add_segment("weather …", style),
//!     Lookup::Unavailable => {}
//! }
//! ```
//!
//! [`Cached::load`] never blocks: it serves whatever is on disk and, when that
//! is missing or older than [`Source::TTL`], re-executes the `superline` binary
//! with the hidden `refresh` subcommand to fetch a new value for a later
//! prompt. [`Cached::load_with_timeout`] does the same but is willing to wait
//! a little for that refresh, so a fast fetch still lands on the current
//! prompt. The fetch itself only ever runs in the child, so a prompt that
//! gives up waiting never wastes or duplicates its work.
//!
//! Every [`Source`] has to be registered in [`crate::modules::run_refresh`] so
//! the child process can find it by [`Source::KIND`].

use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Cached prompt data older than this is unlikely to be useful as a fallback.
const MAX_CACHE_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// Rendering happens frequently, so avoid walking the cache directory each time.
const PRUNE_INTERVAL: Duration = Duration::from_secs(60 * 60);
const PRUNE_MARKER: &str = ".last-pruned";
/// How often [`Cached::load_with_timeout`] checks whether the refresh it is
/// waiting on has finished. A `stat` per tick keeps the wait cheap.
const REFRESH_POLL_INTERVAL: Duration = Duration::from_millis(1);

/// A slow lookup whose result is served from an on-disk cache.
///
/// The implementing type holds the lookup's parameters. It is serialised to
/// JSON and handed to the detached child that performs the fetch, so it must
/// be small and self-contained.
pub trait Source: Serialize + DeserializeOwned {
    /// What the lookup produces. This is what ends up in the cache file.
    type Value: Serialize + DeserializeOwned;

    /// Names this kind of lookup. Used as the cache file prefix and to route
    /// the `refresh` subcommand back to this type.
    const KIND: &'static str;

    /// How long a cached value stays fresh. [`Cached::load`] schedules a
    /// background refresh once a value is older than this. Use
    /// [`Duration::ZERO`] for lookups that should always be re-run.
    const TTL: Duration;

    /// Minimum gap between two refresh attempts for the same cache entry. A
    /// refresh that fails, or one that hangs, will not be retried before this
    /// has elapsed. A refresh that succeeds releases the slot immediately.
    const REFRESH_INTERVAL: Duration = Duration::from_secs(20);

    /// Distinguishes this lookup's cache file from others of the same
    /// [`KIND`](Self::KIND). Prefer [`hash_id`] for anything long or path-like.
    fn cache_id(&self) -> String;

    /// Whether a fetch stands any chance of succeeding. Only consulted when
    /// nothing is cached yet, so a cheap check such as looking for a binary on
    /// `PATH` is fine. When it returns `false`, [`Cached::load`] reports
    /// [`Lookup::Unavailable`] rather than starting a refresh.
    fn fetchable(&self) -> bool {
        true
    }

    /// The slow part. Only ever runs in the detached child. Returning `None`
    /// leaves the previous cached value in place.
    fn fetch(&self) -> Option<Self::Value>;
}

/// Hashes anything into a fixed-width identifier suitable for
/// [`Source::cache_id`].
pub fn hash_id<T: Hash + ?Sized>(key: &T) -> String {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// What a lookup has to show right now.
pub enum Lookup<V> {
    /// A value is available. It may be older than the source's TTL, in which
    /// case a background refresh has already been scheduled.
    Ready(V),
    /// Nothing is cached yet and a refresh is in flight (or was just started).
    Loading,
    /// Nothing is cached and no refresh can run: the source reports it cannot
    /// fetch, or there is no cache directory to refresh into.
    Unavailable,
}

impl<V> Lookup<V> {
    /// The value, if there is one.
    pub fn ready(self) -> Option<V> {
        match self {
            Lookup::Ready(value) => Some(value),
            Lookup::Loading | Lookup::Unavailable => None,
        }
    }
}

/// A cached value together with when it was fetched.
#[derive(Serialize, Deserialize)]
pub struct Entry<V> {
    pub fetched_at: u64,
    pub value: V,
}

impl<V> Entry<V> {
    fn now(value: V) -> Self {
        Entry {
            fetched_at: now_secs(),
            value,
        }
    }

    pub fn age(&self) -> Duration {
        Duration::from_secs(now_secs().saturating_sub(self.fetched_at))
    }

    pub fn is_stale(&self, ttl: Duration) -> bool {
        self.age() >= ttl
    }
}

/// Drives a [`Source`] against its cache file.
pub struct Cached<S> {
    source: S,
    /// `None` when no cache directory can be determined, in which case nothing
    /// is read, written or refreshed.
    path: Option<PathBuf>,
}

impl<S: Source> Cached<S> {
    /// Binds `source` to its file under Superline's cache directory.
    pub fn new(source: S) -> Self {
        let dir = crate::platform::cache_dir().map(|base| base.join("superline"));
        Self::in_dir(source, dir)
    }

    /// Like [`new`](Self::new), with an explicit cache directory.
    pub fn in_dir(source: S, dir: Option<PathBuf>) -> Self {
        let path = dir.map(|dir| dir.join(format!("{}-{}.json", S::KIND, source.cache_id())));
        Cached { source, path }
    }

    pub fn source(&self) -> &S {
        &self.source
    }

    /// The cache file, when there is a cache directory.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Reads the cache without triggering a refresh.
    pub fn read(&self) -> Option<Entry<S::Value>> {
        let file = File::open(self.path.as_ref()?).ok()?;
        serde_json::from_reader(file).ok()
    }

    /// Serves the cached value and refreshes it in the background when it is
    /// missing or older than [`Source::TTL`]. Never blocks on the fetch.
    pub fn load(&self) -> Lookup<S::Value> {
        let entry = self.read();
        match entry {
            Some(entry) => {
                if entry.is_stale(S::TTL) {
                    self.refresh_in_background();
                }
                Lookup::Ready(entry.value)
            }
            None if self.path.is_some() && self.source.fetchable() => {
                self.refresh_in_background();
                Lookup::Loading
            }
            None => Lookup::Unavailable,
        }
    }

    /// Like [`load`](Self::load), but waits up to `timeout` for the refresh
    /// it starts (or finds already running) to finish, so a quick fetch is
    /// served fresh on this prompt. When the wait runs out the cached value,
    /// if any, is served instead and the child carries on for the next
    /// prompt.
    pub fn load_with_timeout(&self, timeout: Duration) -> Lookup<S::Value> {
        let cached = match self.read() {
            Some(entry) if !entry.is_stale(S::TTL) => return Lookup::Ready(entry.value),
            entry => entry,
        };
        let Some(path) = &self.path else {
            return Lookup::Unavailable;
        };
        if cached.is_none() && !self.source.fetchable() {
            return Lookup::Unavailable;
        }
        if !self.refresh_in_background() {
            return cached
                .map(|entry| Lookup::Ready(entry.value))
                .unwrap_or(Lookup::Unavailable);
        }

        // The child releases the refresh slot only after it has written the
        // cache, so the marker vanishing is the completion signal. This also
        // covers a refresh another prompt started moments ago.
        let marker = marker_path(path);
        let deadline = Instant::now() + timeout;
        loop {
            if !marker.exists() {
                return self.cached_or(Lookup::Unavailable);
            }
            let now = Instant::now();
            if now >= deadline {
                return self.cached_or(Lookup::Loading);
            }
            thread::sleep(REFRESH_POLL_INTERVAL.min(deadline - now));
        }
    }

    fn cached_or(&self, fallback: Lookup<S::Value>) -> Lookup<S::Value> {
        self.read()
            .map(|entry| Lookup::Ready(entry.value))
            .unwrap_or(fallback)
    }

    /// Starts a detached refresh unless one was started within
    /// [`Source::REFRESH_INTERVAL`]. Returns whether a refresh is now pending.
    pub fn refresh_in_background(&self) -> bool {
        let Some(path) = &self.path else {
            return false;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let marker = marker_path(path);
        if !claim_refresh(&marker, S::REFRESH_INTERVAL) {
            // Someone else holds the slot, so a refresh is already on its way.
            return true;
        }

        let Ok(source) = serde_json::to_string(&self.source) else {
            let _ = fs::remove_file(&marker);
            return false;
        };
        if spawn_child(S::KIND, &source) {
            true
        } else {
            let _ = fs::remove_file(marker);
            false
        }
    }

    /// Performs the fetch on the calling thread and stores the result. This is
    /// what the detached child runs; a failed fetch leaves the previous value
    /// in place and keeps the refresh slot claimed so it is not retried before
    /// [`Source::REFRESH_INTERVAL`] has passed.
    pub fn refresh_now(&self) -> bool {
        let Some(value) = self.source.fetch() else {
            return false;
        };
        self.write(&value);
        if let Some(path) = &self.path {
            let _ = fs::remove_file(marker_path(path));
        }
        true
    }

    fn write(&self, value: &S::Value) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Write to a temp file and rename so a concurrent reader never sees a
        // half-written cache.
        let tmp = path.with_extension("tmp");
        let Ok(mut file) = File::create(&tmp) else {
            return;
        };
        if serde_json::to_writer(&mut file, &Entry::now(value)).is_ok() && file.flush().is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }
}

/// Runs the refresh half of a lookup from its serialised parameters. Called by
/// [`crate::modules::run_refresh`] once it has matched the `KIND`.
pub fn refresh_from_json<S: Source>(source: &str) -> bool {
    let Ok(source) = serde_json::from_str::<S>(source) else {
        return false;
    };
    Cached::new(source).refresh_now()
}

fn marker_path(cache_path: &Path) -> PathBuf {
    cache_path.with_extension("refresh")
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

/// Atomically claims the refresh slot guarded by `marker`. The marker records
/// when the slot was last claimed; a claim younger than `interval` is still
/// held. Locking the marker while reading and rewriting it stops concurrent
/// prompt processes from both winning as it expires.
fn claim_refresh(marker: &Path, interval: Duration) -> bool {
    let Ok(mut file) = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(marker)
    else {
        return false;
    };
    if fs2::FileExt::try_lock_exclusive(&file).is_err() {
        return false;
    }

    let mut timestamp = String::new();
    if file.read_to_string(&mut timestamp).is_err() {
        return false;
    }
    let now = now_millis();
    if timestamp
        .trim()
        .parse::<u128>()
        .ok()
        .is_some_and(|then| now.saturating_sub(then) < interval.as_millis())
    {
        return false;
    }

    file.set_len(0).is_ok()
        && file.seek(SeekFrom::Start(0)).is_ok()
        && write!(file, "{now}").is_ok()
}

/// Re-executes this binary to run the fetch detached from the prompt. The
/// child's stdio is redirected to null so the shell's command substitution
/// does not block waiting on an inherited pipe.
#[cfg(not(test))]
fn spawn_child(kind: &str, source: &str) -> bool {
    use std::process::{Command, Stdio};

    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    Command::new(exe)
        .arg("refresh")
        .arg(kind)
        .arg(source)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(test)]
fn spawn_child(kind: &str, source: &str) -> bool {
    tests::record_spawn(kind, source)
}

/// Opportunistically remove old files from Superline's cache directory.
///
/// Failure is deliberately ignored: cache maintenance must never prevent the
/// prompt from rendering.
pub fn prune_stale() {
    let Some(cache_dir) = crate::platform::cache_dir().map(|base| base.join("superline")) else {
        return;
    };
    prune_stale_in(&cache_dir, SystemTime::now());
}

fn prune_stale_in(cache_dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return;
    };

    let marker = cache_dir.join(PRUNE_MARKER);
    if modified_age(&marker, now).is_some_and(|age| age < PRUNE_INTERVAL) {
        return;
    }

    // Touch before walking so concurrent prompt processes quickly stand down.
    // A race can cause an occasional duplicate sweep, which is harmless.
    let _ = File::create(&marker);

    for entry in entries.flatten() {
        let path = entry.path();
        if path == marker {
            continue;
        }

        let expired = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= MAX_CACHE_AGE);
        if expired {
            // Cache entries are files; leave unexpected directories alone.
            let _ = fs::remove_file(path);
        }
    }
}

fn modified_age(path: &Path, now: SystemTime) -> Option<Duration> {
    now.duration_since(fs::metadata(path).ok()?.modified().ok()?)
        .ok()
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File, FileTimes};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Barrier, Mutex};

    use super::*;

    static SPAWNED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

    /// Stands in for re-executing the binary: records the spawn, then runs the
    /// probe's refresh on a thread exactly as the child process would.
    pub(super) fn record_spawn(kind: &str, source: &str) -> bool {
        SPAWNED
            .lock()
            .unwrap()
            .push((kind.to_string(), source.to_string()));
        if kind == Probe::KIND {
            let probe: Probe = serde_json::from_str(source).unwrap();
            let dir = probe.dir.clone();
            thread::spawn(move || Cached::in_dir(probe, Some(dir)).refresh_now());
        }
        true
    }

    fn spawned_for(kind: &str) -> Vec<String> {
        SPAWNED
            .lock()
            .unwrap()
            .iter()
            .filter(|(spawned_kind, _)| spawned_kind == kind)
            .map(|(_, source)| source.clone())
            .collect()
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "superline-cache-{label}-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn age_file(path: &Path, age: Duration) {
        let file = File::create(path).unwrap();
        file.set_times(FileTimes::new().set_modified(SystemTime::now() - age))
            .unwrap();
    }

    /// A lookup whose fetch takes `delay_ms` and yields `result` (or `None`
    /// when `fails`). `kind` doubles as the test's name so recorded spawns can
    /// be told apart across tests running in parallel, and `dir` tells the
    /// fake child which cache directory to refresh into.
    #[derive(Clone, Serialize, Deserialize)]
    struct Probe {
        kind: String,
        result: String,
        delay_ms: u64,
        fails: bool,
        fetchable: bool,
        dir: PathBuf,
    }

    impl Probe {
        fn instant(kind: &str, result: &str, dir: &Path) -> Self {
            Probe {
                kind: kind.to_string(),
                result: result.to_string(),
                delay_ms: 0,
                fails: false,
                fetchable: true,
                dir: dir.to_path_buf(),
            }
        }

        fn slow(kind: &str, result: &str, dir: &Path) -> Self {
            Probe {
                delay_ms: 300,
                ..Self::instant(kind, result, dir)
            }
        }
    }

    impl Source for Probe {
        type Value = String;
        const KIND: &'static str = "probe";
        const TTL: Duration = Duration::from_secs(60);

        fn cache_id(&self) -> String {
            hash_id(&self.kind)
        }

        fn fetchable(&self) -> bool {
            self.fetchable
        }

        fn fetch(&self) -> Option<String> {
            thread::sleep(Duration::from_millis(self.delay_ms));
            (!self.fails).then(|| self.result.clone())
        }
    }

    fn write_entry(cached: &Cached<Probe>, value: &str, age: Duration) {
        let entry = Entry {
            fetched_at: now_secs() - age.as_secs(),
            value: value.to_string(),
        };
        fs::write(cached.path().unwrap(), serde_json::to_vec(&entry).unwrap()).unwrap();
    }

    fn spawned_probe_kinds(kind: &str) -> usize {
        spawned_for(Probe::KIND)
            .iter()
            .filter(|source| serde_json::from_str::<Probe>(source).unwrap().kind == kind)
            .count()
    }

    /// Waits for the fake child to finish, bounded so a broken test fails
    /// rather than hangs.
    fn wait_for_child(cached: &Cached<Probe>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while marker_path(cached.path().unwrap()).exists() {
            assert!(Instant::now() < deadline, "the fake child never finished");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn load_serves_a_fresh_value_without_refreshing() {
        let dir = unique_temp_dir("fresh");
        let cached = Cached::in_dir(
            Probe::instant("load-fresh", "unused", &dir),
            Some(dir.clone()),
        );
        write_entry(&cached, "cached", Duration::ZERO);

        assert_eq!(cached.load().ready().as_deref(), Some("cached"));
        assert_eq!(spawned_probe_kinds("load-fresh"), 0);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn load_serves_a_stale_value_and_refreshes_it_once() {
        let dir = unique_temp_dir("stale");
        let cached = Cached::in_dir(Probe::slow("load-stale", "new", &dir), Some(dir.clone()));
        write_entry(&cached, "old", Probe::TTL + Duration::from_secs(1));

        assert_eq!(cached.load().ready().as_deref(), Some("old"));
        // A second prompt inside the refresh interval must not spawn again.
        assert_eq!(cached.load().ready().as_deref(), Some("old"));
        assert_eq!(spawned_probe_kinds("load-stale"), 1);
        assert!(marker_path(cached.path().unwrap()).exists());

        wait_for_child(&cached);
        assert_eq!(cached.load().ready().as_deref(), Some("new"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn load_reports_loading_until_the_first_refresh_lands() {
        let dir = unique_temp_dir("loading");
        let cached = Cached::in_dir(
            Probe::slow("load-loading", "first", &dir),
            Some(dir.clone()),
        );

        assert!(matches!(cached.load(), Lookup::Loading));
        assert_eq!(spawned_probe_kinds("load-loading"), 1);

        wait_for_child(&cached);
        assert!(!marker_path(cached.path().unwrap()).exists());
        assert_eq!(cached.load().ready().as_deref(), Some("first"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unfetchable_sources_and_missing_cache_directories_are_unavailable() {
        let dir = unique_temp_dir("unavailable");
        let mut probe = Probe::instant("load-unfetchable", "unused", &dir);
        probe.fetchable = false;
        let cached = Cached::in_dir(probe.clone(), Some(dir.clone()));
        assert!(matches!(cached.load(), Lookup::Unavailable));
        assert!(matches!(
            cached.load_with_timeout(Duration::from_millis(50)),
            Lookup::Unavailable
        ));
        assert_eq!(spawned_probe_kinds("load-unfetchable"), 0);

        let homeless = Cached::in_dir(Probe::instant("load-homeless", "unused", &dir), None);
        assert!(matches!(homeless.load(), Lookup::Unavailable));
        assert!(matches!(
            homeless.load_with_timeout(Duration::from_millis(50)),
            Lookup::Unavailable
        ));
        assert!(!homeless.refresh_in_background());
        assert_eq!(spawned_probe_kinds("load-homeless"), 0);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_failed_refresh_keeps_the_old_value_and_holds_the_slot() {
        let dir = unique_temp_dir("failed");
        let mut probe = Probe::instant("refresh-failed", "unused", &dir);
        probe.fails = true;
        let cached = Cached::in_dir(probe, Some(dir.clone()));
        write_entry(&cached, "old", Probe::TTL + Duration::from_secs(1));

        assert!(cached.refresh_in_background());
        assert!(!cached.refresh_now());
        assert_eq!(cached.read().unwrap().value, "old");
        assert!(marker_path(cached.path().unwrap()).exists());
        assert!(!claim_refresh(
            &marker_path(cached.path().unwrap()),
            Probe::REFRESH_INTERVAL
        ));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn timeout_load_serves_a_fresh_entry_without_refreshing() {
        let dir = unique_temp_dir("timeout-fresh");
        let cached = Cached::in_dir(
            Probe::instant("timeout-fresh", "unused", &dir),
            Some(dir.clone()),
        );
        write_entry(&cached, "cached", Duration::ZERO);

        let result = cached.load_with_timeout(Duration::from_secs(5));
        assert_eq!(result.ready().as_deref(), Some("cached"));
        assert_eq!(spawned_probe_kinds("timeout-fresh"), 0);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn timeout_load_waits_for_a_quick_refresh_and_serves_it_fresh() {
        let dir = unique_temp_dir("timeout-quick");
        let cached = Cached::in_dir(
            Probe::instant("timeout-quick", "fresh", &dir),
            Some(dir.clone()),
        );
        write_entry(&cached, "stale", Probe::TTL + Duration::from_secs(1));

        let started = Instant::now();
        let result = cached.load_with_timeout(Duration::from_secs(5));
        assert_eq!(result.ready().as_deref(), Some("fresh"));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a finished refresh should be noticed promptly, not at the deadline"
        );
        assert_eq!(cached.read().unwrap().value, "fresh");
        assert!(!marker_path(cached.path().unwrap()).exists());
        assert_eq!(spawned_probe_kinds("timeout-quick"), 1);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn timeout_load_falls_back_to_the_cache_and_leaves_the_refresh_running() {
        let dir = unique_temp_dir("timeout-cached");
        let cached = Cached::in_dir(
            Probe::slow("timeout-cached", "fresh", &dir),
            Some(dir.clone()),
        );
        write_entry(&cached, "cached", Probe::TTL + Duration::from_secs(1));

        let result = cached.load_with_timeout(Duration::from_millis(10));
        assert_eq!(result.ready().as_deref(), Some("cached"));
        assert_eq!(spawned_probe_kinds("timeout-cached"), 1);

        // The next prompt finds the same refresh still running and waits on it
        // rather than starting another.
        wait_for_child(&cached);
        assert_eq!(
            cached
                .load_with_timeout(Duration::from_millis(10))
                .ready()
                .as_deref(),
            Some("fresh")
        );
        assert_eq!(spawned_probe_kinds("timeout-cached"), 1);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn timeout_load_reports_loading_while_the_first_refresh_runs() {
        let dir = unique_temp_dir("timeout-loading");
        let cached = Cached::in_dir(
            Probe::slow("timeout-loading", "fresh", &dir),
            Some(dir.clone()),
        );

        assert!(matches!(
            cached.load_with_timeout(Duration::from_millis(10)),
            Lookup::Loading
        ));
        assert_eq!(spawned_probe_kinds("timeout-loading"), 1);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn timeout_load_treats_a_failed_refresh_like_a_miss() {
        let dir = unique_temp_dir("timeout-failed");
        let mut probe = Probe::instant("timeout-failed", "unused", &dir);
        probe.fails = true;
        let cached = Cached::in_dir(probe, Some(dir.clone()));

        // The failed child keeps the slot, so the wait runs to the deadline.
        // From the prompt's side that is indistinguishable from a refresh that
        // is still running, and it will be retried once the interval passes.
        assert!(matches!(
            cached.load_with_timeout(Duration::from_millis(50)),
            Lookup::Loading
        ));
        // Anything cached before is served instead of the loading state.
        write_entry(&cached, "cached", Probe::TTL + Duration::from_secs(1));
        assert_eq!(
            cached
                .load_with_timeout(Duration::from_millis(50))
                .ready()
                .as_deref(),
            Some("cached")
        );
        assert_eq!(spawned_probe_kinds("timeout-failed"), 1);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn refresh_from_json_rejects_malformed_input() {
        assert!(!refresh_from_json::<Probe>("not json"));
    }

    #[test]
    fn refresh_claim_is_atomic_and_rate_limits_failed_attempts() {
        let dir = unique_temp_dir("claim");
        let marker = dir.join("probe.refresh");
        let interval = Duration::from_secs(60);

        let barrier = Arc::new(Barrier::new(8));
        let claims = (0..8)
            .map(|_| {
                let barrier = barrier.clone();
                let marker = marker.clone();
                thread::spawn(move || {
                    barrier.wait();
                    claim_refresh(&marker, interval)
                })
            })
            .collect::<Vec<_>>();
        let winners = claims
            .into_iter()
            .map(|claim| claim.join().expect("refresh claim thread"))
            .filter(|claimed| *claimed)
            .count();

        assert_eq!(winners, 1);
        assert!(!claim_refresh(&marker, interval));
        assert!(claim_refresh(&marker, Duration::ZERO));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn entries_report_their_age_against_a_ttl() {
        let fresh = Entry::now(());
        assert!(!fresh.is_stale(Duration::from_secs(60)));
        assert!(fresh.is_stale(Duration::ZERO));

        let old = Entry {
            fetched_at: now_secs() - 120,
            value: (),
        };
        assert!(old.is_stale(Duration::from_secs(60)));
        assert!(old.age() >= Duration::from_secs(120));
    }

    #[test]
    fn pruning_removes_old_files_and_keeps_fresh_files_and_directories() {
        let dir = unique_temp_dir("pruning");
        let stale = dir.join("git-stale.json");
        let fresh = dir.join("usage-fresh.json");
        let nested = dir.join("unexpected-directory");
        age_file(&stale, MAX_CACHE_AGE + Duration::from_secs(1));
        File::create(&fresh).unwrap();
        fs::create_dir(&nested).unwrap();

        prune_stale_in(&dir, SystemTime::now());

        assert!(!stale.exists());
        assert!(fresh.exists());
        assert!(nested.exists());
        assert!(dir.join(PRUNE_MARKER).exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_recent_sweep_marker_throttles_pruning() {
        let dir = unique_temp_dir("throttled");
        let stale = dir.join("pr-stale.json");
        age_file(&stale, MAX_CACHE_AGE + Duration::from_secs(1));
        File::create(dir.join(PRUNE_MARKER)).unwrap();

        prune_stale_in(&dir, SystemTime::now());

        assert!(stale.exists());
        fs::remove_dir_all(dir).ok();
    }
}
