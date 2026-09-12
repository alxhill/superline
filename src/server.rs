//! The long-running metadata server.
//!
//! `superline server` binds a loopback TCP socket, writes its port to a small
//! control file, and answers one-line JSON [`MetadataRequest`]s with the latest
//! in-memory [`Metadata`]. Slow work (git status, `gh` PR lookups, Claude/Codex
//! usage) happens in background threads; a request always returns immediately
//! with whatever reading is available, so the prompt never blocks.
//!
//! This replaces the previous per-widget on-disk caches and the detached
//! `refresh-*` subprocesses. The only file on disk now is the control file that
//! tells clients where the server is listening.

use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::UsageProvider;
use crate::metadata::{FetchState, Metadata, MetadataKind, MetadataRequest};
use crate::modules::git::{fetch_git, find_git_dir_from, GitStats};
use crate::modules::pr::{current_branch_and_root_from, fetch_pr, PrInfo, SKIP_BRANCHES};
use crate::modules::usage::{fetch_usage, provider_is_installed, UsageStats};

const CONTROL_FILE: &str = "server.json";

/// How long a fresh reading is reused before a background refresh is started.
const GIT_TTL: Duration = Duration::from_secs(20);
const PR_TTL: Duration = Duration::from_secs(60);
const USAGE_TTL: Duration = Duration::from_secs(60);

/// Test/debug hook: when set, the server serves this JSON snapshot verbatim
/// instead of doing any real fetching.
const FIXTURE_ENV: &str = "SUPERLINE_METADATA_FIXTURE";

#[derive(Serialize, Deserialize)]
pub struct Control {
    pub port: u16,
}

impl Control {
    pub fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.port))
    }
}

pub fn control_path() -> Option<PathBuf> {
    Some(
        crate::platform::cache_dir()?
            .join("superline")
            .join(CONTROL_FILE),
    )
}

pub fn read_control() -> Option<Control> {
    let path = control_path()?;
    let file = File::open(path).ok()?;
    serde_json::from_reader(file).ok()
}

fn write_control(port: u16) {
    let Some(path) = control_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let tmp = path.with_extension("tmp");
    if let Ok(mut file) = File::create(&tmp) {
        if serde_json::to_writer(&mut file, &Control { port }).is_ok() && file.flush().is_ok() {
            let _ = fs::rename(tmp, path);
        }
    }
}

/// Entry point for `superline server`.
pub fn run_server() {
    let Some(control_path) = control_path() else {
        eprintln!("Could not determine the cache directory");
        process::exit(1);
    };

    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("Could not bind the metadata server: {error}");
            process::exit(1);
        }
    };

    let port = listener
        .local_addr()
        .map(|addr| addr.port())
        .unwrap_or_default();
    write_control(port);

    let fixture = load_fixture();
    let store = Arc::new(Mutex::new(Store::default()));

    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        let store = Arc::clone(&store);
        let fixture = fixture.clone();
        let control_path = control_path.clone();
        thread::spawn(move || {
            if handle(stream, store, fixture.as_ref()) {
                // A client asked us to shut down. Remove the control file so a
                // later prompt starts a fresh server.
                let _ = fs::remove_file(&control_path);
                process::exit(0);
            }
        });
    }
}

fn load_fixture() -> Option<Metadata> {
    let path = env::var_os(FIXTURE_ENV)?;
    let file = File::open(path).ok()?;
    serde_json::from_reader(file).ok()
}

/// Handles a single client connection. Returns `true` when the client asked the
/// server to shut down.
fn handle(stream: TcpStream, store: Arc<Mutex<Store>>, fixture: Option<&Metadata>) -> bool {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));

    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(reader) => reader,
        Err(_) => return false,
    });

    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
        return false;
    }

    let mut writer = stream;
    if line.trim() == "shutdown" {
        let _ = writer.write_all(b"ok\n");
        let _ = writer.flush();
        return true;
    }

    let Ok(request) = serde_json::from_str::<MetadataRequest>(line.trim()) else {
        return false;
    };

    let metadata = match fixture {
        Some(metadata) => metadata.clone(),
        None => store.lock().unwrap().snapshot(&request),
    };

    let mut response = Vec::new();
    if serde_json::to_writer(&mut response, &metadata).is_ok() {
        response.push(b'\n');
        let _ = writer.write_all(&response);
        let _ = writer.flush();
    }

    false
}

type SharedTracker<T> = Arc<Mutex<Tracker<T>>>;

struct Tracker<T> {
    state: FetchState<T>,
    fetched_at: Option<Instant>,
    in_flight: bool,
}

impl<T> Default for Tracker<T> {
    fn default() -> Self {
        Tracker {
            state: FetchState::Pending,
            fetched_at: None,
            in_flight: false,
        }
    }
}

#[derive(Default)]
struct Store {
    git: HashMap<PathBuf, SharedTracker<GitStats>>,
    pr: HashMap<(PathBuf, String), SharedTracker<Option<PrInfo>>>,
    usage: HashMap<UsageProvider, SharedTracker<UsageStats>>,
}

impl Store {
    fn snapshot(&mut self, request: &MetadataRequest) -> Metadata {
        let mut metadata = Metadata::default();

        for kind in &request.providers {
            match kind {
                MetadataKind::Git => metadata.git = self.git(&request.cwd),
                MetadataKind::Pr => metadata.pr = self.pr(&request.cwd),
                MetadataKind::Usage(provider) => {
                    let state = self.usage(*provider);
                    match provider {
                        UsageProvider::Claude => metadata.usage_claude = state,
                        UsageProvider::Codex => metadata.usage_codex = state,
                    }
                }
            }
        }

        metadata
    }

    fn git(&mut self, cwd: &Path) -> FetchState<GitStats> {
        let Some((repo_root, is_worktree)) = find_git_dir_from(cwd) else {
            return FetchState::Unavailable;
        };

        let tracker = self.git.entry(repo_root.clone()).or_default();
        current(tracker, GIT_TTL, move || {
            Some(fetch_git(&repo_root, is_worktree))
        })
    }

    fn pr(&mut self, cwd: &Path) -> FetchState<Option<PrInfo>> {
        let Some((branch, repo_root)) = current_branch_and_root_from(cwd) else {
            return FetchState::Unavailable;
        };
        if SKIP_BRANCHES.contains(&branch.as_str()) {
            return FetchState::Unavailable;
        }

        let key = (repo_root.clone(), branch.clone());
        let tracker = self.pr.entry(key).or_default();
        current(tracker, PR_TTL, move || Some(fetch_pr(&branch, &repo_root)))
    }

    fn usage(&mut self, provider: UsageProvider) -> FetchState<UsageStats> {
        if !provider_is_installed(provider) {
            return FetchState::Unavailable;
        }

        let tracker = self.usage.entry(provider).or_default();
        current(tracker, USAGE_TTL, move || fetch_usage(provider))
    }
}

/// Returns the current reading for `tracker`, starting a background fetch if it
/// is missing or stale. The fetch closure returns `None` on failure; a failed
/// fetch leaves the previous reading (or `Pending`) in place so it is retried
/// after the TTL rather than reported as a value.
fn current<T, F>(tracker: &SharedTracker<T>, ttl: Duration, fetch: F) -> FetchState<T>
where
    T: Clone + Send + 'static,
    F: FnOnce() -> Option<T> + Send + 'static,
{
    let mut guard = tracker.lock().unwrap();

    if let FetchState::Ready(value) = &guard.state {
        if guard.fetched_at.is_some_and(|at| at.elapsed() < ttl) {
            return FetchState::Ready(value.clone());
        }
    }

    // A fetch is already in flight, or a previous fetch (including a failed
    // one) started recently enough that we should not hammer the provider.
    // Either way, return the current reading and try again after the TTL.
    if guard.in_flight || guard.fetched_at.is_some_and(|at| at.elapsed() < ttl) {
        return guard.state.clone();
    }

    guard.in_flight = true;
    guard.fetched_at = Some(Instant::now());
    let tracker = Arc::clone(tracker);
    thread::spawn(move || {
        let fetched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(fetch))
            .ok()
            .flatten();

        let mut guard = tracker.lock().unwrap();
        if let Some(value) = fetched {
            guard.state = FetchState::Ready(value);
            guard.fetched_at = Some(Instant::now());
        }
        guard.in_flight = false;
    });

    guard.state.clone()
}
