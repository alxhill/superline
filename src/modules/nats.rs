use std::marker::PhantomData;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{hash_id, Cached, Lookup, Source};
use crate::colors::Color;
use crate::platform::resolve_binary;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

/// The symbol used by Starship's NATS module. Keep the trailing space in the
/// theme value so a custom theme can replace the complete marker cleanly.
const DEFAULT_NATS_ICON: &str = "✉️ ";

/// Shows the currently selected NATS CLI context.
pub struct Nats<S: NatsScheme> {
    scheme: PhantomData<S>,
}

pub trait NatsScheme: DefaultColors {
    fn nats_fg() -> Color {
        Self::default_fg()
    }

    fn nats_bg() -> Color {
        Self::default_bg()
    }

    fn nats_icon() -> &'static str {
        DEFAULT_NATS_ICON
    }
}

impl<S: NatsScheme> Default for Nats<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: NatsScheme> Nats<S> {
    pub fn new() -> Self {
        Self {
            scheme: PhantomData,
        }
    }
}

/// The selected context is read through the NATS CLI and cached because the
/// command starts a Go process and reads the user's context files. The binary
/// path is part of the source so a different installation cannot reuse a
/// result from an old one.
#[derive(Clone, Serialize, Deserialize)]
pub struct NatsLookup {
    pub binary: PathBuf,
}

impl Source for NatsLookup {
    type Value = String;
    const KIND: &'static str = "nats";
    const TTL: Duration = Duration::from_secs(60);
    const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

    fn cache_id(&self) -> String {
        hash_id(&self.binary)
    }

    fn fetchable(&self) -> bool {
        self.binary.is_file()
    }

    fn fetch(&self) -> Option<String> {
        let output = Command::new(&self.binary)
            .args(["context", "info", "--json"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        parse_context_name(&output.stdout)
    }
}

impl<S: NatsScheme> Module for Nats<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(binary) = resolve_binary("nats") else {
            return;
        };

        let Lookup::Ready(name) = Cached::new(NatsLookup { binary }).load() else {
            // A failed or not-yet-complete context lookup is omitted just as
            // Starship omits the module when the CLI returns no context.
            return;
        };

        let label = format!("{}{}", S::nats_icon(), name);
        powerline.add_segment(label, Style::simple(S::nats_fg(), S::nats_bg()));
    }
}

#[derive(Deserialize)]
struct NatsContext {
    name: Option<String>,
}

fn parse_context_name(stdout: &[u8]) -> Option<String> {
    let context = serde_json::from_slice::<NatsContext>(stdout).ok()?;
    let name = context.name?.trim().to_string();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::parse_context_name;

    #[test]
    fn reads_the_context_name_from_nats_json() {
        assert_eq!(
            parse_context_name(br#"{"name":"production","url":"nats://localhost"}"#).as_deref(),
            Some("production")
        );
    }

    #[test]
    fn trims_names_and_rejects_missing_or_empty_contexts() {
        assert_eq!(
            parse_context_name(br#"{"name":"  staging  "}"#).as_deref(),
            Some("staging")
        );
        assert_eq!(parse_context_name(br#"{"name":""}"#), None);
        assert_eq!(parse_context_name(br#"{"url":"nats://localhost"}"#), None);
        assert_eq!(parse_context_name(b"not json"), None);
    }
}
