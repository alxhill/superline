//! Runs superline in real interactive shells through VHS and hands back what
//! the terminal showed.
//!
//! A case is a superline config plus a VHS tape. The rig prepares an isolated
//! home directory, loads `superline init <shell>` into the shell, prepends the
//! shared settings and hidden setup from `tape.template` to the case's tape,
//! and runs VHS. VHS owns the PTY (ConPTY on Windows), the terminal emulation,
//! and the rendering. Each `Screenshot` in the tape becomes a [`Snapshot`]: the
//! PNG plus the visible screen text at the same moment, for the caller to
//! check.
//!
//! Shared by `tests/terminal_snapshots.rs` (the checked cases) and
//! `examples/terminal-snapshot.rs` (one-off captures).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use regex::Regex;
use unicode_width::UnicodeWidthStr;

/// The chevron separator.
pub const SEP: char = '\u{E0B0}';
/// The round separator.
pub const ROUND: char = '\u{E0B4}';
/// The `cmd` widget's success mark.
pub const OK: char = '\u{F105}';

/// Name of the helper command the rig puts on the shell's `PATH`, so tapes
/// can do the same thing in every shell: `sl-test exit <N>` exits with that
/// status, and `sl-test print <TEXT>...` prints without a trailing newline.
pub const HELPER: &str = "sl-test";

pub const CASES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/terminal/cases");
const DEFAULT_CONFIG: &str = include_str!("config.json");
const TAPE_TEMPLATE: &str = include_str!("tape.template");
/// The tape for a case that has none: one snapshot of the first prompt.
const DEFAULT_TAPE: &str = "Screenshot prompt.png\n";
const DEFAULT_DIR: &str = "superline-e2e";
const VHS_TIMEOUT: Duration = Duration::from_secs(240);
/// VHS separates the screen dumps in its text output with this line.
const FRAME_SEPARATOR: &str =
    "────────────────────────────────────────────────────────────────────────────────";

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Acts as the `sl-test` helper when this executable was started under that
/// name, and exits. Call it first thing in `main`.
pub fn run_helper_if_invoked() {
    let mut args = env::args_os();
    let invoked_as = args
        .next()
        .map(PathBuf::from)
        .and_then(|path| path.file_stem().map(|stem| stem.to_os_string()));
    if invoked_as.as_deref() != Some(HELPER.as_ref()) {
        return;
    }
    let args: Vec<String> = args.map(|arg| arg.to_string_lossy().into_owned()).collect();
    match args.split_first() {
        Some((command, rest)) if command == "exit" => {
            let code = rest.first().and_then(|code| code.parse().ok()).unwrap_or(1);
            std::process::exit(code);
        }
        Some((command, rest)) if command == "print" => {
            let mut stdout = std::io::stdout();
            let _ = write!(stdout, "{}", rest.join(" "));
            let _ = stdout.flush();
            std::process::exit(0);
        }
        _ => {
            eprintln!("usage: {HELPER} exit <N> | {HELPER} print <TEXT>...");
            std::process::exit(2);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Pwsh,
    Nu,
}

impl Shell {
    pub const ALL: [Self; 5] = [Self::Bash, Self::Zsh, Self::Fish, Self::Pwsh, Self::Nu];

    pub fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Pwsh => "pwsh",
            Self::Nu => "nu",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "bash" => Ok(Self::Bash),
            "zsh" => Ok(Self::Zsh),
            "fish" => Ok(Self::Fish),
            "pwsh" | "powershell" => Ok(Self::Pwsh),
            "nu" | "nushell" => Ok(Self::Nu),
            _ => Err(format!("unsupported shell {value:?}").into()),
        }
    }

    /// Parses `all` or a list of shell names separated by commas.
    pub fn parse_list(value: &str) -> Result<Vec<Self>> {
        let mut shells = Vec::new();
        for name in value
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if name == "all" {
                shells.extend(Self::ALL);
            } else {
                shells.push(Self::parse(name)?);
            }
        }
        shells.sort_by_key(|shell| shell.name());
        shells.dedup();
        Ok(shells)
    }

    pub fn is_available(self) -> bool {
        find_executable(self.name()).is_some()
    }

    /// Whether the shell draws the right side of the last row. Bash and
    /// PowerShell have no right prompt.
    pub fn draws_last_row_right(self) -> bool {
        matches!(self, Self::Fish | Self::Zsh | Self::Nu)
    }

    fn init_file_name(self) -> &'static str {
        match self {
            Self::Bash => "superline-init.sh",
            Self::Zsh => "superline-init.zsh",
            Self::Fish => "superline-init.fish",
            Self::Pwsh => "superline-init.ps1",
            Self::Nu => "superline-init.nu",
        }
    }

    /// One hidden command line that points the shell at the isolated home,
    /// exports the case's variables, loads the init snippet, enters the
    /// working directory, and clears the setup output. The home is exported
    /// inside the shell rather than on the VHS process: on Windows, Chrome
    /// resolves its own app-data folders through `%USERPROFILE%` and exits
    /// when that points at the fixture. `USERPROFILE` is what superline reads
    /// on Windows, including under Git Bash, where `HOME` is an MSYS path.
    fn setup_command(
        self,
        home: &str,
        init: &str,
        dir: &str,
        extra_env: &BTreeMap<String, String>,
    ) -> String {
        let mut vars = vec![
            ("HOME".to_string(), home.to_string()),
            ("XDG_CONFIG_HOME".to_string(), format!("{home}/.config")),
            ("XDG_CACHE_HOME".to_string(), format!("{home}/.cache")),
        ];
        if self != Self::Fish {
            vars.insert(1, ("USERPROFILE".to_string(), home.to_string()));
        }
        vars.extend(extra_env.iter().map(|(k, v)| (k.clone(), v.clone())));

        match self {
            Self::Bash | Self::Zsh => {
                let exports: Vec<String> = vars.iter().map(|(k, v)| format!("{k}='{v}'")).collect();
                format!(
                    "export {} && source '{init}' && cd '{dir}' && clear",
                    exports.join(" ")
                )
            }
            Self::Fish => {
                let sets: String = vars
                    .iter()
                    .map(|(k, v)| format!("set -gx {k} '{v}'; "))
                    .collect();
                format!("{sets}source '{init}'; and cd '{dir}'; and clear")
            }
            Self::Pwsh => {
                let sets: String = vars
                    .iter()
                    .map(|(k, v)| format!("$env:{k} = '{v}'; "))
                    .collect();
                format!("{sets}. '{init}'; Set-Location '{dir}'; Clear-Host")
            }
            Self::Nu => {
                let sets: String = vars
                    .iter()
                    .map(|(k, v)| format!("$env.{k} = '{v}'; "))
                    .collect();
                format!("{sets}source '{init}'; cd '{dir}'; clear")
            }
        }
    }
}

/// A superline config and the tape that drives it, plus the terminal and
/// fixture to run them in.
#[derive(Clone, Debug)]
pub struct Case {
    pub name: String,
    /// Files written to `~/.config/superline/`; `config.json` is the config.
    pub config_files: Vec<(String, Vec<u8>)>,
    pub tape: String,
    pub columns: Vec<u16>,
    pub rows: u16,
    /// Working directory, relative to the scratch root; created if missing.
    pub dir: String,
    /// An existing directory to run in instead of `dir`.
    pub workdir: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
}

impl Case {
    fn new(name: &str, config_files: Vec<(String, Vec<u8>)>, tape: String) -> Self {
        Self {
            name: name.to_string(),
            config_files,
            tape,
            columns: vec![100],
            rows: 12,
            dir: DEFAULT_DIR.to_string(),
            workdir: None,
            env: BTreeMap::new(),
        }
    }

    /// Loads `tests/terminal/cases/<name>/`: `case.tape` (optional; one
    /// snapshot of the first prompt otherwise) and every other file, which
    /// is copied into the config directory. Without a `config.json` the
    /// shared `tests/terminal/config.json` is used.
    pub fn load(name: &str) -> Result<Self> {
        let dir = Path::new(CASES_DIR).join(name);
        let mut files = Vec::new();
        let mut tape = DEFAULT_TAPE.to_string();
        let entries =
            fs::read_dir(&dir).map_err(|error| format!("no case at {}: {error}", dir.display()))?;
        for entry in entries {
            let path = entry?.path();
            let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
            if file_name == "case.tape" {
                tape = fs::read_to_string(&path)?;
            } else if path.is_file() {
                files.push((file_name, fs::read(&path)?));
            }
        }
        if !files.iter().any(|(name, _)| name == "config.json") {
            files.push((
                "config.json".to_string(),
                DEFAULT_CONFIG.as_bytes().to_vec(),
            ));
        }
        Ok(Self::new(name, files, tape))
    }

    /// A case from a config file (plus a theme file it names relative to
    /// itself) and an optional tape file.
    pub fn from_files(name: &str, config: Option<&Path>, tape: Option<&Path>) -> Result<Self> {
        let mut files = Vec::new();
        match config {
            Some(config) => {
                let contents = fs::read_to_string(config)
                    .map_err(|error| format!("could not read {}: {error}", config.display()))?;
                let parsed: serde_json::Value = serde_json::from_str(&contents)
                    .map_err(|error| format!("{} is not valid JSON: {error}", config.display()))?;
                if let Some(theme) = parsed.get("theme").and_then(|theme| theme.as_str()) {
                    let theme_file = config.parent().unwrap_or(Path::new(".")).join(theme);
                    if theme_file.is_file() {
                        files.push((theme.to_string(), fs::read(theme_file)?));
                    }
                }
                files.push(("config.json".to_string(), contents.into_bytes()));
            }
            None => files.push((
                "config.json".to_string(),
                DEFAULT_CONFIG.as_bytes().to_vec(),
            )),
        }
        let tape = match tape {
            Some(tape) => fs::read_to_string(tape)
                .map_err(|error| format!("could not read {}: {error}", tape.display()))?,
            None => DEFAULT_TAPE.to_string(),
        };
        Ok(Self::new(name, files, tape))
    }

    pub fn columns(mut self, columns: &[u16]) -> Self {
        self.columns = columns.to_vec();
        self
    }

    pub fn rows(mut self, rows: u16) -> Self {
        self.rows = rows;
        self
    }

    pub fn dir(mut self, dir: &str) -> Self {
        self.dir = dir.to_string();
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }
}

/// One run of a case in one shell at one width.
#[derive(Debug)]
pub struct Capture {
    pub case: String,
    pub shell: Shell,
    pub columns: u16,
    pub rows: u16,
    pub snapshots: Vec<Snapshot>,
    pub tape: PathBuf,
}

impl Capture {
    pub fn label(&self) -> String {
        format!(
            "{}/{}-{}x{}",
            self.case,
            self.shell.name(),
            self.columns,
            self.rows
        )
    }

    pub fn snapshot(&self, name: &str) -> &Snapshot {
        self.snapshots
            .iter()
            .find(|snapshot| snapshot.name == name)
            .unwrap_or_else(|| panic!("the tape takes no snapshot named {name:?}"))
    }
}

/// A screenshot and the visible screen text at the same moment. Lines are
/// joined with `\n` and have trailing spaces trimmed.
#[derive(Debug)]
pub struct Snapshot {
    pub name: String,
    pub png: PathBuf,
    pub text_file: PathBuf,
    pub text: String,
}

impl Snapshot {
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.text.lines()
    }

    /// The first line containing `needle`.
    pub fn line(&self, needle: &str) -> &str {
        self.lines()
            .find(|line| line.contains(needle))
            .unwrap_or_else(|| self.fail(format!("no line contains {needle:?}")))
    }

    pub fn matches(&self, pattern: &str) -> bool {
        Regex::new(pattern)
            .unwrap_or_else(|error| panic!("invalid regex {pattern:?}: {error}"))
            .is_match(&self.text)
    }

    /// Panics with `message` and the screen text unless `ok`.
    pub fn check(&self, ok: bool, message: impl std::fmt::Display) {
        if !ok {
            self.fail(message);
        }
    }

    pub fn fail(&self, message: impl std::fmt::Display) -> ! {
        panic!(
            "{message}\nsnapshot {:?} ({}):\n{}",
            self.name,
            self.text_file.display(),
            self.text
        )
    }
}

/// Terminal columns a screen line takes up.
pub fn width(line: &str) -> usize {
    line.width()
}

/// Where and how captures run.
pub struct Rig {
    vhs: PathBuf,
    superline: PathBuf,
    output: PathBuf,
    platform: String,
    jobs: usize,
    /// Holds the `sl-test` helper, put first on the shell's `PATH`.
    bin_dir: PathBuf,
}

impl Rig {
    pub fn new(vhs: &Path, superline: &Path, output: &Path, jobs: usize) -> Result<Self> {
        if !superline.is_file() {
            return Err(format!("{} does not exist", superline.display()).into());
        }
        if !vhs.is_file() {
            return Err(format!("vhs binary {} does not exist", vhs.display()).into());
        }
        fs::create_dir_all(output)?;
        let bin_dir = env::temp_dir().join(format!(
            "superline-terminal-snapshot-{}-bin",
            std::process::id()
        ));
        fs::create_dir_all(&bin_dir)?;
        let helper = bin_dir.join(format!("{HELPER}{}", env::consts::EXE_SUFFIX));
        let _ = fs::remove_file(&helper);
        let exe = env::current_exe()?;
        if fs::hard_link(&exe, &helper).is_err() {
            fs::copy(&exe, &helper)?;
        }
        Ok(Self {
            // VHS runs in the output directory, so relative paths would break.
            vhs: std::path::absolute(vhs)?,
            superline: std::path::absolute(superline)?,
            // Not `canonicalize`: on Windows that yields a `\\?\` verbatim
            // path, which ffmpeg does not accept as a screenshot destination.
            output: std::path::absolute(output)?,
            platform: platform_name(),
            jobs: jobs.max(1),
            bin_dir,
        })
    }

    pub fn vhs_version(&self) -> Result<String> {
        let output = Command::new(&self.vhs).arg("--version").output()?;
        if !output.status.success() {
            return Err(format!("`{} --version` failed", self.vhs.display()).into());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Runs every case in every shell at each of its widths, `jobs` at a
    /// time, and returns the results in that order, each with the index of
    /// its case and a label.
    pub fn run(&self, cases: &[Case], shells: &[Shell]) -> Vec<(usize, String, Result<Capture>)> {
        let runs: Vec<(usize, Shell, u16)> = cases
            .iter()
            .enumerate()
            .flat_map(|(case_index, case)| {
                shells.iter().flat_map(move |&shell| {
                    case.columns
                        .iter()
                        .map(move |&columns| (case_index, shell, columns))
                })
            })
            .collect();
        let next = AtomicUsize::new(0);
        let results = Mutex::new(Vec::new());
        thread::scope(|scope| {
            for _ in 0..self.jobs.min(runs.len()) {
                scope.spawn(|| loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    let Some(&(case_index, shell, columns)) = runs.get(index) else {
                        break;
                    };
                    let case = &cases[case_index];
                    let label = format!("{}/{}-{columns}x{}", case.name, shell.name(), case.rows);
                    let result = self.capture(case, shell, columns, index);
                    results
                        .lock()
                        .unwrap()
                        .push((index, case_index, label, result));
                });
            }
        });
        let mut results = results.into_inner().unwrap();
        results.sort_by_key(|(index, ..)| *index);
        results
            .into_iter()
            .map(|(_, case_index, label, result)| (case_index, label, result))
            .collect()
    }

    fn capture(&self, case: &Case, shell: Shell, columns: u16, index: usize) -> Result<Capture> {
        let case_dir = self.output.join(&case.name);
        fs::create_dir_all(&case_dir)?;
        let stem = format!("{}-{}-{columns}x{}", self.platform, shell.name(), case.rows);
        let tape_path = case_dir.join(format!("{stem}.tape"));
        let log_path = case_dir.join(format!("{stem}.log"));
        let frames_path = case_dir.join(format!("{stem}.frames.txt"));
        let _ = fs::remove_file(&frames_path);

        let fixture = Fixture::prepare(case, shell, index, &self.superline)?;
        let result = (|| {
            let setup = shell.setup_command(
                &forward_slashes(&fixture.home),
                &forward_slashes(&fixture.init),
                &forward_slashes(&fixture.dir),
                &case.env,
            );
            let (body, screenshots) = rewrite_tape(&case.tape, &case_dir, &stem)?;
            let tape = TAPE_TEMPLATE
                .replace("{{frames}}", &forward_slashes(&frames_path))
                .replace("{{shell}}", shell.name())
                .replace("{{columns}}", &columns.to_string())
                .replace("{{rows}}", &case.rows.to_string())
                .replace("{{setup}}", &tape_string(&setup)?)
                .replace("{{tape}}", &body);
            fs::write(&tape_path, &tape)?;
            self.run_vhs(&tape_path, &log_path, &case_dir)?;
            let snapshots = collect_snapshots(&tape, &screenshots, &frames_path)?;
            Ok(Capture {
                case: case.name.clone(),
                shell,
                columns,
                rows: case.rows,
                snapshots,
                tape: tape_path.clone(),
            })
        })();
        fixture.remove();
        result.map_err(|error: Box<dyn Error + Send + Sync>| {
            format!(
                "{error}\nvhs log ({}):\n{}",
                log_path.display(),
                log_tail(&log_path)
            )
            .into()
        })
    }

    fn run_vhs(&self, tape: &Path, log_path: &Path, working_dir: &Path) -> Result<()> {
        let log = fs::File::create(log_path)?;
        let mut paths = vec![
            self.bin_dir.clone(),
            self.superline.parent().unwrap().to_path_buf(),
        ];
        if let Some(path) = env::var_os("PATH") {
            paths.extend(env::split_paths(&path));
        }
        let mut child = Command::new(&self.vhs)
            .arg(tape)
            .current_dir(working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .env("LANG", "en_US.UTF-8")
            .env("LC_ALL", "en_US.UTF-8")
            .env("PATH", env::join_paths(paths)?)
            .spawn()?;

        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() >= VHS_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("vhs did not finish within {VHS_TIMEOUT:?}").into());
            }
            thread::sleep(Duration::from_millis(200));
        };
        if !status.success() {
            return Err(format!("vhs exited with {status}").into());
        }
        Ok(())
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.bin_dir);
    }
}

/// Isolated home and working directory for one shell session.
struct Fixture {
    root: PathBuf,
    home: PathBuf,
    dir: PathBuf,
    init: PathBuf,
}

impl Fixture {
    fn prepare(case: &Case, shell: Shell, index: usize, superline: &Path) -> Result<Self> {
        let root = env::temp_dir().join(format!(
            "superline-terminal-snapshot-{}-{index}",
            std::process::id(),
        ));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        let home = root.join("home");
        let dir = match &case.workdir {
            Some(workdir) if workdir.is_dir() => workdir.clone(),
            Some(workdir) => return Err(format!("{} is not a directory", workdir.display()).into()),
            None => root.join(&case.dir),
        };
        let config_dir = home.join(".config/superline");
        fs::create_dir_all(&config_dir)?;
        fs::create_dir_all(home.join(".cache"))?;
        fs::create_dir_all(&dir)?;
        for (name, contents) in &case.config_files {
            fs::write(config_dir.join(name), contents)?;
        }

        let output = Command::new(superline)
            .args(["init", shell.name()])
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "`superline init {}` failed: {}",
                shell.name(),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let mut init = String::from_utf8(output.stdout)?;
        if shell == Shell::Pwsh {
            // PowerShell resolves the home directory itself, so point it at
            // the fixture config explicitly rather than trusting inheritance.
            let original = "$__pl_args = @('show', '-s', $__pl_status, '-c', $__pl_cols, 'pwsh')";
            if !init.contains(original) {
                return Err("PowerShell init no longer contains the expected argument list".into());
            }
            let config = forward_slashes(&config_dir.join("config.json")).replace('\'', "''");
            init = init.replace(
                original,
                &format!(
                    "$__pl_args = @('show', '-s', $__pl_status, '-c', $__pl_cols, 'pwsh', '--config', '{config}')"
                ),
            );
        }
        let init_path = home.join(shell.init_file_name());
        fs::write(&init_path, init)?;

        Ok(Self {
            root,
            home,
            dir,
            init: init_path,
        })
    }

    fn remove(self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Checks a case's tape and points each `Screenshot <name>.png` at the
/// output directory. A screenshot is written from the next recorded frame,
/// so each one gets a short settle before it and a frame after it. Returns
/// the rewritten tape and the snapshot names with their PNG paths.
fn rewrite_tape(
    tape: &str,
    case_dir: &Path,
    stem: &str,
) -> Result<(String, Vec<(String, PathBuf)>)> {
    let name_pattern = Regex::new(r"^[A-Za-z0-9_-]+$").unwrap();
    let mut lines = Vec::new();
    let mut screenshots: Vec<(String, PathBuf)> = Vec::new();
    for (number, line) in tape.lines().enumerate() {
        let trimmed = line.trim();
        let command = trimmed.split_whitespace().next().unwrap_or("");
        let command = command.split(['@', '+']).next().unwrap_or("");
        if matches!(
            command,
            "Set" | "Output" | "Hide" | "Show" | "Require" | "Source"
        ) {
            return Err(format!(
                "tape line {}: `{command}` is managed by the rig; set sizes and env in code",
                number + 1
            )
            .into());
        }
        if command != "Screenshot" {
            lines.push(line.to_string());
            continue;
        }
        let argument = trimmed["Screenshot".len()..]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'' || c == '`');
        let name = argument.strip_suffix(".png").unwrap_or(argument);
        if !name_pattern.is_match(name) {
            return Err(format!(
                "tape line {}: screenshot names are plain file names like `prompt.png`",
                number + 1
            )
            .into());
        }
        if screenshots.iter().any(|(existing, _)| existing == name) {
            return Err(format!("tape line {}: duplicate screenshot {name:?}", number + 1).into());
        }
        let png = case_dir.join(format!("{stem}-{name}.png"));
        let _ = fs::remove_file(&png);
        lines.push("Sleep 500ms".to_string());
        lines.push(format!(
            "Screenshot {}",
            tape_string(&forward_slashes(&png))?
        ));
        lines.push("Sleep 1s".to_string());
        screenshots.push((name.to_string(), png));
    }
    if screenshots.is_empty() {
        return Err("the tape takes no Screenshot".into());
    }
    Ok((lines.join("\n"), screenshots))
}

/// Splits VHS's text output into screens and pairs each screenshot with the
/// one dumped right after it. Once recording is shown, VHS dumps the screen
/// after every command; how many dumps the hidden setup writes varies, so
/// indexes count back from the end of the file.
fn collect_snapshots(
    tape: &str,
    screenshots: &[(String, PathBuf)],
    frames_path: &Path,
) -> Result<Vec<Snapshot>> {
    let frames_text = fs::read_to_string(frames_path)
        .map_err(|error| format!("vhs did not write {}: {error}", frames_path.display()))?;
    let mut frames: Vec<&str> = frames_text
        .split(&format!("{FRAME_SEPARATOR}\n"))
        .map(|frame| frame.trim_end_matches('\n'))
        .collect();
    // The text after the last separator is empty.
    frames.pop();

    let mut recorded = None;
    let mut offsets = Vec::new();
    for line in tape.lines().map(str::trim) {
        let command = !(line.is_empty() || line.starts_with('#'));
        match recorded.as_mut() {
            None if line == "Show" => recorded = Some(0),
            Some(count) if command => {
                *count += 1;
                if line.starts_with("Screenshot ") {
                    offsets.push(*count);
                }
            }
            _ => {}
        }
    }
    // `Show` itself is the first recorded command.
    let recorded = recorded.ok_or("tape has no Show command")? + 1;
    let first = frames.len().checked_sub(recorded).ok_or_else(|| {
        format!(
            "expected at least {recorded} screen dumps in {}, found {}",
            frames_path.display(),
            frames.len()
        )
    })?;

    let mut snapshots = Vec::new();
    for ((name, png), offset) in screenshots.iter().zip(offsets) {
        let text = frames[first + offset].to_string();
        let text_file = png.with_extension("txt");
        fs::write(&text_file, format!("{text}\n"))?;
        if fs::metadata(png).map(|m| m.len()).unwrap_or(0) == 0 {
            return Err(format!("vhs did not write {}", png.display()).into());
        }
        snapshots.push(Snapshot {
            name: name.clone(),
            png: png.clone(),
            text_file,
            text,
        });
    }
    Ok(snapshots)
}

/// Quote a string for a VHS argument, using whichever of its three
/// delimiters the text does not contain.
fn tape_string(text: &str) -> Result<String> {
    ['"', '\'', '`']
        .into_iter()
        .find(|quote| !text.contains(*quote))
        .map(|quote| format!("{quote}{text}{quote}"))
        .ok_or_else(|| format!("{text:?} uses all of VHS's quote characters").into())
}

fn log_tail(path: &Path) -> String {
    let log = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

pub fn platform_name() -> String {
    env::var("RUNNER_OS")
        .unwrap_or_else(|_| env::consts::OS.to_string())
        .to_ascii_lowercase()
}

/// Paths typed into the shell or written into the tape use forward slashes,
/// which PowerShell, nushell, and VHS accept on Windows as well.
fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn find_executable(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let extensions: Vec<OsString> = if cfg!(windows) {
        env::var_os("PATHEXT")
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
            .to_string_lossy()
            .split(';')
            .map(OsString::from)
            .collect()
    } else {
        vec![OsString::new()]
    };
    for directory in env::split_paths(&path) {
        for extension in &extensions {
            let mut filename = OsString::from(name);
            filename.push(extension);
            let candidate = directory.join(filename);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
