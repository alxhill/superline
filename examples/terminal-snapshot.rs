//! Capture superline prompts from real interactive shells through VHS.
//!
//! The example prepares an isolated home directory with a deterministic
//! config, loads `superline init <shell>` into the shell, and drives the
//! session with a VHS tape generated from `terminal-snapshot/tape.template`.
//! VHS owns the PTY (ConPTY on Windows), the terminal emulation, and the
//! rendering; this file only wires up fixtures and checks that every expected
//! PNG exists afterwards.

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CONFIG: &str = include_str!("terminal-snapshot/config.json");
const TAPE_TEMPLATE: &str = include_str!("terminal-snapshot/tape.template");
const FIXTURE_DIR: &str = "superline-e2e";
const FAILURE_STATUS: &str = "7";
const VHS_TIMEOUT: Duration = Duration::from_secs(240);
/// Powerline separator and success chevron, as RE2 escapes for the tape.
const SEPARATOR: &str = r"\x{E0B0}";
const SUCCESS_MARK: &str = r"\x{F105}";

type Result<T> = std::result::Result<T, Box<dyn Error>>;

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

    /// One hidden command line that loads the init snippet, enters the
    /// fixture directory, and clears the setup output.
    fn setup_command(self, init: &str, fixture: &str) -> String {
        match self {
            Self::Bash | Self::Zsh => format!("source '{init}' && cd '{fixture}' && clear"),
            Self::Fish => format!("source '{init}'; and cd '{fixture}'; and clear"),
            Self::Pwsh => format!(". '{init}'; Set-Location '{fixture}'; Clear-Host"),
            Self::Nu => format!("source '{init}'; cd '{fixture}'; clear"),
        }
    }

    /// A visible command that makes the shell report exit status 7.
    fn failure_command(self) -> &'static str {
        match (self, cfg!(windows)) {
            (Self::Bash | Self::Zsh, _) => "(exit 7)",
            (Self::Fish, _) => "sh -c 'exit 7'",
            (Self::Pwsh, true) => "cmd /c exit 7",
            (Self::Pwsh, false) => "& sh -c 'exit 7'",
            (Self::Nu, true) => "^cmd /c exit 7",
            (Self::Nu, false) => "^sh -c 'exit 7'",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scenario {
    Clean,
    Failure,
}

impl Scenario {
    const ALL: [Self; 2] = [Self::Clean, Self::Failure];

    fn name(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Failure => "failure",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "clean" => Ok(Self::Clean),
            "failure" => Ok(Self::Failure),
            _ => Err(format!("unsupported scenario {value:?}").into()),
        }
    }
}

#[derive(Debug)]
struct Args {
    shells: Vec<Shell>,
    scenarios: Vec<Scenario>,
    output: PathBuf,
    vhs: Option<PathBuf>,
    require_all: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("terminal snapshot failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = parse_args()?;
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

    let mut captured = 0;
    for shell in args.shells {
        if find_executable(shell.name()).is_none() {
            let message = format!("{} is not on PATH", shell.name());
            if args.require_all {
                return Err(message.into());
            }
            eprintln!("skipping {}: {message}", shell.name());
            continue;
        }

        let fixture = Fixture::prepare(shell, &superline)?;
        let stem = format!("{platform}-{}", shell.name());
        let tape_path = output.join(format!("{stem}.tape"));
        let log_path = output.join(format!("{stem}.log"));
        fs::write(
            &tape_path,
            render_tape(shell, &args.scenarios, &fixture, &output, &platform),
        )?;

        let result = run_vhs(&vhs, &tape_path, &log_path, &fixture, &superline, &output)
            .and_then(|()| verify_screenshots(shell, &args.scenarios, &output, &platform));
        let _ = fs::remove_dir_all(&fixture.root);
        if let Err(error) = result {
            return Err(format!(
                "{} capture failed: {error}\nvhs log ({}):\n{}",
                shell.name(),
                log_path.display(),
                log_tail(&log_path)
            )
            .into());
        }
        captured += args.scenarios.len();
    }

    if captured == 0 {
        return Err("no requested shell was available".into());
    }
    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut values = env::args_os().skip(1);
    let mut shells = Vec::new();
    let mut scenarios = Vec::new();
    let mut output = PathBuf::from("target/terminal-snapshots");
    let mut vhs = env::var_os("SUPERLINE_E2E_VHS").map(PathBuf::from);
    let mut require_all = false;

    while let Some(arg) = values.next() {
        match arg.to_str() {
            Some("--shell") => {
                let value = next_utf8(&mut values, "--shell")?;
                if value == "all" {
                    shells.extend(Shell::ALL);
                } else {
                    shells.push(Shell::parse(&value)?);
                }
            }
            Some("--scenario") => {
                let value = next_utf8(&mut values, "--scenario")?;
                if value == "all" {
                    scenarios.extend(Scenario::ALL);
                } else {
                    scenarios.push(Scenario::parse(&value)?);
                }
            }
            Some("--output") => output = PathBuf::from(next_value(&mut values, "--output")?),
            Some("--vhs") => vhs = Some(PathBuf::from(next_value(&mut values, "--vhs")?)),
            Some("--require-all") => require_all = true,
            Some("-h" | "--help") => {
                println!(
                    "Usage: cargo run --example terminal-snapshot -- [--shell <all|bash|zsh|fish|pwsh|nu>]... [--scenario <all|clean|failure>]... [--output <DIR>] [--vhs <VHS_BINARY>] [--require-all]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument {}", arg.to_string_lossy()).into()),
        }
    }

    if shells.is_empty() {
        shells.extend(Shell::ALL);
    }
    shells.sort_by_key(|shell| shell.name());
    shells.dedup();
    if scenarios.is_empty() {
        scenarios.extend(Scenario::ALL);
    }
    scenarios.sort_by_key(|scenario| scenario.name());
    scenarios.dedup();

    Ok(Args {
        shells,
        scenarios,
        output,
        vhs,
        require_all,
    })
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
        Some(path) if path.is_file() => Ok(path.to_path_buf()),
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
    fn prepare(shell: Shell, superline: &Path) -> Result<Self> {
        let root = env::temp_dir().join(format!(
            "superline-terminal-snapshot-{}-{}",
            std::process::id(),
            shell.name()
        ));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        let home = root.join("home");
        let dir = root.join(FIXTURE_DIR);
        let config_dir = home.join(".config/superline");
        fs::create_dir_all(&config_dir)?;
        fs::create_dir_all(home.join(".cache"))?;
        fs::create_dir_all(&dir)?;
        let config = config_dir.join("config.json");
        fs::write(&config, CONFIG)?;

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
}

fn render_tape(
    shell: Shell,
    scenarios: &[Scenario],
    fixture: &Fixture,
    output: &Path,
    platform: &str,
) -> String {
    let name = shell.name();
    let setup = shell.setup_command(
        &forward_slashes(&fixture.init),
        &forward_slashes(&fixture.dir),
    );
    let clean_prompt = format!("{name}{SEPARATOR}{SUCCESS_MARK}{SEPARATOR}");
    let failure_prompt = format!("{name}{SEPARATOR}{FAILURE_STATUS}{SEPARATOR}");
    let screenshot = |scenario: Scenario| {
        let path = output.join(format!("{platform}-{name}-{}.png", scenario.name()));
        format!("Screenshot \"{}\"\nSleep 1s\n", forward_slashes(&path))
    };

    let clean_screenshot = if scenarios.contains(&Scenario::Clean) {
        screenshot(Scenario::Clean)
    } else {
        String::new()
    };
    let failure_scenario = if scenarios.contains(&Scenario::Failure) {
        let command = shell.failure_command();
        format!(
            "Type \"{command}\"\nEnter\n\
             # The clean prompt and the typed command must survive above the new prompt.\n\
             Wait+Screen /{clean_prompt} {}/\n\
             Wait+Screen /{failure_prompt}/\n\
             Sleep 1s\n{}",
            tape_regex(command),
            screenshot(Scenario::Failure)
        )
    } else {
        String::new()
    };

    TAPE_TEMPLATE
        .replace("{{shell}}", name)
        .replace("{{setup}}", &setup)
        .replace("{{clean_prompt}}", &clean_prompt)
        .replace("{{clean_screenshot}}", &clean_screenshot)
        .replace("{{failure_scenario}}", &failure_scenario)
}

/// Escape a literal for a `/.../` regex in the tape: RE2 metacharacters and
/// the slash delimiter itself.
fn tape_regex(literal: &str) -> String {
    regex::escape(literal).replace('/', "\\/")
}

fn run_vhs(
    vhs: &Path,
    tape: &Path,
    log_path: &Path,
    fixture: &Fixture,
    superline: &Path,
    output: &Path,
) -> Result<()> {
    let log = fs::File::create(log_path)?;
    let mut child = Command::new(vhs)
        .arg(tape)
        .current_dir(output)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .env("HOME", &fixture.home)
        .env("USERPROFILE", &fixture.home)
        .env("XDG_CONFIG_HOME", fixture.home.join(".config"))
        .env("XDG_CACHE_HOME", fixture.home.join(".cache"))
        .env("LANG", "en_US.UTF-8")
        .env("LC_ALL", "en_US.UTF-8")
        .env("PATH", path_with_binary(superline)?)
        .env_remove("PWD")
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

fn verify_screenshots(
    shell: Shell,
    scenarios: &[Scenario],
    output: &Path,
    platform: &str,
) -> Result<()> {
    for scenario in scenarios {
        let path = output.join(format!(
            "{platform}-{}-{}.png",
            shell.name(),
            scenario.name()
        ));
        let size = fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if size == 0 {
            return Err(format!("vhs did not write {}", path.display()).into());
        }
        println!("captured {}", path.display());
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
