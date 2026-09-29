use std::env;
use std::fs::File;
use std::io::read_to_string;
use std::marker::PhantomData;
use std::path::PathBuf;

use crate::config::SegmentPadding;
use crate::mise;
use crate::modules::{DefaultPadding, Module};
use crate::themes::DefaultColors;
use crate::{Color, Powerline, Style};

pub struct Java<S> {
    /// Whether to show the major version after the icon.
    show_version: bool,
    /// Whether to show the JDK distribution (corretto, Temurin, ...).
    show_jdk: bool,
    scheme: PhantomData<S>,
}

pub trait JavaScheme: DefaultColors {
    const JAVA_ICON: &'static str = "\u{f0176}";

    fn java_fg() -> Color {
        Self::default_fg()
    }

    fn java_bg() -> Color {
        Self::default_fg()
    }

    fn icon() -> &'static str {
        Self::JAVA_ICON
    }

    /// Marks a version that came from a mise config rather than `.sdkmanrc`.
    fn mise_icon() -> &'static str {
        crate::mise::DEFAULT_ICON
    }
}

impl<S: JavaScheme> Default for Java<S> {
    fn default() -> Self {
        Self::new(true, true)
    }
}
impl<S: JavaScheme> Java<S> {
    pub fn new(show_version: bool, show_jdk: bool) -> Java<S> {
        Java {
            show_version,
            show_jdk,
            scheme: PhantomData,
        }
    }

    fn label(&self, java: &JavaVersion) -> String {
        java_label(
            if java.from_mise { S::mise_icon() } else { "" },
            S::icon(),
            self.show_version.then_some(java.version.as_str()),
            self.show_jdk
                .then(|| distro_name(&java.distribution))
                .as_deref(),
        )
    }
}

/// The JDK in effect for the current directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaVersion {
    /// The major version, e.g. `21`.
    version: String,
    /// The distribution as its version manager names it, e.g. `tem`.
    distribution: String,
    /// Whether the version came from a mise config rather than `.sdkmanrc`.
    from_mise: bool,
}

impl<S: JavaScheme> Module for Java<S> {
    /// The JDK for the current directory, if a version manager pins one.
    type Data = Option<JavaVersion>;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn fetch(&self) -> Option<JavaVersion> {
        // mise wins over sdkman: when a mise config declares java it is the tool
        // actually putting a JDK on the path, even in a repo that also keeps a
        // `.sdkmanrc` around.
        mise::tool_version("java")
            .and_then(mise_java)
            .map(|java| (java, true))
            .or_else(|| sdkman_java().map(|java| (java, false)))
            .map(|((version, distribution), from_mise)| JavaVersion {
                version,
                distribution,
                from_mise,
            })
    }

    fn sample(&self) -> Option<JavaVersion> {
        Some(JavaVersion {
            version: "21".to_string(),
            distribution: "tem".to_string(),
            from_mise: false,
        })
    }

    fn render(&self, java: Option<JavaVersion>, powerline: &mut Powerline) {
        if let Some(java) = java {
            let label = self.label(&java);
            if !label.is_empty() {
                powerline.add_segment(label, Style::simple(S::java_fg(), S::java_bg()));
            }
        }
    }
}

/// Joins the parts of the segment that are enabled and non-empty, so the icon
/// stands alone when both the version and the distribution are hidden.
fn java_label(
    source_icon: &str,
    icon: &str,
    version: Option<&str>,
    distribution: Option<&str>,
) -> String {
    [Some(source_icon), Some(icon), version, distribution]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Reads the `java=<version>-<distribution>` line of the `.sdkmanrc` that
/// sdkman's auto-env support exported for the current directory.
fn sdkman_java() -> Option<(String, String)> {
    let sdkmanrc = env::var("SDKMAN_ENV")
        .ok()
        .map(|path| PathBuf::from(path).join(".sdkmanrc"))
        .and_then(|rc_path| File::open(rc_path).ok())
        .and_then(|f| read_to_string(f).ok())?;

    let java_version = sdkmanrc
        .lines()
        .filter(|line| !line.starts_with("#"))
        .find_map(|line| line.strip_prefix("java="))?;

    let (version, distribution) = java_version.split_once("-")?;

    Some((major_version(version), distribution.to_string()))
}

/// Splits a mise java version such as `corretto-26.0.2.10.1` or
/// `graalvm-community-25.0.2` into its version and distribution. mise orders
/// these the other way round to sdkman, and the distribution may be omitted
/// entirely (`java = "21.0.5"`).
fn mise_java(version: &str) -> Option<(String, String)> {
    let split_at = version
        .split('-')
        .take_while(|part| !part.starts_with(|c: char| c.is_ascii_digit()))
        .count();

    let parts: Vec<&str> = version.split('-').collect();
    let (distribution, version) = parts.split_at(split_at);

    let version = version.join("-");
    if version.is_empty() {
        return None;
    }

    Some((major_version(&version), distribution.join("-")))
}

fn major_version(version: &str) -> String {
    match version.split_once(".") {
        Some((major, _)) => major.to_string(),
        None => version.to_string(),
    }
}

fn distro_name(distribution: &str) -> String {
    match distribution {
        "amzn" | "corretto" => "corretto".to_string(),
        "graal" | "graalce" | "graalvm" | "graalvm-community" | "oracle-graalvm" => {
            "GraalVM".to_string()
        }
        "open" | "openjdk" => "OpenJDK".to_string(),
        "zulu" => "Zulu".to_string(),
        "tem" | "temurin" => "Temurin".to_string(),
        "librca" | "liberica" => "Liberica".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colors::{black, white};

    struct TestTheme;

    impl DefaultColors for TestTheme {
        fn default_bg() -> Color {
            black()
        }

        fn default_fg() -> Color {
            white()
        }
    }

    impl JavaScheme for TestTheme {}

    #[test]
    fn sample_names_the_version_and_distribution() {
        let java = Java::<TestTheme>::default();
        let sample = java.sample().expect("the sample has a JDK");
        assert_eq!(java.label(&sample), "\u{f0176} 21 Temurin");
    }

    #[test]
    fn splits_mise_versions_into_version_and_distribution() {
        assert_eq!(
            mise_java("corretto-26.0.2.10.1"),
            Some(("26".to_string(), "corretto".to_string()))
        );
        assert_eq!(
            mise_java("graalvm-community-25.0.2"),
            Some(("25".to_string(), "graalvm-community".to_string()))
        );
        assert_eq!(mise_java("21.0.5"), Some(("21".to_string(), String::new())));
        assert_eq!(
            mise_java("temurin-21"),
            Some(("21".to_string(), "temurin".to_string()))
        );
    }

    #[test]
    fn ignores_mise_versions_without_a_number() {
        assert_eq!(mise_java("latest"), None);
    }

    #[test]
    fn label_drops_the_parts_that_are_turned_off() {
        assert_eq!(
            java_label("M", "J", Some("21"), Some("Temurin")),
            "M J 21 Temurin"
        );
        assert_eq!(java_label("", "J", Some("21"), None), "J 21");
        assert_eq!(java_label("", "J", None, Some("Temurin")), "J Temurin");
        assert_eq!(java_label("M", "J", None, None), "M J");
        // A mise version with no distribution never leaves a trailing space.
        assert_eq!(java_label("", "J", Some("21"), Some("")), "J 21");
    }

    #[test]
    fn names_the_distributions_both_managers_use() {
        assert_eq!(distro_name("amzn"), "corretto");
        assert_eq!(distro_name("corretto"), "corretto");
        assert_eq!(distro_name("graalvm-community"), "GraalVM");
        assert_eq!(distro_name("sapmachine"), "sapmachine");
        assert_eq!(distro_name(""), "");
    }
}
