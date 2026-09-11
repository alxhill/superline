//! Lifecycle helpers for Superline's on-disk caches.

use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Cached prompt data older than this is unlikely to be useful as a fallback.
const MAX_CACHE_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// Rendering happens frequently, so avoid walking the cache directory each time.
const PRUNE_INTERVAL: Duration = Duration::from_secs(60 * 60);
const PRUNE_MARKER: &str = ".last-pruned";

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

    use super::*;

    fn unique_temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "superline-cache-pruning-{}-{n}",
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

    #[test]
    fn pruning_removes_old_files_and_keeps_fresh_files_and_directories() {
        let dir = unique_temp_dir();
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
        let dir = unique_temp_dir();
        let stale = dir.join("pr-stale.json");
        age_file(&stale, MAX_CACHE_AGE + Duration::from_secs(1));
        File::create(dir.join(PRUNE_MARKER)).unwrap();

        prune_stale_in(&dir, SystemTime::now());

        assert!(stale.exists());
        fs::remove_dir_all(dir).ok();
    }
}
