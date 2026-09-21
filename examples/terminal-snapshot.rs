//! Render Superline through real interactive shells and save the terminal as PNG.
//!
//! This is deliberately an example rather than production code: it adds no
//! weight to the installed `superline` binary, while remaining portable enough
//! to run against Unix PTYs and Windows ConPTY in CI.

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use fontdue::{Font, FontSettings};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use vt100::{Color, Screen};

const COLS: u16 = 100;
const ROWS: u16 = 12;
const TIMEOUT: Duration = Duration::from_secs(15);
const QUIET_PERIOD: Duration = Duration::from_millis(750);
const FONT_SIZE: f32 = 18.0;
const CELL_WIDTH: usize = 11;
const CELL_HEIGHT: usize = 24;
const MARGIN: usize = 16;
const CURSOR_POSITION_REQUEST: &[u8] = b"\x1b[6n";
const CURSOR_POSITION_REPORT: &[u8] = b"\x1b[1;1R";

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type PtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;

#[derive(Default)]
struct CursorQueryScanner {
    tail: Vec<u8>,
}

impl CursorQueryScanner {
    fn sees_request(&mut self, chunk: &[u8]) -> bool {
        self.tail.extend_from_slice(chunk);
        let seen = self
            .tail
            .windows(CURSOR_POSITION_REQUEST.len())
            .any(|window| window == CURSOR_POSITION_REQUEST);
        let consumed = self
            .tail
            .len()
            .saturating_sub(CURSOR_POSITION_REQUEST.len() - 1);
        self.tail.drain(..consumed);
        seen
    }
}

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

    fn executable(self) -> &'static str {
        match self {
            Self::Nu => "nu",
            _ => self.name(),
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
    font: PathBuf,
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

    fs::create_dir_all(&args.output)?;
    let font = Font::from_bytes(fs::read(&args.font)?, FontSettings::default())
        .map_err(|error| format!("could not load font {}: {error}", args.font.display()))?;

    let mut captured = 0;
    for shell in args.shells {
        let Some(executable) = find_executable(shell.executable()) else {
            let message = format!("{} is not on PATH", shell.executable());
            if args.require_all {
                return Err(message.into());
            }
            eprintln!("skipping {}: {message}", shell.name());
            continue;
        };

        for scenario in &args.scenarios {
            let capture = capture(shell, *scenario, &executable, &superline)?;
            save_capture(shell, *scenario, &capture, &font, &args.output)?;
            captured += 1;
        }
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
    let mut font = env::var_os("SUPERLINE_E2E_FONT").map(PathBuf::from);
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
            Some("--output") => output = PathBuf::from(next_value(&mut values, "--output")?),
            Some("--font") => font = Some(PathBuf::from(next_value(&mut values, "--font")?)),
            Some("--scenario") => {
                let value = next_utf8(&mut values, "--scenario")?;
                if value == "all" {
                    scenarios.extend(Scenario::ALL);
                } else {
                    scenarios.push(Scenario::parse(&value)?);
                }
            }
            Some("--require-all") => require_all = true,
            Some("-h" | "--help") => {
                println!(
                    "Usage: cargo run --example terminal-snapshot -- --shell <all|bash|zsh|fish|pwsh|nu> [--shell ...] --scenario <all|clean|failure> --font <NERD_FONT.ttf> [--output <DIR>] [--require-all]"
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
    let font = font.ok_or(
        "pass --font <NERD_FONT.ttf> or set SUPERLINE_E2E_FONT so prompt glyphs render correctly",
    )?;

    Ok(Args {
        shells,
        scenarios,
        output,
        font,
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
    let debug_dir = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("snapshot executable has no target profile directory")?;
    Ok(debug_dir.join(format!("superline{}", env::consts::EXE_SUFFIX)))
}

struct Capture {
    raw: Vec<u8>,
    screen: Screen,
}

fn capture(
    shell: Shell,
    scenario: Scenario,
    executable: &Path,
    superline: &Path,
) -> Result<Capture> {
    let root = scratch_dir(shell, scenario)?;
    let home = root.join("home");
    let fixture = root.join("superline-e2e");
    fs::create_dir_all(home.join(".config/superline"))?;
    fs::create_dir_all(&fixture)?;
    fs::write(
        home.join(".config/superline/config.json"),
        include_str!("terminal-snapshot/config.json"),
    )?;

    let init = std::process::Command::new(superline)
        .args(["init", shell.name()])
        .output()?;
    if !init.status.success() {
        return Err(format!(
            "`superline init {}` failed: {}",
            shell.name(),
            String::from_utf8_lossy(&init.stderr)
        )
        .into());
    }
    let mut init = String::from_utf8(init.stdout)?;
    if shell == Shell::Pwsh {
        let original = "$__pl_args = @('show', '-s', $__pl_status, '-c', $__pl_cols, 'pwsh')";
        let config_path = home
            .join(".config/superline/config.json")
            .to_string_lossy()
            .replace('\'', "''");
        let replacement = format!(
            "$__pl_args = @('show', '-s', $__pl_status, '-c', $__pl_cols, 'pwsh', '--config', '{config_path}')"
        );
        if !init.contains(original) {
            return Err("PowerShell init no longer contains the expected argument list".into());
        }
        init = init.replace(original, &replacement);
    }
    prepare_shell(shell, &home, &init)?;

    let pty = native_pty_system().openpty(PtySize {
        rows: ROWS,
        cols: COLS,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = shell_command(shell, executable, &home)?;
    command.cwd(&fixture);
    command.env("PWD", &fixture);
    command.env("HOME", &home);
    command.env("USERPROFILE", &home);
    command.env("XDG_CONFIG_HOME", home.join(".config"));
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("COLUMNS", COLS.to_string());
    command.env("LINES", ROWS.to_string());
    command.env("BASH_SILENCE_DEPRECATION_WARNING", "1");
    command.env("fish_features", "no-query-terminal");
    command.env("SUPERLINE_BIN", superline);
    command.env("PATH", path_with_binary(superline)?);

    let mut child = pty.slave.spawn_command(command)?;
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader()?;
    let writer: PtyWriter = Arc::new(Mutex::new(pty.master.take_writer()?));
    let cursor_writer = Arc::clone(&writer);
    let (sender, receiver) = mpsc::channel();
    let reader_thread = thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        let mut cursor_queries = CursorQueryScanner::default();
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let chunk = &buffer[..count];
                    if cursor_queries.sees_request(chunk) {
                        let _ = pty_write(&cursor_writer, CURSOR_POSITION_REPORT);
                    }
                    if sender.send(chunk.to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut parser = vt100::Parser::new(ROWS, COLS, 0);
    let mut raw = Vec::new();
    let started = Instant::now();
    let mut last_output = Instant::now();
    let mut saw_prompt = false;
    let mut cleared = false;
    let mut command_sent = false;

    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(bytes) => {
                parser.process(&bytes);
                answer_terminal_queries(&bytes, &writer)?;
                raw.extend_from_slice(&bytes);
                last_output = Instant::now();
                let contents = parser.screen().contents();
                let prompt_count = contents.matches(shell.name()).count();
                saw_prompt = contents.contains("superline-e2e")
                    && prompt_count >= 2
                    && (!command_sent
                        || contents.contains(&format!(
                            "{}{}",
                            shell.name(),
                            failure_status(shell)
                        )));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if saw_prompt && last_output.elapsed() >= QUIET_PERIOD {
            if !cleared {
                pty_write(&writer, b"clear\r")?;
                cleared = true;
                saw_prompt = false;
                last_output = Instant::now();
                continue;
            }
            if scenario == Scenario::Failure && !command_sent {
                let command = format!("{}\r", failure_command(shell));
                pty_write(&writer, command.as_bytes())?;
                command_sent = true;
                saw_prompt = false;
                last_output = Instant::now();
                continue;
            }
            break;
        }
        if started.elapsed() >= TIMEOUT {
            let contents = parser.screen().contents();
            let _ = child.kill();
            let _ = child.wait();
            drop(writer);
            drop(pty.master);
            let _ = reader_thread.join();
            let _ = fs::remove_dir_all(&root);
            return Err(format!(
                "timed out waiting for the {} prompt; terminal contained:\n{contents}\nraw stream:\n{:?}",
                shell.name(),
                String::from_utf8_lossy(&raw)
            )
            .into());
        }
    }

    let screen = parser.screen().clone();
    let _ = child.kill();
    let _ = child.wait();
    drop(writer);
    drop(pty.master);
    drop(receiver);
    let _ = reader_thread.join();
    let _ = fs::remove_dir_all(&root);

    if !saw_prompt {
        return Err(format!(
            "{} exited before rendering a prompt; terminal contained:\n{}",
            shell.name(),
            screen.contents()
        )
        .into());
    }
    Ok(Capture { raw, screen })
}

fn pty_write(writer: &PtyWriter, bytes: &[u8]) -> Result<()> {
    let mut writer = writer.lock().map_err(|_| "PTY writer lock poisoned")?;
    writer.write_all(bytes)?;
    writer.flush()?;
    Ok(())
}

fn answer_terminal_queries(bytes: &[u8], writer: &PtyWriter) -> Result<()> {
    if bytes.windows(4).any(|window| window == b"\x1b[0c") {
        pty_write(writer, b"\x1b[?1;2c")?;
    }
    Ok(())
}

fn prepare_shell(shell: Shell, home: &Path, init: &str) -> Result<()> {
    let contents = match shell {
        Shell::Fish => format!("set --global fish_greeting\n{init}\nclear\n"),
        Shell::Nu => format!("$env.config.show_banner = false\n{init}\nclear\n"),
        _ => format!("{init}\nclear\n"),
    };
    match shell {
        Shell::Bash => fs::write(home.join(".bashrc"), contents)?,
        Shell::Zsh => fs::write(home.join(".zshrc"), contents)?,
        Shell::Fish => {
            let dir = home.join(".config/fish");
            fs::create_dir_all(&dir)?;
            fs::write(dir.join("config.fish"), contents)?;
        }
        Shell::Nu => fs::write(home.join("config.nu"), contents)?,
        Shell::Pwsh => fs::write(
            home.join("profile.ps1"),
            format!("$PSStyle.OutputRendering = 'Ansi'\n{init}\nClear-Host\n"),
        )?,
    }
    Ok(())
}

fn shell_command(shell: Shell, executable: &Path, home: &Path) -> Result<CommandBuilder> {
    let mut command = CommandBuilder::new(executable);
    match shell {
        Shell::Bash => command.args([
            "--noprofile",
            "--rcfile",
            &home.join(".bashrc").to_string_lossy(),
            "-i",
        ]),
        Shell::Zsh => command.args(["-d"]),
        Shell::Fish => command.args(["--interactive", "--features=no-query-terminal"]),
        Shell::Pwsh => command.args([
            "-NoLogo",
            "-NoProfile",
            "-NoExit",
            "-File",
            &home.join("profile.ps1").to_string_lossy(),
        ]),
        Shell::Nu => command.args([
            "--interactive",
            "--config",
            &home.join("config.nu").to_string_lossy(),
        ]),
    }
    if shell == Shell::Zsh {
        command.env("ZDOTDIR", home);
    }
    Ok(command)
}

fn failure_command(shell: Shell) -> &'static str {
    match (shell, cfg!(windows)) {
        (Shell::Pwsh, true) => "cmd /c exit 7",
        (Shell::Pwsh, false) => "& /bin/sh -c 'exit 7'",
        (Shell::Nu, true) => "^cmd /c exit 7",
        (Shell::Nu, false) => "^/bin/sh -c 'exit 7'",
        _ => "false",
    }
}

fn failure_status(shell: Shell) -> u8 {
    match shell {
        Shell::Pwsh | Shell::Nu => 7,
        _ => 1,
    }
}

fn scratch_dir(shell: Shell, scenario: Scenario) -> Result<PathBuf> {
    let root = env::temp_dir().join(format!(
        "superline-terminal-snapshot-{}-{}-{}",
        std::process::id(),
        shell.name(),
        scenario.name()
    ));
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(&root)?;
    Ok(root)
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

fn save_capture(
    shell: Shell,
    scenario: Scenario,
    capture: &Capture,
    font: &Font,
    output: &Path,
) -> Result<()> {
    let platform = env::var("RUNNER_OS")
        .unwrap_or_else(|_| env::consts::OS.to_string())
        .to_ascii_lowercase();
    let stem = format!("{platform}-{}-{}", shell.name(), scenario.name());
    fs::write(output.join(format!("{stem}.ansi")), &capture.raw)?;
    fs::write(
        output.join(format!("{stem}.txt")),
        capture.screen.contents(),
    )?;
    render_png(&capture.screen, font, &output.join(format!("{stem}.png")))?;
    println!("captured {}", output.join(format!("{stem}.png")).display());
    Ok(())
}

fn render_png(screen: &Screen, font: &Font, path: &Path) -> Result<()> {
    let (first_row, last_row) = used_rows(screen).unwrap_or((0, 2));
    let rows = usize::from(last_row - first_row + 1);
    let width = usize::from(COLS) * CELL_WIDTH + MARGIN * 2;
    let height = rows * CELL_HEIGHT + MARGIN * 2;
    let terminal_bg = [30, 30, 30, 255];
    let terminal_fg = [232, 232, 232, 255];
    let mut pixels = vec![0_u8; width * height * 4];
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&terminal_bg);
    }

    for row in first_row..=last_row {
        for col in 0..COLS {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            let mut foreground = color(cell.fgcolor(), terminal_fg);
            let mut background = color(cell.bgcolor(), terminal_bg);
            if cell.inverse() {
                std::mem::swap(&mut foreground, &mut background);
            }
            let x = MARGIN + usize::from(col) * CELL_WIDTH;
            let y = MARGIN + usize::from(row - first_row) * CELL_HEIGHT;
            fill_rect(
                &mut pixels,
                width,
                x,
                y,
                CELL_WIDTH,
                CELL_HEIGHT,
                background,
            );
            if cell.is_wide_continuation() {
                continue;
            }
            for character in cell.contents().chars() {
                draw_glyph(
                    &mut pixels,
                    (width, height),
                    (x, y),
                    character,
                    foreground,
                    font,
                );
            }
            if cell.underline() {
                fill_rect(
                    &mut pixels,
                    width,
                    x,
                    y + CELL_HEIGHT - 3,
                    CELL_WIDTH,
                    1,
                    foreground,
                );
            }
        }
    }

    let file = fs::File::create(path)?;
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&pixels)?;
    Ok(())
}

fn used_rows(screen: &Screen) -> Option<(u16, u16)> {
    let used = |row| {
        (0..COLS).any(|col| {
            screen
                .cell(row, col)
                .is_some_and(|cell| cell.has_contents() || cell.bgcolor() != Color::Default)
        })
    };
    let first = (0..ROWS).find(|row| used(*row))?;
    let last = (first..ROWS).rfind(|row| used(*row)).unwrap_or(first);
    Some((first, last))
}

fn draw_glyph(
    pixels: &mut [u8],
    image_size: (usize, usize),
    cell: (usize, usize),
    character: char,
    color: [u8; 4],
    font: &Font,
) {
    let (image_width, image_height) = image_size;
    let (cell_x, cell_y) = cell;
    let (metrics, bitmap) = font.rasterize(character, FONT_SIZE);
    let baseline = cell_y as i32 + 18;
    let glyph_x = cell_x as i32 + metrics.xmin;
    let glyph_y = baseline - metrics.ymin - metrics.height as i32;

    for bitmap_y in 0..metrics.height {
        for bitmap_x in 0..metrics.width {
            let x = glyph_x + bitmap_x as i32;
            let y = glyph_y + bitmap_y as i32;
            if x < 0 || y < 0 || x >= image_width as i32 || y >= image_height as i32 {
                continue;
            }
            let alpha = bitmap[bitmap_y * metrics.width + bitmap_x];
            blend_pixel(
                &mut pixels[(y as usize * image_width + x as usize) * 4..][..4],
                color,
                alpha,
            );
        }
    }
}

fn fill_rect(
    pixels: &mut [u8],
    image_width: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    color: [u8; 4],
) {
    for row in y..y + height {
        for col in x..x + width {
            pixels[(row * image_width + col) * 4..][..4].copy_from_slice(&color);
        }
    }
}

fn blend_pixel(destination: &mut [u8], source: [u8; 4], coverage: u8) {
    let alpha = u16::from(coverage);
    for channel in 0..3 {
        destination[channel] = ((u16::from(source[channel]) * alpha
            + u16::from(destination[channel]) * (255 - alpha))
            / 255) as u8;
    }
    destination[3] = 255;
}

fn color(color: Color, default: [u8; 4]) -> [u8; 4] {
    match color {
        Color::Default => default,
        Color::Idx(index) => {
            let [red, green, blue] = xterm_color(index);
            [red, green, blue, 255]
        }
        Color::Rgb(red, green, blue) => [red, green, blue, 255],
    }
}

fn xterm_color(index: u8) -> [u8; 3] {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];
    match index {
        0..=15 => ANSI[index as usize],
        16..=231 => {
            let value = index - 16;
            let scale = [0, 95, 135, 175, 215, 255];
            [
                scale[(value / 36) as usize],
                scale[((value % 36) / 6) as usize],
                scale[(value % 6) as usize],
            ]
        }
        _ => {
            let shade = 8 + (index - 232) * 10;
            [shade, shade, shade]
        }
    }
}
