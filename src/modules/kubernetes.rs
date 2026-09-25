use std::env;
use std::fs;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{hash_id, Cached, Lookup, Source};
use crate::colors::Color;
use crate::platform;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

/// How long a prompt waits for a stale kubeconfig to be re-read before
/// serving the cached context. A local kubeconfig parses well within this, so
/// a `kubectl config use-context` shows on the very next prompt; a slow or
/// remote-mounted one falls back to the cache and finishes in the background.
const LOOKUP_TIMEOUT: Duration = Duration::from_millis(50);

/// Shows the current Kubernetes context and namespace from the active
/// kubeconfig, read through [`KubernetesLookup`].
pub struct Kubernetes<S: KubernetesScheme> {
    scheme: PhantomData<S>,
}

/// Colours and the icon used by the Kubernetes segment.
pub trait KubernetesScheme: DefaultColors {
    fn kubernetes_fg() -> Color {
        Self::default_fg()
    }

    fn kubernetes_bg() -> Color {
        Self::default_bg()
    }

    fn kubernetes_icon() -> &'static str {
        "\u{f10fe}" // nf-md-kubernetes
    }
}

impl<S: KubernetesScheme> Default for Kubernetes<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: KubernetesScheme> Kubernetes<S> {
    pub fn new() -> Self {
        Self {
            scheme: PhantomData,
        }
    }
}

/// The part of a kubeconfig context that is useful in a prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KubernetesContext {
    pub name: String,
    pub namespace: Option<String>,
}

/// The current context read from one or more kubeconfig files.
///
/// `Value` is an option deliberately: a readable kubeconfig with no current
/// context is a successful lookup, and should be cached as an empty result
/// rather than retried on every prompt.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KubernetesLookup {
    pub paths: Vec<PathBuf>,
}

impl Source for KubernetesLookup {
    type Value = Option<KubernetesContext>;

    const KIND: &'static str = "kubernetes";

    // Context switches should become visible quickly, while prompts rendered
    // in a tight loop still reuse the parsed kubeconfig for a moment.
    const TTL: Duration = Duration::from_secs(1);
    const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

    fn cache_id(&self) -> String {
        hash_id(&self.paths)
    }

    fn fetchable(&self) -> bool {
        self.paths.iter().any(|path| path.is_file())
    }

    fn fetch(&self) -> Option<Self::Value> {
        Some(read_current_context(&self.paths))
    }
}

#[derive(Debug, Deserialize)]
struct KubeConfig {
    #[serde(rename = "current-context")]
    current_context: Option<String>,
    #[serde(default)]
    contexts: Vec<NamedContext>,
}

#[derive(Debug, Deserialize)]
struct NamedContext {
    name: String,
    #[serde(default)]
    context: ContextDetails,
}

#[derive(Debug, Default, Deserialize)]
struct ContextDetails {
    namespace: Option<String>,
}

impl<S: KubernetesScheme> Module for Kubernetes<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        let paths = kubeconfig_paths();
        if paths.is_empty() {
            return;
        }

        let Lookup::Ready(Some(context)) =
            Cached::new(KubernetesLookup { paths }).load_with_timeout(LOOKUP_TIMEOUT)
        else {
            return;
        };

        let label = format_context(S::kubernetes_icon(), &context);
        powerline.add_segment(label, Style::simple(S::kubernetes_fg(), S::kubernetes_bg()));
    }
}

/// Resolve the kubeconfig paths using the same convention as kubectl:
/// `$KUBECONFIG` is a platform-separated list, otherwise use
/// `$HOME/.kube/config`.
fn kubeconfig_paths() -> Vec<PathBuf> {
    if let Some(config) = env::var_os("KUBECONFIG") {
        let paths: Vec<_> = env::split_paths(&config)
            .filter(|path| !path.as_os_str().is_empty())
            .collect();
        if !paths.is_empty() {
            return paths;
        }
    }

    platform::home_dir()
        .map(|home| vec![home.join(".kube").join("config")])
        .unwrap_or_default()
}

/// Read and merge kubeconfig files in their listed order. Kubernetes' merge
/// rules are first-wins for the current context and named contexts, which is
/// enough for the display-only fields used here.
fn read_current_context(paths: &[PathBuf]) -> Option<KubernetesContext> {
    let mut current_context = None;
    let mut contexts = Vec::new();

    for path in paths {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(config) = serde_yaml::from_str::<KubeConfig>(&contents) else {
            continue;
        };

        if current_context.is_none() {
            current_context = config.current_context.and_then(trim_non_empty);
        }

        for mut context in config.contexts {
            let Some(name) = trim_non_empty(context.name) else {
                continue;
            };
            context.name = name;
            if !contexts
                .iter()
                .any(|existing: &NamedContext| existing.name == context.name)
            {
                contexts.push(context);
            }
        }
    }

    let name = current_context?;
    let context = contexts.into_iter().find(|context| context.name == name)?;
    let namespace = context.context.namespace.and_then(trim_non_empty);

    Some(KubernetesContext { name, namespace })
}

fn trim_non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn format_context(icon: &str, context: &KubernetesContext) -> String {
    let context = match context.namespace.as_deref() {
        Some(namespace) => format!("{} ({namespace})", context.name),
        None => context.name.clone(),
    };
    if icon.is_empty() {
        context
    } else {
        format!("{icon} {context}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("superline-kubernetes-{label}-{suffix}"))
    }

    #[test]
    fn reads_current_context_and_namespace() {
        let path = temp_path("context");
        fs::write(
            &path,
            r#"
current-context: "  staging  "
contexts:
  - name: "  staging  "
    context:
      cluster: staging-cluster
      user: staging-user
      namespace: "  payments  "
  - name: unused
    context:
      namespace: ignored
"#,
        )
        .expect("write kubeconfig");

        assert_eq!(
            read_current_context(std::slice::from_ref(&path)),
            Some(KubernetesContext {
                name: "staging".into(),
                namespace: Some("payments".into()),
            })
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn omits_empty_namespace() {
        let path = temp_path("empty-namespace");
        fs::write(
            &path,
            "current-context: local\ncontexts:\n- name: local\n  context:\n    namespace: '  '\n",
        )
        .expect("write kubeconfig");

        assert_eq!(
            read_current_context(std::slice::from_ref(&path)),
            Some(KubernetesContext {
                name: "local".into(),
                namespace: None,
            })
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn merges_multiple_files_first_context_wins() {
        let first = temp_path("merge-first");
        let second = temp_path("merge-second");
        fs::write(
            &first,
            "current-context: shared\ncontexts:\n- name: shared\n  context:\n    namespace: first\n",
        )
        .expect("write first kubeconfig");
        fs::write(
            &second,
            "current-context: second\ncontexts:\n- name: shared\n  context:\n    namespace: second\n- name: second\n  context:\n    namespace: extra\n",
        )
        .expect("write second kubeconfig");

        assert_eq!(
            read_current_context(&[first.clone(), second.clone()]),
            Some(KubernetesContext {
                name: "shared".into(),
                namespace: Some("first".into()),
            })
        );
        let _ = fs::remove_file(first);
        let _ = fs::remove_file(second);
    }

    #[test]
    fn ignores_missing_and_invalid_files() {
        let missing = temp_path("missing");
        let invalid = temp_path("invalid");
        fs::write(&invalid, "this: [is: not valid yaml").expect("write invalid kubeconfig");

        assert_eq!(read_current_context(&[missing, invalid.clone()]), None);
        let _ = fs::remove_file(invalid);
    }

    #[test]
    fn formats_context_with_optional_namespace() {
        assert_eq!(
            format_context(
                "☸",
                &KubernetesContext {
                    name: "prod".into(),
                    namespace: Some("payments".into()),
                }
            ),
            "☸ prod (payments)"
        );
        assert_eq!(
            format_context(
                "☸",
                &KubernetesContext {
                    name: "local".into(),
                    namespace: None,
                }
            ),
            "☸ local"
        );
        assert_eq!(
            format_context(
                "",
                &KubernetesContext {
                    name: "local".into(),
                    namespace: Some("default".into()),
                }
            ),
            "local (default)"
        );
    }
}
