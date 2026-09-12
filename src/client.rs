//! The metadata client used by the prompt renderer.
//!
//! `fetch_metadata` asks the long-running [`crate::server`] for a snapshot of
//! the slow providers in a config. If no server is running it starts one and
//! waits briefly for it to publish its control file. Starting the server is
//! cheap (bind a socket and write a file); the slow fetching happens in its
//! background threads, so this call still returns quickly with `Pending`
//! readings for anything not fetched yet.

use std::env;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::UsageProvider;
use crate::metadata::{FetchState, Metadata, MetadataKind, MetadataRequest};
use crate::modules::usage::provider_is_installed;
use crate::server::{read_control, Control};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(50);
const READ_TIMEOUT: Duration = Duration::from_millis(500);
const SERVER_STARTUP_TIMEOUT: Duration = Duration::from_millis(500);

/// Set this to skip the daemon entirely. Useful in tests that only care about
/// rendering and don't want to spawn a background server process.
const DISABLE_SERVER_ENV: &str = "SUPERLINE_DISABLE_SERVER";

pub fn fetch_metadata(cwd: &Path, providers: &[MetadataKind]) -> Metadata {
    if providers.is_empty() {
        return Metadata::default();
    }

    if env::var_os(DISABLE_SERVER_ENV).is_some() {
        return fallback(providers);
    }

    if let Some(metadata) = query(cwd, providers) {
        return metadata;
    }

    spawn_server();

    let deadline = Instant::now() + SERVER_STARTUP_TIMEOUT;
    loop {
        if let Some(metadata) = query(cwd, providers) {
            return metadata;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread_sleep(Duration::from_millis(25));
    }

    fallback(providers)
}

fn query(cwd: &Path, providers: &[MetadataKind]) -> Option<Metadata> {
    let control: Control = read_control()?;
    let mut stream = TcpStream::connect_timeout(&control.addr(), CONNECT_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(READ_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(READ_TIMEOUT)).ok()?;

    let request = MetadataRequest {
        cwd: cwd.to_path_buf(),
        providers: providers.to_vec(),
    };

    serde_json::to_writer(&mut stream, &request).ok()?;
    stream.write_all(b"\n").ok()?;
    stream.flush().ok()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    if line.trim().is_empty() {
        return None;
    }

    serde_json::from_str::<Metadata>(line.trim()).ok()
}

fn spawn_server() {
    let Ok(exe) = env::current_exe() else {
        return;
    };

    let _ = Command::new(exe)
        .arg("server")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Best-effort snapshot used when the server can't be reached. Slow readings
/// default to `Pending`; a usage provider that isn't on `PATH` is reported as
/// `Unavailable` so it renders the "not installed" marker rather than a
/// perpetual loading state.
fn fallback(providers: &[MetadataKind]) -> Metadata {
    let mut metadata = Metadata::default();

    for kind in providers {
        if let MetadataKind::Usage(provider) = kind {
            if !provider_is_installed(*provider) {
                let state = FetchState::Unavailable;
                match provider {
                    UsageProvider::Claude => metadata.usage_claude = state,
                    UsageProvider::Codex => metadata.usage_codex = state,
                }
            }
        }
    }

    metadata
}

// Small wrapper so the module reads cleanly and can be swapped for a test clock
// if needed.
fn thread_sleep(duration: Duration) {
    std::thread::sleep(duration);
}
