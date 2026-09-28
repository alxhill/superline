//! The editor's Theme page: the theme file the config names, one module at a
//! time, with a colour picker.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

use super::model::Target;
use super::theme::TextSample;
use super::theme::{
    color_names, color_value, edit_text, parse_bool, parse_color, parse_color_list, PropKind,
    PropSpec, ThemeDoc, ThemeEntry,
};
use super::{glyphs, list_editor, picker};
use super::{json, panel, schema, write_atomic, App, Focus, InputPurpose, Mode};
use crate::themes::{bundled_theme, install_theme, theme_path};

/// Rows PageUp/PageDown move in the icon browser.
const ICON_PAGE: usize = 10;

/// A theme file the config has pointed at during this session.
pub(super) enum Slot {
    Loaded(ThemeDoc),
    Missing,
    Broken(String),
}

pub(super) struct ThemePage {
    pub(super) slots: HashMap<PathBuf, Slot>,
    cursor: usize,
    prop_cursor: usize,
    focus: Focus,
    list: ListState,
    pub(super) color_list: Option<list_editor::OpenList>,
}

impl ThemePage {
    pub(super) fn new() -> ThemePage {
        ThemePage {
            slots: HashMap::new(),
            cursor: 0,
            prop_cursor: 0,
            focus: Focus::Layout,
            list: ListState::default(),
            color_list: None,
        }
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.slots
            .values()
            .any(|slot| matches!(slot, Slot::Loaded(doc) if doc.is_dirty()))
    }

    /// Writes every changed theme, returning the files written.
    pub(super) fn save(&mut self) -> Result<Vec<PathBuf>, String> {
        let mut written = Vec::new();
        for (path, slot) in &mut self.slots {
            if let Slot::Loaded(doc) = slot {
                if doc.is_dirty() {
                    write_atomic(path, &json::to_pretty_theme(doc.root()))
                        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
                    doc.mark_saved();
                    written.push(path.clone());
                }
            }
        }
        Ok(written)
    }
}

fn load_slot(path: &Path) -> Slot {
    match std::fs::read_to_string(path) {
        Ok(text) => match ThemeDoc::load(&text) {
            Ok(doc) => Slot::Loaded(doc),
            Err(error) => Slot::Broken(error),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Slot::Missing,
        Err(error) => Slot::Broken(error.to_string()),
    }
}

impl App {
    pub(super) fn config_dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    /// The theme file the config names.
    fn theme_path(&self) -> Option<PathBuf> {
        let name = self.doc.root().get("theme")?.as_str()?;
        Some(theme_path(self.config_dir(), name))
    }

    /// Reads the named theme the first time the config points at it.
    pub(super) fn load_theme_slot(&mut self) {
        let Some(path) = self.theme_path() else {
            return;
        };
        if !self.theme.slots.contains_key(&path) {
            let slot = self.read_theme(&path);
            self.theme.slots.insert(path, slot);
        }
    }

    /// Reads a theme file, first installing a bundled theme's file that the
    /// config directory is missing.
    fn read_theme(&mut self, path: &Path) -> Slot {
        if let Some(bundled) = bundled_theme(self.config_dir(), path) {
            if let Err(error) = install_theme(path, bundled) {
                self.set_status(format!("could not write {}: {error}", path.display()), true);
            }
        }
        load_slot(path)
    }

    fn slot(&self) -> Option<&Slot> {
        self.theme.slots.get(&self.theme_path()?)
    }

    pub(super) fn theme_doc(&self) -> Option<&ThemeDoc> {
        match self.slot()? {
            Slot::Loaded(doc) => Some(doc),
            _ => None,
        }
    }

    fn theme_doc_mut(&mut self) -> Option<&mut ThemeDoc> {
        let path = self.theme_path()?;
        match self.theme.slots.get_mut(&path)? {
            Slot::Loaded(doc) => Some(doc),
            _ => None,
        }
    }

    fn theme_entry(&self) -> Option<ThemeEntry> {
        let entries = self.theme_doc()?.entries();
        entries
            .get(self.theme.cursor.min(entries.len() - 1))
            .cloned()
    }

    pub(super) fn theme_prop(&self) -> Option<(ThemeEntry, PropSpec, Option<Value>)> {
        let entry = self.theme_entry()?;
        let props = self.theme_doc()?.props(&entry);
        let (spec, value) = props.get(self.theme.prop_cursor)?.clone();
        Some((entry, spec, value))
    }

    /// The in-memory theme for the preview, with the colour under the picker
    /// applied.
    pub(super) fn preview_theme(&self) -> Option<Value> {
        if let Some(theme) = self.picker_theme() {
            return theme;
        }
        let doc = self.theme_doc()?;
        let pending = match &self.mode {
            Mode::ColorPicker { code, .. } if self.list_pending().is_some() => {
                self.list_preview(*code)
            }
            Mode::ColorPicker { code, .. } => Some(Value::from(*code)),
            Mode::IconBrowser { query, selected } => glyphs::search(query)
                .get(*selected)
                .map(|glyph| Value::from(glyph.ch.to_string())),
            _ => None,
        };
        if let (Some(value), Some((entry, spec, _))) = (pending, self.theme_prop()) {
            if let Ok(theme) = doc.with(&entry, &spec.key, Some(value)) {
                return Some(theme);
            }
        }
        Some(doc.root().clone())
    }

    pub(super) fn theme_undo(&mut self, redo: bool) {
        let done = self
            .theme_doc_mut()
            .map(|doc| if redo { doc.redo() } else { doc.undo() })
            .unwrap_or(false);
        if done {
            self.theme_changed();
        } else {
            self.set_status(
                if redo {
                    "Nothing to redo"
                } else {
                    "Nothing to undo"
                },
                false,
            );
        }
    }

    fn theme_changed(&mut self) {
        let count = self.theme_doc().map_or(1, |doc| doc.entries().len());
        self.theme.cursor = self.theme.cursor.min(count - 1);
        let props = self
            .theme_entry()
            .and_then(|entry| Some(self.theme_doc()?.props(&entry).len()))
            .unwrap_or(0);
        self.theme.prop_cursor = self.theme.prop_cursor.min(props.saturating_sub(1));
        self.preview_stale = true;
    }

    pub(super) fn set_theme_prop(&mut self, value: Option<Value>) -> bool {
        let Some((entry, spec, _)) = self.theme_prop() else {
            return false;
        };
        let result = match self.theme_doc_mut() {
            Some(doc) => doc.set(&entry, &spec.key, value),
            None => return false,
        };
        match result {
            Ok(()) => {
                self.theme_changed();
                true
            }
            Err(error) => {
                self.set_status(error, true);
                false
            }
        }
    }

    /// A copy of the theme open on the page for a new theme file, and the name
    /// of what it copies. The example theme stands in when no theme is open.
    fn fork_open_theme(&self) -> (ThemeDoc, String) {
        let name = self.doc.root().get("theme").and_then(Value::as_str);
        match (self.theme_doc(), name) {
            (Some(doc), Some(name)) => (doc.fork(), name.to_string()),
            _ => (
                ThemeDoc::new_from_starter(),
                "the example theme".to_string(),
            ),
        }
    }

    /// Points the config at a theme file, creating it as a copy of the open
    /// theme when it does not exist yet.
    fn use_new_theme(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            self.set_status("Give the theme a file name", true);
            return false;
        }
        let path = theme_path(self.config_dir(), name);
        let forking = self.theme_doc().is_some();
        let mut copied_from = None;
        let slot = match self.read_theme(&path) {
            Slot::Missing => {
                let (doc, source) = self.fork_open_theme();
                copied_from = Some(source);
                Slot::Loaded(doc)
            }
            Slot::Broken(error) => {
                self.set_status(format!("{name} exists but is not a theme: {error}"), true);
                return false;
            }
            loaded => loaded,
        };
        self.theme.slots.insert(path.clone(), slot);
        if let Err(error) =
            self.doc
                .set_option(Target::Settings, schema::THEME.key, Some(Value::from(name)))
        {
            self.set_status(error, true);
            return false;
        }
        let dir = self.config_dir();
        if !self
            .theme_choices
            .iter()
            .any(|choice| theme_path(dir, choice) == path)
        {
            self.theme_choices.push(name.to_string());
        }
        self.changed();
        // A fork has the same modules, so the cursor stays on the one being
        // edited.
        if !(forking && copied_from.is_some()) {
            self.theme.cursor = 0;
            self.theme.prop_cursor = 0;
        }
        self.set_status(
            match copied_from {
                Some(source) => format!("Created {name} from {source} (s to save)"),
                None => format!("Switched to {name}"),
            },
            false,
        );
        true
    }

    /// Applies text typed on the Theme page. Returns whether the input is done.
    pub(super) fn submit_theme_input(&mut self, purpose: InputPurpose, text: &str) -> bool {
        match purpose {
            InputPurpose::NewTheme => self.use_new_theme(text),
            InputPurpose::ThemeName => self.use_theme(text),
            InputPurpose::ListItem => self.submit_list_color(text),
            _ => {
                let Some((_, spec, _)) = self.theme_prop() else {
                    return true;
                };
                let parsed = match spec.kind {
                    PropKind::Color => parse_color(text),
                    PropKind::ColorList => parse_color_list(text),
                    PropKind::Str => Ok(Value::from(text)),
                    PropKind::Bool => parse_bool(text),
                };
                match parsed {
                    Ok(value) => self.set_theme_prop(Some(value)),
                    Err(error) => {
                        self.set_status(error, true);
                        false
                    }
                }
            }
        }
    }

    fn open_theme_input(&mut self, purpose: InputPurpose, buffer: String) {
        self.mode = Mode::Input {
            cursor: buffer.chars().count(),
            buffer,
            purpose,
        };
    }

    fn start_new_theme(&mut self) {
        let suggestion = match self.slot() {
            Some(Slot::Missing) => {
                let name = self.doc.root()["theme"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                self.use_new_theme(&name);
                return;
            }
            _ => {
                let dir = self.path.parent().unwrap_or(Path::new("."));
                (1..)
                    .map(|n| match n {
                        1 => "theme.json".to_string(),
                        n => format!("theme-{n}.json"),
                    })
                    .find(|name| !dir.join(name).exists())
                    .unwrap_or_default()
            }
        };
        self.open_theme_input(InputPurpose::NewTheme, suggestion);
    }

    pub(super) fn on_theme_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('p') && self.theme.color_list.is_none() {
            return self.open_theme_picker();
        }
        if self.theme_doc().is_none() {
            if key.code == KeyCode::Char('n') {
                self.start_new_theme();
            }
            return;
        }
        let entries = self.theme_doc().map_or(0, |doc| doc.entries().len());
        match self.theme.focus {
            Focus::Layout => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.theme.cursor = self.theme.cursor.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.theme.cursor = (self.theme.cursor + 1).min(entries - 1)
                }
                KeyCode::Home | KeyCode::Char('g') => self.theme.cursor = 0,
                KeyCode::End | KeyCode::Char('G') => self.theme.cursor = entries - 1,
                KeyCode::PageUp => self.theme.cursor = self.theme.cursor.saturating_sub(10),
                KeyCode::PageDown => self.theme.cursor = (self.theme.cursor + 10).min(entries - 1),
                KeyCode::Char('n') => self.start_new_theme(),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                    self.theme.focus = Focus::Options;
                    self.theme.prop_cursor = 0;
                }
                _ => {}
            },
            Focus::Options if self.theme.color_list.is_some() => self.on_color_list_key(key),
            Focus::Options => {
                let props = self
                    .theme_entry()
                    .and_then(|entry| Some(self.theme_doc()?.props(&entry).len()))
                    .unwrap_or(0);
                let Some((entry, spec, value)) = self.theme_prop() else {
                    self.theme.focus = Focus::Layout;
                    return;
                };
                match key.code {
                    KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => {
                        self.theme.focus = Focus::Layout
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.theme.prop_cursor == 0 {
                            self.theme.focus = Focus::Layout;
                        } else {
                            self.theme.prop_cursor -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.theme.prop_cursor = (self.theme.prop_cursor + 1).min(props - 1)
                    }
                    KeyCode::Enter | KeyCode::Char(' ') if spec.kind == PropKind::Color => {
                        let code = self
                            .theme_doc()
                            .and_then(|doc| doc.resolve(&entry, &spec, value.as_ref()))
                            .unwrap_or(0);
                        self.mode = Mode::ColorPicker {
                            code,
                            prefer_name: value.as_ref().is_some_and(Value::is_string),
                        };
                        self.preview_stale = true;
                    }
                    KeyCode::Enter | KeyCode::Char(' ') if spec.kind == PropKind::ColorList => {
                        self.open_color_list()
                    }
                    KeyCode::Enter
                    | KeyCode::Char(' ')
                    | KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Char('h')
                    | KeyCode::Char('l')
                        if spec.kind == PropKind::Bool =>
                    {
                        // Off is the default, so switching off removes the key.
                        let on = value.as_ref().and_then(Value::as_bool).unwrap_or(false);
                        self.set_theme_prop((!on).then_some(Value::Bool(true)));
                    }
                    KeyCode::Enter | KeyCode::Char(' ') if spec.kind == PropKind::Str => {
                        let current = value
                            .as_ref()
                            .map(edit_text)
                            .or_else(|| glyphs::fallback_text(&spec.fallback))
                            .and_then(|text| text.chars().next());
                        let selected = current
                            .and_then(|ch| glyphs::all().iter().position(|g| g.ch == ch))
                            .unwrap_or(0);
                        self.mode = Mode::IconBrowser {
                            query: String::new(),
                            selected,
                        };
                    }
                    KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('i') => {
                        let text = value.as_ref().map(edit_text).unwrap_or_default();
                        self.open_theme_input(InputPurpose::ThemeProperty, text);
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l')
                        if spec.kind == PropKind::Color =>
                    {
                        let code = self
                            .theme_doc()
                            .and_then(|doc| doc.resolve(&entry, &spec, value.as_ref()))
                            .unwrap_or(0);
                        let next = if matches!(key.code, KeyCode::Right | KeyCode::Char('l')) {
                            code.wrapping_add(1)
                        } else {
                            code.wrapping_sub(1)
                        };
                        self.set_theme_prop(Some(Value::from(next)));
                    }
                    KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace
                        if value.is_some() && self.set_theme_prop(None) =>
                    {
                        self.set_status(format!("Reset {}", spec.key), false);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Returns the picker's new state, or `None` once it closes.
    pub(super) fn on_color_picker_key(
        &mut self,
        key: KeyEvent,
        code: u8,
        prefer_name: bool,
    ) -> Option<Mode> {
        let moved = match key.code {
            KeyCode::Esc => {
                self.preview_stale = true;
                return None;
            }
            KeyCode::Enter | KeyCode::Char(' ') if self.list_pending().is_some() => {
                self.pick_list_color(code, prefer_name);
                return None;
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.set_theme_prop(Some(color_value(code, prefer_name)));
                return None;
            }
            KeyCode::Char('i') if self.list_pending().is_some() => {
                return Some(Mode::Input {
                    cursor: code.to_string().len(),
                    buffer: code.to_string(),
                    purpose: InputPurpose::ListItem,
                });
            }
            KeyCode::Char('i') => {
                return Some(Mode::Input {
                    cursor: code.to_string().len(),
                    buffer: code.to_string(),
                    purpose: InputPurpose::ThemeProperty,
                });
            }
            KeyCode::Left | KeyCode::Char('h') => picker::step(code, picker::Direction::Left),
            KeyCode::Right | KeyCode::Char('l') => picker::step(code, picker::Direction::Right),
            KeyCode::Up | KeyCode::Char('k') => picker::step(code, picker::Direction::Up),
            KeyCode::Down | KeyCode::Char('j') => picker::step(code, picker::Direction::Down),
            KeyCode::Home => 0,
            KeyCode::End => 255,
            _ => code,
        };
        if moved != code {
            self.preview_stale = true;
        }
        Some(Mode::ColorPicker {
            code: moved,
            prefer_name,
        })
    }

    /// Returns the browser's new state, or `None` once it closes.
    pub(super) fn on_icon_browser_key(
        &mut self,
        key: KeyEvent,
        mut query: String,
        mut selected: usize,
    ) -> Option<Mode> {
        let count = glyphs::search(&query).len();
        let last = count.saturating_sub(1);
        match key.code {
            KeyCode::Esc => {
                self.preview_stale = true;
                return None;
            }
            KeyCode::Enter => {
                if let Some(glyph) = glyphs::search(&query).get(selected) {
                    self.set_theme_prop(Some(Value::from(glyph.ch.to_string())));
                }
                return None;
            }
            KeyCode::Up => selected = selected.saturating_sub(1),
            KeyCode::Down => selected = (selected + 1).min(last),
            KeyCode::PageUp => selected = selected.saturating_sub(ICON_PAGE),
            KeyCode::PageDown => selected = (selected + ICON_PAGE).min(last),
            KeyCode::Home => selected = 0,
            KeyCode::End => selected = last,
            KeyCode::Backspace => {
                query.pop();
                selected = 0;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                query.clear();
                selected = 0;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                query.push(c);
                selected = 0;
            }
            _ => {}
        }
        self.preview_stale = true;
        Some(Mode::IconBrowser { query, selected })
    }

    // ---- drawing ----

    pub(super) fn draw_icon_browser(
        &self,
        frame: &mut Frame,
        area: Rect,
        query: &str,
        selected: usize,
    ) {
        let title = match self.theme_prop() {
            Some((entry, spec, _)) => format!(" Nerd Font icons · {}.{} ", entry.label(), spec.key),
            None => " Nerd Font icons ".to_string(),
        };
        let popup = super::centered(area, 70, area.height.saturating_sub(4).max(12));
        frame.render_widget(Clear, popup);
        let block = panel(&title, true);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let [input, list_area, hints] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        let matches = glyphs::search(query);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("search: ").dark_gray(),
                Span::raw(query.to_string()),
                Span::raw(format!("   {} icons", matches.len())).dark_gray(),
            ])),
            input,
        );
        frame.set_cursor_position((
            input.x + 8 + unicode_width::UnicodeWidthStr::width(query) as u16,
            input.y,
        ));

        // Only the rows on screen are built: the full list is ~11k glyphs.
        let height = list_area.height as usize;
        let offset = selected.saturating_sub(height.saturating_sub(1));
        let items: Vec<ListItem> = matches
            .iter()
            .skip(offset)
            .take(height)
            .map(|glyph| {
                ListItem::new(Line::from(vec![
                    Span::raw(format!(" {}  ", glyph.ch)).bold(),
                    Span::raw(format!("{:<44}", glyph.name)),
                    Span::raw(format!("U+{:04X}", glyph.ch as u32)).dark_gray(),
                ]))
            })
            .collect();
        let mut state = ListState::default().with_selected(Some(selected - offset.min(selected)));
        frame.render_stateful_widget(
            List::new(items).highlight_style(Style::new().bg(Color::Blue).fg(Color::White)),
            list_area,
            &mut state,
        );
        super::draw_scrollbar(frame, list_area, matches.len(), offset);
        if matches.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from("no matching icon").dark_gray()),
                list_area,
            );
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("type").bold(),
                Span::raw(" search names or codes  ").dark_gray(),
                Span::raw("↑↓").bold(),
                Span::raw(" choose  ").dark_gray(),
                Span::raw("⏎").bold(),
                Span::raw(" use  ").dark_gray(),
                Span::raw("esc").bold(),
                Span::raw(" cancel").dark_gray(),
            ])),
            hints,
        );
    }

    pub(super) fn theme_focus(&self) -> &Focus {
        &self.theme.focus
    }

    pub(super) fn draw_theme(&mut self, frame: &mut Frame, area: Rect) {
        let name = self
            .doc
            .root()
            .get("theme")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        let message = match self.slot() {
            None => Some(vec![Line::from("The config names no theme. p picks one.")]),
            Some(Slot::Missing) => Some(vec![
                Line::from(format!("{name} does not exist yet.")),
                Line::default(),
                Line::from(vec![
                    Span::raw("n").bold(),
                    Span::raw(format!(" create {name} from the example theme   ")),
                    Span::raw("p").bold(),
                    Span::raw(" pick another theme"),
                ]),
            ]),
            Some(Slot::Broken(error)) => Some(vec![
                Line::from(format!("{name} can't be edited here:")).red(),
                Line::from(error.clone()).red(),
                Line::default(),
                Line::from(vec![
                    Span::raw("Fix the file by hand, or "),
                    Span::raw("p").bold(),
                    Span::raw(" pick another theme."),
                ]),
            ]),
            Some(Slot::Loaded(_)) => None,
        };
        let input = match &self.mode {
            Mode::Input {
                buffer,
                cursor,
                purpose: InputPurpose::NewTheme,
            } => Some((buffer.clone(), *cursor)),
            _ => None,
        };
        if let Some(message) = message {
            let block = panel(" Theme ", true);
            let inner = block.inner(area);
            let mut lines = message;
            if let Some((buffer, cursor)) = &input {
                lines.push(Line::default());
                lines.push(Line::from(vec![
                    Span::raw("file name: ").dark_gray(),
                    Span::raw(buffer.clone()).underlined(),
                ]));
                let before: String = buffer.chars().take(*cursor).collect();
                let x =
                    inner.x + 11 + unicode_width::UnicodeWidthStr::width(before.as_str()) as u16;
                frame.set_cursor_position((x, inner.y + lines.len() as u16 - 1));
            }
            frame.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .block(block),
                area,
            );
            return;
        }

        let [left, right] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .areas(area);
        self.draw_theme_modules(frame, left, &name);
        self.draw_theme_props(frame, right);
        if let Some((buffer, cursor)) = input {
            draw_fork_prompt(frame, area, &name, &buffer, cursor);
        }
    }

    fn draw_theme_modules(&mut self, frame: &mut Frame, area: Rect, name: &str) {
        let Some(doc) = self.theme_doc() else {
            return;
        };
        let focused = self.theme.focus == Focus::Layout;
        let items: Vec<ListItem> = doc
            .entries()
            .iter()
            .map(|entry| {
                let (fg, bg) = doc.swatch(entry);
                let swatch = Style::new()
                    .fg(fg.map_or(Color::Reset, Color::Indexed))
                    .bg(bg.map_or(Color::Reset, Color::Indexed));
                let set = doc
                    .props(entry)
                    .iter()
                    .filter(|(_, value)| value.is_some())
                    .count();
                let mut spans = vec![Span::styled(format!(" {} ", entry.label()), swatch)];
                if set > 0 && *entry != ThemeEntry::Defaults {
                    spans.push(Span::raw(format!("  {set} set")).dark_gray());
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let highlight = if focused {
            Style::new().bg(Color::Blue).fg(Color::White)
        } else {
            Style::new().bg(Color::DarkGray)
        };
        let total = items.len();
        let list = List::new(items)
            .block(panel(&format!(" {name} "), focused))
            .highlight_symbol("▸")
            .highlight_style(highlight);
        self.theme.list.select(Some(self.theme.cursor));
        frame.render_stateful_widget(list, area, &mut self.theme.list);
        let rows = Block::bordered().inner(area);
        super::draw_scrollbar(frame, rows, total, self.theme.list.offset());
    }

    fn draw_theme_props(&self, frame: &mut Frame, area: Rect) {
        if self.color_list_open() {
            return self.draw_color_list(frame, area);
        }
        let (Some(doc), Some(entry)) = (self.theme_doc(), self.theme_entry()) else {
            return;
        };
        let focused = self.theme.focus == Focus::Options;
        let block = panel(&format!(" {} ", entry.label()), focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [body, help] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(inner);

        let props = doc.props(&entry);
        let key_width = props.iter().map(|(s, _)| s.key.len()).max().unwrap_or(0);
        let mut lines = Vec::new();
        let mut cursor_position = None;
        for (i, (spec, value)) in props.iter().enumerate() {
            let selected = focused && i == self.theme.prop_cursor;
            let key_style = if selected {
                Style::new().bold().fg(Color::Blue)
            } else {
                Style::new()
            };
            let mut spans = vec![
                Span::raw(if selected { "▸ " } else { "  " }),
                Span::styled(format!("{:key_width$}  ", spec.key), key_style),
            ];
            if let (
                true,
                Mode::Input {
                    buffer,
                    cursor,
                    purpose: InputPurpose::ThemeProperty,
                },
            ) = (selected, &self.mode)
            {
                let before: String = buffer.chars().take(*cursor).collect();
                let x = 2 + key_width + 2 + unicode_width::UnicodeWidthStr::width(before.as_str());
                cursor_position = Some((inner.x + x as u16, i));
                spans.push(Span::raw(buffer.clone()).underlined());
            } else {
                spans.extend(prop_value_spans(doc, &entry, spec, value.as_ref()));
                // The preview only draws the text when the prompt shows it
                // (git's unstaged count only in a repo with unstaged changes),
                // so the switch under the cursor draws a sample of it.
                if let Some(sample) = doc
                    .text_sample(&entry, &spec.key)
                    .filter(|_| selected && spec.kind == PropKind::Bool)
                {
                    spans.push(Span::raw("  "));
                    spans.push(text_sample_span(&sample));
                }
            }
            lines.push(Line::from(spans));
        }

        let visible = body.height as usize;
        let scroll = if focused {
            (self.theme.prop_cursor + 1).saturating_sub(visible)
        } else {
            0
        };
        let total = lines.len();
        frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), body);
        super::draw_scrollbar(frame, body, total, scroll);
        if let Some((x, row)) = cursor_position {
            frame.set_cursor_position((x, body.y + (row - scroll) as u16));
        }

        if let Some((spec, _)) = props.get(self.theme.prop_cursor).filter(|_| focused) {
            let mut text = spec.help.clone();
            if !spec.fallback.is_empty() {
                text.push_str(&format!(" Unset: {}.", spec.fallback));
            }
            frame.render_widget(
                Paragraph::new(Line::from(text).dark_gray())
                    .wrap(Wrap { trim: true })
                    .block(Block::new().borders(Borders::TOP).dark_gray()),
                help,
            );
        }
    }

    pub(super) fn draw_color_picker(&self, frame: &mut Frame, area: Rect, code: u8) {
        let title = match self.theme_prop() {
            Some((entry, spec, _)) => {
                format!(" {}.{}{} ", entry.label(), spec.key, self.list_pick_label())
            }
            None => " Colour ".to_string(),
        };
        // Grid, a blank line, the selection and the key hints, inside a border
        // with a line of padding on each side.
        let footer = 3;
        let spaced = area.height >= picker::height(true) + footer + 2;
        let popup = super::centered(area, picker::WIDTH + 4, picker::height(spaced) + footer + 2);
        frame.render_widget(Clear, popup);
        let block = panel(&title, true);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let inner = Rect {
            x: inner.x + 1,
            width: inner.width.saturating_sub(2),
            ..inner
        };
        let [grid, info] = Layout::vertical([
            Constraint::Length(picker::height(spaced)),
            Constraint::Length(footer),
        ])
        .areas(inner);
        picker::render(frame.buffer_mut(), grid, code);

        let names = color_names(code);
        let mut lines = vec![Line::default()];
        lines.push(Line::from(vec![
            Span::styled("      ", Style::new().bg(Color::Indexed(code))),
            Span::raw(format!(" {code}")).bold(),
            Span::raw(if names.is_empty() {
                String::new()
            } else {
                format!("  {}", names.join(", "))
            })
            .dark_gray(),
        ]));
        lines.push(Line::from(vec![
            Span::raw("←↑↓→").bold(),
            Span::raw(" move  ").dark_gray(),
            Span::raw("⏎").bold(),
            Span::raw(" pick  ").dark_gray(),
            Span::raw("i").bold(),
            Span::raw(" type  ").dark_gray(),
            Span::raw("esc").bold(),
            Span::raw(" cancel").dark_gray(),
        ]));
        frame.render_widget(Paragraph::new(lines), info);
    }
}

/// The file name prompt for forking the open theme.
fn draw_fork_prompt(frame: &mut Frame, area: Rect, source: &str, buffer: &str, cursor: usize) {
    let popup = super::centered(area, 64, 6);
    frame.render_widget(Clear, popup);
    let block = panel(" Fork theme ", true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let lines = vec![
        Line::from(format!(
            "Copy {source} to a new theme file next to the config."
        )),
        Line::default(),
        Line::from(vec![
            Span::raw("file name: ").dark_gray(),
            Span::raw(buffer.to_string()).underlined(),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    let before: String = buffer.chars().take(cursor).collect();
    let x = inner.x + 11 + unicode_width::UnicodeWidthStr::width(before.as_str()) as u16;
    frame.set_cursor_position((x, inner.y + 2));
}

/// An icon or symbol value: the text itself, then its code points and glyph
/// name, the same way for a set value and a default.
fn icon_spans(text: &str, default: bool) -> Vec<Span<'static>> {
    let shown = Span::raw(glyphs::visible(text));
    let mut detail = format!("  {}", glyphs::describe(text));
    if default {
        detail.push_str(" (default)");
    }
    vec![
        if default {
            shown.dark_gray()
        } else {
            shown.yellow()
        },
        Span::raw(detail).dark_gray(),
    ]
}

/// Columns a switch's value is padded to: `false (default)`.
const BOOL_WIDTH: usize = 15;

/// Sample text in a text colour, on its background, with its bold, italic
/// and underline switches applied.
fn text_sample_span(sample: &TextSample) -> Span<'static> {
    let mut style = Style::new();
    if let Some(fg) = sample.fg {
        style = style.fg(Color::Indexed(fg));
    }
    if let Some(bg) = sample.bg {
        style = style.bg(Color::Indexed(bg));
    }
    for (on, modifier) in [
        (sample.attrs.bold, Modifier::BOLD),
        (sample.attrs.italic, Modifier::ITALIC),
        (sample.attrs.underline, Modifier::UNDERLINED),
    ] {
        if on {
            style = style.add_modifier(modifier);
        }
    }
    Span::styled(TEXT_SAMPLE, style)
}

const TEXT_SAMPLE: &str = " Sample 123 ";

fn prop_value_spans(
    doc: &ThemeDoc,
    entry: &ThemeEntry,
    spec: &PropSpec,
    value: Option<&Value>,
) -> Vec<Span<'static>> {
    let swatch = |code: Option<u8>| match code {
        Some(code) => Span::styled("  ", Style::new().bg(Color::Indexed(code))),
        None => Span::raw("  "),
    };
    match (spec.kind, value) {
        (PropKind::ColorList, Some(Value::Array(items))) => {
            let mut spans: Vec<Span> = items
                .iter()
                .map(|item| swatch(crate::themes::color_code(item)))
                .collect();
            spans.push(Span::raw(format!(" {}", edit_text(value.unwrap()))).yellow());
            spans
        }
        (PropKind::Str, Some(value)) => icon_spans(&edit_text(value), false),
        (PropKind::Str, None) => match glyphs::fallback_text(&spec.fallback) {
            Some(text) => icon_spans(&text, true),
            None if spec.fallback.is_empty() => vec![Span::raw("unset").dark_gray()],
            None => vec![Span::raw(format!("default: {}", spec.fallback)).dark_gray()],
        },
        // Padded so the sample drawn after a switch stays put when it flips.
        (PropKind::Bool, Some(value)) => {
            vec![Span::raw(format!("{:BOOL_WIDTH$}", edit_text(value))).yellow()]
        }
        (PropKind::Bool, None) => {
            vec![Span::raw(format!("{:BOOL_WIDTH$}", "false (default)")).dark_gray()]
        }
        (_, Some(value)) => vec![
            swatch(doc.resolve(entry, spec, Some(value))),
            Span::raw(format!(" {}", edit_text(value))).yellow(),
        ],
        (_, None) => {
            let resolved = doc.resolve(entry, spec, None);
            let label = if spec.fallback.is_empty() {
                "unset".to_string()
            } else {
                spec.fallback.clone()
            };
            let mut spans = vec![swatch(resolved), Span::raw(format!(" {label}")).dark_gray()];
            if let Some(code) = resolved {
                spans.push(Span::raw(format!(" ({code})")).dark_gray());
            }
            spans
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::model::{Document, Entry};
    use crate::editor::Page;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;
    use ratatui::Terminal;
    use serde_json::json;

    fn app(label: &str, config: Value, files: &[(&str, &str)]) -> (App, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "superline-theme-page-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        let path = dir.join("config.json");
        std::fs::write(&path, config.to_string()).unwrap();
        let mut app = App::new(Document::new(config).unwrap(), path);
        app.page = Page::Theme;
        app.load_theme_slot();
        (app, dir)
    }

    fn press(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn screen(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn bundled(file: &str) -> Value {
        let (_, text) = crate::themes::BUNDLED_THEMES
            .iter()
            .find(|(bundled, _)| *bundled == file)
            .unwrap();
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn a_bundled_theme_is_installed_and_opens_as_a_normal_file() {
        let (mut app, dir) = app(
            "install",
            json!({ "theme": "rainbow", "rows": [{ "left": ["cmd"] }] }),
            &[],
        );
        assert_eq!(
            read_json(&dir.join("rainbow.json")),
            bundled("rainbow.json")
        );
        assert_eq!(
            app.theme_doc().map(ThemeDoc::root),
            Some(&bundled("rainbow.json"))
        );
        assert_eq!(app.preview_theme(), Some(bundled("rainbow.json")));
        let shown = screen(&mut app);
        assert!(shown.contains(" rainbow "), "{shown}");
        assert!(!shown.contains("built in"), "{shown}");

        // Step the first colour of the first module and save.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        while app.theme_prop().unwrap().1.kind != PropKind::Color {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Right);
        assert!(app.theme.is_dirty());
        app.save();
        let saved = read_json(&dir.join("rainbow.json"));
        assert_ne!(saved, bundled("rainbow.json"));
        assert_eq!(read_json(&dir.join("config.json"))["theme"], "rainbow");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_installed_theme_file_is_left_as_it_is() {
        let edited = r#"{ "defaults": { "fg": 1, "bg": 2 }, "modules": {} }"#;
        let (app, dir) = app(
            "existing",
            json!({ "theme": "rainbow.json", "rows": [{ "left": ["cmd"] }] }),
            &[("rainbow.json", edited)],
        );
        assert_eq!(
            app.theme_doc().map(ThemeDoc::root),
            Some(&serde_json::from_str::<Value>(edited).unwrap())
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("rainbow.json")).unwrap(),
            edited
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn settings_offer_theme_files_and_uninstalled_bundled_themes() {
        let custom = r#"{ "defaults": { "fg": 1, "bg": 2 }, "modules": {} }"#;
        let (mut app, dir) = app(
            "choices",
            json!({ "theme": "rainbow", "rows": [{ "left": ["cmd"] }] }),
            &[("ocean.json", custom)],
        );
        assert_eq!(app.theme_choices, ["rainbow", "simple", "gruvbox", "ocean"]);
        assert!(!dir.join("simple.json").exists());

        app.page = Page::Layout;
        app.select(Entry::Settings);
        app.focus = Focus::Options;
        app.option_cursor = app
            .doc
            .options(Target::Settings)
            .iter()
            .position(|(spec, _)| spec.key == schema::THEME.key)
            .unwrap();
        app.cycle(true);
        assert_eq!(app.doc.root()["theme"], "simple");
        app.load_theme_slot();
        assert_eq!(read_json(&dir.join("simple.json")), bundled("simple.json"));
        assert_eq!(
            app.theme_doc().map(ThemeDoc::root),
            Some(&bundled("simple.json"))
        );
        app.cycle(false);
        assert_eq!(app.doc.root()["theme"], "rainbow");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_theme_picker_previews_each_theme_and_installs_the_one_picked() {
        let custom = r#"{ "defaults": { "fg": 1, "bg": 2 }, "modules": {} }"#;
        let (mut app, dir) = app(
            "picker",
            json!({ "theme": "rainbow", "rows": [{ "left": ["cmd"] }] }),
            &[("ocean.json", custom)],
        );
        press(&mut app, KeyCode::Char('p'));
        let shown = screen(&mut app);
        assert!(shown.contains("Pick a theme"), "{shown}");
        assert!(shown.contains("in use"), "{shown}");
        assert!(shown.contains("built in, installs gruvbox.json"), "{shown}");
        assert_eq!(app.preview_theme(), Some(bundled("rainbow.json")));

        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.preview_theme(), Some(bundled("gruvbox.json")));
        press(&mut app, KeyCode::Down);
        assert_eq!(
            app.preview_theme(),
            Some(serde_json::from_str::<Value>(custom).unwrap())
        );
        assert!(!dir.join("gruvbox.json").exists());

        press(&mut app, KeyCode::Esc);
        assert_eq!(app.doc.root()["theme"], "rainbow");
        assert_eq!(app.preview_theme(), Some(bundled("rainbow.json")));

        press(&mut app, KeyCode::Char('p'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.doc.root()["theme"], "gruvbox");
        assert_eq!(
            read_json(&dir.join("gruvbox.json")),
            bundled("gruvbox.json")
        );
        assert_eq!(
            app.theme_doc().map(ThemeDoc::root),
            Some(&bundled("gruvbox.json"))
        );
        app.save();
        assert_eq!(read_json(&dir.join("config.json"))["theme"], "gruvbox");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn enter_on_the_theme_setting_opens_the_picker() {
        let (mut app, dir) = app(
            "picker-settings",
            json!({ "theme": "simple", "rows": [{ "left": ["cmd"] }] }),
            &[],
        );
        app.page = Page::Layout;
        app.select(Entry::Settings);
        app.focus = Focus::Options;
        app.option_cursor = app
            .doc
            .options(Target::Settings)
            .iter()
            .position(|(spec, _)| spec.key == schema::THEME.key)
            .unwrap();
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.mode, Mode::ThemePicker { selected: 1, .. }));

        // i types a file name instead, which need not exist yet.
        press(&mut app, KeyCode::Char('i'));
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        for c in "mine".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.doc.root()["theme"], "mine");
        assert!(matches!(app.mode, Mode::Normal));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn n_forks_the_open_theme_into_a_new_file() {
        let (mut app, dir) = app(
            "fork",
            json!({ "theme": "rainbow", "rows": [{ "left": ["cmd"] }] }),
            &[],
        );
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('n'));
        let shown = screen(&mut app);
        assert!(shown.contains("Fork theme"), "{shown}");
        assert!(shown.contains("file name: theme.json"), "{shown}");

        press(&mut app, KeyCode::Enter);
        assert_eq!(app.doc.root()["theme"], "theme.json");
        assert_eq!(app.theme_entry(), Some(ThemeEntry::Module("cwd".into())));
        app.save();
        assert_eq!(read_json(&dir.join("theme.json")), bundled("rainbow.json"));
        assert_eq!(read_json(&dir.join("config.json"))["theme"], "theme.json");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn forking_a_theme_file_copies_it() {
        let custom =
            r#"{ "defaults": { "fg": 1, "bg": 2 }, "modules": { "git": { "clean_bg": 3 } } }"#;
        let (mut app, dir) = app(
            "custom",
            json!({ "theme": "theme.json", "rows": [{ "left": ["cmd"] }] }),
            &[("theme.json", custom)],
        );
        press(&mut app, KeyCode::Char('n'));
        let shown = screen(&mut app);
        assert!(shown.contains("file name: theme-2.json"), "{shown}");
        press(&mut app, KeyCode::Enter);
        app.save();
        assert_eq!(
            read_json(&dir.join("theme-2.json")),
            serde_json::from_str::<Value>(custom).unwrap()
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn space_switches_a_text_attribute_on_and_off() {
        let dir = std::env::temp_dir().join(format!("superline-theme-page-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("theme.json"),
            r#"{ "defaults": { "fg": 15, "bg": 0 }, "modules": {} }"#,
        )
        .unwrap();
        let config = json!({ "theme": "theme.json", "rows": [{ "left": ["read_only"] }] });
        let mut app = App::new(Document::new(config).unwrap(), dir.join("config.json"));
        app.load_theme_slot();

        let entries = app.theme_doc().unwrap().entries();
        app.theme.cursor = entries
            .iter()
            .position(|e| e.label() == "readonly")
            .unwrap();
        app.theme.focus = Focus::Options;
        app.theme.prop_cursor = 1;
        assert_eq!(app.theme_prop().unwrap().1.key, "bold");

        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        app.on_theme_key(space);
        let modules = |app: &App| app.theme_doc().unwrap().root()["modules"].clone();
        assert_eq!(modules(&app), json!({ "readonly": { "bold": true } }));
        app.on_theme_key(space);
        assert_eq!(modules(&app), json!({}));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_switch_under_the_cursor_draws_a_sample_of_its_text() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let dir = std::env::temp_dir().join(format!(
            "superline-theme-page-sample-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("theme.json"),
            r#"{
                "defaults": { "fg": 15, "bg": 0 },
                "modules": { "git": { "notstaged_fg": 229, "notstaged_bg": 166 } }
            }"#,
        )
        .unwrap();
        let config = json!({ "theme": "theme.json", "rows": [{ "left": ["read_only"] }] });
        let mut app = App::new(Document::new(config).unwrap(), dir.join("config.json"));
        app.load_theme_slot();

        let doc = app.theme_doc().unwrap();
        let entries = doc.entries();
        let git = entries.iter().position(|e| e.label() == "git").unwrap();
        let bold = doc
            .props(&entries[git])
            .iter()
            .position(|(spec, _)| spec.key == "notstaged_bold")
            .unwrap();
        app.theme.cursor = git;
        app.theme.prop_cursor = bold;
        app.theme.focus = Focus::Options;

        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        app.on_theme_key(key(KeyCode::Char(' ')));
        app.on_theme_key(key(KeyCode::Down));
        app.on_theme_key(key(KeyCode::Char(' ')));
        assert_eq!(app.theme_prop().unwrap().1.key, "notstaged_italic");

        let mut terminal = Terminal::new(TestBackend::new(90, 60)).unwrap();
        terminal
            .draw(|frame| app.draw_theme_props(frame, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect();
        let sampled: Vec<usize> = (0..rows.len())
            .filter(|y| rows[*y].contains(TEXT_SAMPLE.trim()))
            .collect();
        assert_eq!(sampled.len(), 1, "{rows:#?}");
        let row = &rows[sampled[0]];
        assert!(row.contains("notstaged_italic"), "{row}");

        let x = row[..row.find(TEXT_SAMPLE.trim()).unwrap()].chars().count() as u16;
        let cell = &buffer[(x, sampled[0] as u16)];
        assert_eq!(cell.fg, Color::Indexed(229));
        assert_eq!(cell.bg, Color::Indexed(166));
        assert_eq!(cell.modifier, Modifier::BOLD | Modifier::ITALIC);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
