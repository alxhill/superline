//! The case manifest: what a capture runs and how its snapshots are checked.

use std::collections::BTreeMap;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::{Result, Shell, DEFAULT_COLUMNS};

/// One capture definition. Every field but `name` and `steps` is optional.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Case {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
    /// Absent: the fixture config. A string: a config file, relative to the
    /// manifest. An object: the config itself.
    #[serde(default)]
    pub(crate) config: Option<Value>,
    #[serde(default)]
    pub(crate) columns: Option<Columns>,
    #[serde(default)]
    pub(crate) rows: Option<u16>,
    /// Restrict the case to these shells.
    #[serde(default)]
    pub(crate) shells: Option<Vec<String>>,
    /// Restrict the case to these platforms (`macos`, `linux`, `windows`).
    #[serde(default)]
    pub(crate) platforms: Option<Vec<String>>,
    /// Working directory, relative to the scratch root; created if missing.
    #[serde(default)]
    pub(crate) dir: Option<String>,
    /// Extra environment variables exported by the setup command.
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
    /// A known bug: failing runs are reported but do not fail the suite,
    /// and a run that passes does, so the marker is removed once fixed.
    #[serde(default)]
    pub(crate) xfail: Option<Xfail>,
    pub(crate) steps: Vec<Step>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Xfail {
    Everywhere(String),
    Scoped {
        reason: String,
        #[serde(default)]
        shells: Option<Vec<String>>,
        #[serde(default)]
        platforms: Option<Vec<String>>,
    },
}

/// A regex that optionally only applies to some shells.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Pattern {
    Everywhere(String),
    Scoped { regex: String, shells: Vec<String> },
}

impl Pattern {
    pub(crate) fn for_shell(&self, shell: &str) -> Option<&str> {
        match self {
            Self::Everywhere(regex) => Some(regex),
            Self::Scoped { regex, shells } => shell_listed(shells, shell).then_some(regex),
        }
    }
}

pub(crate) fn shell_listed(shells: &[String], shell: &str) -> bool {
    shells
        .iter()
        .any(|name| Shell::parse(name).is_ok_and(|listed| listed.name() == shell))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Columns {
    One(u16),
    Many(Vec<u16>),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Step {
    /// Type a command and press Enter.
    Run(Input),
    /// Type text without pressing Enter.
    Type(Input),
    /// Press a key, as a VHS key command such as `Enter`, `Ctrl+C` or `Tab 2`.
    Key(String),
    /// Wait until the whole screen matches a regex.
    Wait(String),
    /// Wait until the cursor line matches a regex.
    WaitLine(String),
    /// Pause for a VHS duration such as `500ms` or `2s`.
    Sleep(String),
    /// Capture a PNG and the screen text, then check the text.
    Snapshot(Snapshot),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Input {
    Text(String),
    /// A command that exits with this status in the current shell.
    Exit {
        exit: u8,
    },
    /// A command per shell name, with `default` for the rest.
    PerShell(BTreeMap<String, String>),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Snapshot {
    Name(String),
    Spec(SnapshotSpec),
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotSpec {
    pub(crate) name: String,
    /// Regexes the screen text must match.
    #[serde(default)]
    pub(crate) expect: Vec<Pattern>,
    /// Regexes the screen text must not match.
    #[serde(default)]
    pub(crate) reject: Vec<Pattern>,
    /// Display-width checks on the lines matching a regex.
    #[serde(default)]
    pub(crate) widths: Vec<WidthCheck>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WidthCheck {
    /// Every screen line matching this regex is checked; at least one must.
    pub(crate) line: String,
    #[serde(default)]
    pub(crate) width: Option<Value>,
    #[serde(default)]
    pub(crate) min: Option<Value>,
    #[serde(default)]
    pub(crate) max: Option<Value>,
    /// Only check on these shells.
    #[serde(default)]
    pub(crate) shells: Option<Vec<String>>,
}

impl Snapshot {
    pub(crate) fn spec(&self) -> SnapshotSpec {
        match self {
            Self::Name(name) => SnapshotSpec {
                name: name.clone(),
                ..SnapshotSpec::default()
            },
            Self::Spec(spec) => spec.clone(),
        }
    }
}

impl Case {
    pub(crate) fn columns(&self) -> Vec<u16> {
        match &self.columns {
            None => vec![DEFAULT_COLUMNS],
            Some(Columns::One(columns)) => vec![*columns],
            Some(Columns::Many(columns)) => columns.clone(),
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let invalid = |message: String| -> Result<()> {
            Err(format!("case {:?}: {message}", self.name).into())
        };
        if self.name.is_empty()
            || !self
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return invalid("names may only use ASCII letters, digits, '-' and '_'".into());
        }
        let mut shell_names: Vec<&String> = self.shells.iter().flatten().collect();
        if let Some(Xfail::Scoped { shells, .. }) = &self.xfail {
            shell_names.extend(shells.iter().flatten());
        }
        for step in &self.steps {
            if let Step::Snapshot(Snapshot::Spec(spec)) = step {
                for pattern in spec.expect.iter().chain(&spec.reject) {
                    if let Pattern::Scoped { shells, .. } = pattern {
                        shell_names.extend(shells);
                    }
                }
                for check in &spec.widths {
                    shell_names.extend(check.shells.iter().flatten());
                }
            }
        }
        for shell in shell_names {
            Shell::parse(shell)?;
        }
        let mut names = Vec::new();
        for step in &self.steps {
            match step {
                Step::Snapshot(snapshot) => {
                    let spec = snapshot.spec();
                    if names.contains(&spec.name) {
                        return invalid(format!("duplicate snapshot {:?}", spec.name));
                    }
                    if spec.name.is_empty()
                        || !spec
                            .name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    {
                        return invalid(format!("invalid snapshot name {:?}", spec.name));
                    }
                    names.push(spec.name);
                }
                Step::Key(key) if !valid_key(key) => {
                    return invalid(format!("unsupported key {key:?}"));
                }
                Step::Run(Input::PerShell(map)) | Step::Type(Input::PerShell(map)) => {
                    for shell in map.keys().filter(|name| *name != "default") {
                        Shell::parse(shell)?;
                    }
                }
                _ => {}
            }
        }
        if names.is_empty() {
            return invalid("has no snapshot step".into());
        }
        for (key, value) in &self.env {
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return invalid(format!("invalid environment variable name {key:?}"));
            }
            if value.contains('\'') {
                return invalid(format!("the value of {key} may not contain a single quote"));
            }
        }
        Ok(())
    }

    /// Why this run is expected to fail, if it is.
    pub(crate) fn xfail_reason(&self, shell: Shell, platform: &str) -> Option<&str> {
        match self.xfail.as_ref()? {
            Xfail::Everywhere(reason) => Some(reason),
            Xfail::Scoped {
                reason,
                shells,
                platforms,
            } => {
                let shell_ok = shells
                    .as_ref()
                    .is_none_or(|shells| shell_listed(shells, shell.name()));
                let platform_ok = platforms
                    .as_ref()
                    .is_none_or(|platforms| platforms.iter().any(|p| p == platform));
                (shell_ok && platform_ok).then_some(reason)
            }
        }
    }

    pub(crate) fn runs_on(&self, shell: Shell, platform: &str) -> bool {
        let shell_ok = self
            .shells
            .as_ref()
            .is_none_or(|shells| shell_listed(shells, shell.name()));
        let platform_ok = self
            .platforms
            .as_ref()
            .is_none_or(|platforms| platforms.iter().any(|p| p == platform));
        shell_ok && platform_ok
    }
}

pub(crate) fn valid_key(key: &str) -> bool {
    let pattern = Regex::new(
        r"^(Enter|Tab|Space|Backspace|Delete|Escape|Up|Down|Left|Right|PageUp|PageDown|Home|End|Insert|(Ctrl|Alt|Shift)\+\S+)( [0-9]+)?$",
    )
    .expect("valid key regex");
    pattern.is_match(key)
}

#[derive(Clone, Debug)]
pub(crate) struct Placeholders {
    pub(crate) shell: &'static str,
    pub(crate) columns: u16,
    pub(crate) rows: u16,
    /// The last typed text, escaped as a regex.
    pub(crate) last: String,
}

impl Placeholders {
    /// Fill `{{name}}` placeholders. `sep`, `round`, and `ok` expand to regex
    /// escapes for superline's chevron and round separators and the `cmd`
    /// widget's success mark, so they only belong in regexes.
    pub(crate) fn fill(&self, template: &str) -> String {
        template
            .replace("{{shell}}", self.shell)
            .replace("{{columns}}", &self.columns.to_string())
            .replace("{{rows}}", &self.rows.to_string())
            .replace("{{last}}", &self.last)
            .replace("{{sep}}", r"\x{E0B0}")
            .replace("{{round}}", r"\x{E0B4}")
            .replace("{{ok}}", r"\x{F105}")
    }
}

impl Input {
    pub(crate) fn resolve(&self, shell: Shell) -> Result<String> {
        match self {
            Self::Text(text) => Ok(text.clone()),
            Self::Exit { exit } => Ok(shell.exit_command(*exit)),
            Self::PerShell(commands) => commands
                .iter()
                .find(|(name, _)| Shell::parse(name).is_ok_and(|s| s == shell))
                .or_else(|| commands.get_key_value("default"))
                .map(|(_, command)| command.clone())
                .ok_or_else(|| format!("no command for {} and no default", shell.name()).into()),
        }
    }
}

pub(crate) fn check_text(
    text: &str,
    spec: &SnapshotSpec,
    placeholders: &Placeholders,
) -> Vec<String> {
    let mut failures = Vec::new();
    let compile = |pattern: &str, failures: &mut Vec<String>| {
        let pattern = placeholders.fill(pattern);
        match Regex::new(&pattern) {
            Ok(regex) => Some(regex),
            Err(error) => {
                failures.push(format!("invalid regex {pattern:?}: {error}"));
                None
            }
        }
    };
    let shell = placeholders.shell;
    for pattern in spec.expect.iter().filter_map(|p| p.for_shell(shell)) {
        if let Some(regex) = compile(pattern, &mut failures) {
            if !regex.is_match(text) {
                failures.push(format!("screen does not match {:?}", regex.as_str()));
            }
        }
    }
    for pattern in spec.reject.iter().filter_map(|p| p.for_shell(shell)) {
        if let Some(regex) = compile(pattern, &mut failures) {
            if let Some(found) = regex.find(text) {
                failures.push(format!(
                    "screen matches rejected {:?} at {:?}",
                    regex.as_str(),
                    found.as_str()
                ));
            }
        }
    }
    for check in &spec.widths {
        if check
            .shells
            .as_ref()
            .is_some_and(|shells| !shell_listed(shells, shell))
        {
            continue;
        }
        let Some(regex) = compile(&check.line, &mut failures) else {
            continue;
        };
        let bound = |value: &Option<Value>, failures: &mut Vec<String>| match value {
            None => None,
            Some(value) => match evaluate(value, placeholders) {
                Ok(n) => Some(n),
                Err(error) => {
                    failures.push(error);
                    None
                }
            },
        };
        let exact = bound(&check.width, &mut failures);
        let min = bound(&check.min, &mut failures).or(exact);
        let max = bound(&check.max, &mut failures).or(exact);
        let matching: Vec<&str> = text.lines().filter(|line| regex.is_match(line)).collect();
        if matching.is_empty() {
            failures.push(format!("no screen line matches {:?}", regex.as_str()));
        }
        for line in matching {
            let width = display_width(line) as i64;
            if min.is_some_and(|min| width < min) || max.is_some_and(|max| width > max) {
                let wanted = match (min, max) {
                    (Some(a), Some(b)) if a == b => format!("{a}"),
                    (Some(a), Some(b)) => format!("{a}..={b}"),
                    (Some(a), None) => format!(">= {a}"),
                    (None, Some(b)) => format!("<= {b}"),
                    (None, None) => unreachable!(),
                };
                failures.push(format!(
                    "line is {width} columns wide, wanted {wanted}: {line:?}"
                ));
            }
        }
    }
    failures
}

/// A width bound: a number, or a string such as `"{{columns}}"` or
/// `"{{columns}} - 1"` after filling placeholders.
pub(crate) fn evaluate(
    value: &Value,
    placeholders: &Placeholders,
) -> std::result::Result<i64, String> {
    if let Some(n) = value.as_i64() {
        return Ok(n);
    }
    let Some(expr) = value.as_str() else {
        return Err(format!(
            "width bound {value} is neither a number nor a string"
        ));
    };
    let filled = placeholders.fill(expr);
    let mut total = 0i64;
    let mut sign = 1i64;
    for token in filled.split_whitespace().flat_map(|t| {
        // Split `a-b` into `a`, `-`, `b` so spacing is optional.
        let mut parts = Vec::new();
        let mut current = String::new();
        for c in t.chars() {
            if c == '+' || c == '-' {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
                parts.push(c.to_string());
            } else {
                current.push(c);
            }
        }
        if !current.is_empty() {
            parts.push(current);
        }
        parts
    }) {
        match token.as_str() {
            "+" => sign = 1,
            "-" => sign = -1,
            number => {
                let n: i64 = number
                    .parse()
                    .map_err(|_| format!("width bound {filled:?} is not arithmetic"))?;
                total += sign * n;
                sign = 1;
            }
        }
    }
    Ok(total)
}

/// Terminal columns taken by a screen line. xterm.js omits the placeholder
/// cell after a wide character from its text, so each wide character counts
/// twice here.
pub(crate) fn display_width(line: &str) -> usize {
    line.chars().map(char_width).sum()
}

pub(crate) fn char_width(c: char) -> usize {
    let c = c as u32;
    let zero = matches!(c, 0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F | 0xE0100..=0xE01EF);
    let wide = matches!(c,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD);
    if zero {
        0
    } else if wide {
        2
    } else {
        1
    }
}
