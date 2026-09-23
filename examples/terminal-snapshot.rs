//! Capture superline prompts from real interactive shells through VHS.
//!
//! Every capture is a *case*: a superline config, a terminal size, a working
//! directory, and a list of steps (type a command, wait for the screen to
//! match, take a snapshot). Cases come from `terminal-snapshot/cases.json`, a
//! manifest passed with `--cases`, or the command line (`--config`, `--run`,
//! `--snapshot`, ...). Each case runs once per shell and terminal width.
//!
//! The example prepares an isolated home directory, loads
//! `superline init <shell>` into the shell, and drives the session with a VHS
//! tape generated from `terminal-snapshot/tape.template`. VHS owns the PTY
//! (ConPTY on Windows), the terminal emulation, and the rendering; this file
//! wires up fixtures, turns steps into tape commands, and checks each
//! snapshot's PNG and screen text afterwards.

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

#[path = "terminal-snapshot/case.rs"]
mod case;

use case::{check_text, Case, Input, Placeholders, Snapshot, SnapshotSpec, Step};

const DEFAULT_CONFIG: &str = include_str!("terminal-snapshot/config.json");
const DEFAULT_CASES: &str = include_str!("terminal-snapshot/cases.json");
const TAPE_TEMPLATE: &str = include_str!("terminal-snapshot/tape.template");
const DEFAULT_DIR: &str = "superline-e2e";
const DEFAULT_COLUMNS: u16 = 100;
const DEFAULT_ROWS: u16 = 12;
const VHS_TIMEOUT: Duration = Duration::from_secs(240);
/// VHS separates the screen dumps in its text output with this line.
const FRAME_SEPARATOR: &str =
    "────────────────────────────────────────────────────────────────────────────────";

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shell {
    Bash,
    Zsh,
    Fish,
    Pwsh,
    Nu,
}

impl Shell {
    const ALL: [Self; 5] = [Self::Bash, Self::Zsh, Self::Fish, Self::Pwsh, Self::Nu];

    fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Pwsh => "pwsh",
            Self::Nu => "nu",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "bash" => Ok(Self::Bash),
            "zsh" => Ok(Self::Zsh),
            "fish" => Ok(Self::Fish),
            "pwsh" | "powershell" => Ok(Self::Pwsh),
            "nu" | "nushell" => Ok(Self::Nu),
            _ => Err(format!("unsupported shell {value:?}").into()),
        }
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
                let sets: Vec<String> = vars
                    .iter()
                    .map(|(k, v)| format!("set -gx {k} '{v}'; "))
                    .collect();
                format!(
                    "{}source '{init}'; and cd '{dir}'; and clear",
                    sets.concat()
                )
            }
            Self::Pwsh => {
                let sets: Vec<String> = vars
                    .iter()
                    .map(|(k, v)| format!("$env:{k} = '{v}'; "))
                    .collect();
                format!(
                    "{}. '{init}'; Set-Location '{dir}'; Clear-Host",
                    sets.concat()
                )
            }
            Self::Nu => {
                let sets: Vec<String> = vars
                    .iter()
                    .map(|(k, v)| format!("$env.{k} = '{v}'; "))
                    .collect();
                format!("{}source '{init}'; cd '{dir}'; clear", sets.concat())
            }
        }
    }

    /// A command that makes the shell report exit status `code`.
    fn exit_command(self, code: u8) -> String {
        match (self, cfg!(windows)) {
            (Self::Bash | Self::Zsh, _) => format!("(exit {code})"),
            (Self::Fish, _) => format!("sh -c 'exit {code}'"),
            (Self::Pwsh, true) => format!("cmd /c exit {code}"),
            (Self::Pwsh, false) => format!("& sh -c 'exit {code}'"),
            (Self::Nu, true) => format!("^cmd /c exit {code}"),
            (Self::Nu, false) => format!("^sh -c 'exit {code}'"),
        }
    }
}

#[derive(Debug, Default)]
struct Args {
    shells: Vec<Shell>,
    cases_file: Option<PathBuf>,
    case_names: Vec<String>,
    output: PathBuf,
    vhs: Option<PathBuf>,
    config: Option<PathBuf>,
    workdir: Option<PathBuf>,
    columns: Vec<u16>,
    rows: Option<u16>,
    env: BTreeMap<String, String>,
    name: Option<String>,
    steps: Vec<Step>,
    jobs: usize,
    require_all: bool,
    print: bool,
    list: bool,
}

impl Args {
    /// A case assembled from the command line rather than a manifest.
    fn ad_hoc(&self) -> bool {
        self.config.is_some() || !self.steps.is_empty()
    }
}

const USAGE: &str = "\
Usage: cargo run --example terminal-snapshot -- [OPTIONS]

Runs the cases in examples/terminal-snapshot/cases.json (or --cases FILE)
once per shell and terminal width, or a single case built from the command
line when --config or any step flag is given.

Selection:
  --shell <all|bash|zsh|fish|pwsh|nu>  Shells to run (repeatable; default all)
  --case <NAME>                        Manifest cases to run (repeatable; default all)
  --cases <FILE>                       Case manifest to read instead of the built-in one
  --list                               List the manifest's cases and exit

Terminal and fixture (override the manifest for every case):
  --columns <N>                        Terminal width (repeatable; one run per width)
  --rows <N>                           Terminal height
  --workdir <DIR>                      Run in an existing directory
  --env <KEY=VALUE>                    Export a variable before the first prompt (repeatable)

Ad-hoc case (steps run in the order given):
  --config <FILE>                      superline config to capture (plus a sibling theme it names)
  --name <NAME>                        Case name for output files (default adhoc)
  --run <CMD>                          Type a command and press Enter
  --exit <N>                           Run a command that exits with status N
  --type <TEXT>                        Type text without pressing Enter
  --key <KEY>                          Press a VHS key such as Enter, Ctrl+C or 'Tab 2'
  --wait <REGEX>                       Wait until the screen matches
  --sleep <DURATION>                   Pause, e.g. 500ms
  --snapshot <NAME>                    Capture here; without any, one is taken
                                       after the first prompt and after each command

Output:
  --output <DIR>                       Output directory (default target/terminal-snapshots)
  --vhs <VHS_BINARY>                   VHS build to use (or SUPERLINE_E2E_VHS, or PATH)
  --jobs <N>                           Captures to run in parallel (default 2)
  --print                              Print each snapshot's screen text
  --require-all                        Fail instead of skipping missing shells
";

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("terminal snapshot failed: {error}");
            std::process::exit(2);
        }
    }
}

/// A case bound to one shell and width.
struct Run {
    case: Case,
    base_dir: PathBuf,
    shell: Shell,
    columns: u16,
    rows: u16,
}

impl Run {
    fn stem(&self, platform: &str) -> String {
        format!(
            "{platform}-{}-{}x{}",
            self.shell.name(),
            self.columns,
            self.rows
        )
    }
}

struct Context {
    superline: PathBuf,
    vhs: PathBuf,
    output: PathBuf,
    platform: String,
    workdir: Option<PathBuf>,
    extra_env: BTreeMap<String, String>,
}

#[derive(Debug)]
struct SnapshotResult {
    name: String,
    png: PathBuf,
    text_path: PathBuf,
    text: String,
    failures: Vec<String>,
}

fn run() -> Result<bool> {
    let args = parse_args()?;
    let (cases, base_dir) = load_cases(&args)?;
    if args.list {
        for case in &cases {
            let columns: Vec<String> = case.columns().iter().map(u16::to_string).collect();
            println!(
                "{:<16} {:>12} cols  {}",
                case.name,
                columns.join(","),
                case.description.as_deref().unwrap_or("")
            );
        }
        return Ok(true);
    }

    let superline = superline_binary()?;
    if !superline.is_file() {
        return Err(format!(
            "{} does not exist; run `cargo build --bin superline` first",
            superline.display()
        )
        .into());
    }
    let vhs = locate_vhs(args.vhs.as_deref())?;
    println!("using {}", vhs_version(&vhs)?);

    fs::create_dir_all(&args.output)?;
    // Not `canonicalize`: on Windows that yields a `\\?\` verbatim path,
    // which ffmpeg does not accept as a screenshot destination.
    let output = std::path::absolute(&args.output)?;
    let platform = platform_name();

    let mut available = Vec::new();
    for &shell in &args.shells {
        if find_executable(shell.name()).is_some() {
            available.push(shell);
        } else if args.require_all {
            return Err(format!("{} is not on PATH", shell.name()).into());
        } else {
            eprintln!("skipping {}: not on PATH", shell.name());
        }
    }

    let mut runs = Vec::new();
    for case in &cases {
        let columns = if args.columns.is_empty() {
            case.columns()
        } else {
            args.columns.clone()
        };
        for &shell in &available {
            if !case.runs_on(shell, &platform) {
                continue;
            }
            for &cols in &columns {
                runs.push(Run {
                    case: case.clone(),
                    base_dir: base_dir.clone(),
                    shell,
                    columns: cols,
                    rows: args.rows.or(case.rows).unwrap_or(DEFAULT_ROWS),
                });
            }
        }
    }
    if runs.is_empty() {
        return Err("no case runs on the available shells".into());
    }

    let context = Context {
        superline,
        vhs,
        output,
        platform,
        workdir: args.workdir.clone(),
        extra_env: args.env.clone(),
    };
    let results = run_all(&runs, &context, args.jobs.max(1));

    let mut summary = Vec::new();
    let (mut failed, mut xfailed) = (0, 0);
    for (run, result) in runs.iter().zip(&results) {
        let label = format!("{}/{}", run.case.name, run.stem(&context.platform));
        let xfail = run.case.xfail_reason(run.shell, &context.platform);
        let mut problems = Vec::new();
        let mut record = json!({
            "case": run.case.name,
            "shell": run.shell.name(),
            "platform": context.platform,
            "columns": run.columns,
            "rows": run.rows,
            "xfail": xfail,
        });
        match result {
            Ok(snapshots) => {
                for snapshot in snapshots {
                    if args.print {
                        println!("── {label} {} ──\n{}", snapshot.name, snapshot.text);
                    }
                    problems.extend(
                        snapshot
                            .failures
                            .iter()
                            .map(|failure| format!("{}: {failure}", snapshot.name)),
                    );
                }
                record["snapshots"] = snapshots
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "png": s.png,
                            "text_file": s.text_path,
                            "text": s.text,
                            "failures": s.failures,
                        })
                    })
                    .collect();
            }
            Err(error) => {
                problems.push(error.clone());
                record["error"] = json!(error);
            }
        }
        let status = match (xfail, problems.is_empty()) {
            (None, true) => "pass",
            (None, false) => "fail",
            (Some(_), false) => "xfail",
            (Some(_), true) => "xpass",
        };
        record["status"] = json!(status);
        match status {
            "fail" => {
                failed += 1;
                println!("FAIL  {label}");
                for problem in &problems {
                    println!("      {problem}");
                }
            }
            "xfail" => {
                xfailed += 1;
                println!("XFAIL {label}: {}", xfail.unwrap_or_default());
            }
            "xpass" => {
                failed += 1;
                println!("XPASS {label}: passed despite xfail; remove the marker");
            }
            _ => {}
        }
        summary.push(record);
    }
    let summary_path = context
        .output
        .join(format!("{}-summary.json", context.platform));
    fs::write(&summary_path, serde_json::to_string_pretty(&summary)?)?;
    println!(
        "{} runs: {} passed, {xfailed} expected failures, {failed} failed; summary in {}",
        runs.len(),
        runs.len() - failed - xfailed,
        summary_path.display()
    );
    Ok(failed == 0)
}

/// Runs every capture on `jobs` worker threads, keeping the results in
/// `runs` order.
fn run_all(
    runs: &[Run],
    context: &Context,
    jobs: usize,
) -> Vec<std::result::Result<Vec<SnapshotResult>, String>> {
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..jobs.min(runs.len()) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::SeqCst);
                let Some(run) = runs.get(index) else { break };
                let label = format!("{}/{}", run.case.name, run.stem(&context.platform));
                let result = capture(run, index, context).map_err(|error| error.to_string());
                match &result {
                    Ok(snapshots) => {
                        for snapshot in snapshots {
                            println!("captured {}", snapshot.png.display());
                        }
                    }
                    Err(_) => println!("error    {label}"),
                }
                results.lock().unwrap().push((index, result));
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

fn capture(run: &Run, index: usize, context: &Context) -> Result<Vec<SnapshotResult>> {
    let case_dir = context.output.join(&run.case.name);
    fs::create_dir_all(&case_dir)?;
    let stem = run.stem(&context.platform);
    let tape_path = case_dir.join(format!("{stem}.tape"));
    let log_path = case_dir.join(format!("{stem}.log"));
    let frames_path = case_dir.join(format!("{stem}.frames.txt"));
    let _ = fs::remove_file(&frames_path);

    let fixture = Fixture::prepare(run, index, context)?;
    let tape = render_tape(run, &fixture, context, &case_dir, &frames_path);
    let result = tape.and_then(|tape| {
        fs::write(&tape_path, &tape.text)?;
        run_vhs(
            &context.vhs,
            &tape_path,
            &log_path,
            &context.superline,
            &case_dir,
        )?;
        collect_snapshots(&tape, &frames_path)
    });
    fixture.remove();
    result.map_err(|error| {
        format!(
            "{error}\nvhs log ({}):\n{}",
            log_path.display(),
            log_tail(&log_path)
        )
        .into()
    })
}

fn parse_args() -> Result<Args> {
    let mut values = env::args_os().skip(1);
    let mut args = Args {
        output: PathBuf::from("target/terminal-snapshots"),
        vhs: env::var_os("SUPERLINE_E2E_VHS").map(PathBuf::from),
        jobs: 2,
        ..Args::default()
    };

    while let Some(arg) = values.next() {
        let Some(flag) = arg.to_str() else {
            return Err(format!("unknown argument {}", arg.to_string_lossy()).into());
        };
        match flag {
            "--shell" => {
                let value = next_utf8(&mut values, flag)?;
                if value == "all" {
                    args.shells.extend(Shell::ALL);
                } else {
                    args.shells.push(Shell::parse(&value)?);
                }
            }
            "--case" => args.case_names.push(next_utf8(&mut values, flag)?),
            "--cases" => args.cases_file = Some(PathBuf::from(next_value(&mut values, flag)?)),
            "--list" => args.list = true,
            "--columns" => args.columns.push(next_number(&mut values, flag)?),
            "--rows" => args.rows = Some(next_number(&mut values, flag)?),
            "--workdir" => args.workdir = Some(PathBuf::from(next_value(&mut values, flag)?)),
            "--env" => {
                let value = next_utf8(&mut values, flag)?;
                let (key, value) = value.split_once('=').ok_or("--env takes KEY=VALUE")?;
                args.env.insert(key.to_string(), value.to_string());
            }
            "--config" => args.config = Some(PathBuf::from(next_value(&mut values, flag)?)),
            "--name" => args.name = Some(next_utf8(&mut values, flag)?),
            "--run" => args
                .steps
                .push(Step::Run(Input::Text(next_utf8(&mut values, flag)?))),
            "--exit" => args.steps.push(Step::Run(Input::Exit {
                exit: next_number(&mut values, flag)?,
            })),
            "--type" => args
                .steps
                .push(Step::Type(Input::Text(next_utf8(&mut values, flag)?))),
            "--key" => args.steps.push(Step::Key(next_utf8(&mut values, flag)?)),
            "--wait" => args.steps.push(Step::Wait(next_utf8(&mut values, flag)?)),
            "--sleep" => args.steps.push(Step::Sleep(next_utf8(&mut values, flag)?)),
            "--snapshot" => args.steps.push(Step::Snapshot(Snapshot::Name(next_utf8(
                &mut values,
                flag,
            )?))),
            "--output" => args.output = PathBuf::from(next_value(&mut values, flag)?),
            "--vhs" => args.vhs = Some(PathBuf::from(next_value(&mut values, flag)?)),
            "--jobs" => args.jobs = next_number(&mut values, flag)?,
            "--print" => args.print = true,
            "--require-all" => args.require_all = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument {flag}\n\n{USAGE}").into()),
        }
    }

    if args.shells.is_empty() {
        args.shells.extend(Shell::ALL);
    }
    args.shells.sort_by_key(|shell| shell.name());
    args.shells.dedup();
    if args.ad_hoc() && (args.cases_file.is_some() || !args.case_names.is_empty()) {
        return Err("--case/--cases cannot be combined with an ad-hoc case".into());
    }
    if args.ad_hoc() {
        args.print = true;
    }
    args.config = args.config.map(std::path::absolute).transpose()?;
    args.workdir = args.workdir.map(std::path::absolute).transpose()?;
    Ok(args)
}

/// The cases to run and the directory their relative config paths resolve
/// against.
fn load_cases(args: &Args) -> Result<(Vec<Case>, PathBuf)> {
    if args.ad_hoc() {
        let mut steps = args.steps.clone();
        if !steps.iter().any(|step| matches!(step, Step::Snapshot(_))) {
            steps = auto_snapshots(steps);
        }
        let case = Case {
            name: args.name.clone().unwrap_or_else(|| "adhoc".to_string()),
            description: None,
            config: args
                .config
                .as_ref()
                .map(|path| Value::String(path.to_string_lossy().into_owned())),
            columns: None,
            rows: None,
            shells: None,
            platforms: None,
            dir: None,
            env: BTreeMap::new(),
            xfail: None,
            steps,
        };
        case.validate()?;
        return Ok((vec![case], env::current_dir()?));
    }

    let (contents, base_dir) = match &args.cases_file {
        Some(path) => (
            fs::read_to_string(path)
                .map_err(|error| format!("could not read {}: {error}", path.display()))?,
            std::path::absolute(path)?
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
        ),
        None => (
            DEFAULT_CASES.to_string(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/terminal-snapshot"),
        ),
    };
    let cases: Vec<Case> = serde_json::from_str(&contents)
        .map_err(|error| format!("invalid case manifest: {error}"))?;
    for case in &cases {
        case.validate()?;
        if cases.iter().filter(|other| other.name == case.name).count() > 1 {
            return Err(format!("duplicate case {:?}", case.name).into());
        }
    }
    if args.case_names.is_empty() {
        return Ok((cases, base_dir));
    }
    let mut selected = Vec::new();
    for name in &args.case_names {
        let case = cases
            .iter()
            .find(|case| &case.name == name)
            .ok_or_else(|| format!("no case named {name:?} (see --list)"))?;
        selected.push(case.clone());
    }
    Ok((selected, base_dir))
}

/// Ad-hoc steps with a snapshot after the first prompt and after every
/// command, waiting for each command to show up on screen first.
fn auto_snapshots(steps: Vec<Step>) -> Vec<Step> {
    let snapshot = |n: usize| Step::Snapshot(Snapshot::Name(n.to_string()));
    let mut out = vec![snapshot(0)];
    let mut count = 0;
    for step in steps {
        let is_run = matches!(step, Step::Run(_));
        out.push(step);
        if is_run {
            count += 1;
            out.push(Step::Wait("{{last}}".to_string()));
            out.push(snapshot(count));
        }
    }
    out
}

fn next_value(values: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<OsString> {
    values
        .next()
        .ok_or_else(|| format!("{flag} requires a value").into())
}

fn next_utf8(values: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<String> {
    next_value(values, flag)?
        .into_string()
        .map_err(|_| format!("{flag} must be valid UTF-8").into())
}

fn next_number<T: std::str::FromStr>(
    values: &mut impl Iterator<Item = OsString>,
    flag: &str,
) -> Result<T> {
    let value = next_utf8(values, flag)?;
    value
        .parse()
        .map_err(|_| format!("{flag} takes a number, not {value:?}").into())
}

fn superline_binary() -> Result<PathBuf> {
    let executable = env::current_exe()?;
    let profile_dir = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("snapshot executable has no target profile directory")?;
    Ok(profile_dir.join(format!("superline{}", env::consts::EXE_SUFFIX)))
}

fn locate_vhs(requested: Option<&Path>) -> Result<PathBuf> {
    match requested {
        // VHS runs with the output directory as its working directory, so a
        // relative binary path must be resolved first.
        Some(path) if path.is_file() => Ok(std::path::absolute(path)?),
        Some(path) => Err(format!("vhs binary {} does not exist", path.display()).into()),
        None => find_executable("vhs").ok_or_else(|| {
            "vhs is not on PATH; pass --vhs or set SUPERLINE_E2E_VHS (see docs/terminal-snapshots.md)"
                .into()
        }),
    }
}

fn vhs_version(vhs: &Path) -> Result<String> {
    let output = Command::new(vhs).arg("--version").output()?;
    if !output.status.success() {
        return Err(format!(
            "`{} --version` failed: {}",
            vhs.display(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn platform_name() -> String {
    env::var("RUNNER_OS")
        .unwrap_or_else(|_| env::consts::OS.to_string())
        .to_ascii_lowercase()
}

/// Isolated home and working directory for one shell session.
struct Fixture {
    root: PathBuf,
    home: PathBuf,
    dir: PathBuf,
    init: PathBuf,
}

impl Fixture {
    fn prepare(run: &Run, index: usize, context: &Context) -> Result<Self> {
        let root = env::temp_dir().join(format!(
            "superline-terminal-snapshot-{}-{index}",
            std::process::id(),
        ));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        let home = root.join("home");
        let dir = match &context.workdir {
            Some(workdir) if workdir.is_dir() => workdir.clone(),
            Some(workdir) => {
                return Err(format!("--workdir {} is not a directory", workdir.display()).into())
            }
            None => root.join(run.case.dir.as_deref().unwrap_or(DEFAULT_DIR)),
        };
        let config_dir = home.join(".config/superline");
        fs::create_dir_all(&config_dir)?;
        fs::create_dir_all(home.join(".cache"))?;
        fs::create_dir_all(&dir)?;
        let config = config_dir.join("config.json");
        match &run.case.config {
            None | Some(Value::Null) => fs::write(&config, DEFAULT_CONFIG)?,
            Some(Value::String(path)) => copy_config(&run.base_dir.join(path), &config_dir)?,
            Some(inline @ Value::Object(_)) => {
                fs::write(&config, serde_json::to_string_pretty(inline)?)?
            }
            Some(other) => {
                return Err(format!(
                    "case {:?}: config must be a path or an object, not {other}",
                    run.case.name
                )
                .into())
            }
        }

        let shell = run.shell;
        let output = Command::new(&context.superline)
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
            let config = forward_slashes(&config).replace('\'', "''");
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

/// Copy a user config into the fixture, along with a theme file it names
/// relative to its own directory.
fn copy_config(source: &Path, config_dir: &Path) -> Result<()> {
    let contents = fs::read_to_string(source)
        .map_err(|error| format!("could not read config {}: {error}", source.display()))?;
    fs::write(config_dir.join("config.json"), &contents)?;
    let parsed: Value = serde_json::from_str(&contents)
        .map_err(|error| format!("config {} is not valid JSON: {error}", source.display()))?;
    if let Some(theme) = parsed.get("theme").and_then(|theme| theme.as_str()) {
        let theme_file = source.parent().unwrap_or(Path::new(".")).join(theme);
        if theme_file.is_file() {
            fs::copy(&theme_file, config_dir.join(theme))?;
        }
    }
    Ok(())
}

/// A generated tape and the snapshot each `Screenshot` command belongs to.
struct Tape {
    text: String,
    /// For each snapshot, in step order: its spec, PNG path, and the
    /// placeholder values in effect when it was taken.
    snapshots: Vec<(SnapshotSpec, PathBuf, Placeholders)>,
}

fn render_tape(
    run: &Run,
    fixture: &Fixture,
    context: &Context,
    case_dir: &Path,
    frames_path: &Path,
) -> Result<Tape> {
    let shell = run.shell;
    let mut env = run.case.env.clone();
    env.extend(context.extra_env.clone());
    let setup = shell.setup_command(
        &forward_slashes(&fixture.home),
        &forward_slashes(&fixture.init),
        &forward_slashes(&fixture.dir),
        &env,
    );
    let mut placeholders = Placeholders {
        shell: shell.name(),
        columns: run.columns,
        rows: run.rows,
        last: String::new(),
    };
    let mut lines = Vec::new();
    let mut snapshots = Vec::new();
    for step in &run.case.steps {
        match step {
            Step::Run(input) | Step::Type(input) => {
                let text = placeholders.fill(&input.resolve(shell)?);
                lines.push(format!("Type {}", tape_string(&text)?));
                if matches!(step, Step::Run(_)) {
                    lines.push("Enter".to_string());
                }
                placeholders.last = regex::escape(&text);
            }
            Step::Key(key) => lines.push(key.clone()),
            Step::Wait(pattern) => lines.push(format!(
                "Wait+Screen /{}/",
                tape_regex(&placeholders.fill(pattern))
            )),
            Step::WaitLine(pattern) => lines.push(format!(
                "Wait+Line /{}/",
                tape_regex(&placeholders.fill(pattern))
            )),
            Step::Sleep(duration) => lines.push(format!("Sleep {duration}")),
            Step::Snapshot(snapshot) => {
                let spec = snapshot.spec();
                let png =
                    case_dir.join(format!("{}-{}.png", run.stem(&context.platform), spec.name));
                let _ = fs::remove_file(&png);
                // Settle before the capture, and give VHS a frame after it:
                // a screenshot is written from the next recorded frame.
                lines.push("Sleep 1s".to_string());
                lines.push(format!(
                    "Screenshot {}",
                    tape_string(&forward_slashes(&png))?
                ));
                lines.push("Sleep 1s".to_string());
                snapshots.push((spec, png, placeholders.clone()));
            }
        }
    }

    let text = TAPE_TEMPLATE
        .replace("{{frames}}", &forward_slashes(frames_path))
        .replace("{{shell}}", shell.name())
        .replace("{{columns}}", &run.columns.to_string())
        .replace("{{rows}}", &run.rows.to_string())
        .replace("{{setup}}", &tape_string(&setup)?)
        .replace("{{steps}}", &lines.join("\n"));
    Ok(Tape { text, snapshots })
}

/// Quote a string for a VHS `Type` or path argument, using whichever of its
/// three delimiters the text does not contain.
fn tape_string(text: &str) -> Result<String> {
    if text.contains('\n') {
        return Err(format!("{text:?} spans lines; use separate steps").into());
    }
    ['"', '\'', '`']
        .into_iter()
        .find(|quote| !text.contains(*quote))
        .map(|quote| format!("{quote}{text}{quote}"))
        .ok_or_else(|| format!("{text:?} uses all of VHS's quote characters").into())
}

/// Escape the slash delimiter of a `/.../` regex in the tape, and literal
/// line breaks, which would end the tape command.
fn tape_regex(pattern: &str) -> String {
    pattern
        .replace('/', "\\/")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// Split VHS's text output into screens and check each snapshot's PNG and
/// text. Once recording is shown, VHS dumps the screen after every command,
/// so the dump for a snapshot is the one written right after its
/// `Screenshot`. How many dumps the hidden setup writes varies, so indexes
/// count back from the end of the file.
fn collect_snapshots(tape: &Tape, frames_path: &Path) -> Result<Vec<SnapshotResult>> {
    let frames_text = fs::read_to_string(frames_path)
        .map_err(|error| format!("vhs did not write {}: {error}", frames_path.display()))?;
    let mut frames: Vec<String> = frames_text
        .split(&format!("{FRAME_SEPARATOR}\n"))
        .map(|frame| frame.trim_end_matches('\n').to_string())
        .collect();
    // The text after the last separator is empty.
    frames.pop();

    let mut recorded = None;
    let mut screenshot_offsets = Vec::new();
    for line in tape.text.lines().map(str::trim) {
        let command = !(line.is_empty() || line.starts_with('#'));
        match recorded.as_mut() {
            None if line == "Show" => recorded = Some(0),
            Some(count) if command => {
                *count += 1;
                if line.starts_with("Screenshot ") {
                    screenshot_offsets.push(*count);
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
    let screenshot_frames = screenshot_offsets.into_iter().map(|offset| first + offset);

    let mut results = Vec::new();
    for ((spec, png, placeholders), frame) in tape.snapshots.iter().zip(screenshot_frames) {
        let text = frames[frame].clone();
        let text_path = png.with_extension("txt");
        fs::write(&text_path, format!("{text}\n"))?;
        let mut failures = Vec::new();
        let size = fs::metadata(png).map(|m| m.len()).unwrap_or(0);
        if size == 0 {
            failures.push(format!("vhs did not write {}", png.display()));
        }
        failures.extend(check_text(&text, spec, placeholders));
        results.push(SnapshotResult {
            name: spec.name.clone(),
            png: png.clone(),
            text_path,
            text,
            failures,
        });
    }
    Ok(results)
}

fn run_vhs(
    vhs: &Path,
    tape: &Path,
    log_path: &Path,
    superline: &Path,
    working_dir: &Path,
) -> Result<()> {
    let log = fs::File::create(log_path)?;
    let mut child = Command::new(vhs)
        .arg(tape)
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .env("LANG", "en_US.UTF-8")
        .env("LC_ALL", "en_US.UTF-8")
        .env("PATH", path_with_binary(superline)?)
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

fn log_tail(path: &Path) -> String {
    let log = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    let start = lines.len().saturating_sub(40);
    lines[start..].join("\n")
}

/// Paths typed into the shell or written into the tape use forward slashes,
/// which PowerShell, nushell, and VHS accept on Windows as well.
fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn path_with_binary(superline: &Path) -> Result<OsString> {
    let bin_dir = superline.parent().ok_or("superline binary has no parent")?;
    let mut paths = vec![bin_dir.to_path_buf()];
    if let Some(path) = env::var_os("PATH") {
        paths.extend(env::split_paths(&path));
    }
    Ok(env::join_paths(paths)?)
}

fn find_executable(name: &str) -> Option<PathBuf> {
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
