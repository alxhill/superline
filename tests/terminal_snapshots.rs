//! Terminal snapshot tests: superline in real interactive shells, driven by
//! VHS.
//!
//! Each test names a directory under `tests/terminal/cases/` holding a
//! superline `config.json` and a VHS `case.tape`, the terminal sizes and
//! fixture to run it in, and a check over what the terminal showed. Every
//! test runs in every available shell.
//!
//! Skipped unless `SUPERLINE_E2E_VHS` points at the patched VHS build (see
//! docs/terminal-snapshots.md):
//!
//! ```bash
//! SUPERLINE_E2E_VHS=target/vhs-bin/vhs cargo test --test terminal_snapshots [-- <name>...]
//! ```
//!
//! `SUPERLINE_E2E_SHELLS` (comma-separated, default all) picks shells,
//! `SUPERLINE_E2E_REQUIRE_ALL=1` fails instead of skipping a missing one,
//! `SUPERLINE_E2E_OUTPUT` sets the output directory (default
//! `target/terminal-snapshots`), and `SUPERLINE_E2E_JOBS` the number of
//! captures run at once (default 2).

#[path = "terminal/rig.rs"]
mod rig;

use std::cell::RefCell;
use std::env;
use std::panic;
use std::path::PathBuf;

use rig::{width, Capture, Case, Rig, Shell, Snapshot, OK, SEP};

struct Test {
    name: &'static str,
    columns: &'static [u16],
    rows: u16,
    dir: Option<&'static str>,
    check: fn(&Capture),
}

const DEFAULT: Test = Test {
    name: "",
    columns: &[100],
    rows: 12,
    dir: None,
    check: |_| {},
};

const TESTS: &[Test] = &[
    Test {
        name: "status",
        check: status,
        ..DEFAULT
    },
    Test {
        name: "multiline",
        check: multiline,
        ..DEFAULT
    },
    Test {
        name: "widths",
        columns: &[40, 80, 160],
        rows: 6,
        check: widths,
        ..DEFAULT
    },
    Test {
        name: "narrow",
        columns: &[30],
        check: narrow,
        ..DEFAULT
    },
    Test {
        name: "wide-chars",
        columns: &[60],
        dir: Some("superline-e2e/日本語/データ"),
        check: wide_chars,
        ..DEFAULT
    },
    Test {
        name: "continuation",
        check: continuation,
        ..DEFAULT
    },
    Test {
        name: "no-newline",
        check: no_newline,
        ..DEFAULT
    },
];

/// The last row of the shared config, and most case configs: the shell name
/// followed by the `cmd` widget's success mark.
fn ready(c: &Capture) -> String {
    format!("{}{SEP}{OK}", c.shell.name())
}

/// A line ending `margin` columns from the right edge.
fn check_flush(s: &Snapshot, c: &Capture, needle: &str, margins: std::ops::RangeInclusive<usize>) {
    let line = s.line(needle);
    let margin = usize::from(c.columns).saturating_sub(width(line));
    s.check(
        margins.contains(&margin),
        format!("{needle:?} ends {margin} columns from the edge, wanted {margins:?}"),
    );
}

/// The success chevron, then a failing command's status below the intact
/// previous prompt.
fn status(c: &Capture) {
    let shell = c.shell.name();
    let clean = c.snapshot("clean");
    clean.check(
        clean.text.contains(&format!("{shell}{SEP}{OK}{SEP}")),
        "the last row should be the shell name and the success mark",
    );
    let failure = c.snapshot("failure");
    failure.check(
        failure.matches(&format!("{shell}{SEP}{OK}{SEP}[^ ]* sl-test exit 7\n")),
        "the first prompt and the command should survive above the new prompt",
    );
    failure.check(
        failure.text.contains(&format!("{shell}{SEP}7{SEP}")),
        "the new prompt should show exit status 7",
    );
}

/// Three rows with right sides survive a command and redraw in order.
fn multiline(c: &Capture) {
    let ready = regex::escape(&ready(c));
    let prompt = r"[^\n]*row-one[^\n]*right-one[^\n]*\n[^\n]*row-two[^\n]*right-two[^\n]*\n";
    for name in ["first", "second"] {
        let s = c.snapshot(name);
        check_flush(s, c, "right-one", 1..=1);
        check_flush(s, c, "right-two", 1..=1);
    }

    let first = c.snapshot("first");
    first.check(
        first.matches(&format!(r"\A{prompt}{ready}")),
        "the three rows should be drawn in order",
    );
    if c.shell.draws_last_row_right() {
        // zsh keeps one column free to the right of its right prompt.
        check_flush(first, c, "right-three", 0..=1);
    } else {
        first.check(
            !first.text.contains("right-three"),
            "this shell has no right prompt for the last row",
        );
    }

    let second = c.snapshot("second");
    second.check(
        second.matches(&format!(
            r"\A{prompt}{ready}[^\n]*echo hello[^\n]*\nhello\n{prompt}{ready}"
        )),
        "the first prompt should survive above the command output and the new prompt",
    );
}

/// Right sides stay flush with the edge at narrow, medium and wide terminals.
fn widths(c: &Capture) {
    let s = c.snapshot("prompt");
    check_flush(s, c, "right-top", 1..=1);
    if c.shell.draws_last_row_right() {
        check_flush(s, c, "right-bottom", 0..=1);
    }
}

/// A row wider than the terminal wraps (or is truncated by fish) without
/// corrupting the next prompt.
fn narrow(c: &Capture) {
    let after = c.snapshot("after");
    after.check(
        after.matches(&format!(
            r"\n{}[^\n]*echo hi\nhi\n[^\n]*(a-long|that-overflows)",
            regex::escape(&ready(c))
        )),
        "the output should sit between the old prompt and the new one",
    );
}

/// Double-width characters in the path keep the right side flush with the
/// edge.
fn wide_chars(c: &Capture) {
    let s = c.snapshot("prompt");
    s.check(
        s.matches(&format!(
            r"\A[^\n]*データ[^\n]*edge[^\n]*\n{}",
            regex::escape(&ready(c))
        )),
        "the first row should fit on one line",
    );
    check_flush(s, c, "edge", 1..=1);
}

/// A command continued onto a second input line runs, and the next prompt
/// draws below its output.
fn continuation(c: &Capture) {
    let after = c.snapshot("after");
    after.check(
        after.matches(&format!(
            r"\none\ntwo\n[^\n]*superline-e2e[^\n]*\n{}",
            regex::escape(&ready(c))
        )),
        "the next prompt should follow the output",
    );
}

/// Output without a trailing newline does not push the first row off the
/// left edge.
fn no_newline(c: &Capture) {
    let after = c.snapshot("after");
    let joined = after.matches(r"partial[^\n]*superline-e2e");
    if c.shell == Shell::Bash {
        // Known bug: bash has no PROMPT_SP equivalent, so the first row starts
        // right after the unterminated output. Fixing it should flip this.
        after.check(
            joined,
            "bash now starts the prompt on a fresh line; update this test",
        );
        return;
    }
    after.check(
        !joined,
        "the first row should not share a line with the output",
    );
    after.check(
        after.matches(&format!(
            r"\npartial[^\n]*\n[^\n]*superline-e2e[^\n]*\n{}",
            regex::escape(&ready(c))
        )),
        "the prompt should start on the line after the output",
    );
}

thread_local! {
    static PANIC_MESSAGE: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn main() {
    rig::run_helper_if_invoked();
    let Some(vhs) = env::var_os("SUPERLINE_E2E_VHS").map(PathBuf::from) else {
        println!("terminal snapshots skipped: set SUPERLINE_E2E_VHS to the patched VHS build (see docs/terminal-snapshots.md)");
        return;
    };
    if let Err(error) = run(vhs) {
        eprintln!("terminal snapshots failed: {error}");
        std::process::exit(1);
    }
}

fn run(vhs: PathBuf) -> rig::Result<()> {
    // Arguments that are not flags filter tests by name; cargo's own flags
    // (`--nocapture`, `--test-threads`, ...) are accepted and ignored.
    let filters: Vec<String> = env::args()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .collect();
    let shells = Shell::parse_list(&env::var("SUPERLINE_E2E_SHELLS").unwrap_or("all".into()))?;
    let require_all = env::var("SUPERLINE_E2E_REQUIRE_ALL").is_ok_and(|value| value == "1");
    let output = env::var_os("SUPERLINE_E2E_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/terminal-snapshots")
        });
    let jobs = env::var("SUPERLINE_E2E_JOBS")
        .ok()
        .and_then(|jobs| jobs.parse().ok())
        .unwrap_or(2);

    let mut available = Vec::new();
    for shell in shells {
        if shell.is_available() {
            available.push(shell);
        } else if require_all {
            return Err(format!("{} is not on PATH", shell.name()).into());
        } else {
            println!("skipping {}: not on PATH", shell.name());
        }
    }

    let tests: Vec<&Test> = TESTS
        .iter()
        .filter(|test| filters.is_empty() || filters.iter().any(|f| test.name.contains(f.as_str())))
        .collect();
    let mut cases = Vec::new();
    for test in &tests {
        let mut case = Case::load(test.name)?.columns(test.columns).rows(test.rows);
        if let Some(dir) = test.dir {
            case = case.dir(dir);
        }
        cases.push(case);
    }

    let rig = Rig::new(
        &vhs,
        env!("CARGO_BIN_EXE_superline").as_ref(),
        &output,
        jobs,
    )?;
    println!("using {}", rig.vhs_version()?);
    panic::set_hook(Box::new(|info| {
        let message = match info.payload().downcast_ref::<String>() {
            Some(message) => message.clone(),
            None => info
                .payload()
                .downcast_ref::<&str>()
                .map(|message| message.to_string())
                .unwrap_or_default(),
        };
        PANIC_MESSAGE.with(|slot| *slot.borrow_mut() = Some(message));
    }));

    let results = rig.run(&cases, &available);
    let mut failed = 0;
    for (case_index, label, result) in &results {
        let test = tests[*case_index];
        let outcome = match result {
            Ok(capture) => panic::catch_unwind(panic::AssertUnwindSafe(|| (test.check)(capture)))
                .map_err(|_| {
                    PANIC_MESSAGE
                        .with(|slot| slot.borrow_mut().take())
                        .unwrap_or_default()
                }),
            Err(error) => Err(error.to_string()),
        };
        match outcome {
            Ok(()) => println!("ok    {label}"),
            Err(message) => {
                failed += 1;
                println!("FAIL  {label}\n{}", indent(&message));
            }
        }
    }
    println!(
        "{} runs, {failed} failed; captures in {}",
        results.len(),
        output.display()
    );
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|line| format!("      {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
