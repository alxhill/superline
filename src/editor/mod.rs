//! `superline config`: an interactive terminal editor for the config file,
//! with a live preview of the prompt it produces.

mod ansi;
mod glyphs;
mod json;
mod model;
mod picker;
mod preview;
mod schema;
mod theme;
mod theme_page;

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use model::{describe, show_value, widget_spec, Document, Entry, SegPos, Side, Target};
use preview::{Preview, Request};
use schema::{Kind, OptionSpec, WidgetSpec, WIDGETS};
use theme_page::ThemePage;

/// Cached widgets (git, PR, AI usage) fill in after a background refresh, so
/// the preview is redrawn this often even when nothing was edited.
const PREVIEW_REFRESH: Duration = Duration::from_secs(2);

/// Opens the editor on `path` and runs until the user quits.
pub fn run(path: &Path) -> io::Result<()> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let doc = load(&path).map_err(io::Error::other)?;
    let mut app = App::new(doc, path);

    loop {
        let mut terminal = ratatui::try_init()?;
        let outcome = app.event_loop(&mut terminal);
        ratatui::restore();
        match outcome? {
            Exit::Quit => return Ok(()),
            Exit::ExternalEditor => app.edit_externally(),
        }
    }
}

fn load(path: &Path) -> Result<Document, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} is not valid JSON: {e}", path.display()))?;
    Document::new(value).map_err(|e| format!("{}: {e}", path.display()))
}

enum Exit {
    Quit,
    ExternalEditor,
}

#[derive(PartialEq)]
enum Focus {
    Layout,
    Options,
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Layout,
    Theme,
}

#[derive(Clone, Copy, PartialEq)]
enum InputPurpose {
    /// A widget option or setting on the Layout page.
    Option,
    ThemeProperty,
    /// The file name of a new custom theme.
    NewTheme,
}

enum Mode {
    Normal,
    Picker {
        filter: String,
        selected: usize,
    },
    Input {
        buffer: String,
        cursor: usize,
        purpose: InputPurpose,
    },
    ColorPicker {
        code: u8,
        /// Write the pick as a colour name when it has one.
        prefer_name: bool,
    },
    IconBrowser {
        query: String,
        selected: usize,
    },
    ConfirmQuit,
    Help,
}

struct Status {
    text: String,
    error: bool,
}

struct App {
    page: Page,
    doc: Document,
    path: PathBuf,
    theme: ThemePage,
    entries: Vec<Entry>,
    cursor: usize,
    list: ListState,
    focus: Focus,
    option_cursor: usize,
    mode: Mode,
    status: Option<Status>,
    theme_choices: Vec<String>,

    preview: Preview,
    preview_lines: Vec<Line<'static>>,
    preview_error: Option<String>,
    preview_columns: u16,
    preview_stale: bool,
    preview_in_flight: bool,
    preview_requested: Instant,

    exit: Option<Exit>,
}

impl App {
    fn new(doc: Document, path: PathBuf) -> App {
        let entries = doc.entries();
        let theme_choices = theme_choices(&path);
        App {
            page: Page::Layout,
            theme: ThemePage::new(),
            preview: Preview::spawn(&path),
            doc,
            path,
            entries,
            cursor: 0,
            list: ListState::default(),
            focus: Focus::Layout,
            option_cursor: 0,
            mode: Mode::Normal,
            status: None,
            theme_choices,
            preview_lines: Vec::new(),
            preview_error: None,
            preview_columns: 0,
            preview_stale: true,
            preview_in_flight: false,
            preview_requested: Instant::now(),
            exit: None,
        }
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> io::Result<Exit> {
        self.preview_stale = true;
        loop {
            if let Some(rendered) = self.preview.poll() {
                self.preview_in_flight = false;
                match rendered {
                    Ok(text) => {
                        self.preview_lines = ansi::to_lines(&text);
                        self.preview_error = None;
                    }
                    Err(error) => self.preview_error = Some(error),
                }
            }

            let columns = terminal.size()?.width.saturating_sub(2);
            if columns != self.preview_columns {
                self.preview_columns = columns;
                self.preview_stale = true;
            }
            let due = !self.preview_in_flight && self.preview_requested.elapsed() > PREVIEW_REFRESH;
            self.load_theme_slot();
            if self.preview_stale || due {
                self.preview.request(Request {
                    config: self.doc.root().clone(),
                    theme: self.preview_theme(),
                    columns,
                });
                self.preview_stale = false;
                self.preview_in_flight = true;
                self.preview_requested = Instant::now();
            }

            terminal.draw(|frame| self.draw(frame))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.on_key(key);
                    }
                }
            }
            if let Some(exit) = self.exit.take() {
                return Ok(exit);
            }
        }
    }

    // ---- state helpers ----

    fn entry(&self) -> Entry {
        self.entries[self.cursor]
    }

    fn changed(&mut self) {
        self.entries = self.doc.entries();
        self.cursor = self.cursor.min(self.entries.len() - 1);
        let options = self
            .current_target()
            .map_or(0, |t| self.doc.options(t).len());
        self.option_cursor = self.option_cursor.min(options.saturating_sub(1));
        if options == 0 {
            self.focus = Focus::Layout;
        }
        self.preview_stale = true;
    }

    fn select(&mut self, entry: Entry) {
        if let Some(index) = self.entries.iter().position(|e| *e == entry) {
            self.cursor = index;
        }
    }

    fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some(Status {
            text: text.into(),
            error,
        });
    }

    fn current_target(&self) -> Option<Target> {
        match self.entry() {
            Entry::Settings => Some(Target::Settings),
            Entry::Segment(pos) => Some(Target::Segment(pos)),
            _ => None,
        }
    }

    fn current_option(&self) -> Option<(Target, OptionSpec, Option<Value>)> {
        let target = self.current_target()?;
        let options = self.doc.options(target);
        let (spec, value) = options.get(self.option_cursor)?;
        Some((target, *spec, value.cloned()))
    }

    fn apply(&mut self, target: Target, key: &str, value: Option<Value>) -> bool {
        match self.doc.set_option(target, key, value) {
            Ok(()) => {
                self.changed();
                true
            }
            Err(error) => {
                self.set_status(error, true);
                false
            }
        }
    }

    /// Where a new widget goes: just below the cursor.
    fn insert_position(&self) -> SegPos {
        match self.entry() {
            Entry::Settings => SegPos {
                row: 0,
                side: Side::Left,
                index: 0,
            },
            Entry::Row(row) => SegPos {
                row,
                side: Side::Left,
                index: 0,
            },
            Entry::Side(row, side) => SegPos {
                row,
                side,
                index: 0,
            },
            Entry::Segment(pos) => SegPos {
                index: pos.index + 1,
                ..pos
            },
        }
    }

    fn picker_matches(filter: &str) -> Vec<&'static WidgetSpec> {
        let filter = filter.to_lowercase();
        WIDGETS
            .iter()
            .filter(|spec| spec.listed)
            .filter(|spec| {
                spec.name.contains(&filter)
                    || spec.aliases.iter().any(|a| a.contains(&filter))
                    || spec.summary.to_lowercase().contains(&filter)
            })
            .collect()
    }

    // ---- actions ----

    fn is_dirty(&self) -> bool {
        self.doc.is_dirty() || self.theme.is_dirty()
    }

    fn save(&mut self) {
        let mut written = Vec::new();
        if self.doc.is_dirty() {
            let text = json::to_pretty(self.doc.root());
            if let Err(error) = write_atomic(&self.path, &text) {
                return self.set_status(format!("Save failed: {error}"), true);
            }
            self.doc.mark_saved();
            written.push(self.path.clone());
        }
        match self.theme.save() {
            Ok(themes) => written.extend(themes),
            Err(error) => return self.set_status(format!("Save failed: {error}"), true),
        }
        let names: Vec<String> = written
            .iter()
            .map(|path| {
                path.file_name().map_or_else(
                    || display_path(path),
                    |name| name.to_string_lossy().into_owned(),
                )
            })
            .collect();
        match names.as_slice() {
            [] => self.set_status("Nothing to save", false),
            names => self.set_status(format!("Saved {}", names.join(" and ")), false),
        }
    }

    fn request_quit(&mut self) {
        if self.is_dirty() {
            self.mode = Mode::ConfirmQuit;
        } else {
            self.exit = Some(Exit::Quit);
        }
    }

    fn edit_externally(&mut self) {
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| if cfg!(windows) { "notepad" } else { "vi" }.into());
        // The variable may carry arguments, e.g. `code --wait`.
        let mut parts = editor.split_whitespace();
        let Some(program) = parts.next() else {
            self.set_status("$EDITOR is empty", true);
            return;
        };
        let status = Command::new(program).args(parts).arg(&self.path).status();
        match status {
            Ok(_) => match load(&self.path) {
                Ok(doc) => {
                    let _ = self.doc.replace(doc.root().clone());
                    self.changed();
                    self.set_status("Reloaded after editing", false);
                }
                Err(error) => self.set_status(format!("Kept the previous config: {error}"), true),
            },
            Err(error) => self.set_status(format!("Could not run {program}: {error}"), true),
        }
    }

    fn open_picker(&mut self) {
        self.mode = Mode::Picker {
            filter: String::new(),
            selected: 0,
        };
    }

    fn add_widget(&mut self, spec: &WidgetSpec) {
        let pos = self.insert_position();
        let pos = self
            .doc
            .insert(pos.row, pos.side, pos.index, spec.template());
        self.changed();
        self.select(Entry::Segment(pos));
        self.set_status(format!("Added {}", spec.name), false);
    }

    fn delete(&mut self) {
        match self.entry() {
            Entry::Segment(pos) => {
                let (name, _) = describe(self.doc.segment(pos));
                self.doc.remove(pos);
                self.changed();
                self.set_status(format!("Removed {name} (u to undo)"), false);
            }
            Entry::Row(row) => match self.doc.remove_row(row) {
                Ok(()) => {
                    self.changed();
                    self.select(Entry::Row(row.min(self.doc.row_count() - 1)));
                    self.set_status(format!("Removed row {} (u to undo)", row + 1), false);
                }
                Err(error) => self.set_status(error, true),
            },
            _ => {}
        }
    }

    fn reorder(&mut self, down: bool) {
        match self.entry() {
            Entry::Segment(pos) => {
                if let Some(pos) = self.doc.move_segment(pos, down) {
                    self.changed();
                    self.select(Entry::Segment(pos));
                }
            }
            Entry::Row(row) => {
                if let Some(row) = self.doc.move_row(row, down) {
                    self.changed();
                    self.select(Entry::Row(row));
                }
            }
            _ => {}
        }
    }

    fn new_row(&mut self) {
        let after = match self.entry() {
            Entry::Settings => 0,
            Entry::Row(row) | Entry::Side(row, _) => row + 1,
            Entry::Segment(pos) => pos.row + 1,
        };
        let row = self.doc.add_row(after);
        self.changed();
        self.select(Entry::Side(row, Side::Left));
        self.set_status(format!("Added row {}", row + 1), false);
    }

    fn cycle(&mut self, forward: bool) {
        let Some((target, spec, value)) = self.current_option() else {
            return;
        };
        let next = match spec.kind {
            Kind::Bool { default } => Some(Value::Bool(
                !value.and_then(|v| v.as_bool()).unwrap_or(default),
            )),
            Kind::Choice { variants, default } => {
                let mut choices: Vec<Option<&str>> = variants.iter().copied().map(Some).collect();
                if !spec.required && default.is_none() {
                    choices.insert(0, None);
                }
                let current = value.as_ref().and_then(Value::as_str).or(default);
                let next = match choices.iter().position(|c| *c == current) {
                    Some(i) if forward => (i + 1) % choices.len(),
                    Some(i) => (i + choices.len() - 1) % choices.len(),
                    None => 0,
                };
                choices[next].map(Value::from)
            }
            Kind::Str { .. } if target == Target::Settings && spec.key == schema::THEME.key => {
                let current = value.as_ref().and_then(Value::as_str).unwrap_or_default();
                let choices = &self.theme_choices;
                let next = match choices.iter().position(|c| c == current) {
                    Some(i) if forward => (i + 1) % choices.len(),
                    Some(i) => (i + choices.len() - 1) % choices.len(),
                    None => 0,
                };
                Some(Value::from(choices[next].clone()))
            }
            _ => return,
        };
        self.apply(target, spec.key, next);
    }

    fn activate_option(&mut self) {
        let Some((_, spec, value)) = self.current_option() else {
            return;
        };
        if spec.is_text() {
            let buffer = value
                .or_else(|| spec.default_value())
                .map(|v| match v {
                    Value::String(s) => s,
                    other => other.to_string(),
                })
                .unwrap_or_default();
            self.mode = Mode::Input {
                cursor: buffer.chars().count(),
                buffer,
                purpose: InputPurpose::Option,
            };
        } else {
            self.cycle(true);
        }
    }

    fn reset_option(&mut self) {
        let Some((target, spec, value)) = self.current_option() else {
            return;
        };
        if !spec.can_unset() {
            self.set_status(format!("{} is required", spec.key), true);
        } else if value.is_some() && self.apply(target, spec.key, None) {
            self.set_status(format!("Reset {}", spec.key), false);
        }
    }

    // ---- keys ----

    fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if matches!(self.mode, Mode::ConfirmQuit) {
                self.exit = Some(Exit::Quit);
            } else {
                self.mode = Mode::Normal;
                self.request_quit();
            }
            return;
        }

        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.on_normal_key(key),
            Mode::Help => {}
            Mode::ConfirmQuit => match key.code {
                KeyCode::Char('y') | KeyCode::Char('s') => {
                    self.save();
                    if !self.is_dirty() {
                        self.exit = Some(Exit::Quit);
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('d') => self.exit = Some(Exit::Quit),
                _ => {}
            },
            Mode::Picker {
                mut filter,
                mut selected,
            } => {
                let matches = Self::picker_matches(&filter);
                match key.code {
                    KeyCode::Esc => return,
                    KeyCode::Enter => {
                        if let Some(spec) = matches.get(selected) {
                            self.add_widget(spec);
                        }
                        return;
                    }
                    KeyCode::Up => selected = selected.saturating_sub(1),
                    KeyCode::Down => selected = (selected + 1).min(matches.len().saturating_sub(1)),
                    KeyCode::Backspace => {
                        filter.pop();
                        selected = 0;
                    }
                    KeyCode::Char(c) => {
                        filter.push(c);
                        selected = 0;
                    }
                    _ => {}
                }
                self.mode = Mode::Picker { filter, selected };
            }
            Mode::ColorPicker { code, prefer_name } => {
                if let Some(mode) = self.on_color_picker_key(key, code, prefer_name) {
                    self.mode = mode;
                }
            }
            Mode::IconBrowser { query, selected } => {
                if let Some(mode) = self.on_icon_browser_key(key, query, selected) {
                    self.mode = mode;
                }
            }
            Mode::Input {
                mut buffer,
                mut cursor,
                purpose,
            } => {
                let byte = |buffer: &str, cursor: usize| {
                    buffer
                        .char_indices()
                        .nth(cursor)
                        .map_or(buffer.len(), |(i, _)| i)
                };
                match key.code {
                    KeyCode::Esc => return,
                    KeyCode::Enter if purpose != InputPurpose::Option => {
                        if self.submit_theme_input(purpose, &buffer) {
                            return;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some((target, spec, _)) = self.current_option() {
                            match spec.parse(&buffer) {
                                Ok(value) => {
                                    if self.apply(target, spec.key, value) {
                                        return;
                                    }
                                }
                                Err(error) => self.set_status(error, true),
                            }
                        }
                    }
                    KeyCode::Left => cursor = cursor.saturating_sub(1),
                    KeyCode::Right => cursor = (cursor + 1).min(buffer.chars().count()),
                    KeyCode::Home => cursor = 0,
                    KeyCode::End => cursor = buffer.chars().count(),
                    KeyCode::Backspace if cursor > 0 => {
                        cursor -= 1;
                        buffer.remove(byte(&buffer, cursor));
                    }
                    KeyCode::Delete if cursor < buffer.chars().count() => {
                        buffer.remove(byte(&buffer, cursor));
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        buffer.clear();
                        cursor = 0;
                    }
                    KeyCode::Char(c) => {
                        buffer.insert(byte(&buffer, cursor), c);
                        cursor += 1;
                    }
                    _ => {}
                }
                self.mode = Mode::Input {
                    buffer,
                    cursor,
                    purpose,
                };
            }
        }
    }

    fn on_normal_key(&mut self, key: KeyEvent) {
        self.status = None;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('q') => return self.request_quit(),
            KeyCode::Char('s') => return self.save(),
            KeyCode::Char('1') => return self.page = Page::Layout,
            KeyCode::Char('2') => return self.page = Page::Theme,
            KeyCode::Char('t') => {
                self.page = match self.page {
                    Page::Layout => Page::Theme,
                    Page::Theme => Page::Layout,
                };
                return;
            }
            KeyCode::Char('u') if self.page == Page::Theme => return self.theme_undo(false),
            KeyCode::Char('U') if self.page == Page::Theme => return self.theme_undo(true),
            KeyCode::Char('r') if ctrl && self.page == Page::Theme => return self.theme_undo(true),
            KeyCode::Char('u') => {
                if self.doc.undo() {
                    self.changed();
                } else {
                    self.set_status("Nothing to undo", false);
                }
                return;
            }
            KeyCode::Char('r') if ctrl => {
                if self.doc.redo() {
                    self.changed();
                } else {
                    self.set_status("Nothing to redo", false);
                }
                return;
            }
            KeyCode::Char('U') => {
                if self.doc.redo() {
                    self.changed();
                }
                return;
            }
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                return;
            }
            KeyCode::Char('e') => {
                if self.is_dirty() {
                    self.set_status("Save (s) or undo (u) before opening $EDITOR", true);
                } else {
                    self.exit = Some(Exit::ExternalEditor);
                }
                return;
            }
            _ => {}
        }

        if self.page == Page::Theme {
            return self.on_theme_key(key);
        }

        match self.focus {
            Focus::Layout => match key.code {
                KeyCode::Up | KeyCode::Char('K') if shift => self.reorder(false),
                KeyCode::Down | KeyCode::Char('J') if shift => self.reorder(true),
                KeyCode::Char('K') => self.reorder(false),
                KeyCode::Char('J') => self.reorder(true),
                KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    self.cursor = (self.cursor + 1).min(self.entries.len() - 1)
                }
                KeyCode::Home | KeyCode::Char('g') => self.cursor = 0,
                KeyCode::End | KeyCode::Char('G') => self.cursor = self.entries.len() - 1,
                KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(10),
                KeyCode::PageDown => self.cursor = (self.cursor + 10).min(self.entries.len() - 1),
                KeyCode::Char('a') | KeyCode::Char('+') | KeyCode::Insert => self.open_picker(),
                KeyCode::Char('d') | KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
                    self.delete()
                }
                KeyCode::Char('c') => {
                    if let Entry::Segment(pos) = self.entry() {
                        let pos = self.doc.duplicate(pos);
                        self.changed();
                        self.select(Entry::Segment(pos));
                    }
                }
                KeyCode::Char('n') => self.new_row(),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                    match self.current_target() {
                        Some(target) if !self.doc.options(target).is_empty() => {
                            self.focus = Focus::Options;
                            self.option_cursor = 0;
                        }
                        Some(_) => {}
                        None if matches!(key.code, KeyCode::Enter) => self.open_picker(),
                        None => {}
                    }
                }
                _ => {}
            },
            Focus::Options => {
                let count = self
                    .current_target()
                    .map_or(0, |t| self.doc.options(t).len());
                match key.code {
                    KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => self.focus = Focus::Layout,
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.option_cursor == 0 {
                            self.focus = Focus::Layout;
                        } else {
                            self.option_cursor -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.option_cursor = (self.option_cursor + 1).min(count.saturating_sub(1))
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => self.activate_option(),
                    KeyCode::Right | KeyCode::Char('l') => self.cycle(true),
                    KeyCode::Left | KeyCode::Char('h') => self.cycle(false),
                    KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
                        self.reset_option()
                    }
                    _ => {}
                }
            }
        }
    }

    // ---- drawing ----

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let preview_rows = match &self.preview_error {
            Some(error) => error
                .lines()
                .map(|line| line.width() / area.width.saturating_sub(2).max(1) as usize + 1)
                .sum::<usize>()
                .max(1),
            None => self.preview_lines.len().max(1),
        } as u16;
        let preview_height = (preview_rows + 2).min(area.height / 3).max(3);
        let [top, tabs, middle, bottom] = Layout::vertical([
            Constraint::Length(preview_height),
            Constraint::Length(1),
            Constraint::Min(6),
            Constraint::Length(1),
        ])
        .areas(area);

        self.draw_preview(frame, top);
        self.draw_tabs(frame, tabs);
        match self.page {
            Page::Layout => {
                let [left, right] =
                    Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                        .areas(middle);
                self.draw_layout(frame, left);
                self.draw_options(frame, right);
            }
            Page::Theme => self.draw_theme(frame, middle),
        }
        self.draw_footer(frame, bottom);

        match &self.mode {
            Mode::Picker { filter, selected } => draw_picker(frame, area, filter, *selected),
            Mode::ColorPicker { code, .. } => self.draw_color_picker(frame, area, *code),
            Mode::IconBrowser { query, selected } => {
                self.draw_icon_browser(frame, area, query, *selected)
            }
            Mode::ConfirmQuit => draw_confirm(frame, area),
            Mode::Help => draw_help(frame, area),
            _ => {}
        }
    }

    fn draw_tabs(&self, frame: &mut Frame, area: Rect) {
        let tab = |page: Page, key: &str, name: &str, dirty: bool| {
            let label = format!(" {key} {name}{} ", if dirty { " ●" } else { "" });
            if self.page == page {
                Span::raw(label).bold().fg(Color::White).bg(Color::Blue)
            } else {
                Span::raw(label).dark_gray()
            }
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                tab(Page::Layout, "1", "Layout", self.doc.is_dirty()),
                Span::raw(" "),
                tab(Page::Theme, "2", "Theme", self.theme.is_dirty()),
                Span::raw("   t switches").dark_gray(),
            ])),
            area,
        );
    }

    fn draw_preview(&self, frame: &mut Frame, area: Rect) {
        let suffix = if self.is_dirty() { " ● modified" } else { "" };
        let room = (area.width as usize).saturating_sub(16 + suffix.len());
        let text = format!(
            " {}{suffix} ",
            truncate_start(&display_path(&self.path), room)
        );
        let path = if self.is_dirty() {
            Span::raw(text).yellow()
        } else {
            Span::raw(text).dark_gray()
        };
        let block = Block::bordered()
            .title(Line::from(" Preview ").bold())
            .title(Line::from(path).right_aligned())
            .border_style(Style::new().dark_gray());
        let body = match &self.preview_error {
            Some(error) => Paragraph::new(
                error
                    .lines()
                    .map(|line| Line::from(line.to_string()).red())
                    .collect::<Vec<_>>(),
            )
            .wrap(Wrap { trim: false }),
            None if self.preview_lines.is_empty() => {
                Paragraph::new(Line::from("rendering…").dark_gray())
            }
            None => Paragraph::new(self.preview_lines.clone()),
        };
        frame.render_widget(body.block(block), area);
    }

    fn draw_layout(&mut self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Layout;
        let rows = self.doc.row_count();
        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|entry| match *entry {
                Entry::Settings => {
                    let theme = self
                        .doc
                        .root()
                        .get("theme")
                        .map(show_value)
                        .unwrap_or_else(|| "?".into());
                    ListItem::new(Line::from(vec![
                        Span::raw("Settings").bold(),
                        Span::raw(format!("  theme {theme}")).dark_gray(),
                    ]))
                }
                Entry::Row(row) => {
                    let mut line = vec![Span::raw(format!("Row {}", row + 1)).bold()];
                    if row + 1 == rows && rows > 0 {
                        line.push(Span::raw("  input line").dark_gray());
                    }
                    ListItem::new(Line::from(line))
                }
                Entry::Side(row, side) => {
                    let name = match side {
                        Side::Left => "left",
                        Side::Right => "right",
                    };
                    let mut line = vec![Span::raw(format!("  {name}")).cyan()];
                    if self.doc.segments(row, side).is_empty() {
                        line.push(Span::raw("  empty").dark_gray());
                    }
                    ListItem::new(Line::from(line))
                }
                Entry::Segment(pos) => {
                    let segment = self.doc.segment(pos);
                    let (name, detail) = describe(segment);
                    let name_style = match widget_spec(segment) {
                        None => Style::new().red(),
                        Some(spec) if is_layout(spec) => Style::new().magenta(),
                        Some(_) => Style::new(),
                    };
                    ListItem::new(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(name, name_style),
                        Span::raw("  "),
                        Span::raw(detail).dark_gray(),
                    ]))
                }
            })
            .collect();

        let highlight = if focused {
            Style::new().bg(Color::Blue).fg(Color::White)
        } else {
            Style::new().bg(Color::DarkGray)
        };
        let list = List::new(items)
            .block(panel(" Layout ", focused))
            .highlight_style(highlight);
        self.list.select(Some(self.cursor));
        frame.render_stateful_widget(list, area, &mut self.list);
    }

    fn draw_options(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Options;
        let (title, intro, target) = match self.entry() {
            Entry::Settings => (
                " Settings ".to_string(),
                vec![Line::from("Theme and update behaviour.").italic()],
                Some(Target::Settings),
            ),
            Entry::Row(row) => {
                let mut intro = vec![
                    Line::from(format!("Row {} of {}.", row + 1, self.doc.row_count())),
                    Line::default(),
                    Line::from("a add a widget   n new row   J/K move the row   d delete it"),
                ];
                if row + 1 == self.doc.row_count() {
                    intro.push(Line::default());
                    intro.push(
                        Line::from(
                            "The last row is where you type. Its right side is drawn by the \
                             shell's right prompt (fish, zsh and nushell only).",
                        )
                        .dark_gray(),
                    );
                }
                (format!(" Row {} ", row + 1), intro, None)
            }
            Entry::Side(_, side) => (
                match side {
                    Side::Left => " Left side ".to_string(),
                    Side::Right => " Right side ".to_string(),
                },
                vec![
                    Line::from("Widgets in this list are drawn in order."),
                    Line::default(),
                    Line::from("a add a widget at the top of the list"),
                ],
                None,
            ),
            Entry::Segment(pos) => {
                let segment = self.doc.segment(pos);
                let (name, _) = describe(segment);
                let intro = match widget_spec(segment) {
                    Some(spec) => {
                        let mut intro = vec![Line::from(spec.summary).italic()];
                        if spec.options().is_empty() {
                            intro.push(Line::default());
                            intro.push(Line::from("No options.").dark_gray());
                        }
                        intro
                    }
                    None => vec![Line::from(
                        "superline does not recognise this widget. It is kept exactly as written.",
                    )
                    .red()],
                };
                (format!(" {name} "), intro, Some(Target::Segment(pos)))
            }
        };

        let block = panel(&title, focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let options = target.map(|t| self.doc.options(t)).unwrap_or_default();
        let help_height = if options.is_empty() { 0 } else { 3 };
        let [body, help] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(help_height)]).areas(inner);

        let key_width = options.iter().map(|(o, _)| o.key.len()).max().unwrap_or(0);
        let mut lines = intro;
        if !options.is_empty() {
            lines.push(Line::default());
        }
        let first_option_line = lines.len();
        let mut cursor_position = None;
        for (i, (spec, value)) in options.iter().enumerate() {
            let selected = focused && i == self.option_cursor;
            let marker = if selected { "▸ " } else { "  " };
            let key_style = if selected {
                Style::new().bold().fg(Color::Blue)
            } else {
                Style::new()
            };
            let mut spans = vec![
                Span::raw(marker),
                Span::styled(format!("{:key_width$}  ", spec.key), key_style),
            ];
            match (&self.mode, selected) {
                (Mode::Input { buffer, cursor, .. }, true) => {
                    let before: String = buffer.chars().take(*cursor).collect();
                    let x = inner.x + (2 + key_width + 2 + before.width()) as u16;
                    cursor_position = Some((x, (first_option_line + i) as u16));
                    spans.push(Span::raw(buffer.clone()).underlined());
                }
                _ => spans.extend(option_value_spans(spec, *value, selected)),
            }
            lines.push(Line::from(spans));
        }

        // Keep the selected option on screen.
        let visible = body.height as usize;
        let wanted = first_option_line + self.option_cursor + 1;
        let scroll = if focused && wanted > visible {
            wanted - visible
        } else {
            0
        };
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((scroll as u16, 0)),
            body,
        );
        if let Some((x, row)) = cursor_position {
            let y = body.y + row.saturating_sub(scroll as u16);
            if y < body.y + body.height {
                frame.set_cursor_position((x, y));
            }
        }

        if let Some((spec, _)) = options.get(self.option_cursor).filter(|_| focused) {
            let mut text = spec.help.to_string();
            if spec.required {
                text.push_str(" Required.");
            }
            frame.render_widget(
                Paragraph::new(Line::from(text).dark_gray())
                    .wrap(Wrap { trim: true })
                    .block(
                        Block::new()
                            .borders(ratatui::widgets::Borders::TOP)
                            .dark_gray(),
                    ),
                help,
            );
        }
    }

    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let line = match &self.status {
            Some(Status { text, error: true }) => Line::from(format!(" {text}")).red(),
            Some(Status { text, error: false }) => Line::from(format!(" {text}")).green(),
            None => {
                let focus = match self.page {
                    Page::Layout => &self.focus,
                    Page::Theme => self.theme_focus(),
                };
                let hints: &[(&str, &str)] = match (&self.mode, focus) {
                    (Mode::IconBrowser { .. }, _) => &[
                        ("type", "search"),
                        ("↑↓", "choose"),
                        ("⏎", "use"),
                        ("esc", "cancel"),
                    ],
                    (Mode::ColorPicker { .. }, _) => &[
                        ("←↑↓→", "choose"),
                        ("⏎", "pick"),
                        ("i", "type"),
                        ("esc", "cancel"),
                    ],
                    (Mode::Input { .. }, _) => {
                        &[("⏎", "apply"), ("esc", "cancel"), ("^U", "clear")]
                    }
                    (Mode::Picker { .. }, _) => &[
                        ("type", "filter"),
                        ("↑↓", "choose"),
                        ("⏎", "add"),
                        ("esc", "cancel"),
                    ],
                    (_, Focus::Layout) if self.page == Page::Theme => &[
                        ("↑↓", "move"),
                        ("⏎", "edit"),
                        ("n", "fork theme"),
                        ("u", "undo"),
                        ("s", "save"),
                        ("q", "quit"),
                        ("?", "help"),
                    ],
                    (_, Focus::Options) if self.page == Page::Theme => &[
                        ("↑↓", "select"),
                        ("⏎", "change"),
                        ("←→", "step colour"),
                        ("i", "type"),
                        ("x", "reset"),
                        ("esc", "back"),
                        ("s", "save"),
                    ],
                    (_, Focus::Layout) => &[
                        ("↑↓", "move"),
                        ("⏎", "edit"),
                        ("a", "add"),
                        ("d", "delete"),
                        ("J/K", "reorder"),
                        ("c", "copy"),
                        ("n", "new row"),
                        ("u", "undo"),
                        ("s", "save"),
                        ("q", "quit"),
                        ("?", "help"),
                    ],
                    (_, Focus::Options) => &[
                        ("↑↓", "select"),
                        ("⏎", "change"),
                        ("←→", "cycle"),
                        ("x", "reset"),
                        ("esc", "back"),
                        ("s", "save"),
                        ("q", "quit"),
                    ],
                };
                Line::from(
                    hints
                        .iter()
                        .flat_map(|(key, action)| {
                            [
                                Span::raw(format!(" {key}")).bold(),
                                Span::raw(format!(" {action} ")).dark_gray(),
                            ]
                        })
                        .collect::<Vec<_>>(),
                )
            }
        };
        frame.render_widget(Paragraph::new(line), area);
    }
}

fn option_value_spans(
    spec: &OptionSpec,
    value: Option<&Value>,
    selected: bool,
) -> Vec<Span<'static>> {
    let cyclable = matches!(spec.kind, Kind::Bool { .. } | Kind::Choice { .. });
    let (text, style) = match (value, spec.default_value()) {
        (Some(value), _) => (show_value(value), Style::new().fg(Color::Yellow)),
        (None, Some(default)) => (
            format!("{} (default)", show_value(&default)),
            Style::new().dark_gray(),
        ),
        (None, None) if spec.required => ("missing".into(), Style::new().red()),
        (None, None) => ("unset".into(), Style::new().dark_gray()),
    };
    if selected && cyclable {
        vec![
            Span::raw("‹ ").dark_gray(),
            Span::styled(text, style),
            Span::raw(" ›").dark_gray(),
        ]
    } else {
        vec![Span::styled(text, style)]
    }
}

fn is_layout(spec: &WidgetSpec) -> bool {
    matches!(
        spec.name,
        "separator" | "padding" | "small_spacer" | "large_spacer"
    )
}

fn panel(title: &str, focused: bool) -> Block<'static> {
    let border = if focused {
        Style::new().fg(Color::Blue)
    } else {
        Style::new().dark_gray()
    };
    Block::bordered()
        .title(Line::from(title.to_string()).bold())
        .border_style(border)
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn draw_picker(frame: &mut Frame, area: Rect, filter: &str, selected: usize) {
    let matches = App::picker_matches(filter);
    let popup = centered(area, 72, (matches.len() as u16 + 4).max(8));
    frame.render_widget(Clear, popup);
    let block = panel(" Add widget ", true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let [input, list_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("filter: ").dark_gray(),
            Span::raw(filter.to_string()),
        ])),
        input,
    );
    frame.set_cursor_position((input.x + 8 + filter.width() as u16, input.y));

    let name_width = WIDGETS.iter().map(|w| w.name.len()).max().unwrap_or(0);
    let items: Vec<ListItem> = matches
        .iter()
        .map(|spec| {
            let name_style = if is_layout(spec) {
                Style::new().magenta()
            } else {
                Style::new().bold()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:name_width$}  ", spec.name), name_style),
                Span::raw(spec.summary).dark_gray(),
            ]))
        })
        .collect();
    let empty = items.is_empty();
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::new().bg(Color::Blue).fg(Color::White)),
        list_area,
        &mut state,
    );
    if empty {
        frame.render_widget(
            Paragraph::new(Line::from("no matching widget").dark_gray()),
            list_area,
        );
    }
}

fn draw_confirm(frame: &mut Frame, area: Rect) {
    let popup = centered(area, 50, 5);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Save your changes before quitting?"),
            Line::default(),
            Line::from(vec![
                Span::raw("y").bold(),
                Span::raw(" save   ").dark_gray(),
                Span::raw("n").bold(),
                Span::raw(" discard   ").dark_gray(),
                Span::raw("esc").bold(),
                Span::raw(" keep editing").dark_gray(),
            ]),
        ])
        .block(panel(" Unsaved changes ", true)),
        popup,
    );
}

fn draw_help(frame: &mut Frame, area: Rect) {
    const KEYS: &[(&str, &str)] = &[
        ("Layout", ""),
        ("↑ ↓  j k", "move the cursor"),
        (
            "⏎  →  tab",
            "edit the widget's options (⏎ on a row adds a widget)",
        ),
        ("a  +", "add a widget below the cursor"),
        ("d  x  del", "delete the widget or row"),
        (
            "J K  ⇧↑ ⇧↓",
            "move the widget (across sides and rows) or row",
        ),
        ("c", "duplicate the widget"),
        ("n", "add a row below"),
        ("", ""),
        ("Options", ""),
        ("↑ ↓", "select an option"),
        ("⏎  space", "toggle, cycle, or type a new value"),
        ("← →  h l", "cycle through the choices"),
        ("x  del", "reset the option to its default"),
        ("esc  tab", "back to the layout"),
        ("", ""),
        ("Theme", ""),
        (
            "⏎",
            "edit a module; ⏎ opens the colour picker or icon browser",
        ),
        ("← →", "step a colour by one code"),
        ("i", "type a colour name, a 0-255 code, or text"),
        ("x", "reset the property to its fallback"),
        ("n", "fork the theme into a new theme file"),
        ("", ""),
        ("Anywhere", ""),
        ("1  2  t", "Layout page / Theme page / switch"),
        ("u  /  U  ^R", "undo / redo"),
        ("s", "save to the config file"),
        ("e", "open the file in $EDITOR"),
        ("q", "quit"),
    ];
    let lines: Vec<Line> = KEYS
        .iter()
        .map(|(key, action)| {
            if action.is_empty() {
                Line::from(key.to_string()).bold().cyan()
            } else {
                Line::from(vec![
                    Span::raw(format!("  {key:14}")).bold(),
                    Span::raw(action.to_string()),
                ])
            }
        })
        .collect();
    let popup = centered(area, 74, lines.len() as u16 + 2);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(panel(" Keys (any key to close) ", true)),
        popup,
    );
}

fn display_path(path: &Path) -> String {
    if let Some(home) = crate::platform::home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

/// Keeps the end of `text` (the file name) when it is wider than `width`.
fn truncate_start(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut kept = String::new();
    let mut used = 1;
    for c in text.chars().rev() {
        used += c.to_string().width();
        if used > width {
            break;
        }
        kept.insert(0, c);
    }
    format!("…{kept}")
}

/// The built-in themes plus any theme files next to the config.
fn theme_choices(config_path: &Path) -> Vec<String> {
    let mut choices: Vec<String> = crate::themes::BUILTIN_THEMES
        .iter()
        .map(|(name, _)| name.to_string())
        .collect();
    let Some(dir) = config_path.parent() else {
        return choices;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return choices;
    };
    let mut themes: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter(|path| is_theme_file(path))
        .filter_map(|path| path.file_name()?.to_str().map(str::to_string))
        .collect();
    themes.sort();
    choices.extend(themes);
    choices
}

fn is_theme_file(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|value| {
            value.get("rows").is_none()
                && (value.get("defaults").is_some() || value.get("modules").is_some())
        })
}

/// Writes through a sibling temp file so a failed write never leaves a
/// truncated config behind.
fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.json".into());
    let temp = path.with_file_name(format!(".{name}.superline-tmp"));
    std::fs::write(&temp, contents)?;
    if let Ok(metadata) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&temp, metadata.permissions());
    }
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}
