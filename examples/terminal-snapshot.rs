//! Capture superline prompts from real interactive shells through VHS: pass
//! a config (and optionally a tape), get back PNGs and screen text.
//!
//! The checked cases live in `tests/terminal_snapshots.rs`; this is for
//! one-off captures, such as debugging a prompt from a real config.

#[path = "../tests/terminal/rig.rs"]
mod rig;

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rig::{Case, Rig, Shell};

const USAGE: &str = "\
Usage: cargo run --example terminal-snapshot -- [OPTIONS]

Runs a superline config in real shells through VHS and prints the screen
text of every snapshot.

  --config <FILE>      superline config (plus a sibling theme it names);
                       default tests/terminal/config.json
  --tape <FILE>        VHS commands to run after the first prompt, with
                       `Screenshot <name>.png` for each capture; default one
                       snapshot of the first prompt
  --case <NAME>        Use the config and tape of tests/terminal/cases/<NAME>
  --shell <LIST>       Shells, comma-separated or repeated (default all)
  --columns <N>        Terminal width (repeatable; one run each; default 100)
  --rows <N>           Terminal height (default 12)
  --workdir <DIR>      Run in an existing directory
  --env <KEY=VALUE>    Export a variable before the first prompt (repeatable)
  --output <DIR>       Output directory (default target/terminal-snapshots)
  --vhs <VHS_BINARY>   VHS build (or SUPERLINE_E2E_VHS, or vhs on PATH)
  --jobs <N>           Captures to run at once (default 2)
";

fn main() {
    rig::run_helper_if_invoked();
    if let Err(error) = run() {
        eprintln!("terminal snapshot failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> rig::Result<()> {
    let mut values = env::args_os().skip(1);
    let mut config = None;
    let mut tape = None;
    let mut case_name = None;
    let mut shells = Vec::new();
    let mut columns = Vec::new();
    let mut rows = None;
    let mut workdir = None;
    let mut vars = Vec::new();
    let mut output = PathBuf::from("target/terminal-snapshots");
    let mut vhs = env::var_os("SUPERLINE_E2E_VHS").map(PathBuf::from);
    let mut jobs = 2;

    while let Some(arg) = values.next() {
        let flag = arg.to_string_lossy().into_owned();
        match flag.as_str() {
            "--config" => config = Some(PathBuf::from(value(&mut values, &flag)?)),
            "--tape" => tape = Some(PathBuf::from(value(&mut values, &flag)?)),
            "--case" => case_name = Some(utf8(&mut values, &flag)?),
            "--shell" => shells.extend(Shell::parse_list(&utf8(&mut values, &flag)?)?),
            "--columns" => columns.push(number(&mut values, &flag)?),
            "--rows" => rows = Some(number(&mut values, &flag)?),
            "--workdir" => workdir = Some(std::path::absolute(value(&mut values, &flag)?)?),
            "--env" => {
                let pair = utf8(&mut values, &flag)?;
                let (key, value) = pair.split_once('=').ok_or("--env takes KEY=VALUE")?;
                vars.push((key.to_string(), value.to_string()));
            }
            "--output" => output = PathBuf::from(value(&mut values, &flag)?),
            "--vhs" => vhs = Some(PathBuf::from(value(&mut values, &flag)?)),
            "--jobs" => jobs = number(&mut values, &flag)?,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            _ => return Err(format!("unknown argument {flag}\n\n{USAGE}").into()),
        }
    }

    let mut case = match case_name {
        Some(name) if config.is_none() && tape.is_none() => Case::load(&name)?,
        Some(_) => return Err("--case cannot be combined with --config or --tape".into()),
        None => Case::from_files("adhoc", config.as_deref(), tape.as_deref())?,
    };
    if !columns.is_empty() {
        case = case.columns(&columns);
    }
    if let Some(rows) = rows {
        case = case.rows(rows);
    }
    case.workdir = workdir;
    for (key, value) in &vars {
        case = case.env(key, value);
    }

    if shells.is_empty() {
        shells.extend(Shell::ALL);
    }
    shells.sort_by_key(|shell| shell.name());
    shells.dedup();
    shells.retain(|shell| {
        let available = shell.is_available();
        if !available {
            eprintln!("skipping {}: not on PATH", shell.name());
        }
        available
    });
    if shells.is_empty() {
        return Err("none of the requested shells is on PATH".into());
    }

    let vhs = match vhs {
        Some(vhs) => vhs,
        None => rig::find_executable("vhs").ok_or(
            "vhs is not on PATH; pass --vhs or set SUPERLINE_E2E_VHS (see docs/terminal-snapshots.md)",
        )?,
    };
    let rig = Rig::new(&vhs, &superline_binary()?, &output, jobs)?;
    println!("using {}", rig.vhs_version()?);

    let mut failed = false;
    for (_, label, result) in rig.run(&[case], &shells) {
        match result {
            Ok(capture) => {
                for snapshot in &capture.snapshots {
                    println!(
                        "── {label} {} ({}) ──\n{}",
                        snapshot.name,
                        snapshot.png.display(),
                        snapshot.text
                    );
                }
            }
            Err(error) => {
                failed = true;
                println!("FAIL {label}: {error}");
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
    Ok(())
}

/// The `superline` binary built alongside this example.
fn superline_binary() -> rig::Result<PathBuf> {
    let executable = env::current_exe()?;
    let profile_dir = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("example executable has no target profile directory")?;
    let superline = profile_dir.join(format!("superline{}", env::consts::EXE_SUFFIX));
    if !superline.is_file() {
        return Err(format!(
            "{} does not exist; run `cargo build --bin superline` first",
            superline.display()
        )
        .into());
    }
    Ok(superline)
}

fn value(values: &mut impl Iterator<Item = OsString>, flag: &str) -> rig::Result<OsString> {
    values
        .next()
        .ok_or_else(|| format!("{flag} requires a value").into())
}

fn utf8(values: &mut impl Iterator<Item = OsString>, flag: &str) -> rig::Result<String> {
    value(values, flag)?
        .into_string()
        .map_err(|_| format!("{flag} must be valid UTF-8").into())
}

fn number<T: std::str::FromStr>(
    values: &mut impl Iterator<Item = OsString>,
    flag: &str,
) -> rig::Result<T> {
    let text = utf8(values, flag)?;
    text.parse()
        .map_err(|_| format!("{flag} takes a number, not {text:?}").into())
}
