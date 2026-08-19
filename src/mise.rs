//! Lookup of tool versions declared in [mise](https://mise.jdx.dev) config files.
//!
//! Language modules use this as a version source alongside the tool-specific
//! files they already read (`.sdkmanrc`, `.nvmrc`, `.python-version`). Configs
//! are searched from the current directory upwards, with the nearest
//! declaration of a tool winning, mirroring how mise resolves versions.
//!
//! Only the `[tools]` table is parsed, with a hand-rolled reader rather than a
//! full TOML dependency - the prompt runs on every keypress-to-newline, so the
//! parse stays proportional to the configs actually found.

use std::collections::HashMap;
use std::env;
use std::fs::read_to_string;
use std::sync::OnceLock;

use crate::platform;

/// Marker shown next to a version that a mise config is responsible for. Themes
/// can override it per module, and set it to an empty string to hide it.
pub const DEFAULT_ICON: &str = "\u{f1064}";

/// Config file names mise reads from a directory, highest priority first.
const CONFIG_FILES: [&str; 8] = [
    "mise.local.toml",
    ".mise.local.toml",
    "mise.toml",
    ".mise.toml",
    ".config/mise.toml",
    ".config/mise/config.toml",
    ".mise/config.toml",
    ".tool-versions",
];

static TOOLS: OnceLock<HashMap<String, String>> = OnceLock::new();

/// The version mise would use for `tool` in the current directory, if any.
pub fn tool_version(tool: &str) -> Option<&'static str> {
    TOOLS.get_or_init(load_tools).get(tool).map(|s| s.as_str())
}

fn load_tools() -> HashMap<String, String> {
    let mut tools = HashMap::new();

    let Ok(cwd) = env::current_dir() else {
        return tools;
    };
    let home = platform::home_dir();

    // Walk towards the root so nearer configs are seen first; `insert_if_absent`
    // then keeps the nearest declaration of each tool.
    for dir in cwd.ancestors() {
        // The global config in `$HOME` is deliberately skipped: these segments
        // report the tools a project pins, and a global `python` entry would
        // otherwise light them up in every directory.
        if home.as_deref().is_some_and(|home| dir == home) {
            break;
        }

        for file in CONFIG_FILES {
            let path = dir.join(file);
            let Ok(contents) = read_to_string(&path) else {
                continue;
            };

            if file.ends_with(".toml") {
                parse_toml_tools(&contents, &mut tools);
            } else {
                parse_tool_versions(&contents, &mut tools);
            }
        }
    }

    tools
}

fn insert_if_absent(tools: &mut HashMap<String, String>, name: &str, version: &str) {
    if name.is_empty() || version.is_empty() {
        return;
    }
    tools
        .entry(name.to_string())
        .or_insert_with(|| version.to_string());
}

/// Reads `name = version` entries from the `[tools]` table of a mise config.
fn parse_toml_tools(contents: &str, tools: &mut HashMap<String, String>) {
    let mut in_tools = false;

    for line in contents.lines() {
        let line = line.trim();

        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_tools = header.trim() == "tools";
            continue;
        }

        if !in_tools || line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((name, value)) = line.split_once('=') else {
            continue;
        };

        if let Some(version) = toml_version(value.trim()) {
            insert_if_absent(tools, unquote(name.trim()), version.trim());
        }
    }
}

/// The version out of a `[tools]` value, which may be a bare string, a list of
/// versions, or an inline table carrying other options alongside `version`.
fn toml_version(value: &str) -> Option<&str> {
    match value.chars().next()? {
        '"' | '\'' => Some(unquote(strip_comment(value))),
        // Multiple versions can be requested; the first one is the primary.
        '[' => value
            .trim_start_matches('[')
            .split(',')
            .next()
            .map(|first| unquote(first.trim().trim_end_matches(']').trim())),
        '{' => value
            .trim_start_matches('{')
            .trim_end_matches('}')
            .split(',')
            .filter_map(|entry| entry.split_once('='))
            .find(|(key, _)| unquote(key.trim()) == "version")
            .map(|(_, version)| unquote(version.trim())),
        _ => Some(strip_comment(value).trim()),
    }
}

/// Reads the asdf-style `.tool-versions` format mise also supports.
fn parse_tool_versions(contents: &str, tools: &mut HashMap<String, String>) {
    for line in contents.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();

        let mut fields = line.split_whitespace();
        if let (Some(name), Some(version)) = (fields.next(), fields.next()) {
            insert_if_absent(tools, name, version);
        }
    }
}

fn strip_comment(value: &str) -> &str {
    // Only safe once the value is known to be quoted or bare, so a `#` inside a
    // string is never treated as the start of a comment.
    match value.chars().next() {
        Some(quote @ ('"' | '\'')) => match value[1..].find(quote) {
            Some(end) => &value[..end + 2],
            None => value,
        },
        _ => value.split('#').next().unwrap_or_default(),
    }
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(contents: &str) -> HashMap<String, String> {
        let mut tools = HashMap::new();
        parse_toml_tools(contents, &mut tools);
        tools
    }

    #[test]
    fn reads_tools_table_entries() {
        let tools = parse(
            r#"
[tools]
java = "corretto-26.0.2.10.1"
python = '3.13.3'
node = 22
"#,
        );

        assert_eq!(tools.get("java").unwrap(), "corretto-26.0.2.10.1");
        assert_eq!(tools.get("python").unwrap(), "3.13.3");
        assert_eq!(tools.get("node").unwrap(), "22");
    }

    #[test]
    fn ignores_entries_outside_the_tools_table() {
        let tools = parse(
            r#"
[tools]
java = "21"

[env]
java = "not-a-tool"

[tasks.build]
run = "gradle build"
"#,
        );

        assert_eq!(tools.get("java").unwrap(), "21");
        assert_eq!(tools.len(), 1);
    }

    #[test]
    fn reads_list_and_inline_table_values() {
        let tools = parse(
            r#"
[tools]
python = ["3.13.3", "3.12.8"]
node = { version = "22.14.0", postinstall = "corepack enable" }
"#,
        );

        assert_eq!(tools.get("python").unwrap(), "3.13.3");
        assert_eq!(tools.get("node").unwrap(), "22.14.0");
    }

    #[test]
    fn keeps_quoted_and_commented_values_intact() {
        let tools = parse(
            r#"
[tools]
protoc = "29.6"  # differs from CI on purpose
"github:withered-magic/starpls" = "0.1.22"
# gh = "latest"
"#,
        );

        assert_eq!(tools.get("protoc").unwrap(), "29.6");
        assert_eq!(
            tools.get("github:withered-magic/starpls").unwrap(),
            "0.1.22"
        );
        assert!(!tools.contains_key("gh"));
    }

    #[test]
    fn nearest_declaration_wins() {
        let mut tools = HashMap::new();
        parse_toml_tools("[tools]\njava = \"21\"\n", &mut tools);
        parse_toml_tools("[tools]\njava = \"17\"\nnode = \"22\"\n", &mut tools);

        assert_eq!(tools.get("java").unwrap(), "21");
        assert_eq!(tools.get("node").unwrap(), "22");
    }

    #[test]
    fn reads_tool_versions_format() {
        let mut tools = HashMap::new();
        parse_tool_versions(
            "java corretto-21.0.5\n# comment\npython 3.13.3\n",
            &mut tools,
        );

        assert_eq!(tools.get("java").unwrap(), "corretto-21.0.5");
        assert_eq!(tools.get("python").unwrap(), "3.13.3");
    }
}
