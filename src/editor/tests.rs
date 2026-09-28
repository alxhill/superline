//! The editor drawn to a `TestBackend`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::Terminal;

use super::theme::{ThemeDoc, STARTER_THEME};
use super::{draw_scrollbar, load, App};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 30;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "superline-editor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The editor on the Theme page of a config using a copy of the example theme.
fn theme_page(dir: &Path) -> App {
    let config = dir.join("config.json");
    std::fs::write(
        &config,
        r#"{ "theme": "theme.json", "rows": [{ "left": ["cwd"] }] }"#,
    )
    .unwrap();
    std::fs::write(dir.join("theme.json"), STARTER_THEME).unwrap();
    let mut app = App::new(load(&config).unwrap(), config);
    app.load_theme_slot();
    press(&mut app, KeyCode::Char('2'), 1);
    app
}

/// Moves the module list to `module` and opens its properties.
fn open_module(app: &mut App, module: &str) {
    let index = ThemeDoc::load(STARTER_THEME)
        .unwrap()
        .entries()
        .iter()
        .position(|entry| entry.label() == module)
        .unwrap();
    press(app, KeyCode::Down, index);
    press(app, KeyCode::Enter, 1);
}

fn press(app: &mut App, code: KeyCode, times: usize) {
    for _ in 0..times {
        app.on_key(KeyEvent::from(code));
    }
}

fn draw(app: &mut App) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    terminal.backend().buffer().clone()
}

/// The property panel's right border between its corners, split into the
/// rows beside the properties and the rows beside the help text.
fn property_border(buffer: &Buffer) -> (Vec<String>, Vec<String>) {
    let column: Vec<&str> = (0..HEIGHT)
        .map(|y| buffer[(WIDTH - 1, y)].symbol())
        .collect();
    let top = column.iter().rposition(|s| *s == "┐").unwrap();
    let bottom = column.iter().rposition(|s| *s == "┘").unwrap();
    let mut rows: Vec<String> = column[top + 1..bottom]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let help = rows.split_off(rows.len() - 3);
    (rows, help)
}

fn screen_text(buffer: &Buffer) -> String {
    buffer.content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn long_property_list_has_a_scrollbar_that_follows_the_selection() {
    let dir = Scratch::new();
    let mut app = theme_page(&dir.0);
    open_module(&mut app, "git");

    let (rows, help) = property_border(&draw(&mut app));
    assert_eq!(
        rows.first().unwrap(),
        "█",
        "thumb starts at the top: {rows:?}"
    );
    assert_eq!(rows.last().unwrap(), "│", "track below the thumb: {rows:?}");
    assert!(help.iter().all(|s| s == "│"), "help rows: {help:?}");

    press(&mut app, KeyCode::Down, 100);
    let buffer = draw(&mut app);
    let (rows, _) = property_border(&buffer);
    assert_eq!(
        rows.first().unwrap(),
        "│",
        "track above the thumb: {rows:?}"
    );
    assert_eq!(
        rows.last().unwrap(),
        "█",
        "thumb ends at the bottom: {rows:?}"
    );
    assert!(screen_text(&buffer).contains("behind_icon"));

    // Back in the module list, the panel shows its first rows again.
    press(&mut app, KeyCode::Esc, 1);
    let (rows, _) = property_border(&draw(&mut app));
    assert_eq!(rows.first().unwrap(), "█", "{rows:?}");
}

#[test]
fn short_property_list_has_no_scrollbar() {
    let dir = Scratch::new();
    let mut app = theme_page(&dir.0);
    open_module(&mut app, "time");

    let (rows, help) = property_border(&draw(&mut app));
    assert!(rows.iter().chain(&help).all(|s| s == "│"), "{rows:?}");
}

#[test]
fn scrollbar_thumb_spans_the_rows_on_screen() {
    let render = |total: usize, offset: usize| -> String {
        let mut terminal = Terminal::new(TestBackend::new(2, 4)).unwrap();
        terminal
            .draw(|frame| draw_scrollbar(frame, Rect::new(0, 0, 1, 4), total, offset))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..4).map(|y| buffer[(1, y)].symbol()).collect()
    };
    assert_eq!(render(4, 0), "    ");
    assert_eq!(render(8, 0), "██││");
    assert_eq!(render(8, 2), "│██│");
    assert_eq!(render(8, 4), "││██");
}
