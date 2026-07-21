extern crate superline;

use std::fs::{create_dir_all, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use std::{env, io};

use clap::{Args, Parser, Subcommand, ValueEnum};
use thiserror::Error;

use superline::config::{CommandLine, Config, LineSegment, TerminalRuntimeMetadata};
use superline::modules::{refresh_git, refresh_pr};
use superline::terminal::{Shell, SHELL};
use superline::themes::{CustomTheme, CustomThemeError, RainbowTheme, SimpleTheme};
use superline::Powerline;

const FISH_CONF: &str = r#"
set -gx SUPERLINE_FISH 1

function __pl_cache_duration --on-event fish_postexec
  set -gx __pl_duration $CMD_DURATION
end

function fish_prompt
  superline show -s $status -c $COLUMNS fish $__pl_duration
end

function fish_right_prompt
  superline show-right -s $status -c $COLUMNS fish $__pl_duration
end
"#;

const FISH_INSTALL: &str = r#"
# automatically added by superline
superline init fish | source
"#;

const ZSH_CONF: &str = r#"
export SUPERLINE_ZSH=1

function preexec() {
    if command -v gdate >/dev/null 2>&1; then
        __pl_timer=$(($(gdate +%s%0N)/1000000))
    fi
}

function _update_ps1() {
    if [ $__pl_timer ]; then
        _now=$(($(gdate +%s%0N)/1000000))
        if [ $_now -ge $__pl_timer ]; then
            _elapsed=$(($_now-$__pl_timer))
        fi
    fi
    PS1="$(superline show -s $? -c $COLUMNS zsh $_elapsed)"
    RPS1="$(superline show-right -s $? -c $COLUMNS zsh $_elapsed)"
    unset __pl_timer _elapsed _now
}

precmd_functions=(_update_ps1)
"#;

const ZSH_INSTALL: &str = r#"
# automatically added by superline
source <(superline init zsh)
"#;

// note: does not support showing last cmd duration
const BASH_CONF: &str = r#"
export SUPERLINE_BASH=1

function _update_ps1() {
    PS1="$(superline show -s $? -c $COLUMNS bash)"
}

if [ "$TERM" != "linux" ]; then
    PROMPT_COMMAND="_update_ps1; $PROMPT_COMMAND"
fi
"#;

const BASH_INSTALL: &str = r#"
# automatically added by superline
source <(superline init bash)
"#;

const PWSH_CONF: &str = r#"
$env:SUPERLINE_PWSH = 1

# superline emits UTF-8: powerline separators and Nerd Font icons live in the
# Unicode private-use area. PowerShell decodes a native command's stdout using
# [Console]::OutputEncoding, which on Windows defaults to the legacy OEM code
# page - so the bytes get re-read as that code page and the glyphs turn to
# mojibake (e.g. the U+E0B0 separator shows as "ee 82 b0" decoded to "εé░").
# Force UTF-8 so the captured output decodes correctly. Wrapped in try/catch
# for hosts without a real console (remoting, redirected output, some IDEs).
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

function global:prompt {
    # Capture command state first: every statement below (even an assignment)
    # resets $?, so read it before anything else - including before reading
    # $LASTEXITCODE, which a plain assignment would otherwise flip back to true.
    $__pl_ok = $?
    $__pl_exit = $LASTEXITCODE

    # Mirror the bash/zsh convention: 0 on success, otherwise the native exit
    # code (falling back to 1 for cmdlet failures that leave $LASTEXITCODE unset).
    if ($__pl_ok) {
        $__pl_status = 0
    } elseif ($__pl_exit) {
        $__pl_status = $__pl_exit
    } else {
        $__pl_status = 1
    }

    $__pl_cols = 0
    try { $__pl_cols = $Host.UI.RawUI.WindowSize.Width } catch {}
    if (-not $__pl_cols -or $__pl_cols -le 0) { $__pl_cols = 80 }

    $__pl_args = @('show', '-s', $__pl_status, '-c', $__pl_cols, 'pwsh')

    # Duration of the last command, in milliseconds, from session history.
    $__pl_last = Get-History -Count 1
    if ($__pl_last) {
        $__pl_ms = [long][math]::Round(($__pl_last.EndExecutionTime - $__pl_last.StartExecutionTime).TotalMilliseconds)
        if ($__pl_ms -ge 0) { $__pl_args += $__pl_ms }
    }

    # Join lines with `n (not Out-String, which can wrap/pad to the host width).
    $__pl_out = (& superline @__pl_args) -join "`n"

    # Restore $LASTEXITCODE so our own commands don't clobber the user's value.
    $global:LASTEXITCODE = $__pl_exit

    $__pl_out
}
"#;

const PWSH_INSTALL: &str = r#"
# automatically added by superline
(& superline init pwsh) -join "`n" | Invoke-Expression
"#;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
enum PowerlineArgs {
    #[command(subcommand)]
    Init(ShellSubcommand),
    Show(ShowArgs),
    ShowRight(ShowArgs),
    Install(InstallArgs),
    Config,
    /// Internal: refresh the cached PR lookup for a branch. Spawned in the
    /// background by the `pr` module - not intended to be called by hand.
    #[command(hide = true)]
    RefreshPr(RefreshPrArgs),
    /// Internal: refresh cached git status after a render timeout. Spawned in
    /// the background by the `git` module - not intended to be called by hand.
    #[command(hide = true)]
    RefreshGit(RefreshGitArgs),
}

#[derive(Debug, Clone, Subcommand)]
enum ShellSubcommand {
    Bash,
    Zsh,
    Fish,
    Pwsh,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum ShellArg {
    Bash,
    Zsh,
    Fish,
    Pwsh,
}

impl ShellArg {
    fn name(&self) -> &'static str {
        match self {
            ShellArg::Bash => "bash",
            ShellArg::Zsh => "zsh",
            ShellArg::Fish => "fish",
            ShellArg::Pwsh => "pwsh",
        }
    }
}

#[derive(Debug, Args)]
struct ShowArgs {
    #[arg(value_enum)]
    shell: ShellArg,
    // not an arg to allow passing a variable that may be empty
    duration: Option<u64>,
    #[arg(short, long)]
    columns: usize,
    #[arg(short, long)]
    status: String,
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct RefreshPrArgs {
    #[arg(long)]
    branch: String,
    #[arg(long)]
    repo_dir: PathBuf,
    #[arg(long)]
    cache: PathBuf,
}

#[derive(Debug, Args)]
struct RefreshGitArgs {
    #[arg(long)]
    repo_dir: PathBuf,
    #[arg(long)]
    cache: PathBuf,
}

#[derive(Debug, Args)]
struct InstallArgs {
    #[arg(value_enum)]
    shell: ShellArg,
    #[arg(long, action)]
    force: bool,
}

impl TerminalRuntimeMetadata for &ShowArgs {
    fn shell_name(&self) -> String {
        self.shell.name().to_string()
    }

    fn total_columns(&self) -> usize {
        self.columns
    }

    fn last_command_duration(&self) -> Option<Duration> {
        self.duration.map(Duration::from_millis)
    }

    fn last_command_status(&self) -> &str {
        self.status.as_str()
    }
}

fn main() {
    let args = PowerlineArgs::parse();

    match args {
        PowerlineArgs::Init(shell) => print_shell_conf(shell),
        PowerlineArgs::Show(args) => show(args, false),
        PowerlineArgs::ShowRight(args) => show(args, true),
        PowerlineArgs::Install(args) => install(args),
        PowerlineArgs::Config => open_config(),
        PowerlineArgs::RefreshPr(args) => refresh_pr(&args.branch, &args.repo_dir, &args.cache),
        PowerlineArgs::RefreshGit(args) => refresh_git(&args.repo_dir, &args.cache),
    }
}

fn install(args: InstallArgs) {
    let shell = args.shell;

    let (conf_path, conf_contents) = match shell {
        ShellArg::Fish => (home_config(".config/fish/config.fish"), FISH_INSTALL),
        ShellArg::Zsh => (home_config(".zshrc"), ZSH_INSTALL),
        ShellArg::Bash => (home_config(".bashrc"), BASH_INSTALL),
        ShellArg::Pwsh => (powershell_profile_path(), PWSH_INSTALL),
    };

    // Skip re-appending when the snippet is already present. This replaces the
    // old guard, which checked the `SUPERLINE_<SHELL>` runtime marker - a marker
    // only set once the snippet has been *sourced*, never in the shell that runs
    // `install` itself. As a result repeated installs (e.g. when the first one
    // wrote to a profile the user's shell doesn't load) stacked duplicate blocks
    // instead of being recognised as already done.
    if !args.force && already_installed(&conf_path, shell) {
        println!(
            "superline already installed for {} in {}",
            shell.name(),
            conf_path.display()
        );
        return;
    }

    println!("Installing superline for {} shell", shell.name());
    append_conf(&conf_path, conf_contents);
    println!(
        "Done - added superline to {}.\nPlease restart your shell for changes to take effect.",
        conf_path.display()
    );
}

/// Whether `conf_path` already contains superline's init line for `shell`, used
/// to keep `install` idempotent. A missing or unreadable file counts as "not
/// installed" so the install proceeds and surfaces any real error on write.
fn already_installed(conf_path: &Path, shell: ShellArg) -> bool {
    std::fs::read_to_string(conf_path)
        .map(|contents| contents_have_install(&contents, shell))
        .unwrap_or(false)
}

/// Whether `contents` already includes the `superline init <shell>` invocation
/// that the install snippet adds.
fn contents_have_install(contents: &str, shell: ShellArg) -> bool {
    contents.contains(&format!("superline init {}", shell.name()))
}

/// Resolve a path inside the user's home directory used by the bash/zsh/fish
/// config files.
fn home_config(rel: &str) -> PathBuf {
    let home_dir = superline::platform::home_dir().expect("could not determine home directory");
    assert!(home_dir.is_dir(), "home directory does not exist");
    home_dir.join(rel)
}

/// Ask PowerShell itself for the current-user profile path: it varies per
/// platform/edition and avoids relying on `$HOME`, which Windows does not set.
///
/// Windows ships two PowerShells side by side - PowerShell Core (`pwsh`, v6+)
/// and Windows PowerShell (`powershell`, v5.1) - and each keeps its profile in
/// a *different* folder (`Documents\PowerShell` vs `Documents\WindowsPowerShell`).
/// We therefore query whichever edition the user ran `install` from first
/// (inferred from `$PSModulePath`) and only fall back to the other when the
/// preferred one isn't on PATH. Querying the wrong edition writes the snippet to
/// a profile the user's shell never loads, so the prompt silently never updates.
fn powershell_profile_path() -> PathBuf {
    let run = |cmd: &str| {
        Command::new(cmd)
            .args(["-NoProfile", "-Command", "$PROFILE.CurrentUserCurrentHost"])
            .output()
    };

    let [preferred, fallback] = powershell_executables();
    let output = run(preferred).or_else(|_| run(fallback)).expect(
        "could not run pwsh/powershell to locate the profile - is PowerShell installed and on PATH?",
    );

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        !path.is_empty(),
        "PowerShell returned an empty profile path"
    );
    PathBuf::from(path)
}

/// The order in which to try the two PowerShell executables when locating the
/// profile, preferring the edition `superline install pwsh` was launched from.
fn powershell_executables() -> [&'static str; 2] {
    if invoked_from_powershell_core(env::var("PSModulePath").ok().as_deref()) {
        ["pwsh", "powershell"]
    } else {
        ["powershell", "pwsh"]
    }
}

/// Heuristic for "was `install` launched from PowerShell Core (`pwsh`, v6+)
/// rather than Windows PowerShell (`powershell`, v5.1)".
///
/// The two editions store modules under differently named folders: Core uses a
/// path component named exactly `PowerShell` (e.g. the install dir
/// `...\powershell\7\Modules` or the user dir `...\Documents\PowerShell\Modules`),
/// whereas Windows PowerShell only ever uses `WindowsPowerShell`. Child
/// processes inherit `$PSModulePath`, so its entries reveal which one launched
/// us. Matching the bare `PowerShell` component (rather than a version number)
/// keeps this correct for PowerShell 8 and beyond, and splitting on both path
/// separators handles the `/`-delimited paths used by `pwsh` on macOS/Linux.
fn invoked_from_powershell_core(ps_module_path: Option<&str>) -> bool {
    ps_module_path
        .unwrap_or("")
        .split(';')
        .flat_map(|entry| entry.split(['\\', '/']))
        .any(|segment| segment.eq_ignore_ascii_case("powershell"))
}

fn append_conf(conf_path: &Path, conf_contents: &str) {
    if let Some(parent) = conf_path.parent() {
        create_dir_all(parent).unwrap_or_else(|e| {
            panic!(
                "could not create config directory {}: {e}",
                parent.display()
            )
        });
    }

    let mut conf = OpenOptions::new()
        .create(true)
        .append(true)
        .open(conf_path)
        .unwrap_or_else(|_| {
            panic!(
                "could not open shell config file: {}",
                conf_path.to_str().unwrap_or("")
            )
        });

    conf.write_all(conf_contents.as_bytes())
        .expect("failed to append to config");
}

fn open_config() {
    let conf = get_or_create_conf_file().unwrap();

    let editor = env::var("EDITOR").unwrap_or("vim".to_string());

    Command::new(editor)
        .arg(conf)
        .status()
        .expect("Failed to get editor exit status");
}

fn print_shell_conf(shell: ShellSubcommand) {
    match shell {
        ShellSubcommand::Bash => println!("{}", BASH_CONF),
        ShellSubcommand::Zsh => println!("{}", ZSH_CONF),
        ShellSubcommand::Fish => println!("{}", FISH_CONF),
        ShellSubcommand::Pwsh => println!("{}", PWSH_CONF),
    }
}

fn show(args: ShowArgs, right_only: bool) {
    match args.shell {
        ShellArg::Bash => SHELL.set(Shell::Bash),
        ShellArg::Zsh => SHELL.set(Shell::Zsh),
        ShellArg::Fish => SHELL.set(Shell::Bare),
        // PowerShell's PSReadLine handles raw ANSI escapes itself, so it
        // uses the same bare escapes as fish (no non-printing markers).
        ShellArg::Pwsh => SHELL.set(Shell::Bare),
    }
    .expect("failed to set shell");

    match load_config(args.config.clone()) {
        Ok((mut conf, conf_root)) => match load_theme(&conf, &conf_root) {
            Ok(theme) => render_prompt(&args, conf, theme, right_only),
            Err(error @ PowerlineError::InvalidTheme(_)) => {
                prepend_error_module(&mut conf, fallback_message(&error));
                render_prompt(&args, conf, LoadedTheme::Rainbow, right_only);
            }
            Err(error) => show_fallback(&args, &error, right_only),
        },
        Err(e) => {
            show_fallback(&args, &e, right_only);
        }
    }
}

fn render_prompt(args: &ShowArgs, conf: Config, theme: LoadedTheme, right_only: bool) {
    if right_only {
        render_right(args, conf, theme);
    } else {
        render_normal(args, conf, theme);
    }
}

fn render_right(args: &ShowArgs, conf: Config, theme: LoadedTheme) {
    if let Some(prompt) = conf.rows.last() {
        let powerline = powerline_from_conf(prompt, args, theme);
        powerline.print_right();
    }
}

fn render_normal(args: &ShowArgs, conf: Config, theme: LoadedTheme) {
    let mut powerlines = conf
        .rows
        .into_iter()
        .map(|prompt| powerline_from_conf(&prompt, args, theme))
        .collect::<Vec<Powerline>>();

    if let Some((last, all_bar_last)) = powerlines.split_last_mut() {
        for powerline in all_bar_last {
            powerline.print_left();
            powerline.print_padding(args.columns);
            powerline.print_right();
            println!();
        }
        // the shell handles printing the final right prompt
        last.print_left();
        println!();
    }
}

#[derive(Clone, Copy)]
enum LoadedTheme {
    Rainbow,
    Simple,
    Custom,
}

fn load_theme(conf: &Config, conf_root: &Path) -> Result<LoadedTheme, PowerlineError> {
    match conf.theme.as_str() {
        "rainbow" => Ok(LoadedTheme::Rainbow),
        "simple" => Ok(LoadedTheme::Simple),
        theme_path => {
            let path = match theme_path.as_bytes() {
                [b'/', ..] => PathBuf::from(theme_path),
                _ => conf_root.join(theme_path),
            };
            CustomTheme::load(&path)?;
            Ok(LoadedTheme::Custom)
        }
    }
}

fn powerline_from_conf(prompt: &CommandLine, args: &ShowArgs, theme: LoadedTheme) -> Powerline {
    match theme {
        LoadedTheme::Rainbow => Powerline::from_conf::<RainbowTheme>(prompt, args),
        LoadedTheme::Simple => Powerline::from_conf::<SimpleTheme>(prompt, args),
        LoadedTheme::Custom => Powerline::from_conf::<CustomTheme>(prompt, args),
    }
}

fn show_fallback(args: &ShowArgs, error: &PowerlineError, right_only: bool) {
    let conf = fallback_config(error);
    render_prompt(args, conf, LoadedTheme::Rainbow, right_only);
}

fn fallback_config(error: &PowerlineError) -> Config {
    let mut conf = Config::default();
    prepend_error_module(&mut conf, fallback_message(error));
    conf
}

fn prepend_error_module(conf: &mut Config, message: String) {
    if let Some(first_row) = conf.rows.first_mut() {
        first_row.left.insert(0, LineSegment::Error { message });
        first_row.left.insert(1, LineSegment::Padding(1));
    }
}

fn fallback_message(error: &PowerlineError) -> String {
    match error {
        PowerlineError::HomeDirNotFound => "home directory not found".to_string(),
        PowerlineError::IoError(_) => "config file not read".to_string(),
        PowerlineError::InvalidConfig(_) => "config file not parsed".to_string(),
        PowerlineError::InvalidTheme(theme_error) => match theme_error {
            CustomThemeError::Open { .. } => "theme file not loaded".to_string(),
            CustomThemeError::Parse { .. } => "theme file not parsed".to_string(),
            CustomThemeError::Invalid { .. } => "theme file invalid".to_string(),
        },
    }
}

#[derive(Error, Debug)]
enum PowerlineError {
    #[error("could not determine home directory")]
    HomeDirNotFound,
    #[error("could not read config file")]
    IoError(#[from] io::Error),
    #[error("config file could not be parsed")]
    InvalidConfig(#[from] serde_json::Error),
    #[error(transparent)]
    InvalidTheme(#[from] CustomThemeError),
}

fn load_config(conf_file: Option<PathBuf>) -> Result<(Config, PathBuf), PowerlineError> {
    let conf_path = match conf_file {
        Some(path) => path,
        None => get_or_create_conf_file()?,
    };
    let conf_file = File::open(&conf_path)?;
    let conf: Config = serde_json::from_reader(conf_file)?;
    Ok((
        conf,
        conf_path.parent().unwrap_or_else(|| Path::new(".")).into(),
    ))
}

fn get_or_create_conf_file() -> Result<PathBuf, PowerlineError> {
    let home_dir = superline::platform::home_dir().ok_or(PowerlineError::HomeDirNotFound)?;
    let config_dir = home_dir.join(".config/superline");
    if !config_dir.exists() {
        create_dir_all(&config_dir)?;
    }

    let conf_file = config_dir.join("config.json");
    if !conf_file.exists() {
        println!(
            "config file not found, creating default conf at {:?}",
            &conf_file
        );
        File::create(&conf_file)?
            .write_all(serde_json::to_string_pretty(&Config::default())?.as_bytes())?;
    }

    Ok(conf_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_powershell_core_from_module_path() {
        // What a child process inherits from pwsh 7 on Windows: a bare
        // `PowerShell` component appears (both the user dir and the install dir).
        let core = r"C:\Users\me\Documents\PowerShell\Modules;C:\Program Files\PowerShell\Modules;c:\program files\powershell\7\Modules;C:\WINDOWS\system32\WindowsPowerShell\v1.0\Modules";
        assert!(invoked_from_powershell_core(Some(core)));
    }

    #[test]
    fn detects_windows_powershell_from_module_path() {
        // Windows PowerShell 5.1 only ever uses `WindowsPowerShell` components.
        let desktop = r"C:\Users\me\Documents\WindowsPowerShell\Modules;C:\Program Files\WindowsPowerShell\Modules;C:\WINDOWS\system32\WindowsPowerShell\v1.0\Modules";
        assert!(!invoked_from_powershell_core(Some(desktop)));
    }

    #[test]
    fn unknown_module_path_is_not_treated_as_core() {
        // No signal -> fall back to the Windows-PowerShell-first order, which
        // still resolves correctly via `.or_else` if only `pwsh` exists.
        assert!(!invoked_from_powershell_core(None));
        assert!(!invoked_from_powershell_core(Some("")));
        assert!(!invoked_from_powershell_core(Some(
            r"C:\Some\Other\Modules"
        )));
    }

    #[test]
    fn detects_core_with_unix_style_paths() {
        // `pwsh` on macOS/Linux uses `/`-delimited, `:`-joined module paths; the
        // inner separator split still finds the `powershell` component.
        let core = "/home/me/.local/share/powershell/Modules:/usr/local/share/powershell/Modules";
        assert!(invoked_from_powershell_core(Some(core)));
    }

    #[test]
    fn install_detection_matches_each_shell_snippet() {
        // The real install snippets must be recognised by the idempotency check.
        assert!(contents_have_install(BASH_INSTALL, ShellArg::Bash));
        assert!(contents_have_install(ZSH_INSTALL, ShellArg::Zsh));
        assert!(contents_have_install(FISH_INSTALL, ShellArg::Fish));
        assert!(contents_have_install(PWSH_INSTALL, ShellArg::Pwsh));
    }

    #[test]
    fn install_detection_is_shell_specific_and_absent_by_default() {
        assert!(!contents_have_install(PWSH_INSTALL, ShellArg::Bash));
        assert!(!contents_have_install(
            "# an unrelated profile\n",
            ShellArg::Pwsh
        ));
        assert!(!contents_have_install("", ShellArg::Zsh));
    }
}
