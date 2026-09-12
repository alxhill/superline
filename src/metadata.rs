//! Shared types for the client/server metadata protocol.
//!
//! The prompt renderer (the *client*, `superline show`) asks a long-running
//! `superline server` process for the data that is slow to fetch - git status,
//! the current branch's PR, and Claude/Codex usage. The server does the
//! fetching in background threads and keeps the latest reading in memory, so
//! the client never blocks on git or network calls and never maintains its own
//! per-widget cache files.
//!
//! The wire format is one newline-delimited JSON object per request/response,
//! over a loopback TCP socket whose port is published in a small control file.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::{Config, LineSegment, UsageProvider};
use crate::modules::git::GitStats;
use crate::modules::pr::PrInfo;
use crate::modules::usage::UsageStats;

/// Which slow provider the client wants the server to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MetadataKind {
    Git,
    Pr,
    Usage(UsageProvider),
}

/// A request from the client to the server. `cwd` is the shell's current
/// directory at render time; the server derives the git repository (and thus
/// the branch/PR) from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataRequest {
    pub cwd: PathBuf,
    pub providers: Vec<MetadataKind>,
}

/// The state of a single provider reading.
///
/// * `Pending` - the server has been asked but has no reading yet (a fetch is
///   in flight, or the server has not started one).
/// * `Ready` - the latest reading.
/// * `Unavailable` - the reading cannot exist in this context: not inside a
///   git repository, on a branch that never has a PR, or the provider CLI is
///   not installed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum FetchState<T> {
    #[default]
    Pending,
    Ready(T),
    Unavailable,
}

/// The complete snapshot the server hands back to the client.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Metadata {
    pub git: FetchState<GitStats>,
    /// `Ready(None)` means "looked up, but this branch has no PR".
    pub pr: FetchState<Option<PrInfo>>,
    pub usage_claude: FetchState<UsageStats>,
    pub usage_codex: FetchState<UsageStats>,
}

impl Metadata {
    pub fn usage(&self, provider: UsageProvider) -> &FetchState<UsageStats> {
        match provider {
            UsageProvider::Claude => &self.usage_claude,
            UsageProvider::Codex => &self.usage_codex,
        }
    }
}

impl Config {
    /// The slow providers referenced by any widget in this config.
    pub fn metadata_kinds(&self) -> Vec<MetadataKind> {
        let mut kinds = Vec::new();

        for row in &self.rows {
            let mut visit = |segments: &[LineSegment]| {
                for segment in segments {
                    match segment {
                        LineSegment::Git { .. } => push_unique(&mut kinds, MetadataKind::Git),
                        LineSegment::Pr { .. } => push_unique(&mut kinds, MetadataKind::Pr),
                        LineSegment::AiUsage { provider, .. } => {
                            push_unique(&mut kinds, MetadataKind::Usage(*provider));
                        }
                        _ => {}
                    }
                }
            };

            visit(&row.left);
            if let Some(right) = &row.right {
                visit(right);
            }
        }

        kinds
    }
}

fn push_unique(kinds: &mut Vec<MetadataKind>, kind: MetadataKind) {
    if !kinds.contains(&kind) {
        kinds.push(kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_kinds_collects_only_slow_widgets_once() {
        let conf: Config = serde_json::from_str(
            r#"{
                "theme": "rainbow",
                "rows": [{
                    "left": [
                        "read_only",
                        "git",
                        {"pr":{"status":true}},
                        {"ai_usage":{"provider":"claude"}},
                        {"ai_usage":{"provider":"codex"}},
                        {"ai_usage":{"provider":"claude","weekly":false}},
                        "shell"
                    ]
                }]
            }"#,
        )
        .expect("config should parse");

        assert_eq!(
            conf.metadata_kinds(),
            vec![
                MetadataKind::Git,
                MetadataKind::Pr,
                MetadataKind::Usage(UsageProvider::Claude),
                MetadataKind::Usage(UsageProvider::Codex),
            ]
        );
    }

    #[test]
    fn fetch_state_serialises_in_the_expected_wire_shape() {
        let metadata = Metadata {
            git: FetchState::Pending,
            pr: FetchState::Ready(None),
            usage_claude: FetchState::Unavailable,
            usage_codex: FetchState::Pending,
        };

        let json = serde_json::to_value(&metadata).expect("metadata should serialise");
        assert_eq!(json["git"], "Pending");
        assert_eq!(json["pr"], serde_json::json!({ "Ready": null }));
        assert_eq!(json["usage_claude"], "Unavailable");
        assert_eq!(json["usage_codex"], "Pending");

        let round_tripped: Metadata =
            serde_json::from_value(json).expect("metadata should deserialise");
        assert_eq!(round_tripped, metadata);
    }
}
