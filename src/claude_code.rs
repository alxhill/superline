//! The session data Claude Code pipes to a status line command on stdin. See
//! <https://code.claude.com/docs/en/statusline#available-data>.
//!
//! Claude Code adds fields over time and leaves many out until they apply, so
//! everything here is optional and unknown fields are ignored.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct ClaudeCodeStatus {
    pub cwd: Option<PathBuf>,
    pub session_name: Option<String>,
    pub model: Model,
    pub workspace: Workspace,
    pub cost: Cost,
    pub context_window: Option<ContextWindow>,
    pub prompt_cache: Option<PromptCache>,
    #[serde(deserialize_with = "null_as_false")]
    pub fast_mode: bool,
    pub effort: Option<Effort>,
    pub rate_limits: Option<RateLimits>,
    pub vim: Option<Vim>,
    pub agent: Option<Agent>,
    pub pr: Option<PullRequest>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Model {
    pub id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub current_dir: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Cost {
    pub total_cost_usd: Option<f64>,
    pub total_duration_ms: Option<u64>,
    pub total_api_duration_ms: Option<u64>,
    pub total_lines_added: Option<u64>,
    pub total_lines_removed: Option<u64>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct ContextWindow {
    pub total_input_tokens: Option<u64>,
    pub context_window_size: Option<u64>,
    /// `null` early in a session.
    pub used_percentage: Option<f64>,
}

impl ContextWindow {
    /// Percent of the window in use, worked out from the token counts when
    /// Claude Code leaves `used_percentage` null.
    pub fn used_percent(&self) -> Option<f64> {
        self.used_percentage
            .or_else(|| {
                let size = self.context_window_size.filter(|size| *size > 0)?;
                Some(self.total_input_tokens? as f64 * 100.0 / size as f64)
            })
            .filter(|percent| percent.is_finite())
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct PromptCache {
    #[serde(deserialize_with = "null_as_false")]
    pub warm: bool,
    #[serde(deserialize_with = "null_as_false")]
    pub caching_observed: bool,
    /// Cache reads as a fraction (0 to 1) of all input tokens this session.
    pub hit_ratio: Option<f64>,
    pub misses: Option<u64>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Effort {
    pub level: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct RateLimits {
    pub five_hour: Option<RateLimitWindow>,
    pub seven_day: Option<RateLimitWindow>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct RateLimitWindow {
    pub used_percentage: Option<f64>,
    /// Unix epoch seconds.
    pub resets_at: Option<u64>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Vim {
    pub mode: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Agent {
    pub name: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct PullRequest {
    pub number: Option<u64>,
    pub url: Option<String>,
    /// `approved`, `pending`, `changes_requested` or `draft`.
    pub review_state: Option<String>,
}

impl PullRequest {
    pub fn is_draft(&self) -> bool {
        self.review_state.as_deref() == Some("draft")
    }
}

fn null_as_false<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<bool>::deserialize(deserializer)?.unwrap_or(false))
}

impl ClaudeCodeStatus {
    pub fn parse(text: &str) -> Result<ClaudeCodeStatus, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The session's working directory, which the git, cwd and other
    /// directory-based widgets should look at.
    pub fn current_dir(&self) -> Option<&Path> {
        self.workspace
            .current_dir
            .as_deref()
            .or(self.cwd.as_deref())
            .filter(|dir| !dir.as_os_str().is_empty())
    }

    /// Data for every widget, for previewing a layout outside Claude Code.
    pub fn sample() -> ClaudeCodeStatus {
        Self::parse(SAMPLE).expect("the sample status parses")
    }
}

const SAMPLE: &str = r#"{
  "session_name": "claude-code-statusline",
  "model": { "id": "claude-opus-5-5", "display_name": "Opus 5.5" },
  "cost": {
    "total_cost_usd": 3.4721,
    "total_duration_ms": 2843000,
    "total_api_duration_ms": 912000,
    "total_lines_added": 412,
    "total_lines_removed": 87
  },
  "context_window": {
    "total_input_tokens": 92400,
    "total_output_tokens": 2100,
    "context_window_size": 200000,
    "used_percentage": 46.2
  },
  "prompt_cache": { "warm": true, "caching_observed": true, "hit_ratio": 0.91, "misses": 1 },
  "fast_mode": false,
  "effort": { "level": "high" },
  "rate_limits": {
    "five_hour": { "used_percentage": 23.5 },
    "seven_day": { "used_percentage": 41.2 }
  },
  "vim": { "mode": "INSERT" },
  "agent": { "name": "reviewer" }
}"#;

/// The command `superline install claude-code` sets as the status line.
pub const STATUS_LINE_COMMAND: &str = "superline claude-code";

/// What [`install_status_line`] did to a Claude Code settings file.
#[derive(Debug, PartialEq, Eq)]
pub enum Installed {
    Added,
    /// The settings already run superline.
    AlreadyPresent,
    /// Another status line was replaced.
    Replaced(String),
}

/// Points the `statusLine` of Claude Code's settings (`~/.claude/settings.json`)
/// at superline. Another command is only replaced with `force`, and errors
/// name it.
pub fn install_status_line(settings: &mut Value, force: bool) -> Result<Installed, String> {
    let settings = settings
        .as_object_mut()
        .ok_or("the settings file is not a JSON object")?;
    let existing = settings.get("statusLine").map(|line| {
        line.get("command")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| line.to_string())
    });
    let outcome = match existing {
        None => Installed::Added,
        Some(command) if command.contains(STATUS_LINE_COMMAND) => {
            return Ok(Installed::AlreadyPresent)
        }
        Some(command) if force => Installed::Replaced(command),
        Some(command) => {
            return Err(format!(
                "a status line is already set (`{command}`); rerun with --force to replace it"
            ))
        }
    };
    settings.insert(
        "statusLine".to_string(),
        json!({ "type": "command", "command": STATUS_LINE_COMMAND, "padding": 0 }),
    );
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example from the Claude Code status line docs.
    const DOCS_EXAMPLE: &str = r#"{
        "cwd": "/current/working/directory",
        "session_id": "abc123...",
        "session_name": "my-session",
        "prompt_id": "550e8400-e29b-41d4-a716-446655440000",
        "transcript_path": "/path/to/transcript.jsonl",
        "model": { "id": "claude-opus-5-5", "display_name": "Opus" },
        "workspace": {
            "current_dir": "/current/working/directory",
            "project_dir": "/original/project/directory",
            "added_dirs": [],
            "git_worktree": "feature-xyz",
            "repo": { "host": "github.com", "owner": "anthropics", "name": "claude-code" }
        },
        "version": "2.1.90",
        "output_style": { "name": "default" },
        "cost": {
            "total_cost_usd": 0.01234,
            "total_duration_ms": 45000,
            "total_api_duration_ms": 2300,
            "total_lines_added": 156,
            "total_lines_removed": 23
        },
        "context_window": {
            "total_input_tokens": 15500,
            "total_output_tokens": 1200,
            "context_window_size": 200000,
            "used_percentage": 8,
            "remaining_percentage": 92,
            "current_usage": {
                "input_tokens": 8500,
                "output_tokens": 1200,
                "cache_creation_input_tokens": 5000,
                "cache_read_input_tokens": 2000
            }
        },
        "exceeds_200k_tokens": false,
        "prompt_cache": {
            "warm": true,
            "caching_observed": true,
            "ttl": "1h",
            "expires_at": 1738429200,
            "requests": 14,
            "misses": 2,
            "hit_ratio": 0.91,
            "last_miss_cause": { "causes": ["tools_changed"], "tools_added": 2 },
            "recache_tokens_if_cold": 45000
        },
        "fast_mode": false,
        "effort": { "level": "high" },
        "thinking": { "enabled": true },
        "rate_limits": {
            "five_hour": { "used_percentage": 23.5, "resets_at": 1738425600 },
            "seven_day": { "used_percentage": 41.2, "resets_at": 1738857600 },
            "spend_limit": { "used_percentage": 62.8, "resets_at": 1740787200 }
        },
        "vim": { "mode": "NORMAL" },
        "agent": { "name": "security-reviewer" },
        "pr": {
            "number": 1234,
            "url": "https://github.com/anthropics/claude-code/pull/1234",
            "review_state": "pending"
        },
        "worktree": {
            "name": "my-feature",
            "path": "/path/to/.claude/worktrees/my-feature",
            "branch": "worktree-my-feature",
            "original_cwd": "/path/to/project",
            "original_branch": "main"
        }
    }"#;

    #[test]
    fn parses_the_documented_example() {
        let status = ClaudeCodeStatus::parse(DOCS_EXAMPLE).unwrap();
        assert_eq!(status.model.display_name.as_deref(), Some("Opus"));
        assert_eq!(
            status.current_dir(),
            Some(Path::new("/current/working/directory"))
        );
        assert_eq!(status.cost.total_lines_added, Some(156));
        assert_eq!(status.context_window.unwrap().used_percent(), Some(8.0));
        assert_eq!(status.effort.unwrap().level.as_deref(), Some("high"));
        let five_hour = status.rate_limits.unwrap().five_hour.unwrap();
        assert_eq!(five_hour.used_percentage, Some(23.5));
        assert_eq!(five_hour.resets_at, Some(1738425600));
        assert_eq!(status.vim.unwrap().mode.as_deref(), Some("NORMAL"));
        assert_eq!(status.pr.unwrap().number, Some(1234));
        assert_eq!(status.prompt_cache.unwrap().hit_ratio, Some(0.91));
    }

    #[test]
    fn a_minimal_status_leaves_everything_unset() {
        let status = ClaudeCodeStatus::parse("{}").unwrap();
        assert!(status.model.display_name.is_none());
        assert!(status.context_window.is_none());
        assert!(status.rate_limits.is_none());
        assert!(status.current_dir().is_none());
        assert!(!status.fast_mode);
    }

    #[test]
    fn nulls_early_in_a_session_parse() {
        let status = ClaudeCodeStatus::parse(
            r#"{
                "fast_mode": null,
                "context_window": {
                    "total_input_tokens": 0,
                    "context_window_size": 200000,
                    "used_percentage": null,
                    "current_usage": null
                },
                "prompt_cache": { "warm": false, "hit_ratio": null, "expires_at": null }
            }"#,
        )
        .unwrap();
        assert_eq!(status.context_window.unwrap().used_percent(), Some(0.0));
        assert!(status.prompt_cache.unwrap().hit_ratio.is_none());
    }

    #[test]
    fn context_percent_falls_back_to_the_token_counts() {
        let window = ContextWindow {
            total_input_tokens: Some(50_000),
            context_window_size: Some(200_000),
            ..Default::default()
        };
        assert_eq!(window.used_percent(), Some(25.0));
        let empty = ContextWindow {
            context_window_size: Some(0),
            total_input_tokens: Some(1),
            ..Default::default()
        };
        assert_eq!(empty.used_percent(), None);
    }

    #[test]
    fn the_cwd_field_stands_in_for_the_workspace() {
        let status = ClaudeCodeStatus::parse(r#"{ "cwd": "/a" }"#).unwrap();
        assert_eq!(status.current_dir(), Some(Path::new("/a")));
        let status = ClaudeCodeStatus::parse(r#"{ "cwd": "" }"#).unwrap();
        assert_eq!(status.current_dir(), None);
    }

    #[test]
    fn the_sample_fills_every_widget() {
        let status = ClaudeCodeStatus::sample();
        assert!(status.model.display_name.is_some());
        assert!(status.context_window.is_some());
        assert!(status.prompt_cache.is_some());
        assert!(status.vim.is_some());
        assert!(status.agent.is_some());
        assert!(status.session_name.is_some());
        assert!(status.rate_limits.is_some());
    }

    #[test]
    fn installs_into_settings_without_a_status_line() {
        let mut settings = json!({ "model": "opus", "theme": "dark" });
        assert_eq!(
            install_status_line(&mut settings, false),
            Ok(Installed::Added)
        );
        assert_eq!(
            settings["statusLine"],
            json!({ "type": "command", "command": "superline claude-code", "padding": 0 })
        );
        let keys: Vec<_> = settings.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["model", "theme", "statusLine"]);
    }

    #[test]
    fn leaves_another_status_line_alone_unless_forced() {
        let other = json!({ "statusLine": { "type": "command", "command": "~/bin/line.sh" } });
        let mut settings = other.clone();
        let error = install_status_line(&mut settings, false).unwrap_err();
        assert!(error.contains("~/bin/line.sh"), "{error}");
        assert_eq!(settings, other);

        assert_eq!(
            install_status_line(&mut settings, true),
            Ok(Installed::Replaced("~/bin/line.sh".to_string()))
        );
        assert_eq!(settings["statusLine"]["command"], "superline claude-code");
    }

    #[test]
    fn an_existing_superline_status_line_is_kept() {
        let mut settings = json!({
            "statusLine": { "type": "command", "command": "superline claude-code --margin 6" }
        });
        let before = settings.clone();
        assert_eq!(
            install_status_line(&mut settings, true),
            Ok(Installed::AlreadyPresent)
        );
        assert_eq!(settings, before);
    }

    #[test]
    fn settings_must_be_an_object() {
        assert!(install_status_line(&mut json!([]), false).is_err());
    }
}
